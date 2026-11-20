//! Whole anofox dashboards as reports (issue #44, Phase 3).
//!
//! A report skill may carry a compiled anofox dashboard:
//!
//! ```yaml
//! dashboard:
//!   script: |            # the author's dashboard.sql, verbatim
//!     SELECT …::KPI FROM order_lines WHERE …;
//!   queries:             # emitted by `peacock author compile`
//!     - { key: <sha256>, query: "[[query::nw-dashboard-q2]]", params: [from, to], draws: true }
//! ```
//!
//! `peacock author compile` (author time, `peacock-bin`) planned the script
//! and turned every statement into an escurel query page. At render time
//! peacock **sends no SQL**: it prefetches each compiled query page from
//! escurel by reference with the typed params it declares, then hands the
//! script to anofox's pure dashboard renderer and answers its reads from that
//! prefetched set, matched by the statement's content hash. A statement that
//! was not compiled is an error, never a guess; a compiled panel that anofox
//! did not draw (the silent-drop slip) fails the render.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anofox_visualization::Value as AxValue;
use peacock_types::{Error, Principal, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::chart::ChartLook;
use crate::data::{ReportData, RowSet};

/// One compiled statement: the content hash of anofox's planned statement,
/// the escurel query page that answers it, the params that page declares,
/// and whether the statement draws a panel (vs a heading/input/directive).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompiledQuery {
    pub key: String,
    pub query: String,
    #[serde(default)]
    pub params: Vec<String>,
    #[serde(default)]
    pub draws: bool,
}

/// A report skill's compiled dashboard.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DashboardSpec {
    pub script: String,
    pub queries: Vec<CompiledQuery>,
}

impl DashboardSpec {
    /// Parse the `dashboard:` frontmatter block (absent ⇒ `None`).
    pub fn from_frontmatter(id: &str, fm: &Value) -> Result<Option<Self>> {
        let Some(d) = fm.get("dashboard") else {
            return Ok(None);
        };
        let spec: DashboardSpec = serde_json::from_value(d.clone()).map_err(|e| {
            Error::render(format!(
                "report `{id}`: bad `dashboard:` block ({e}) — recompile it with \
                 `peacock author compile`"
            ))
        })?;
        if spec.queries.is_empty() {
            return Err(Error::render(format!(
                "report `{id}`: the dashboard has no compiled queries"
            )));
        }
        for q in &spec.queries {
            if q.key.len() != 64 || !q.key.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::render(format!(
                    "report `{id}`: dashboard query `{}` has a malformed key",
                    q.query
                )));
            }
        }
        Ok(Some(spec))
    }
}

