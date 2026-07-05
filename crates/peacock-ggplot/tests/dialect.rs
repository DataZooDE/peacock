//! The stat-spec dialect rendered (issue #7): `density`, `boxplot`, `ecdf`
//! join `histogram`; `color` / `facet_wrap` aesthetics and the `vline` / `p90`
//! annotations draw where ggplot-rs 0.9.2 supports them and error clearly
//! where it does not. Same contract as the histogram suite: headless,
//! in-memory, deterministic.

use peacock_ggplot::render_stat_to_png;
use serde_json::{Value, json};

/// Two-supplier lead-time rows — the reference use case's shape (numeric
/// `lead_days`, categorical `supplier`).
fn rows() -> Value {
    json!(
        (0..120)
            .map(|i| {
                let supplier = if i % 2 == 0 { "acme" } else { "globex" };
                let lead = 5.0 + f64::from(i % 17) + f64::from(i % 2) * 4.0 + f64::from(i) * 0.01;
                json!({ "lead_days": lead, "supplier": supplier })
            })
            .collect::<Vec<_>>()
    )
}

fn render(spec: Value) -> Result<Vec<u8>, String> {
    render_stat_to_png(&spec, &rows(), None, 1.0).map_err(|e| e.to_string())
}

fn assert_png(bytes: &[u8]) {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG magic header");
    assert!(bytes.len() > 500, "a real chart ({} bytes)", bytes.len());
}

// ── every geom renders, deterministically ───────────────────────────────────

#[test]
fn density_renders_a_deterministic_png() {
    let spec = json!({ "geom": "density", "x": "lead_days" });
    let a = render(spec.clone()).expect("density renders");
    assert_png(&a);
    let b = render(spec).unwrap();
    assert_eq!(a, b, "two renders are byte-equal");
}

#[test]
fn ecdf_renders_a_deterministic_png() {
    let spec = json!({ "geom": "ecdf", "x": "lead_days" });
    let a = render(spec.clone()).expect("ecdf renders");
    assert_png(&a);
    let b = render(spec).unwrap();
    assert_eq!(a, b, "two renders are byte-equal");
}

#[test]
fn boxplot_renders_a_deterministic_png() {
    let spec = json!({ "geom": "boxplot", "x": "supplier", "y": "lead_days" });
    let a = render(spec.clone()).expect("boxplot renders");
    assert_png(&a);
    let b = render(spec).unwrap();
    assert_eq!(a, b, "two renders are byte-equal");
}

// ── aesthetics ──────────────────────────────────────────────────────────────

#[test]
fn density_color_series_changes_the_image() {
    let plain = render(json!({ "geom": "density", "x": "lead_days" })).unwrap();
    let colored = render(json!({ "geom": "density", "x": "lead_days", "color": "supplier" }))
        .expect("per-supplier density renders");
    assert_png(&colored);
    assert_ne!(plain, colored, "a color series visibly changes the chart");
}

#[test]
fn facet_wrap_changes_the_image_for_every_geom() {
    for (geom, base) in [
        (
            "histogram",
            json!({ "geom": "histogram", "x": "lead_days" }),
        ),
        ("density", json!({ "geom": "density", "x": "lead_days" })),
        ("ecdf", json!({ "geom": "ecdf", "x": "lead_days" })),
        (
            "boxplot",
            json!({ "geom": "boxplot", "x": "supplier", "y": "lead_days" }),
        ),
    ] {
        let mut faceted = base.clone();
        faceted["facet_wrap"] = json!("supplier");
        let plain = render(base).unwrap_or_else(|e| panic!("`{geom}` renders: {e}"));
        let small_multiples =
            render(faceted).unwrap_or_else(|e| panic!("faceted `{geom}` renders: {e}"));
        assert_png(&small_multiples);
        assert_ne!(plain, small_multiples, "facet_wrap changes `{geom}`");
    }
}

// ── annotations ─────────────────────────────────────────────────────────────

#[test]
fn vline_annotation_changes_the_image() {
    for geom in ["histogram", "density", "ecdf"] {
        let base = json!({ "geom": geom, "x": "lead_days" });
        let mut with = base.clone();
        with["annotations"] = json!([{ "kind": "vline", "at": 14.0, "label": "contract" }]);
        let plain = render(base).unwrap();
        let annotated = render(with).unwrap_or_else(|e| panic!("`{geom}` + vline renders: {e}"));
        assert_png(&annotated);
        assert_ne!(plain, annotated, "the contract vline shows on `{geom}`");
    }
}

