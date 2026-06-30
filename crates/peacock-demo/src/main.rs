//! `peacock-demo` — a one-command, self-contained demo to **verify peacock**.
//!
//! It spawns a *real* escurel (seeded with the paper's Northwind report over an
//! offline Parquet view), points a real peacock HTTP service at it, and serves
//! a Microsoft-Copilot-style chat client at `http://127.0.0.1:8080`. Ask for
//! the Northwind revenue report, see the rendered dashboard (KPI + chart +
//! table), and click a category to drill — every render goes through the real
//! peacock render core against real escurel. No mocks.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use peacock_core::EscurelData;
use peacock_server::{AppState, serve};
use peacock_test_support::NorthwindEscurel;
use triton_tests::TritonProcess;

const DEMO_HTML: &str = include_str!("../assets/demo.html");

/// Spawn a **real** Triton in front of peacock so the demo's render path can
/// mirror each call through it and show the genuine Triton→peacock dispatch in
/// the inspector. Returns `None` (and logs) when the `triton` binary isn't
/// built — the demo still runs, the Triton step just omits its payload.
async fn spawn_triton(peacock_addr: &str) -> Option<TritonProcess> {
    let bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../triton/target/debug/triton");
    if !bin.exists() {
        eprintln!(
            "peacock-demo: triton binary not found ({}); the inspector's Triton \
             step will have no captured payload. Build it with `cargo build --bin \
             triton` in ../triton to enable the real gateway hop.",
            bin.display()
        );
        return None;
    }
    let env = HashMap::from([
        ("TRITON_ENV".into(), "nonprod".into()),
        (
            "TRITON_STATIC_UPSTREAMS".into(),
            format!("render_report={peacock_addr}"),
        ),
    ]);
    // `TritonProcess::spawn_with_env` *panics* on any boot problem — including
    // its own staleness check trying to rebuild `triton-bin` from peacock's
    // workspace (where that package doesn't exist). Run it in a task so a panic
    // surfaces as a JoinError we can swallow: the demo then simply runs without
    // the gateway hop instead of aborting.
    match tokio::spawn(
        async move { TritonProcess::spawn_with_env(Duration::from_secs(15), env).await },
    )
    .await
    {
        Ok(triton) => {
            eprintln!("peacock-demo: real Triton up at {}", triton.mcp_url("/"));
            Some(triton)
        }
        Err(e) => {
            eprintln!(
                "peacock-demo: could not start the real Triton ({e}); continuing \
                 without the gateway hop — the inspector's Triton step will have no \
                 captured payload. (Build a fresh triton binary: `cargo build --bin \
                 triton` in ../triton.)"
            );
            None
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("warn,peacock_demo=info")
        .init();

    eprintln!("peacock-demo: starting a real escurel seeded with Northwind…");
    let nw = NorthwindEscurel::spawn().await;
    eprintln!("peacock-demo: escurel up at {}", nw.endpoint());

    let port: u16 = std::env::var("PEACOCK_DEMO_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    // Serve the built Flutter-web client at /app when the bundle exists.
    let flutter_dir = {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/peacock-web/build/web");
        d.is_dir().then_some(d)
    };
    // The demo is directly reachable, so its MCP-Apps `ui://` resource uses the
    // Flutter shim pointing at this server's own `/app/`.
    let flutter_app_url = flutter_dir
        .as_ref()
        .map(|_| format!("http://127.0.0.1:{port}/app/"));

    // A real Triton in front of peacock (when its binary is built), so the
    // inspector's Triton step shows a genuine captured dispatch.
    let triton = spawn_triton(&format!("127.0.0.1:{port}")).await;
    let triton_url = triton.as_ref().map(|t| t.mcp_url("/"));

    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: DEMO_HTML,
        flutter_dir,
        flutter_app_url,
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url,
        upstream_capture: Default::default(),
    });

    println!("\n  ✦ peacock demo ready → http://{addr}\n");
    // Keep the escurel + triton processes alive for the server's lifetime.
    let _escurel = nw;
    let _triton = triton;
    serve(addr, state).await.expect("peacock-demo server");
}
