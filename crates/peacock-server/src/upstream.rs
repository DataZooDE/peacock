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
use base64::Engine as _;
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
        // Capture the genuine inbound dispatch (the real headers Triton set and
        // the args body) so the demo's inspector can show it verbatim — exactly
        // what crossed the Triton→peacock wire.
        if let Ok(mut slot) = state.upstream_capture.lock() {
            let hdr = |k: &str| {
                headers
                    .get(k)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned)
            };
            *slot = Some(json!({
                "request": "POST / HTTP/1.1",
                "headers": {
                    "X-Triton-Tool": tool,
                    "Authorization": hdr("authorization"),
                    "Content-Type": hdr("content-type")
                },
                "body": body
            }));
        }
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        // Per-request identity: when the embedding host forwards a caller
        // tenant + a bearer minted for it (loopback headers), render as THAT
        // tenant; otherwise the deployment's configured principal. This is
        // what keeps a multi-tenant embed's report renders isolated (#677) —
        // escurel is the authority and verifies the forwarded token, so this
        // is peacock's original "forward the caller's bearer per request"
        // model, restored for the embedded case.
        let principal = request_principal(&state, &headers);
        return tool_call(&state, &principal, host, tool, body).await;
    }
    if let Some(op) = headers.get("x-triton-mcp").and_then(|v| v.to_str().ok()) {
        // The proxied host flavor rides the Host header (Triton forwards it);
        // unknown flavors resolve to the stock look.
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        return match op {
            "resources/read" => match resources_read(&state, host, &body) {
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

/// The escurel principal to render THIS request as. When the caller
/// (the embedding agent) forwards a verified tenant and a bearer minted for
/// it — `X-Escurel-Tenant` + `X-Escurel-Bearer`, both set together —
/// peacock renders as that tenant, so a multi-tenant deployment's reports
/// read only the caller's own data. Absent either header, the deployment's
/// configured principal is used unchanged (single-tenant, and every path
/// that predates this). The tenant carried here is advisory (branding); the
/// bearer is the authority — escurel verifies it, and its own `tenant`
/// claim scopes the read.
fn request_principal(state: &AppState, headers: &HeaderMap) -> peacock_types::Principal {
    let hv = |k: &str| {
        headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    match (hv("x-escurel-tenant"), hv("x-escurel-bearer")) {
        (Some(tenant), Some(bearer)) => peacock_types::Principal {
            tenant: tenant.to_owned(),
            raw_token: bearer.to_owned(),
            ..state.principal.clone()
        },
        _ => state.principal.clone(),
    }
}

async fn tool_call(
    state: &AppState,
    principal: &peacock_types::Principal,
    host: &str,
    tool: &str,
    args: Value,
) -> Response {
    match tool {
        "render_report" => render_report_tool(state, principal, host, args).await,
        // The resolved deployment theme as data — peacock owns all theming;
        // chat adapters brand their card chrome from this (see `mcp::get_theme`).
        "get_theme" => Json(crate::mcp::get_theme(state, host)).into_response(),
        // A document action's event, validated against the skill page and
        // captured in escurel as the caller (see `mcp::emit_document_event`).
        "emit_document_event" => {
            match crate::mcp::emit_document_event(state, principal, &args).await {
                Ok(r) => Json(r).into_response(),
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
        // Part D (#143 D): rasterize Triton's dashboard `{title, tiles}` to a
        // PNG and return it base64-encoded — the capability Triton's chat
        // surface delegates to via TRITON_RASTERIZE_UPSTREAM.
        "render_a2ui_to_png" => render_a2ui_to_png(state, args),
        other => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("unknown tool `{other}`") })),
        )
            .into_response(),
    }
}

/// Cap parity with Triton's `MAX_RESPONSE_BYTES` — a rendered PNG over 2 MiB is
/// refused rather than shipped.
const MAX_PNG_BYTES: usize = 2 * 1024 * 1024;

fn render_a2ui_to_png(state: &AppState, args: Value) -> Response {
    let req: peacock_rasterizer::DashboardRequest = match serde_json::from_value(args) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("bad dashboard spec: {e}") })),
            )
                .into_response();
        }
    };
    match peacock_rasterizer::render_dashboard_to_png(&req, state.png_scale) {
        Ok(png) if png.len() <= MAX_PNG_BYTES => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            Json(json!({ "png_base64": b64 })).into_response()
        }
        Ok(png) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": format!("png exceeds {MAX_PNG_BYTES}-byte cap ({} bytes)", png.len()) })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": format!("rasterization failed: {e}") })),
        )
            .into_response(),
    }
}

