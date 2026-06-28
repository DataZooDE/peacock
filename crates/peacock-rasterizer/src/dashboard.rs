//! Render a Triton **dashboard** (`{title, tiles}`) to PNG/SVG — the
//! `render_a2ui_to_png` capability Triton's chat surface delegates to (#143 D).
//!
//! Triton's `DashboardRequest` carries a title and KPI tiles
//! (`{label, value, trend?}`); peacock lays them out as cards and rasterizes
//! with the same pure-Rust path as the charts (no Node/Deno/network).

use std::fmt::Write as _;

use serde::Deserialize;

use crate::{RasterError, render_svg_to_png};

/// Triton's dashboard spec (the JSON `render_a2ui_to_png` receives).
#[derive(Debug, Clone, Deserialize)]
pub struct DashboardRequest {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub tiles: Vec<Tile>,
}

/// One KPI tile (`triton_core::a2ui::DashboardTile`).
#[derive(Debug, Clone, Deserialize)]
pub struct Tile {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub trend: Option<String>,
}

const COLS: usize = 3;
const TILE_W: f64 = 280.0;
const TILE_H: f64 = 120.0;
const GAP: f64 = 18.0;
const PAD: f64 = 24.0;
const TITLE_H: f64 = 44.0;

/// Lay out the dashboard as an SVG document.
pub fn render_dashboard_to_svg(req: &DashboardRequest) -> String {
    let n = req.tiles.len().max(1);
    let rows = n.div_ceil(COLS);
    let cols = req.tiles.len().clamp(1, COLS);
    let w = PAD * 2.0 + (cols as f64) * TILE_W + ((cols.saturating_sub(1)) as f64) * GAP;
    let h = PAD * 2.0 + TITLE_H + (rows as f64) * TILE_H + ((rows.saturating_sub(1)) as f64) * GAP;

    let mut svg = String::new();
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" font-family="DejaVu Sans, sans-serif">"##
    );
    let _ = write!(
        svg,
        r##"<rect width="{w:.0}" height="{h:.0}" fill="#faf9f8"/>"##
    );

    // Title.
    let _ = write!(
        svg,
        r##"<text x="{PAD}" y="{:.0}" font-size="22" font-weight="bold" fill="#201f1e">{}</text>"##,
        PAD + 26.0,
        escape(&req.title)
    );

    for (i, tile) in req.tiles.iter().enumerate() {
        let r = i / COLS;
        let c = i % COLS;
        let x = PAD + (c as f64) * (TILE_W + GAP);
        let y = PAD + TITLE_H + (r as f64) * (TILE_H + GAP);

        let _ = write!(
            svg,
            r##"<rect x="{x:.1}" y="{y:.1}" width="{TILE_W}" height="{TILE_H}" rx="12" fill="#ffffff" stroke="#e1dfdd"/>"##
        );
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="12" fill="#605e5c" letter-spacing="0.5">{}</text>"##,
            x + 18.0,
            y + 28.0,
            escape(&tile.label.to_uppercase())
        );
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="32" font-weight="bold" fill="#0f6cbd">{}</text>"##,
            x + 18.0,
            y + 70.0,
            escape(&tile.value)
        );
        if let Some(trend) = &tile.trend {
            let color = trend_color(trend);
            let _ = write!(
                svg,
                r##"<text x="{:.1}" y="{:.1}" font-size="13" fill="{color}">{}</text>"##,
                x + 18.0,
                y + 98.0,
                escape(trend)
            );
        }
    }

    svg.push_str("</svg>");
    svg
}

/// Rasterize a dashboard to PNG bytes (`render_a2ui_to_png`).
pub fn render_dashboard_to_png(req: &DashboardRequest, scale: f32) -> Result<Vec<u8>, RasterError> {
    render_svg_to_png(&render_dashboard_to_svg(req), scale)
}

fn trend_color(trend: &str) -> &'static str {
    let t = trend.trim_start();
    if t.starts_with('+') {
        "#3b6e22"
    } else if t.starts_with('-') {
        "#c4314b"
    } else {
        "#605e5c"
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
