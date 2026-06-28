//! The peacock HTTP app: `render_report`, observability, and the demo SPA.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine;
use peacock_core::{EscurelData, RenderOpts, render};
use peacock_types::{Error, Principal};
use serde::Deserialize;
use serde_json::{Value, json};

/// Shared service state. The `principal` is the dev/demo identity peacock
/// forwards to escurel; in production it is rebuilt per request from the
/// Triton-forwarded token (Phase 9).
pub struct AppState {
    pub escurel: EscurelData,
    pub principal: Principal,
    /// PNG scale for chart rasterization on the chat/demo path.
    pub png_scale: f32,
    /// The embedded demo SPA (served at `/`).
    pub demo_html: &'static str,
    /// Optional built Flutter-web bundle directory, served at `/app` (the
    /// richer client surface, FR-M-1). `None` skips it.
    pub flutter_dir: Option<std::path::PathBuf>,
    /// Optional **host-reachable** absolute base URL for peacock's hosted
    /// Flutter `/app/` (e.g. `http://peacock.tailnet:8080/app/`). When set, the
    /// MCP-Apps `ui://` resource is the Flutter shim that nests it; when `None`
    /// (the default, and required behind Triton's proxy where the host cannot
    /// reach the bundle) the `ui://` resource is the self-contained iframe.
    pub flutter_app_url: Option<String>,
    /// Theme registry: resolves `(brand, host)` to a corporate-identity ⊕
    /// host-look theme that styles both the chart PNG and the web surfaces.
    pub themes: peacock_rasterizer::ThemeRegistry,
}

/// Build the peacock HTTP router.
pub fn router(state: Arc<AppState>) -> Router {
    let mut app = Router::new()
        .route("/healthz", get(healthz))
        .route("/version", get(version))
        .route("/v1/render_report", post(render_report))
        // MCP-Apps JSON-RPC surface (peacock's own host-facing endpoint).
        .route("/mcp", post(crate::mcp::handle))
        // Triton upstream contract (GET / serves the demo SPA; POST / is the
        // header-routed Triton dispatch).
        .route("/", get(index).post(crate::upstream::handle));

    // The Flutter-web client, when a built bundle is provided.
    if let Some(dir) = &state.flutter_dir {
        app = app
            .nest_service("/app", tower_http::services::ServeDir::new(dir))
            // The MCP-Apps `ui://` runtime shim: a single self-contained HTML
            // resource that nests the multi-file Flutter bundle (served at
            // `/app/`) and bridges the host's postMessage channel to it. This
            // is what a host's `ui://peacock/<report>` iframe should point at;
            // serving it here keeps the bridge with the bundle it embeds.
            .route("/app-shim", get(app_shim));
    }

    app.with_state(state)
}

/// The runtime shim served at `GET /app-shim?report=<id>` (FR-M-1). It inlines
/// nothing of the Flutter bundle — it nests `/app/` in a child iframe and relays
/// MCP-Apps `postMessage` between the host and the Flutter app. See
/// `doc/flutter-iframe-runtime-proposal.md`.
const FLUTTER_SHIM_HTML: &str = include_str!("../assets/flutter-shim.html");

#[derive(Deserialize)]
struct ShimQuery {
    /// The report id the embedded Flutter app should render.
    #[serde(default)]
    report: String,
}

async fn app_shim(Query(q): Query<ShimQuery>) -> impl IntoResponse {
    // Inject the report id; the app base is fixed to peacock's `/app/` mount.
    let html = FLUTTER_SHIM_HTML
        .replace("__PEACOCK_APP_BASE__", "/app/")
        .replace("__REPORT_ID__", &q.report);
    Html(html)
}

/// Bind `addr` and serve until the process exits.
pub async fn serve(addr: std::net::SocketAddr, state: Arc<AppState>) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn version() -> impl IntoResponse {
    Json(json!({
        "name": "peacock",
        "version": peacock_types::VERSION,
        // Filled with real SHAs by the build (binary / image / bundle) later.
        "binary_sha": option_env!("PEACOCK_BINARY_SHA").unwrap_or("dev"),
        "bundle_sha": option_env!("PEACOCK_BUNDLE_SHA").unwrap_or("dev"),
    }))
}

