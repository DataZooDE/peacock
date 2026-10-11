use peacock_rasterizer::{render_vega_to_png, render_vega_to_svg};
use serde_json::{Value, json};

fn spec(legend: Option<Value>, labels: &[&str]) -> Value {
    let mut color = json!({"field": "series", "type": "nominal"});
    if let Some(legend) = legend {
        color["legend"] = legend;
    }
    json!({
        "data": {"values": labels.iter().map(|label| json!({"x": "Q1", "y": 1, "series": label})).collect::<Vec<_>>()},
        "mark": "line",
        "encoding": {
            "x": {"field": "x", "type": "ordinal"},
            "y": {"field": "y", "type": "quantitative"},
            "color": color
        }
    })
}

#[test]
fn default_right_legend_keeps_title_and_entries() {
    let svg = render_vega_to_svg(&spec(None, &["A", "B"])).unwrap();
    assert!(svg.contains(">series</text>"));
    assert!(svg.contains(">A</text>"));
    assert!(svg.contains(">B</text>"));
}

#[test]
fn long_right_labels_fit_and_label_limit_ellipsizes() {
    let long = "Cumulative expected demand";
    let svg = render_vega_to_svg(&spec(
        Some(json!({"orient": "right", "labelLimit": 0})),
        &[long],
    ))
    .unwrap();
    assert!(svg.contains(&format!(">{long}</text>")));
    assert!(
        svg.contains("width=\"680"),
        "outer width stays bounded: {svg}"
    );
    let limited = render_vega_to_svg(&spec(Some(json!({"labelLimit": 60})), &[long])).unwrap();
    assert!(limited.contains('…'), "limited label has an ellipsis");
    assert!(!limited.contains(&format!(">{long}</text>")));
    let capped = render_vega_to_svg(&spec(
        None,
        &["A very long series name that cannot fit in forty percent of the canvas width"],
    ))
    .unwrap();
    assert!(capped.contains('…'), "bounded right legend ellipsizes");
}

#[test]
fn bottom_legend_uses_full_plot_width_and_hides_null_title() {
    let svg = render_vega_to_svg(&spec(
        Some(json!({"orient": "bottom", "title": null, "labelLimit": 0})),
        &["Cumulative expected demand", "Last-time-buy quantity"],
    ))
    .unwrap();
    assert!(svg.contains("Cumulative expected demand"));
    assert!(svg.contains("Last-time-buy quantity"));
    assert!(!svg.contains(">series</text>"));
    assert!(svg.contains("width=\"680"));
    assert!(svg.contains("x2=\"656.0\""), "plot gets full width: {svg}");
    assert!(svg.contains("height=\""));
    assert!(
        render_vega_to_png(
            &spec(
                Some(json!({"orient": "bottom", "title": null, "labelLimit": 0})),
                &["Cumulative expected demand", "Last-time-buy quantity"]
            ),
            1.0
        )
        .is_ok()
    );
}

#[test]
fn none_hides_legend_and_unsupported_orient_falls_back_right() {
    let hidden = render_vega_to_svg(&spec(Some(json!({"orient": "none"})), &["A"])).unwrap();
    assert!(!hidden.contains(">series</text>"));
    let fallback = render_vega_to_svg(&spec(Some(json!({"orient": "top"})), &["A"])).unwrap();
    assert!(fallback.contains(">series</text>"));
}

#[test]
fn bottom_entries_wrap_and_arc_uses_same_options() {
    let svg = render_vega_to_svg(&spec(
        Some(json!({"orient": "bottom", "title": null})),
        &[
            "A long first series name",
            "A long second series name",
            "A long third series name",
            "A long fourth series name",
        ],
    ))
    .unwrap();
    assert!(svg.contains("A long fourth series name"));
    assert!(svg.contains("height=\""));

    let arc = json!({
        "data": {"values": [{"category": "Cumulative expected demand", "value": 2}, {"category": "Last-time-buy quantity", "value": 3}]},
        "mark": "arc",
        "encoding": {
            "theta": {"field": "value", "type": "quantitative"},
            "color": {"field": "category", "type": "nominal", "legend": {"orient": "bottom", "title": null, "labelLimit": 0}}
        }
    });
    let svg = render_vega_to_svg(&arc).unwrap();
    assert!(svg.contains("Cumulative expected demand"));
    assert!(svg.contains("Last-time-buy quantity"));
    assert!(!svg.contains(">category</text>"));
    assert!(render_vega_to_png(&arc, 1.0).is_ok());
}
