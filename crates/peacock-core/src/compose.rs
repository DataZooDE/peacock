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
use crate::guardrail::{check_mosaic_source, check_stat_spec, check_vega_spec};
use crate::instance::InstancePage;
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
    pages: &BTreeMap<String, InstancePage>,
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
    let mut stat_specs: Vec<Value> = Vec::new();

    // Two layouts. When the body carries inline `{{tags}}` (the "markdown +
    // tags" page format), render its segments in document order — narrative
    // text interleaved with the tagged views + the follow-up buttons. Otherwise
    // the classic layout: the frontmatter `views:` in order, then the narrative.
    let segments = crate::body_tags::parse_body(&skill.narrative);
    let has_tags = segments
        .iter()
        .any(|s| matches!(s, crate::body_tags::BodySegment::Tag(_)));

    if has_tags {
        for seg in &segments {
            match seg {
                crate::body_tags::BodySegment::Markdown(md) => {
                    let t = md.trim();
                    if !t.is_empty() {
                        components.push(json!({ "kind": "text", "value": t }));
                    }
                }
                crate::body_tags::BodySegment::Tag(tag) => {
                    if tag.name == "followups" {
                        components.extend(followups_buttons(&skill.followups));
                    } else {
                        let view = tag_to_view(tag).ok_or_else(|| {
                            Error::render(format!(
                                "report `{}`: unknown body tag `{{{{{}}}}}`",
                                skill.id, tag.name
                            ))
                        })?;
                        components.push(render_view(
                            &view,
                            skill,
                            rows,
                            pages,
                            bound,
                            mosaic_threshold,
                            &mut vega_specs,
                            &mut stat_specs,
                            &mut any_mosaic,
                        )?);
                    }
                }
            }
        }
    } else {
        for view in &skill.views {
            components.push(render_view(
                view,
                skill,
                rows,
                pages,
                bound,
                mosaic_threshold,
                &mut vega_specs,
                &mut stat_specs,
                &mut any_mosaic,
            )?);
        }
        if !skill.narrative.trim().is_empty() {
            components.push(json!({ "kind": "text", "value": skill.narrative.trim() }));
        }
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
        instances: instances_content(skill, pages),
        document: None,
    };

    Ok(Artifact {
        a2ui,
        vega_specs,
        stat_specs,
        structured_content,
        png: None,
    })
}

