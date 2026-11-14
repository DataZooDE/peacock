//! The anofox chart backend, pure (no escurel): an authored `kind: anofox`
//! spec plus escurel rows become an anofox panel spec, the renderer turns it
//! into inert SVG, and the guardrails reject anything outside the safe
//! subset (ACC-4). Issue #44, Phase 1.

use peacock_core::chart::{self, ChartLook, Variant};
use peacock_core::data::{Column, RowSet};
use serde_json::{Value, json};

fn col(name: &str, ty: &str) -> Column {
    Column {
        name: name.into(),
        type_name: ty.into(),
    }
}

fn nw_rows() -> RowSet {
    RowSet {
        rows: json!([
            { "month": "1997-01-01", "category": "Beverages", "revenue": 180.5 },
            { "month": "1997-02-01", "category": "Beverages", "revenue": 81.0 },
            { "month": "1997-01-01", "category": "Condiments", "revenue": 110.0 }
        ]),
        schema: vec![
            col("month", "VARCHAR"),
            col("category", "VARCHAR"),
            col("revenue", "DOUBLE"),
        ],
        truncated: false,
    }
}

fn stacked_spec() -> Value {
    json!({
        "title": "Revenue by category",
        "roles": [
            ["month", "XAXIS"],
            ["category", "CATEGORY"],
            ["revenue", "BARCHART_STACKED", "Revenue"]
        ]
    })
}

#[test]
fn panel_spec_rekeys_rows_by_role_position() {
    let panel = chart::panel_spec(&stacked_spec(), &nw_rows()).expect("panel");
    // anofox reads row keys `c{i}` where `i` is the role's position.
    // The title rides as a trailing `TITLE` role (anofox's card title bar).
    assert_eq!(
        panel["rows"][0],
        json!({ "c0": "1997-01-01", "c1": "Beverages", "c2": 180.5, "c3": "Revenue by category" })
    );
    assert_eq!(
        panel["roles"],
        json!([
            [0, "XAXIS", "month"],
            [1, "CATEGORY", "category"],
            [2, "BARCHART_STACKED", "Revenue"],
            [3, "TITLE", ""]
        ])
    );
}

#[test]
fn a_role_may_name_its_column_by_schema_index() {
    let spec = json!({ "roles": [[0, "XAXIS"], [2, "BARCHART"]] });
    let panel = chart::panel_spec(&spec, &nw_rows()).expect("panel");
    assert_eq!(panel["rows"][1], json!({ "c0": "1997-02-01", "c1": 81.0 }));
}

#[test]
fn unknown_role_is_rejected() {
    let spec = json!({ "roles": [["month", "XAXIS"], ["revenue", "SPARKLE_CHART"]] });
    let err = chart::check_spec(&spec, &["month", "revenue"]).unwrap_err();
    assert!(err.to_string().contains("SPARKLE_CHART"), "{err}");
}

#[test]
fn unknown_column_is_rejected() {
    let spec = json!({ "roles": [["month", "XAXIS"], ["profit", "BARCHART"]] });
    let err = chart::check_spec(&spec, &["month", "revenue"]).unwrap_err();
    assert!(err.to_string().contains("profit"), "{err}");
}

#[test]
fn link_role_is_rejected() {
    // `::LINK` turns a cell into an href — a navigation escape hatch.
    let spec = json!({ "roles": [["category", "TABLE"], ["month", "LINK"]] });
    let err = chart::check_spec(&spec, &["category", "month"]).unwrap_err();
    assert!(err.to_string().contains("LINK"), "{err}");
}

#[test]
fn layout_and_input_roles_are_rejected_in_a_panel() {
    // A layout/input role inside a data panel is the spike's silent-drop
    // slip: refuse it instead of rendering nothing.
    for role in ["COLUMNS", "GROUP", "DROPDOWN", "THEME"] {
        let spec = json!({ "roles": [["month", "XAXIS"], ["revenue", role]] });
        assert!(
            chart::check_spec(&spec, &["month", "revenue"]).is_err(),
            "{role} must be rejected inside a panel"
        );
    }
}

#[test]
fn escape_hatch_keys_are_rejected() {
    for (k, v) in [
        ("url", json!("https://evil.example/x.csv")),
        ("plot", json!("terms")),
        (
            "theme",
            json!({ "brand": { "logo": "https://evil/x.svg" } }),
        ),
        ("rows", json!([])),
    ] {
        let mut spec = stacked_spec();
        spec[k] = v;
        assert!(
            chart::check_spec(&spec, &["month", "category", "revenue"]).is_err(),
            "authored `{k}` must be rejected (peacock injects rows/theme itself)"
        );
    }
}

