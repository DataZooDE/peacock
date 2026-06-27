//! peacock service surfaces (thin shells over `peacock-core`):
//! the MCP-App surface (`ui://` resource + drill bridge), the chat/upstream
//! shim (the `POST /` Triton-upstream contract + `render_a2ui_to_png`),
//! observability (`/healthz`, `/version`, `/metrics`, audit), and lifecycle
//! (cold start, SIGTERM drain). Surfaces carry no composition logic (HLD §5).

#[cfg(test)]
mod tests {
    #[test]
    fn server_crate_links() {
        assert_eq!(2 + 2, 4);
    }
}
