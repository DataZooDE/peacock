//! Phase 6: the embedded library face (FR-E-1..3, ACC-7). A Rust caller links
//! `peacock-core` in-process, supplies an escurel binding + principal, and gets
//! an artifact via the **same** render path as the service — no iframe, no
//! credentials of its own.

use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{NW_REPORT, NorthwindEscurel};
use serde_json::json;

#[tokio::test]
async fn embedded_caller_renders_via_the_same_core_path() {
    // The "agent" embeds peacock and supplies the escurel binding + principal.
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let art = render(
        NW_REPORT,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("embedded render");

    // Same artifact shape the service produces (single render path, FR-R-1).
    assert_eq!(art.a2ui["version"], "0.9");
    assert_eq!(art.structured_content.rows.as_array().unwrap().len(), 16);

    // Determinism across the embedded path (FR-R-2): a second identical call
    // reproduces the artifact byte-for-byte.
    let again = render(
        NW_REPORT,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .unwrap();
    assert_eq!(art, again);

    nw.shutdown().await;
}
