//! `peacock-rasterizer` — peacock's Vega-Lite → SVG/PNG renderer (FR-V-2,
//! FR-C-2), the `render_a2ui_to_png` capability Triton's chat surface
//! delegates to.
//!
//! Pure Rust, no Node, no Deno, no network (NFR-S-5): peacock compiles its
//! guardrail-restricted Vega-Lite subset to SVG itself ([`vegalite_svg`]),
//! then rasterizes with `resvg`/`tiny-skia`. A permissively-licensed font is
//! vendored for deterministic, offline text. See the discovered note for why
//! this replaces vl-convert.

pub mod dashboard;
mod raster;
pub mod vegalite_svg;

pub use dashboard::{DashboardRequest, render_dashboard_to_png, render_dashboard_to_svg};
pub use raster::render_svg_to_png;
pub use vegalite_svg::vegalite_to_svg;

use serde_json::Value;

/// Rasterization / spec error.
#[derive(Debug, thiserror::Error)]
#[error("rasterization failed: {0}")]
pub struct RasterError(String);

impl RasterError {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        RasterError(msg.into())
    }
}

impl From<RasterError> for peacock_types::Error {
    fn from(e: RasterError) -> Self {
        peacock_types::Error::render(e.0)
    }
}

/// Render a safe-subset Vega-Lite spec (rows inline) to PNG bytes. `scale`
/// ≥ 1.0 controls resolution. This is `render_a2ui_to_png`.
pub fn render_vega_to_png(spec: &Value, scale: f32) -> Result<Vec<u8>, RasterError> {
    let svg = vegalite_to_svg(spec)?;
    render_svg_to_png(&svg, scale)
}

/// Render a safe-subset Vega-Lite spec to SVG text.
pub fn render_vega_to_svg(spec: &Value) -> Result<String, RasterError> {
    vegalite_to_svg(spec)
}
