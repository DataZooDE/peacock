//! FR-O-4 / #46: one structured audit line per render on stdout, from the
//! **real** `peacock` binary against a **real** escurel. No mocks.
//!
//! Every render surface (peacock's own `/mcp` `tools/call`, the HTTP
//! `/v1/render_report`, the Triton upstream `POST /`) writes exactly one
//! `kind: "audit"` line per render, failed renders included. The line names
//! the outcome (report, surface, result, duration, rows, the truncation when
//! a view was capped) and NEVER carries rows, cell values, param values or
//! the bearer.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use peacock_test_support::{
    NW_REPORT, NW_REPORT_OVERSIZED, NorthwindEscurel, NorthwindOpts, PeacockProcess,
    oversized_fixtures,
};
use serde_json::{Value, json};

/// The closed audit schema: a line carrying any other key fails the test, so
/// a field added later is a deliberate, reviewed schema change.
const AUDIT_KEYS: &[&str] = &[
    "kind",
    "event",
    "when",
    "surface",
    "host",
    "tenant",
    "who",
    "trace_id",
    "report_id",
    "param_names",
    "result",
    "duration_ms",
    "rows",
    "truncated",
];

fn env_for(nw: &NorthwindEscurel) -> HashMap<String, String> {
    let p = nw.sales_principal();
    HashMap::from([
        ("PEACOCK_ESCUREL_URL".into(), nw.endpoint().to_string()),
        ("PEACOCK_ESCUREL_TOKEN".into(), p.raw_token.clone()),
        ("PEACOCK_TENANT".into(), p.tenant.clone()),
        ("PEACOCK_SUB".into(), p.sub.clone()),
    ])
}

async fn mcp_call(http: &reqwest::Client, base: &str, args: Value) -> Value {
    http.post(format!("{base}/mcp"))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": { "name": "render_report", "arguments": args } }))
        .send()
        .await
        .expect("mcp tools/call")
        .json()
        .await
        .expect("json-rpc reply")
}

#[tokio::test]
async fn every_render_writes_exactly_one_audit_line() {
    let (extra_skills, extra_instances) = oversized_fixtures();
    let nw = NorthwindEscurel::spawn_with(NorthwindOpts {
        extra_skills,
        extra_instances,
        ..Default::default()
    })
    .await;
    let token = nw.sales_principal().raw_token.clone();
    let peacock = PeacockProcess::spawn(env_for(&nw)).await;
    let base = peacock.base_url();
    let http = reqwest::Client::new();

    // No render yet, no audit line (boot writes only `log` lines).
    assert!(
        peacock.audit_lines().is_empty(),
        "{:?}",
        peacock.stdout_lines()
    );

    // 1. MCP tools/call, a view over escurel's row cap.
    let r = mcp_call(&http, &base, json!({ "report_id": NW_REPORT_OVERSIZED })).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    let first_row = r["result"]["structuredContent"]["rows"][0].clone();
    assert!(first_row.is_object(), "{r}");

    // 2. HTTP /v1/render_report with a param.
    let resp = http
        .post(format!("{base}/v1/render_report"))
        .json(&json!({ "report_id": NW_REPORT, "params": { "category": "Seafood" } }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());

    // 3. The Triton upstream entry point (`POST /` + `X-Triton-Tool`).
    let resp = http
        .post(format!("{base}/"))
        .header("X-Triton-Tool", "render_report")
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "report_id": NW_REPORT, "params": { "from": "1997-03-01" } }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());

    // 4. A failed render: the report does not exist.
    let r = mcp_call(&http, &base, json!({ "report_id": "no-such-report" })).await;
    assert!(r.get("error").is_some(), "{r}");

    // 5. A render refused before it starts (bad look arg) still audits.
    let r = mcp_call(
        &http,
        &base,
        json!({ "report_id": NW_REPORT, "variant": "sepia" }),
    )
    .await;
    assert_eq!(r["error"]["code"], -32602, "{r}");

    let lines = peacock
        .wait_for_audit_lines(5, Duration::from_secs(5))
        .await;
    // Give a stray duplicate a moment to show up before counting.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let lines = if lines.len() == 5 {
        peacock.audit_lines()
    } else {
        lines
    };
    assert_eq!(
        lines.len(),
        5,
        "exactly one audit line per render: {lines:#?}"
    );

    // The schema is closed and every line is a render event.
    for l in &lines {
        let keys: BTreeSet<&str> = l.as_object().unwrap().keys().map(String::as_str).collect();
        let extra: Vec<&&str> = keys.iter().filter(|k| !AUDIT_KEYS.contains(k)).collect();
        assert!(extra.is_empty(), "unexpected audit keys {extra:?} in {l}");
        assert_eq!(l["event"], "render", "{l}");
        assert_eq!(l["tenant"], "acme", "{l}");
        assert!(l["duration_ms"].is_u64(), "{l}");
        assert!(l["when"].as_str().is_some_and(|w| w.ends_with('Z')), "{l}");
    }

    // 1: the oversized MCP render records the cut.
    let l = &lines[0];
    assert_eq!(l["surface"], "mcp", "{l}");
    assert_eq!(l["report_id"], NW_REPORT_OVERSIZED);
    assert_eq!(l["result"], "ok");
    assert_eq!(l["rows"], 10_000);
    assert_eq!(
        l["truncated"],
        json!({ "rows_shown": 10_000, "limit": 10_000, "view": "lines" })
    );

    // 2: HTTP — the param's NAME, never its value.
    let l = &lines[1];
    assert_eq!(l["surface"], "http", "{l}");
    assert_eq!(l["report_id"], NW_REPORT);
    assert_eq!(l["result"], "ok");
    assert_eq!(l["param_names"], json!(["category"]));
    assert!(l.get("truncated").is_none(), "nothing was capped: {l}");

    // 3: the Triton upstream surface.
    let l = &lines[2];
    assert_eq!(l["surface"], "triton", "{l}");
    assert_eq!(l["result"], "ok");
    assert_eq!(l["param_names"], json!(["from"]));
    assert!(l["rows"].as_u64().is_some_and(|n| n > 0), "{l}");

    // 4 + 5: failures carry the stable error tag.
    let l = &lines[3];
    assert_eq!(l["surface"], "mcp", "{l}");
    assert_eq!(l["report_id"], "no-such-report");
    assert!(
        l["result"]
            .as_str()
            .is_some_and(|r| r.starts_with("error:")),
        "{l}"
    );
    assert!(l.get("rows").is_none(), "{l}");
    let l = &lines[4];
    assert_eq!(l["result"], "error:validation", "{l}");
    assert_eq!(l["report_id"], NW_REPORT);

    // Never rows, cell values, param values or the bearer.
    let all = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!all.contains(&token), "the bearer leaked into the audit");
    for needle in ["Seafood", "1997-03-01", "Beverages", "sepia"] {
        assert!(!all.contains(needle), "`{needle}` leaked: {all}");
    }
    for (_, v) in first_row.as_object().unwrap() {
        if let Some(s) = v.as_str() {
            assert!(!all.contains(s), "cell value `{s}` leaked: {all}");
        }
    }

    let status = peacock.terminate();
    assert!(status.success(), "{status:?}");
    nw.shutdown().await;
}
