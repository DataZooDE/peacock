//! The render core: the single stateless pivot every surface funnels through
//! (FR-R-1). Pure with respect to `(report skill, params, rows)` (FR-R-2):
//! resolve the report skill, validate params, read each view's rows from
//! escurel with the params bound, and compose the artifact.

use std::collections::BTreeMap;

use peacock_types::{Artifact, Principal, Result, SharedSelection};
use serde_json::{Value, json};

use crate::compose::{DEFAULT_MAX_ROWS, compose};
use crate::data::{ReportData, RowSet};
use crate::skill::ReportSkills;

/// Knobs for one render.
#[derive(Debug, Clone)]
pub struct RenderOpts {
    /// Per-view row cap (NFR-P-3).
    pub max_rows: usize,
    /// Rasterize the first chart to PNG and attach it to the artifact
    /// (the chat surface / embedded preview path, FR-C-2/FR-E-2). `None`
    /// skips rasterization; `Some(scale)` renders at `scale` ≥ 1.0.
    pub png_scale: Option<f32>,
    /// Optional theme applied to the rasterized chart (corporate identity ⊕
    /// host look). `None` renders peacock's stock palette. The matching CSS for
    /// the web surfaces is attached at the service boundary, not here.
    pub theme: Option<peacock_rasterizer::ThemeTokens>,
    /// Optional sink capturing the **real** escurel wire payloads this render
    /// issues (resolve + each query_instance, request and response). Used by the
    /// demo's "under the hood" inspector to show genuine — not reconstructed —
    /// traffic. `None` records nothing.
    pub trace: Option<crate::TraceSink>,
    /// Optional shared exploration selection the conversation context holds
    /// (FR-X-6 / OQ-5). When set and the report's param schema declares a param
    /// named after the selection's `dimension`, the report **inherits** that
    /// value ("now show me this in the other chart"). A report that does not
    /// declare the dimension silently ignores it. An absolute param the caller
    /// supplies always wins over the selection (the projections cannot drift,
    /// HLD §state-sync). peacock holds none of this — it is an input only.
    pub selection: Option<SharedSelection>,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            max_rows: DEFAULT_MAX_ROWS,
            png_scale: None,
            theme: None,
            trace: None,
            selection: None,
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
    let skill = escurel
        .resolve_report(report_id, principal, opts.trace.as_ref())
        .await?;

    // 2. Inherit the shared exploration selection when the report declares the
    //    selection's dimension and the caller left it unset (FR-X-6 / OQ-5).
    //    A report without that param ignores the selection; a caller-supplied
    //    absolute param always wins (no drift, HLD §state-sync).
    let params = apply_selection(params, opts.selection.as_ref(), &skill.params);

    // 3. Validate params against the declared schema and fill defaults — the
    //    absolute parameter vector (FR-R-4, FR-X-2). Type mismatch ⇒ Validation.
    let absolute = skill.params.validate_and_default(&params)?;
    let bound: Value = Value::Object(
        absolute
            .iter()
            .map(|(k, v)| (k.clone(), v.0.clone()))
            .collect(),
    );

    // 4. Read each referenced view with the params bound (escurel binds them
    //    as prepared-statement parameters; peacock builds no SQL).
    let mut rows: BTreeMap<String, RowSet> = BTreeMap::new();
    for (alias, query_ref) in &skill.data {
        let rs = escurel
            .query_view(query_ref, &bound, principal, opts.trace.as_ref())
            .await?;
        rows.insert(alias.clone(), rs);
    }

    // 5. Compose the one artifact (FR-R-3).
    let mut artifact = compose(&skill, &absolute, &rows, opts.max_rows)?;

    // 6. Optionally rasterize the first chart to PNG (chat / embedded preview),
    //    themed with the resolved corporate identity ⊕ host look when set.
    if let Some(scale) = opts.png_scale
        && let Some(spec) = artifact.vega_specs.first()
    {
        let png = match &opts.theme {
            Some(theme) => peacock_rasterizer::render_vega_to_png_themed(spec, scale, theme)?,
            None => peacock_rasterizer::render_vega_to_png(spec, scale)?,
        };
        artifact.png = Some(png);
    }

    Ok(artifact)
}

/// Rasterize a single Vega-Lite chart spec to PNG — the `render_a2ui_to_png`
/// capability Triton's chat surface delegates to (FR-V-2, FR-C-2).
pub fn render_a2ui_to_png(spec: &serde_json::Value, scale: f32) -> Result<Vec<u8>> {
    Ok(peacock_rasterizer::render_vega_to_png(spec, scale)?)
}

/// Bind a shared selection into the param vector when the report declares its
/// dimension and the caller did not supply that param (FR-X-6 / OQ-5). Caller
/// params win; a report without the dimension is returned unchanged. Returns an
/// owned `Value` so the caller's input is never mutated (statelessness).
fn apply_selection(
    params: &Value,
    selection: Option<&SharedSelection>,
    schema: &peacock_types::ParamSchema,
) -> Value {
    let Some(sel) = selection else {
        return params.clone();
    };
    // The report must declare the selection's dimension to inherit it.
    if !schema.0.contains_key(&sel.dimension) {
        return params.clone();
    }
    let mut obj = match params.as_object() {
        Some(o) => o.clone(),
        None => return params.clone(), // let validation report the bad shape
    };
    // The caller's absolute param wins; only fill an absent/null dimension.
    let unset = obj.get(&sel.dimension).map(Value::is_null).unwrap_or(true);
    if unset {
        obj.insert(sel.dimension.clone(), sel.value.clone());
    }
    Value::Object(obj)
}

/// Promote a committed drill to a shared, named selection (FR-X-6 / OQ-5): the
/// first param whose current value differs from its declared default is the
/// salient selection ("the thing the user drilled into"). Returns `None` when
/// every param sits at its default (nothing committed to promote). The
/// selection's `name` is its `dimension` — the conversation context may rename
/// it. Reads only the artifact's compact view state; never the rows.
pub fn promotable_selection(artifact: &Artifact) -> Option<SharedSelection> {
    let current = artifact.structured_content.current_params.as_object()?;
    let schema = artifact.structured_content.param_schema.as_object()?;
    for (name, value) in current {
        let default = schema
            .get(name)
            .and_then(|spec| spec.get("default"))
            .unwrap_or(&Value::Null);
        if value != default {
            return Some(SharedSelection::new(
                name.clone(),
                name.clone(),
                value.clone(),
            ));
        }
    }
    None
}

/// Build the compact view-state record pushed to the model on a committed
/// drill (FR-X-3, ACC-12): `{report_id, params, salient_summary}` — **never
/// rows**. When a param has been drilled off its default it also carries the
/// promotable shared `selection` (FR-X-6) other reports can inherit. Returned
/// to the surface shells so the MCP-App / chat paths can emit it via
/// `updateModelContext` / the signed token.
pub fn view_state_record(report_id: &str, artifact: &Artifact, summary: &str) -> Value {
    let mut rec = json!({
        "report_id": report_id,
        "params": artifact.structured_content.current_params,
        "salient_summary": summary,
    });
    if let Some(sel) = promotable_selection(artifact) {
        rec["selection"] = serde_json::to_value(sel).unwrap_or(Value::Null);
    }
    rec
}
