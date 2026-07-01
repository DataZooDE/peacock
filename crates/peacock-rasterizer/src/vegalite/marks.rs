//! Mark rendering: line / area / bar / point / circle / rect / tick / rule /
//! text (cartesian) and arc (polar pie/donut).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value;

use super::layout::Rendered;
use super::parse::{Encoding, Mark, MarkDef};
use super::scales::{self, BandScale, LinearScale};
use super::transforms::Row;
use super::unit::{Agg, Frame};
use super::{data, svgutil};
use crate::RasterError;

/// Everything a cartesian mark needs to draw itself.
pub(crate) struct MarkCtx<'a> {
    pub frame: &'a Frame,
    pub x_cats: &'a [String],
    pub series: &'a [String],
    pub band: Option<&'a BandScale>,
    pub x_lin: Option<&'a LinearScale>,
    pub y_lin: &'a LinearScale,
    pub y_band: Option<&'a BandScale>,
    pub y_cats: &'a [String],
    pub agg: &'a Agg,
    pub stacked: bool,
    pub mark_def: &'a MarkDef,
    pub enc: &'a Encoding,
    pub rows: &'a [Row],
}

impl MarkCtx<'_> {
    fn x_center(&self, xi: usize) -> f64 {
        self.band
            .map(|b| b.center(xi))
            .unwrap_or((self.frame.plot_x0 + self.frame.plot_x1) / 2.0)
    }
}

pub(crate) fn draw_marks<F>(svg: &mut String, mark: Mark, ctx: &MarkCtx, color_for: &F)
where
    F: Fn(usize, Option<f64>) -> String,
{
    match mark {
        Mark::Line => draw_line(svg, ctx, color_for, false),
        Mark::Area => draw_area(svg, ctx, color_for),
        Mark::Bar => draw_bar(svg, ctx, color_for),
        Mark::Point | Mark::Circle => draw_points(svg, ctx, color_for),
        Mark::Rect => draw_rect(svg, ctx, color_for),
        Mark::Tick => draw_tick(svg, ctx, color_for),
        Mark::Rule => draw_rule(svg, ctx, color_for),
        Mark::Text => draw_text(svg, ctx, color_for),
        Mark::Arc => {}
    }
    // mark.point overlay for line marks
    if mark == Mark::Line && ctx.mark_def.point {
        draw_line(svg, ctx, color_for, true);
    }
}

