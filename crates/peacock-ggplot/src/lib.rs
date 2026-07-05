//! `peacock-ggplot` — the pluggable STATISTICAL render backend (issues
//! #6/#7/#8), peer to `peacock-rasterizer`: `(spec, rows, schema, theme,
//! size) → PNG`.
//!
//! Wraps [`ggplot-rs`](https://github.com/sipemu/ggplot-rs) (Grammar of
//! Graphics over plotters): pure Rust, headless in-memory PNG, fonts bundled
//! — no polars (default features off), no R, no Node, no network (NFR-S-5).
//! peacock-core routes a spec here when its JSON carries a top-level `geom`
//! key (Vega-Lite uses `mark`, never `geom`); everything else stays on the
//! Vega-Lite rasterizer. This crate only compiles behind peacock-core's
//! `ggplot` cargo feature, so the base tree never pulls the plotters graph.
//!
//! The spec is the typed stat dialect ([`peacock_types::StatSpec`], issue
//! #7): geoms `histogram | density | boxplot | ecdf`, the `color` /
//! `facet_wrap` aesthetics, and the `vline` / `p90` annotations. The rows are
//! escurel's ACL-checked JSON rows plus their SCHEMA — the [`adapter`] types
//! every column from the schema's type names (numeric vs categorical vs
//! temporal), never by sniffing the JSON (issue #8). Where ggplot-rs 0.9.2
//! cannot honour a declared aesthetic (see the per-geom notes below) this
//! backend returns a clear structured error — it never silently drops a
//! declared mark.
//!
//! ## Backend support matrix (ggplot-rs 0.9.2)
//!
//! | geom      | color series | facet_wrap | vline / p90 | note |
//! |-----------|--------------|------------|-------------|------|
//! | histogram | ✗ error      | ✓          | ✓ + label   | `StatBin` drops grouping columns; `GeomHistogram` draws one fill |
//! | density   | ✓            | ✓          | ✓ + label   | |
//! | ecdf      | ✗ error      | ✓          | ✓ + label   | `GeomStep` draws a single path — groups would interleave |
//! | boxplot   | ✗ error      | ✓          | ✓ (as hline on the value axis, no label) | `GeomBoxplot` draws one fill; text can't anchor on the discrete axis |
//!
//! ## Theming (issue #8)
//!
//! One brand source: the deployment's `--pk-*` tokens map onto ggplot-rs's
//! programmatic [`Theme`] — `font` → the (bundled) text face, `bg` →
//! plot/panel background, `surface` → legend/facet-strip fills, `text` →
//! body + title, `muted` → axis text + captions, `grid` → gridlines,
//! `axis` → axis lines/ticks, `border` → the panel border, `brand` → the
//! single-series primary, `accent` → annotation reference lines, and
//! `palette` → the categorical multi-series colors. Rendering is
//! deterministic: fonts are bundled DejaVu (plotters `ab_glyph`, no system
//! lookup), so same brand ⇒ same bytes.

mod adapter;

pub use adapter::{ColumnKind, ColumnSchema};

use adapter::{NeededColumn, aligned_columns};
use ggplot_rs::data::Value as GgValue;
use ggplot_rs::prelude::{
    Aes, ElementLine, ElementRect, GGPlot, GeomHistogram, GeomHline, GeomVline, Linetype,
    RGBAColor, StatEcdf, theme_minimal,
};
use ggplot_rs::theme::Theme;
use peacock_theme::ThemeTokens;
use peacock_types::{StatAnnotation, StatGeom, StatSpec};
use serde_json::Value;
use std::collections::BTreeSet;

/// Statistical-rendering / spec error.
#[derive(Debug, thiserror::Error)]
#[error("ggplot render failed: {0}")]
pub struct GgplotError(String);

