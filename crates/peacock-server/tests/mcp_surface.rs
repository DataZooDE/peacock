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
