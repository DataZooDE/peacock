//! Phase 8: peacock's own MCP surface (FR-M-1..3, ACC-5/12) against a **real**
//! escurel — `tools/call render_report` returns structuredContent + the linked
//! `ui://` resource; `resources/read` returns the iframe; a drill is a fresh
//! render with absolute params.

use std::sync::Arc;

use peacock_core::EscurelData;
use peacock_server::{AppState, serve};
use peacock_test_support::{NW_REPORT, NorthwindEscurel};
use serde_json::{Value, json};

async fn start() -> (NorthwindEscurel, String) {
    let nw = NorthwindEscurel::spawn().await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: None,
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (nw, format!("http://{addr}"))
}

async fn rpc(base: &str, method: &str, params: Value) -> Value {
    reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn initialize_and_tools_list() {
    let (nw, base) = start().await;
    let init = rpc(&base, "initialize", json!({})).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "peacock");
    assert!(init["result"]["capabilities"].get("resources").is_some());

    let tools = rpc(&base, "tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"render_report"));
    nw.shutdown().await;
}

#[tokio::test]
async fn tools_call_links_structured_content_and_ui_resource() {
    // FR-M-2: the tool result carries structuredContent AND the ui:// link.
    let (nw, base) = start().await;
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report", "arguments": { "report_id": NW_REPORT } }),
    )
    .await;
    let res = &r["result"];
    assert_eq!(res["isError"], false);
    assert_eq!(
        res["structuredContent"]["rows"].as_array().unwrap().len(),
        16
    );
    assert_eq!(
        res["_meta"]["ui"]["resourceUri"],
        format!("ui://peacock/{NW_REPORT}")
    );
    nw.shutdown().await;
}

#[tokio::test]
async fn resources_read_serves_the_iframe_runtime() {
    // FR-M-1: resources/read of the ui:// URI returns the iframe HTML, with the
    // report id injected.
    let (nw, base) = start().await;
    let r = rpc(
        &base,
        "resources/read",
        json!({ "uri": format!("ui://peacock/{NW_REPORT}") }),
    )
    .await;
    let c = &r["result"]["contents"][0];
    assert_eq!(c["mimeType"], "text/html");
    let html = c["text"].as_str().unwrap();
    assert!(html.contains("peacock report"));
    assert!(
        html.contains(NW_REPORT),
        "report id injected into the iframe"
    );
    assert!(
        html.contains("updateModelContext"),
        "iframe pushes view state"
    );
    // No flutter_app_url configured → the self-contained iframe, not the shim.
    assert!(
        !html.contains("peacock-app"),
        "without flutter_app_url, the ui:// resource is the self-contained iframe"
    );
    nw.shutdown().await;
}

#[tokio::test]
async fn resources_read_serves_the_flutter_shim_when_app_url_is_set() {
    // With a host-reachable flutter_app_url, the ui:// resource is the Flutter
    // shim that nests the hosted /app/ bundle (the direct-reachable deployment).
    let nw = NorthwindEscurel::spawn().await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: Some("http://peacock.example/app/".into()),
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let r = rpc(
        &format!("http://{addr}"),
        "resources/read",
        json!({ "uri": format!("ui://peacock/{NW_REPORT}") }),
    )
    .await;
    let html = r["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(
        html.contains("peacock-app"),
        "the shim nests the Flutter app"
    );
    assert!(
        html.contains("http://peacock.example/app/"),
        "shim points at the configured app base"
    );
    assert!(html.contains(NW_REPORT));
    nw.shutdown().await;
}

#[tokio::test]
async fn in_iframe_drill_is_a_fresh_render() {
    // FR-M-3 / FR-X-2: a callServerTool drill = tools/call with absolute params.
    let (nw, base) = start().await;
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report",
                "arguments": { "report_id": NW_REPORT, "params": { "category": "Beverages" } } }),
    )
    .await;
    let sc = &r["result"]["structuredContent"];
    assert_eq!(sc["current_params"]["category"], "Beverages");
    // Every returned row is Beverages (the drilled view).
    assert!(
        sc["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["category"] == "Beverages")
    );
    nw.shutdown().await;
}

