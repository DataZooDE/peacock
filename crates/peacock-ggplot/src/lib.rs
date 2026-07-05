//! `peacock-ggplot` — the pluggable STATISTICAL render backend (issues #6/#7),
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
//! The spec is the typed stat dialect ([`peacock_types::StatSpec`], issue
//! #7): geoms `histogram | density | boxplot | ecdf`, the `color` /
//! `facet_wrap` aesthetics, and the `vline` / `p90` annotations. Where
//! ggplot-rs 0.9.2 cannot honour a declared aesthetic (see the per-geom
//! notes below) this backend returns a clear structured error — it never
//! silently drops a declared mark.
//!
//! ## Backend support matrix (ggplot-rs 0.9.2)
//!
//! | geom      | color series | facet_wrap | vline / p90 | note |
//! |-----------|--------------|------------|-------------|------|
//! | histogram | ✗ error      | ✓          | ✓ + label   | `StatBin` drops grouping columns; `GeomHistogram` draws one fill |
//! | density   | ✓            | ✓          | ✓ + label   | |
//! | ecdf      | ✗ error      | ✓          | ✓ + label   | `GeomStep` draws a single path — groups would interleave |
//! | boxplot   | ✗ error      | ✓          | ✓ (as hline on the value axis, no label) | `GeomBoxplot` draws one fill; text can't anchor on the discrete axis |

use ggplot_rs::data::Value as GgValue;
use ggplot_rs::prelude::{
    Aes, ElementRect, GGPlot, GeomHistogram, GeomHline, GeomVline, Linetype, StatEcdf,
    theme_minimal,
};
use ggplot_rs::theme::Theme;
use peacock_theme::ThemeTokens;
use peacock_types::{StatAnnotation, StatGeom, StatSpec};
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

