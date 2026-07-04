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

/// The self-contained single-file iframe runtime — the `ui://peacock/<report>`
/// resource when peacock is reached only via Triton (the host cannot fetch
/// peacock's multi-file Flutter bundle). It renders the report inline from
/// `callServerTool` + the PNG, needing nothing external.
const IFRAME_HTML: &str = include_str!("../assets/iframe.html");

/// The Flutter runtime shim — used as the `ui://` resource when a host-reachable
/// `flutter_app_url` is configured (it nests peacock's hosted `/app/` Flutter
/// bundle and relays the MCP-Apps postMessage channel). See
/// `doc/flutter-iframe-runtime-proposal.md`.
const FLUTTER_SHIM_HTML: &str = include_str!("../assets/flutter-shim.html");

/// JSON-RPC entrypoint for `POST /mcp`.
pub async fn handle(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<Value>,
) -> impl IntoResponse {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    // The host flavor rides the Host header (as on the HTTP demo path); the
    // brand defaults to the deployment principal's tenant. Unknown names
    // resolve to peacock's stock look — theming never fails a request.
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let result = match method {
        "initialize" => Ok(initialize()),
        "tools/list" => Ok(tools_list()),
        "tools/call" => tools_call(&state, host, &params).await,
        "resources/read" => resources_read(&state, host, &params),
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
    }, {
        "name": "emit_document_event",
        "description": "Execute an `event` action a document's escurel SKILL \
                        page declares (`actions:` frontmatter): the event is \
                        validated against the skill page server-side and \
                        captured in escurel as the caller.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "skill": { "type": "string" },
                "id": { "type": "string" },
                "action": { "type": "string" }
            },
            "required": ["skill", "id", "action"]
        }
    }] })
}

/// `tools/call render_report` → the artifact's structuredContent + the linked
/// UI resource (`_meta.ui.resourceUri`, FR-M-2). A drill is just this call
/// with new absolute params (FR-M-3).
async fn tools_call(state: &AppState, host: &str, params: &Value) -> Result<Value, Error> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if name == "emit_document_event" {
        return emit_document_event(state, &args).await;
    }
    if name != "render_report" {
        return Err(Error::validation(format!("unknown tool `{name}`")));
    }
    let report_id = args
        .get("report_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::validation("arguments.report_id is required"))?;
    let report_params = args.get("params").cloned().unwrap_or(json!({}));

    // Apply styling on THIS endpoint too (not just the HTTP demo path): the
    // rasterized chart / instance card carries the resolved corporate
    // identity ⊕ host look. Unknown names resolve to the stock palette.
    let theme = state.themes.resolve(&state.principal.tenant, host);
    let opts = RenderOpts {
        png_scale: Some(state.png_scale),
        theme: Some(theme.tokens),
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

    Ok(tool_result(report_id, &report_params, &art))
}

/// `tools/call emit_document_event` → validate the named action against the
/// document's SKILL page and capture its event in escurel as the caller
/// (peacock's only write path; the forwarded bearer keeps escurel's ACL in
/// charge). The core owns the whole validation chain.
pub(crate) async fn emit_document_event(state: &AppState, args: &Value) -> Result<Value, Error> {
    let field = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::validation(format!("arguments.{k} is required")))
    };
    let (skill, id, action) = (field("skill")?, field("id")?, field("action")?);
    let event_id = peacock_core::emit_document_event(
        skill,
        id,
        action,
        &state.principal,
        &state.escurel,
        None,
    )
    .await?;
    Ok(json!({ "ok": true, "event_id": event_id }))
}

/// Build the MCP `tools/call` result: `structuredContent` + a text summary +
/// `_meta.ui.resourceUri` so the host renders the iframe (FR-M-2). The
/// CALLER's params ride the resource URI — a param-REQUIRED report (a
/// customer record, a briefing) is unrenderable from a bare URI, so the
/// served runtime seeds its first render from them.
pub fn tool_result(report_id: &str, caller_params: &Value, art: &Artifact) -> Value {
    let png_b64 = art.png.as_ref().map(|p| {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(p)
    });
    json!({
        "content": [{ "type": "text", "text": summary(report_id, art) }],
        "structuredContent": art.structured_content,
        "isError": false,
        "_meta": {
            "ui": { "resourceUri": resource_uri(report_id, caller_params) },
            "png_base64": png_b64
        }
    })
}