#[test]
fn p90_annotation_changes_the_image() {
    for geom in ["histogram", "density", "ecdf"] {
        let base = json!({ "geom": geom, "x": "lead_days" });
        let mut with = base.clone();
        with["annotations"] = json!([{ "kind": "p90", "label": "p90" }]);
        let plain = render(base).unwrap();
        let annotated = render(with).unwrap_or_else(|e| panic!("`{geom}` + p90 renders: {e}"));
        assert_png(&annotated);
        assert_ne!(plain, annotated, "the p90 marker shows on `{geom}`");
    }
}

#[test]
fn p90_and_vline_differ_from_each_other() {
    // The p90 quantile of these rows is nowhere near 14.0, so the two
    // annotated charts must differ — the p90 line is computed, not fixed.
    let vline = render(json!({
        "geom": "density", "x": "lead_days",
        "annotations": [{ "kind": "vline", "at": 14.0 }]
    }))
    .unwrap();
    let p90 = render(json!({
        "geom": "density", "x": "lead_days",
        "annotations": [{ "kind": "p90" }]
    }))
    .unwrap();
    assert_ne!(vline, p90, "p90 is computed from the data");
}

#[test]
fn boxplot_annotations_draw_on_the_value_axis() {
    // A boxplot's numeric axis is y, so the contract line / p90 marker draw
    // as horizontal reference lines at the value.
    let base = json!({ "geom": "boxplot", "x": "supplier", "y": "lead_days" });
    let mut with = base.clone();
    with["annotations"] = json!([{ "kind": "vline", "at": 14.0 }, { "kind": "p90" }]);
    let plain = render(base).unwrap();
    let annotated = render(with).expect("boxplot renders value-axis reference lines");
    assert_ne!(plain, annotated, "the reference lines show on the boxplot");
}

// ── honest gaps: unsupported combinations error, never silently drop ────────

#[test]
fn color_on_geoms_the_backend_cannot_group_errors_clearly() {
    for (geom, spec) in [
        (
            "histogram",
            json!({ "geom": "histogram", "x": "lead_days", "color": "supplier" }),
        ),
        (
            "ecdf",
            json!({ "geom": "ecdf", "x": "lead_days", "color": "supplier" }),
        ),
        (
            "boxplot",
            json!({ "geom": "boxplot", "x": "supplier", "y": "lead_days", "color": "supplier" }),
        ),
    ] {
        let err = render(spec).expect_err("unsupported color grouping must error, not drop");
        assert!(
            err.contains("color") && err.contains(geom),
            "`{geom}` error names the unsupported aesthetic: {err}"
        );
        assert!(
            err.contains("facet_wrap"),
            "`{geom}` error points at the supported alternative: {err}"
        );
    }
}

#[test]
fn labelled_annotation_on_boxplot_errors_clearly() {
    let err = render(json!({
        "geom": "boxplot", "x": "supplier", "y": "lead_days",
        "annotations": [{ "kind": "vline", "at": 14.0, "label": "contract" }]
    }))
    .expect_err("a labelled annotation on a discrete-x boxplot must error, not drop the label");
    assert!(
        err.contains("label") && err.contains("boxplot"),
        "error names the gap: {err}"
    );
}

// ── dialect errors surface through the renderer too ─────────────────────────

#[test]
fn dialect_violations_error_at_render() {
    // The renderer re-parses the typed dialect, so a spec that dodged compose
    // (e.g. handed straight to the library) still fails closed.
    let err = render(json!({ "geom": "density", "x": "lead_days", "bins": 10 }))
        .expect_err("bins on density is a dialect violation");
    assert!(err.contains("bins"), "names the field: {err}");

    let err = render(json!({ "geom": "boxplot", "x": "supplier" }))
        .expect_err("boxplot without y is a dialect violation");
    assert!(err.contains("y"), "names the missing aesthetic: {err}");
}

#[test]
fn non_numeric_value_column_errors_clearly() {
    for spec in [
        json!({ "geom": "density", "x": "supplier" }),
        json!({ "geom": "ecdf", "x": "supplier" }),
        json!({ "geom": "boxplot", "x": "lead_days", "y": "supplier" }),
    ] {
        let err = render(spec).expect_err("a non-numeric value column must error");
        assert!(
            err.contains("supplier"),
            "error names the offending column: {err}"
        );
    }
}
