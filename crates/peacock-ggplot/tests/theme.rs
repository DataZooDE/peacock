//! The full `--pk-*` token → ggplot-rs `Theme` mapping (issue #8): one brand
//! source paints the whole chart — background, panel, grid, axis text/lines,
//! title, the categorical palette, the annotation accent — and stays
//! deterministic: same brand ⇒ same bytes, different brand ⇒ different bytes,
//! and no system font can sneak in (ggplot-rs bundles DejaVu).

use peacock_ggplot::{ColumnSchema, render_stat_to_png};
use peacock_theme::ThemeTokens;
use serde_json::{Value, json};

fn schema() -> Vec<ColumnSchema> {
    [("lead_days", "DOUBLE"), ("supplier", "VARCHAR")]
        .iter()
        .map(|(name, type_name)| ColumnSchema {
            name: (*name).to_owned(),
            type_name: (*type_name).to_owned(),
        })
        .collect()
}

fn rows() -> Value {
    json!(
        (0..120)
            .map(|i| {
                let supplier = match i % 3 {
                    0 => "alpine",
                    1 => "borealis",
                    _ => "cormorant",
                };
                let lead = 5.0 + f64::from(i % 17) + f64::from(i % 3) * 3.0;
                json!({ "lead_days": lead, "supplier": supplier })
            })
            .collect::<Vec<_>>()
    )
}

fn density_spec() -> Value {
    json!({ "geom": "density", "x": "lead_days" })
}

fn render(spec: &Value, tokens: Option<&ThemeTokens>) -> Vec<u8> {
    render_stat_to_png(spec, &rows(), &schema(), tokens, 1.0).expect("chart renders")
}

// ── every token group reaches the chart ──────────────────────────────────────

