//! Render an **instance card** (title + facts + body lines + activity) to
//! PNG/SVG — the chat surface for instance reports (a customer record, a
//! briefing), mirroring the dashboard raster: same pure-Rust SVG →
//! tiny-skia path, same stock palette (so [`crate::apply_dashboard_theme`]
//! re-themes it), hard caps so the card is bounded and byte-reproducible.

use std::fmt::Write as _;

use crate::{RasterError, render_svg_to_png};

/// The composed card content (already view-selected by the render core).
#[derive(Debug, Clone, Default)]
pub struct InstanceCardRequest {
    pub title: String,
    /// e.g. `account · beverages-gmbh`.
    pub subtitle: String,
    pub facts: Vec<(String, String)>,
    /// Flattened body lines (markdown read as plain text).
    pub body_lines: Vec<String>,
    /// Activity entries: `(title, meta/body line)`.
    pub events: Vec<(String, String)>,
}

const W: f64 = 640.0;
const PAD: f64 = 24.0;
const FACT_COLS: usize = 2;
const FACT_W: f64 = (W - PAD * 2.0 - GAP) / 2.0;
const FACT_H: f64 = 58.0;
const GAP: f64 = 14.0;
const LINE_H: f64 = 20.0;
const EVENT_H: f64 = 40.0;

// Hard caps: the card is a bounded summary, never an unbounded render.
const MAX_FACTS: usize = 8;
const MAX_BODY_LINES: usize = 12;
const MAX_EVENTS: usize = 6;
const MAX_CHARS: usize = 76;

/// Truncate to `max` chars on a char boundary, with an ellipsis.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// Lay out the card as an SVG document (stock palette — theme applied by
/// [`crate::apply_dashboard_theme`] string-wise, like the dashboard).
pub fn render_instance_card_to_svg(req: &InstanceCardRequest) -> String {
    let facts: Vec<_> = req.facts.iter().take(MAX_FACTS).collect();
    let body: Vec<_> = req.body_lines.iter().take(MAX_BODY_LINES).collect();
    let events: Vec<_> = req.events.iter().take(MAX_EVENTS).collect();

    let fact_rows = facts.len().div_ceil(FACT_COLS);
    let head_h = 64.0;
    let facts_h = (fact_rows as f64) * (FACT_H + GAP);
    let body_h = if body.is_empty() {
        0.0
    } else {
        (body.len() as f64) * LINE_H + GAP
    };
    let events_h = if events.is_empty() {
        0.0
    } else {
        (events.len() as f64) * EVENT_H + GAP + 20.0
    };
    let h = PAD * 2.0 + head_h + facts_h + body_h + events_h;

    let mut svg = String::new();
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{W:.0}" height="{h:.0}" viewBox="0 0 {W:.0} {h:.0}" font-family="DejaVu Sans, sans-serif">"##
    );
    let _ = write!(
        svg,
        r##"<rect width="{W:.0}" height="{h:.0}" fill="#faf9f8"/>"##
    );

    // Title + subtitle.
    let _ = write!(
        svg,
        r##"<text x="{PAD}" y="{:.0}" font-size="22" font-weight="bold" fill="#201f1e">{}</text>"##,
        PAD + 24.0,
        escape(&clip(&req.title, 48))
    );
    let _ = write!(
        svg,
        r##"<text x="{PAD}" y="{:.0}" font-size="12" fill="#605e5c">{}</text>"##,
        PAD + 44.0,
        escape(&clip(&req.subtitle, MAX_CHARS))
    );

    // Facts grid.
    let facts_y = PAD + head_h;
    for (i, (k, v)) in facts.iter().enumerate() {
        let r = i / FACT_COLS;
        let c = i % FACT_COLS;
        let x = PAD + (c as f64) * (FACT_W + GAP);
        let y = facts_y + (r as f64) * (FACT_H + GAP);
        let _ = write!(
            svg,
            r##"<rect x="{x:.1}" y="{y:.1}" width="{FACT_W:.1}" height="{FACT_H}" rx="10" fill="#ffffff" stroke="#e1dfdd"/>"##
        );
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="11" fill="#605e5c" letter-spacing="0.5">{}</text>"##,
            x + 14.0,
            y + 22.0,
            escape(&clip(&k.to_uppercase(), 28))
        );
        let _ = write!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" font-size="17" font-weight="bold" fill="#0f6cbd">{}</text>"##,
            x + 14.0,
            y + 44.0,
            escape(&clip(v, 34))
        );
    }

    // Body lines.
    let body_y = facts_y + facts_h + GAP;
    for (i, line) in body.iter().enumerate() {
        let _ = write!(
            svg,
            r##"<text x="{PAD}" y="{:.1}" font-size="13" fill="#201f1e">{}</text>"##,
            body_y + (i as f64) * LINE_H,
            escape(&clip(line, MAX_CHARS))
        );
    }

    // Activity.
    if !events.is_empty() {
        let ev_y = body_y + body_h + GAP;
        let _ = write!(
            svg,
            r##"<text x="{PAD}" y="{ev_y:.1}" font-size="12" fill="#605e5c" letter-spacing="0.5">ACTIVITY</text>"##
        );
        for (i, (title, meta)) in events.iter().enumerate() {
            let y = ev_y + 16.0 + (i as f64) * EVENT_H;
            let _ = write!(
                svg,
                r##"<text x="{PAD}" y="{:.1}" font-size="13" font-weight="bold" fill="#201f1e">{}</text>"##,
                y + 14.0,
                escape(&clip(title, 60))
            );
            let _ = write!(
                svg,
                r##"<text x="{PAD}" y="{:.1}" font-size="11.5" fill="#605e5c">{}</text>"##,
                y + 30.0,
                escape(&clip(meta, MAX_CHARS))
            );
        }
    }

    svg.push_str("</svg>");
    svg
}

