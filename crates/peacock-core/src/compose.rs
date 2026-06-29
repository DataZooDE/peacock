//! The composer: `(report skill, absolute params, rows) → Artifact`, one pure
//! pass (FR-R-2, FR-R-3, FR-V-1/3). Builds the A2UI v0.9 layout (KPI / vega /
//! table / text), the `kind: vega` custom components carrying Vega-Lite specs
//! with the escurel rows injected **inline** (FR-V-3), and the
//! `structuredContent` (rows + param schema + current resolved params,
//! FR-X-1). Guardrails (FR-V-4) and an oversize bound (NFR-P-3) run here.

use std::collections::BTreeMap;

use peacock_types::{Artifact, Error, ParamValue, Result, StructuredContent};
use serde_json::{Value, json};

use crate::data::RowSet;
use crate::guardrail::{check_mosaic_source, check_vega_spec};
use crate::skill::{Agg, ReportSkill, ViewSpec};

/// Default cap on rows a single view may carry into a rendered artifact
/// (NFR-P-3). Beyond this peacock returns a bounded `Render` error rather than
/// rendering unboundedly.
pub const DEFAULT_MAX_ROWS: usize = 10_000;

/// Compose the artifact. `rows` is keyed by the report's data aliases.
/// `bound` is the absolute parameter vector as JSON — it travels with a
/// Mosaic-mode view's escurel-owned data-source reference. `mosaic_threshold`,
/// when `Some(n)`, switches any chart view whose row count exceeds `n` from
/// inline data to Mosaic mode (BRD §7) instead of inlining the rows.
pub fn compose(
    skill: &ReportSkill,
    params: &BTreeMap<String, ParamValue>,
    bound: &Value,
    rows: &BTreeMap<String, RowSet>,
    max_rows: usize,
    mosaic_threshold: Option<usize>,
) -> Result<Artifact> {
    // Oversize guard before building anything (NFR-P-3). Mosaic mode (which
    // streams from escurel rather than inlining) lifts this for chart views;
    // a non-chart view (table/kpi) over `max_rows` still refuses to render.
    for (alias, rs) in rows {
        let n = rs.rows.as_array().map(Vec::len).unwrap_or(0);
        let mosaic_covers = mosaic_threshold.is_some_and(|t| n > t);
        if n > max_rows && !mosaic_covers {
            return Err(Error::render(format!(
                "view `{alias}` returned {n} rows (> {max_rows}); refusing to render unbounded"
            )));
        }
    }
    // Track whether any view rendered in Mosaic mode: the structuredContent
    // summary must then omit the big inline rows (the row count stands in).
    let mut any_mosaic = false;

    let mut components: Vec<Value> = Vec::new();
    let mut vega_specs: Vec<Value> = Vec::new();

    for view in &skill.views {
        match view {
            ViewSpec::Kpi {
                data,
                agg,
                field,
                label,
            } => {
                let rs = rowset(rows, data)?;
                let value = fold(rs, *agg, field);
                components.push(json!({
                    "kind": "kpi",
                    "label": label,
                    "field": field,
                    "value": value,
                }));
            }
            ViewSpec::Vega {
                data,
                spec,
                spec_single,
            } => {
                let rs = rowset(rows, data)?;
                let n = rs.rows.as_array().map(Vec::len).unwrap_or(0);
                // Pick the single-series spec (e.g. a line) when the data has
                // ≤1 colour series — a stacked bar across categories reads well,
                // but a drilled single category reads better as a line.
                let chosen = match spec_single {
                    Some(single) if series_count(skill, spec, rs) <= 1 => single,
                    _ => spec,
                };
                let mut chart = skill.specs.get(chosen).cloned().ok_or_else(|| {
                    Error::render(format!("vega view names unknown spec `{chosen}`"))
                })?;
                // Guardrail the AUTHORED spec first — a remote `data.url` or an
                // expression escape hatch is rejected (ACC-4), not silently
                // stripped.
                check_vega_spec(&chart)?;

                if mosaic_threshold.is_some_and(|t| n > t) {
                    // Mosaic mode: do NOT inline the big rows. Emit a vgplot
                    // spec (mark + encodings, no data) plus the escurel-owned
                    // data-source reference (`query_ref` + bound params) — the
                    // single allow-listed non-inline source.
                    let query_ref = skill.data.get(data).cloned().unwrap_or_default();
                    let source = json!({
                        "connector": "escurel",
                        "query_ref": query_ref,
                        "params": bound.clone(),
                    });
                    check_mosaic_source(&source)?;
                    any_mosaic = true;
                    components.push(json!({
                        "kind": "mosaic",
                        "artifact": {
                            "spec": chart,
                            "source": source,
                            "row_count": n,
                        },
                    }));
                } else {
                    // Default model: inject the escurel rows as inline data.
                    inject_inline_data(&mut chart, rs.rows.clone());
                    vega_specs.push(chart.clone());
                    components.push(json!({ "kind": "vega", "spec": chart }));
                }
            }
            ViewSpec::Table { data } => {
                let rs = rowset(rows, data)?;
                let columns: Vec<&str> = rs.schema.iter().map(|c| c.name.as_str()).collect();
                components.push(json!({
                    "kind": "table",
                    "columns": columns,
                    "rows": rs.rows.clone(),
                }));
            }
        }
    }

    if !skill.narrative.trim().is_empty() {
        components.push(json!({ "kind": "text", "value": skill.narrative.trim() }));
    }

    let a2ui = json!({ "version": "0.9", "components": components });

    // In Mosaic mode the primary rows are too big to inline; the summary keeps
    // the resolved params (the view state, FR-X-1) and an empty rows array (the
    // per-view `row_count` lives on the mosaic component).
    let structured_content = StructuredContent {
        rows: if any_mosaic {
            json!([])
        } else {
            primary_rows(skill, rows)
        },
        param_schema: serde_json::to_value(&skill.params).unwrap_or(Value::Null),
        current_params: params_to_json(params),
    };

    Ok(Artifact {
        a2ui,
        vega_specs,
        structured_content,
        png: None,
    })
}

