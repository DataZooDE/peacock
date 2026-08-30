//! Statistical-spec composition (issue #6): a named spec whose JSON has a
//! top-level `geom` key is a STATISTICAL spec (Vega-Lite uses `mark`, never
//! `geom`). The composer routes it into `artifact.stat_specs` — rows injected
//! inline exactly like a Vega view, so structuredContent / iframe parity
//! holds — and guards it with the stat guardrail. All of this is
//! backend-independent: it happens with or without the `ggplot` feature;
//! only the PNG step is feature-gated.

use std::collections::BTreeMap;

use peacock_core::compose::{DEFAULT_MAX_ROWS, compose};
use peacock_core::data::{Column, RowSet};
use peacock_core::skill::{ReportSkill, ViewSpec};
use peacock_types::{ParamSchema, ParamSpec, ParamType, ParamValue};
use serde_json::{Value, json};

fn line_rows() -> RowSet {
    RowSet {
        rows: json!([
            { "category": "Beverages",  "revenue": 180.0 },
            { "category": "Beverages",  "revenue": 81.0 },
            { "category": "Condiments", "revenue": 110.0 }
        ]),
        schema: vec![
            Column {
                name: "category".into(),
                type_name: "VARCHAR".into(),
            },
            Column {
                name: "revenue".into(),
                type_name: "DOUBLE".into(),
            },
        ],
        truncated: false,
    }
}

fn report(specs: Value) -> ReportSkill {
    ReportSkill {
        id: "northwind-revenue-distribution".into(),
        params: ParamSchema::from_specs([(
            "category",
            ParamSpec::new(ParamType::String).with_default(json!("ALL")),
        )]),
        data: BTreeMap::from([(
            "line_values".to_string(),
            "nw_order_line_values".to_string(),
        )]),
        instances: BTreeMap::new(),
        views: vec![ViewSpec::Vega {
            data: "line_values".into(),
            spec: "revenue_hist".into(),
            spec_single: None,
        }],
        specs: specs.as_object().unwrap().clone().into_iter().collect(),
        narrative: String::new(),
        viewer: None,
        actions: Vec::new(),
    }
}

fn params() -> BTreeMap<String, ParamValue> {
    BTreeMap::from([("category".to_string(), ParamValue::from(json!("ALL")))])
}

fn rows_map() -> BTreeMap<String, RowSet> {
    BTreeMap::from([("line_values".to_string(), line_rows())])
}

fn compose_with(specs: Value) -> peacock_types::Result<peacock_types::Artifact> {
    compose(
        &report(specs),
        &params(),
        &json!({}),
        &rows_map(),
        &BTreeMap::new(),
        DEFAULT_MAX_ROWS,
        None,
    )
}

#[test]
fn stat_spec_composes_into_stat_specs_with_inline_rows() {
    let art = compose_with(json!({
        "revenue_hist": { "geom": "histogram", "x": "revenue", "bins": 20 }
    }))
    .expect("a statistical spec composes without the ggplot feature");

    // Routed to stat_specs, NOT vega_specs (the backend selector's input).
    assert_eq!(art.stat_specs.len(), 1);
    assert!(art.vega_specs.is_empty());

    // Rows are injected inline exactly like a Vega view (parity) — plus the
    // RowSet's SCHEMA, so the ggplot backend types columns from escurel's
    // type names instead of sniffing JSON (issue #8).
    let spec = &art.stat_specs[0];
    assert_eq!(spec["geom"], "histogram");
    assert_eq!(spec["data"]["values"].as_array().unwrap().len(), 3);
    assert_eq!(
        spec["data"]["schema"],
        json!([
            { "name": "category", "type": "VARCHAR" },
            { "name": "revenue",  "type": "DOUBLE" }
        ]),
        "the composed stat spec carries the escurel column schema"
    );

    // The layout carries the stat component; structuredContent has the rows.
    let comps = art.a2ui["components"].as_array().unwrap();
    assert_eq!(comps.iter().filter(|c| c["kind"] == "stat").count(), 1);
    assert_eq!(art.structured_content.rows.as_array().unwrap().len(), 3);
}

