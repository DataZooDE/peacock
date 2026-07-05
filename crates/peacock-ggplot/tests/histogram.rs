//! The ggplot backend contract (issue #6): `(spec, rows, theme, size) → PNG`,
//! headless and in-memory — no files, no R, no network. Only `histogram`
//! renders in this issue; the other stat geoms error clearly until the
//! stat-spec dialect lands (issue #7).

use peacock_ggplot::render_stat_to_png;
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

#[test]
fn histogram_renders_an_in_memory_png() {
    let png = render_stat_to_png(&hist_spec(), &rows(), None, 1.0)
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
    let a = render_stat_to_png(&hist_spec(), &rows(), None, 1.0).unwrap();
    let b = render_stat_to_png(&hist_spec(), &rows(), None, 1.0).unwrap();
    assert_eq!(a, b, "two renders of the same inputs are byte-equal");
}

#[test]
fn scale_grows_the_image() {
    let small = render_stat_to_png(&hist_spec(), &rows(), None, 1.0).unwrap();
    let big = render_stat_to_png(&hist_spec(), &rows(), None, 2.0).unwrap();
    // PNG IHDR width lives at bytes 16..20 (big-endian) right after the magic.
    let w = |png: &[u8]| u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    assert_eq!(w(&big), 2 * w(&small), "scale multiplies the pixel size");
}

#[test]
fn a_theme_changes_the_image() {
    let stock = render_stat_to_png(&hist_spec(), &rows(), None, 1.0).unwrap();
    let themed = render_stat_to_png(
        &hist_spec(),
        &rows(),
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
fn not_yet_implemented_geoms_error_clearly() {
    for geom in ["density", "boxplot", "ecdf"] {
        let err = render_stat_to_png(&json!({ "geom": geom, "x": "revenue" }), &rows(), None, 1.0)
            .expect_err("declared-but-unimplemented geoms must not render");
        assert!(
            err.to_string().contains("not yet implemented"),
            "`{geom}` names the gap: {err}"
        );
    }
}

#[test]
fn unknown_geom_errors() {
    let err = render_stat_to_png(
        &json!({ "geom": "pie", "x": "revenue" }),
        &rows(),
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
        None,
        1.0,
    )
    .expect_err("a non-numeric x column must error, not render garbage");
    assert!(
        err.to_string().contains("category"),
        "error names the column: {err}"
    );
}
