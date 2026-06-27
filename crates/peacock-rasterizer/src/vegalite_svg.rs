//! Compile peacock's guardrail-restricted **Vega-Lite subset** to SVG, in
//! pure Rust (no Deno, no Node, no network).
//!
//! Supported subset (exactly what peacock's composer emits, FR-V-1/3): inline
//! `data.values`; mark `line` | `bar` | `point` | `area`; encodings `x`
//! (temporal/ordinal/nominal), `y` (quantitative, optional `sum` aggregate),
//! `color` (nominal series). Axes with ticks + labels, gridlines, and a
//! legend. Anything outside the subset is already rejected upstream by the
//! render guardrail (FR-V-4).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value;

use crate::RasterError;

const W: f64 = 680.0;
const H: f64 = 420.0;
const M_LEFT: f64 = 64.0;
const M_RIGHT: f64 = 150.0; // room for the legend
const M_TOP: f64 = 28.0;
const M_BOTTOM: f64 = 56.0;

/// Vega's default categorical palette (Tableau10), used per color series.
const PALETTE: &[&str] = &[
    "#4c78a8", "#f58518", "#54a24b", "#e45756", "#72b7b2", "#ff9da6", "#9d755d", "#bab0ac",
    "#e377c2", "#17becf",
];

#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Line,
    Bar,
    Point,
    Area,
}

struct Enc {
    x_field: String,
    y_field: String,
    color_field: Option<String>,
    y_sum: bool,
    x_title: String,
    y_title: String,
    /// `x` channel type — decides axis ordering (temporal/quantitative/ordinal
    /// sort; nominal keeps first-seen order).
    x_type: String,
}

