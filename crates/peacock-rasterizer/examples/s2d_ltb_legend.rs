//! Reproduce the S2D last-time-buy chart with a bottom legend.
use peacock_rasterizer::{render_vega_to_png, render_vega_to_svg};
use serde_json::json;

fn main() {
    let png_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/s2d-ltb-bottom-legend.png".to_owned());
    let orient = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "bottom".to_owned());
    let spec = json!({
        "data": {"values": [
            {"month": "2026-01", "demand": 18, "ltb": 5},
            {"month": "2026-02", "demand": 35, "ltb": 14},
            {"month": "2026-03", "demand": 53, "ltb": 24},
            {"month": "2026-04", "demand": 76, "ltb": 39}
        ]},
        "transform": [
            {"aggregate": [
                {"op": "sum", "field": "demand", "as": "Cumulative expected demand"},
                {"op": "sum", "field": "ltb", "as": "Last-time-buy quantity"}
            ], "groupby": ["month"]},
            {"fold": ["Cumulative expected demand", "Last-time-buy quantity"], "as": ["series", "quantity"]}
        ],
        "mark": "line",
        "encoding": {
            "x": {"field": "month", "type": "ordinal"},
            "y": {"field": "quantity", "type": "quantitative"},
            "color": {"field": "series", "type": "nominal", "legend": {"orient": orient, "title": null, "labelLimit": 0}}
        }
    });
    std::fs::write(&png_path, render_vega_to_png(&spec, 2.0).expect("png")).expect("write png");
    std::fs::write(
        png_path.replace(".png", ".svg"),
        render_vega_to_svg(&spec).expect("svg"),
    )
    .expect("write svg");
    println!("{png_path}");
}
