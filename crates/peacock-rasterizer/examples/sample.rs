//! Render the Northwind revenue chart to a PNG for visual inspection:
//! `cargo run -p peacock-rasterizer --example sample -- <out.png>`

use serde_json::json;

fn main() {
    let spec = json!({
        "data": {"values": [
            {"month":"1997-01-01","category":"Beverages","revenue":180.0},
            {"month":"1997-02-01","category":"Beverages","revenue":81.0},
            {"month":"1997-04-01","category":"Beverages","revenue":216.0},
            {"month":"1997-07-01","category":"Beverages","revenue":144.0},
            {"month":"1997-10-01","category":"Beverages","revenue":180.0},
            {"month":"1997-01-01","category":"Condiments","revenue":110.0},
            {"month":"1997-05-01","category":"Condiments","revenue":198.0},
            {"month":"1997-09-01","category":"Condiments","revenue":88.0},
            {"month":"1997-02-01","category":"Dairy Products","revenue":340.0},
            {"month":"1997-06-01","category":"Dairy Products","revenue":170.0},
            {"month":"1997-11-01","category":"Dairy Products","revenue":204.0}
        ]},
        "mark":"line",
        "encoding":{
            "x":{"field":"month","type":"temporal","title":"Month"},
            "y":{"field":"revenue","type":"quantitative","aggregate":"sum","title":"Revenue"},
            "color":{"field":"category","type":"nominal"}
        }
    });
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "nw_chart.png".into());
    let png = peacock_rasterizer::render_vega_to_png(&spec, 2.0).unwrap();
    std::fs::write(&out, png).unwrap();
    println!("wrote {out}");
}