/// Render ONE view into its A2UI component, pushing any Vega/stat spec onto the
/// tracking vectors and flagging Mosaic mode. Shared by the classic `views:`
/// loop and the inline-tag body layout, so both produce byte-identical
/// components for the same view.
#[allow(clippy::too_many_arguments)]
fn render_view(
    view: &ViewSpec,
    skill: &ReportSkill,
    rows: &BTreeMap<String, RowSet>,
    pages: &BTreeMap<String, InstancePage>,
    bound: &Value,
    mosaic_threshold: Option<usize>,
    vega_specs: &mut Vec<Value>,
    stat_specs: &mut Vec<Value>,
    any_mosaic: &mut bool,
) -> Result<Value> {
    match view {
        ViewSpec::Kpi {
            data,
            agg,
            field,
            label,
        } => {
            let rs = rowset(rows, data)?;
            let value = fold(rs, *agg, field);
            Ok(json!({ "kind": "kpi", "label": label, "field": field, "value": value }))
        }
        ViewSpec::Vega {
            data,
            spec,
            spec_single,
        } => {
            let rs = rowset(rows, data)?;
            let n = rs.rows.as_array().map(Vec::len).unwrap_or(0);
            // Pick the single-series spec (e.g. a line) when the data has ≤1
            // colour series — a stacked bar across categories reads well, but a
            // drilled single category reads better as a line.
            let chosen = match spec_single {
                Some(single) if series_count(skill, spec, rs) <= 1 => single,
                _ => spec,
            };
            let mut chart =
                skill.specs.get(chosen).cloned().ok_or_else(|| {
                    Error::render(format!("vega view names unknown spec `{chosen}`"))
                })?;

            // A top-level `geom` key marks a STATISTICAL spec (issue #6;
            // Vega-Lite uses `mark`, never `geom`): guardrail it, inject the
            // rows exactly like a Vega view (structuredContent / iframe parity),
            // and route it to `stat_specs`. Stat views always inline.
            if chart.get("geom").is_some() {
                let columns: Vec<&str> = rs.schema.iter().map(|c| c.name.as_str()).collect();
                check_stat_spec(&chart, &columns)?;
                inject_stat_data(&mut chart, rs);
                stat_specs.push(chart.clone());
                return Ok(json!({ "kind": "stat", "spec": chart }));
            }

            // Guardrail the AUTHORED spec first — a remote `data.url` or an
            // expression escape hatch is rejected (ACC-4), not silently stripped.
            check_vega_spec(&chart)?;

            if mosaic_threshold.is_some_and(|t| n > t) {
                let query_ref = skill.data.get(data).cloned().unwrap_or_default();
                let source = json!({
                    "connector": "escurel",
                    "query_ref": query_ref,
                    "params": bound.clone(),
                });
                check_mosaic_source(&source)?;
                *any_mosaic = true;
                Ok(json!({
                    "kind": "mosaic",
                    "artifact": { "spec": chart, "source": source, "row_count": n },
                }))
            } else {
                inject_inline_data(&mut chart, rs.rows.clone());
                vega_specs.push(chart.clone());
                Ok(json!({ "kind": "vega", "spec": chart }))
            }
        }
        ViewSpec::Table { data } => {
            let rs = rowset(rows, data)?;
            let columns: Vec<&str> = rs.schema.iter().map(|c| c.name.as_str()).collect();
            Ok(json!({ "kind": "table", "columns": columns, "rows": rs.rows.clone() }))
        }
        ViewSpec::Markdown { instance } => {
            let page = instance_page(pages, instance)?;
            Ok(json!({ "kind": "markdown", "value": page.body }))
        }
        ViewSpec::Frontmatter {
            instance,
            keys,
            label,
        } => {
            let page = instance_page(pages, instance)?;
            let facts: Vec<Value> = keys
                .iter()
                .filter_map(|k| {
                    page.frontmatter
                        .get(k)
                        .map(|v| json!({ "key": k, "value": v }))
                })
                .collect();
            Ok(json!({ "kind": "frontmatter", "label": label, "facts": facts }))
        }
        ViewSpec::Timeline { instance, limit } => {
            let page = instance_page(pages, instance)?;
            let events: Vec<Value> = page
                .events
                .iter()
                .take(*limit as usize)
                .map(event_json)
                .collect();
            Ok(json!({ "kind": "timeline", "events": events }))
        }
    }
}

