//! The render `Artifact` — peacock's output for one `(report, params)`
//! (BRD §2, FR-R-3). Three coupled outputs from one pass plus an optional
//! rasterization:
//!
//! - `a2ui` — the A2UI v0.9 layout document (KPI/table/text/controls);
//! - `vega_specs` — the Vega-Lite specs the layout's `kind:vega` components
//!   embed (kept separable from layout, FR-V-1);
//! - `structured_content` — typed rows + the parameter schema **and the
//!   current resolved parameter values** (the view state, FR-X-1);
//! - `png` — an on-demand chart rasterization (chat path, FR-V-2).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed payload an agent reads and re-drills by changing params (FR-R-3).
/// `current_params` is what lets a consuming agent know the visualization's
/// state without a separate channel (FR-X-1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredContent {
    pub rows: Value,
    pub param_schema: Value,
    pub current_params: Value,
    /// Instance-page views' typed contract, keyed by the report's instance
    /// alias: `{ alias: { skill, id, page_id, facts, markdown, … } }`.
    /// Populated only from what the views selected (data minimality) and
    /// only for reports that declare `instances:` — absent otherwise, so
    /// row-report artifacts stay byte-identical to before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<Value>,
    /// The `document` pseudo-report's contract: `{ skill, id, actions }` —
    /// the rendered instance's identity plus the affordances its SKILL page
    /// declares (`actions:` frontmatter). Absent on every other report, so
    /// existing artifacts stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<Value>,
}

/// One render's output, shared verbatim across every surface (FR-R-1/2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub a2ui: Value,
    pub vega_specs: Vec<Value>,
    pub structured_content: StructuredContent,
    /// PNG bytes, present only when a surface asked for rasterization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub png: Option<Vec<u8>>,
}
