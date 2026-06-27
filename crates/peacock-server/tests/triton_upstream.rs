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