/// Render a STATISTICAL spec (the typed dialect, issue #7) to PNG bytes,
/// headless and in-memory. `rows` is the inline data the composer injected at
/// the spec's `data.values` — an array of `{column: value}` objects. `theme`
/// maps the deployment's `--pk-*` tokens onto the chart; `None` renders the
/// stock look. `scale` ≥ 1.0 multiplies the 800×600 natural size (the same
/// semantics as `render_vega_to_png`).
pub fn render_stat_to_png(
    spec: &Value,
    rows: &Value,
    theme: Option<&ThemeTokens>,
    scale: f32,
) -> Result<Vec<u8>, GgplotError> {
    // Re-parse the typed dialect here too: this is a public entry point, so a
    // spec that never went through compose still fails closed.
    let spec = StatSpec::parse(spec).map_err(|e| GgplotError::new(e.to_string()))?;
    check_backend_support(&spec)?;

    let plot = match spec.geom {
        StatGeom::Boxplot => boxplot_plot(&spec, rows)?,
        StatGeom::Histogram | StatGeom::Density | StatGeom::Ecdf => distribution_plot(&spec, rows)?,
    };
    finish(plot, &spec, theme, scale)
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
/// ecdf): one numeric `x` column, optional `color` series (density only,
/// enforced above), optional facet column, vline/p90 on the x axis.
fn distribution_plot(spec: &StatSpec, rows: &Value) -> Result<GGPlot, GgplotError> {
    let geom = spec.geom;
    let mut columns = vec![NeededColumn::numeric(&spec.x)];
    if let Some(c) = &spec.color {
        columns.push(NeededColumn::categorical(c));
    }
    if let Some(f) = &spec.facet_wrap {
        columns.push(NeededColumn::categorical(f));
    }
    let data = aligned_columns(rows, &columns, geom)?;
    let x_values: Vec<f64> = data[0]
        .1
        .iter()
        .filter_map(GgValue::as_f64)
        .collect::<Vec<_>>();

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

    // Annotations live on the x (value) axis; labels sit on the baseline
    // (y = 0 is on-scale for counts, densities and the ecdf alike).
    for ann in &spec.annotations {
        let (at, label, linetype) = annotation_line(ann, &x_values)?;
        plot = plot.geom_vline_with(GeomVline {
            linetype,
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
fn boxplot_plot(spec: &StatSpec, rows: &Value) -> Result<GGPlot, GgplotError> {
    let y = spec
        .y
        .as_deref()
        .expect("StatSpec::parse guarantees boxplot has a y");
    let mut columns = vec![NeededColumn::categorical(&spec.x), NeededColumn::numeric(y)];
    if let Some(f) = &spec.facet_wrap {
        columns.push(NeededColumn::categorical(f));
    }
    let data = aligned_columns(rows, &columns, spec.geom)?;
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
    theme: Option<&ThemeTokens>,
    scale: f32,
) -> Result<Vec<u8>, GgplotError> {
    if let Some(facet) = &spec.facet_wrap {
        plot = plot.facet_wrap(facet, None);
    }
    if let Some(title) = &spec.title {
        plot = plot.title(title);
    }
    plot = plot.theme(plot_theme(theme));

    let scale = scale.max(1.0);
    let w = (BASE_W * scale).ceil() as u32;
    let h = (BASE_H * scale).ceil() as u32;
    plot.render_png_with_size(w, h)
        .map_err(|e| GgplotError::new(format!("{} render: {e}", spec.geom.as_str())))
}

/// One column to pull out of the inline rows.
struct NeededColumn<'a> {
    name: &'a str,
    numeric: bool,
}

impl<'a> NeededColumn<'a> {
    fn numeric(name: &'a str) -> Self {
        NeededColumn {
            name,
            numeric: true,
        }
    }
    fn categorical(name: &'a str) -> Self {
        NeededColumn {
            name,
            numeric: false,
        }
    }
}

/// Extract `columns` from the inline rows as row-aligned ggplot columns
/// (grouping/facet values must stay aligned with the measure). A row whose
/// FIRST requested column (the geometry's driving column) is `null` is
/// skipped whole (NA); a non-numeric value in a numeric column is a clear
/// error — never a silently garbled chart.
fn aligned_columns(
    rows: &Value,
    columns: &[NeededColumn<'_>],
    geom: StatGeom,
) -> Result<Vec<(String, Vec<GgValue>)>, GgplotError> {
    let arr = rows
        .as_array()
        .ok_or_else(|| GgplotError::new("rows must be a JSON array of objects"))?;
    let mut out: Vec<(String, Vec<GgValue>)> = columns
        .iter()
        .map(|c| (c.name.to_owned(), Vec::with_capacity(arr.len())))
        .collect();

    'row: for row in arr {
        // Drop the row when the driving column is NA — checked first so no
        // partial row is pushed.
        if matches!(row.get(columns[0].name), None | Some(Value::Null)) {
            continue;
        }
        let mut converted = Vec::with_capacity(columns.len());
        for col in columns {
            let v = row.get(col.name).unwrap_or(&Value::Null);
            if col.numeric {
                match v {
                    Value::Null => continue 'row, // NA in a secondary numeric column
                    _ => converted.push(GgValue::Float(v.as_f64().ok_or_else(|| {
                        GgplotError::new(format!(
                            "column `{}` has a non-numeric value ({v}); geom `{}` needs numbers",
                            col.name,
                            geom.as_str()
                        ))
                    })?)),
                }
            } else {
                converted.push(categorical(v));
            }
        }
        for (slot, value) in out.iter_mut().zip(converted) {
            slot.1.push(value);
        }
    }

    if out[0].1.is_empty() {
        return Err(GgplotError::new(format!(
            "column `{}` has no values to plot",
            columns[0].name
        )));
    }
    Ok(out)
}

/// A categorical cell: strings pass through; numbers/bools become their text
/// form; `null` groups as `"NA"`.
fn categorical(v: &Value) -> GgValue {
    match v {
        Value::String(s) => GgValue::Str(s.clone()),
        Value::Null => GgValue::Str("NA".to_owned()),
        other => GgValue::Str(other.to_string()),
    }
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
