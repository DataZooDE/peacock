//! Apply a [`ThemeTokens`] to a rendered SVG by substituting peacock's known
//! default tokens (colours/font) with the theme's. The renderer emits a fixed,
//! documented set of literals, so this is deterministic — and it keeps the
//! large Vega module free of theme plumbing. (resvg has no CSS `var()` support,
//! so post-substitution is the pragmatic seam for the rasterized PNG; the web
//! surfaces consume the same theme as real CSS.)

use peacock_theme::ThemeTokens;

use crate::RasterError;
use crate::dashboard::DashboardRequest;

/// peacock's stock categorical palette (Tableau10) — the literals the chart
/// renderer emits, remapped index-for-index onto the theme palette.
const DEFAULT_PALETTE: &[&str] = &[
    "#4c78a8", "#f58518", "#54a24b", "#e45756", "#72b7b2", "#ff9da6", "#9d755d", "#bab0ac",
    "#e377c2", "#17becf",
];

fn replace_quoted(s: &mut String, from: &str, to: &str) {
    if from != to {
        *s = s.replace(&format!("\"{from}\""), &format!("\"{to}\""));
    }
}

/// SVG-safe font-family (the theme value may contain double quotes; the SVG
/// attribute is itself double-quoted, so swap to single quotes) with the
/// vendored face as a guaranteed fallback for the rasterizer.
fn font_attr(t: &ThemeTokens) -> String {
    let f = t.font.replace('"', "'");
    format!("font-family=\"{f}, 'DejaVu Sans', sans-serif\"")
}

fn apply_palette(s: &mut String, t: &ThemeTokens) {
    for (i, def) in DEFAULT_PALETTE.iter().enumerate() {
        if let Some(themed) = t.palette.get(i) {
            replace_quoted(s, def, themed);
        }
    }
}

/// Restyle a **chart** SVG with the theme.
pub fn apply_chart_theme(svg: &str, t: &ThemeTokens) -> String {
    let mut s = svg.replace("font-family=\"DejaVu Sans, sans-serif\"", &font_attr(t));
    apply_palette(&mut s, t);
    // Structural tokens the chart renderer emits.
    replace_quoted(&mut s, "#ffffff", &t.bg); // plot / title background
    replace_quoted(&mut s, "#e6e6e6", &t.grid); // gridlines
    replace_quoted(&mut s, "#888", &t.axis); // axis lines
    replace_quoted(&mut s, "#444", &t.muted); // tick / minor labels
    replace_quoted(&mut s, "#222", &t.text); // titles / strong text
    replace_quoted(&mut s, "#333", &t.text); // legend / facet headers
    s
}

/// Restyle a **dashboard** SVG with the theme (different default→token map: the
/// dashboard's page bg is `#faf9f8` and its cards are `#ffffff`).
pub fn apply_dashboard_theme(svg: &str, t: &ThemeTokens) -> String {
    let mut s = svg.replace("font-family=\"DejaVu Sans, sans-serif\"", &font_attr(t));
    replace_quoted(&mut s, "#faf9f8", &t.bg); // page background
    replace_quoted(&mut s, "#ffffff", &t.surface); // tile/card surface
    replace_quoted(&mut s, "#e1dfdd", &t.border); // card borders
    replace_quoted(&mut s, "#201f1e", &t.text); // title text
    replace_quoted(&mut s, "#605e5c", &t.muted); // tile labels
    replace_quoted(&mut s, "#0f6cbd", &t.brand); // tile values (brand accent)
    s
}

// ── themed public entrypoints ───────────────────────────────────────────────

/// Render a Vega-Lite chart to SVG, restyled with `theme`.
pub fn render_vega_to_svg_themed(
    spec: &serde_json::Value,
    theme: &ThemeTokens,
) -> Result<String, RasterError> {
    Ok(apply_chart_theme(&crate::vegalite_to_svg(spec)?, theme))
}

/// Render a Vega-Lite chart to PNG, restyled with `theme`.
pub fn render_vega_to_png_themed(
    spec: &serde_json::Value,
    scale: f32,
    theme: &ThemeTokens,
) -> Result<Vec<u8>, RasterError> {
    crate::render_svg_to_png(&render_vega_to_svg_themed(spec, theme)?, scale)
}

/// Render a dashboard to PNG, restyled with `theme`.
pub fn render_dashboard_to_png_themed(
    req: &DashboardRequest,
    scale: f32,
    theme: &ThemeTokens,
) -> Result<Vec<u8>, RasterError> {
    let svg = apply_dashboard_theme(&crate::dashboard::render_dashboard_to_svg(req), theme);
    crate::render_svg_to_png(&svg, scale)
}
