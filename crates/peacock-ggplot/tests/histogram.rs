//! The ggplot backend contract (issue #6): `(spec, rows, theme, size) → PNG`,
//! headless and in-memory — no files, no R, no network. The histogram suite;
//! the rest of the stat-spec dialect (issue #7) is `tests/dialect.rs`.

use peacock_ggplot::{ColumnSchema, render_stat_to_png};
use peacock_theme::ThemeTokens;
use serde_json::json;

fn hist_spec() -> serde_json::Value {
    json!({ "geom": "histogram", "x": "revenue", "bins": 10 })
}

fn rows() -> serde_json::Value {
    json!(
        (0..200)
            .map(|i| json!({ "revenue": f64::from(i % 37) + f64::from(i) / 200.0 }))
            .collect::<Vec<_>>()
    )
}

fn schema() -> Vec<ColumnSchema> {
    vec![
        ColumnSchema {
            name: "revenue".into(),
            type_name: "DOUBLE".into(),
        },
        ColumnSchema {
            name: "category".into(),
            type_name: "VARCHAR".into(),
        },
    ]
}

#[test]
fn histogram_renders_an_in_memory_png() {
    let png = render_stat_to_png(&hist_spec(), &rows(), &schema(), None, 1.0)
        .expect("histogram renders headless to PNG bytes");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic header");
    assert!(
        png.len() > 500,
        "a real chart, not a stub ({} bytes)",
        png.len()
    );
}

#[test]
fn rendering_is_deterministic() {
    // Statelessness (ADR-P7): identical inputs reproduce the identical image.
    let a = render_stat_to_png(&hist_spec(), &rows(), &schema(), None, 1.0).unwrap();
    let b = render_stat_to_png(&hist_spec(), &rows(), &schema(), None, 1.0).unwrap();
    assert_eq!(a, b, "two renders of the same inputs are byte-equal");
}

#[test]
fn scale_grows_the_image() {
    let small = render_stat_to_png(&hist_spec(), &rows(), &schema(), None, 1.0).unwrap();
    let big = render_stat_to_png(&hist_spec(), &rows(), &schema(), None, 2.0).unwrap();
    // PNG IHDR width lives at bytes 16..20 (big-endian) right after the magic.
    let w = |png: &[u8]| u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    assert_eq!(w(&big), 2 * w(&small), "scale multiplies the pixel size");
}

#[test]
fn a_theme_changes_the_image() {
    let stock = render_stat_to_png(&hist_spec(), &rows(), &schema(), None, 1.0).unwrap();
    let themed = render_stat_to_png(
        &hist_spec(),
        &rows(),
        &schema(),
        Some(&ThemeTokens {
            bg: "#10233f".into(),
            brand: "#e2543e".into(),
            text: "#f4f9fe".into(),
            ..ThemeTokens::default()
        }),
        1.0,
    )
    .unwrap();
    assert_ne!(
        stock, themed,
        "bg/brand/text tokens visibly restyle the chart"
    );
}

#[test]
fn unknown_geom_errors() {
    let err = render_stat_to_png(
        &json!({ "geom": "pie", "x": "revenue" }),
        &rows(),
        &schema(),
        None,
        1.0,
    )
    .expect_err("an unknown geom must error");
    assert!(
        err.to_string().contains("pie"),
        "error names the geom: {err}"
    );
}

#[test]
fn non_numeric_x_column_errors_clearly() {
    let rows = json!([
        { "category": "Beverages" },
        { "category": "Condiments" }
    ]);
    let err = render_stat_to_png(
        &json!({ "geom": "histogram", "x": "category" }),
        &rows,
        &schema(),
        None,
        1.0,
    )
    .expect_err("a non-numeric x column must error, not render garbage");
    assert!(
        err.to_string().contains("category"),
        "error names the column: {err}"
    );
}
