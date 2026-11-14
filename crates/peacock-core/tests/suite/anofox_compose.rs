//! Composing `kind: anofox` views (pure, no escurel) and the hard cut of the
//! old chart dialects (issue #44): `kind: vega` views, Vega-Lite `mark` specs
//! and `geom:` statistical specs are clear author errors now.

use std::collections::BTreeMap;

use peacock_core::compose::{DEFAULT_MAX_ROWS, compose};
use peacock_core::data::{Column, RowSet};
use peacock_core::parse_skill_markdown;
use peacock_core::skill::{ReportSkill, ViewSpec};
use peacock_types::{ParamSchema, ParamSpec, ParamType, ParamValue};
use serde_json::{Value, json};

fn rows() -> RowSet {
    RowSet {
        rows: json!([
            { "month": "1997-01-01", "category": "Beverages", "revenue": 180.0 },
            { "month": "1997-02-01", "category": "Beverages", "revenue": 81.0 },
            { "month": "1997-01-01", "category": "Condiments", "revenue": 110.0 }
        ]),
        schema: ["month", "category", "revenue"]
            .iter()
            .map(|n| Column {
                name: (*n).into(),
                type_name: if *n == "revenue" { "DOUBLE" } else { "VARCHAR" }.into(),
            })
            .collect(),
        truncated: false,
    }
}

fn skill(views: Vec<ViewSpec>) -> ReportSkill {
    ReportSkill {
        id: "northwind-monthly-revenue".into(),
        params: ParamSchema::from_specs([(
            "category",
            ParamSpec::new(ParamType::String).with_default(json!("ALL")),
        )]),
        data: BTreeMap::from([("rev".to_string(), "nw_revenue_by_category".to_string())]),
        instances: BTreeMap::new(),
        views,
        specs: BTreeMap::from([
            (
                "rev_bar".to_string(),
                json!({ "title": "Revenue", "roles": [
                    ["month", "XAXIS"], ["category", "CATEGORY"], ["revenue", "BARCHART_STACKED", "Revenue"]
                ]}),
            ),
            (
                "rev_line".to_string(),
                json!({ "roles": [["month", "XAXIS"], ["revenue", "LINECHART", "Revenue"]] }),
            ),
            (
                "rev_hist".to_string(),
                json!({ "roles": [["revenue", "HISTOGRAM"]] }),
            ),
        ]),
        narrative: String::new(),
        viewer: None,
        actions: Vec::new(),
        followups: Vec::new(),
    }
}

fn chart(spec: &str, single: Option<&str>) -> ViewSpec {
    ViewSpec::Chart {
        data: "rev".into(),
        spec: spec.into(),
        spec_single: single.map(str::to_owned),
    }
}

fn run(skill: &ReportSkill, rs: RowSet) -> peacock_types::Artifact {
    compose(
        skill,
        &BTreeMap::from([("category".to_string(), ParamValue::from(json!("ALL")))]),
        &json!({ "category": "ALL" }),
        &BTreeMap::from([("rev".to_string(), rs)]),
        &BTreeMap::new(),
        DEFAULT_MAX_ROWS,
        None,
    )
    .unwrap()
}

#[test]
fn an_anofox_view_composes_a_panel_component() {
    let art = run(&skill(vec![chart("rev_bar", None)]), rows());
    let comp = &art.a2ui["components"][0];
    assert_eq!(comp["kind"], "anofox");
    assert_eq!(comp["spec"]["rows"].as_array().unwrap().len(), 3);
    assert_eq!(comp["spec"]["roles"][2][1], "BARCHART_STACKED");
    assert_eq!(art.anofox_specs.len(), 1);
    assert_eq!(art.anofox_specs[0], comp["spec"]);
}

#[test]
fn every_chart_is_kept_not_only_the_first() {
    let art = run(
        &skill(vec![chart("rev_bar", None), chart("rev_hist", None)]),
        rows(),
    );
    assert_eq!(art.anofox_specs.len(), 2);
}

