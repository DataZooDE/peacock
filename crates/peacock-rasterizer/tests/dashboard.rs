//! Render a Triton dashboard (`{title, tiles}`) to SVG + a real PNG — the
//! `render_a2ui_to_png` delegation peacock provides to Triton (#143 D).

use peacock_rasterizer::{DashboardRequest, render_dashboard_to_png, render_dashboard_to_svg};
use serde_json::json;

fn req() -> DashboardRequest {
    serde_json::from_value(json!({
        "title": "Northwind revenue",
        "tiles": [
            { "label": "Total revenue", "value": "$2,986", "trend": "+12%" },
            { "label": "Categories", "value": "5" },
            { "label": "Top category", "value": "Beverages", "trend": "-3%" }
        ]
    }))
    .unwrap()
}

#[test]
fn renders_tiles_to_svg() {
    let svg = render_dashboard_to_svg(&req());
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Northwind revenue"));
    assert!(svg.contains("$2,986"));
    assert!(svg.contains("Beverages"));
    // One card rect per tile (+ the background rect).
    assert!(svg.matches("<rect").count() >= 4);
}

#[test]
fn renders_a_real_png() {
    let png = render_dashboard_to_png(&req(), 2.0).expect("png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(png.len() > 1000);
}

#[test]
fn empty_dashboard_still_renders() {
    let r: DashboardRequest = serde_json::from_value(json!({ "title": "Empty" })).unwrap();
    let png = render_dashboard_to_png(&r, 1.0).expect("png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}