/// Map an inline body tag to the [`ViewSpec`] it renders. The tag's `primary`
/// bareword (or `data`/`instance` arg) is the alias; keyed args tune the view.
/// Returns `None` for an unknown tag name (the caller errors with the name).
fn tag_to_view(tag: &crate::body_tags::BodyTag) -> Option<ViewSpec> {
    let alias = || tag.alias().unwrap_or("rows").to_string();
    let arg = |k: &str| tag.args.get(k).cloned();
    match tag.name.as_str() {
        // `{{chart: rows spec=convergence spec_single=conv_line}}`
        "chart" | "vega" => Some(ViewSpec::Vega {
            data: alias(),
            spec: arg("spec").unwrap_or_else(|| "chart".to_string()),
            spec_single: arg("spec_single"),
        }),
        "table" => Some(ViewSpec::Table { data: alias() }),
        "kpi" => Some(ViewSpec::Kpi {
            data: alias(),
            agg: parse_agg(arg("agg").as_deref()),
            field: arg("field").unwrap_or_default(),
            label: arg("label").unwrap_or_default(),
        }),
        // `{{markdown: instance}}`
        "markdown" => Some(ViewSpec::Markdown { instance: alias() }),
        // `{{summary: instance keys=a,b,c label=Summary}}` — a frontmatter block
        // (the "summary section like the skill frontmatter").
        "summary" | "frontmatter" => Some(ViewSpec::Frontmatter {
            instance: alias(),
            keys: arg("keys")
                .map(|s| {
                    s.split(',')
                        .map(|k| k.trim().to_string())
                        .filter(|k| !k.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            label: arg("label").unwrap_or_default(),
        }),
        "timeline" => Some(ViewSpec::Timeline {
            instance: alias(),
            limit: arg("limit").and_then(|s| s.parse().ok()).unwrap_or(10),
        }),
        _ => None,
    }
}

/// Parse a `{{kpi agg=…}}` aggregate name (reusing `Agg`'s lowercase serde),
/// defaulting to `sum` — same default as the frontmatter `kpi` view.
fn parse_agg(s: Option<&str>) -> crate::skill::Agg {
    s.and_then(|s| serde_json::from_value(json!(s)).ok())
        .unwrap_or(crate::skill::Agg::Sum)
}

/// The `{{followups}}` buttons: one re-ask `button` component per declared
/// follow-up, in the EXACT shape the chat surfaces already render (a
/// `render_report`-less button whose `args.question` re-asks the agent —
/// agent-core `AssistantTool`), so GE / Teams / Google Chat / Copilot render
/// them with no per-surface change. Empty ⇒ no components.
fn followups_buttons(followups: &[crate::skill::FollowupButton]) -> Vec<Value> {
    followups
        .iter()
        .map(|f| {
            json!({
                "kind": "button",
                "label": f.label,
                "tool": "assistant",
                "args": { "question": f.question },
            })
        })
        .collect()
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

fn instance_page<'a>(
    pages: &'a BTreeMap<String, InstancePage>,
    alias: &str,
) -> Result<&'a InstancePage> {
    pages
        .get(alias)
        .ok_or_else(|| Error::render(format!("view references unbound instance alias `{alias}`")))
}

/// The instance contract for structuredContent, built FROM THE VIEWS (data
/// minimality): a facts view contributes its selected keys, a markdown view
/// the body — never the raw full frontmatter. `None` when the report
/// declares no instances, so row-report artifacts stay byte-identical.
fn instances_content(skill: &ReportSkill, pages: &BTreeMap<String, InstancePage>) -> Option<Value> {
    if skill.instances.is_empty() {
        return None;
    }
    let mut out = serde_json::Map::new();
    for (alias, page) in pages {
        let mut entry = serde_json::Map::new();
        entry.insert("skill".to_owned(), json!(page.skill));
        entry.insert("id".to_owned(), json!(page.id));
        entry.insert("page_id".to_owned(), json!(page.page_id));
        for view in &skill.views {
            match view {
                ViewSpec::Frontmatter { instance, keys, .. } if instance == alias => {
                    let facts: Vec<Value> = keys
                        .iter()
                        .filter_map(|k| {
                            page.frontmatter
                                .get(k)
                                .map(|v| json!({ "key": k, "value": v }))
                        })
                        .collect();
                    entry.insert("facts".to_owned(), json!(facts));
                }
                ViewSpec::Markdown { instance } if instance == alias => {
                    entry.insert("markdown".to_owned(), json!(page.body));
                }
                ViewSpec::Timeline { instance, limit } if instance == alias => {
                    let events: Vec<Value> = page
                        .events
                        .iter()
                        .take(*limit as usize)
                        .map(event_json)
                        .collect();
                    entry.insert("events".to_owned(), json!(events));
                }
                _ => {}
            }
        }
        out.insert(alias.clone(), Value::Object(entry));
    }
    Some(Value::Object(out))
}

/// One timeline event's JSON shape (shared by the component and the typed
/// structuredContent contract).
fn event_json(e: &crate::instance::InstanceEvent) -> Value {
    json!({
        "at": e.at,
        "source": e.source,
        "label": e.label,
        "title": e.title,
        "body": e.body,
    })
}

/// Inject `data: { values: rows }` into a Vega-Lite spec, replacing any
/// existing `data` (so an authored remote `data.url` can never survive).
fn inject_inline_data(spec: &mut Value, rows: Value) {
    if let Value::Object(map) = spec {
        map.insert("data".to_owned(), json!({ "values": rows }));
    }
}

/// Inject `data: { values, schema }` into a STATISTICAL spec — the rows plus
/// escurel's column schema, so the ggplot backend types every column from the
/// reported type names instead of sniffing the JSON (issue #8). Replaces any
/// authored `data` (same escape-hatch rule as [`inject_inline_data`]).
fn inject_stat_data(spec: &mut Value, rs: &RowSet) {
    if let Value::Object(map) = spec {
        let schema: Vec<Value> = rs
            .schema
            .iter()
            .map(|c| json!({ "name": c.name, "type": c.type_name }))
            .collect();
        map.insert(
            "data".to_owned(),
            json!({ "values": rs.rows.clone(), "schema": schema }),
        );
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