#[test]
fn chartable_panels_also_carry_derived_vega_for_triton() {
    let art = run(
        &skill(vec![chart("rev_bar", None), chart("rev_hist", None)]),
        rows(),
    );
    // The bar derives; the histogram has no minimal Vega-Lite form.
    assert_eq!(art.vega_specs.len(), 1);
    assert_eq!(art.vega_specs[0]["mark"]["type"], "bar");
    assert_eq!(art.vega_specs[0]["encoding"]["color"]["field"], "category");
}

#[test]
fn spec_single_is_used_when_one_series_remains() {
    let drilled = RowSet {
        rows: json!([
            { "month": "1997-01-01", "category": "Beverages", "revenue": 180.0 },
            { "month": "1997-02-01", "category": "Beverages", "revenue": 81.0 }
        ]),
        ..rows()
    };
    let art = run(&skill(vec![chart("rev_bar", Some("rev_line"))]), drilled);
    assert_eq!(art.anofox_specs[0]["roles"][1][1], "LINECHART");
    let art = run(&skill(vec![chart("rev_bar", Some("rev_line"))]), rows());
    assert_eq!(art.anofox_specs[0]["roles"][2][1], "BARCHART_STACKED");
}

#[test]
fn a_spec_naming_an_unknown_column_fails_the_render() {
    let mut s = skill(vec![chart("rev_bar", None)]);
    s.specs.insert(
        "rev_bar".into(),
        json!({ "roles": [["month", "XAXIS"], ["profit", "BARCHART"]] }),
    );
    let err = compose(
        &s,
        &BTreeMap::new(),
        &json!({}),
        &BTreeMap::from([("rev".to_string(), rows())]),
        &BTreeMap::new(),
        DEFAULT_MAX_ROWS,
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("profit"), "{err}");
}

fn md(views: &str, specs: &str) -> String {
    format!(
        "---\nkind: skill\nid: r\nrender: a2ui\ndata:\n  rev: \"[[query::q]]\"\nviews:\n{views}\nspecs:\n{specs}\n---\nbody\n"
    )
}

#[test]
fn kind_vega_is_a_clear_migration_error() {
    let err = parse_skill_markdown(&md(
        "  - { kind: vega, data: rev, spec: s }",
        "  s: { mark: bar }",
    ))
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("kind: vega") && err.contains("kind: anofox"),
        "{err}"
    );
}

#[test]
fn a_vega_lite_or_geom_spec_is_a_clear_migration_error() {
    for spec in [
        "  s: { mark: bar, encoding: { x: { field: month } } }",
        "  s: { geom: histogram, x: revenue }",
    ] {
        let err = parse_skill_markdown(&md("  - { kind: anofox, data: rev, spec: s }", spec))
            .unwrap_err()
            .to_string();
        assert!(err.contains("roles"), "{spec}: {err}");
    }
}

#[test]
fn kind_anofox_parses() {
    let s = parse_skill_markdown(&md(
        "  - { kind: anofox, data: rev, spec: s, spec_single: t }",
        "  s: { roles: [[month, XAXIS], [revenue, BARCHART]] }\n  t: { roles: [[month, XAXIS], [revenue, LINECHART]] }",
    ))
    .unwrap();
    assert_eq!(
        s.views,
        vec![ViewSpec::Chart {
            data: "rev".into(),
            spec: "s".into(),
            spec_single: Some("t".into())
        }]
    );
}

#[test]
fn the_chart_body_tag_renders_anofox() {
    let mut s = skill(vec![]);
    s.narrative = "Intro.\n\n{{chart: rev spec=rev_bar}}\n".into();
    let art = run(&s, rows());
    let kinds: Vec<&Value> = art.a2ui["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| &c["kind"])
        .collect();
    assert_eq!(kinds, vec!["text", "anofox"]);
}
