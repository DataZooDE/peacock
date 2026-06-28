//! Phase 11: single-render-path parity (ACC-1, FR-R-1/2). The same `(report,
//! params, principal)` yields parsed-identical structuredContent across the
//! **embedded**, **structured HTTP**, **MCP**, and **Triton-upstream** surfaces
//! — and identical A2UI where the surface carries it. Surfaces are thin shells
//! over one core (HLD §5); this test is the divergence regression guard.

use std::sync::Arc;

use peacock_core::{EscurelData, RenderOpts, render};
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
async fn all_surfaces_produce_identical_structured_content() {
    let (nw, base) = start().await;
    let http = reqwest::Client::new();
    let params = json!({ "category": "Beverages" });

    // 1. Embedded library face (in-process render core).
    let embedded = render(
        NW_REPORT,
        &params,
        &nw.sales_principal(),
        &EscurelData::new(nw.endpoint()),
        &RenderOpts::default(),
    )
    .await
    .unwrap();
    let embedded_sc = serde_json::to_value(&embedded.structured_content).unwrap();

    // 2. Structured HTTP surface.
    let http_body: Value = http
        .post(format!("{base}/v1/render_report"))
        .json(&json!({ "report_id": NW_REPORT, "params": params }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // 3. MCP surface.
    let mcp: Value = http
        .post(format!("{base}/mcp"))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": { "name": "render_report",
                                   "arguments": { "report_id": NW_REPORT, "params": params } } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // 4. Triton-upstream contract (the POST / shape Triton dispatches).
    let upstream: Value = http
        .post(format!("{base}/"))
        .header("X-Triton-Tool", "render_report")
        .json(&json!({ "report_id": NW_REPORT, "params": params }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // structuredContent is parsed-identical across all four surfaces.
    assert_eq!(
        embedded_sc, http_body["structuredContent"],
        "embedded vs http"
    );
    assert_eq!(
        embedded_sc, mcp["result"]["structuredContent"],
        "embedded vs mcp"
    );
    assert_eq!(
        embedded_sc, upstream["structuredContent"],
        "embedded vs upstream"
    );

    // A2UI is identical on the surfaces that carry it (embedded, http).
    let embedded_a2ui = serde_json::to_value(&embedded.a2ui).unwrap();
    assert_eq!(embedded_a2ui, http_body["a2ui"], "a2ui embedded vs http");

    nw.shutdown().await;
}
