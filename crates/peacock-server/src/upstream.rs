//! The Triton-upstream contract (FR-M-4, FR-C-1..3): peacock as an internal
//! upstream behind Triton's ingress. Triton dispatches over `POST /` with a
//! header selecting the operation:
//!
//! - `X-Triton-Tool: render_report` + body = tool args → the tool result
//!   (structuredContent + `_meta.ui.resourceUri`), which Triton surfaces to
//!   the MCP host and projects to chat (issue #143 A).
//! - `X-Triton-MCP: resources/read` + body `{ uri }` → the `ui://` resource
//!   contents (Triton proxies this for the host, #143 B).
//! - `X-Triton-MCP: updateModelContext` + body `{ uri, record }` → ack
//!   (Triton relays the compact view-state record, #143 C).
//!
//! Identity is the Triton-minted bearer; peacock forwards its configured
//! escurel principal (the deployment's escurel binding). The same render core
//! serves every surface (FR-R-1).

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use peacock_core::{RenderOpts, render};
use serde_json::{Value, json};

use crate::AppState;
use crate::mcp::{resources_read, tool_result};

/// `POST /` — the header-routed Triton upstream entrypoint.
pub async fn handle(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Some(tool) = headers.get("x-triton-tool").and_then(|v| v.to_str().ok()) {
        return tool_call(&state, tool, body).await;
    }
    if let Some(op) = headers.get("x-triton-mcp").and_then(|v| v.to_str().ok()) {
        return match op {
            "resources/read" => match resources_read(&body) {
                Ok(r) => Json(r).into_response(),
                Err(e) => {
                    (StatusCode::NOT_FOUND, Json(json!({ "error": e.kind() }))).into_response()
                }
            },
            // The record rides through untouched; the host owns the channel.
            "updateModelContext" => Json(json!({ "ok": true })).into_response(),
            other => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("unknown X-Triton-MCP op `{other}`") })),
            )
                .into_response(),
        };
    }
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "missing X-Triton-Tool or X-Triton-MCP header" })),
    )
        .into_response()
}

async fn tool_call(state: &AppState, tool: &str, args: Value) -> Response {
    if tool != "render_report" {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("unknown tool `{tool}`") })),
        )
            .into_response();
    }
    let report_id = match args.get("report_id").and_then(Value::as_str) {
        Some(r) => r.to_owned(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "report_id is required" })),
            )
                .into_response();
        }
    };
    let report_params = args.get("params").cloned().unwrap_or(json!({}));
    let opts = RenderOpts {
        png_scale: Some(state.png_scale),
        ..Default::default()
    };
    match render(
        &report_id,
        &report_params,
        &state.principal,
        &state.escurel,
        &opts,
    )
    .await
    {
        Ok(art) => Json(tool_result(&report_id, &art)).into_response(),
        Err(e) => {
            let status = match e {
                peacock_types::Error::Auth(_) => StatusCode::UNAUTHORIZED,
                peacock_types::Error::Validation(_) => StatusCode::BAD_REQUEST,
                _ => StatusCode::BAD_GATEWAY,
            };
            (
                status,
                Json(json!({ "error": e.kind(), "message": e.to_string() })),
            )
                .into_response()
        }
    }
}
