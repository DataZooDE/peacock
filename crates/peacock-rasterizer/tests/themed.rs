//! A resolved (brand × host) theme restyles the chart and dashboard.

use peacock_rasterizer::{
    DashboardRequest, ThemeRegistry, render_dashboard_to_png_themed, render_vega_to_png_themed,
    render_vega_to_svg_themed,
};
use serde_json::json;

fn chart() -> serde_json::Value {
    json!({
        "data": { "values": [
            { "m": "Jan", "c": "A", "v": 10 }, { "m": "Feb", "c": "A", "v": 14 },
            { "m": "Jan", "c": "B", "v": 7 },  { "m": "Feb", "c": "B", "v": 9 }
        ]},
        "mark": "line",
        "encoding": {
            "x": { "field": "m", "type": "ordinal" },
            "y": { "field": "v", "type": "quantitative" },
            "color": { "field": "c", "type": "nominal" }
        }
    })
}

#[test]
fn theme_changes_chart_palette_background_and_font() {
    let reg = ThemeRegistry::builtin();
    let a = reg.resolve("company-a", "copilot"); // purple brand
    let b = reg.resolve("company-b", "whatsapp"); // orange brand, beige bg

    let svg_a = render_vega_to_svg_themed(&chart(), &a.tokens).unwrap();
    let svg_b = render_vega_to_svg_themed(&chart(), &b.tokens).unwrap();

    // Company A's purple palette is in A's SVG; Tableau10 default is gone.
    assert!(svg_a.contains("#6b3fa0"), "Acme A purple series colour");
    assert!(!svg_a.contains("#4c78a8"), "stock Tableau10 replaced");
    // Company B on WhatsApp uses the beige host background + orange palette.
    assert!(svg_b.contains("#efeae2"), "WhatsApp beige background");
    assert!(svg_b.contains("#e8590c"), "Beta B orange series colour");
    // The two themes are visibly different.
    assert_ne!(svg_a, svg_b);
    // Brand font requested (with the vendored fallback for the rasterizer).
    assert!(svg_a.contains("Inter") && svg_a.contains("DejaVu Sans"));
}

#[test]
fn themed_chart_and_dashboard_render_real_pngs() {
    let reg = ThemeRegistry::builtin();
    let t = reg.resolve("company-b", "gemini");

    let png = render_vega_to_png_themed(&chart(), 2.0, &t.tokens).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");

    let dash: DashboardRequest = serde_json::from_value(json!({
        "title": "Revenue", "tiles": [{ "label": "Total", "value": "$2,986", "trend": "+12%" }]
    }))
    .unwrap();
    let dpng = render_dashboard_to_png_themed(&dash, 2.0, &t.tokens).unwrap();
    assert_eq!(&dpng[..8], b"\x89PNG\r\n\x1a\n");
}
