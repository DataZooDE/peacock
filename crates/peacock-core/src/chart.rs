//! The chart engine: anofox-visualization (issue #44).
//!
//! An authored `kind: anofox` spec names **roles** over the view's escurel
//! columns:
//!
//! ```yaml
//! specs:
//!   rev:
//!     title: Revenue by category
//!     roles:
//!       - [month, XAXIS]
//!       - [category, CATEGORY]
//!       - [revenue, BARCHART_STACKED, Revenue]   # optional display name
//! ```
//!
//! A role's column is a schema column **name** or a 0-based schema **index**.
//! [`panel_spec`] turns that plus the rows into anofox's panel JSON (rows keyed
//! `c{i}` by role position, `[[i, ROLE, name]]`); [`render_svg`] renders it
//! themed, through anofox's checked host entry point with size caps; and
//! [`svg_to_png`] rasterizes with bundled fonts. Planning and rendering are
//! pure — no SQL, no data access; the rows are what escurel released.
//!
//! Guardrails (ACC-4, [`check_spec`]): only roles from anofox's registry, none
//! of the layout / input / chrome roles (they draw nothing inside a panel —
//! the silent-drop slip), no `LINK`/`IMAGE`/download roles, and no authored
//! `rows` / `theme` / `plot` / URL keys (peacock injects rows and theme
//! itself). The rendered SVG is checked inert afterwards as defence in depth.

use std::sync::{Arc, OnceLock};

use peacock_types::{Error, Result};
use serde_json::{Map, Value, json};

use crate::data::RowSet;

pub use peacock_theme::Variant;

/// The authored keys a spec may carry. Anything else is rejected (an
/// unknown key is a typo or an escape hatch, never silently ignored).
const AUTHORED_KEYS: &[&str] = &[
    "roles",
    "title",
    "width",
    "height",
    "max_categories",
    "max_line_points",
];

/// Role categories that draw nothing inside a single panel.
const PANEL_FORBIDDEN_CATEGORIES: &[&str] = &["input", "layout", "chrome"];

/// Roles that fetch or navigate (an href, a remote image, a download button).
const FORBIDDEN_ROLES: &[&str] = &[
    "LINK",
    "IMAGE",
    "DOWNLOAD_CSV",
    "DOWNLOAD_JSON",
    "DOWNLOAD_PDF",
    "DOWNLOAD_XLSX",
    "CLASS",
];

/// How a chart is painted: the resolved `--pk-*` tokens (corporate identity ⊕
/// host look), the light/dark variant, and the number/date locale.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChartLook {
    pub theme: Option<peacock_theme::ThemeTokens>,
    pub variant: Variant,
    pub locale: Option<String>,
}