/// The content key of a planned statement (its SHA-256, hex). The compiler
/// records it per query page; the render matches anofox's reads against it.
pub fn statement_key(statement: &str) -> String {
    let digest = Sha256::digest(statement.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// `[[query::id]]` / `[[id]]` / `id` → `id`.
fn query_ref(link: &str) -> &str {
    let inner = link.trim().trim_start_matches("[[").trim_end_matches("]]");
    inner.rsplit_once("::").map(|(_, id)| id).unwrap_or(inner)
}

/// The painted dashboard.
pub struct Painted {
    pub svg: String,
    /// Panels drawn (the compiled `draws` statements anofox read).
    pub panels: usize,
    /// Rows of the first drawn panel (the structuredContent summary).
    pub primary_rows: Value,
    /// Set when escurel's row cap clipped a compiled query (#46): the first
    /// drawn panel's query if it was clipped, else the first clipped query.
    pub truncated: Option<peacock_types::Truncation>,
}

/// Prefetch every compiled query page, then render the script through anofox.
pub async fn render_dashboard<E: ReportData>(
    spec: &DashboardSpec,
    bound: &Value,
    principal: &Principal,
    escurel: &E,
    look: &ChartLook,
    trace: Option<&crate::TraceSink>,
) -> Result<Painted> {
    // 1. Read each compiled query page by REFERENCE with only the params it
    //    declares (escurel binds them as prepared-statement parameters).
    let mut by_key: HashMap<String, (usize, RowSet)> = HashMap::new();
    for (i, q) in spec.queries.iter().enumerate() {
        let params: serde_json::Map<String, Value> = q
            .params
            .iter()
            .map(|p| {
                bound
                    .get(p)
                    .cloned()
                    .map(|v| (p.clone(), v))
                    .ok_or_else(|| {
                        Error::render(format!(
                            "dashboard query `{}` binds `{p}`, which the report does not declare",
                            q.query
                        ))
                    })
            })
            .collect::<Result<_>>()?;
        let rs = escurel
            .query_view(
                query_ref(&q.query),
                &Value::Object(params),
                principal,
                trace,
            )
            .await?;
        by_key.insert(q.key.clone(), (i, rs));
    }

    // 2. Render (pure). anofox's reads are answered from the prefetched set.
    let mut provider = Prefetched {
        by_key: &by_key,
        hits: BTreeSet::new(),
        refused: Vec::new(),
    };
    // The look rides the anofox theme: tokens, variant, and the locale.
    let theme = match (&look.theme, &look.locale) {
        (Some(t), locale) => {
            let mut j = t.to_anofox_json(look.variant);
            if let Some(l) = locale {
                j["locale"] = Value::String(l.clone());
            }
            Some(j.to_string())
        }
        (None, Some(l)) => Some(serde_json::json!({ "extends": "light", "locale": l }).to_string()),
        (None, None) => None,
    };
    let opts = anofox_visualization::dashboard::DashboardOptions {
        theme,
        variant: match look.variant {
            crate::chart::Variant::Dark => anofox_visualization::theme::Variant::Dark,
            crate::chart::Variant::Light => anofox_visualization::theme::Variant::Light,
        },
        ..Default::default()
    };
    let svg =
        anofox_visualization::dashboard::render_dashboard_svg(&spec.script, &mut provider, &opts)
            .map_err(|e| Error::render(format!("anofox dashboard: {e}")))?;
    if let Some(r) = provider.refused.first() {
        return Err(Error::render(format!(
            "dashboard: {r} — the script and its compiled query pages disagree; \
             recompile with `peacock author compile`"
        )));
    }
    crate::chart::check_inert(&svg)?;

    // 3. The silent-drop guard: every compiled panel must have been drawn.
    let expected: Vec<usize> = spec
        .queries
        .iter()
        .enumerate()
        .filter(|(_, q)| q.draws)
        .map(|(i, _)| i)
        .collect();
    let missing: Vec<&str> = expected
        .iter()
        .filter(|i| !provider.hits.contains(i))
        .map(|i| spec.queries[*i].query.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(Error::render(format!(
            "dashboard drew {} of {} compiled panels (not drawn: {}) — the script and its \
             compiled query pages disagree; recompile with `peacock author compile`",
            expected.len() - missing.len(),
            expected.len(),
            missing.join(", ")
        )));
    }
    let primary_rows = expected
        .first()
        .and_then(|i| by_key.values().find(|(j, _)| j == i))
        .map(|(_, rs)| rs.rows.clone())
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let clipped = |i: usize| {
        by_key
            .values()
            .find(|(j, rs)| *j == i && rs.truncated)
            .map(|(_, rs)| {
                let n = rs.rows.as_array().map(Vec::len).unwrap_or(0);
                peacock_types::Truncation {
                    rows_shown: n,
                    rows_total: None,
                    limit: n,
                    view: query_ref(&spec.queries[i].query).to_owned(),
                }
            })
    };
    let truncated = expected
        .first()
        .and_then(|i| clipped(*i))
        .or_else(|| (0..spec.queries.len()).find_map(clipped));
    Ok(Painted {
        svg,
        panels: expected.len(),
        primary_rows,
        truncated,
    })
}

/// anofox's [`DataProvider`](anofox_visualization::dashboard::DataProvider)
/// over the prefetched escurel rows. Never forwards a statement anywhere.
struct Prefetched<'a> {
    by_key: &'a HashMap<String, (usize, RowSet)>,
    hits: BTreeSet<usize>,
    refused: Vec<String>,
}

impl anofox_visualization::dashboard::DataProvider for Prefetched<'_> {
    fn execute(&mut self, statement: &str) -> std::result::Result<(), String> {
        // An input binds its default with `SET VARIABLE`; in peacock the
        // report's params are bound by escurel instead, so that is a no-op.
        // Anything else is setup, which compile rejects — refuse it.
        if statement
            .trim_start()
            .get(..12)
            .is_some_and(|p| p.eq_ignore_ascii_case("SET VARIABLE"))
        {
            return Ok(());
        }
        let msg = "a setup statement reached the renderer".to_owned();
        self.refused.push(msg.clone());
        Err(msg)
    }

    fn query(
        &mut self,
        statement: &str,
    ) -> std::result::Result<Vec<(String, Vec<AxValue>)>, String> {
        match self.by_key.get(&statement_key(statement)) {
            Some((i, rs)) => {
                self.hits.insert(*i);
                Ok(columns(rs))
            }
            None => {
                let msg = "a dashboard statement has no compiled query page".to_owned();
                self.refused.push(msg.clone());
                Err(msg)
            }
        }
    }
}

