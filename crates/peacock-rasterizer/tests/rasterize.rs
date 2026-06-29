//! Phase 5: render peacock's Vega-Lite subset to SVG + a real PNG, pure Rust
//! (no Node, no Deno, no network — NFR-S-5, FR-V-2).

use peacock_rasterizer::{render_vega_to_png, render_vega_to_svg};
use serde_json::json;

/// The Northwind revenue line chart with aggregated rows inline — exactly the
/// shape peacock's composer produces (FR-V-3).
fn nw_chart() -> serde_json::Value {
    json!({
        "data": { "values": [
            { "month": "1997-01-01", "category": "Beverages",  "revenue": 180.0 },
            { "month": "1997-02-01", "category": "Beverages",  "revenue": 81.0 },
            { "month": "1997-03-01", "category": "Beverages",  "revenue": 0.0 },
            { "month": "1997-01-01", "category": "Condiments", "revenue": 110.0 },
            { "month": "1997-02-01", "category": "Condiments", "revenue": 0.0 },
            { "month": "1997-03-01", "category": "Condiments", "revenue": 198.0 }
        ]},
        "mark": "line",
        "encoding": {
            "x": { "field": "month", "type": "temporal", "title": "Month" },
            "y": { "field": "revenue", "type": "quantitative", "aggregate": "sum" },
            "color": { "field": "category", "type": "nominal" }
        }
    })
}

#[test]
fn compiles_subset_to_svg_with_axes_and_legend() {
    let svg = render_vega_to_svg(&nw_chart()).expect("svg");
    assert!(svg.starts_with("<svg"));
    assert!(svg.ends_with("</svg>"));
    // One polyline per series (2 categories).
    assert_eq!(svg.matches("<polyline").count(), 2);
    // Legend labels for both series.
    assert!(svg.contains("Beverages"));
    assert!(svg.contains("Condiments"));
    // Axis title from the encoding.
    assert!(svg.contains("Month"));
    // No external data references in the output — only the inert SVG xmlns is
    // allowed (NFR-S-5). The renderer emits shapes/text from inline rows only.
    assert!(!svg.contains("href"), "no hyperlinks / external refs");
    assert!(!svg.contains("<image"), "no embedded/remote images");
}

#[test]
fn temporal_x_axis_is_chronologically_ordered() {
    // Rows arrive out of order; a temporal x axis must render sorted.
    let spec = json!({
        "data": { "values": [
            { "month": "1997-10-01", "category": "A", "revenue": 5.0 },
            { "month": "1997-01-01", "category": "A", "revenue": 1.0 },
            { "month": "1997-05-01", "category": "A", "revenue": 3.0 }
        ]},
        "mark": "line",
        "encoding": {
            "x": { "field": "month", "type": "temporal" },
            "y": { "field": "revenue", "type": "quantitative" }
        }
    });
    let svg = render_vega_to_svg(&spec).expect("svg");
    let jan = svg.find("1997-01").unwrap();
    let may = svg.find("1997-05").unwrap();
    let oct = svg.find("1997-10").unwrap();
    assert!(jan < may && may < oct, "x labels must be chronological");
}

#[test]
fn bar_mark_emits_rects() {
    let mut spec = nw_chart();
    spec["mark"] = json!("bar");
    let svg = render_vega_to_svg(&spec).expect("svg");
    assert!(svg.contains("<rect"), "bar mark draws rects");
}

#[test]
fn renders_a_real_png() {
    let png = render_vega_to_png(&nw_chart(), 2.0).expect("png");
    // Valid PNG magic + non-trivial body.
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic header");
    assert!(png.len() > 1000, "non-empty raster: {} bytes", png.len());
}

#[test]
fn unsupported_mark_is_an_error() {
    let mut spec = nw_chart();
    spec["mark"] = json!("geoshape");
    assert!(render_vega_to_svg(&spec).is_err());
}
