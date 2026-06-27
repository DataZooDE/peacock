//! The render core: the single stateless pivot every surface funnels through
//! (FR-R-1). Pure with respect to `(report skill, params, rows)` (FR-R-2):
//! resolve the report skill, validate params, read each view's rows from
//! escurel with the params bound, and compose the artifact.

use std::collections::BTreeMap;

use peacock_types::{Artifact, Principal, Result};
use serde_json::{Value, json};

use crate::compose::{DEFAULT_MAX_ROWS, compose};
use crate::data::{ReportData, RowSet};
use crate::skill::ReportSkills;

/// Knobs for one render.
#[derive(Debug, Clone)]
pub struct RenderOpts {
    /// Per-view row cap (NFR-P-3).
    pub max_rows: usize,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            max_rows: DEFAULT_MAX_ROWS,
        }
    }
}

/// Render `(report_id, params)` for `principal` against an escurel binding.
/// `escurel` supplies both the report-skill resolution and the row reads, so
/// the embedded face and the service share this exact path (FR-R-1, FR-E-1).
pub async fn render<E>(
    report_id: &str,
    params: &Value,
    principal: &Principal,
    escurel: &E,
    opts: &RenderOpts,
) -> Result<Artifact>
where
    E: ReportData + ReportSkills,
{
    // 1. Resolve the report skill (escurel resolve/expand).
    let skill = escurel.resolve_report(report_id, principal).await?;

    // 2. Validate params against the declared schema and fill defaults — the
    //    absolute parameter vector (FR-R-4, FR-X-2). Type mismatch ⇒ Validation.
    let absolute = skill.params.validate_and_default(params)?;
    let bound: Value = Value::Object(
        absolute
            .iter()
            .map(|(k, v)| (k.clone(), v.0.clone()))
            .collect(),
    );

    // 3. Read each referenced view with the params bound (escurel binds them
    //    as prepared-statement parameters; peacock builds no SQL).
    let mut rows: BTreeMap<String, RowSet> = BTreeMap::new();
    for (alias, query_ref) in &skill.data {
        let rs = escurel.query_view(query_ref, &bound, principal).await?;
        rows.insert(alias.clone(), rs);
    }

    // 4. Compose the one artifact (FR-R-3).
    compose(&skill, &absolute, &rows, opts.max_rows)
}

/// Build the compact view-state record pushed to the model on a committed
/// drill (FR-X-3, ACC-12): `{report_id, params, salient_summary}` — **never
/// rows**. Returned to the surface shells so the MCP-App / chat paths can emit
/// it via `updateModelContext` / the signed token.
pub fn view_state_record(report_id: &str, artifact: &Artifact, summary: &str) -> Value {
    json!({
        "report_id": report_id,
        "params": artifact.structured_content.current_params,
        "salient_summary": summary,
    })
}
