//! J1 (issue #44): an analyst in an MCP host asks for "Northwind revenue by
//! category 1997". The host calls peacock's real `/mcp` `render_report`
//! against a real escurel over the real Northwind Parquet. They get the
//! anofox chart (painted SVG + PNG), a KPI that matches the fixture, and —
//! in a dark client, in a German locale — a readable, localised chart. A
//! drill is a fresh render with absolute params. No mocks.

use std::sync::Arc;

use base64::Engine;
use peacock_core::EscurelData;
use peacock_server::{AppState, serve};
use peacock_test_support::{NW_REPORT, NorthwindEscurel};
use serde_json::{Value, json};

async fn start() -> (NorthwindEscurel, String) {
    let nw = NorthwindEscurel::spawn().await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 1.0,
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
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (nw, format!("http://{addr}"))
}

async fn render_report(base: &str, args: Value) -> Value {
    let r: Value = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": { "name": "render_report", "arguments": args } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(r.get("error").is_none(), "render_report failed: {r}");
    r["result"].clone()
}

fn png_of(res: &Value) -> Vec<u8> {
    let b64 = res["_meta"]["png_base64"]
        .as_str()
        .expect("a PNG for the chat card");
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap()
}

#[tokio::test]
async fn mcp_host_renders_northwind_with_anofox() {
    let (nw, base) = start().await;

    // The host discovers the knobs a dark / localised client needs.
    let tools: Value = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let schema = &tools["result"]["tools"][0]["inputSchema"]["properties"];
    assert_eq!(schema["variant"]["enum"], json!(["light", "dark"]));
    assert_eq!(schema["locale"]["type"], "string");

    // "Northwind revenue by category, 1997."
    let res = render_report(&base, json!({ "report_id": NW_REPORT })).await;
    assert_eq!(res["isError"], false);
    let sc = &res["structuredContent"];
    let rows = sc["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    let total: f64 = rows.iter().map(|r| r["revenue"].as_f64().unwrap()).sum();
    assert_eq!(
        total.round(),
        2986.0,
        "the KPI the analyst sees matches the fixture"
    );

    // The chart itself: anofox SVG for the iframe, a PNG for the card, and the
    // derived Vega-Lite for native-chart hosts.
    let svg = res["_meta"]["svgs"][0]
        .as_str()
        .expect("the painted chart SVG");
    for label in ["Revenue by category", "Beverages", "Seafood"] {
        assert!(svg.contains(label), "the chart shows {label}");
    }
    let light_png = png_of(&res);
    assert_eq!(&light_png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(res["_meta"]["vega_specs"][0]["mark"]["type"], "bar");

    // A dark client in Germany.
    let dark = render_report(
        &base,
        json!({ "report_id": NW_REPORT, "variant": "dark", "locale": "de-DE" }),
    )
    .await;
    assert_ne!(
        png_of(&dark),
        light_png,
        "the dark variant is a different picture"
    );
    let dark_svg = dark["_meta"]["svgs"][0].as_str().unwrap();
    assert_ne!(dark_svg, svg);

    // Same request ⇒ byte-identical picture (ACC-8 / ADR-P7).
    let again = render_report(&base, json!({ "report_id": NW_REPORT })).await;
    assert_eq!(again["_meta"]["svgs"], res["_meta"]["svgs"]);
    assert_eq!(png_of(&again), light_png);

    // Drill: Beverages only — a fresh render from absolute params; one series
    // left, so the report's `spec_single` line chart is used.
    let drilled = render_report(
        &base,
        json!({ "report_id": NW_REPORT, "params": {
            "from": "1997-01-01", "to": "1997-12-31", "category": "Beverages" } }),
    )
    .await;
    assert_eq!(
        drilled["structuredContent"]["current_params"]["category"],
        "Beverages"
    );
    assert_eq!(drilled["_meta"]["vega_specs"][0]["mark"]["type"], "line");
    let dsvg = drilled["_meta"]["svgs"][0].as_str().unwrap();
    assert!(dsvg.contains("Beverages") && !dsvg.contains("Seafood"));

    // An unknown variant is a validation error, not a silent light render.
    let bad: Value = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": { "name": "render_report",
                                   "arguments": { "report_id": NW_REPORT, "variant": "sepia" } } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(bad.get("error").is_some(), "{bad}");

    nw.shutdown().await;
}

/// Phase 5: the `ui://peacock` runtime shows the INTERACTIVE anofox chart —
/// the painted SVG inline (hover tooltip, click-to-drill on a mark), the PNG
/// only as a fallback — and peacock serves the same page standalone at
/// `GET /ui/<report>`, so a browser (or `scripts/verify-ui.sh`) can open it.
#[tokio::test]
async fn the_ui_runtime_inlines_the_interactive_chart() {
    let (nw, base) = start().await;

    let read: Value = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/read",
                       "params": { "uri": format!("ui://peacock/{NW_REPORT}") } }),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let html = read["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    // It reads the painted SVGs from the tool result and wires the marks.
    for hook in ["_meta.svgs", "gg-mark", "data-series", "id=\"tip\""] {
        assert!(html.contains(hook), "runtime wires `{hook}`");
    }
    // The PNG stays a fallback, not the primary picture.
    assert!(html.contains("png_base64"));

    // The same page, standalone.
    let page = reqwest::get(format!("{base}/ui/{NW_REPORT}"))
        .await
        .unwrap();
    assert!(page.status().is_success());
    assert!(
        page.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html"),
        "an HTML page"
    );
    assert_eq!(
        page.text().await.unwrap(),
        html,
        "the same runtime as the ui:// resource"
    );

    // An unknown / unsafe id is not served.
    let bad = reqwest::get(format!("{base}/ui/..%2Fetc")).await.unwrap();
    assert!(bad.status().is_client_error());

    nw.shutdown().await;
}
