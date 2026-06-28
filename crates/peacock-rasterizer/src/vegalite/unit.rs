//! Render a single (unit) view: derive scales from its encoding + data, then
//! emit axes, marks and a legend. Pie/donut (`arc`) is a polar special case.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value;

use super::layout::Rendered;
use super::parse::{Channel, Encoding, Mark, MarkDef};
use super::scales::{self, BandScale, ContinuousKind, LinearScale};
use super::transforms::Row;
use super::{axes, data, legend, marks, svgutil};
use crate::RasterError;

pub(crate) const M_LEFT: f64 = 64.0;
pub(crate) const M_TOP: f64 = 20.0;
pub(crate) const M_BOTTOM: f64 = 56.0;
const LEGEND_W: f64 = 150.0;
const M_RIGHT_BARE: f64 = 24.0;

/// Render one unit view. `overlay` true means "don't redraw the background /
/// axes" (used for layered marks after the first).
pub fn render_unit(
    spec: &Value,
    rows: &[Row],
    overlay: Option<bool>,
) -> Result<Rendered, RasterError> {
    let mark = Mark::parse(spec)?;
    let mark_def = MarkDef::parse(spec);
    let mut enc = Encoding::parse(spec);
    let overlay = overlay.unwrap_or(false);

    // Encoding-level `bin: true` rewrites the field to bin-start values and
    // makes the channel ordinal (computed in Rust, declaratively).
    let rows = apply_encoding_bins(rows, &mut enc);

    if mark == Mark::Arc {
        return marks::render_arc(spec, &rows, &enc, &mark_def);
    }

    render_cartesian(spec, &rows, mark, &mark_def, &enc, overlay)
}

/// Apply `bin: true` on x/y channels by rewriting each row's field to its bin
/// start and switching the channel to ordinal (so it lays out as bands).
fn apply_encoding_bins(rows: &[Row], enc: &mut Encoding) -> Vec<Row> {
    let mut rows = rows.to_vec();
    for ch in [enc.x.as_mut(), enc.y.as_mut()].into_iter().flatten() {
        if !ch.bin {
            continue;
        }
        let field = match &ch.field {
            Some(f) => f.clone(),
            None => continue,
        };
        let vals: Vec<f64> = rows.iter().map(|r| data::cell_num(r.get(&field))).collect();
        let (lo, _hi, width) = super::transforms::bin_params(&vals, 10.0);
        for r in &mut rows {
            if let Some(v) = r.get(&field).and_then(Value::as_f64) {
                let idx = ((v - lo) / width).floor().max(0.0);
                let start = lo + idx * width;
                let label = svgutil::fmt_num(start);
                r.insert(field.clone(), Value::String(label));
            }
        }
        ch.ty = "ordinal".to_owned();
        ch.bin = false;
    }
    rows
}

/// Shared geometry for a cartesian unit view (plot rectangle + outer height).
pub(crate) struct Frame {
    pub height: f64,
    pub plot_x0: f64,
    pub plot_x1: f64,
    pub plot_y0: f64,
    pub plot_y1: f64,
}