#[test]
fn renders_inert_svg_with_the_data() {
    let panel = chart::panel_spec(&stacked_spec(), &nw_rows()).unwrap();
    let svg = chart::render_svg(&panel, &ChartLook::default()).expect("svg");
    assert!(
        svg.trim_start().starts_with("<svg"),
        "{}",
        &svg[..80.min(svg.len())]
    );
    assert!(svg.contains("Beverages") && svg.contains("Condiments"));
    assert!(svg.contains("Revenue by category"));
    for bad in ["<script", "foreignObject", "javascript:", "href=\"http"] {
        assert!(!svg.contains(bad), "svg must be inert, found {bad}");
    }
}

#[test]
fn render_is_deterministic() {
    let panel = chart::panel_spec(&stacked_spec(), &nw_rows()).unwrap();
    let look = ChartLook::default();
    assert_eq!(
        chart::render_svg(&panel, &look).unwrap(),
        chart::render_svg(&panel, &look).unwrap()
    );
}

#[test]
fn theme_brand_reaches_the_svg() {
    let tokens = peacock_theme::ThemeTokens {
        palette: vec!["#6b3fa0".into(), "#ff8800".into()],
        ..Default::default()
    };
    let panel = chart::panel_spec(&stacked_spec(), &nw_rows()).unwrap();
    let look = ChartLook {
        theme: Some(tokens),
        ..Default::default()
    };
    let svg = chart::render_svg(&panel, &look).unwrap().to_lowercase();
    assert!(svg.contains("#6b3fa0"), "first series uses --pk-cat-1");
}

