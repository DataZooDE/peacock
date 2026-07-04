//! peacock theming — **one CSS file is the single source of truth**.
//!
//! A theme is a set of `--pk-*` CSS custom properties. Web surfaces (the
//! iframe / Flutter / demo) consume the CSS natively (the cascade does the
//! work); peacock extracts the same tokens to style the server-rendered chart
//! PNG. So a chart and the chrome around it always match, from one definition.
//!
//! The look is a **composition of two layers** — the host application's
//! look-and-feel (Copilot Studio / WhatsApp / Gemini) under the company's
//! corporate identity (brand):
//!
//! ```text
//! theme(tenant, host) = host-flavor CSS  ⊕  brand CSS   (brand wins the cascade)
//! ```
//!
//! Brand CSS is small and **agent-authorable** (a styling agent emits the
//! `--pk-*` declarations — e.g. from a logo via the palette extractor in
//! `peacock-rasterizer`); peacock applies it deterministically.

use std::collections::BTreeMap;

mod registry;
pub use registry::ThemeRegistry;

/// How a theme's `--pk-logo` is placed by chrome consumers (the chat-card
/// header, web headers). Peacock owns ALL theming — chat adapters fetch the
/// resolved theme (`get_theme`) instead of carrying their own brand config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogoStyle {
    /// Small round image in the card header (square icon logos).
    #[default]
    Avatar,
    /// Full-width image at the top of the card (wide wordmark logos).
    Banner,
}

impl LogoStyle {
    /// The wire name (`--pk-logo-style` value and the `get_theme` JSON).
    pub fn as_str(&self) -> &'static str {
        match self {
            LogoStyle::Avatar => "avatar",
            LogoStyle::Banner => "banner",
        }
    }

    /// Parse a `--pk-logo-style` value; anything unrecognised is the default
    /// (theming never fails a request).
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "banner" => LogoStyle::Banner,
            _ => LogoStyle::Avatar,
        }
    }
}

/// The token contract peacock reads to style a chart. Every field has a
/// sensible default (the stock peacock look), so a theme need only override
/// what it cares about.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeTokens {
    /// `--pk-font` — font family (web honours it fully; the chart falls back to
    /// a vendored face if the family is unavailable to the rasterizer).
    pub font: String,
    /// `--pk-brand` — primary brand colour (single-series marks, KPI accents).
    pub brand: String,
    /// `--pk-accent` — secondary accent.
    pub accent: String,
    /// `--pk-bg` — chart/page background.
    pub bg: String,
    /// `--pk-surface` — card/tile background.
    pub surface: String,
    /// `--pk-text` — primary text.
    pub text: String,
    /// `--pk-muted` — secondary text (axis labels, captions).
    pub muted: String,
    /// `--pk-border` — hairlines / card borders.
    pub border: String,
    /// `--pk-grid` — chart gridlines.
    pub grid: String,
    /// `--pk-axis` — chart axis lines.
    pub axis: String,
    /// `--pk-radius` — corner radius in px.
    pub radius: f64,
    /// `--pk-cat-1..N` — the categorical series palette.
    pub palette: Vec<String>,
    /// `--pk-logo` — a logo URL/data-URI (web chrome).
    pub logo: Option<String>,
    /// `--pk-name` — the company display name.
    pub name: Option<String>,
    /// `--pk-logo-style` — how chrome consumers place the logo.
    pub logo_style: LogoStyle,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self {
            font: "\"DejaVu Sans\", system-ui, sans-serif".into(),
            brand: "#0f6cbd".into(),
            accent: "#22d3c5".into(),
            bg: "#ffffff".into(),
            surface: "#f4f9fe".into(),
            text: "#201f1e".into(),
            muted: "#605e5c".into(),
            border: "#e1dfdd".into(),
            grid: "#e6e6e6".into(),
            axis: "#888888".into(),
            radius: 12.0,
            palette: tableau10(),
            logo: None,
            name: None,
            logo_style: LogoStyle::default(),
        }
    }
}

