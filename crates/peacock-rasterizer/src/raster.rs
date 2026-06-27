//! SVG → PNG via `resvg`/`usvg`/`tiny-skia`, with a vendored font for
//! deterministic, offline text (NFR-S-5).

use crate::RasterError;

/// Vendored, permissively-licensed font (DejaVu Fonts License).
const FONT: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

/// Rasterize an SVG document to PNG bytes at `scale` (≥ 1.0).
pub fn render_svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, RasterError> {
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_font_data(FONT.to_vec());
    // Map the generic families used in our SVG to the vendored face.
    opt.font_family = "DejaVu Sans".to_string();

    let tree = usvg::Tree::from_str(svg, &opt)
        .map_err(|e| RasterError::new(format!("usvg parse: {e}")))?;

    let scale = scale.max(1.0);
    let size = tree.size();
    let w = (size.width() * scale).ceil().max(1.0) as u32;
    let h = (size.height() * scale).ceil().max(1.0) as u32;

    let mut pixmap = tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| RasterError::new("could not allocate pixmap"))?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    pixmap
        .encode_png()
        .map_err(|e| RasterError::new(format!("png encode: {e}")))
}
