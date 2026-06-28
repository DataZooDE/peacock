//! SVG → PNG via `resvg`/`usvg`/`tiny-skia`.
//!
//! Fonts: a shared font database is built once and reused. It always carries
//! the vendored DejaVu Sans/Serif (a deterministic, offline fallback, NFR-S-5)
//! and, unless `PEACOCK_NO_SYSTEM_FONTS` is set, the host's installed fonts —
//! so a **brand's real corporate font** renders in the chart when present. The
//! generic `serif`/`sans-serif` families map to the vendored faces, so a serif
//! brand theme visibly renders serif even with no matching family installed.

use std::sync::{Arc, OnceLock};

use crate::RasterError;

/// Vendored, permissively-licensed fallback faces (DejaVu Fonts License).
const SANS: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
const SERIF: &[u8] = include_bytes!("../assets/DejaVuSerif.ttf");

fn font_db() -> &'static Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_font_data(SANS.to_vec());
        db.load_font_data(SERIF.to_vec());
        // Installed fonts let a brand's real corporate font resolve in the
        // chart. Offline (no network); opt out for byte-reproducible renders.
        if std::env::var_os("PEACOCK_NO_SYSTEM_FONTS").is_none() {
            db.load_system_fonts();
        }
        // Generic-family fallbacks (a serif theme renders serif).
        db.set_sans_serif_family("DejaVu Sans");
        db.set_serif_family("DejaVu Serif");
        Arc::new(db)
    })
}

/// Rasterize an SVG document to PNG bytes at `scale` (≥ 1.0).
pub fn render_svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, RasterError> {
    let mut opt = usvg::Options {
        // The default family when an element names none; brand themes always
        // set `font-family`, so this is only a last resort.
        font_family: "DejaVu Sans".to_string(),
        ..Default::default()
    };
    opt.fontdb = font_db().clone();

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