#[allow(clippy::too_many_lines)]
fn render_cartesian(
    spec: &Value,
    rows: &[Row],
    mark: Mark,
    mark_def: &MarkDef,
    enc: &Encoding,
    overlay: bool,
) -> Result<Rendered, RasterError> {
    let x_ch = enc.x.clone().unwrap_or_default();
    let y_ch = enc.y.clone().unwrap_or_default();

    let has_color = enc.color.as_ref().and_then(|c| c.field.clone()).is_some();
    let color_continuous = enc
        .color
        .as_ref()
        .map(|c| c.is_quantitative())
        .unwrap_or(false);
    let has_legend = has_color;

    let user_w = spec.get("width").and_then(Value::as_f64);
    let user_h = spec.get("height").and_then(Value::as_f64);

    let m_right = if has_legend { LEGEND_W } else { M_RIGHT_BARE };
    let plot_w = user_w.unwrap_or(super::DEFAULT_W - M_LEFT - m_right);
    let plot_h = user_h.unwrap_or(super::DEFAULT_H - M_TOP - M_BOTTOM);
    let width = M_LEFT + plot_w + m_right;
    let height = M_TOP + plot_h + M_BOTTOM;

    let plot_x0 = M_LEFT;
    let plot_x1 = M_LEFT + plot_w;
    let plot_y0 = M_TOP;
    let plot_y1 = M_TOP + plot_h;

    let frame = Frame {
        height,
        plot_x0,
        plot_x1,
        plot_y0,
        plot_y1,
    };

    // --- Determine x positioning: discrete band/point vs continuous. ---
    let x_field = x_ch.field.clone();
    let x_discrete = x_ch.discrete() || x_ch.is_temporal();
    let x_quant = x_ch.is_quantitative();

    // distinct x categories (for discrete x).
    let mut x_cats: Vec<String> = Vec::new();
    if let Some(xf) = &x_field {
        for r in rows {
            let v = data::cell_string(r.get(xf));
            data::index_of(&mut x_cats, &v);
        }
        if x_ch.is_temporal() || x_ch.ty == "ordinal" {
            apply_sort(&mut x_cats, &x_ch);
        } else if x_ch.ty == "nominal" {
            apply_sort_nominal(&mut x_cats, &x_ch);
        }
    }

    // color series
    let color_field = enc.color.as_ref().and_then(|c| c.field.clone());
    let mut series: Vec<String> = Vec::new();
    for r in rows {
        let s = match &color_field {
            Some(cf) => data::cell_string(r.get(cf)),
            None => y_ch.field.clone().unwrap_or_default(),
        };
        data::index_of(&mut series, &s);
    }
    if !color_continuous {
        series.sort();
    }

    // Stacking applies to bar/area with a color series and quantitative y.
    let stacked = should_stack(mark, &y_ch, enc.color.as_ref(), &series);

    // x scale
    let band = if x_discrete && !x_cats.is_empty() {
        let is_point = matches!(mark, Mark::Line | Mark::Point | Mark::Circle | Mark::Tick)
            && !x_ch.discrete()
            || x_ch.is_temporal();
        if is_point {
            Some(BandScale::point(x_cats.len(), plot_x0, plot_x1))
        } else {
            Some(BandScale::band(x_cats.len(), plot_x0, plot_x1, 0.2))
        }
    } else {
        None
    };

    // For quantitative x, build a linear scale.
    let x_lin = if x_quant {
        let xf = x_field.clone().unwrap_or_default();
        let vals: Vec<f64> = rows.iter().map(|r| data::cell_num(r.get(&xf))).collect();
        let (lo, hi) = continuous_domain(&vals, &x_ch, false);
        Some(make_linear(lo, hi, plot_x0, plot_x1, &x_ch))
    } else {
        None
    };

    // --- y domain ---
    let y_field = y_ch.field.clone();
    let y_discrete = y_ch.discrete();

    // aggregate (x_index|x_value, series) -> y, and stacked tops.
    let agg = aggregate_points(rows, enc, &x_cats, &series, &x_ch, &y_ch, x_lin.is_some());

    let (y_lo, y_hi) = if y_discrete {
        (0.0, 1.0)
    } else if stacked {
        let mut tops: BTreeMap<usize, f64> = BTreeMap::new();
        for ((xi, _si), v) in &agg.values {
            *tops.entry(*xi).or_insert(0.0) += v.max(0.0);
        }
        let max = tops.values().cloned().fold(0.0_f64, f64::max).max(1.0);
        (0.0, scales::nice_ceiling(max))
    } else {
        let mut all: Vec<f64> = agg.values.values().cloned().collect();
        // Continuous-x scatter keeps its y values in `raw`, not `values`.
        all.extend(agg.raw.iter().map(|p| p.y));
        // include y2 endpoints (ranged marks) in the domain.
        if let Some(y2f) = enc.y2.as_ref().and_then(|c| c.field.clone()) {
            all.extend(rows.iter().map(|r| data::cell_num(r.get(&y2f))));
        }
        if all.is_empty() {
            all.push(1.0);
        }
        continuous_domain(&all, &y_ch, true)
    };

    // y axis: discrete (heatmap) or continuous
    let mut y_cats: Vec<String> = Vec::new();
    if y_discrete && let Some(yf) = &y_field {
        for r in rows {
            let v = data::cell_string(r.get(yf));
            data::index_of(&mut y_cats, &v);
        }
        apply_sort(&mut y_cats, &y_ch);
    }
    let y_band = if y_discrete && !y_cats.is_empty() {
        Some(BandScale::band(y_cats.len(), plot_y1, plot_y0, 0.1))
    } else {
        None
    };
    let y_lin = LinearScale::new(y_lo, y_hi, plot_y1, plot_y0).with_kind(continuous_kind(&y_ch));

    // --- colour resolver ---
    let palette = resolve_palette(enc.color.as_ref());
    let color_for = |series_idx: usize, value: Option<f64>| -> String {
        if color_continuous {
            let scheme = enc.color.as_ref().and_then(|c| c.scheme.clone());
            let (lo, hi) = color_domain(rows, enc.color.as_ref());
            let t = if (hi - lo).abs() < f64::EPSILON {
                0.5
            } else {
                (value.unwrap_or(lo) - lo) / (hi - lo)
            };
            scales::sequential_color(scheme.as_deref(), t)
        } else if let Some(c) = &mark_def.color {
            if !has_color {
                c.clone()
            } else {
                palette[series_idx % palette.len()].clone()
            }
        } else if !has_color {
            // single-series default colour
            scales::TABLEAU10[0].to_owned()
        } else {
            palette[series_idx % palette.len()].clone()
        }
    };

    // --- emit ---
    let mut body = String::new();
    if !overlay {
        // background within this unit
        let _ = write!(
            body,
            r##"<rect x="0" y="0" width="{:.1}" height="{:.1}" fill="#ffffff"/>"##,
            width, height
        );
        // axes (skip y gridlines/labels for pure-discrete-y heatmaps handled below)
        axes::draw_axes(
            &mut body,
            &frame,
            &x_ch,
            &y_ch,
            &x_cats,
            band.as_ref(),
            x_lin.as_ref(),
            &y_lin,
            y_discrete,
            &y_cats,
            y_band.as_ref(),
        );
    }

    // marks
    let ctx = marks::MarkCtx {
        frame: &frame,
        x_cats: &x_cats,
        series: &series,
        band: band.as_ref(),
        x_lin: x_lin.as_ref(),
        y_lin: &y_lin,
        y_band: y_band.as_ref(),
        y_cats: &y_cats,
        agg: &agg,
        stacked,
        mark_def,
        enc,
        rows,
    };
    marks::draw_marks(&mut body, mark, &ctx, &color_for);

    // legend
    if !overlay && has_legend {
        if color_continuous {
            let (lo, hi) = color_domain(rows, enc.color.as_ref());
            let scheme = enc.color.as_ref().and_then(|c| c.scheme.clone());
            legend::draw_gradient_legend(
                &mut body,
                &frame,
                lo,
                hi,
                scheme.as_deref(),
                enc.color.as_ref().and_then(|c| c.effective_title()),
            );
        } else {
            let labels: Vec<(String, String)> = series
                .iter()
                .enumerate()
                .map(|(i, s)| (color_for(i, None), s.clone()))
                .collect();
            legend::draw_discrete_legend(
                &mut body,
                &frame,
                &labels,
                enc.color.as_ref().and_then(|c| c.effective_title()),
            );
        }
    }

    Ok(Rendered {
        body,
        width,
        height,
    })
}

