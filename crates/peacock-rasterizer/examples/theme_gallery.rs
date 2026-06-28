//! Render the same Northwind chart under several (brand × host) themes:
//! `cargo run -p peacock-rasterizer --example theme_gallery -- <out_dir>`

use peacock_rasterizer::{ThemeRegistry, render_vega_to_png_themed};
use serde_json::json;

fn main() {
    let spec = json!({
        "data": {"values": [
            {"month":"1997-01","category":"Beverages","revenue":180.0},
            {"month":"1997-02","category":"Beverages","revenue":81.0},
            {"month":"1997-04","category":"Beverages","revenue":216.0},
            {"month":"1997-01","category":"Condiments","revenue":110.0},
            {"month":"1997-05","category":"Condiments","revenue":198.0},
            {"month":"1997-02","category":"Dairy Products","revenue":340.0},
            {"month":"1997-06","category":"Dairy Products","revenue":170.0}
        ]},
        "mark":"bar",
        "encoding":{
            "x":{"field":"month","type":"ordinal","title":"Month"},
            "y":{"field":"revenue","type":"quantitative","aggregate":"sum","title":"Revenue"},
            "color":{"field":"category","type":"nominal"}
        }
    });
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let reg = ThemeRegistry::builtin();
    for (brand, host) in [
        ("company-a", "whatsapp"),
        ("company-a", "copilot"),
        ("company-b", "gemini"),
    ] {
        let t = reg.resolve(brand, host);
        let png = render_vega_to_png_themed(&spec, 2.0, &t.tokens).unwrap();
        let path = format!("{dir}/theme_{brand}_{host}.png");
        std::fs::write(&path, png).unwrap();
        println!("wrote {path}");
    }
}
