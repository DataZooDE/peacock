//! peacock's MCP surface (FR-M-1..4): a JSON-RPC `/mcp` endpoint that returns
//! a `render_report` tool result carrying `structuredContent` **and** the
//! MCP-Apps `ui://` resource link, serves that `ui://` resource (the iframe
//! runtime), and accepts `updateModelContext` records. Reached directly (for
//! tests / direct hosts) or behind Triton's MCP-Apps proxy (issue #143).
//!
//! Drills arrive as a fresh `tools/call render_report` with the **absolute**
//! params (FR-M-3, FR-X-2) — peacock holds no server-side UI state.

use std::sync::Arc;

use axum::{Json, extract::State, response::IntoResponse};
use peacock_core::{RenderOpts, render};
use peacock_types::{Artifact, Error};
use serde_json::{Value, json};

use crate::AppState;

/// The `ui://` authority peacock owns (matches its Triton upstream name).
pub const UI_AUTHORITY: &str = "peacock";

/// The iframe runtime served as the `ui://peacock/<report>` resource.
const IFRAME_HTML: &str = include_str!("../assets/iframe.html");

/// JSON-RPC entrypoint for `POST /mcp`.
pub async fn handle(
    State(state): State<Arc<AppState>>,
    Json(req): Json<Value>,
) -> impl IntoResponse {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => Ok(initialize()),
        "tools/list" => Ok(tools_list()),
        "tools/call" => tools_call(&state, &params).await,
        "resources/read" => resources_read(&params),
        "updateModelContext" => Ok(json!({ "ok": true })),
        other => Err(Error::validation(format!("unknown method `{other}`"))),
    };

    match result {
        Ok(r) => Json(json!({ "jsonrpc": "2.0", "id": id, "result": r })),
        Err(e) => Json(json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": e.jsonrpc_code(), "message": e.to_string() }
        })),
    }
}

fn initialize() -> Value {
    json!({
        "protocolVersion": "2025-06-18",
        "capabilities": { "tools": {}, "resources": {} },
        "serverInfo": { "name": "peacock", "version": peacock_types::VERSION }
    })
}

fn tools_list() -> Value {
    json!({ "tools": [{
        "name": "render_report",
        "description": "Render an escurel report skill to an A2UI v0.9 artifact \
                        (structuredContent + a linked ui:// MCP-App).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "report_id": { "type": "string" },
                "params": { "type": "object" }
            },
            "required": ["report_id"]
        }
    }] })
}

/// `tools/call render_report` → the artifact's structuredContent + the linked
/// UI resource (`_meta.ui.resourceUri`, FR-M-2). A drill is just this call
/// with new absolute params (FR-M-3).
async fn tools_call(state: &AppState, params: &Value) -> Result<Value, Error> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    if name != "render_report" {
        return Err(Error::validation(format!("unknown tool `{name}`")));
    }
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let report_id = args
        .get("report_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::validation("arguments.report_id is required"))?;
    let report_params = args.get("params").cloned().unwrap_or(json!({}));

    let opts = RenderOpts {
        png_scale: Some(state.png_scale),
        ..Default::default()
    };
    let art = render(
        report_id,
        &report_params,
        &state.principal,
        &state.escurel,
        &opts,
    )
    .await?;

    Ok(tool_result(report_id, &art))
}

/// Build the MCP `tools/call` result: `structuredContent` + a text summary +
/// `_meta.ui.resourceUri` so the host renders the iframe (FR-M-2).
pub fn tool_result(report_id: &str, art: &Artifact) -> Value {
    let png_b64 = art.png.as_ref().map(|p| {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(p)
    });
    json!({
        "content": [{ "type": "text", "text": summary(report_id, art) }],
        "structuredContent": art.structured_content,
        "isError": false,
        "_meta": {
            "ui": { "resourceUri": format!("ui://{UI_AUTHORITY}/{report_id}") },
            "png_base64": png_b64
        }
    })
}

fn summary(report_id: &str, art: &Artifact) -> String {
    let rows = art
        .structured_content
        .rows
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    format!("Rendered `{report_id}` — {rows} rows. View state in structuredContent.current_params.")
}

/// `resources/read ui://peacock/<report>` → the iframe runtime HTML (FR-M-1).
pub(crate) fn resources_read(params: &Value) -> Result<Value, Error> {
    let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
    let report_id = uri
        .strip_prefix(&format!("ui://{UI_AUTHORITY}/"))
        .ok_or_else(|| Error::validation(format!("not a peacock ui:// resource: {uri}")))?;

    // Inject the report id the iframe should render.
    let html = IFRAME_HTML.replace("__REPORT_ID__", report_id);
    Ok(json!({
        "contents": [{ "uri": uri, "mimeType": "text/html", "text": html }]
    }))
}