async fn render_report_tool(
    state: &AppState,
    principal: &peacock_types::Principal,
    host: &str,
    args: Value,
) -> Response {
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
    // Apply styling on the Triton-dispatched path too: the PNG (chart or
    // instance card) carries the resolved corporate identity ⊕ host look.
    let theme = state.themes.resolve(&principal.tenant, host);
    let opts = RenderOpts {
        png_scale: Some(state.png_scale),
        theme: Some(theme.tokens),
        ..Default::default()
    };
    match render(&report_id, &report_params, principal, &state.escurel, &opts).await {
        Ok(art) => Json(tool_result(&report_id, &report_params, &art)).into_response(),
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

#[cfg(test)]
mod request_principal_tests {
    use super::*;
    use peacock_types::Principal;

    fn state_with_base() -> Arc<AppState> {
        Arc::new(AppState {
            escurel: peacock_core::EscurelData::new("http://escurel.invalid".to_string()),
            principal: Principal {
                sub: "peacock".to_string(),
                scopes: Vec::new(),
                groups: Vec::new(),
                tenant: "default".to_string(),
                raw_token: "base-token".to_string(),
                trace_id: String::new(),
            },
            png_scale: 2.0,
            demo_html: "",
            flutter_dir: None,
            flutter_app_url: None,
            themes: peacock_rasterizer::ThemeRegistry::builtin(),
            triton_url: None,
            upstream_capture: Default::default(),
        })
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        use axum::http::header::{HeaderName, HeaderValue};
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                k.parse::<HeaderName>().unwrap(),
                v.parse::<HeaderValue>().unwrap(),
            );
        }
        h
    }

    /// Both headers present ⇒ render as the forwarded tenant with the
    /// forwarded (per-tenant, caller-authorised) bearer. This is the
    /// isolation guarantee (#677): a multi-tenant embed's report reads only
    /// the caller's own data because escurel receives a token scoped to it.
    #[test]
    fn both_headers_override_tenant_and_bearer() {
        let state = state_with_base();
        let p = request_principal(
            &state,
            &headers(&[
                ("x-escurel-tenant", "acme"),
                ("x-escurel-bearer", "acme-scoped-token"),
            ]),
        );
        assert_eq!(p.tenant, "acme");
        assert_eq!(p.raw_token, "acme-scoped-token");
        // Non-identity fields inherit the deployment principal.
        assert_eq!(p.sub, "peacock");
    }

    /// No headers ⇒ the deployment principal, unchanged. Every path that
    /// predates per-tenant forwarding (single-tenant, native MCP) lands here.
    #[test]
    fn no_headers_falls_back_to_the_deployment_principal() {
        let state = state_with_base();
        let p = request_principal(&state, &headers(&[]));
        assert_eq!(p.tenant, "default");
        assert_eq!(p.raw_token, "base-token");
    }

    /// A tenant WITHOUT its authorising bearer must NOT render as that
    /// tenant — both are required together, or peacock would read a tenant's
    /// data under the deployment's standing credential. Falls back closed.
    #[test]
    fn tenant_without_bearer_does_not_switch_tenant() {
        let state = state_with_base();
        let p = request_principal(&state, &headers(&[("x-escurel-tenant", "acme")]));
        assert_eq!(p.tenant, "default");
        assert_eq!(p.raw_token, "base-token");
    }

    /// A bearer without a tenant is likewise ignored — the pair is atomic.
    #[test]
    fn bearer_without_tenant_is_ignored() {
        let state = state_with_base();
        let p = request_principal(&state, &headers(&[("x-escurel-bearer", "loose-token")]));
        assert_eq!(p.tenant, "default");
        assert_eq!(p.raw_token, "base-token");
    }

    /// Empty / whitespace header values are treated as absent (fall back),
    /// never as an empty tenant that would read the wrong data.
    #[test]
    fn blank_values_are_treated_as_absent() {
        let state = state_with_base();
        let p = request_principal(
            &state,
            &headers(&[("x-escurel-tenant", "   "), ("x-escurel-bearer", "")]),
        );
        assert_eq!(p.tenant, "default");
        assert_eq!(p.raw_token, "base-token");
    }
}