async fn index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(state.demo_html)
}

#[derive(Deserialize)]
struct RenderReq {
    report_id: String,
    #[serde(default)]
    params: Value,
    /// Include a rasterized PNG of the chart (chat/demo path).
    #[serde(default)]
    png: bool,
    /// The host the rendering is presented in (`copilot` / `whatsapp` /
    /// `gemini`) — selects the host look-and-feel.
    #[serde(default)]
    host: Option<String>,
    /// The company/brand whose corporate identity to apply. Defaults to the
    /// caller's tenant.
    #[serde(default)]
    brand: Option<String>,
}

async fn render_report(State(state): State<Arc<AppState>>, Json(req): Json<RenderReq>) -> Response {
    // Resolve the theme: corporate identity (brand, default = the caller's
    // tenant) composed under the host look. The same theme styles the chart
    // (tokens) and the web surfaces (CSS).
    let host = req.host.as_deref().unwrap_or("");
    let brand = req
        .brand
        .clone()
        .unwrap_or_else(|| state.principal.tenant.clone());
    let theme = state.themes.resolve(&brand, host);

    let opts = RenderOpts {
        png_scale: req.png.then_some(state.png_scale),
        theme: Some(theme.tokens.clone()),
        ..Default::default()
    };
    let params = if req.params.is_null() {
        json!({})
    } else {
        req.params
    };

    match render(
        &req.report_id,
        &params,
        &state.principal,
        &state.escurel,
        &opts,
    )
    .await
    {
        Ok(art) => {
            let png_b64 = art
                .png
                .as_ref()
                .map(|p| base64::engine::general_purpose::STANDARD.encode(p));
            let trace = pipeline_trace(&req.report_id, host, &brand, &theme, &art, req.png);
            Json(json!({
                "report_id": req.report_id,
                "a2ui": art.a2ui,
                "structuredContent": art.structured_content,
                "vega_specs": art.vega_specs,
                "png_base64": png_b64,
                // The matching CSS for web surfaces (host ⊕ brand). One theme,
                // both the chart and the chrome.
                "theme_css": theme.css,
                "theme": { "host": theme.host, "brand": theme.brand },
                // The render pipeline, step by step (the demo's inspector panel).
                "trace": trace,
            }))
            .into_response()
        }
        Err(e) => error_response(&e),
    }
}

