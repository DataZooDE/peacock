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
