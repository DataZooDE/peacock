//! Phase 7: the peacock HTTP surface against a **real** escurel — `/healthz`,
//! `/version`, and `render_report` returning the Northwind artifact + PNG.

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
        demo_html: "<!doctype html><title>peacock demo</title>",
        flutter_dir: None,
        flutter_app_url: None,
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener); // free the port for serve() to rebind
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    // Give the server a moment to bind.
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (nw, format!("http://{addr}"))
}

#[tokio::test]
async fn healthz_and_version() {
    let (nw, base) = start().await;
    let h = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert!(h.status().is_success());
    assert_eq!(h.text().await.unwrap(), "ok");

    let v: Value = reqwest::get(format!("{base}/version"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["name"], "peacock");
    nw.shutdown().await;
}

#[tokio::test]
async fn render_report_returns_artifact_and_png() {
    let (nw, base) = start().await;
    let client = reqwest::Client::new();
    let body: Value = client
        .post(format!("{base}/v1/render_report"))
        .json(&json!({ "report_id": NW_REPORT, "png": true }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(body["a2ui"]["version"], "0.9");
    assert_eq!(
        body["structuredContent"]["rows"].as_array().unwrap().len(),
        16
    );
    // The chart PNG rides along base64-encoded for the chat/demo surface.
    assert!(body["png_base64"].as_str().unwrap().len() > 1000);
    nw.shutdown().await;
}

#[tokio::test]
async fn render_report_applies_theme_by_host_and_brand() {
    // The same report, two corporate identities → matching theme CSS for the web
    // surfaces and a visibly different chart PNG (corporate identity ⊕ host look).
    let (nw, base) = start().await;
    let client = reqwest::Client::new();

    let fetch = |brand: &'static str, host: &'static str| {
        let client = client.clone();
        let base = base.clone();
        async move {
            client
                .post(format!("{base}/v1/render_report"))
                .json(&json!({ "report_id": NW_REPORT, "png": true, "host": host, "brand": brand }))
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };

    let a = fetch("company-a", "copilot").await;
    let b = fetch("company-b", "gemini").await;

    // The composed CSS (one source of truth for the web chrome) carries the
    // brand colour and the host look.
    assert!(
        a["theme_css"].as_str().unwrap().contains("#6b3fa0"),
        "Acme A purple in CSS"
    );
    assert!(
        b["theme_css"].as_str().unwrap().contains("#e8590c"),
        "Beta B orange in CSS"
    );
    assert_eq!(a["theme"]["brand"], "company-a");
    assert_eq!(b["theme"]["host"], "gemini");

    // The chart PNGs differ between brands (re-palette + background).
    assert_ne!(
        a["png_base64"].as_str().unwrap(),
        b["png_base64"].as_str().unwrap(),
        "different corporate identities render different charts"
    );
    nw.shutdown().await;
}

#[tokio::test]
async fn drill_param_re_renders() {
    let (nw, base) = start().await;
    let client = reqwest::Client::new();
    let body: Value = client
        .post(format!("{base}/v1/render_report"))
        .json(&json!({ "report_id": NW_REPORT, "params": { "category": "Beverages" } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let kpi = body["a2ui"]["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "kpi")
        .unwrap();
    assert_eq!(kpi["value"].as_f64().unwrap(), 801.0);
    assert_eq!(
        body["structuredContent"]["current_params"]["category"],
        "Beverages"
    );
    nw.shutdown().await;
}

/// Start a peacock with a (fake) Flutter bundle directory so `/app` and the
/// MCP-Apps shim route are mounted.
async fn start_with_flutter() -> (NorthwindEscurel, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><title>peacock_web</title>",
    )
    .unwrap();
    let nw = NorthwindEscurel::spawn().await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html><title>peacock demo</title>",
        flutter_dir: Some(dir.path().to_path_buf()),
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
    (nw, format!("http://{addr}"), dir)
}

#[tokio::test]
async fn flutter_bundle_and_mcp_shim_are_served() {
    // The Flutter client mounts at `/app`, and the MCP-Apps runtime shim that
    // nests it mounts at `/app-shim` — the bridge a host's `ui://` iframe loads.
    let (nw, base, _dir) = start_with_flutter().await;
    let client = reqwest::Client::new();

    // /app serves the bundle's index.html.
    let app = client.get(format!("{base}/app/")).send().await.unwrap();
    assert!(app.status().is_success());
    assert!(app.text().await.unwrap().contains("peacock_web"));

    // /app-shim nests the Flutter app and carries the report id on the hash, and
    // bridges the MCP-Apps postMessage verbs.
    let shim = client
        .get(format!("{base}/app-shim?report=northwind-monthly-revenue"))
        .send()
        .await
        .unwrap();
    assert!(shim.status().is_success());
    let html = shim.text().await.unwrap();
    assert!(
        html.contains("/app/"),
        "shim nests the Flutter app at /app/"
    );
    assert!(
        html.contains("northwind-monthly-revenue"),
        "shim carries the report id"
    );
    assert!(
        html.contains("mcp:callServerTool") && html.contains("mcp:updateModelContext"),
        "shim relays the MCP-Apps verbs"
    );
    nw.shutdown().await;
}