/// Describe the render pipeline that just ran as a cross-actor swimlane — what
/// the demo's "under the hood" inspector shows. Each step names the **actor**
/// that performs it (frontend · agent · triton · peacock · escurel), so a viewer
/// can see who does what and where state is handed off. Steps also carry a
/// `kind` (badge), a title, a one-line `detail`, and optional `code` to expand.
fn pipeline_trace(
    report_id: &str,
    host: &str,
    brand: &str,
    theme: &peacock_rasterizer::Theme,
    art: &peacock_types::Artifact,
    png: bool,
) -> Value {
    let sc = &art.structured_content;
    let rows = sc.rows.as_array().map(Vec::len).unwrap_or(0);
    let host_name = match host {
        "whatsapp" => "WhatsApp",
        "gemini" => "Gemini",
        "copilot" | "" => "Copilot",
        other => other,
    };
    let kinds: Vec<&str> = art
        .a2ui
        .get("components")
        .and_then(Value::as_array)
        .map(|cs| {
            cs.iter()
                .filter_map(|c| c.get("kind").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    let vega = art.vega_specs.first().cloned().unwrap_or(Value::Null);
    let mark = vega
        .get("mark")
        .and_then(|m| m.as_str().or_else(|| m.get("type").and_then(Value::as_str)))
        .unwrap_or("—");

    let params_pretty = serde_json::to_string_pretty(&sc.current_params).unwrap_or_default();

    json!([
        {
            "n": 1, "actor": "frontend", "kind": "ask", "title": "User asks",
            "detail": format!("a question is typed into the {host_name} chat and handed to the agent")
        },
        {
            "n": 2, "actor": "agent", "kind": "plan", "title": "Plan + call tool",
            "detail": "the agent picks the report and calls the render_report tool with absolute params",
            "code": serde_json::to_string_pretty(&json!({
                "name": "render_report",
                "arguments": { "report_id": report_id, "params": sc.current_params, "host": host, "brand": brand }
            })).unwrap_or_default()
        },
        {
            "n": 3, "actor": "triton", "kind": "route", "title": "Authorize + dispatch",
            "detail": "terminates TLS/OIDC, mints the principal, routes to the peacock upstream (POST / · X-Triton-Tool: render_report · Bearer)"
        },
        {
            "n": 4, "actor": "peacock", "kind": "resolve", "title": "Resolve report skill",
            "detail": format!("asks escurel for [[skill::{report_id}]] — peacock holds no DSN, no DB driver")
        },
        {
            "n": 5, "actor": "escurel", "kind": "data", "title": "resolve(skill)",
            "detail": "returns the report skill: params schema, data refs, view layout, chart specs"
        },
        {
            "n": 6, "actor": "peacock", "kind": "read", "title": "Read rows",
            "detail": "calls query_instance(view, params) — untrusted params travel as typed values, peacock builds no SQL string"
        },
        {
            "n": 7, "actor": "escurel", "kind": "data", "title": "query_instance",
            "detail": format!("{rows} access-checked rows · :params bound as prepared-statement values · ACL enforced here (the only data path)"),
            "code": params_pretty
        },
        {
            "n": 8, "actor": "peacock", "kind": "compose", "title": "Compose A2UI v0.9",
            "detail": format!("layout components: {}", if kinds.is_empty() { "—".into() } else { kinds.join(", ") })
        },
        {
            "n": 9, "actor": "peacock", "kind": "guardrail", "title": "Render guardrail",
            "detail": "inline-data-only · no remote data.url · no expr/signal — an agent-authored spec can't fetch or compute beyond its rows ✓"
        },
        {
            "n": 10, "actor": "peacock", "kind": "vega", "title": format!("Vega-Lite spec · mark “{mark}”"),
            "detail": "the named chart spec with the rows injected inline",
            "code": serde_json::to_string_pretty(&vega).unwrap_or_default()
        },
        {
            "n": 11, "actor": "peacock", "kind": "theme", "title": format!("Theme · {brand} ⊕ {host}"),
            "detail": "one CSS source styles the chart tokens AND the web chrome",
            "code": theme.css.trim().to_string()
        },
        {
            "n": 12, "actor": "peacock", "kind": "raster", "title": "Rasterize → PNG",
            "detail": if png { "pure-Rust Vega-Lite → SVG → PNG (resvg/tiny-skia, no Node/Deno/network)" } else { "(PNG not requested on this surface)" }
        },
        {
            "n": 13, "actor": "triton", "kind": "relay", "title": "Relay surface",
            "detail": "passes the A2UI surface + structuredContent + the ui:// resource back toward the agent and host"
        },
        {
            "n": 14, "actor": "agent", "kind": "state", "title": "updateModelContext (FR-X)",
            "detail": "view state = the absolute parameter vector; the agent keeps a compact {report_id, params, summary} — no rows. peacock stays stateless.",
            "code": serde_json::to_string_pretty(&json!({
                "report_id": report_id, "params": sc.current_params, "salient_summary": "…"
            })).unwrap_or_default()
        },
        {
            "n": 15, "actor": "frontend", "kind": "render", "title": "Render the card",
            "detail": format!("{host_name} paints the themed report; a drill or follow-up loops back to the agent (step 2)")
        }
    ])
}

/// Map a peacock error to an HTTP status by **variant** (HLD §8.3), never by
/// inspecting the message.
fn error_response(e: &Error) -> Response {
    let status = match e {
        Error::Auth(_) => StatusCode::UNAUTHORIZED,
        Error::Validation(_) => StatusCode::BAD_REQUEST,
        Error::Data(_) => StatusCode::BAD_GATEWAY,
        Error::Render(_) => StatusCode::UNPROCESSABLE_ENTITY,
    };
    (
        status,
        Json(json!({ "error": e.kind(), "message": e.to_string() })),
    )
        .into_response()
}
