//! Legends: discrete swatch list and continuous gradient bar.

use std::fmt::Write as _;

use super::scales::sequential_color;
use super::svgutil::{self, escape, fmt_num};
use super::unit::Frame;

pub(crate) fn draw_discrete_legend(
    svg: &mut String,
    frame: &Frame,
    entries: &[(String, String)], // (color, label)
    title: Option<String>,
) {
    let lx = frame.plot_x1 + 18.0;
    let mut ly = frame.plot_y0 + 6.0;
    if let Some(t) = &title {
        let _ = write!(
            svg,
            r##"<text x="{lx:.1}" y="{ly:.1}" font-size="11" font-weight="bold" fill="#333">{}</text>"##,
            escape(t)
        );
        ly += 16.0;
    }
    for (i, (color, label)) in entries.iter().enumerate() {
        let y = ly + (i as f64) * 20.0;
        let _ = write!(
            svg,
            r##"<rect x="{lx:.1}" y="{y:.1}" width="12" height="12" fill="{color}"/>"##
        );
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="11" fill="#333">{}</text>"##,
            lx + 18.0,
            y + 11.0,
            escape(&svgutil::short_label(label))
        );
    }
}

pub(crate) fn draw_gradient_legend(
    svg: &mut String,
    frame: &Frame,
    lo: f64,
    hi: f64,
    scheme: Option<&str>,
    title: Option<String>,
) {
    let lx = frame.plot_x1 + 18.0;
    let mut ly = frame.plot_y0 + 6.0;
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
    // Emit the gradient as stacked thin rects (no <defs>/url refs — keeps the
    // output free of href-like attributes for the guardrail-friendly check).
    let steps = 40;
    for i in 0..steps {
        let t = i as f64 / (steps - 1) as f64;
        let color = sequential_color(scheme, 1.0 - t); // top = high
        let y = ly + t * bar_h;
        let _ = write!(
            svg,
            r##"<rect x="{lx:.1}" y="{y:.1}" width="{bar_w:.1}" height="{:.2}" fill="{color}"/>"##,
            bar_h / steps as f64 + 0.5
        );
    }
    // tick labels (hi at top, lo at bottom)
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