#[test]
fn dark_variant_changes_the_card() {
    let panel = chart::panel_spec(&stacked_spec(), &nw_rows()).unwrap();
    let light = chart::render_svg(&panel, &ChartLook::default()).unwrap();
    let dark = chart::render_svg(
        &panel,
        &ChartLook {
            variant: Variant::Dark,
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(light, dark);
}

#[test]
fn locale_formats_numbers() {
    let spec = json!({ "roles": [["revenue", "KPI", "Revenue"]] });
    let rows = RowSet {
        rows: json!([{ "revenue": 1234567.5 }]),
        schema: vec![col("revenue", "DOUBLE")],
        truncated: false,
    };
    let panel = chart::panel_spec(&spec, &rows).unwrap();
    let de = chart::render_svg(
        &panel,
        &ChartLook {
            locale: Some("de-DE".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        de.contains("1.234.567") || de.contains("1,2 Mio") || de.contains("1,23"),
        "de-DE grouping/decimal marks expected"
    );
}

#[test]
fn png_rasterizes_the_svg() {
    let panel = chart::panel_spec(&stacked_spec(), &nw_rows()).unwrap();
    let svg = chart::render_svg(&panel, &ChartLook::default()).unwrap();
    let png = chart::svg_to_png(&svg, 1.0).expect("png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(
        png,
        chart::svg_to_png(&svg, 1.0).unwrap(),
        "deterministic PNG"
    );
}

// ── derived Vega-Lite: the Triton native-chart wire contract (Phase 2) ──────

fn derived(roles: Value) -> Option<Value> {
    let spec = json!({ "title": "Revenue by category", "roles": roles });
    let panel = chart::panel_spec(&spec, &nw_rows()).unwrap();
    chart::derive_vega_lite(&panel)
}

#[test]
fn bar_roles_derive_the_shape_triton_reads() {
    for (role, stack) in [("BARCHART", false), ("BARCHART_STACKED", true)] {
        let v = derived(json!([
            ["month", "XAXIS"],
            ["category", "CATEGORY"],
            ["revenue", role, "Revenue"]
        ]))
        .unwrap_or_else(|| panic!("{role} must derive"));
        assert_eq!(v["mark"]["type"], "bar", "{role}");
        assert_eq!(v["title"], "Revenue by category");
        // triton's chart_from_vega: data.values + encoding.{x,y,color}.field.
        assert_eq!(v["encoding"]["x"]["field"], "month");
        assert_eq!(v["encoding"]["y"]["field"], "Revenue");
        assert_eq!(v["encoding"]["color"]["field"], "category");
        assert_eq!(v["data"]["values"].as_array().unwrap().len(), 3);
        assert_eq!(v["data"]["values"][0]["Revenue"], 180.5);
        assert_eq!(v["data"]["values"][0]["category"], "Beverages");
        assert_eq!(v["encoding"]["y"].get("stack").is_some(), !stack, "{role}");
    }
}

#[test]
fn line_and_area_derive() {
    for (role, mark) in [
        ("LINECHART", "line"),
        ("AREACHART", "area"),
        ("AREACHART_STACKED", "area"),
    ] {
        let v = derived(json!([["month", "XAXIS"], ["revenue", role]])).unwrap();
        assert_eq!(v["mark"]["type"], mark, "{role}");
        assert!(v["encoding"].get("color").is_none());
    }
}

#[test]
fn pie_and_donut_derive_an_arc() {
    for (role, inner) in [("PIE", false), ("DONUT", true)] {
        let v = derived(json!([["category", "CATEGORY"], ["revenue", role]])).unwrap();
        assert_eq!(v["mark"]["type"], "arc");
        assert_eq!(v["encoding"]["theta"]["field"], "revenue");
        assert_eq!(v["encoding"]["color"]["field"], "category");
        assert_eq!(v["mark"].get("innerRadius").is_some(), inner, "{role}");
    }
}

#[test]
fn non_chartable_roles_derive_nothing() {
    for roles in [
        json!([["revenue", "KPI"]]),
        json!([["revenue", "HISTOGRAM"]]),
        json!([
            ["month", "XAXIS"],
            ["category", "YAXIS"],
            ["revenue", "HEATMAP"]
        ]),
        json!([["category", "TABLE"], ["revenue", "TABLE"]]),
    ] {
        assert!(derived(roles.clone()).is_none(), "{roles}");
    }
}

#[test]
fn png_is_painted_on_the_variant_card_not_transparent() {
    // A transparent dark-variant PNG puts light text on whatever the chat
    // client paints behind it (usually white) — unreadable.
    let panel = chart::panel_spec(&stacked_spec(), &nw_rows()).unwrap();
    for (variant, expect_dark) in [(Variant::Light, false), (Variant::Dark, true)] {
        let look = ChartLook {
            variant,
            ..Default::default()
        };
        let svg = chart::render_svg(&panel, &look).unwrap();
        let png = chart::svg_to_png_on(&svg, 1.0, &look).unwrap();
        let img = image_corner(&png);
        assert_eq!(img.3, 255, "{variant:?}: opaque corner");
        let lum = (img.0 as u32 + img.1 as u32 + img.2 as u32) / 3;
        assert_eq!(lum < 64, expect_dark, "{variant:?}: corner {img:?}");
    }
}

/// The top-left pixel (RGBA) of a PNG.
fn image_corner(png: &[u8]) -> (u8, u8, u8, u8) {
    let p = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = p.pixel(0, 0).unwrap().demultiply();
    (c.red(), c.green(), c.blue(), c.alpha())
}

// ── non-chart images: KPI tiles (Triton's dashboard) and the instance card ──

#[test]
fn kpi_tiles_render_each_tile_with_its_label() {
    let tiles = vec![
        chart::Tile {
            label: "Total revenue".into(),
            value: "$2,986".into(),
            trend: Some("+12%".into()),
        },
        chart::Tile {
            label: "Categories".into(),
            value: "5".into(),
            trend: None,
        },
    ];
    let svg = chart::kpi_tiles_svg("Northwind revenue", &tiles, &ChartLook::default()).unwrap();
    for text in [
        "Northwind revenue",
        "$2,986",
        "Total revenue",
        "+12%",
        "Categories",
    ] {
        assert!(
            svg.contains(&text.replace('&', "&amp;")),
            "{text} in the tiles"
        );
    }
    chart::check_inert(&svg).unwrap();
    let png = chart::svg_to_png_on(&svg, 1.0, &ChartLook::default()).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn instance_card_shows_title_facts_body_and_activity() {
    let card = chart::InstanceCard {
        title: "Beverages GmbH".into(),
        subtitle: "account · beverages-gmbh".into(),
        facts: vec![
            ("status".into(), "active".into()),
            ("owner".into(), "Alice".into()),
        ],
        body_lines: vec!["Key account in Hamburg.".into()],
        events: vec![("Email filed".into(), "mail · Renewal question".into())],
    };
    let svg = chart::instance_card_svg(&card, &ChartLook::default()).unwrap();
    for text in [
        "Beverages GmbH",
        "status",
        "active",
        "Key account in Hamburg.",
        "Email filed",
    ] {
        assert!(svg.contains(text), "{text} on the card");
    }
    chart::check_inert(&svg).unwrap();
    // Untrusted page content is escaped, never markup.
    let evil = chart::InstanceCard {
        title: "<script>x</script>".into(),
        ..card
    };
    let svg = chart::instance_card_svg(&evil, &ChartLook::default()).unwrap();
    assert!(!svg.contains("<script"), "escaped");
}
