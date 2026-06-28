//! Broad Vega-Lite parity tests (Track C). Each test asserts the rendered SVG
//! structure for a feature and that `render_vega_to_png` yields a valid PNG.
//! Pure Rust — no Node, no Deno, no network (NFR-S-5).

use peacock_rasterizer::{render_vega_to_png, render_vega_to_svg};
use serde_json::{Value, json};

/// Count occurrences of an SVG tag.
fn count(svg: &str, tag: &str) -> usize {
    svg.matches(tag).count()
}

/// Assert the spec renders both to SVG and to a valid PNG.
fn renders_png(spec: &Value) -> String {
    let svg = render_vega_to_svg(spec).expect("svg");
    let png = render_vega_to_png(spec, 2.0).expect("png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic header");
    assert!(png.len() > 500, "non-trivial raster: {} bytes", png.len());
    svg
}

// ---------------------------------------------------------------------------
// Marks
// ---------------------------------------------------------------------------

#[test]
fn stacked_bar_emits_one_rect_per_datum() {
    let spec = json!({
        "data": {"values": [
            {"q": "Q1", "cat": "A", "v": 10.0},
            {"q": "Q1", "cat": "B", "v": 5.0},
            {"q": "Q2", "cat": "A", "v": 8.0},
            {"q": "Q2", "cat": "B", "v": 12.0}
        ]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative", "stack": "zero"},
            "color": {"field": "cat", "type": "nominal"}
        }
    });
    let svg = renders_png(&spec);
    // 4 data rects + 1 background rect + 2 legend swatch rects = 7.
    assert_eq!(
        count(&svg, "<rect"),
        7,
        "stacked bars: one rect per (x,series)"
    );
}

