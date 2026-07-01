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

#[test]
fn continuous_colour_bars_map_each_bar_to_its_value() {
    // A quantitative colour on a BAR mark colours each bar by ITS value on the
    // sequential ramp — not a single flat colour. High risk → red-dominant,
    // low risk → green-dominant (the `risk` scheme).
    let spec = json!({
        "mark": "bar",
        "data": { "values": [
            { "s": "high", "v": 100, "risk": 0.95 },
            { "s": "low",  "v": 80,  "risk": 0.05 }
        ] },
        "encoding": {
            "x": { "field": "s", "type": "nominal" },
            "y": { "field": "v", "type": "quantitative" },
            "color": { "field": "risk", "type": "quantitative",
                       "scale": { "scheme": "risk" } }
        }
    });
    let svg = render_vega_to_svg(&spec).expect("svg");
    // Pull every `fill="#rrggbb"` and classify.
    let fills: Vec<(u8, u8, u8)> = svg
        .split("fill=\"#")
        .skip(1)
        .filter_map(|s| {
            let h = s.get(..6)?;
            Some((
                u8::from_str_radix(&h[0..2], 16).ok()?,
                u8::from_str_radix(&h[2..4], 16).ok()?,
                u8::from_str_radix(&h[4..6], 16).ok()?,
            ))
        })
        .collect();
    let red = fills.iter().any(|(r, g, _)| *r as i16 - *g as i16 > 40);
    let green = fills.iter().any(|(r, g, _)| *g as i16 - *r as i16 > 40);
    assert!(red, "high-risk bar is red-dominant; fills: {fills:?}");
    assert!(green, "low-risk bar is green-dominant; fills: {fills:?}");
}

#[test]
fn horizontal_bar_renders_full_band_bars() {
    // Categories on y, measure on x → horizontal bars. Each bar spans its full
    // y-band (not a 1px sliver from the inverted y-range) and starts at x=0.
    let spec = json!({
        "mark": "bar",
        "data": { "values": [
            { "cat": "Alpha", "v": 100 },
            { "cat": "Beta",  "v": 40 },
            { "cat": "Gamma", "v": 70 }
        ] },
        "encoding": {
            "y": { "field": "cat", "type": "nominal" },
            "x": { "field": "v",   "type": "quantitative" }
        }
    });
    let svg = render_vega_to_svg(&spec).expect("svg");
    // Rect heights: the background is the tallest; the 3 bars share a full-band
    // height well above the 1px clamp a mis-oriented render would produce.
    let mut heights: Vec<f64> = svg
        .split("<rect")
        .skip(1)
        .filter_map(|frag| {
            let h = frag.split("height=\"").nth(1)?.split('"').next()?;
            h.parse::<f64>().ok()
        })
        .collect();
    heights.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let max_h = heights.first().copied().unwrap_or(0.0);
    let bars = heights
        .iter()
        .filter(|&&h| h > 15.0 && h < max_h * 0.9)
        .count();
    assert!(
        bars >= 3,
        "3 full-band horizontal bars; heights: {heights:?}"
    );
}
