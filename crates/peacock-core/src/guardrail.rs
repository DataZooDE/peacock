//! Render guardrails (FR-V-4, NFR-S-3): a rendered artifact must not be able
//! to fetch data or compute beyond the rows escurel released.
//!
//! The safe Vega-Lite subset peacock accepts:
//! - **inline data only** — `data.values` (peacock injects the escurel rows);
//!   no `data.url` / `data.name` / remote loaders anywhere in the spec;
//! - **declarative marks + encodings**, and simple field-predicate
//!   `transform` filters;
//! - **no expression escape hatches** — the keys `expr`, `signal`, `signals`,
//!   and `calculate` are rejected (they evaluate arbitrary expressions).
//!
//! Anything outside the subset is a `Render` error. The check is structural:
//! it walks the whole JSON, so a violation nested anywhere is caught.
//!
//! ## The one allow-listed non-inline source: Mosaic mode
//!
//! Mosaic mode (BRD §7) is the *single* exception to inline-data-only, and it
//! is a tightly bounded one. A Mosaic-mode chart carries no rows; instead it
//! references the **escurel** query as its data source. The invariant:
//!
//! > The only permitted non-inline data source is an **escurel-owned** query
//! > reference — `{ connector: "escurel", query_ref, params }`. It is a typed
//! > ref (never a SQL string, never a URL, never a remote loader); escurel
//! > remains the sole data path and ACL boundary. A `url`/`loader`/`expr`
//! > source is still rejected. The default inline-data rule for normal charts
//! > is unchanged.
//!
//! [`check_mosaic_source`] enforces exactly this allow-list; the Mosaic
//! vgplot **spec** itself (mark + encodings, no data) still goes through
//! [`check_vega_spec`].

use peacock_types::{Error, Result};
use serde_json::Value;

/// Keys whose mere presence indicates a remote fetch or arbitrary computation.
const FORBIDDEN_KEYS: &[&str] = &["url", "expr", "signal", "signals", "calculate", "loader"];

/// Validate a Vega-Lite spec against the safe subset. `data.values` is the
/// only sanctioned data source.
pub fn check_vega_spec(spec: &Value) -> Result<()> {
    walk(spec)
}

/// The statistical geoms the stat-spec dialect declares (issue #6/#7). A
/// spec is STATISTICAL when its JSON carries a top-level `geom` key
/// (Vega-Lite uses `mark`, never `geom`).
pub const STAT_GEOMS: &[&str] = &["histogram", "density", "boxplot", "ecdf"];

/// Validate a STATISTICAL spec: the same no-escape-hatch walk as
/// [`check_vega_spec`] plus the dialect's minimal shape — a JSON object, a
/// known `geom`, and an `x` naming a column of the view's RowSet (`columns`).
/// Backend-independent: this runs at compose whether or not the `ggplot`
/// rasterization feature is enabled.
pub fn check_stat_spec(spec: &Value, columns: &[&str]) -> Result<()> {
    let Value::Object(map) = spec else {
        return Err(Error::render(
            "statistical spec must be a JSON object".to_owned(),
        ));
    };
    let geom = map.get("geom").and_then(Value::as_str).unwrap_or_default();
    if !STAT_GEOMS.contains(&geom) {
        return Err(Error::render(format!(
            "statistical spec names unknown geom `{geom}` (one of {STAT_GEOMS:?})"
        )));
    }
    match map.get("x").and_then(Value::as_str) {
        Some(x) if columns.contains(&x) => {}
        Some(x) => {
            return Err(Error::render(format!(
                "statistical spec's x `{x}` is not a column of the view's rows ({columns:?})"
            )));
        }
        None => {
            return Err(Error::render(
                "statistical spec must name its `x` column".to_owned(),
            ));
        }
    }
    walk(spec)
}

fn walk(v: &Value) -> Result<()> {
    match v {
        Value::Object(map) => {
            for key in map.keys() {
                if FORBIDDEN_KEYS.contains(&key.as_str()) {
                    return Err(Error::render(format!(
                        "chart spec uses disallowed feature `{key}` (inline-data-only, no \
                         expressions) — see the safe Vega-Lite subset"
                    )));
                }
            }
            for child in map.values() {
                walk(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                walk(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Validate a Mosaic-mode data-**source** reference: the one allow-listed
/// non-inline source. It must be an escurel-owned typed `query_ref` (+ params)
/// — `{ connector: "escurel", query_ref: <string>, params: <object|null> }`.
/// Any other connector, a missing/blank `query_ref`, or a SQL/URL escape hatch
/// is a `Render` error: escurel stays the sole data path (FR-D-2/3, NFR-S-1).
pub fn check_mosaic_source(source: &Value) -> Result<()> {
    let connector = source.get("connector").and_then(Value::as_str);
    if connector != Some("escurel") {
        return Err(Error::render(format!(
            "mosaic source connector must be `escurel` (the only allow-listed              non-inline source), got {connector:?}"
        )));
    }
    match source.get("query_ref").and_then(Value::as_str) {
        Some(r) if !r.trim().is_empty() => {}
        _ => {
            return Err(Error::render(
                "mosaic source must carry a non-empty escurel `query_ref` (a typed                  reference, never SQL)"
                    .to_owned(),
            ));
        }
    }
    // No SQL / URL / loader escape hatch may ride along in the source ref.
    walk(source)
}
