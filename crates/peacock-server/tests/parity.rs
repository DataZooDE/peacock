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

/// ACC-1 extended to the ggplot backend (issue #8): a STATISTICAL report
/// (top-level `geom`) yields the same parsed-identical structuredContent and
/// A2UI across all four surfaces — the stat spec (rows + schema inline) is
/// composed once, backend-independently, and every surface funnels through
/// that one core. Feature-gated: the base tree doesn't build the plotters
/// graph, and the spec composes either way.
#[cfg(feature = "ggplot")]
#[tokio::test]
async fn stat_report_produces_identical_content_across_surfaces() {
    use peacock_test_support::{NW_REPORT_DISTRIBUTION, NorthwindOpts, skill_report_distribution};

    let nw = peacock_test_support::NorthwindEscurel::spawn_with(NorthwindOpts {
        extra_skills: vec![(
            NW_REPORT_DISTRIBUTION.to_owned(),
            skill_report_distribution(),
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
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let params = json!({});

    // 1. Embedded library face (in-process render core).
    let embedded = render(
        NW_REPORT_DISTRIBUTION,
        &params,
        &nw.sales_principal(),
        &EscurelData::new(nw.endpoint()),
        &RenderOpts::default(),
    )
    .await
    .unwrap();
    let embedded_sc = serde_json::to_value(&embedded.structured_content).unwrap();
    assert_eq!(embedded.stat_specs.len(), 1, "the report is statistical");

    // 2. Structured HTTP surface — with the PNG, so the ggplot backend rides
    //    the existing artifact path unchanged.
    let http_body: Value = http
        .post(format!("{base}/v1/render_report"))
        .json(&json!({ "report_id": NW_REPORT_DISTRIBUTION, "params": params, "png": true }))
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
                                   "arguments": { "report_id": NW_REPORT_DISTRIBUTION,
                                                  "params": params } } }))
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
        .json(&json!({ "report_id": NW_REPORT_DISTRIBUTION, "params": params }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

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

    // A2UI is identical on the surfaces that carry it — including the `stat`
    // component with the inline rows + schema.
    let embedded_a2ui = serde_json::to_value(&embedded.a2ui).unwrap();
    assert_eq!(embedded_a2ui, http_body["a2ui"], "a2ui embedded vs http");
    let stat_component = embedded_a2ui["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "stat")
        .expect("the a2ui carries the stat component");
    assert!(stat_component["spec"]["data"]["schema"].is_array());

    // The ggplot PNG rides the existing artifact path (`png_base64` on the
    // structured surface, `_meta.png_base64` on MCP) — same as a Vega chart.
    assert!(http_body["png_base64"].as_str().unwrap().len() > 1000);
    assert!(mcp["result"]["_meta"]["png_base64"].as_str().unwrap().len() > 1000);

    nw.shutdown().await;
}
