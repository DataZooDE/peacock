//! `peacock-ggplot` — the pluggable STATISTICAL render backend (issue #6),
//! peer to `peacock-rasterizer`: `(spec, rows, theme, size) → PNG`.
//!
//! Wraps [`ggplot-rs`](https://github.com/sipemu/ggplot-rs) (Grammar of
//! Graphics over plotters): pure Rust, headless in-memory PNG, fonts bundled
//! — no polars (default features off), no R, no Node, no network (NFR-S-5).
//! peacock-core routes a spec here when its JSON carries a top-level `geom`
//! key (Vega-Lite uses `mark`, never `geom`); everything else stays on the
//! Vega-Lite rasterizer. This crate only compiles behind peacock-core's
//! `ggplot` cargo feature, so the base tree never pulls the plotters graph.
//!
//! This issue ships the `histogram` geom end-to-end; the other declared stat
//! geoms (`density`, `boxplot`, `ecdf`) error clearly until the stat-spec
//! dialect lands (issue #7).

use ggplot_rs::data::Value as GgValue;
use ggplot_rs::prelude::{Aes, ElementRect, GGPlot, GeomHistogram, theme_minimal};
use ggplot_rs::theme::Theme;
use peacock_theme::ThemeTokens;
use serde_json::Value;

/// Statistical-rendering / spec error.
#[derive(Debug, thiserror::Error)]
#[error("ggplot render failed: {0}")]
pub struct GgplotError(String);

impl GgplotError {
    fn new(msg: impl Into<String>) -> Self {
        GgplotError(msg.into())
    }
}

impl From<GgplotError> for peacock_types::Error {
    fn from(e: GgplotError) -> Self {
        peacock_types::Error::render(e.0)
    }
}

/// The natural (scale = 1.0) canvas, mirroring ggplot-rs's own default. As in
/// the Vega rasterizer, `scale` (clamped to ≥ 1.0) multiplies the pixel size.
const BASE_W: f32 = 800.0;
const BASE_H: f32 = 600.0;

/// Render a STATISTICAL spec (top-level `geom`) to PNG bytes, headless and
/// in-memory. `rows` is the inline data the composer injected at the spec's
/// `data.values` — an array of `{column: value}` objects. `theme` maps the
/// deployment's `--pk-*` tokens onto the chart; `None` renders the stock
/// look. `scale` ≥ 1.0 multiplies the 800×600 natural size (the same
/// semantics as `render_vega_to_png`).
pub fn render_stat_to_png(
    spec: &Value,
    rows: &Value,
    theme: Option<&ThemeTokens>,
    scale: f32,
) -> Result<Vec<u8>, GgplotError> {
    let geom = spec
        .get("geom")
        .and_then(Value::as_str)
        .ok_or_else(|| GgplotError::new("statistical spec has no `geom` key"))?;
    match geom {
        "histogram" => render_histogram(spec, rows, theme, scale),
        "density" | "boxplot" | "ecdf" => Err(GgplotError::new(format!(
            "geom `{geom}` is not yet implemented (issue #7); only `histogram` renders today"
        ))),
        other => Err(GgplotError::new(format!(
            "unknown statistical geom `{other}`"
        ))),
    }
}

fn render_histogram(
    spec: &Value,
    rows: &Value,
    theme: Option<&ThemeTokens>,
    scale: f32,
) -> Result<Vec<u8>, GgplotError> {
    let x = spec
        .get("x")
        .and_then(Value::as_str)
        .ok_or_else(|| GgplotError::new("histogram spec must name its `x` column"))?;
    let values = numeric_column(rows, x)?;
    let bins = spec
        .get("bins")
        .and_then(Value::as_u64)
        .map_or(30, |b| b.clamp(1, 1000) as usize);

    let data = vec![(
        x.to_owned(),
        values.into_iter().map(GgValue::Float).collect::<Vec<_>>(),
    )];

    let mut plot = GGPlot::new(data)
        .aes(Aes::new().x(x))
        .geom_histogram_with(GeomHistogram {
            bins,
            ..Default::default()
        })
        .xlab(x)
        .ylab("count")
        .theme(plot_theme(theme));
    if let Some(title) = spec.get("title").and_then(Value::as_str) {
        plot = plot.title(title);
    }

    let scale = scale.max(1.0);
    let w = (BASE_W * scale).ceil() as u32;
    let h = (BASE_H * scale).ceil() as u32;
    plot.render_png_with_size(w, h)
        .map_err(|e| GgplotError::new(format!("histogram render: {e}")))
}

/// Extract column `x` from the inline rows as f64s. `null`s are skipped (NA);
/// any non-numeric value is a clear error — never a silently garbled chart.
fn numeric_column(rows: &Value, x: &str) -> Result<Vec<f64>, GgplotError> {
    let arr = rows
        .as_array()
        .ok_or_else(|| GgplotError::new("rows must be a JSON array of objects"))?;
    let mut out = Vec::with_capacity(arr.len());
    for row in arr {
        match row.get(x) {
            None | Some(Value::Null) => continue,
            Some(v) => out.push(v.as_f64().ok_or_else(|| {
                GgplotError::new(format!(
                    "column `{x}` has a non-numeric value ({v}); a histogram needs numbers"
                ))
            })?),
        }
    }
    if out.is_empty() {
        return Err(GgplotError::new(format!(
            "column `{x}` has no numeric values to bin"
        )));
    }
    Ok(out)
}

/// Map the deployment's `--pk-*` tokens onto a ggplot-rs theme — minimally:
/// `bg` → plot/panel background, `text` → text colour, `brand` → the
/// single-series primary. Tokens that don't parse as `#rrggbb`/`#rgb` hex are
/// accepted and ignored (theming never fails a render). The full token
/// mapping (grid, axis, palette, fonts) lands with the theme-parity issue
/// (peacock#8).
fn plot_theme(tokens: Option<&ThemeTokens>) -> Theme {
    let mut t = theme_minimal();
    let Some(tok) = tokens else {
        return t;
    };
    if let Some(bg) = parse_hex(&tok.bg) {
        let panel = ElementRect {
            fill: Some(bg),
            color: None,
            width: 0.0,
            visible: true,
        };
        t = t
            .set_plot_background(panel.clone())
            .set_panel_background(panel);
    }
    if let Some(text) = parse_hex(&tok.text) {
        let mut el = t.text.clone();
        el.color = text;
        t = t.set_text(el);
    }
    if let Some(brand) = parse_hex(&tok.brand) {
        t = t.with_primary(brand);
    }
    t
}

/// Parse `#rrggbb` / `#rgb` into an RGB triple; anything else is `None`.
fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.trim().strip_prefix('#')?;
    match hex.len() {
        6 => {
            let n = u32::from_str_radix(hex, 16).ok()?;
            Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
        }
        3 => {
            let n = u32::from_str_radix(hex, 16).ok()?;
            let (r, g, b) = ((n >> 8) & 0xf, (n >> 4) & 0xf, n & 0xf);
            Some(((r * 17) as u8, (g * 17) as u8, (b * 17) as u8))
        }
        _ => None,
    }
}