#[test]
fn stat_spec_with_forbidden_key_is_rejected() {
    // The same escape hatches as the Vega guardrail (ACC-4): a remote `url`
    // anywhere in a statistical spec is a Render error, not silently stripped.
    let err = compose_with(json!({
        "revenue_hist": {
            "geom": "histogram", "x": "revenue",
            "data": { "url": "https://evil.example/rows.json" }
        }
    }))
    .expect_err("a stat spec with a forbidden key must be rejected");
    assert!(
        err.to_string().contains("disallowed"),
        "error names the guardrail: {err}"
    );
}

#[test]
fn stat_spec_with_unknown_geom_is_rejected() {
    let err = compose_with(json!({
        "revenue_hist": { "geom": "scatter3d", "x": "revenue" }
    }))
    .expect_err("an unknown geom must be rejected at compose");
    assert!(err.to_string().contains("geom"), "error names geom: {err}");
}

#[test]
fn stat_spec_x_must_name_a_column_of_the_rowset() {
    let err = compose_with(json!({
        "revenue_hist": { "geom": "histogram", "x": "no_such_column" }
    }))
    .expect_err("an x that is not a RowSet column must be rejected");
    assert!(
        err.to_string().contains("no_such_column"),
        "error names the column: {err}"
    );
}

#[test]
fn stat_spec_aesthetics_must_name_rowset_columns() {
    // Issue #7: the column check extends beyond x to every aesthetic.
    for spec in [
        json!({ "geom": "density", "x": "revenue", "color": "no_such_column" }),
        json!({ "geom": "density", "x": "revenue", "facet_wrap": "no_such_column" }),
        json!({ "geom": "boxplot", "x": "category", "y": "no_such_column" }),
    ] {
        let err = compose_with(json!({ "revenue_hist": spec }))
            .expect_err("an aesthetic naming a missing column must be rejected at compose");
        assert!(
            err.to_string().contains("no_such_column"),
            "error names the column: {err}"
        );
    }
}

#[test]
fn stat_spec_unknown_field_is_rejected() {
    let err = compose_with(json!({
        "revenue_hist": { "geom": "histogram", "x": "revenue", "colour": "category" }
    }))
    .expect_err("an unknown dialect field must be rejected, not silently dropped");
    assert!(
        err.to_string().contains("colour"),
        "error names the field: {err}"
    );
}

#[test]
fn stat_spec_malformed_annotation_is_rejected() {
    let err = compose_with(json!({
        "revenue_hist": {
            "geom": "histogram", "x": "revenue",
            "annotations": [ { "kind": "vline" } ]
        }
    }))
    .expect_err("a vline without `at` must be rejected at compose");
    assert!(err.to_string().contains("at"), "error names `at`: {err}");
}

#[test]
fn stat_spec_with_annotations_composes() {
    let art = compose_with(json!({
        "revenue_hist": {
            "geom": "ecdf", "x": "revenue",
            "color": "category", "facet_wrap": "category",
            "annotations": [
                { "kind": "vline", "at": 100.0, "label": "contract" },
                { "kind": "p90" }
            ]
        }
    }))
    .expect("the full dialect surface composes (backend-independent)");
    assert_eq!(art.stat_specs.len(), 1);
    assert_eq!(
        art.stat_specs[0]["annotations"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn vega_only_artifact_serializes_without_a_stat_specs_field() {
    // Byte-identity guard: an artifact with no authored stat spec must
    // serialize exactly as before this field existed.
    let art = compose_with(json!({
        "revenue_hist": {
            "mark": "bar",
            "encoding": { "x": { "field": "category" }, "y": { "field": "revenue" } }
        }
    }))
    .expect("a plain Vega spec still composes");
    assert!(art.stat_specs.is_empty());
    let v = serde_json::to_value(&art).unwrap();
    assert!(
        v.get("stat_specs").is_none(),
        "no stat spec authored ⇒ no stat_specs key in the serialized artifact"
    );
}