/// `ui://peacock/<id>[?k=v&…]` — scalar caller params urlencoded into the
/// resource URI. A caller that sent none keeps the bare URI (byte-compat
/// with every pre-existing consumer).
fn resource_uri(report_id: &str, params: &Value) -> String {
    let base = format!("ui://{UI_AUTHORITY}/{report_id}");
    let Some(obj) = params.as_object().filter(|o| !o.is_empty()) else {
        return base;
    };
    let query: Vec<String> = obj
        .iter()
        .filter_map(|(k, v)| {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => return None, // nested values don't ride a URI
            };
            Some(format!("{}={}", urlencode(k), urlencode(&s)))
        })
        .collect();
    if query.is_empty() {
        base
    } else {
        format!("{base}?{}", query.join("&"))
    }
}

/// Minimal percent-encoding (RFC 3986 unreserved kept verbatim).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Parse a resource URI's query back into the initial-params object,
/// coercing scalars (the inverse of [`resource_uri`], lossy by design —
/// only what a URI can carry).
fn query_params(query: &str) -> Value {
    let mut obj = serde_json::Map::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let (k, v) = (urldecode(k), urldecode(v));
        let value = if let Ok(n) = v.parse::<i64>() {
            json!(n)
        } else if let Ok(f) = v.parse::<f64>() {
            json!(f)
        } else if v == "true" || v == "false" {
            json!(v == "true")
        } else {
            Value::String(v)
        };
        obj.insert(k, value);
    }
    Value::Object(obj)
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(
                &format!("{}{}", bytes[i + 1] as char, bytes[i + 2] as char),
                16,
            )
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
///
/// When `flutter_app_url` is set (a host-reachable absolute base for peacock's
/// hosted Flutter `/app/`), the resource is the **Flutter shim** that nests it.
/// Otherwise — the default, and the only correct choice when peacock is reached
/// only via Triton's proxy — it is the **self-contained** `iframe.html` (the
/// host can't fetch the multi-file Flutter bundle through `resources/read`).
pub(crate) fn resources_read(state: &AppState, host: &str, params: &Value) -> Result<Value, Error> {
    let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
    let rest = uri
        .strip_prefix(&format!("ui://{UI_AUTHORITY}/"))
        .ok_or_else(|| Error::validation(format!("not a peacock ui:// resource: {uri}")))?;
    // `<report_id>[?initial params]` — the caller's params minted into the
    // URI by [`tool_result`] seed the runtime's FIRST render (a
    // param-required report is unrenderable from a bare URI).
    let (report_id, query) = rest.split_once('?').unwrap_or((rest, ""));
    // Report ids are slugs; anything else never reaches the template splice.
    if !peacock_core::is_slug(report_id) {
        return Err(Error::validation(format!("not a report id: `{report_id}`")));
    }
    let initial = query_params(query);
    // Serialized as a JSON literal into a <script type="application/json">
    // island; `<` escaped so hostile param VALUES can never close the tag.
    let initial_json = serde_json::to_string(&initial)
        .unwrap_or_else(|_| "{}".into())
        .replace('<', "\\u003c");

    // The resolved theme (brand = the deployment principal's tenant ⊕ the
    // Host flavor) styles the served runtime — the same registry the HTTP
    // demo path and the chart rasterizer use.
    let theme = state.themes.resolve(&state.principal.tenant, host);

    let html = match state.flutter_app_url.as_deref() {
        Some(base) => FLUTTER_SHIM_HTML
            .replace("__PEACOCK_APP_BASE__", base)
            .replace("__REPORT_ID__", report_id),
        None => IFRAME_HTML
            .replace("__REPORT_ID__", report_id)
            .replace("__INITIAL_PARAMS__", &initial_json)
            .replace("__THEME_CSS__", &theme.css),
    };
    Ok(json!({
        "contents": [{ "uri": uri, "mimeType": "text/html", "text": html }]
    }))
}