/// Compile a safe-subset Vega-Lite spec into a standalone SVG document string.
pub fn vegalite_to_svg(spec: &Value) -> Result<String, RasterError> {
    let mark = parse_mark(spec)?;
    let enc = parse_encoding(spec)?;
    let rows = spec
        .get("data")
        .and_then(|d| d.get("values"))
        .and_then(Value::as_array)
        .ok_or_else(|| RasterError::new("spec has no inline data.values"))?;

    // Pass 1: collect distinct x categories and color series (first-seen).
    let mut x_cats: Vec<String> = Vec::new();
    let mut series: Vec<String> = Vec::new();
    for row in rows {
        let x = cell_to_string(row.get(&enc.x_field));
        index_of(&mut x_cats, &x);
        let s = match &enc.color_field {
            Some(cf) => cell_to_string(row.get(cf)),
            None => enc.y_field.clone(),
        };
        index_of(&mut series, &s);
    }
    // A temporal/quantitative/ordinal x axis must be ordered (ISO dates and
    // numbers sort correctly as strings/values); nominal stays first-seen.
    if matches!(enc.x_type.as_str(), "temporal" | "quantitative" | "ordinal") {
        x_cats.sort_by(|a, b| match (a.parse::<f64>(), b.parse::<f64>()) {
            (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
            _ => a.cmp(b),
        });
    }
    // Series order is deterministic (alphabetical) for a stable legend.
    series.sort();

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

    // Pass 2: aggregate (x, series) → y against the ordered indices.
    let mut points: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for row in rows {
        let x = cell_to_string(row.get(&enc.x_field));
        let s = match &enc.color_field {
            Some(cf) => cell_to_string(row.get(cf)),
            None => enc.y_field.clone(),
        };
        let y = row.get(&enc.y_field).and_then(Value::as_f64).unwrap_or(0.0);
        let (Some(&xi), Some(&si)) = (x_idx.get(x.as_str()), s_idx.get(s.as_str())) else {
            continue;
        };
        let slot = points.entry((xi, si)).or_insert(0.0);
        if enc.y_sum {
            *slot += y;
        } else {
            *slot = y;
        }
    }

    let y_max = points.values().cloned().fold(0.0_f64, f64::max).max(1.0);
    let y_max = nice_ceiling(y_max);

    let plot_w = W - M_LEFT - M_RIGHT;
    let plot_h = H - M_TOP - M_BOTTOM;
    let x_pos = |xi: usize| -> f64 {
        if x_cats.len() <= 1 {
            M_LEFT + plot_w / 2.0
        } else {
            M_LEFT + plot_w * (xi as f64) / ((x_cats.len() - 1) as f64)
        }
    };
    let y_pos = |y: f64| -> f64 { M_TOP + plot_h * (1.0 - y / y_max) };

    let mut svg = String::new();
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" font-family="DejaVu Sans, sans-serif">"##
    );
    let _ = write!(svg, r##"<rect width="{W}" height="{H}" fill="#ffffff"/>"##);

    // Y gridlines + ticks + labels (5 intervals).
    for i in 0..=5 {
        let v = y_max * (i as f64) / 5.0;
        let y = y_pos(v);
        let _ = write!(
            svg,
            r##"<line x1="{M_LEFT}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="#e6e6e6" stroke-width="1"/>"##,
            M_LEFT + plot_w
        );
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="11" text-anchor="end" fill="#444">{}</text>"##,
            M_LEFT - 8.0,
            y + 4.0,
            fmt_num(v)
        );
    }

    // X axis labels.
    for (xi, label) in x_cats.iter().enumerate() {
        let x = x_pos(xi);
        let _ = write!(
            svg,
            r##"<text x="{x:.1}" y="{:.1}" font-size="11" text-anchor="middle" fill="#444">{}</text>"##,
            M_TOP + plot_h + 18.0,
            escape(&short_label(label))
        );
    }

    // Axis lines.
    let _ = write!(
        svg,
        r##"<line x1="{M_LEFT}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#888" stroke-width="1"/>"##,
        M_TOP + plot_h,
        M_LEFT + plot_w,
        M_TOP + plot_h
    );
    let _ = write!(
        svg,
        r##"<line x1="{M_LEFT}" y1="{M_TOP}" x2="{M_LEFT}" y2="{:.1}" stroke="#888" stroke-width="1"/>"##,
        M_TOP + plot_h
    );

    // Series marks.
    for (si, name) in series.iter().enumerate() {
        let color = PALETTE[si % PALETTE.len()];
        match mark {
            Mark::Line | Mark::Area | Mark::Point => {
                let pts: Vec<(f64, f64)> = (0..x_cats.len())
                    .filter_map(|xi| points.get(&(xi, si)).map(|y| (x_pos(xi), y_pos(*y))))
                    .collect();
                if mark != Mark::Point && pts.len() > 1 {
                    let d: String = pts
                        .iter()
                        .map(|(x, y)| format!("{x:.1},{y:.1}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if mark == Mark::Area {
                        let base = y_pos(0.0);
                        let _ = write!(
                            svg,
                            r##"<polygon points="{:.1},{base:.1} {d} {:.1},{base:.1}" fill="{color}" fill-opacity="0.3"/>"##,
                            pts.first().unwrap().0,
                            pts.last().unwrap().0
                        );
                    }
                    let _ = write!(
                        svg,
                        r##"<polyline points="{d}" fill="none" stroke="{color}" stroke-width="2"/>"##
                    );
                }
                for (x, y) in &pts {
                    let _ = write!(
                        svg,
                        r##"<circle cx="{x:.1}" cy="{y:.1}" r="3" fill="{color}"/>"##
                    );
                }
            }
            Mark::Bar => {
                let band = if x_cats.len() <= 1 {
                    plot_w * 0.6
                } else {
                    plot_w / (x_cats.len() as f64) * 0.8
                };
                let bw = band / (series.len() as f64);
                for xi in 0..x_cats.len() {
                    if let Some(y) = points.get(&(xi, si)) {
                        let cx = x_pos(xi) - band / 2.0 + bw * (si as f64);
                        let top = y_pos(*y);
                        let h = (M_TOP + plot_h) - top;
                        let _ = write!(
                            svg,
                            r##"<rect x="{cx:.1}" y="{top:.1}" width="{bw:.1}" height="{h:.1}" fill="{color}"/>"##
                        );
                    }
                }
            }
        }
        let _ = name; // series name used in the legend below
    }

    // Legend.
    let lx = M_LEFT + plot_w + 18.0;
    for (si, name) in series.iter().enumerate() {
        let color = PALETTE[si % PALETTE.len()];
        let ly = M_TOP + 6.0 + (si as f64) * 20.0;
        let _ = write!(
            svg,
            r##"<rect x="{lx:.1}" y="{ly:.1}" width="12" height="12" fill="{color}"/>"##
        );
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="11" fill="#333">{}</text>"##,
            lx + 18.0,
            ly + 11.0,
            escape(&short_label(name))
        );
    }

    // Axis titles.
    let _ = write!(
        svg,
        r##"<text x="{:.1}" y="{:.1}" font-size="12" text-anchor="middle" fill="#222">{}</text>"##,
        M_LEFT + plot_w / 2.0,
        H - 12.0,
        escape(&enc.x_title)
    );
    let _ = write!(
        svg,
        r##"<text x="14" y="{:.1}" font-size="12" text-anchor="middle" fill="#222" transform="rotate(-90 14 {:.1})">{}</text>"##,
        M_TOP + plot_h / 2.0,
        M_TOP + plot_h / 2.0,
        escape(&enc.y_title)
    );

    svg.push_str("</svg>");
    Ok(svg)
}