#[test]
fn grouped_bar_places_series_side_by_side() {
    let spec = json!({
        "data": {"values": [
            {"q": "Q1", "cat": "A", "v": 10.0},
            {"q": "Q1", "cat": "B", "v": 5.0},
            {"q": "Q2", "cat": "A", "v": 8.0},
            {"q": "Q2", "cat": "B", "v": 12.0}
        ]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative", "stack": null},
            "color": {"field": "cat", "type": "nominal"}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(
        count(&svg, "<rect"),
        7,
        "grouped bars: one rect per (x,series)"
    );
}

#[test]
fn stacked_area_emits_polygon_per_series() {
    let spec = json!({
        "data": {"values": [
            {"t": "1997-01-01", "cat": "A", "v": 10.0},
            {"t": "1997-02-01", "cat": "A", "v": 20.0},
            {"t": "1997-01-01", "cat": "B", "v": 5.0},
            {"t": "1997-02-01", "cat": "B", "v": 8.0}
        ]},
        "mark": "area",
        "encoding": {
            "x": {"field": "t", "type": "temporal"},
            "y": {"field": "v", "type": "quantitative"},
            "color": {"field": "cat", "type": "nominal"}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<polygon"), 2, "one filled band per series");
}

#[test]
fn point_scatter_on_quantitative_x() {
    let spec = json!({
        "data": {"values": [
            {"x": 1.0, "y": 2.0},
            {"x": 3.0, "y": 5.0},
            {"x": 7.0, "y": 1.0}
        ]},
        "mark": "point",
        "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<circle"), 3, "one circle per scatter datum");
}

#[test]
fn arc_pie_emits_path_wedges() {
    let spec = json!({
        "data": {"values": [
            {"cat": "A", "v": 30.0},
            {"cat": "B", "v": 50.0},
            {"cat": "C", "v": 20.0}
        ]},
        "mark": "arc",
        "encoding": {
            "theta": {"field": "v", "type": "quantitative"},
            "color": {"field": "cat", "type": "nominal"}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<path"), 3, "one wedge per category");
    assert!(svg.contains("A") && svg.contains("B") && svg.contains("C"));
}

#[test]
fn arc_donut_uses_inner_radius() {
    let spec = json!({
        "data": {"values": [
            {"cat": "A", "v": 30.0},
            {"cat": "B", "v": 70.0}
        ]},
        "mark": {"type": "arc", "innerRadius": 60.0},
        "encoding": {
            "theta": {"field": "v", "type": "quantitative"},
            "color": {"field": "cat", "type": "nominal"}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<path"), 2);
    // donut wedges are annular: two arc segments → contains a second 'A' arc cmd.
    assert!(
        svg.matches(" A").count() >= 4,
        "annular paths have two arcs each"
    );
}

#[test]
fn rect_heatmap_x_by_y_by_color() {
    let spec = json!({
        "data": {"values": [
            {"r": "r1", "c": "c1", "v": 1.0},
            {"r": "r1", "c": "c2", "v": 2.0},
            {"r": "r2", "c": "c1", "v": 3.0},
            {"r": "r2", "c": "c2", "v": 4.0}
        ]},
        "mark": "rect",
        "encoding": {
            "x": {"field": "c", "type": "ordinal"},
            "y": {"field": "r", "type": "ordinal"},
            "color": {"field": "v", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    // 4 heatmap cells + background + gradient legend rects.
    assert!(count(&svg, "<rect") >= 5, "one rect per cell");
    // continuous colour → gradient legend (many thin rects)
    assert!(count(&svg, "<rect") > 10, "gradient legend present");
}

#[test]
fn tick_mark_emits_lines() {
    let spec = json!({
        "data": {"values": [
            {"x": 1.0, "y": 2.0},
            {"x": 3.0, "y": 5.0}
        ]},
        "mark": "tick",
        "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    // axis lines (2) + 2 ticks
    assert!(count(&svg, "<line") >= 4);
}

#[test]
fn rule_reference_line_from_value() {
    let spec = json!({
        "data": {"values": [
            {"q": "Q1", "v": 10.0},
            {"q": "Q2", "v": 20.0}
        ]},
        "mark": "rule",
        "encoding": {
            "y": {"value": 15.0}
        }
    });
    let svg = renders_png(&spec);
    assert!(count(&svg, "<line") >= 1, "a horizontal reference rule");
}

#[test]
fn text_mark_labels_values() {
    let spec = json!({
        "data": {"values": [
            {"q": "Q1", "v": 10.0},
            {"q": "Q2", "v": 20.0}
        ]},
        "mark": "text",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert!(
        svg.contains(">10<") && svg.contains(">20<"),
        "value labels rendered"
    );
}

#[test]
fn circle_mark_renders() {
    let spec = json!({
        "data": {"values": [{"x": 1.0, "y": 2.0}, {"x": 2.0, "y": 4.0}]},
        "mark": "circle",
        "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<circle"), 2);
}

// ---------------------------------------------------------------------------
// Encodings
// ---------------------------------------------------------------------------

#[test]
fn size_encoding_varies_radius() {
    let spec = json!({
        "data": {"values": [
            {"x": 1.0, "y": 1.0, "s": 1.0},
            {"x": 2.0, "y": 2.0, "s": 100.0}
        ]},
        "mark": "point",
        "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative"},
            "size": {"field": "s", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<circle"), 2);
    // radii should differ between the two points
    let radii: Vec<&str> = svg
        .match_indices("<circle")
        .filter_map(|(i, _)| {
            let frag = &svg[i..];
            let r_at = frag.find(" r=\"")? + 4;
            let rest = &frag[r_at..];
            let end = rest.find('"')?;
            Some(&rest[..end])
        })
        .collect();
    assert!(
        radii.len() == 2 && radii[0] != radii[1],
        "size maps to radius: {radii:?}"
    );
}

#[test]
fn opacity_encoding_emits_fill_opacity() {
    let spec = json!({
        "data": {"values": [
            {"x": 1.0, "y": 1.0, "o": 0.0},
            {"x": 2.0, "y": 2.0, "o": 100.0}
        ]},
        "mark": "point",
        "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative"},
            "opacity": {"field": "o", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert!(svg.contains("fill-opacity"), "opacity encoding present");
}

#[test]
fn tooltip_encoding_is_ignored_gracefully() {
    let spec = json!({
        "data": {"values": [{"q": "Q1", "v": 5.0}]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative"},
            "tooltip": {"field": "v", "type": "quantitative"}
        }
    });
    renders_png(&spec); // must not error
}

#[test]
fn order_sort_on_x_explicit() {
    let spec = json!({
        "data": {"values": [
            {"q": "Low", "v": 1.0},
            {"q": "High", "v": 3.0},
            {"q": "Mid", "v": 2.0}
        ]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal", "sort": ["Low", "Mid", "High"]},
            "y": {"field": "v", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    let low = svg.find(">Low<").unwrap();
    let mid = svg.find(">Mid<").unwrap();
    let high = svg.find(">High<").unwrap();
    assert!(low < mid && mid < high, "explicit sort order respected");
}

#[test]
fn quantitative_y_axis_temporal_x() {
    // x temporal, y quantitative — the canonical line chart.
    let spec = json!({
        "data": {"values": [
            {"t": "2020-01-01", "v": 3.0},
            {"t": "2020-02-01", "v": 9.0}
        ]},
        "mark": "line",
        "encoding": {
            "x": {"field": "t", "type": "temporal"},
            "y": {"field": "v", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<polyline"), 1);
}

// ---------------------------------------------------------------------------
// Scales
// ---------------------------------------------------------------------------

#[test]
fn log_scale_y() {
    let spec = json!({
        "data": {"values": [
            {"x": 1.0, "y": 1.0},
            {"x": 2.0, "y": 100.0},
            {"x": 3.0, "y": 10000.0}
        ]},
        "mark": "point",
        "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative", "scale": {"type": "log"}}
        }
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<circle"), 3);
    // log axis renders decade labels
    assert!(svg.contains(">100<") || svg.contains(">1000<"));
}

#[test]
fn sqrt_scale_y_renders() {
    let spec = json!({
        "data": {"values": [{"x": 1.0, "y": 4.0}, {"x": 2.0, "y": 16.0}]},
        "mark": "point",
        "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative", "scale": {"type": "sqrt"}}
        }
    });
    renders_png(&spec);
}

#[test]
fn color_scheme_category10_used() {
    let spec = json!({
        "data": {"values": [
            {"q": "Q1", "cat": "A", "v": 1.0},
            {"q": "Q1", "cat": "B", "v": 2.0}
        ]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative", "stack": null},
            "color": {"field": "cat", "type": "nominal", "scale": {"scheme": "category10"}}
        }
    });
    let svg = renders_png(&spec);
    assert!(svg.contains("#1f77b4"), "category10 first colour");
}

#[test]
fn explicit_color_range_used() {
    let spec = json!({
        "data": {"values": [
            {"q": "Q1", "cat": "A", "v": 1.0},
            {"q": "Q1", "cat": "B", "v": 2.0}
        ]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative", "stack": null},
            "color": {"field": "cat", "type": "nominal", "scale": {"range": ["#ff0000", "#00ff00"]}}
        }
    });
    let svg = renders_png(&spec);
    assert!(
        svg.contains("#ff0000") && svg.contains("#00ff00"),
        "explicit range honoured"
    );
}

#[test]
fn continuous_color_gradient_legend() {
    let spec = json!({
        "data": {"values": [
            {"x": 1.0, "y": 1.0, "z": 0.0},
            {"x": 2.0, "y": 2.0, "z": 50.0},
            {"x": 3.0, "y": 3.0, "z": 100.0}
        ]},
        "mark": "point",
        "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative"},
            "color": {"field": "z", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    // gradient legend → many thin rects + numeric domain labels
    assert!(count(&svg, "<rect") > 20, "gradient bar of stacked rects");
    assert!(svg.contains(">100<") || svg.contains(">0<"));
}

// ---------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------

#[test]
fn aggregate_transform_sum_groupby() {
    let spec = json!({
        "data": {"values": [
            {"cat": "A", "v": 1.0},
            {"cat": "A", "v": 2.0},
            {"cat": "B", "v": 10.0}
        ]},
        "transform": [
            {"aggregate": [{"op": "sum", "field": "v", "as": "total"}], "groupby": ["cat"]}
        ],
        "mark": "bar",
        "encoding": {
            "x": {"field": "cat", "type": "nominal"},
            "y": {"field": "total", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    // 2 grouped categories → 2 bars + background = 3 rects
    assert_eq!(count(&svg, "<rect"), 3);
}

#[test]
fn filter_transform_oneof() {
    let spec = json!({
        "data": {"values": [
            {"cat": "A", "v": 1.0},
            {"cat": "B", "v": 2.0},
            {"cat": "C", "v": 3.0}
        ]},
        "transform": [{"filter": {"field": "cat", "oneOf": ["A", "C"]}}],
        "mark": "bar",
        "encoding": {
            "x": {"field": "cat", "type": "nominal"},
            "y": {"field": "v", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert!(svg.contains(">A<") && svg.contains(">C<"));
    assert!(!svg.contains(">B<"), "B filtered out");
}

#[test]
fn filter_transform_range() {
    let spec = json!({
        "data": {"values": [
            {"cat": "A", "v": 1.0},
            {"cat": "B", "v": 5.0},
            {"cat": "C", "v": 9.0}
        ]},
        "transform": [{"filter": {"field": "v", "range": [4.0, 10.0]}}],
        "mark": "bar",
        "encoding": {
            "x": {"field": "cat", "type": "nominal"},
            "y": {"field": "v", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert!(!svg.contains(">A<"), "A (v=1) filtered out by range");
    assert!(svg.contains(">B<") && svg.contains(">C<"));
}

#[test]
fn bin_encoding_buckets_x() {
    let spec = json!({
        "data": {"values": [
            {"x": 1.0}, {"x": 2.0}, {"x": 3.0}, {"x": 11.0}, {"x": 12.0}
        ]},
        "transform": [{"aggregate": [{"op": "count", "as": "n"}], "groupby": ["x"]}],
        "mark": "bar",
        "encoding": {
            "x": {"field": "x", "type": "quantitative", "bin": true},
            "y": {"field": "n", "type": "quantitative"}
        }
    });
    // bin then count — must still render a valid PNG with bars.
    let svg = renders_png(&spec);
    assert!(count(&svg, "<rect") >= 2);
}

#[test]
fn fold_transform_unpivots() {
    let spec = json!({
        "data": {"values": [
            {"t": "Q1", "a": 1.0, "b": 2.0},
            {"t": "Q2", "a": 3.0, "b": 4.0}
        ]},
        "transform": [{"fold": ["a", "b"], "as": ["k", "val"]}],
        "mark": "bar",
        "encoding": {
            "x": {"field": "t", "type": "ordinal"},
            "y": {"field": "val", "type": "quantitative", "stack": null},
            "color": {"field": "k", "type": "nominal"}
        }
    });
    let svg = renders_png(&spec);
    // folded into series a & b → legend entries
    assert!(svg.contains(">a<") && svg.contains(">b<"));
}

#[test]
fn mean_aggregate_via_encoding() {
    let spec = json!({
        "data": {"values": [
            {"cat": "A", "v": 2.0},
            {"cat": "A", "v": 4.0}
        ]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "cat", "type": "nominal"},
            "y": {"field": "v", "type": "quantitative", "aggregate": "mean"}
        }
    });
    // mean of [2,4] = 3 → bar should top near 3, renders fine.
    renders_png(&spec);
}

// ---------------------------------------------------------------------------
// Composition
// ---------------------------------------------------------------------------

#[test]
fn layer_overlays_two_marks() {
    let spec = json!({
        "data": {"values": [
            {"t": "1997-01-01", "v": 10.0},
            {"t": "1997-02-01", "v": 20.0}
        ]},
        "encoding": {
            "x": {"field": "t", "type": "temporal"},
            "y": {"field": "v", "type": "quantitative"}
        },
        "layer": [
            {"mark": "line"},
            {"mark": "point"}
        ]
    });
    let svg = renders_png(&spec);
    assert_eq!(count(&svg, "<polyline"), 1, "line layer");
    assert!(count(&svg, "<circle") >= 2, "point layer");
}

#[test]
fn hconcat_places_two_charts() {
    let one = json!({
        "data": {"values": [{"q": "Q1", "v": 1.0}]},
        "mark": "bar",
        "encoding": {"x": {"field": "q", "type": "ordinal"}, "y": {"field": "v", "type": "quantitative"}}
    });
    let spec = json!({ "hconcat": [one.clone(), one] });
    let svg = renders_png(&spec);
    // two unit views → two background rects translated apart
    assert!(svg.contains("translate("));
}

#[test]
fn vconcat_stacks_two_charts() {
    let one = json!({
        "data": {"values": [{"q": "Q1", "v": 1.0}]},
        "mark": "bar",
        "encoding": {"x": {"field": "q", "type": "ordinal"}, "y": {"field": "v", "type": "quantitative"}}
    });
    let spec = json!({ "vconcat": [one.clone(), one] });
    renders_png(&spec);
}

#[test]
fn facet_column_small_multiples() {
    let spec = json!({
        "data": {"values": [
            {"g": "G1", "q": "Q1", "v": 1.0},
            {"g": "G1", "q": "Q2", "v": 2.0},
            {"g": "G2", "q": "Q1", "v": 3.0},
            {"g": "G2", "q": "Q2", "v": 4.0}
        ]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative"},
            "column": {"field": "g", "type": "nominal"}
        }
    });
    let svg = renders_png(&spec);
    assert!(
        svg.contains("g = G1") && svg.contains("g = G2"),
        "one panel per facet value"
    );
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn title_is_rendered() {
    let spec = json!({
        "title": "My Chart",
        "data": {"values": [{"q": "Q1", "v": 1.0}]},
        "mark": "bar",
        "encoding": {"x": {"field": "q", "type": "ordinal"}, "y": {"field": "v", "type": "quantitative"}}
    });
    let svg = renders_png(&spec);
    assert!(svg.contains("My Chart"));
}

#[test]
fn width_height_config_changes_canvas() {
    let spec = json!({
        "width": 300,
        "height": 200,
        "data": {"values": [{"q": "Q1", "v": 1.0}]},
        "mark": "bar",
        "encoding": {"x": {"field": "q", "type": "ordinal"}, "y": {"field": "v", "type": "quantitative"}}
    });
    let svg = render_vega_to_svg(&spec).expect("svg");
    // plot is 300 wide + margins; canvas width must reflect the override.
    assert!(
        svg.contains("width=\"388\""),
        "300 plot + 64 left + 24 right"
    );
}

#[test]
fn label_angle_rotates_x_labels() {
    let spec = json!({
        "data": {"values": [{"q": "LongLabel", "v": 1.0}]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal", "axis": {"labelAngle": -45}},
            "y": {"field": "v", "type": "quantitative"}
        }
    });
    let svg = renders_png(&spec);
    assert!(svg.contains("rotate(-45"), "x labels rotated");
}

#[test]
fn crowded_x_labels_auto_rotate() {
    // Many wide categorical labels can't fit flat in their bands — Vega-Lite
    // auto-rotates them rather than overplotting. With 12 "1997-NN" months in a
    // default-width chart, the flat labels collide, so we expect rotation even
    // though the spec sets no labelAngle (regression: the demo's stacked bar).
    let values: Vec<_> = (1..=12)
        .map(|m| json!({"month": format!("1997-{m:02}"), "category": "Beverages", "revenue": (m * 100) as f64}))
        .collect();
    let spec = json!({
        "mark": "bar",
        "encoding": {
            "x": {"field": "month", "type": "ordinal", "title": "Month"},
            "y": {"field": "revenue", "type": "quantitative"},
            "color": {"field": "category", "type": "nominal"}
        },
        "data": {"values": values}
    });
    let svg = render_vega_to_svg(&spec).expect("svg");
    // The y-axis title is always rotate(-90); the x labels rotating is the
    // signal — so look for a rotate that is NOT the -90 title, on a month text.
    let x_rotations = svg.matches("rotate(").count() - svg.matches("rotate(-90").count();
    assert!(
        x_rotations >= 12,
        "all 12 crowded month labels should auto-rotate, found {x_rotations} non-title rotations:\n{svg}"
    );
}

#[test]
fn sparse_x_labels_stay_flat() {
    // A handful of short labels fit comfortably — no gratuitous rotation.
    let spec = json!({
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative"}
        },
        "data": {"values": [
            {"q": "Q1", "v": 1.0}, {"q": "Q2", "v": 2.0},
            {"q": "Q3", "v": 3.0}, {"q": "Q4", "v": 4.0}
        ]}
    });
    let svg = render_vega_to_svg(&spec).expect("svg");
    let x_rotations = svg.matches("rotate(").count() - svg.matches("rotate(-90").count();
    assert_eq!(
        x_rotations, 0,
        "few short labels should stay flat (only the y-title may rotate -90):\n{svg}"
    );
}

#[test]
fn axis_format_percent() {
    let spec = json!({
        "data": {"values": [{"q": "Q1", "v": 0.5}]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative", "axis": {"format": "%"}}
        }
    });
    let svg = renders_png(&spec);
    assert!(svg.contains("%"), "percent-formatted y axis labels");
}

#[test]
fn legend_title_from_color_field() {
    let spec = json!({
        "data": {"values": [
            {"q": "Q1", "cat": "A", "v": 1.0},
            {"q": "Q1", "cat": "B", "v": 2.0}
        ]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative", "stack": null},
            "color": {"field": "cat", "type": "nominal", "title": "Category"}
        }
    });
    let svg = renders_png(&spec);
    assert!(svg.contains("Category"), "legend title rendered");
}

// ---------------------------------------------------------------------------
// Guardrail (must still reject)
// ---------------------------------------------------------------------------

#[test]
fn guardrail_rejects_data_url() {
    let spec = json!({
        "data": {"url": "https://evil.example/data.json"},
        "mark": "bar",
        "encoding": {"x": {"field": "q", "type": "ordinal"}, "y": {"field": "v", "type": "quantitative"}}
    });
    assert!(
        render_vega_to_svg(&spec).is_err(),
        "data.url must be rejected"
    );
}

#[test]
fn guardrail_rejects_calculate_transform() {
    let spec = json!({
        "data": {"values": [{"q": "Q1", "v": 1.0}]},
        "transform": [{"calculate": "datum.v * 2", "as": "doubled"}],
        "mark": "bar",
        "encoding": {"x": {"field": "q", "type": "ordinal"}, "y": {"field": "doubled", "type": "quantitative"}}
    });
    assert!(
        render_vega_to_svg(&spec).is_err(),
        "calculate must be rejected"
    );
}

#[test]
fn guardrail_rejects_expr_and_signal() {
    let expr = json!({
        "data": {"values": [{"q": "Q1", "v": 1.0}]},
        "mark": "bar",
        "encoding": {
            "x": {"field": "q", "type": "ordinal"},
            "y": {"field": "v", "type": "quantitative"},
            "color": {"value": {"expr": "1+1"}}
        }
    });
    assert!(render_vega_to_svg(&expr).is_err(), "expr must be rejected");

    let signal = json!({
        "data": {"values": [{"q": "Q1", "v": 1.0}]},
        "signals": [{"name": "x", "value": 1}],
        "mark": "bar",
        "encoding": {"x": {"field": "q", "type": "ordinal"}, "y": {"field": "v", "type": "quantitative"}}
    });
    assert!(
        render_vega_to_svg(&signal).is_err(),
        "signals must be rejected"
    );
}

#[test]
fn unsupported_mark_still_errors() {
    let spec = json!({
        "data": {"values": [{"q": "Q1", "v": 1.0}]},
        "mark": "geoshape",
        "encoding": {"x": {"field": "q", "type": "ordinal"}, "y": {"field": "v", "type": "quantitative"}}
    });
    assert!(render_vega_to_svg(&spec).is_err());
}
