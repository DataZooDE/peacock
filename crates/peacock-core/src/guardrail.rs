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

use peacock_types::{Error, Result};
use serde_json::Value;

/// Keys whose mere presence indicates a remote fetch or arbitrary computation.
const FORBIDDEN_KEYS: &[&str] = &["url", "expr", "signal", "signals", "calculate", "loader"];

/// Validate a Vega-Lite spec against the safe subset. `data.values` is the
/// only sanctioned data source.
pub fn check_vega_spec(spec: &Value) -> Result<()> {
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
