//! The statistical spec DIALECT (issue #7): typed parsing + validation of the
//! declarative surface a report-skill author writes under `specs:` — geoms
//! `histogram | density | boxplot | ecdf`, aesthetics `x` / `y` / `color` /
//! `facet_wrap`, and the `vline` / `p90` annotations. These tests pin the
//! grammar: what parses, what is rejected, and that every rejection carries a
//! message an author can act on.

use peacock_types::{StatAnnotation, StatGeom, StatSpec};
use serde_json::json;

fn parse(v: serde_json::Value) -> Result<StatSpec, peacock_types::Error> {
    StatSpec::parse(&v)
}

// ── every geom parses ───────────────────────────────────────────────────────

#[test]
fn all_four_geoms_parse() {
    for (geom, expected) in [
        ("histogram", StatGeom::Histogram),
        ("density", StatGeom::Density),
        ("ecdf", StatGeom::Ecdf),
    ] {
        let s = parse(json!({ "geom": geom, "x": "lead_days" }))
            .unwrap_or_else(|e| panic!("`{geom}` must parse: {e}"));
        assert_eq!(s.geom, expected);
        assert_eq!(s.x, "lead_days");
    }
    // boxplot carries the value on `y` (x is the grouping category).
    let s = parse(json!({ "geom": "boxplot", "x": "supplier", "y": "lead_days" }))
        .expect("boxplot with x + y parses");
    assert_eq!(s.geom, StatGeom::Boxplot);
    assert_eq!(s.y.as_deref(), Some("lead_days"));
}

// ── aesthetics ──────────────────────────────────────────────────────────────

#[test]
fn color_and_facet_wrap_parse() {
    let s = parse(json!({
        "geom": "density", "x": "lead_days",
        "color": "supplier", "facet_wrap": "supplier"
    }))
    .expect("color + facet_wrap parse");
    assert_eq!(s.color.as_deref(), Some("supplier"));
    assert_eq!(s.facet_wrap.as_deref(), Some("supplier"));
}

#[test]
fn boxplot_requires_y() {
    let err = parse(json!({ "geom": "boxplot", "x": "supplier" }))
        .expect_err("boxplot without y must be rejected");
    assert!(err.to_string().contains("y"), "names the gap: {err}");
}

#[test]
fn y_is_rejected_where_not_meaningful() {
    for geom in ["histogram", "density", "ecdf"] {
        let err = parse(json!({ "geom": geom, "x": "lead_days", "y": "other" }))
            .expect_err("y on a distribution geom must be rejected");
        assert!(
            err.to_string().contains("`y`"),
            "`{geom}` error names y: {err}"
        );
    }
}

#[test]
fn bins_only_meaningful_on_histogram() {
    assert!(parse(json!({ "geom": "histogram", "x": "v", "bins": 20 })).is_ok());
    let err = parse(json!({ "geom": "density", "x": "v", "bins": 20 }))
        .expect_err("bins on a non-histogram geom must be rejected");
    assert!(err.to_string().contains("bins"), "names bins: {err}");
}

// ── annotations ─────────────────────────────────────────────────────────────

#[test]
fn vline_and_p90_annotations_parse() {
    let s = parse(json!({
        "geom": "ecdf", "x": "lead_days",
        "annotations": [
            { "kind": "vline", "at": 14.0, "label": "contract" },
            { "kind": "p90" }
        ]
    }))
    .expect("both annotation kinds parse");
    assert_eq!(s.annotations.len(), 2);
    assert!(matches!(
        &s.annotations[0],
        StatAnnotation::Vline { at, label: Some(l) } if *at == 14.0 && l == "contract"
    ));
    assert!(matches!(
        &s.annotations[1],
        StatAnnotation::P90 { label: None }
    ));
}

#[test]
fn malformed_vline_is_rejected() {
    // Missing `at`.
    let err = parse(json!({
        "geom": "ecdf", "x": "v",
        "annotations": [ { "kind": "vline", "label": "contract" } ]
    }))
    .expect_err("a vline without `at` must be rejected");
    assert!(err.to_string().contains("at"), "names `at`: {err}");

    // Non-numeric `at` — annotation values are type-checked.
    let err = parse(json!({
        "geom": "ecdf", "x": "v",
        "annotations": [ { "kind": "vline", "at": "fourteen" } ]
    }))
    .expect_err("a non-numeric vline `at` must be rejected");
    assert!(!err.to_string().is_empty());
}

#[test]
fn unknown_annotation_kind_is_rejected() {
    let err = parse(json!({
        "geom": "ecdf", "x": "v",
        "annotations": [ { "kind": "arrow", "at": 1.0 } ]
    }))
    .expect_err("an unknown annotation kind must be rejected");
    assert!(err.to_string().contains("arrow"), "names the kind: {err}");
}

// ── rejections ──────────────────────────────────────────────────────────────

#[test]
fn unknown_geom_is_rejected_naming_the_alternatives() {
    let err =
        parse(json!({ "geom": "scatter3d", "x": "v" })).expect_err("unknown geom must be rejected");
    let msg = err.to_string();
    assert!(msg.contains("geom") && msg.contains("scatter3d"), "{msg}");
    assert!(msg.contains("histogram"), "lists the alternatives: {msg}");
}

#[test]
fn unknown_field_is_rejected() {
    let err = parse(json!({ "geom": "density", "x": "v", "colour": "s" }))
        .expect_err("an unknown field must be rejected, not silently dropped");
    assert!(err.to_string().contains("colour"), "names the field: {err}");
}

#[test]
fn missing_x_is_rejected() {
    let err = parse(json!({ "geom": "density" })).expect_err("x is required");
    assert!(err.to_string().contains("x"), "names x: {err}");
}

#[test]
fn non_object_spec_is_rejected() {
    let err = parse(json!("density")).expect_err("a spec must be a JSON object");
    assert!(!err.to_string().is_empty());
}

// ── column cross-checks (the RowSet contract) ───────────────────────────────

#[test]
fn aesthetics_must_name_rowset_columns() {
    let columns = ["lead_days", "supplier"];
    let ok = parse(json!({
        "geom": "density", "x": "lead_days",
        "color": "supplier", "facet_wrap": "supplier"
    }))
    .unwrap();
    ok.check_columns(&columns).expect("all aesthetics resolve");

    for (field, spec) in [
        ("x", json!({ "geom": "density", "x": "nope" })),
        (
            "y",
            json!({ "geom": "boxplot", "x": "supplier", "y": "nope" }),
        ),
        (
            "color",
            json!({ "geom": "density", "x": "lead_days", "color": "nope" }),
        ),
        (
            "facet_wrap",
            json!({ "geom": "density", "x": "lead_days", "facet_wrap": "nope" }),
        ),
    ] {
        let err = parse(spec)
            .unwrap()
            .check_columns(&columns)
            .expect_err("an aesthetic naming a missing column must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("nope") && msg.contains(field),
            "`{field}` error names the aesthetic and the column: {msg}"
        );
    }
}

// ── the composer-injected data key is tolerated ─────────────────────────────

#[test]
fn injected_inline_data_is_tolerated() {
    // The composer injects rows at `data.values` before the artifact ships;
    // the typed parse must accept the composed form too.
    let s = parse(json!({
        "geom": "histogram", "x": "v",
        "data": { "values": [ { "v": 1.0 } ] }
    }))
    .expect("a composed spec (with injected data) still parses");
    assert!(s.data.is_some());
}
