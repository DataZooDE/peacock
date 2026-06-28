//! The `peacock` binary: CLI / `PEACOCK_*` settings + manifest loader and the
//! process entrypoint (FR-L, FR-O, FR-I). Cold-starts with no local state,
//! closed-checks the manifest, binds the tailnet listeners, and drains
//! in-flight renders on SIGTERM/SIGINT (FR-L-2). Identity is the configured
//! escurel principal (the deployment's binding); behind Triton the forwarded
//! token would be exchanged per request.

mod manifest;

use std::sync::Arc;

use clap::Parser;
use peacock_core::EscurelData;
use peacock_server::{AppState, router};
use peacock_types::Principal;
use tokio::signal::unix::{SignalKind, signal};

use crate::manifest::Manifest;

/// peacock — the report renderer + MCP-App host.
#[derive(Parser, Debug)]
#[command(name = "peacock", version)]
struct Settings {
    /// Bind address for the service listeners.
    #[arg(long, env = "PEACOCK_BIND", default_value = "127.0.0.1:8080")]
    bind: String,

    /// escurel endpoint (overrides the manifest's `escurel.url`).
    #[arg(long, env = "PEACOCK_ESCUREL_URL")]
    escurel_url: Option<String>,

    /// Path to the boot manifest (TOML).
    #[arg(long, env = "PEACOCK_MANIFEST")]
    manifest: Option<String>,

    /// Deployment environment; `prod` enables Vault-ref-only secrets.
    #[arg(long, env = "PEACOCK_ENV", default_value = "local")]
    env: String,

    /// Tenant forwarded to escurel.
    #[arg(long, env = "PEACOCK_TENANT", default_value = "acme")]
    tenant: String,

    /// Subject forwarded to escurel.
    #[arg(long, env = "PEACOCK_SUB", default_value = "peacock")]
    sub: String,

    /// The escurel bearer token (dev binding). In production this is a
    /// Vault-minted, short-lived token injected by the substrate.
    #[arg(long, env = "PEACOCK_ESCUREL_TOKEN", default_value = "")]
    escurel_token: String,

    /// Chart PNG scale for the chat surface.
    #[arg(long, env = "PEACOCK_PNG_SCALE", default_value = "2.0")]
    png_scale: f32,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    if let Err(e) = run(Settings::parse()).await {
        // A boot failure is fatal and named (ACC-10).
        eprintln!("peacock: boot refused: {e}");
        std::process::exit(1);
    }
}

async fn run(s: Settings) -> Result<(), String> {
    let production = s.env == "prod" || s.env == "nonprod";

    // Closed-check the manifest at boot; refuse on any unknown value (FR-L-3).
    let manifest = match &s.manifest {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("reading manifest {path}: {e}"))?;
            Some(Manifest::parse(&text, production).map_err(|e| e.to_string())?)
        }
        None => None,
    };

    // Resolve the escurel binding (CLI/env wins over the manifest).
    let escurel_url = s
        .escurel_url
        .clone()
        .or_else(|| manifest.as_ref().and_then(|m| m.escurel_url.clone()))
        .ok_or("no escurel endpoint (set --escurel-url / PEACOCK_ESCUREL_URL or manifest)")?;

    let principal = Principal {
        sub: s.sub.clone(),
        scopes: Vec::new(),
        groups: Vec::new(),
        tenant: s.tenant.clone(),
        raw_token: s.escurel_token.clone(),
        trace_id: String::new(),
    };

    let state = Arc::new(AppState {
        escurel: EscurelData::new(escurel_url),
        principal,
        png_scale: s.png_scale,
        demo_html: "<!doctype html><title>peacock</title><p>peacock is running. \
                    POST /v1/render_report or /mcp.</p>",
        flutter_dir: None,
        flutter_app_url: None,
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });

    let addr: std::net::SocketAddr = s
        .bind
        .parse()
        .map_err(|e| format!("invalid --bind {}: {e}", s.bind))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;

    tracing::info!(%addr, "peacock listening");

    // Serve with graceful shutdown: stop accepting new work on SIGTERM/SIGINT,
    // drain in-flight renders, exit 0 (FR-L-2).
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("serve: {e}"))
}

async fn shutdown_signal() {
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut intr = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = term.recv() => tracing::info!("SIGTERM — draining"),
        _ = intr.recv() => tracing::info!("SIGINT — draining"),
    }
}
