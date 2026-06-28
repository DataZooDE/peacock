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

/// Render a Vega-Lite chart to SVG, restyled with `theme`. Heatmaps / continuous
/// colour pick up a brand-derived sequential ramp (corporate hue).
pub fn render_vega_to_svg_themed(
    spec: &serde_json::Value,
    theme: &ThemeTokens,
) -> Result<String, RasterError> {
    crate::vegalite::set_sequential_override(brand_ramp(&theme.brand));
    let svg = crate::vegalite_to_svg(spec);
    crate::vegalite::set_sequential_override(None); // always clear
    Ok(apply_chart_theme(&svg?, theme))
}

/// A single-hue sequential ramp derived from the brand colour: a near-white
/// tint → the brand → a darkened brand. `None` for an unparseable colour (the
/// named scheme is kept).
fn brand_ramp(brand: &str) -> Option<Vec<(u8, u8, u8)>> {
    let c = parse_hex(brand)?;
    let mix = |a: (u8, u8, u8), b: (u8, u8, u8), f: f64| -> (u8, u8, u8) {
        let m = |x: u8, y: u8| (x as f64 * (1.0 - f) + y as f64 * f).round() as u8;
        (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
    };
    let white = (255, 255, 255);
    let black = (0, 0, 0);
    Some(vec![
        mix(c, white, 0.88),
        mix(c, white, 0.35),
        c,
        mix(c, black, 0.35),
    ])
}

fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().strip_prefix('#')?;
    let full = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => h.to_string(),
        _ => return None,
    };
    let r = u8::from_str_radix(&full[0..2], 16).ok()?;
    let g = u8::from_str_radix(&full[2..4], 16).ok()?;
    let b = u8::from_str_radix(&full[4..6], 16).ok()?;
    Some((r, g, b))
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
