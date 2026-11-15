//! The anofox theme bridge (issue #44): resolved `--pk-*` tokens → an
//! anofox-visualization theme JSON object, so the chart engine paints with the
//! same corporate identity ⊕ host look as the web chrome.
//!
//! Host ⊕ brand composition stays in peacock (anofox's `extends` / `a+b` only
//! know its built-ins); this bridge only translates the already-composed
//! tokens. peacock has no dark token set of its own, so the dark variant keeps
//! anofox's dark surfaces and **lifts** the brand colours until they meet
//! WCAG AA on the dark card (4.5:1 for accents that colour text, 3:1 for
//! series marks).

use serde_json::{Value, json};

use crate::ThemeTokens;

/// Which colour variant a render uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    #[default]
    Light,
    Dark,
}

impl Variant {
    /// `"dark"` (any case) → Dark; anything else → Light (theming never fails
    /// a request).
    pub fn parse(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("dark") {
            Variant::Dark
        } else {
            Variant::Light
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Variant::Light => "light",
            Variant::Dark => "dark",
        }
    }
}

/// anofox's built-in dark card — the surface dark-variant colours are
/// measured against.
pub const DARK_CARD: &str = "#141414";
const DARK_BG: &str = "#0a0a0a";
/// WCAG AA for text-coloured accents (KPI values, links, headings).
const AA_TEXT: f64 = 4.5;
/// WCAG AA for non-text graphics (series marks).
const AA_GRAPHIC: f64 = 3.0;

impl ThemeTokens {
    /// The anofox theme definition for these tokens. `variant` selects which
    /// look the definition's `mode` pins; the definition always carries the
    /// lifted `dark` block so both variants resolve.
    pub fn to_anofox_json(&self, variant: Variant) -> Value {
        let palette: Vec<String> = self.palette.iter().map(|c| norm_hex(c)).collect();
        let brand = norm_hex(&self.brand);
        let accent = norm_hex(&self.accent);
        let text = norm_hex(&self.text);
        let muted = norm_hex(&self.muted);
        let border = norm_hex(&self.border);
        let surface = norm_hex(&self.surface);

        let dark_brand = lift_to_contrast(&brand, DARK_CARD, AA_TEXT);
        let dark_accent = lift_to_contrast(&accent, DARK_CARD, AA_TEXT);
        let dark_palette = lift_palette(&palette, DARK_CARD, AA_GRAPHIC);

        let mut def = json!({
            "name": self.name.clone().unwrap_or_else(|| "peacock".into()),
            "extends": "light",
            "mode": variant.as_str(),
            "palette": palette,
            "sequential": [norm_hex(&self.bg), brand],
            "colors": {
                "bg": norm_hex(&self.bg),
                "bg2": norm_hex(&self.bg),
                "card": surface,
                "ink": text,
                "heading": text,
                "muted": muted,
                "line": border,
                "accent": brand,
                "accent2": accent,
                "info": brand,
                "link": brand,
            },
            "chart": {
                "heading": text,
                "ink": text,
                "muted": muted,
                "note": muted,
                "line": norm_hex(&self.grid),
            },
            "font": { "family": self.font, "display": self.font },
            "radius": self.radius,
            "brand": {},
            "dark": {
                "palette": dark_palette,
                "colors": {
                    "bg": DARK_BG,
                    "bg2": DARK_BG,
                    "card": DARK_CARD,
                    "accent": dark_brand,
                    "accent2": dark_accent,
                    "info": dark_brand,
                    "link": dark_brand,
                },
            },
        });
        if let Some(name) = &self.name {
            def["brand"]["name"] = json!(name);
        }
        def
    }
}

/// WCAG 2.x contrast ratio of two `#rrggbb` colours (`None` if unparsable).
pub fn contrast_ratio(a: &str, b: &str) -> Option<f64> {
    let la = luminance(parse_hex(a)?);
    let lb = luminance(parse_hex(b)?);
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    Some((hi + 0.05) / (lo + 0.05))
}

