//! Compile peacock's guardrail-restricted **Vega-Lite subset** to SVG, in
//! pure Rust (no Deno, no Node, no network).
//!
//! Pipeline: guardrail check → resolve composition (`layer` / `facet` /
//! `row`+`column` / `hconcat` / `vconcat`) → for each unit view: apply
//! transforms (filter/fold/aggregate/bin) → derive scales → emit marks, axes
//! and legend. Anything outside the subset is a `RasterError`.

mod axes;
mod data;
mod layout;
mod legend;
mod marks;
mod parse;
mod scales;
mod svgutil;
mod transforms;
mod unit;

use std::fmt::Write as _;

use serde_json::Value;

use crate::RasterError;

/// Set the per-thread brand sequential ramp used by the themed render path.
pub(crate) use scales::set_sequential_override;

/// Outer canvas defaults.
pub(crate) const DEFAULT_W: f64 = 680.0;
pub(crate) const DEFAULT_H: f64 = 420.0;

/// Compile a safe-subset Vega-Lite spec into a standalone SVG document string.
pub fn vegalite_to_svg(spec: &Value) -> Result<String, RasterError> {
    parse::check_guardrail(spec)?;

    // Render the composition into a body + measured size, then wrap once.
    let rendered = layout::render_view(spec, None)?;

    let title = spec.get("title").and_then(title_text).map(str::to_owned);
    let title_h = if title.is_some() { 30.0 } else { 0.0 };

    let w = rendered.width;
    let h = rendered.height + title_h;

    let mut svg = String::new();
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="DejaVu Sans, sans-serif">"##
    );
    // Title strip background only — each view paints its own plot background, so
    // a single-unit chart has exactly one background rect (deterministic counts).
    if title.is_some() {
        let _ = write!(
            svg,
            r##"<rect width="{w}" height="{title_h}" fill="#ffffff"/>"##
        );
    }
    if let Some(t) = &title {
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="20" font-size="15" font-weight="bold" text-anchor="middle" fill="#222">{}</text>"##,
            w / 2.0,
            svgutil::escape(t)
        );
    }
    let _ = write!(svg, r##"<g transform="translate(0,{title_h})">"##);
    svg.push_str(&rendered.body);
    svg.push_str("</g>");
    svg.push_str("</svg>");
    Ok(svg)
}

fn title_text(v: &Value) -> Option<&str> {
    match v {
        Value::String(s) => Some(s.as_str()),
        Value::Object(o) => o.get("text").and_then(Value::as_str),
        _ => None,
    }
}