/// Aggregated (x_index, series_index) → y plus, for continuous x, the raw
/// (x_value, y_value, series) tuples for scatter marks.
pub(crate) struct Agg {
    /// (x category index | binned index, series index) → aggregated y.
    pub values: BTreeMap<(usize, usize), f64>,
    /// Raw scatter rows: (x_value, y_value, series_index, size, opacity).
    pub raw: Vec<RawPoint>,
    pub continuous_x: bool,
}

pub(crate) struct RawPoint {
    pub x: f64,
    pub y: f64,
    pub series: usize,
    pub size: Option<f64>,
    pub opacity: Option<f64>,
    pub color_value: Option<f64>,
    pub text: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn aggregate_points(
    rows: &[Row],
    enc: &Encoding,
    x_cats: &[String],
    series: &[String],
    x_ch: &Channel,
    y_ch: &Channel,
    continuous_x: bool,
) -> Agg {
    let x_idx: BTreeMap<&str, usize> = x_cats
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let s_idx: BTreeMap<&str, usize> = series
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let color_field = enc.color.as_ref().and_then(|c| c.field.clone());
    let y_field = y_ch.field.clone().unwrap_or_default();
    let x_field = x_ch.field.clone().unwrap_or_default();
    let agg_op = y_ch.aggregate.clone();

    let mut values: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    let mut counts: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    let mut raw = Vec::new();

    for r in rows {
        let si = match &color_field {
            Some(cf) => s_idx.get(data::cell_string(r.get(cf)).as_str()).copied(),
            None => s_idx.get(y_field.as_str()).copied().or(Some(0)),
        }
        .unwrap_or(0);
        let yv = data::cell_num(r.get(&y_field));

        if continuous_x {
            let xv = data::cell_num(r.get(&x_field));
            raw.push(RawPoint {
                x: xv,
                y: yv,
                series: si,
                size: enc
                    .size
                    .as_ref()
                    .and_then(|c| c.field.as_ref())
                    .map(|f| data::cell_num(r.get(f))),
                opacity: enc
                    .opacity
                    .as_ref()
                    .and_then(|c| c.field.as_ref())
                    .map(|f| data::cell_num(r.get(f))),
                color_value: enc
                    .color
                    .as_ref()
                    .filter(|c| c.is_quantitative())
                    .and_then(|c| c.field.as_ref())
                    .map(|f| data::cell_num(r.get(f))),
                text: enc
                    .text
                    .as_ref()
                    .and_then(|c| c.field.as_ref())
                    .map(|f| data::cell_string(r.get(f))),
            });
            continue;
        }

        let xi = match x_idx.get(data::cell_string(r.get(&x_field)).as_str()) {
            Some(&i) => i,
            None => continue,
        };
        let slot = values.entry((xi, si)).or_insert(0.0);
        *counts.entry((xi, si)).or_insert(0) += 1;
        match agg_op.as_deref() {
            Some("sum") => *slot += yv,
            Some("mean") | Some("average") => *slot += yv, // divided below
            Some("count") => *slot += 1.0,
            Some("min") => {
                *slot = if counts[&(xi, si)] == 1 {
                    yv
                } else {
                    slot.min(yv)
                }
            }
            Some("max") => {
                *slot = if counts[&(xi, si)] == 1 {
                    yv
                } else {
                    slot.max(yv)
                }
            }
            Some(_) | None => *slot = yv,
        }
        // record text for non-aggregated rows
        if let Some(tf) = enc.text.as_ref().and_then(|c| c.field.as_ref()) {
            raw.push(RawPoint {
                x: xi as f64,
                y: yv,
                series: si,
                size: None,
                opacity: None,
                color_value: None,
                text: Some(data::cell_string(r.get(tf))),
            });
        }
    }
    // finalize mean
    if matches!(agg_op.as_deref(), Some("mean") | Some("average")) {
        for (k, v) in values.iter_mut() {
            let c = counts.get(k).copied().unwrap_or(1).max(1);
            *v /= c as f64;
        }
    }

    Agg {
        values,
        raw,
        continuous_x,
    }
}

fn should_stack(mark: Mark, y_ch: &Channel, color: Option<&Channel>, series: &[String]) -> bool {
    if !matches!(mark, Mark::Bar | Mark::Area) {
        return false;
    }
    if color.and_then(|c| c.field.as_ref()).is_none() || series.len() <= 1 {
        return false;
    }
    if let Some(st) = &y_ch.stack {
        return st != "none";
    }
    y_ch.is_quantitative()
}

fn continuous_kind(ch: &Channel) -> ContinuousKind {
    match ch.scale_type.as_deref() {
        Some("log") => ContinuousKind::Log,
        Some("sqrt") => ContinuousKind::Sqrt,
        Some("pow") => ContinuousKind::Pow,
        _ => ContinuousKind::Linear,
    }
}

fn make_linear(lo: f64, hi: f64, r0: f64, r1: f64, ch: &Channel) -> LinearScale {
    LinearScale::new(lo, hi, r0, r1).with_kind(continuous_kind(ch))
}

/// Compute a continuous domain (lo, hi) honouring `scale.zero` (default true
/// for y, false for x) and `scale.type=log`.
fn continuous_domain(vals: &[f64], ch: &Channel, is_y: bool) -> (f64, f64) {
    let min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let (mut min, mut max) = if min.is_finite() {
        (min, max)
    } else {
        (0.0, 1.0)
    };
    let log = ch.scale_type.as_deref() == Some("log");
    let zero = ch.scale_zero.unwrap_or(is_y && !log);
    if log {
        if min <= 0.0 {
            min = max.max(10.0) / 1000.0;
        }
        return (min, max.max(min * 10.0));
    }
    if zero {
        min = min.min(0.0);
        max = max.max(0.0);
    }
    if (max - min).abs() < f64::EPSILON {
        max = min + 1.0;
    }
    if is_y {
        // nice the top (and bottom if negative)
        let top = scales::nice_ceiling(max.max(0.0));
        let bot = if min < 0.0 {
            scales::nice_floor(min)
        } else {
            0.0_f64.min(min)
        };
        (bot, top.max(max))
    } else {
        (min, max)
    }
}

fn color_domain(rows: &[Row], ch: Option<&Channel>) -> (f64, f64) {
    let field = ch.and_then(|c| c.field.clone()).unwrap_or_default();
    let vals: Vec<f64> = rows.iter().map(|r| data::cell_num(r.get(&field))).collect();
    let min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if min.is_finite() {
        (min, max)
    } else {
        (0.0, 1.0)
    }
}

fn resolve_palette(ch: Option<&Channel>) -> Vec<String> {
    if let Some(c) = ch {
        if let Some(range) = &c.range
            && !range.is_empty()
        {
            return range.clone();
        }
        let scheme = c.scheme.as_deref();
        return scales::categorical_scheme(scheme)
            .iter()
            .map(|s| s.to_string())
            .collect();
    }
    scales::TABLEAU10.iter().map(|s| s.to_string()).collect()
}

/// Apply an ordinal/temporal sort (chronological / numeric), honouring an
/// explicit `sort` array if present.
fn apply_sort(cats: &mut [String], ch: &Channel) {
    if let Some(order) = explicit_order(ch) {
        sort_by_explicit(cats, &order);
        return;
    }
    data::sort_categories(cats);
}

fn apply_sort_nominal(cats: &mut [String], ch: &Channel) {
    if let Some(order) = explicit_order(ch) {
        sort_by_explicit(cats, &order);
    }
    // else keep first-seen order
}

fn explicit_order(ch: &Channel) -> Option<Vec<String>> {
    match &ch.sort {
        Some(Value::Array(a)) => Some(a.iter().map(|v| data::cell_string(Some(v))).collect()),
        _ => None,
    }
}

fn sort_by_explicit(cats: &mut [String], order: &[String]) {
    cats.sort_by_key(|c| order.iter().position(|o| o == c).unwrap_or(usize::MAX));
}