fn opacity_attr(o: Option<f64>) -> String {
    match o {
        Some(v) => format!(r#" fill-opacity="{v:.2}""#),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// line
// ---------------------------------------------------------------------------

fn draw_line<F>(svg: &mut String, ctx: &MarkCtx, color_for: &F, points_only: bool)
where
    F: Fn(usize, Option<f64>) -> String,
{
    for (si, _name) in ctx.series.iter().enumerate() {
        let color = color_for(si, None);
        let pts: Vec<(f64, f64)> = (0..ctx.x_cats.len())
            .filter_map(|xi| {
                ctx.agg
                    .values
                    .get(&(xi, si))
                    .map(|y| (ctx.x_center(xi), ctx.y_lin.map(*y)))
            })
            .collect();
        if !points_only && pts.len() > 1 {
            let d: String = pts
                .iter()
                .map(|(x, y)| format!("{x:.1},{y:.1}"))
                .collect::<Vec<_>>()
                .join(" ");
            let _ = write!(
                svg,
                r##"<polyline points="{d}" fill="none" stroke="{color}" stroke-width="2"/>"##
            );
        }
        if points_only {
            for (x, y) in &pts {
                let _ = write!(
                    svg,
                    r##"<circle cx="{x:.1}" cy="{y:.1}" r="3" fill="{color}"/>"##
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// area (stacked or overlaid)
// ---------------------------------------------------------------------------

fn draw_area<F>(svg: &mut String, ctx: &MarkCtx, color_for: &F)
where
    F: Fn(usize, Option<f64>) -> String,
{
    let base = ctx.y_lin.map(0.0);
    // running baseline per x index for stacking
    let mut baseline: BTreeMap<usize, f64> = BTreeMap::new();
    for (si, _name) in ctx.series.iter().enumerate() {
        let color = color_for(si, None);
        let mut top: Vec<(f64, f64)> = Vec::new();
        let mut bot: Vec<(f64, f64)> = Vec::new();
        for xi in 0..ctx.x_cats.len() {
            if let Some(y) = ctx.agg.values.get(&(xi, si)) {
                let cx = ctx.x_center(xi);
                let prev = *baseline.get(&xi).unwrap_or(&0.0);
                let (lo_v, hi_v) = if ctx.stacked {
                    let nv = prev + *y;
                    baseline.insert(xi, nv);
                    (prev, nv)
                } else {
                    (0.0, *y)
                };
                top.push((cx, ctx.y_lin.map(hi_v)));
                bot.push((cx, ctx.y_lin.map(lo_v)));
            }
        }
        if top.len() > 1 {
            let mut d = String::new();
            for (x, y) in &top {
                let _ = write!(d, "{x:.1},{y:.1} ");
            }
            for (x, y) in bot.iter().rev() {
                let _ = write!(d, "{x:.1},{y:.1} ");
            }
            let fill_op = if ctx.stacked { 0.85 } else { 0.4 };
            let _ = write!(
                svg,
                r##"<polygon points="{}" fill="{color}" fill-opacity="{fill_op}"/>"##,
                d.trim_end()
            );
        }
    }
    let _ = base;
}

// ---------------------------------------------------------------------------
// bar (grouped or stacked)
// ---------------------------------------------------------------------------

fn draw_bar<F>(svg: &mut String, ctx: &MarkCtx, color_for: &F)
where
    F: Fn(usize, Option<f64>) -> String,
{
    let band = match ctx.band {
        Some(b) => b,
        None => return,
    };
    let bandwidth = band.bandwidth().max(1.0);
    let n_series = ctx.series.len().max(1);
    let base_px = ctx.y_lin.map(0.0);

    // A CONTINUOUS colour encoding (e.g. a numeric risk score) colours each bar
    // by its OWN value on the sequential ramp — not by a discrete series. Look
    // the value up per x-category; a nominal colour stays series-indexed
    // (`None` ⇒ the palette by series index, as before).
    let continuous = ctx
        .enc
        .color
        .as_ref()
        .is_some_and(super::parse::Channel::is_quantitative);
    let color_field = ctx.enc.color.as_ref().and_then(|c| c.field.clone());
    let x_field = ctx.enc.x.as_ref().and_then(|c| c.field.clone());
    let color_value = |xi: usize| -> Option<f64> {
        if !continuous {
            return None;
        }
        let (cf, xf) = (color_field.as_ref()?, x_field.as_ref()?);
        let xcat = ctx.x_cats.get(xi)?;
        ctx.rows
            .iter()
            .find(|r| data::cell_string(r.get(xf)) == *xcat)
            .map(|r| data::cell_num(r.get(cf)))
    };

    if ctx.stacked {
        let mut baseline: BTreeMap<usize, f64> = BTreeMap::new();
        for xi in 0..ctx.x_cats.len() {
            for (si, _n) in ctx.series.iter().enumerate() {
                if let Some(y) = ctx.agg.values.get(&(xi, si)) {
                    let prev = *baseline.get(&xi).unwrap_or(&0.0);
                    let nv = prev + y.max(0.0);
                    baseline.insert(xi, nv);
                    let x = band.band_start(xi);
                    let y0 = ctx.y_lin.map(prev);
                    let y1 = ctx.y_lin.map(nv);
                    let color = color_for(si, color_value(xi));
                    rect(svg, x, y1.min(y0), bandwidth, (y0 - y1).abs(), &color, None);
                }
            }
        }
    } else {
        let bw = bandwidth / n_series as f64;
        for xi in 0..ctx.x_cats.len() {
            for (si, _n) in ctx.series.iter().enumerate() {
                if let Some(y) = ctx.agg.values.get(&(xi, si)) {
                    let x = band.band_start(xi) + bw * si as f64;
                    let top = ctx.y_lin.map(*y);
                    let color = color_for(si, color_value(xi));
                    let (yy, hh) = if top <= base_px {
                        (top, base_px - top)
                    } else {
                        (base_px, top - base_px)
                    };
                    rect(
                        svg,
                        x,
                        yy,
                        bw.max(1.0),
                        hh.max(0.0),
                        &color,
                        ctx.mark_def.opacity,
                    );
                }
            }
        }
    }
}

fn rect(svg: &mut String, x: f64, y: f64, w: f64, h: f64, color: &str, opacity: Option<f64>) {
    let _ = write!(
        svg,
        r##"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" fill="{color}"{}/>"##,
        opacity_attr(opacity)
    );
}

// ---------------------------------------------------------------------------
// point / circle scatter
// ---------------------------------------------------------------------------

fn draw_points<F>(svg: &mut String, ctx: &MarkCtx, color_for: &F)
where
    F: Fn(usize, Option<f64>) -> String,
{
    // size range
    let size_range = size_extent(ctx);
    let default_r = ctx
        .mark_def
        .size
        .map(|s| (s / std::f64::consts::PI).sqrt())
        .unwrap_or(3.5);

    let stroked = ctx.mark_def.filled == Some(false);
    if ctx.agg.continuous_x {
        let xl = match ctx.x_lin {
            Some(x) => x,
            None => return,
        };
        for p in &ctx.agg.raw {
            let cx = xl.map(p.x);
            let cy = ctx.y_lin.map(p.y);
            let r = radius_for(p.size, size_range, default_r);
            let color = color_for(p.series, p.color_value);
            let op = p.opacity.map(|o| normalize_opacity(o, ctx));
            if stroked {
                circle_stroked(svg, cx, cy, r, &color);
            } else {
                circle(svg, cx, cy, r, &color, op);
            }
        }
    } else {
        // discrete x scatter (one point per (x,series))
        for (si, _n) in ctx.series.iter().enumerate() {
            let color = color_for(si, None);
            for xi in 0..ctx.x_cats.len() {
                if let Some(y) = ctx.agg.values.get(&(xi, si)) {
                    let cx = ctx.x_center(xi);
                    let cy = ctx.y_lin.map(*y);
                    circle(svg, cx, cy, default_r, &color, ctx.mark_def.opacity);
                }
            }
        }
    }
}

fn circle(svg: &mut String, cx: f64, cy: f64, r: f64, color: &str, opacity: Option<f64>) {
    let _ = write!(
        svg,
        r##"<circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.2}" fill="{color}"{}/>"##,
        opacity_attr(opacity)
    );
}

fn circle_stroked(svg: &mut String, cx: f64, cy: f64, r: f64, color: &str) {
    let _ = write!(
        svg,
        r##"<circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.2}" fill="none" stroke="{color}" stroke-width="1.5"/>"##
    );
}

fn size_extent(ctx: &MarkCtx) -> Option<(f64, f64)> {
    let field = ctx.enc.size.as_ref().and_then(|c| c.field.clone())?;
    let vals: Vec<f64> = ctx
        .rows
        .iter()
        .map(|r| data::cell_num(r.get(&field)))
        .collect();
    let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    lo.is_finite().then_some((lo, hi))
}

fn radius_for(size: Option<f64>, extent: Option<(f64, f64)>, default_r: f64) -> f64 {
    match (size, extent) {
        (Some(v), Some((lo, hi))) => {
            let t = if (hi - lo).abs() < f64::EPSILON {
                0.5
            } else {
                (v - lo) / (hi - lo)
            };
            3.0 + t.clamp(0.0, 1.0) * 9.0
        }
        _ => default_r,
    }
}

fn normalize_opacity(v: f64, ctx: &MarkCtx) -> f64 {
    let field = match ctx.enc.opacity.as_ref().and_then(|c| c.field.clone()) {
        Some(f) => f,
        None => return v.clamp(0.0, 1.0),
    };
    let vals: Vec<f64> = ctx
        .rows
        .iter()
        .map(|r| data::cell_num(r.get(&field)))
        .collect();
    let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if (hi - lo).abs() < f64::EPSILON {
        1.0
    } else {
        (0.15 + 0.85 * (v - lo) / (hi - lo)).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// rect (heatmap)
// ---------------------------------------------------------------------------

fn draw_rect<F>(svg: &mut String, ctx: &MarkCtx, color_for: &F)
where
    F: Fn(usize, Option<f64>) -> String,
{
    let (xband, yband) = match (ctx.band, ctx.y_band) {
        (Some(x), Some(y)) => (x, y),
        _ => return,
    };
    // The y band is built with an inverted range (top↔bottom), so its
    // bandwidth is negative — take the magnitude, and place each cell on its
    // band centre so the fill is correct regardless of axis orientation.
    let bw = xband.bandwidth().abs().max(1.0);
    let bh = yband.bandwidth().abs().max(1.0);
    let x_field = ctx
        .enc
        .x
        .as_ref()
        .and_then(|c| c.field.clone())
        .unwrap_or_default();
    let y_field = ctx
        .enc
        .y
        .as_ref()
        .and_then(|c| c.field.clone())
        .unwrap_or_default();
    let c_field = ctx.enc.color.as_ref().and_then(|c| c.field.clone());

    let x_index: BTreeMap<&str, usize> = ctx
        .x_cats
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let y_index: BTreeMap<&str, usize> = ctx
        .y_cats
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();

    for r in ctx.rows {
        let xs = data::cell_string(r.get(&x_field));
        let ys = data::cell_string(r.get(&y_field));
        let (Some(&xi), Some(&yi)) = (x_index.get(xs.as_str()), y_index.get(ys.as_str())) else {
            continue;
        };
        let cv = c_field.as_ref().map(|f| data::cell_num(r.get(f)));
        let color = color_for(0, cv);
        let x = xband.center(xi) - bw / 2.0;
        let y = yband.center(yi) - bh / 2.0;
        rect(svg, x, y, bw, bh, &color, None);
    }
}

// ---------------------------------------------------------------------------
// tick
// ---------------------------------------------------------------------------

fn draw_tick<F>(svg: &mut String, ctx: &MarkCtx, color_for: &F)
where
    F: Fn(usize, Option<f64>) -> String,
{
    let tick_h = 14.0;
    if ctx.agg.continuous_x {
        if let Some(xl) = ctx.x_lin {
            for p in &ctx.agg.raw {
                let cx = xl.map(p.x);
                let cy = ctx.y_lin.map(p.y);
                let color = color_for(p.series, None);
                let _ = write!(
                    svg,
                    r##"<line x1="{cx:.1}" y1="{:.1}" x2="{cx:.1}" y2="{:.1}" stroke="{color}" stroke-width="2"/>"##,
                    cy - tick_h / 2.0,
                    cy + tick_h / 2.0
                );
            }
        }
    } else {
        for (si, _n) in ctx.series.iter().enumerate() {
            let color = color_for(si, None);
            for xi in 0..ctx.x_cats.len() {
                if let Some(y) = ctx.agg.values.get(&(xi, si)) {
                    let cx = ctx.x_center(xi);
                    let cy = ctx.y_lin.map(*y);
                    let _ = write!(
                        svg,
                        r##"<line x1="{:.1}" y1="{cy:.1}" x2="{:.1}" y2="{cy:.1}" stroke="{color}" stroke-width="2"/>"##,
                        cx - 10.0,
                        cx + 10.0
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// rule
// ---------------------------------------------------------------------------

fn draw_rule<F>(svg: &mut String, ctx: &MarkCtx, color_for: &F)
where
    F: Fn(usize, Option<f64>) -> String,
{
    let color = color_for(0, None);

    // Constant reference line: `y: {value: N}` → horizontal rule; `x: {value:
    // N}` → vertical rule, optionally bounded by x2/y2.
    if let Some(yv) = ctx
        .enc
        .y
        .as_ref()
        .and_then(|c| c.value.as_ref())
        .and_then(serde_json::Value::as_f64)
    {
        let y = ctx.y_lin.map(yv);
        let _ = write!(
            svg,
            r##"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="{color}" stroke-width="1.5"/>"##,
            ctx.frame.plot_x0, ctx.frame.plot_x1
        );
        return;
    }

    // Horizontal ranged rules: x..x2 at each y category (gantt / interval).
    if let (Some(xf), Some(x2f), Some(yband), Some(xl)) = (
        ctx.enc.x.as_ref().and_then(|c| c.field.clone()),
        ctx.enc.x2.as_ref().and_then(|c| c.field.clone()),
        ctx.y_band,
        ctx.x_lin,
    ) {
        let yfield = ctx
            .enc
            .y
            .as_ref()
            .and_then(|c| c.field.clone())
            .unwrap_or_default();
        for r in ctx.rows {
            let ys = data::cell_string(r.get(&yfield));
            if let Some(yi) = ctx.y_cats.iter().position(|c| c == &ys) {
                let cy = yband.center(yi);
                let x1 = xl.map(data::cell_num(r.get(&xf)));
                let x2 = xl.map(data::cell_num(r.get(&x2f)));
                let _ = write!(
                    svg,
                    r##"<line x1="{x1:.1}" y1="{cy:.1}" x2="{x2:.1}" y2="{cy:.1}" stroke="{color}" stroke-width="3"/>"##
                );
            }
        }
        return;
    }

    // Ranged rules: y from y to y2 at each x category (e.g. error bars).
    if let (Some(yf), Some(y2f), Some(band)) = (
        ctx.enc.y.as_ref().and_then(|c| c.field.clone()),
        ctx.enc.y2.as_ref().and_then(|c| c.field.clone()),
        ctx.band,
    ) {
        let xfield = ctx
            .enc
            .x
            .as_ref()
            .and_then(|c| c.field.clone())
            .unwrap_or_default();
        for r in ctx.rows {
            let xs = data::cell_string(r.get(&xfield));
            if let Some(xi) = ctx.x_cats.iter().position(|c| c == &xs) {
                let cx = band.center(xi);
                let y1 = ctx.y_lin.map(data::cell_num(r.get(&yf)));
                let y2 = ctx.y_lin.map(data::cell_num(r.get(&y2f)));
                let _ = write!(
                    svg,
                    r##"<line x1="{cx:.1}" y1="{y1:.1}" x2="{cx:.1}" y2="{y2:.1}" stroke="{color}" stroke-width="2"/>"##
                );
            }
        }
        return;
    }

    // A rule per x category spanning the y extent (or a horizontal baseline).
    if !ctx.x_cats.is_empty() {
        for xi in 0..ctx.x_cats.len() {
            let cx = ctx.x_center(xi);
            let _ = write!(
                svg,
                r##"<line x1="{cx:.1}" y1="{:.1}" x2="{cx:.1}" y2="{:.1}" stroke="{color}" stroke-width="1"/>"##,
                ctx.frame.plot_y0, ctx.frame.plot_y1
            );
        }
    } else {
        let y = ctx.y_lin.map(0.0);
        let _ = write!(
            svg,
            r##"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="{color}" stroke-width="1"/>"##,
            ctx.frame.plot_x0, ctx.frame.plot_x1
        );
    }
}

// ---------------------------------------------------------------------------
// text (value labels)
// ---------------------------------------------------------------------------

fn draw_text<F>(svg: &mut String, ctx: &MarkCtx, color_for: &F)
where
    F: Fn(usize, Option<f64>) -> String,
{
    let color = ctx
        .mark_def
        .color
        .clone()
        .unwrap_or_else(|| "#333".to_owned());
    let _ = color_for;
    // Continuous-x scatter labels (text encoding on raw points).
    if ctx.agg.continuous_x {
        if let Some(xl) = ctx.x_lin {
            for p in &ctx.agg.raw {
                let label = p.text.clone().unwrap_or_else(|| svgutil::fmt_num(p.y));
                let cx = xl.map(p.x);
                let cy = ctx.y_lin.map(p.y) - 4.0;
                let _ = write!(
                    svg,
                    r##"<text x="{cx:.1}" y="{cy:.1}" font-size="10" text-anchor="middle" fill="{color}">{}</text>"##,
                    svgutil::escape(&label)
                );
            }
        }
        return;
    }
    for (si, _n) in ctx.series.iter().enumerate() {
        for xi in 0..ctx.x_cats.len() {
            if let Some(y) = ctx.agg.values.get(&(xi, si)) {
                let cx = ctx.x_center(xi);
                let cy = ctx.y_lin.map(*y) - 4.0;
                let label = svgutil::fmt_num(*y);
                let _ = write!(
                    svg,
                    r##"<text x="{cx:.1}" y="{cy:.1}" font-size="10" text-anchor="middle" fill="{color}">{}</text>"##,
                    svgutil::escape(&label)
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// arc (pie / donut)
// ---------------------------------------------------------------------------

pub(crate) fn render_arc(
    spec: &Value,
    rows: &[Row],
    enc: &Encoding,
    mark_def: &MarkDef,
) -> Result<Rendered, RasterError> {
    let width = spec
        .get("width")
        .and_then(Value::as_f64)
        .unwrap_or(super::DEFAULT_W);
    let height = spec
        .get("height")
        .and_then(Value::as_f64)
        .unwrap_or(super::DEFAULT_H);
    let has_legend = enc.color.as_ref().and_then(|c| c.field.clone()).is_some();
    let cx = if has_legend {
        (width - 150.0) / 2.0
    } else {
        width / 2.0
    };
    let cy = height / 2.0;
    let radius = (cx.min(cy) - 24.0).max(20.0);
    let inner = mark_def
        .inner_radius
        .map(|r| r.min(radius - 4.0))
        .unwrap_or(0.0);

    // category = color field (or theta field); value = theta field.
    let theta_field = enc
        .theta
        .as_ref()
        .and_then(|c| c.field.clone())
        .or_else(|| enc.color.as_ref().and_then(|c| c.field.clone()))
        .ok_or_else(|| RasterError::new("arc mark needs a theta or color field"))?;
    let cat_field = enc
        .color
        .as_ref()
        .and_then(|c| c.field.clone())
        .unwrap_or_else(|| theta_field.clone());
    let agg = enc.theta.as_ref().and_then(|c| c.aggregate.clone());

    // aggregate value per category (first-seen order).
    let mut cats: Vec<String> = Vec::new();
    let mut totals: BTreeMap<String, f64> = BTreeMap::new();
    for r in rows {
        let cat = data::cell_string(r.get(&cat_field));
        data::index_of(&mut cats, &cat);
        let v = data::cell_num(r.get(&theta_field));
        let slot = totals.entry(cat).or_insert(0.0);
        match agg.as_deref() {
            Some("count") => *slot += 1.0,
            _ => *slot += v,
        }
    }
    let total: f64 = totals.values().sum::<f64>().max(f64::EPSILON);

    let palette = if let Some(range) = enc.color.as_ref().and_then(|c| c.range.clone()) {
        range
    } else {
        let scheme = enc.color.as_ref().and_then(|c| c.scheme.clone());
        scales::categorical_scheme(scheme.as_deref())
            .iter()
            .map(|s| s.to_string())
            .collect()
    };

    let mut body = String::new();
    let _ = write!(
        body,
        r##"<rect x="0" y="0" width="{width:.1}" height="{height:.1}" fill="#ffffff"/>"##
    );
    let mut angle = -std::f64::consts::FRAC_PI_2; // start at 12 o'clock
    for (i, cat) in cats.iter().enumerate() {
        let frac = totals[cat] / total;
        let sweep = frac * std::f64::consts::TAU;
        let a0 = angle;
        let a1 = angle + sweep;
        angle = a1;
        let color = &palette[i % palette.len()];
        let path = arc_path(cx, cy, radius, inner, a0, a1);
        let _ = write!(body, r##"<path d="{path}" fill="{color}"/>"##);
    }

    // legend
    let frame = Frame {
        height,
        plot_x0: 0.0,
        plot_x1: width - 150.0,
        plot_y0: 24.0,
        plot_y1: height - 24.0,
    };
    if has_legend {
        let labels: Vec<(String, String)> = cats
            .iter()
            .enumerate()
            .map(|(i, c)| (palette[i % palette.len()].clone(), c.clone()))
            .collect();
        super::legend::draw_discrete_legend(
            &mut body,
            &frame,
            &labels,
            enc.color.as_ref().and_then(|c| c.effective_title()),
        );
    }

    Ok(Rendered {
        body,
        width,
        height,
    })
}

/// SVG path for an annular/pie wedge from angle `a0` to `a1` (radians).
fn arc_path(cx: f64, cy: f64, r: f64, r_inner: f64, a0: f64, a1: f64) -> String {
    let (sx0, sy0) = (cx + r * a0.cos(), cy + r * a0.sin());
    let (sx1, sy1) = (cx + r * a1.cos(), cy + r * a1.sin());
    let large = if (a1 - a0) > std::f64::consts::PI {
        1
    } else {
        0
    };
    if r_inner <= 0.0 {
        format!(
            "M{cx:.1},{cy:.1} L{sx0:.1},{sy0:.1} A{r:.1},{r:.1} 0 {large} 1 {sx1:.1},{sy1:.1} Z"
        )
    } else {
        let (ix0, iy0) = (cx + r_inner * a0.cos(), cy + r_inner * a0.sin());
        let (ix1, iy1) = (cx + r_inner * a1.cos(), cy + r_inner * a1.sin());
        format!(
            "M{sx0:.1},{sy0:.1} A{r:.1},{r:.1} 0 {large} 1 {sx1:.1},{sy1:.1} \
             L{ix1:.1},{iy1:.1} A{r_inner:.1},{r_inner:.1} 0 {large} 0 {ix0:.1},{iy0:.1} Z"
        )
    }
}