/// escurel rows (JSON objects + Arrow-typed schema) → anofox columns in
/// SELECT order, typed from the schema rather than sniffed from the JSON.
fn columns(rs: &RowSet) -> Vec<(String, Vec<AxValue>)> {
    let rows = rs.rows.as_array().map(Vec::as_slice).unwrap_or_default();
    rs.schema
        .iter()
        .map(|c| {
            let kind = ColKind::of(&c.type_name);
            let vals = rows.iter().map(|r| kind.value(r.get(&c.name))).collect();
            (c.name.clone(), vals)
        })
        .collect()
}

#[derive(Clone, Copy)]
enum ColKind {
    Text,
    Int,
    Float,
    Bool,
    Temporal,
}

impl ColKind {
    /// From escurel's column type name (Arrow `DataType` Debug form, e.g.
    /// `Utf8`, `Float64`, `Date32`, `Timestamp(Microsecond, None)`).
    fn of(type_name: &str) -> Self {
        let t = type_name.trim();
        if t.starts_with("Date") || t.starts_with("Timestamp") {
            ColKind::Temporal
        } else if t.starts_with("Int") || t.starts_with("UInt") {
            ColKind::Int
        } else if t.starts_with("Float") || t.starts_with("Decimal") {
            ColKind::Float
        } else if t == "Boolean" {
            ColKind::Bool
        } else {
            ColKind::Text
        }
    }

    fn value(self, v: Option<&Value>) -> AxValue {
        let Some(v) = v.filter(|v| !v.is_null()) else {
            return AxValue::Na;
        };
        match self {
            ColKind::Int => v
                .as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                .map(AxValue::Integer)
                .unwrap_or(AxValue::Na),
            ColKind::Float => v
                .as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                .map(AxValue::Float)
                .unwrap_or(AxValue::Na),
            ColKind::Bool => v.as_bool().map(AxValue::Bool).unwrap_or(AxValue::Na),
            ColKind::Temporal => v
                .as_str()
                .and_then(epoch_seconds)
                .map(AxValue::DateTime)
                .unwrap_or_else(|| AxValue::Str(v.to_string())),
            ColKind::Text => match v {
                Value::String(s) => AxValue::Str(s.clone()),
                other => AxValue::Str(other.to_string()),
            },
        }
    }
}

/// `YYYY-MM-DD[ T]HH:MM:SS[.fff][Z]` → seconds since the epoch (UTC).
fn epoch_seconds(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.get(4..5) != Some("-") || s.get(7..8) != Some("-") {
        return None;
    }
    let (hh, mm, ss) = if s.len() >= 19 {
        (num(11, 13)?, num(14, 16)?, num(17, 19)?)
    } else {
        (0, 0, 0)
    };
    Some(days_from_civil(y, m, d) * 86_400 + hh * 3_600 + mm * 60 + ss)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (H. Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Map a report's absolute params to the JSON object the queries bind from.
pub fn bound_params(absolute: &BTreeMap<String, peacock_types::ParamValue>) -> Value {
    Value::Object(
        absolute
            .iter()
            .map(|(k, v)| (k.clone(), v.0.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_of_known_dates() {
        assert_eq!(epoch_seconds("1970-01-01"), Some(0));
        assert_eq!(epoch_seconds("1997-01-01"), Some(852_076_800));
        assert_eq!(epoch_seconds("1997-01-01T00:00:00"), Some(852_076_800));
        assert_eq!(epoch_seconds("1997-01-01 12:00:00"), Some(852_120_000));
        assert_eq!(epoch_seconds("Beverages"), None);
    }

    #[test]
    fn statement_key_is_stable_sha256() {
        assert_eq!(
            statement_key(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
