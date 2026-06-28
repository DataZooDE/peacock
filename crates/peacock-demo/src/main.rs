//! `peacock-demo` — a one-command, self-contained demo to **verify peacock**.
//!
//! It spawns a *real* escurel (seeded with the paper's Northwind report over an
//! offline Parquet view), points a real peacock HTTP service at it, and serves
//! a Microsoft-Copilot-style chat client at `http://127.0.0.1:8080`. Ask for
//! the Northwind revenue report, see the rendered dashboard (KPI + chart +
//! table), and click a category to drill — every render goes through the real
//! peacock render core against real escurel. No mocks.

use std::sync::Arc;

use peacock_core::EscurelData;
use peacock_server::{AppState, serve};
use peacock_test_support::NorthwindEscurel;

const DEMO_HTML: &str = include_str!("../assets/demo.html");

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

    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: DEMO_HTML,
        flutter_dir,
        flutter_app_url,
    });

    println!("\n  ✦ peacock demo ready → http://{addr}\n");
    // Keep the escurel process alive for the server's lifetime.
    let _escurel = nw;
    serve(addr, state).await.expect("peacock-demo server");
}
