//! The report-skill model and its escurel resolution (FR-D-1).
//!
//! A report is an escurel skill (`render: a2ui`) whose front matter declares
//! render `params`, binds data by typed reference (`data`), lays out `views`,
//! and carries named Vega-Lite `specs`. peacock resolves it via
//! `escurel-client` (`resolve` + `expand`) and parses the frontmatter into
//! this typed model. It executes no SQL and reads no credential.

use async_trait::async_trait;
use escurel_client::{Client, ExpandRequest, ResolveRequest};
use peacock_types::{Error, ParamSchema, Principal, Result};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::Value;

/// One laid-out view in a report.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewSpec {
    /// A single aggregate tile: `agg(field)` folded over the view's rows.
    Kpi {
        data: String,
        agg: Agg,
        field: String,
        label: String,
    },
    /// A chart: a named Vega-Lite spec rendered with the view's rows inline.
    /// `spec_single` (optional) is used instead when the data resolves to a
    /// single colour series — e.g. a stacked bar across categories, but a line
    /// when drilled to one category.
    Vega {
        data: String,
        spec: String,
        spec_single: Option<String>,
    },
    /// A data table over the view's rows.
    Table { data: String },
}

/// KPI aggregation functions (the only client-side fold peacock does — a
/// summary of already-aggregated rows, never a substitute for a view, FR-D-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agg {
    Sum,
    Count,
    Min,
    Max,
    Avg,
}

/// A parsed report skill.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportSkill {
    pub id: String,
    pub params: ParamSchema,
    /// Alias → escurel query ref (the structured-data-view read).
    pub data: std::collections::BTreeMap<String, String>,
    pub views: Vec<ViewSpec>,
    /// Named Vega-Lite specs the `vega` views reference.
    pub specs: std::collections::BTreeMap<String, Value>,
    /// The agent-authored narrative (skill body).
    pub narrative: String,
}

impl ReportSkill {
    /// Parse a report skill from its escurel frontmatter + body.
    pub fn from_frontmatter(id: &str, fm: &Value, body: &str) -> Result<Self> {
        let params: ParamSchema = match fm.get("params") {
            Some(p) => serde_json::from_value(p.clone())
                .map_err(|e| Error::render(format!("report `{id}`: bad params block: {e}")))?,
            None => ParamSchema::default(),
        };

        let mut data = std::collections::BTreeMap::new();
        if let Some(obj) = fm.get("data").and_then(Value::as_object) {
            for (alias, link) in obj {
                let link = link.as_str().ok_or_else(|| {
                    Error::render(format!(
                        "report `{id}`: data.{alias} must be a wikilink string"
                    ))
                })?;
                data.insert(alias.clone(), query_ref_of(link));
            }
        }

        let specs = fm
            .get("specs")
            .and_then(Value::as_object)
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let views = parse_views(id, fm)?;

        Ok(ReportSkill {
            id: id.to_owned(),
            params,
            data,
            views,
            specs,
            narrative: body.to_owned(),
        })
    }
}

/// Extract the bare ref from a wikilink: `[[query::nw_x]]` → `nw_x`,
/// `[[nw_x]]` → `nw_x`, plain `nw_x` → `nw_x`.
fn query_ref_of(link: &str) -> String {
    let inner = link.trim().trim_start_matches("[[").trim_end_matches("]]");
    match inner.rsplit_once("::") {
        Some((_, id)) => id.to_owned(),
        None => inner.to_owned(),
    }
}

fn parse_views(id: &str, fm: &Value) -> Result<Vec<ViewSpec>> {
    let arr = match fm.get("views") {
        Some(Value::Array(a)) => a,
        Some(_) => {
            return Err(Error::render(format!(
                "report `{id}`: views must be a list"
            )));
        }
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let kind = v
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::render(format!("report `{id}`: a view is missing its `kind`")))?;
        let data = v
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let view = match kind {
            "kpi" => ViewSpec::Kpi {
                data,
                agg: v
                    .get("agg")
                    .and_then(|a| serde_json::from_value::<Agg>(a.clone()).ok())
                    .unwrap_or(Agg::Sum),
                field: v
                    .get("field")
                    .and_then(Value::as_str)
                    .unwrap_or("value")
                    .to_owned(),
                label: v
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("Total")
                    .to_owned(),
            },
            "vega" => ViewSpec::Vega {
                data,
                spec: v
                    .get("spec")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Error::render(format!("report `{id}`: a vega view names no `spec`"))
                    })?
                    .to_owned(),
                spec_single: v
                    .get("spec_single")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            },
            "table" => ViewSpec::Table { data },
            other => {
                return Err(Error::render(format!(
                    "report `{id}`: unknown view kind `{other}`"
                )));
            }
        };
        out.push(view);
    }
    Ok(out)
}

/// The report-skill resolution port (kept separate from row reads so the
/// embedded face can supply its own escurel binding, FR-E-3).
#[async_trait]
pub trait ReportSkills: Send + Sync {
    /// Resolve + expand the report skill. When `trace` is `Some`, the **real**
    /// resolve request and the literal frontmatter/body escurel returned are
    /// recorded into it.
    async fn resolve_report(
        &self,
        report_id: &str,
        principal: &Principal,
        trace: Option<&crate::TraceSink>,
    ) -> Result<ReportSkill>;
}

#[async_trait]
impl ReportSkills for crate::data::EscurelData {
    async fn resolve_report(
        &self,
        report_id: &str,
        principal: &Principal,
        trace: Option<&crate::TraceSink>,
    ) -> Result<ReportSkill> {
        let token = SecretString::from(principal.raw_token.clone());
        let client = Client::connect(self.endpoint(), token)
            .await
            .map_err(crate::data::map_err)?;

        // `[[skill::<id>]]` resolves the skill page itself — escurel treats
        // `skill::` as a reserved namespace meaning "the skill definition"
        // (escurel #212).
        let wikilink = format!("[[skill::{report_id}]]");
        let resolved = client
            .resolve(ResolveRequest {
                wikilink: wikilink.clone(),
                scenario: String::new(),
            })
            .await
            .map_err(crate::data::map_err)?;
        let page = resolved
            .page
            .ok_or_else(|| Error::data(format!("report skill `{report_id}` not found")))?;

        let expanded = client
            .expand(ExpandRequest {
                page_id: page.page_id.clone(),
                ..Default::default()
            })
            .await
            .map_err(crate::data::map_err)?;

        // Record the genuine resolve/expand exchange — the verbatim frontmatter
        // escurel released is what the report skill is parsed from.
        crate::record(
            trace,
            serde_json::json!({
                "hop": "peacock→escurel",
                "method": "resolve + expand",
                "request": { "wikilink": wikilink, "scenario": "" },
                "response": {
                    "page_id": page.page_id,
                    "frontmatter": expanded.frontmatter,
                    "body": expanded.body
                }
            }),
        );

        ReportSkill::from_frontmatter(report_id, &expanded.frontmatter, &expanded.body)
    }
}
