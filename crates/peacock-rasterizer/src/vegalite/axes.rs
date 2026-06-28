//! Axis rendering: gridlines, ticks, labels, axis lines and titles, for both
//! continuous (linear/log) and discrete (band/point) channels on x and y.

use std::fmt::Write as _;

use super::parse::Channel;
use super::scales::{BandScale, LinearScale};
use super::svgutil::{self, escape, fmt_num};
use super::unit::Frame;

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_axes(
    svg: &mut String,
    frame: &Frame,
    x_ch: &Channel,
    y_ch: &Channel,
    x_cats: &[String],
    band: Option<&BandScale>,
    x_lin: Option<&LinearScale>,
    y_lin: &LinearScale,
    y_discrete: bool,
    y_cats: &[String],
    y_band: Option<&BandScale>,
) {
    let (x0, x1, y0, y1) = (frame.plot_x0, frame.plot_x1, frame.plot_y0, frame.plot_y1);
    let grid = y_ch.grid.unwrap_or(true);

    // --- Y axis ---
    if y_discrete {
        if let Some(yb) = y_band {
            for (i, label) in y_cats.iter().enumerate() {
                let cy = yb.center(i);
                let _ = write!(
                    svg,
                    r##"<text x="{:.1}" y="{:.1}" font-size="11" text-anchor="end" fill="#444">{}</text>"##,
                    x0 - 8.0,
                    cy + 4.0,
                    escape(&svgutil::short_label(label))
                );
            }
        }
    } else {
        for v in y_lin.ticks(5) {
            let y = y_lin.map(v);
            if grid {
                let _ = write!(
                    svg,
                    r##"<line x1="{x0:.1}" y1="{y:.1}" x2="{x1:.1}" y2="{y:.1}" stroke="#e6e6e6" stroke-width="1"/>"##
                );
            }
            let label = match &y_ch.format {
                Some(f) => svgutil::fmt_with(v, f),
                None => fmt_num(v),
            };
            let _ = write!(
                svg,
                r##"<text x="{:.1}" y="{:.1}" font-size="11" text-anchor="end" fill="#444">{}</text>"##,
                x0 - 8.0,
                y + 4.0,
                escape(&label)
            );
        }
    }

    // --- X axis labels ---
    let angle = x_ch.label_angle.unwrap_or(0.0);
    if let Some(b) = band {
        for (i, label) in x_cats.iter().enumerate() {
            let cx = b.center(i);
            write_x_label(svg, cx, y1 + 18.0, &svgutil::short_label(label), angle);
        }
    } else if let Some(xl) = x_lin {
        for v in xl.ticks(5) {
            let cx = xl.map(v);
            if grid {
                let _ = write!(
                    svg,
                    r##"<line x1="{cx:.1}" y1="{y0:.1}" x2="{cx:.1}" y2="{y1:.1}" stroke="#f0f0f0" stroke-width="1"/>"##
                );
            }
            write_x_label(svg, cx, y1 + 18.0, &fmt_num(v), angle);
        }
    }

    // --- Axis lines ---
    let _ = write!(
        svg,
        r##"<line x1="{x0:.1}" y1="{y1:.1}" x2="{x1:.1}" y2="{y1:.1}" stroke="#888" stroke-width="1"/>"##
    );
    let _ = write!(
        svg,
        r##"<line x1="{x0:.1}" y1="{y0:.1}" x2="{x0:.1}" y2="{y1:.1}" stroke="#888" stroke-width="1"/>"##
    );

    // --- Axis titles ---
    if let Some(t) = x_ch.effective_title() {
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="12" text-anchor="middle" fill="#222">{}</text>"##,
            (x0 + x1) / 2.0,
            frame.height - 12.0,
            escape(&t)
        );
    }
    if let Some(t) = y_ch.effective_title() {
        let midy = (y0 + y1) / 2.0;
        let _ = write!(
            svg,
            r##"<text x="14" y="{midy:.1}" font-size="12" text-anchor="middle" fill="#222" transform="rotate(-90 14 {midy:.1})">{}</text>"##,
            escape(&t)
        );
    }
}

fn write_x_label(svg: &mut String, cx: f64, y: f64, label: &str, angle: f64) {
    if angle.abs() > 1.0 {
        let _ = write!(
            svg,
            r##"<text x="{cx:.1}" y="{y:.1}" font-size="11" text-anchor="end" fill="#444" transform="rotate({angle} {cx:.1} {y:.1})">{}</text>"##,
            escape(label)
        );
    } else {
        let _ = write!(
            svg,
            r##"<text x="{cx:.1}" y="{y:.1}" font-size="11" text-anchor="middle" fill="#444">{}</text>"##,
            escape(label)
        );
    }
}