fn parse_mark(spec: &Value) -> Result<Mark, RasterError> {
    let m = spec.get("mark");
    let name = match m {
        Some(Value::String(s)) => s.as_str(),
        Some(Value::Object(o)) => o.get("type").and_then(Value::as_str).unwrap_or(""),
        _ => "",
    };
    match name {
        "line" => Ok(Mark::Line),
        "bar" => Ok(Mark::Bar),
        "point" | "circle" => Ok(Mark::Point),
        "area" => Ok(Mark::Area),
        other => Err(RasterError::new(format!("unsupported mark `{other}`"))),
    }
}

fn parse_encoding(spec: &Value) -> Result<Enc, RasterError> {
    let enc = spec
        .get("encoding")
        .ok_or_else(|| RasterError::new("spec has no encoding"))?;
    let field = |ch: &str| -> Option<String> {
        enc.get(ch)
            .and_then(|c| c.get("field"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let title = |ch: &str, fallback: &str| -> String {
        enc.get(ch)
            .and_then(|c| c.get("title"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| fallback.to_owned())
    };
    let x_field = field("x").ok_or_else(|| RasterError::new("encoding.x has no field"))?;
    let y_field = field("y").ok_or_else(|| RasterError::new("encoding.y has no field"))?;
    let y_sum = enc
        .get("y")
        .and_then(|c| c.get("aggregate"))
        .and_then(Value::as_str)
        == Some("sum");
    let x_type = enc
        .get("x")
        .and_then(|c| c.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("nominal")
        .to_owned();
    Ok(Enc {
        x_title: title("x", &x_field),
        y_title: title("y", &y_field),
        x_field,
        y_field,
        color_field: field("color"),
        y_sum,
        x_type,
    })
}

fn index_of(v: &mut Vec<String>, s: &str) -> usize {
    if let Some(i) = v.iter().position(|e| e == s) {
        i
    } else {
        v.push(s.to_owned());
        v.len() - 1
    }
}

fn cell_to_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Round a max up to a "nice" axis ceiling (1/2/2.5/5 × 10^k).
fn nice_ceiling(max: f64) -> f64 {
    if max <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(max.log10().floor());
    let norm = max / mag;
    let nice = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 2.5 {
        2.5
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// Trim an ISO date like `1997-01-01` to `1997-01` for a compact axis label.
fn short_label(s: &str) -> String {
    if s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-') {
        s[..7].to_owned()
    } else {
        s.to_owned()
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
