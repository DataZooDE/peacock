//! Phase 9: peacock as a **real** internal upstream behind a **real** Triton,
//! reading from a **real** escurel (FR-M-4, FR-C-1..3, ACC-5/6). No mocks:
//! `TritonProcess` (the real triton binary) + `peacock-server` + escurel.
//!
//! Exercises Triton's MCP-Apps proxying (issue #143): `tools/call render_report`
//! is dispatched to peacock and its `_meta.ui.resourceUri` is surfaced; a
//! `resources/read ui://peacock/<report>` is proxied to peacock and returns the
//! iframe runtime.
//!
//! Requires the `triton` binary built at `../triton/target/debug/triton`
//! (the harness walks up to find it).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use peacock_core::EscurelData;
use peacock_server::{AppState, serve};
use peacock_test_support::{NW_REPORT, NorthwindEscurel};
use serde_json::{Value, json};
use triton_tests::TritonProcess;

/// Start a real escurel + a real peacock upstream; return (escurel, peacock host:port).
async fn start_peacock() -> (NorthwindEscurel, std::net::SocketAddr) {
    let nw = NorthwindEscurel::spawn().await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: None,
        themes: peacock_theme::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let st = state.clone();
    tokio::spawn(async move { serve(addr, st).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    (nw, addr)
}

async fn triton_mcp(
    triton: &TritonProcess,
    http: &reqwest::Client,
    method: &str,
    params: Value,
) -> Value {
    http.post(triton.mcp_url("/"))
        .bearer_auth("dev-token")
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .send()
        .await
        .expect("triton mcp request")
        .json()
        .await
        .expect("json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn triton_proxies_render_report_and_the_ui_resource() {
    let (nw, peacock) = start_peacock().await;

    // Register peacock under both the tool name and the ui:// authority so
    // Triton resolves tool dispatch AND resource-owner by authority.
    let env = HashMap::from([
        ("TRITON_ENV".into(), "nonprod".into()),
        (
            "TRITON_STATIC_UPSTREAMS".into(),
            format!("render_report={peacock},peacock={peacock}"),
        ),
    ]);
    let triton = TritonProcess::spawn_with_env(Duration::from_secs(10), env).await;
    let http = reqwest::Client::new();

    // 1. tools/call render_report → Triton dispatches to peacock; the result
    //    surfaces structuredContent (the rows) AND the ui:// link (#143 A).
    let call = triton_mcp(
        &triton,
        &http,
        "tools/call",
        json!({ "name": "render_report", "arguments": { "report_id": NW_REPORT } }),
    )
    .await;
    let res = &call["result"];
    let text = serde_json::to_string(res).unwrap();
    assert!(
        text.contains("1997-01-01"),
        "rows flowed through Triton: {text}"
    );
    assert_eq!(
        res["_meta"]["ui"]["resourceUri"],
        format!("ui://peacock/{NW_REPORT}"),
        "Triton surfaced peacock's ui:// link: {res}"
    );
    // J2 (#44): Triton's MCP surface rebuilds `_meta` (trace + ui only); the
    // chat inputs — the anofox PNG, the painted SVG and the DERIVED Vega-Lite
    // native-chart consumers (GE / Teams) read — ride Triton's tool-dispatch
    // result, which its chat and A2A adapters consume (REST shows it whole).
    let rest: Value = http
        .post(triton.rest_url("/v1/tools/render_report"))
        .bearer_auth("dev-token")
        .json(&json!({ "report_id": NW_REPORT }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let meta = &rest["result"]["_meta"];
    let png = meta["png_base64"].as_str().expect("png through Triton");
    assert!(png.starts_with("iVBORw0KGgo"), "a base64 PNG");
    assert!(
        meta["svgs"][0]
            .as_str()
            .unwrap()
            .contains("Revenue by category")
    );
    let vega = &meta["vega_specs"][0];
    assert_eq!(vega["mark"]["type"], "bar", "derived Vega-Lite: {vega}");
    assert_eq!(vega["encoding"]["x"]["field"], "month");
    assert_eq!(vega["encoding"]["color"]["field"], "category");
    assert_eq!(vega["data"]["values"].as_array().unwrap().len(), 16);

    // 2. resources/read ui://peacock/<report> → Triton proxies to peacock,
    //    which returns the iframe runtime HTML (#143 B).
    let read = triton_mcp(
        &triton,
        &http,
        "resources/read",
        json!({ "uri": format!("ui://peacock/{NW_REPORT}") }),
    )
    .await;
    let contents = &read["result"]["contents"][0];
    assert_eq!(contents["mimeType"], "text/html");
    assert!(
        contents["text"]
            .as_str()
            .unwrap()
            .contains("peacock report"),
        "Triton returned peacock's iframe: {read}"
    );

    nw.shutdown().await;
}

/// A document `event` action dispatched BY Triton (`X-Triton-Tool:
/// emit_document_event`): validated against the skill page, captured in
/// escurel — the whole Sources-action chain minus the browser.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn triton_dispatches_emit_document_event() {
    const ACTIONS_ACCOUNT_SKILL: &str = "---\nkind: skill\nid: account\n\
        description: A customer account.\nrequired_frontmatter: [id, name]\n\
        actions:\n\
        \x20 - name: renewal-at-risk\n\
        \x20   kind: event\n\
        \x20   label: Flag renewal at risk\n\
        \x20   event: follow_up\n\
        \x20   title: \"{frontmatter.name}\"\n\
        \x20   body: \"renewal at risk\"\n\
        ---\n# account\n";
    const ACCOUNT: &str = "---\nkind: instance\nskill: account\nid: initech-corp\n\
        name: Initech Corp\n---\n# Initech Corp\n";

    let nw = NorthwindEscurel::spawn_with(peacock_test_support::NorthwindOpts {
        extra_skills: vec![("account".to_owned(), ACTIONS_ACCOUNT_SKILL.to_owned())],
        extra_instances: vec![(
            "account".to_owned(),
            "initech-corp".to_owned(),
            ACCOUNT.to_owned(),
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
        themes: peacock_theme::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peacock = listener.local_addr().unwrap();
    drop(listener);
    let st = state.clone();
    tokio::spawn(async move { serve(peacock, st).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{peacock}/healthz"))
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let env = HashMap::from([
        ("TRITON_ENV".into(), "nonprod".into()),
        (
            "TRITON_STATIC_UPSTREAMS".into(),
            format!("render_report={peacock},emit_document_event={peacock},peacock={peacock}"),
        ),
    ]);
    let triton = TritonProcess::spawn_with_env(Duration::from_secs(10), env).await;
    let http = reqwest::Client::new();

    let call = triton_mcp(
        &triton,
        &http,
        "tools/call",
        json!({ "name": "emit_document_event", "arguments": {
            "skill": "account", "id": "initech-corp", "action": "renewal-at-risk",
        }}),
    )
    .await;
    let text = serde_json::to_string(&call).unwrap();
    assert!(
        text.contains("\"ok\":true") && text.contains("event_id"),
        "the event action round-tripped through Triton: {text}"
    );

    // The event is really in escurel, templates substituted server-side.
    let client = nw.sales_client().await;
    let inbox = client
        .list_inbox(escurel_client::ListInboxRequest {
            limit: 50,
            ..Default::default()
        })
        .await
        .expect("list inbox");
    assert!(
        inbox
            .events
            .iter()
            .any(|e| e.source == "peacock" && e.title == "Initech Corp"),
        "captured: {:?}",
        inbox.events
    );

    nw.shutdown().await;
}

/// `get_theme` dispatched BY Triton (`X-Triton-Tool: get_theme`): the chat
/// adapter's brand source — peacock owns all theming; triton only consumes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn triton_dispatches_get_theme() {
    let nw = NorthwindEscurel::spawn().await;
    let mut themes = peacock_theme::ThemeRegistry::builtin();
    themes.register_brand(
        "acme",
        ":root { --pk-name: \"DataZoo\"; --pk-brand: #1a73e8; --pk-logo-style: banner; }",
    );
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: None,
        themes,
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peacock = listener.local_addr().unwrap();
    drop(listener);
    let st = state.clone();
    tokio::spawn(async move { serve(peacock, st).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{peacock}/healthz"))
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let env = HashMap::from([
        ("TRITON_ENV".into(), "nonprod".into()),
        (
            "TRITON_STATIC_UPSTREAMS".into(),
            format!("get_theme={peacock},peacock={peacock}"),
        ),
    ]);
    let triton = TritonProcess::spawn_with_env(Duration::from_secs(10), env).await;
    let http = reqwest::Client::new();

    let call = triton_mcp(&triton, &http, "tools/call", json!({ "name": "get_theme" })).await;
    let text = serde_json::to_string(&call).unwrap();
    assert!(
        text.contains("DataZoo") && text.contains("#1a73e8") && text.contains("banner"),
        "the resolved theme round-tripped through Triton: {text}"
    );

    nw.shutdown().await;
}

// ── J2b (#44): a Teams user gets a NATIVE chart from an anofox report ───────

const TEAMS_AUDIENCE: &str = "triton-msteams-test-appid";

fn teams_claims(fake: &triton_tests::chat_courier_fixture::FakeBotFramework) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    json!({
        "iss": "https://api.botframework.com",
        "aud": TEAMS_AUDIENCE,
        "exp": now + 600,
        "iat": now - 5,
        "serviceurl": fake.service_url(),
    })
}

/// Alice types into Teams; her agent answers with the Northwind report.
/// Triton dispatches `render_report` to the REAL peacock (real escurel, real
/// Parquet), maps the derived Vega-Lite to a native `Chart.*` element, and
/// serves peacock's anofox PNG as the chart's fallback image. Microsoft's
/// side is triton-tests' `FakeBotFramework` (a real HTTP server for the
/// third-party Bot Connector); the agent is a `FakeAgent` — neither is the
/// boundary under test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn teams_message_gets_native_chart_from_anofox_report() {
    use triton_tests::chat_courier_fixture::FakeBotFramework;
    use triton_tests::upstream_fixture::FakeAgent;

    let (nw, peacock) = start_peacock().await;
    let fake = FakeBotFramework::start().await;
    let assistant = FakeAgent::start_returning(json!({
        "surface": { "components": [
            { "kind": "text", "value": "Here is revenue by category for 1997." },
            { "kind": "report", "report_id": NW_REPORT, "args": { "params": {} } }
        ] }
    }))
    .await;
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../triton/crates/triton-tests/fixtures/manifest-msteams-report-test.yaml");
    let env = HashMap::from([
        ("TRITON_ENV".to_string(), "local".to_string()),
        (
            "TRITON_MANIFEST_PATH".to_string(),
            manifest.display().to_string(),
        ),
        ("TRITON_MSTEAMS_OPENID_URL".to_string(), fake.openid_url()),
        ("TRITON_MSTEAMS_TOKEN_URL".to_string(), fake.token_url()),
        (
            "TRITON_MSTEAMS_EXTRA_SERVICE_URL_HOSTS".to_string(),
            "127.0.0.1".to_string(),
        ),
        (
            "TRITON_STATIC_UPSTREAMS".to_string(),
            format!(
                "assistant={},render_report={peacock}",
                assistant.host_port()
            ),
        ),
        (
            "TRITON_MSTEAMS_PUBLIC_BASE".to_string(),
            "https://teams.example".to_string(),
        ),
    ]);
    let triton = TritonProcess::spawn_with_env(Duration::from_secs(5), env).await;
    let webhook = triton.chat_webhook_addr.expect("chat webhook listener");

    let resp = reqwest::Client::new()
        .post(format!("http://{webhook}/msteams/webhook"))
        .header(
            "Authorization",
            format!("Bearer {}", fake.sign_jwt(teams_claims(&fake))),
        )
        .json(&json!({
            "type": "message", "id": "msg-1",
            "serviceUrl": "https://placeholder.example/",
            "channelId": "msteams",
            "from": { "id": "29:1abc", "name": "Alice" },
            "conversation": { "id": "a:conv-1", "conversationType": "personal" },
            "recipient": { "id": "28:bot-1", "name": "MyBot" },
            "text": "how did our categories do in 1997?",
            "textFormat": "plain"
        }))
        .send()
        .await
        .expect("POST inbound activity");
    assert!(resp.status().is_success(), "{}", resp.status());

    // The reply Teams receives: an Adaptive Card with a NATIVE grouped bar
    // chart (one series per category) built from peacock's derived spec.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let card = loop {
        if let Some(a) = fake
            .captured()
            .into_iter()
            .find(|a| a.body["attachments"][0]["content"]["type"] == "AdaptiveCard")
        {
            break a.body["attachments"][0]["content"].clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no card reached Teams; captured: {:?}",
            fake.captured().iter().map(|a| &a.body).collect::<Vec<_>>()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let body = card["body"].as_array().expect("card body");
    let chart = body
        .iter()
        .find(|e| e["type"].as_str().is_some_and(|t| t.starts_with("Chart.")))
        .unwrap_or_else(|| panic!("expected a native Chart.* element; got: {card}"));
    assert_eq!(chart["type"], "Chart.VerticalBar.Grouped", "{chart}");
    assert_eq!(chart["title"], "Revenue by category");
    let legends: Vec<&str> = chart["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["legend"].as_str().unwrap())
        .collect();
    for c in [
        "Beverages",
        "Condiments",
        "Dairy Products",
        "Produce",
        "Seafood",
    ] {
        assert!(legends.contains(&c), "series {c} in {legends:?}");
    }

    // Hosts without chart support show the fallback image: the signed img
    // route, which renders peacock's anofox PNG on fetch.
    let url = chart["fallback"]["url"]
        .as_str()
        .expect("fallback image url");
    let token = url.rsplit('/').next().unwrap();
    let img = reqwest::Client::new()
        .get(format!("http://{webhook}/msteams/img/{token}"))
        .send()
        .await
        .expect("GET img");
    assert!(img.status().is_success(), "{}", img.status());
    let bytes = img.bytes().await.unwrap();
    assert_eq!(&bytes[..4], b"\x89PNG");
    assert!(bytes.len() > 5_000, "a real chart, not a placeholder");

    nw.shutdown().await;
}
