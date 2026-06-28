//! Phase 4 pure composer tests (no escurel): A2UI v0.9 structure, inline-data
//! injection, KPI fold, the guardrail (ACC-4), the oversize bound (NFR-P-3),
//! and purity (FR-R-2).

use std::collections::BTreeMap;

use peacock_core::compose::{DEFAULT_MAX_ROWS, compose};
use peacock_core::data::{Column, RowSet};
use peacock_core::skill::{Agg, ReportSkill, ViewSpec};
use peacock_types::{ParamSchema, ParamSpec, ParamType, ParamValue};
use serde_json::{Value, json};

fn nw_rows() -> RowSet {
    RowSet {
        rows: json!([
            { "month": "1997-01-01", "category": "Beverages", "revenue": 180.0 },
            { "month": "1997-02-01", "category": "Beverages", "revenue": 81.0 },
            { "month": "1997-01-01", "category": "Condiments", "revenue": 110.0 }
        ]),
        schema: vec![
            Column {
                name: "month".into(),
                type_name: "VARCHAR".into(),
            },
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

fn report(specs: Value, views: Vec<ViewSpec>) -> ReportSkill {
    let mut data = BTreeMap::new();
    data.insert(
        "rev_by_cat".to_string(),
        "nw_revenue_by_category".to_string(),
    );
    ReportSkill {
        id: "northwind-monthly-revenue".into(),
        params: ParamSchema::from_specs([(
            "category",
            ParamSpec::new(ParamType::String).with_default(json!("ALL")),
        )]),
        data,
        views,
        specs: specs.as_object().unwrap().clone().into_iter().collect(),
        narrative: "EMEA orders only.".into(),
    }
}

fn rev_line_spec() -> Value {
    json!({
        "mark": "line",
        "encoding": {
            "x": { "field": "month", "type": "temporal", "title": "Month" },
            "y": { "field": "revenue", "type": "quantitative", "aggregate": "sum" },
            "color": { "field": "category", "type": "nominal" }
        }
    })
}

fn params() -> BTreeMap<String, ParamValue> {
    BTreeMap::from([("category".to_string(), ParamValue::from(json!("ALL")))])
}

fn rows_map() -> BTreeMap<String, RowSet> {
    BTreeMap::from([("rev_by_cat".to_string(), nw_rows())])
}

#[test]
fn composes_a2ui_v09_with_kpi_vega_inline_and_table() {
    let skill = report(
        json!({ "rev_line": rev_line_spec() }),
        vec![
            ViewSpec::Kpi {
                data: "rev_by_cat".into(),
                agg: Agg::Sum,
                field: "revenue".into(),
                label: "Total revenue".into(),
            },
            ViewSpec::Vega {
                data: "rev_by_cat".into(),
                spec: "rev_line".into(),
                spec_single: None,
            },
            ViewSpec::Table {
                data: "rev_by_cat".into(),
            },
        ],
    );

    let art = compose(&skill, &params(), &rows_map(), DEFAULT_MAX_ROWS).unwrap();

    assert_eq!(art.a2ui["version"], "0.9");
    let comps = art.a2ui["components"].as_array().unwrap();
    // kpi, vega, table, text(narrative).
    assert_eq!(comps.len(), 4);

    // KPI folds sum(revenue) = 180 + 81 + 110 = 371.
    let kpi = &comps[0];
    assert_eq!(kpi["kind"], "kpi");
    assert_eq!(kpi["value"].as_f64().unwrap(), 371.0);

    // Vega component carries the spec with rows injected INLINE (FR-V-3) and
    // no remote URL; the spec is also exposed separately (FR-V-1).
    let vega = &comps[1];
    assert_eq!(vega["kind"], "vega");
    assert_eq!(vega["spec"]["data"]["values"].as_array().unwrap().len(), 3);
    assert!(vega["spec"]["data"].get("url").is_none());
    assert_eq!(art.vega_specs.len(), 1);
    assert_eq!(art.vega_specs[0]["mark"], "line");

    // structuredContent carries rows + schema + current params (FR-X-1).
    assert_eq!(art.structured_content.rows.as_array().unwrap().len(), 3);
    assert_eq!(art.structured_content.current_params["category"], "ALL");
    assert!(
        art.structured_content
            .param_schema
            .get("category")
            .is_some()
    );
}

#[test]
fn guardrail_rejects_remote_data_url() {
    // ACC-4: a spec that loads remote data is a Render error.
    let mut spec = rev_line_spec();
    spec["data"] = json!({ "url": "https://evil.example/rows.json" });
    let skill = report(
        json!({ "rev_line": spec }),
        vec![ViewSpec::Vega {
            data: "rev_by_cat".into(),
            spec: "rev_line".into(),
            spec_single: None,
        }],
    );
    let err = compose(&skill, &params(), &rows_map(), DEFAULT_MAX_ROWS).unwrap_err();
    assert_eq!(err.kind(), "render");
}

#[test]
fn guardrail_rejects_expression_escape_hatch() {
    // ACC-4: an arbitrary-expression feature is a Render error.
    let mut spec = rev_line_spec();
    spec["transform"] = json!([{ "calculate": "datum.revenue * 1000", "as": "x" }]);
    let skill = report(
        json!({ "rev_line": spec }),
        vec![ViewSpec::Vega {
            data: "rev_by_cat".into(),
            spec: "rev_line".into(),
            spec_single: None,
        }],
    );
    let err = compose(&skill, &params(), &rows_map(), DEFAULT_MAX_ROWS).unwrap_err();
    assert_eq!(err.kind(), "render");
}

#[test]
fn oversize_result_set_is_a_bounded_render_error() {
    // NFR-P-3: refuse to render unbounded.
    let skill = report(
        json!({ "rev_line": rev_line_spec() }),
        vec![ViewSpec::Table {
            data: "rev_by_cat".into(),
        }],
    );
    let err = compose(&skill, &params(), &rows_map(), 2).unwrap_err();
    assert_eq!(err.kind(), "render");
}

#[test]
fn composition_is_pure_same_inputs_same_artifact() {
    // FR-R-2: parsed-structure equality across two identical composes.
    let skill = report(
        json!({ "rev_line": rev_line_spec() }),
        vec![
            ViewSpec::Vega {
                data: "rev_by_cat".into(),
                spec: "rev_line".into(),
                spec_single: None,
            },
            ViewSpec::Table {
                data: "rev_by_cat".into(),
            },
        ],
    );
    let a = compose(&skill, &params(), &rows_map(), DEFAULT_MAX_ROWS).unwrap();
    let b = compose(&skill, &params(), &rows_map(), DEFAULT_MAX_ROWS).unwrap();
    assert_eq!(a, b);
}
