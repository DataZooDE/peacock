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

/// Describe the render pipeline that just ran, step by step — what the demo's
/// "under the hood" inspector shows. Each step carries a `kind` (for styling), a
/// title, a one-line `detail`, and optional `code` (JSON/CSS) to expand.
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

    json!([
        {
            "n": 1, "kind": "tool", "title": "Tool call · render_report",
            "detail": format!("host = {host} · brand = {brand}"),
            "code": serde_json::to_string_pretty(&json!({
                "name": "render_report",
                "arguments": { "report_id": report_id, "params": sc.current_params, "host": host, "brand": brand }
            })).unwrap_or_default()
        },
        {
            "n": 2, "kind": "data", "title": "escurel · resolve(skill)",
            "detail": format!("[[skill::{report_id}]] → report skill (params schema, data refs, views)")
        },
        {
            "n": 3, "kind": "data", "title": "escurel · query_instance(view, params)",
            "detail": format!("{rows} access-checked rows · params bound as prepared-statement values (peacock builds no SQL, holds no DSN)"),
            "code": serde_json::to_string_pretty(&sc.current_params).unwrap_or_default()
        },
        {
            "n": 4, "kind": "compose", "title": "Compose A2UI v0.9",
            "detail": format!("layout components: {}", if kinds.is_empty() { "—".into() } else { kinds.join(", ") })
        },
        {
            "n": 5, "kind": "guardrail", "title": "Render guardrail",
            "detail": "inline-data-only · no remote data.url · no expr/signal — an agent-authored spec can't fetch or compute beyond its rows ✓"
        },
        {
            "n": 6, "kind": "vega", "title": format!("Vega-Lite spec · mark “{mark}” (rows injected inline)"),
            "code": serde_json::to_string_pretty(&vega).unwrap_or_default()
        },
        {
            "n": 7, "kind": "theme", "title": format!("Theme · {brand} ⊕ {host}"),
            "detail": "one CSS source styles the chart tokens AND the web chrome",
            "code": theme.css.trim().to_string()
        },
        {
            "n": 8, "kind": "raster", "title": "Rasterize → PNG",
            "detail": if png { "pure-Rust Vega-Lite → SVG → PNG (resvg/tiny-skia, no Node/Deno/network)" } else { "(not requested)" }
        },
        {
            "n": 9, "kind": "state", "title": "State transfer (FR-X)",
            "detail": "view state = the absolute parameter vector; pushed to the model via updateModelContext {report_id, params, summary} — compact, no rows. peacock stays stateless.",
            "code": serde_json::to_string_pretty(&json!({
                "report_id": report_id, "params": sc.current_params, "salient_summary": "…"
            })).unwrap_or_default()
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
