//! The peacock HTTP app: `render_report`, observability, and the demo SPA.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
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
}

/// Build the peacock HTTP router.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/version", get(version))
        .route("/v1/render_report", post(render_report))
        .route("/", get(index))
        .with_state(state)
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
}

async fn render_report(State(state): State<Arc<AppState>>, Json(req): Json<RenderReq>) -> Response {
    let opts = RenderOpts {
        png_scale: req.png.then_some(state.png_scale),
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
            Json(json!({
                "report_id": req.report_id,
                "a2ui": art.a2ui,
                "structuredContent": art.structured_content,
                "vega_specs": art.vega_specs,
                "png_base64": png_b64,
            }))
            .into_response()
        }
        Err(e) => error_response(&e),
    }
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