/// One authored role: the escurel column it reads, the anofox role token, and
/// an optional display name.
#[derive(Debug, Clone, PartialEq)]
struct RoleRef {
    column: ColumnRef,
    token: String,
    name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum ColumnRef {
    Name(String),
    Index(usize),
}

/// Validate an authored spec against the safe subset and the view's columns.
pub fn check_spec(spec: &Value, columns: &[&str]) -> Result<()> {
    check_spec_shape(spec)?;
    for r in parse_roles(spec)? {
        match &r.column {
            ColumnRef::Name(n) if !columns.contains(&n.as_str()) => {
                return Err(Error::render(format!(
                    "anofox role `{}` names unknown column `{n}` (columns: {})",
                    r.token,
                    columns.join(", ")
                )));
            }
            ColumnRef::Index(i) if *i >= columns.len() => {
                return Err(Error::render(format!(
                    "anofox role `{}` names column index {i}, but the view has {} columns",
                    r.token,
                    columns.len()
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate an authored spec's shape without the rows: allowed keys, known
/// and permitted roles. Used at parse/author time, where no columns exist yet.
pub fn check_spec_shape(spec: &Value) -> Result<()> {
    let obj = spec
        .as_object()
        .ok_or_else(|| Error::render("anofox spec must be a mapping".to_owned()))?;
    for key in obj.keys() {
        if !AUTHORED_KEYS.contains(&key.as_str()) {
            return Err(Error::render(format!(
                "anofox spec key `{key}` is not allowed (allowed: {}); peacock injects rows and \
                 theme itself",
                AUTHORED_KEYS.join(", ")
            )));
        }
    }
    if let Some(t) = obj.get("title")
        && !t.is_string()
    {
        return Err(Error::render(
            "anofox spec `title` must be a string".to_owned(),
        ));
    }
    for k in ["width", "height", "max_categories", "max_line_points"] {
        if let Some(v) = obj.get(k)
            && !v.is_u64()
        {
            return Err(Error::render(format!(
                "anofox spec `{k}` must be a positive integer"
            )));
        }
    }
    let roles = parse_roles(spec)?;
    if roles.is_empty() {
        return Err(Error::render(
            "anofox spec needs at least one role".to_owned(),
        ));
    }
    for r in &roles {
        let rs = anofox_visualization::roles::lookup(&r.token)
            .ok_or_else(|| Error::render(format!("unknown anofox role `{}`", r.token)))?;
        if PANEL_FORBIDDEN_CATEGORIES.contains(&rs.category) {
            return Err(Error::render(format!(
                "anofox role `{}` is a {} directive and draws nothing inside a chart panel",
                r.token, rs.category
            )));
        }
        if FORBIDDEN_ROLES.contains(&rs.token) {
            return Err(Error::render(format!(
                "anofox role `{}` is not allowed in a peacock report (it fetches or navigates)",
                r.token
            )));
        }
    }
    Ok(())
}

/// The escurel column name a spec's `CATEGORY` role reads, if any (the
/// series a drill narrows; picks `spec_single`).
pub fn category_column(spec: &Value, columns: &[&str]) -> Option<String> {
    parse_roles(spec).ok()?.into_iter().find_map(|r| {
        let canonical = anofox_visualization::roles::lookup(&r.token)?.token;
        (canonical == "CATEGORY").then(|| match r.column {
            ColumnRef::Name(n) => Some(n),
            ColumnRef::Index(i) => columns.get(i).map(|c| (*c).to_owned()),
        })?
    })
}

fn parse_roles(spec: &Value) -> Result<Vec<RoleRef>> {
    let arr = spec.get("roles").and_then(Value::as_array).ok_or_else(|| {
        Error::render("anofox spec needs `roles: [[column, ROLE, name?], …]`".to_owned())
    })?;
    arr.iter()
        .map(|e| {
            let bad = || {
                Error::render(format!(
                    "bad anofox role entry {e} (want [column, ROLE, name?])"
                ))
            };
            let a = e.as_array().ok_or_else(bad)?;
            let column = match a.first() {
                Some(Value::String(s)) => ColumnRef::Name(s.clone()),
                Some(Value::Number(n)) => ColumnRef::Index(n.as_u64().ok_or_else(bad)? as usize),
                _ => return Err(bad()),
            };
            let token = a.get(1).and_then(Value::as_str).ok_or_else(bad)?.to_owned();
            let name = a.get(2).and_then(Value::as_str).map(str::to_owned);
            Ok(RoleRef {
                column,
                token,
                name,
            })
        })
        .collect()
}

/// Build anofox's panel spec from an authored spec and the view's rows: rows
/// re-keyed `c{i}` by role position, roles as `[i, TOKEN, name]`, the title as
/// a trailing `TITLE` role (anofox draws it as the card's title bar). Checks
/// the spec first.
pub fn panel_spec(spec: &Value, rs: &RowSet) -> Result<Value> {
    let columns: Vec<&str> = rs.schema.iter().map(|c| c.name.as_str()).collect();
    check_spec(spec, &columns)?;
    let roles = parse_roles(spec)?;
    let names: Vec<String> = roles
        .iter()
        .map(|r| match &r.column {
            ColumnRef::Name(n) => n.clone(),
            ColumnRef::Index(i) => columns[*i].to_owned(),
        })
        .collect();
    let title = spec.get("title").and_then(Value::as_str);

    let src = rs.rows.as_array().map(Vec::as_slice).unwrap_or_default();
    let rows: Vec<Value> = src
        .iter()
        .map(|row| {
            let mut out = Map::new();
            for (i, col) in names.iter().enumerate() {
                out.insert(
                    format!("c{i}"),
                    row.get(col).cloned().unwrap_or(Value::Null),
                );
            }
            if let Some(t) = title {
                out.insert(format!("c{}", names.len()), json!(t));
            }
            Value::Object(out)
        })
        .collect();

    let mut role_entries: Vec<Value> = roles
        .iter()
        .zip(&names)
        .enumerate()
        .map(|(i, (r, col))| {
            let canonical = anofox_visualization::roles::lookup(&r.token)
                .map(|s| s.token)
                .unwrap_or(r.token.as_str());
            json!([i, canonical, r.name.clone().unwrap_or_else(|| col.clone())])
        })
        .collect();
    if title.is_some() {
        role_entries.push(json!([names.len(), "TITLE", ""]));
    }

    let mut panel = json!({ "rows": rows, "roles": role_entries });
    for k in ["width", "height", "max_categories", "max_line_points"] {
        if let Some(v) = spec.get(k) {
            panel[k] = v.clone();
        }
    }
    Ok(panel)
}

/// Parse the caller-facing look knobs (`variant`, `locale`) a surface
/// receives. An unknown variant or locale is a validation error — never a
/// silent fall back to the light, `en-US` picture.
pub fn parse_look_args(
    variant: Option<&str>,
    locale: Option<&str>,
) -> Result<(Variant, Option<String>)> {
    let v = match variant.map(str::trim) {
        None | Some("") | Some("light") => Variant::Light,
        Some("dark") => Variant::Dark,
        Some(other) => {
            return Err(Error::validation(format!(
                "`variant` must be \"light\" or \"dark\", got {other:?}"
            )));
        }
    };
    let l = match locale.map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(tag) => {
            if anofox_visualization::format::Locale::from_tag(tag).is_none() {
                return Err(Error::validation(format!(
                    "unknown `locale` {tag:?} (known: {})",
                    anofox_visualization::format::Locale::TAGS.join(", ")
                )));
            }
            Some(tag.to_owned())
        }
    };
    Ok((v, l))
}

/// The anofox panel spec with the look applied (theme object, variant,
/// locale) — exactly the JSON handed to the renderer.
pub fn themed_spec(panel: &Value, look: &ChartLook) -> Value {
    let mut spec = panel.clone();
    if let Some(t) = &look.theme {
        spec["theme"] = t.to_anofox_json(look.variant);
    }
    spec["variant"] = json!(look.variant.as_str());
    if let Some(l) = &look.locale {
        spec["locale"] = json!(l);
    }
    spec
}

/// Render a panel spec to SVG through anofox's checked host entry point
/// (row / size / dimension caps), then assert the output is inert.
pub fn render_svg(panel: &Value, look: &ChartLook) -> Result<String> {
    let spec = themed_spec(panel, look);
    let limits = anofox_visualization::host::RenderLimits {
        max_rows: crate::compose::DEFAULT_MAX_ROWS,
        ..Default::default()
    };
    let svg = anofox_visualization::host::render_spec_checked(&spec.to_string(), &limits)
        .map_err(|e| Error::render(format!("anofox render: {e}")))?;
    check_inert(&svg)?;
    Ok(svg)
}

/// The rendered SVG must not script, embed foreign content or fetch.
pub fn check_inert(svg: &str) -> Result<()> {
    let lower = svg.to_ascii_lowercase();
    for bad in [
        "<script",
        "foreignobject",
        "javascript:",
        "href=\"http",
        "href='http",
        "href=\"//",
        "<iframe",
        "@import",
    ] {
        if lower.contains(bad) {
            return Err(Error::render(format!(
                "anofox output is not inert (contains `{bad}`)"
            )));
        }
    }
    Ok(())
}

// ── PNG ─────────────────────────────────────────────────────────────────────

/// Vendored faces: Hanken Grotesk (anofox's default family, OFL), DejaVu
/// Sans/Serif (generic fallbacks) and DejaVu Sans Symbols (glyph coverage).
const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-SemiBold.ttf"),
    include_bytes!("../assets/fonts/DejaVuSans.ttf"),
    include_bytes!("../assets/fonts/DejaVuSerif.ttf"),
    include_bytes!("../assets/fonts/DejaVuSans-Symbols.ttf"),
];

fn font_db() -> &'static Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for f in FONTS {
            db.load_font_data(f.to_vec());
        }
        // Installed fonts let a brand's real corporate font resolve. Offline
        // (no network); opt out for byte-reproducible renders across hosts.
        if std::env::var_os("PEACOCK_NO_SYSTEM_FONTS").is_none() {
            db.load_system_fonts();
        }
        db.set_sans_serif_family("Hanken Grotesk");
        db.set_serif_family("DejaVu Serif");
        Arc::new(db)
    })
}

/// Rasterize a chart onto its variant's card colour — the PNG a chat client
/// shows. anofox's SVG is transparent (the page supplies the card), so a
/// transparent dark-variant PNG would put light text on whatever the client
/// paints behind it.
pub fn svg_to_png_on(svg: &str, scale: f32, look: &ChartLook) -> Result<Vec<u8>> {
    let card = match look.variant {
        Variant::Dark => peacock_theme::DARK_CARD.to_owned(),
        Variant::Light => look
            .theme
            .as_ref()
            .map(|t| t.surface.clone())
            .unwrap_or_else(|| "#ffffff".to_owned()),
    };
    rasterize(svg, scale, Some(&card))
}

/// Rasterize an SVG document to PNG at `scale` (≥ 1.0), transparent. Only
/// `data:` images resolve (usvg is given no resource directory, so nothing is
/// read from disk or the network).
pub fn svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>> {
    rasterize(svg, scale, None)
}

fn rasterize(svg: &str, scale: f32, background: Option<&str>) -> Result<Vec<u8>> {
    let mut opt = usvg::Options {
        font_family: "Hanken Grotesk".to_string(),
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            // A non-`data:` href (a path or URL) never resolves.
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    opt.fontdb = font_db().clone();
    let tree =
        usvg::Tree::from_str(svg, &opt).map_err(|e| Error::render(format!("svg parse: {e}")))?;
    let scale = scale.max(1.0);
    let size = tree.size();
    let w = (size.width() * scale).ceil().max(1.0) as u32;
    let h = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| Error::render("could not allocate pixmap".to_owned()))?;
    if let Some((r, g, b)) = background.and_then(parse_rgb) {
        pixmap.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
    }
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| Error::render(format!("png encode: {e}")))
}

// ── derived Vega-Lite (compatibility output) ────────────────────────────────

/// A minimal Vega-Lite spec derived from a panel, for consumers that read
/// Vega-Lite natively: Triton's Teams `Chart.VerticalBar` mapping, Gemini
/// Enterprise's `VegaChart`, and Mosaic mode. Only bar / line / area /
/// scatter / pie / donut panels derive (`XAXIS` + optional `CATEGORY` /
/// `SIZE` + one value role);
/// everything else is `None` and those surfaces show the PNG. Field names are
/// the roles' display names, so the native chart's labels read like the PNG.
pub fn derive_vega_lite(panel: &Value) -> Option<Value> {
    let roles = panel.get("roles")?.as_array()?;
    let mut x: Option<(usize, String)> = None;
    let mut cat: Option<(usize, String)> = None;
    let mut size: Option<(usize, String)> = None;
    let mut val: Option<(usize, String, &'static str)> = None;
    let mut title: Option<usize> = None;
    for r in roles {
        let i = r.get(0)?.as_u64()? as usize;
        let tok = r.get(1)?.as_str()?;
        let name = r
            .get(2)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let canonical = anofox_visualization::roles::lookup(tok)?.token;
        match canonical {
            "XAXIS" => x = Some((i, name)),
            "CATEGORY" => cat = Some((i, name)),
            "SIZE" => size = Some((i, name)),
            "TITLE" => title = Some(i),
            "BARCHART" | "BARCHART_STACKED" | "LINECHART" | "AREACHART" | "AREACHART_STACKED"
            | "SCATTER" | "PIE" | "DONUTCHART"
                if val.is_none() =>
            {
                val = Some((i, name, canonical));
            }
            // Any other role (a second measure, a modifier, a heatmap axis)
            // means the panel is richer than the minimal spec can say.
            _ => return None,
        }
    }
    let (vi, vname, kind) = val?;
    let rows = panel.get("rows")?.as_array()?;
    let key = |i: usize| format!("c{i}");
    let values: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut out = Map::new();
            let mut put = |slot: &Option<(usize, String)>| {
                if let Some((i, n)) = slot {
                    out.insert(n.clone(), row.get(key(*i)).cloned().unwrap_or(Value::Null));
                }
            };
            put(&x);
            put(&cat);
            put(&size);
            out.insert(
                vname.clone(),
                row.get(key(vi)).cloned().unwrap_or(Value::Null),
            );
            Value::Object(out)
        })
        .collect();
    let title_text = title
        .and_then(|i| rows.first().and_then(|r| r.get(key(i))))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut spec = json!({
        "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
        "title": title_text,
        "data": { "values": values },
    });
    match kind {
        "PIE" | "DONUTCHART" => {
            let (_, cname) = cat.as_ref()?;
            let mut mark = json!({ "type": "arc" });
            if kind == "DONUTCHART" {
                mark["innerRadius"] = json!(50);
            }
            spec["mark"] = mark;
            spec["encoding"] = json!({
                "theta": { "field": vname, "type": "quantitative" },
                "color": { "field": cname, "type": "nominal" },
            });
        }
        _ => {
            let (_, xname) = x.as_ref()?;
            let mark = match kind {
                "LINECHART" => "line",
                "AREACHART" | "AREACHART_STACKED" => "area",
                "SCATTER" => "point",
                _ => "bar",
            };
            spec["mark"] = json!({ "type": mark });
            let mut y = json!({ "field": vname, "type": "quantitative" });
            if !matches!(kind, "BARCHART_STACKED" | "AREACHART_STACKED" | "SCATTER") {
                y["stack"] = Value::Null;
            }
            let x_type = if kind == "SCATTER" {
                "quantitative"
            } else {
                "ordinal"
            };
            let mut enc = json!({
                "x": { "field": xname, "type": x_type },
                "y": y,
            });
            if let Some((_, cname)) = &cat {
                enc["color"] = json!({ "field": cname, "type": "nominal" });
            }
            if let Some((_, sname)) = &size {
                enc["size"] = json!({ "field": sname, "type": "quantitative" });
            }
            spec["encoding"] = enc;
        }
    }
    Some(spec)
}

