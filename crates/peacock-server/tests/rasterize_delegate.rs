//! Part D (#143 D): peacock is the `render_a2ui_to_png` rasterizer Triton's
//! chat surface delegates dashboard rendering to. Tested both directly against
//! peacock's upstream `POST /` contract and **through a real Triton binary**
//! dispatching the tool to peacock.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use peacock_core::EscurelData;
use peacock_server::{AppState, serve};
use peacock_test_support::NorthwindEscurel;
use serde_json::{Value, json};
use triton_tests::TritonProcess;

async fn start_peacock() -> (NorthwindEscurel, std::net::SocketAddr) {
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

fn dashboard() -> Value {
    json!({
        "title": "Northwind revenue",
        "tiles": [
            { "label": "Total revenue", "value": "$2,986", "trend": "+12%" },
            { "label": "Categories", "value": "5" }
        ]
    })
}

#[tokio::test]
async fn peacock_renders_render_a2ui_to_png_directly() {
    let (nw, addr) = start_peacock().await;
    let body: Value = reqwest::Client::new()
        .post(format!("http://{addr}/"))
        .header("X-Triton-Tool", "render_a2ui_to_png")
        .json(&dashboard())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let b64 = body["png_base64"].as_str().expect("png_base64 present");
    let png = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    nw.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn triton_delegates_rasterization_to_peacock() {
    // Register peacock as the `render_a2ui_to_png` upstream and dispatch the
    // tool through a real Triton: Triton → peacock → PNG.
    let (nw, peacock) = start_peacock().await;
    let env = HashMap::from([
        ("TRITON_ENV".into(), "nonprod".into()),
        (
            "TRITON_STATIC_UPSTREAMS".into(),
            format!("render_a2ui_to_png={peacock}"),
        ),
    ]);
    let triton = TritonProcess::spawn_with_env(Duration::from_secs(10), env).await;

    let resp: Value = reqwest::Client::new()
        .post(triton.rest_url("/v1/tools/render_a2ui_to_png"))
        .bearer_auth("dev-token")
        .json(&dashboard())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Triton wrapped peacock's result; the base64 PNG rode through.
    let text = serde_json::to_string(&resp).unwrap();
    let b64 = resp["result"]["png_base64"]
        .as_str()
        .or_else(|| resp["png_base64"].as_str())
        .unwrap_or_else(|| panic!("no png_base64 in Triton response: {text}"));
    let png = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    nw.shutdown().await;
}