/// Count the distinct colour-series in a view's rows, per the named spec's
/// `encoding.color.field`. Used to pick a single-series chart on a drill.
fn series_count(skill: &ReportSkill, spec: &str, rs: &RowSet) -> usize {
    let color_field = skill
        .specs
        .get(spec)
        .and_then(|s| s.get("encoding"))
        .and_then(|e| e.get("color"))
        .and_then(|c| c.get("field"))
        .and_then(Value::as_str);
    match color_field {
        Some(field) => {
            let mut seen = std::collections::BTreeSet::new();
            if let Some(arr) = rs.rows.as_array() {
                for row in arr {
                    if let Some(v) = row.get(field).and_then(Value::as_str) {
                        seen.insert(v.to_owned());
                    }
                }
            }
            seen.len()
        }
        None => 1,
    }
}

fn rowset<'a>(rows: &'a BTreeMap<String, RowSet>, alias: &str) -> Result<&'a RowSet> {
    rows.get(alias)
        .ok_or_else(|| Error::render(format!("view references unbound data alias `{alias}`")))
}

/// Inject `data: { values: rows }` into a Vega-Lite spec, replacing any
/// existing `data` (so an authored remote `data.url` can never survive).
fn inject_inline_data(spec: &mut Value, rows: Value) {
    if let Value::Object(map) = spec {
        map.insert("data".to_owned(), json!({ "values": rows }));
    }
}

/// Fold an aggregate over a view's rows — a summary of already-aggregated rows
/// (FR-D-4), never a substitute for a missing view aggregation.
fn fold(rs: &RowSet, agg: Agg, field: &str) -> Value {
    let nums: Vec<f64> = rs
        .rows
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r.get(field).and_then(Value::as_f64))
                .collect()
        })
        .unwrap_or_default();
    match agg {
        Agg::Count => json!(rs.rows.as_array().map(Vec::len).unwrap_or(0)),
        Agg::Sum => json!(nums.iter().sum::<f64>()),
        Agg::Avg if !nums.is_empty() => json!(nums.iter().sum::<f64>() / nums.len() as f64),
        Agg::Avg => Value::Null,
        Agg::Min => nums
            .iter()
            .cloned()
            .fold(None, |acc: Option<f64>, x| {
                Some(acc.map_or(x, |a| a.min(x)))
            })
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
        Agg::Max => nums
            .iter()
            .cloned()
            .fold(None, |acc: Option<f64>, x| {
                Some(acc.map_or(x, |a| a.max(x)))
            })
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
    }
}

/// structuredContent rows: the first data alias's rows (the report's primary
/// view). Deterministic via the skill's ordered data map.
fn primary_rows(skill: &ReportSkill, rows: &BTreeMap<String, RowSet>) -> Value {
    skill
        .data
        .keys()
        .find_map(|alias| rows.get(alias))
        .map(|rs| rs.rows.clone())
        .unwrap_or_else(|| json!([]))
}

fn params_to_json(params: &BTreeMap<String, ParamValue>) -> Value {
    Value::Object(
        params
            .iter()
            .map(|(k, v)| (k.clone(), v.0.clone()))
            .collect(),
    )
}