// ── instance reports on the MCP surface ──

const ACCOUNT_SKILL: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n---\n# account\n";

/// The body carries hostile markup — the artifact transports it RAW (typed
/// contract); only the RENDERER escapes. The iframe test below pins the
/// escape path.
const HOSTILE_ACCOUNT: &str = "---\ntype: instance\nskill: account\nid: beverages-gmbh\n\
    name: \"Beverages <script>alert(1)</script>\"\n---\n# Beverages GmbH\n\n\
    Note: <script>alert(1)</script> rides verbatim.\n";

const CUSTOMER_REPORT: &str = "---\ntype: skill\nid: customer-report\nrender: a2ui\n\
    description: One customer account as a card.\n\
    params:\n  account: { type: string }\n\
    instances:\n  acct: \"[[account::{account}]]\"\n\
    views:\n\
      - { kind: frontmatter, instance: acct, keys: [name], label: Account }\n\
      - { kind: markdown, instance: acct }\n\
      - { kind: timeline, instance: acct, limit: 5 }\n\
    ---\n";

async fn start_with_customer_report() -> (NorthwindEscurel, String) {
    let nw = NorthwindEscurel::spawn_with(peacock_test_support::NorthwindOpts {
        extra_skills: vec![
            ("account".to_owned(), ACCOUNT_SKILL.to_owned()),
            ("customer-report".to_owned(), CUSTOMER_REPORT.to_owned()),
        ],
        extra_instances: vec![(
            "account".to_owned(),
            "beverages-gmbh".to_owned(),
            HOSTILE_ACCOUNT.to_owned(),
        )],
        ..Default::default()
    })
    .await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: None,
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (nw, format!("http://{addr}"))
}

#[tokio::test]
async fn tools_call_instance_report_carries_the_typed_contract() {
    let (nw, base) = start_with_customer_report().await;
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report", "arguments": {
            "report_id": "customer-report",
            "params": { "account": "beverages-gmbh" },
        }}),
    )
    .await;
    let res = &r["result"];
    assert_eq!(res["isError"], false, "{r}");
    let inst = &res["structuredContent"]["instances"]["acct"];
    assert_eq!(inst["id"], "beverages-gmbh");
    assert_eq!(inst["facts"][0]["key"], "name");
    // RAW in the typed contract — the consumer (iframe/chat mapper) encodes.
    assert!(
        inst["markdown"].as_str().unwrap().contains("<script>"),
        "the artifact transports the body verbatim: {inst}"
    );
    assert!(inst["events"].as_array().is_some(), "timeline contract");
    assert_eq!(
        res["_meta"]["ui"]["resourceUri"],
        "ui://peacock/customer-report"
    );
    // The chat surface: a chartless instance report still carries a PNG —
    // the themed INSTANCE CARD (protocol adaptation on this endpoint too).
    assert!(
        res["_meta"]["png_base64"]
            .as_str()
            .is_some_and(|b| !b.is_empty()),
        "instance-card png on tools/call: {}",
        res["_meta"]
    );
    nw.shutdown().await;
}

#[tokio::test]
async fn resources_read_is_themed_and_escapes_before_formatting() {
    // The served runtime carries the RESOLVED theme css (brand ⊕ host — the
    // "apply styling" contract on the MCP endpoint) and its renderer only
    // ever writes escaped content (escape-then-format; the browser-level
    // gold check drives the semantics tree per CLAUDE.md).
    let (nw, base) = start_with_customer_report().await;
    let r = rpc(
        &base,
        "resources/read",
        json!({ "uri": "ui://peacock/customer-report" }),
    )
    .await;
    let html = r["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(
        !html.contains("__THEME_CSS__"),
        "the theme placeholder is substituted"
    );
    assert!(
        html.contains("peacock theme — host flavor"),
        "the resolved theme css is embedded: {}",
        &html[..600]
    );
    // The escape path: dynamic content goes through esc() before innerHTML,
    // and the markdown renderer formats the ESCAPED line.
    assert!(html.contains("const esc ="));
    assert!(html.contains("esc(raw.trim())"), "escape-then-format");
    nw.shutdown().await;
}