/// Move `fg` toward the pole opposite `bg`'s luminance (lighter on a dark
/// background, darker on a light one) in HSL lightness — hue and saturation
/// kept — until it reaches `min` contrast. A colour that already passes, or
/// one that cannot be parsed, is returned unchanged (normalised).
pub fn lift_to_contrast(fg: &str, bg: &str, min: f64) -> String {
    let (Some(f), Some(b)) = (parse_hex(fg), parse_hex(bg)) else {
        return fg.to_string();
    };
    if contrast_ratio(&hex(f), &hex(b)).unwrap_or(0.0) >= min {
        return hex(f);
    }
    let (h, s, l) = rgb_to_hsl(f);
    let lighten = luminance(b) < 0.5;
    let mut l2 = l;
    for _ in 0..200 {
        l2 = if lighten {
            (l2 + 0.005).min(1.0)
        } else {
            (l2 - 0.005).max(0.0)
        };
        let c = hsl_to_rgb(h, s, l2);
        if contrast_ratio(&hex(c), &hex(b)).unwrap_or(0.0) >= min || l2 <= 0.0 || l2 >= 1.0 {
            return hex(c);
        }
    }
    hex(hsl_to_rgb(h, s, l2))
}

/// Smallest RGB distance two series colours keep after lifting.
const MIN_SERIES_DISTANCE: f64 = 24.0;

/// Lift every palette colour to `min` contrast on `bg`, keeping the series
/// distinguishable: two dark shades of one hue lift to nearly the same
/// colour, so a colour that lands too close to an earlier one keeps moving
/// in lightness (then, at the pole, the other way) until it is distinct.
fn lift_palette(palette: &[String], bg: &str, min: f64) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(palette.len());
    let Some(b) = parse_hex(bg) else {
        return palette.to_vec();
    };
    let lighten = luminance(b) < 0.5;
    for c in palette {
        let lifted = lift_to_contrast(c, bg, min);
        let Some(rgb) = parse_hex(&lifted) else {
            out.push(lifted);
            continue;
        };
        let clashes = |x: Rgb, out: &[String]| {
            out.iter()
                .filter_map(|o| parse_hex(o))
                .any(|o| distance(o, x) < MIN_SERIES_DISTANCE)
        };
        if !clashes(rgb, &out) {
            out.push(lifted);
            continue;
        }
        let (h, s, l) = rgb_to_hsl(rgb);
        let mut found = None;
        for dir in [
            if lighten { 1.0 } else { -1.0 },
            if lighten { -1.0 } else { 1.0 },
        ] {
            let mut l2 = l;
            while (0.0..=1.0).contains(&l2) {
                l2 += dir * 0.01;
                let cand = hsl_to_rgb(h, s, l2.clamp(0.0, 1.0));
                if contrast_ratio(&hex(cand), bg).unwrap_or(0.0) >= min && !clashes(cand, &out) {
                    found = Some(hex(cand));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        out.push(found.unwrap_or(lifted));
    }
    out
}

fn distance(a: Rgb, b: Rgb) -> f64 {
    let d = |x: u8, y: u8| (x as f64 - y as f64).powi(2);
    (d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)).sqrt()
}

type Rgb = (u8, u8, u8);

fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.trim().strip_prefix('#')?;
    let h: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

fn hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `#ABC` / `#AABBCC` → `#aabbcc`; anything else passes through unchanged
/// (anofox rejects it with a clear message rather than peacock guessing).
fn norm_hex(s: &str) -> String {
    parse_hex(s)
        .map(hex)
        .unwrap_or_else(|| s.trim().to_string())
}

fn luminance((r, g, b): Rgb) -> f64 {
    let ch = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

fn rgb_to_hsl((r, g, b): Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h * 6.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r1), to(g1), to(b1))
}
