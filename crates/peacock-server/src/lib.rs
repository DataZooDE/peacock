//! peacock service surfaces (thin shells over `peacock-core`).
//!
//! This crate hosts the HTTP faces: the structured/`render_report` surface
//! (JSON artifact for agents and the demo chat client), observability
//! (`/healthz`, `/version`), and static serving of the demo SPA. The MCP-App
//! and Triton-upstream wire shapes layer on top of the same `render_report`
//! handler.
//!
//! Every surface funnels through `peacock_core::render` — surfaces carry no
//! composition logic (FR-R-1, HLD §5).

mod http;
mod mcp;
mod upstream;

pub use http::{AppState, router, serve};
pub use mcp::UI_AUTHORITY;
