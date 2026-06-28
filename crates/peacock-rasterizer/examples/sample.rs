//! Render a gallery of Vega-Lite subset charts to PNGs for visual inspection:
//! `cargo run -p peacock-rasterizer --example sample -- <out_dir>`
//! (defaults to the current directory). Each chart is written as
//! `vega_<name>.png`.

use serde_json::{Value, json};

fn gallery() -> Vec<(&'static str, Value)> {
    vec![
        (
            "line",
            json!({
                "data": {"values": [
                    {"month":"1997-01-01","category":"Beverages","revenue":180.0},
                    {"month":"1997-02-01","category":"Beverages","revenue":81.0},
                    {"month":"1997-04-01","category":"Beverages","revenue":216.0},
                    {"month":"1997-07-01","category":"Beverages","revenue":144.0},
                    {"month":"1997-01-01","category":"Condiments","revenue":110.0},
                    {"month":"1997-05-01","category":"Condiments","revenue":198.0},
                    {"month":"1997-02-01","category":"Dairy","revenue":340.0},
                    {"month":"1997-06-01","category":"Dairy","revenue":170.0}
                ]},
                "title": "Monthly Revenue by Category",
                "mark":"line",
                "encoding":{
                    "x":{"field":"month","type":"temporal","title":"Month"},
                    "y":{"field":"revenue","type":"quantitative","aggregate":"sum","title":"Revenue"},
                    "color":{"field":"category","type":"nominal","title":"Category"}
                }
            }),
        ),
        (
            "stacked_bar",
            json!({
                "data": {"values": [
                    {"q":"Q1","cat":"A","v":10.0},{"q":"Q1","cat":"B","v":5.0},{"q":"Q1","cat":"C","v":7.0},
                    {"q":"Q2","cat":"A","v":8.0},{"q":"Q2","cat":"B","v":12.0},{"q":"Q2","cat":"C","v":3.0},
                    {"q":"Q3","cat":"A","v":14.0},{"q":"Q3","cat":"B","v":6.0},{"q":"Q3","cat":"C","v":9.0}
                ]},
                "title": "Stacked Bar",
                "mark":"bar",
                "encoding":{
                    "x":{"field":"q","type":"ordinal"},
                    "y":{"field":"v","type":"quantitative","stack":"zero"},
                    "color":{"field":"cat","type":"nominal"}
                }
            }),
        ),
        (
            "grouped_bar",
            json!({
                "data": {"values": [
                    {"q":"Q1","cat":"A","v":10.0},{"q":"Q1","cat":"B","v":5.0},
                    {"q":"Q2","cat":"A","v":8.0},{"q":"Q2","cat":"B","v":12.0}
                ]},
                "title": "Grouped Bar",
                "mark":"bar",
                "encoding":{
                    "x":{"field":"q","type":"ordinal"},
                    "y":{"field":"v","type":"quantitative","stack":null},
                    "color":{"field":"cat","type":"nominal","scale":{"scheme":"category10"}}
                }
            }),
        ),
        (
            "scatter",
            json!({
                "data": {"values": [
                    {"x":1.0,"y":2.0,"s":10.0,"g":"A"},
                    {"x":3.0,"y":5.0,"s":80.0,"g":"A"},
                    {"x":5.0,"y":3.0,"s":40.0,"g":"B"},
                    {"x":7.0,"y":8.0,"s":120.0,"g":"B"}
                ]},
                "title": "Bubble Scatter",
                "mark":"point",
                "encoding":{
                    "x":{"field":"x","type":"quantitative"},
                    "y":{"field":"y","type":"quantitative"},
                    "size":{"field":"s","type":"quantitative"},
                    "color":{"field":"g","type":"nominal"}
                }
            }),
        ),
        (
            "pie",
            json!({
                "data": {"values": [
                    {"cat":"A","v":30.0},{"cat":"B","v":50.0},{"cat":"C","v":20.0}
                ]},
                "title": "Pie",
                "mark":"arc",
                "encoding":{
                    "theta":{"field":"v","type":"quantitative"},
                    "color":{"field":"cat","type":"nominal"}
                }
            }),
        ),
        (
            "donut",
            json!({
                "data": {"values": [
                    {"cat":"A","v":30.0},{"cat":"B","v":50.0},{"cat":"C","v":20.0}
                ]},
                "title": "Donut",
                "mark":{"type":"arc","innerRadius":70.0},
                "encoding":{
                    "theta":{"field":"v","type":"quantitative"},
                    "color":{"field":"cat","type":"nominal"}
                }
            }),
        ),
        (
            "heatmap",
            json!({
                "data": {"values": [
                    {"r":"Mon","c":"AM","v":1.0},{"r":"Mon","c":"PM","v":4.0},
                    {"r":"Tue","c":"AM","v":2.0},{"r":"Tue","c":"PM","v":8.0},
                    {"r":"Wed","c":"AM","v":6.0},{"r":"Wed","c":"PM","v":3.0}
                ]},
                "title": "Heatmap",
                "mark":"rect",
                "encoding":{
                    "x":{"field":"c","type":"ordinal"},
                    "y":{"field":"r","type":"ordinal"},
                    "color":{"field":"v","type":"quantitative"}
                }
            }),
        ),
        (
            "stacked_area",
            json!({
                "data": {"values": [
                    {"t":"2020-01-01","cat":"A","v":10.0},{"t":"2020-02-01","cat":"A","v":20.0},{"t":"2020-03-01","cat":"A","v":15.0},
                    {"t":"2020-01-01","cat":"B","v":5.0},{"t":"2020-02-01","cat":"B","v":8.0},{"t":"2020-03-01","cat":"B","v":18.0}
                ]},
                "title": "Stacked Area",
                "mark":"area",
                "encoding":{
                    "x":{"field":"t","type":"temporal"},
                    "y":{"field":"v","type":"quantitative"},
                    "color":{"field":"cat","type":"nominal"}
                }
            }),
        ),
        (
            "layer",
            json!({
                "data": {"values": [
                    {"t":"2020-01-01","v":10.0},{"t":"2020-02-01","v":20.0},{"t":"2020-03-01","v":15.0}
                ]},
                "title": "Line + Points (layer)",
                "encoding":{
                    "x":{"field":"t","type":"temporal"},
                    "y":{"field":"v","type":"quantitative"}
                },
                "layer":[{"mark":"line"},{"mark":"point"}]
            }),
        ),
        (
            "facet",
            json!({
                "data": {"values": [
                    {"g":"North","q":"Q1","v":3.0},{"g":"North","q":"Q2","v":6.0},
                    {"g":"South","q":"Q1","v":5.0},{"g":"South","q":"Q2","v":2.0}
                ]},
                "mark":"bar",
                "encoding":{
                    "x":{"field":"q","type":"ordinal"},
                    "y":{"field":"v","type":"quantitative"},
                    "column":{"field":"g","type":"nominal"}
                }
            }),
        ),
    ]
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    for (name, spec) in gallery() {
        let png = peacock_rasterizer::render_vega_to_png(&spec, 2.0)
            .unwrap_or_else(|e| panic!("render {name}: {e}"));
        let out = format!("{dir}/vega_{name}.png");
        std::fs::write(&out, png).unwrap();
        println!("wrote {out}");
    }
}
