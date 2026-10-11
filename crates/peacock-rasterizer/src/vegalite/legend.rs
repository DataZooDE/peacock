//! Legends: discrete swatch list and continuous gradient bar.

use std::fmt::Write as _;
use std::sync::OnceLock;

use super::scales::sequential_color;
use super::svgutil::{escape, fmt_num};
use super::unit::Frame;

const FONT_SIZE: f64 = 11.0;
const ENTRY_GAP: f64 = 14.0;
const ENTRY_HEIGHT: f64 = 20.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Orient {
    Right,
    Bottom,
    None,
}

impl Orient {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("bottom") => Self::Bottom,
            Some("none") => Self::None,
            _ => Self::Right, // unsupported Vega-Lite orients fall back to right
        }
    }
}

fn face() -> &'static ttf_parser::Face<'static> {
    static FACE: OnceLock<ttf_parser::Face<'static>> = OnceLock::new();
    FACE.get_or_init(|| {
        ttf_parser::Face::parse(include_bytes!("../../assets/DejaVuSans.ttf"), 0)
            .expect("vendored font")
    })
}

pub(crate) fn text_width(s: &str) -> f64 {
    s.chars().map(glyph_width).sum()
}

fn glyph_width(c: char) -> f64 {
    let face = face();
    let units = face
        .glyph_index(c)
        .and_then(|g| face.glyph_hor_advance(g))
        .unwrap_or(0);
    f64::from(units) * FONT_SIZE / f64::from(face.units_per_em())
}

fn fit_label(label: &str, limit: f64) -> String {
    if limit <= 0.0 || text_width(label) <= limit {
        return label.to_owned();
    }
    let mut fitted = String::new();
    let mut width = text_width("…");
    for c in label.chars() {
        let next_width = width + glyph_width(c);
        if next_width > limit {
            break;
        }
        fitted.push(c);
        width = next_width;
    }
    format!("{fitted}…")
}

pub(crate) fn right_width(
    labels: &[String],
    title: Option<&str>,
    outer_width: f64,
    label_limit: Option<f64>,
) -> f64 {
    let natural = labels
        .iter()
        .map(|s| text_width(s))
        .fold(title.map(text_width).unwrap_or(0.0), f64::max);
    let requested = match label_limit {
        Some(v) if v > 0.0 => natural.min(v),
        _ => natural,
    };
    (requested + 46.0).max(150.0).min(outer_width * 0.4)
}

pub(crate) fn bottom_height(
    labels: &[String],
    title: Option<&str>,
    width: f64,
    label_limit: Option<f64>,
) -> f64 {
    let mut rows = 1;
    let mut x = 0.0;
    for label in labels {
        let label_w = match label_limit {
            Some(v) if v > 0.0 => text_width(label).min(v),
            _ => text_width(label),
        };
        let entry_w = 18.0 + label_w.min((width - 30.0).max(20.0)) + ENTRY_GAP;
        if x > 0.0 && x + entry_w > width {
            rows += 1;
            x = 0.0;
        }
        x += entry_w;
    }
    34.0 + if title.is_some() { 16.0 } else { 0.0 } + f64::from(rows) * ENTRY_HEIGHT + 8.0
}

pub(crate) fn draw_discrete_legend(
    svg: &mut String,
    frame: &Frame,
    entries: &[(String, String)],
    title: Option<String>,
    orient: Orient,
    label_limit: Option<f64>,
    width: f64,
) {
    let (start_x, mut y) = match orient {
        Orient::Right => (frame.plot_x1 + 18.0, frame.plot_y0 + 6.0),
        Orient::Bottom => (frame.plot_x0, frame.plot_y1 + 90.0),
        Orient::None => return,
    };
    if let Some(t) = &title {
        let title_width = (width - start_x - 10.0).max(12.0);
        let _ = write!(
            svg,
            r##"<text x="{start_x:.1}" y="{y:.1}" font-size="11" font-weight="bold" fill="#333">{}</text>"##,
            escape(&fit_label(t, title_width))
        );
        y += 16.0;
    }
    let mut x = start_x;
    let max_label = match orient {
        Orient::Right => (width - start_x - 10.0).max(12.0),
        Orient::Bottom => (width - frame.plot_x0 - 30.0).max(12.0),
        Orient::None => unreachable!(),
    };
    let max_label = match label_limit {
        Some(v) if v > 0.0 => max_label.min(v),
        _ => max_label,
    };
    for (color, label) in entries {
        let shown = fit_label(label, max_label);
        let entry_w = 18.0 + text_width(&shown) + ENTRY_GAP;
        if orient == Orient::Bottom && x > start_x && x + entry_w > width - 12.0 {
            x = start_x;
            y += ENTRY_HEIGHT;
        }
        let _ = write!(
            svg,
            r##"<rect x="{x:.1}" y="{y:.1}" width="12" height="12" fill="{color}"/>"##
        );
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="11" fill="#333">{}</text>"##,
            x + 18.0,
            y + 11.0,
            escape(&shown)
        );
        if orient == Orient::Right {
            y += ENTRY_HEIGHT;
        } else {
            x += entry_w;
        }
    }
}

pub(crate) fn draw_gradient_legend(
    svg: &mut String,
    frame: &Frame,
    lo: f64,
    hi: f64,
    scheme: Option<&str>,
    title: Option<String>,
    orient: Orient,
) {
    if orient == Orient::None {
        return;
    }
    let lx = if orient == Orient::Bottom {
        frame.plot_x0
    } else {
        frame.plot_x1 + 18.0
    };
    let mut ly = if orient == Orient::Bottom {
        frame.plot_y1 + 90.0
    } else {
        frame.plot_y0 + 6.0
    };
    if let Some(t) = &title {
        let _ = write!(
            svg,
            r##"<text x="{lx:.1}" y="{ly:.1}" font-size="11" font-weight="bold" fill="#333">{}</text>"##,
            escape(t)
        );
        ly += 14.0;
    }
    let bar_w = 14.0;
    let bar_h = 120.0;
    let steps = 40;
    for i in 0..steps {
        let t = i as f64 / (steps - 1) as f64;
        let color = sequential_color(scheme, 1.0 - t);
        let y = ly + t * bar_h;
        let _ = write!(
            svg,
            r##"<rect x="{lx:.1}" y="{y:.1}" width="{bar_w:.1}" height="{:.2}" fill="{color}"/>"##,
            bar_h / steps as f64 + 0.5
        );
    }
    let _ = write!(
        svg,
        r##"<text x="{:.1}" y="{:.1}" font-size="10" fill="#444">{}</text>"##,
        lx + bar_w + 6.0,
        ly + 8.0,
        escape(&fmt_num(hi))
    );
    let _ = write!(
        svg,
        r##"<text x="{:.1}" y="{:.1}" font-size="10" fill="#444">{}</text>"##,
        lx + bar_w + 6.0,
        ly + bar_h,
        escape(&fmt_num(lo))
    );
}