/// Rasterize an instance card to PNG bytes.
pub fn render_instance_card_to_png(
    req: &InstanceCardRequest,
    scale: f32,
) -> Result<Vec<u8>, RasterError> {
    render_svg_to_png(&render_instance_card_to_svg(req), scale)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> InstanceCardRequest {
        InstanceCardRequest {
            title: "Beverages GmbH".into(),
            subtitle: "account · beverages-gmbh".into(),
            facts: vec![
                ("name".into(), "Beverages GmbH".into()),
                ("status".into(), "follow_up".into()),
            ],
            body_lines: vec!["EU beverages distributor; renewal due in Q3.".into()],
            events: vec![("Email filed".into(), "mail · Renewal question".into())],
        }
    }

    #[test]
    fn card_svg_carries_title_facts_body_and_activity() {
        let svg = render_instance_card_to_svg(&card());
        assert!(svg.contains("Beverages GmbH"));
        assert!(svg.contains("STATUS") && svg.contains("follow_up"));
        assert!(svg.contains("renewal due in Q3"));
        assert!(svg.contains("ACTIVITY") && svg.contains("Email filed"));
        // Stock palette markers — what apply_dashboard_theme re-colors.
        assert!(svg.contains("#0f6cbd") && svg.contains("#faf9f8"));
    }

    #[test]
    fn card_svg_escapes_and_caps() {
        let mut req = card();
        req.title = "<script>alert(1)</script>".into();
        req.body_lines = (0..40)
            .map(|i| format!("line {i} {}", "x".repeat(200)))
            .collect();
        let svg = render_instance_card_to_svg(&req);
        assert!(!svg.contains("<script>"), "escaped");
        assert!(svg.contains("&lt;script&gt;"));
        // Caps: at most MAX_BODY_LINES lines, each clipped with an ellipsis.
        assert_eq!(svg.matches("line ").count(), MAX_BODY_LINES);
        assert!(svg.contains('…'));
    }

    #[test]
    fn card_raster_is_byte_reproducible() {
        let a = render_instance_card_to_png(&card(), 2.0).unwrap();
        let b = render_instance_card_to_png(&card(), 2.0).unwrap();
        assert_eq!(&a[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(a, b);
    }

    #[test]
    fn themed_card_recolors_the_stock_palette() {
        let t = peacock_theme::ThemeTokens {
            brand: "#6b3fa0".into(),
            ..Default::default()
        };
        let themed = crate::apply_dashboard_theme(&render_instance_card_to_svg(&card()), &t);
        assert!(themed.contains("#6b3fa0"), "brand applied");
        assert!(!themed.contains("#0f6cbd"), "stock accent replaced");
    }
}