fn tableau10() -> Vec<String> {
    [
        "#4c78a8", "#f58518", "#54a24b", "#e45756", "#72b7b2", "#ff9da6", "#9d755d", "#bab0ac",
        "#e377c2", "#17becf",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl ThemeTokens {
    /// Build tokens from a parsed `--pk-*` map, over the defaults.
    pub fn from_vars(vars: &BTreeMap<String, String>) -> Self {
        let mut t = ThemeTokens::default();
        let get = |k: &str| vars.get(k).cloned();
        if let Some(v) = get("font") {
            t.font = v;
        }
        if let Some(v) = get("brand") {
            t.brand = v;
        }
        if let Some(v) = get("accent") {
            t.accent = v;
        }
        if let Some(v) = get("bg") {
            t.bg = v;
        }
        if let Some(v) = get("surface") {
            t.surface = v;
        }
        if let Some(v) = get("text") {
            t.text = v;
        }
        if let Some(v) = get("muted") {
            t.muted = v;
        }
        if let Some(v) = get("border") {
            t.border = v;
        }
        if let Some(v) = get("grid") {
            t.grid = v;
        }
        if let Some(v) = get("axis") {
            t.axis = v;
        }
        if let Some(v) = get("radius") {
            if let Some(px) = v.trim().strip_suffix("px") {
                if let Ok(n) = px.trim().parse() {
                    t.radius = n;
                }
            } else if let Ok(n) = v.trim().parse() {
                t.radius = n;
            }
        }
        t.logo = get("logo");
        t.name = get("name").map(|s| unquote(&s));
        if let Some(v) = get("logo-style") {
            t.logo_style = LogoStyle::parse(&v);
        }
        // Categorical palette: --pk-cat-1, --pk-cat-2, … in order.
        let mut palette = Vec::new();
        for i in 1.. {
            match get(&format!("cat-{i}")) {
                Some(c) => palette.push(c),
                None => break,
            }
        }
        if !palette.is_empty() {
            t.palette = palette;
        }
        t
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(s)
        .to_string()
}

/// Parse `--pk-NAME: VALUE;` custom-property declarations from CSS. Keys are
/// returned without the `--pk-` prefix; later declarations win (the cascade).
/// Comments (`/* … */`) are stripped. No full CSS parser is needed — we only
/// read the design tokens.
pub fn parse_vars(css: &str) -> BTreeMap<String, String> {
    let css = strip_comments(css);
    let mut out = BTreeMap::new();
    let bytes = css.as_bytes();
    let mut i = 0;
    while let Some(rel) = css[i..].find("--pk-") {
        let start = i + rel + "--pk-".len();
        // name up to ':'
        let Some(colon_rel) = css[start..].find(':') else {
            break;
        };
        let name = css[start..start + colon_rel].trim().to_string();
        let val_start = start + colon_rel + 1;
        // value up to ';' or '}'
        let end_rel = css[val_start..]
            .find([';', '}'])
            .unwrap_or(css.len() - val_start);
        let value = css[val_start..val_start + end_rel].trim().to_string();
        if !name.is_empty() {
            out.insert(name, value);
        }
        i = val_start + end_rel;
        if i >= bytes.len() {
            break;
        }
    }
    out
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start..].find("*/") {
            Some(end) => rest = &rest[start + end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Compose `host` CSS under `brand` CSS — concatenation is enough because the
/// brand's `:root { --pk-* }` declarations come last and win the cascade (and
/// `from_vars` likewise takes the last value).
pub fn compose(host_css: &str, brand_css: &str) -> String {
    format!(
        "/* peacock theme — host flavor */\n{}\n/* brand overlay */\n{}\n",
        host_css.trim(),
        brand_css.trim()
    )
}

/// A fully resolved theme: the composed CSS for web surfaces, and the extracted
/// tokens peacock uses to style the chart.
#[derive(Debug, Clone)]
pub struct Theme {
    pub host: String,
    pub brand: String,
    pub css: String,
    pub tokens: ThemeTokens,
}

impl Theme {
    /// Resolve a theme from already-loaded host + brand CSS.
    pub fn from_css(host: &str, brand: &str, host_css: &str, brand_css: &str) -> Self {
        let css = compose(host_css, brand_css);
        let tokens = ThemeTokens::from_vars(&parse_vars(&css));
        Theme {
            host: host.to_string(),
            brand: brand.to_string(),
            css,
            tokens,
        }
    }
}