fn parse_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().strip_prefix('#')?;
    let h: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_owned(),
        _ => return None,
    };
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

// ── non-chart images: KPI tiles and the instance card ──────────────────────

/// One KPI tile of Triton's `render_a2ui_to_png` dashboard.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Tile {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub trend: Option<String>,
}

/// A chartless instance report's preview: the record's title, facts, body and
/// recent activity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstanceCard {
    pub title: String,
    pub subtitle: String,
    pub facts: Vec<(String, String)>,
    pub body_lines: Vec<String>,
    pub events: Vec<(String, String)>,
}

const TILE_W: u64 = 280;
const TILE_H: u64 = 110;

/// Triton's KPI dashboard as anofox `TEXT_LARGE` tiles (each captioned with a
/// `LABEL`), three per row under the dashboard title.
pub fn kpi_tiles_svg(title: &str, tiles: &[Tile], look: &ChartLook) -> Result<String> {
    let panels = tiles
        .iter()
        .map(|t| {
            let caption = match &t.trend {
                Some(tr) if !tr.is_empty() => format!("{} · {tr}", t.label),
                _ => t.label.clone(),
            };
            let spec = json!({
                "rows": [{ "c0": t.value, "c1": caption }],
                "roles": [[0, "TEXT_LARGE", ""], [1, "LABEL", ""]],
                "width": TILE_W,
                "height": TILE_H,
            });
            render_svg(&spec, look)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(grid_svg(Some(title), &panels, 3, look))
}

/// The instance card as one anofox `MARKDOWN` panel under a `TITLE` bar.
/// Every value is page content (untrusted): anofox escapes it into SVG text.
pub fn instance_card_svg(card: &InstanceCard, look: &ChartLook) -> Result<String> {
    // Markdown structure characters in page content would be interpreted;
    // keep each fact/event on its own list line and strip leading markers.
    let clean = |s: &str| {
        s.trim()
            .trim_start_matches(['#', '-', '*', '>'])
            .trim()
            .to_owned()
    };
    let mut md = Vec::new();
    if !card.subtitle.is_empty() {
        md.push(clean(&card.subtitle));
        md.push(String::new());
    }
    for (k, v) in &card.facts {
        md.push(format!("- {}: {}", clean(k), clean(v)));
    }
    if !card.body_lines.is_empty() {
        md.push(String::new());
        md.extend(card.body_lines.iter().map(|l| clean(l)));
    }
    if !card.events.is_empty() {
        md.push(String::new());
        md.push("Activity".to_owned());
        for (t, meta) in &card.events {
            md.push(format!("- {} · {}", clean(t), clean(meta)));
        }
    }
    let lines = md.len() as u64;
    let spec = json!({
        "rows": [{ "c0": md.join("\n"), "c1": clean(&card.title) }],
        "roles": [[0, "MARKDOWN", ""], [1, "TITLE", ""]],
        "width": 720,
        "height": (60 + lines * 20).min(2000),
    });
    let panel = render_svg(&spec, look)?;
    Ok(grid_svg(None, &[panel], 1, look))
}

/// Lay rendered panels out as cards on a page in `columns` columns, with an
/// optional page title. Each panel is nested as its own `<svg>`, so its
/// coordinates stay its own.
fn grid_svg(title: Option<&str>, panels: &[String], columns: usize, look: &ChartLook) -> String {
    const PAD: u64 = 16;
    const GAP: u64 = 12;
    let (bg, card, line, ink) = page_colours(look);
    let dims: Vec<(u64, u64)> = panels.iter().map(|p| svg_size(p)).collect();
    let columns = columns.max(1).min(panels.len().max(1));
    let col_w = dims.iter().map(|d| d.0).max().unwrap_or(TILE_W);
    let title_h = if title.is_some() { 40 } else { 0 };
    let mut rows_h = Vec::new();
    for chunk in dims.chunks(columns) {
        rows_h.push(chunk.iter().map(|d| d.1).max().unwrap_or(0) + 2 * PAD);
    }
    let w = 2 * PAD + columns as u64 * (col_w + 2 * PAD) + (columns as u64 - 1) * GAP;
    let h = 2 * PAD
        + title_h
        + rows_h.iter().sum::<u64>()
        + rows_h.len().saturating_sub(1) as u64 * GAP;
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="{bg}"/>"#
    );
    if let Some(t) = title {
        out.push_str(&format!(
            r#"<text x="{PAD}" y="{}" font-family="Hanken Grotesk, sans-serif" font-size="18" font-weight="600" fill="{ink}">{}</text>"#,
            PAD + 20,
            escape_xml(t)
        ));
    }
    let mut y = PAD + title_h;
    for (r, chunk) in panels.chunks(columns).enumerate() {
        for (c, panel) in chunk.iter().enumerate() {
            let x = PAD + c as u64 * (col_w + 2 * PAD + GAP);
            out.push_str(&format!(
                r#"<rect x="{x}" y="{y}" width="{}" height="{}" rx="10" fill="{card}" stroke="{line}"/>"#,
                col_w + 2 * PAD,
                rows_h[r]
            ));
            out.push_str(&nest(panel, x + PAD, y + PAD));
        }
        y += rows_h[r] + GAP;
    }
    out.push_str("</svg>");
    out
}

/// The page background, card, hairline and ink colours of a look.
fn page_colours(look: &ChartLook) -> (String, String, String, String) {
    match look.variant {
        Variant::Dark => (
            "#0a0a0a".into(),
            peacock_theme::DARK_CARD.into(),
            "#262626".into(),
            "#ededed".into(),
        ),
        Variant::Light => match &look.theme {
            Some(t) => (
                t.bg.clone(),
                t.surface.clone(),
                t.border.clone(),
                t.text.clone(),
            ),
            None => (
                "#f4f6f9".into(),
                "#ffffff".into(),
                "#e5e7eb".into(),
                "#1f2937".into(),
            ),
        },
    }
}

/// Re-root a panel `<svg …>` at `(x, y)` inside another SVG.
fn nest(panel: &str, x: u64, y: u64) -> String {
    match panel.find("<svg") {
        Some(i) => format!("{}<svg x=\"{x}\" y=\"{y}\"{}", &panel[..i], &panel[i + 4..]),
        None => String::new(),
    }
}

/// The `width`/`height` attributes of an SVG's root element.
fn svg_size(svg: &str) -> (u64, u64) {
    let head = &svg[..svg.find('>').unwrap_or(svg.len())];
    let attr = |name: &str| {
        let key = format!(" {name}=\"");
        head.find(&key).and_then(|i| {
            let rest = &head[i + key.len()..];
            rest[..rest.find('"')?].parse::<f64>().ok()
        })
    };
    (
        attr("width").unwrap_or(TILE_W as f64) as u64,
        attr("height").unwrap_or(TILE_H as f64) as u64,
    )
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