impl GgplotError {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
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

/// Render a STATISTICAL spec (the typed dialect, issue #7) to PNG bytes,
/// headless and in-memory. `rows` is the escurel row array the composer
/// injected at the spec's `data.values`; `schema` is that RowSet's column
/// schema (`data.schema`) — the adapter types every column from it (issue
/// #8). `theme` maps the deployment's `--pk-*` tokens onto the chart; `None`
/// renders the stock look. `scale` ≥ 1.0 multiplies the 800×600 natural size
/// (the same semantics as `render_vega_to_png`).
pub fn render_stat_to_png(
    spec: &Value,
    rows: &Value,
    schema: &[ColumnSchema],
    theme: Option<&ThemeTokens>,
    scale: f32,
) -> Result<Vec<u8>, GgplotError> {
    // Re-parse the typed dialect here too: this is a public entry point, so a
    // spec that never went through compose still fails closed.
    let spec = StatSpec::parse(spec).map_err(|e| GgplotError::new(e.to_string()))?;
    check_backend_support(&spec)?;
    let resolved = resolve_theme(theme);

    let plot = match spec.geom {
        StatGeom::Boxplot => boxplot_plot(&spec, rows, schema, &resolved)?,
        StatGeom::Histogram | StatGeom::Density | StatGeom::Ecdf => {
            distribution_plot(&spec, rows, schema, &resolved)?
        }
    };
    finish(plot, &spec, resolved.theme, scale)
}

/// Reject the aesthetic combinations ggplot-rs 0.9.2 cannot honour — a clear
/// structured error, never a silently dropped mark (see the support matrix in
/// the crate docs).
fn check_backend_support(spec: &StatSpec) -> Result<(), GgplotError> {
    if spec.color.is_some() && spec.geom != StatGeom::Density {
        return Err(GgplotError::new(format!(
            "the ggplot backend cannot draw a `color` series for geom `{}` (ggplot-rs 0.9.2 \
             renders it as a single series); declare `facet_wrap` for per-series small \
             multiples, or use geom `density` for coloured overlays",
            spec.geom.as_str()
        )));
    }
    if spec.geom == StatGeom::Boxplot
        && spec.annotations.iter().any(|a| {
            matches!(
                a,
                StatAnnotation::Vline { label: Some(_), .. }
                    | StatAnnotation::P90 { label: Some(_) }
            )
        })
    {
        return Err(GgplotError::new(
            "the ggplot backend cannot place an annotation `label` on a boxplot (the discrete \
             category axis has no numeric anchor in ggplot-rs 0.9.2); drop the label — the \
             reference line itself still draws on the value axis"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Build the plot for the x-numeric distribution geoms (histogram / density /
/// ecdf): one numeric/temporal `x` column, optional `color` series (density
/// only, enforced above), optional facet column, vline/p90 on the x axis.
fn distribution_plot(
    spec: &StatSpec,
    rows: &Value,
    schema: &[ColumnSchema],
    resolved: &ResolvedTheme,
) -> Result<GGPlot, GgplotError> {
    let geom = spec.geom;
    let mut columns = vec![NeededColumn::value(&spec.x)];
    if let Some(c) = &spec.color {
        columns.push(NeededColumn::group(c));
    }
    if let Some(f) = &spec.facet_wrap {
        columns.push(NeededColumn::group(f));
    }
    let data = aligned_columns(rows, schema, &columns, geom)?;
    let x_values: Vec<f64> = data[0]
        .1
        .iter()
        .filter_map(GgValue::as_f64)
        .collect::<Vec<_>>();
    // The color column's distinct levels (sorted, deterministic) — the
    // categorical palette maps onto them below.
    let color_levels: BTreeSet<String> = match spec.color {
        Some(_) => data[1]
            .1
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        None => BTreeSet::new(),
    };

    let mut aes = Aes::new().x(&spec.x);
    if let Some(c) = &spec.color {
        aes = aes.color(c);
    }
    let mut plot = GGPlot::new(data).aes(aes);
    plot = match geom {
        StatGeom::Histogram => {
            let bins = spec.bins.map_or(30, |b| b.clamp(1, 1000) as usize);
            plot.geom_histogram_with(GeomHistogram {
                bins,
                ..Default::default()
            })
            .ylab("count")
        }
        StatGeom::Density => plot.geom_density().ylab("density"),
        StatGeom::Ecdf => plot.geom_step().stat(StatEcdf).ylab("ecdf"),
        StatGeom::Boxplot => unreachable!("boxplot has its own builder"),
    };
    plot = plot.xlab(&spec.x);

    // The brand's categorical palette drives the multi-series colors, mapped
    // over the sorted distinct levels (cycling when the palette is shorter).
    if !color_levels.is_empty() && !resolved.palette.is_empty() {
        let pairs: Vec<(&str, RGBAColor)> = color_levels
            .iter()
            .enumerate()
            .map(|(i, level)| {
                let (r, g, b) = resolved.palette[i % resolved.palette.len()];
                (level.as_str(), RGBAColor::new(r, g, b))
            })
            .collect();
        plot = plot.scale_color_manual(pairs);
    }

    // Annotations live on the x (value) axis; labels sit on the baseline
    // (y = 0 is on-scale for counts, densities and the ecdf alike).
    for ann in &spec.annotations {
        let (at, label, linetype) = annotation_line(ann, &x_values)?;
        plot = plot.geom_vline_with(GeomVline {
            linetype,
            color: resolved.accent,
            ..GeomVline::new(at)
        });
        if let Some(label) = label {
            plot = plot.annotate_text(label, at, 0.0);
        }
    }
    Ok(plot)
}

/// Build the boxplot: categorical `x`, numeric `y`, optional facet column.
/// Its numeric axis is y, so vline/p90 draw as HORIZONTAL reference lines at
/// the value (labels are rejected in [`check_backend_support`]).
fn boxplot_plot(
    spec: &StatSpec,
    rows: &Value,
    schema: &[ColumnSchema],
    resolved: &ResolvedTheme,
) -> Result<GGPlot, GgplotError> {
    let y = spec
        .y
        .as_deref()
        .expect("StatSpec::parse guarantees boxplot has a y");
    let mut columns = vec![NeededColumn::group(&spec.x), NeededColumn::value(y)];
    if let Some(f) = &spec.facet_wrap {
        columns.push(NeededColumn::group(f));
    }
    let data = aligned_columns(rows, schema, &columns, spec.geom)?;
    let y_values: Vec<f64> = data[1]
        .1
        .iter()
        .filter_map(GgValue::as_f64)
        .collect::<Vec<_>>();

    let mut plot = GGPlot::new(data)
        .aes(Aes::new().x(&spec.x).y(y))
        .geom_boxplot()
        .xlab(&spec.x)
        .ylab(y);

    for ann in &spec.annotations {
        let (at, _label, linetype) = annotation_line(ann, &y_values)?;
        plot = plot.geom_hline_with(GeomHline {
            linetype,
            color: resolved.accent,
            ..GeomHline::new(at)
        });
    }
    Ok(plot)
}

/// Resolve one annotation into `(value, label, linetype)`: a `vline` is the
/// authored value (dashed, ggplot's reference-line default); a `p90` is the
/// computed 90th percentile of the chart's value column (dotted, so contract
/// and quantile stay distinguishable).
fn annotation_line<'a>(
    ann: &'a StatAnnotation,
    values: &[f64],
) -> Result<(f64, Option<&'a str>, Linetype), GgplotError> {
    match ann {
        StatAnnotation::Vline { at, label } => Ok((*at, label.as_deref(), Linetype::Dashed)),
        StatAnnotation::P90 { label } => Ok((
            quantile(values, 0.9)?,
            Some(label.as_deref().unwrap_or("p90")),
            Linetype::Dotted,
        )),
    }
}

/// The type-7 (R default) `q`-quantile of `values`.
fn quantile(values: &[f64], q: f64) -> Result<f64, GgplotError> {
    if values.is_empty() {
        return Err(GgplotError::new(
            "cannot compute a quantile of an empty column".to_owned(),
        ));
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let h = (sorted.len() - 1) as f64 * q;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    Ok(sorted[lo] + (h - h.floor()) * (sorted[hi] - sorted[lo]))
}

/// Apply the cross-geom finishers (facet, title, theme, size) and render.
fn finish(
    mut plot: GGPlot,
    spec: &StatSpec,
    theme: Theme,
    scale: f32,
) -> Result<Vec<u8>, GgplotError> {
    if let Some(facet) = &spec.facet_wrap {
        plot = plot.facet_wrap(facet, None);
    }
    if let Some(title) = &spec.title {
        plot = plot.title(title);
    }
    plot = plot.theme(theme);

    let scale = scale.max(1.0);
    let w = (BASE_W * scale).ceil() as u32;
    let h = (BASE_H * scale).ceil() as u32;
    plot.render_png_with_size(w, h)
        .map_err(|e| GgplotError::new(format!("{} render: {e}", spec.geom.as_str())))
}

/// The brand, resolved for the ggplot backend: the programmatic [`Theme`]
/// plus the parts the theme struct cannot carry — the categorical palette
/// (applied per-plot as a manual color scale) and the annotation accent.
struct ResolvedTheme {
    theme: Theme,
    palette: Vec<(u8, u8, u8)>,
    accent: (u8, u8, u8),
}

/// Map the deployment's `--pk-*` tokens onto the backend (issue #8, the full
/// mapping): `font` → the root text face (ggplot-rs propagates it to every
/// text element; fonts are bundled, so an unavailable family falls back to
/// DejaVu deterministically), `bg` → plot + panel background, `surface` →
/// legend + facet-strip fills, `text` → body + title, `muted` → axis
/// text / caption / legend text, `grid` → major + minor gridlines, `axis` →
/// axis lines + ticks, `border` → the panel border, `brand` → the
/// single-series primary, `accent` → annotation reference lines, `palette` →
/// the categorical series colors. Tokens that don't parse as
/// `#rrggbb`/`#rgb` hex are accepted and ignored (theming never fails a
/// render); `None` is the stock `theme_minimal` look.
fn resolve_theme(tokens: Option<&ThemeTokens>) -> ResolvedTheme {
    let mut t = theme_minimal();
    let Some(tok) = tokens else {
        return ResolvedTheme {
            theme: t,
            palette: Vec::new(),
            accent: (0, 0, 0),
        };
    };

    if let Some(family) = first_font_family(&tok.font) {
        t.text.family = family;
    }
    if let Some(bg) = parse_hex(&tok.bg) {
        let panel = ElementRect {
            fill: Some(bg),
            color: None,
            width: 0.0,
            visible: true,
        };
        t.plot_background = panel.clone();
        t.panel_background = panel;
    }
    if let Some(surface) = parse_hex(&tok.surface) {
        let card = ElementRect {
            fill: Some(surface),
            color: parse_hex(&tok.border),
            width: 0.5,
            visible: true,
        };
        t.legend_background = card.clone();
        t.legend_key = ElementRect {
            color: None,
            width: 0.0,
            ..card.clone()
        };
        t.strip_background = card;
    }
    if let Some(text) = parse_hex(&tok.text) {
        t.text.color = text;
        t.title.color = text;
    }
    if let Some(muted) = parse_hex(&tok.muted) {
        t.axis_text_x.color = muted;
        t.axis_text_y.color = muted;
        t.caption.color = muted;
        t.legend_text.color = muted;
    }
    if let Some(grid) = parse_hex(&tok.grid) {
        t.panel_grid_major.color = grid;
        t.panel_grid_minor.color = grid;
    }
    if let Some(axis) = parse_hex(&tok.axis) {
        let line = ElementLine {
            color: axis,
            width: 1.0,
            visible: true,
            linetype: Linetype::Solid,
        };
        t.axis_line = line.clone();
        t.axis_ticks = ElementLine { width: 0.5, ..line };
    }
    if let Some(border) = parse_hex(&tok.border) {
        t.panel_border = ElementLine {
            color: border,
            width: 1.0,
            visible: true,
            linetype: Linetype::Solid,
        };
    }
    if let Some(brand) = parse_hex(&tok.brand) {
        t = t.with_primary(brand);
    }

    ResolvedTheme {
        theme: t,
        palette: tok.palette.iter().filter_map(|c| parse_hex(c)).collect(),
        accent: parse_hex(&tok.accent).unwrap_or((0, 0, 0)),
    }
}

/// The first family of a CSS `font-family` list, quotes stripped — the name
/// ggplot-rs registers its bundled face under (`serif`/`mono` keywords pick
/// the matching bundled DejaVu face; anything else is the sans face).
fn first_font_family(css: &str) -> Option<String> {
    let first = css.split(',').next()?.trim().trim_matches(['"', '\'']);
    if first.is_empty() {
        None
    } else {
        Some(first.to_owned())
    }
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