#[test]
fn grid_token_changes_the_chart() {
    let a = render(
        &density_spec(),
        Some(&ThemeTokens {
            grid: "#e6e6e6".into(),
            ..ThemeTokens::default()
        }),
    );
    let b = render(
        &density_spec(),
        Some(&ThemeTokens {
            grid: "#204060".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(a, b, "the grid token recolors the gridlines");
}

#[test]
fn axis_token_changes_the_chart() {
    let a = render(
        &density_spec(),
        Some(&ThemeTokens {
            axis: "#888888".into(),
            ..ThemeTokens::default()
        }),
    );
    let b = render(
        &density_spec(),
        Some(&ThemeTokens {
            axis: "#c02020".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(a, b, "the axis token recolors the axis lines");
}

#[test]
fn muted_token_changes_the_axis_text() {
    let a = render(
        &density_spec(),
        Some(&ThemeTokens {
            muted: "#605e5c".into(),
            ..ThemeTokens::default()
        }),
    );
    let b = render(
        &density_spec(),
        Some(&ThemeTokens {
            muted: "#0b7a3c".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(a, b, "the muted token recolors axis labels");
}

#[test]
fn border_token_changes_the_chart() {
    let a = render(
        &density_spec(),
        Some(&ThemeTokens {
            border: "#e1dfdd".into(),
            ..ThemeTokens::default()
        }),
    );
    let b = render(
        &density_spec(),
        Some(&ThemeTokens {
            border: "#3a2f80".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(a, b, "the border token recolors the panel border");
}

#[test]
fn surface_token_changes_a_faceted_chart() {
    // The surface token paints the facet strip background.
    let spec = json!({ "geom": "density", "x": "lead_days", "facet_wrap": "supplier" });
    let a = render(
        &spec,
        Some(&ThemeTokens {
            surface: "#f4f9fe".into(),
            ..ThemeTokens::default()
        }),
    );
    let b = render(
        &spec,
        Some(&ThemeTokens {
            surface: "#2b2b40".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(a, b, "the surface token repaints the facet strips");
}

#[test]
fn title_is_painted_with_the_text_token() {
    let spec = json!({ "geom": "density", "x": "lead_days", "title": "Lead times" });
    let a = render(
        &spec,
        Some(&ThemeTokens {
            text: "#201f1e".into(),
            ..ThemeTokens::default()
        }),
    );
    let b = render(
        &spec,
        Some(&ThemeTokens {
            text: "#7a1fa2".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(a, b, "the text token recolors the title");
}

#[test]
fn accent_token_recolors_annotation_lines() {
    let spec = json!({
        "geom": "density", "x": "lead_days",
        "annotations": [{ "kind": "vline", "at": 14.0, "label": "contract" }]
    });
    let a = render(
        &spec,
        Some(&ThemeTokens {
            accent: "#22d3c5".into(),
            ..ThemeTokens::default()
        }),
    );
    let b = render(
        &spec,
        Some(&ThemeTokens {
            accent: "#d32222".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(a, b, "the accent token recolors the reference lines");
}

// ── the categorical palette drives multi-series color ───────────────────────

#[test]
fn palette_drives_multi_series_colors() {
    let spec = json!({ "geom": "density", "x": "lead_days", "color": "supplier" });
    let warm = render(
        &spec,
        Some(&ThemeTokens {
            palette: vec!["#d62728".into(), "#ff7f0e".into(), "#e8c547".into()],
            ..ThemeTokens::default()
        }),
    );
    let cold = render(
        &spec,
        Some(&ThemeTokens {
            palette: vec!["#1f77b4".into(), "#17becf".into(), "#2ca02c".into()],
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(warm, cold, "the palette tokens recolor the series");
}

#[test]
fn short_palettes_cycle_instead_of_failing() {
    // Three suppliers, one palette color: the mapping cycles.
    let spec = json!({ "geom": "density", "x": "lead_days", "color": "supplier" });
    let png = render_stat_to_png(
        &spec,
        &rows(),
        &schema(),
        Some(&ThemeTokens {
            palette: vec!["#d62728".into()],
            ..ThemeTokens::default()
        }),
        1.0,
    )
    .expect("a short palette cycles across levels");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

// ── brand determinism ────────────────────────────────────────────────────────

#[test]
fn same_brand_same_bytes() {
    let tokens = ThemeTokens {
        font: "\"Brandface\", sans-serif".into(),
        brand: "#6b3fa0".into(),
        accent: "#e8590c".into(),
        bg: "#0f1220".into(),
        surface: "#1a1f33".into(),
        text: "#f4f4f8".into(),
        muted: "#9aa0b5".into(),
        border: "#2c3350".into(),
        grid: "#232a45".into(),
        axis: "#5b6488".into(),
        palette: vec!["#6b3fa0".into(), "#e8590c".into(), "#22d3c5".into()],
        ..ThemeTokens::default()
    };
    let spec = json!({
        "geom": "density", "x": "lead_days", "color": "supplier",
        "title": "Lead times",
        "annotations": [{ "kind": "vline", "at": 14.0, "label": "contract" }, { "kind": "p90" }]
    });
    let a = render(&spec, Some(&tokens));
    let b = render(&spec, Some(&tokens));
    assert_eq!(a, b, "one brand always renders the same bytes");
}

#[test]
fn different_brands_different_bytes() {
    let spec = json!({ "geom": "density", "x": "lead_days" });
    let a = render(
        &spec,
        Some(&ThemeTokens {
            brand: "#6b3fa0".into(),
            bg: "#ffffff".into(),
            ..ThemeTokens::default()
        }),
    );
    let b = render(
        &spec,
        Some(&ThemeTokens {
            brand: "#e8590c".into(),
            bg: "#101418".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(a, b, "two corporate identities render different charts");
}

// ── fonts: bundled DejaVu only, no system-font fallback ─────────────────────

#[test]
fn unknown_font_family_maps_to_the_bundled_face() {
    // ggplot-rs registers every requested family against its bundled DejaVu
    // (plotters ab_glyph does no system lookup), so an exotic brand font must
    // render EXACTLY the bytes of the stock sans face — if a system font ever
    // leaked in, these would differ (render_northwind.rs's determinism worry).
    let exotic = render(
        &density_spec(),
        Some(&ThemeTokens {
            font: "\"Totally Unreal Grotesk\", cursive".into(),
            ..ThemeTokens::default()
        }),
    );
    let stock = render(
        &density_spec(),
        Some(&ThemeTokens {
            font: "\"DejaVu Sans\", system-ui, sans-serif".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_eq!(
        exotic, stock,
        "an unavailable family falls back to the bundled face, deterministically"
    );
}

#[test]
fn serif_font_token_changes_the_glyphs() {
    // The bundled set DOES carry distinct serif faces — the font token is
    // honoured where a bundled face exists, not ignored.
    let sans = render(
        &density_spec(),
        Some(&ThemeTokens {
            font: "\"DejaVu Sans\", sans-serif".into(),
            ..ThemeTokens::default()
        }),
    );
    let serif = render(
        &density_spec(),
        Some(&ThemeTokens {
            font: "serif".into(),
            ..ThemeTokens::default()
        }),
    );
    assert_ne!(sans, serif, "the serif face renders different glyphs");
}

#[test]
fn font_rendering_is_deterministic_across_renders() {
    let tokens = ThemeTokens {
        font: "\"Some Brand Face\", sans-serif".into(),
        ..ThemeTokens::default()
    };
    let spec = json!({ "geom": "density", "x": "lead_days", "title": "Lead times" });
    let a = render(&spec, Some(&tokens));
    let b = render(&spec, Some(&tokens));
    assert_eq!(a, b, "text layout is byte-reproducible");
}
