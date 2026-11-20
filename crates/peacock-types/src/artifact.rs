//! The render `Artifact` — peacock's output for one `(report, params)`
//! (BRD §2, FR-R-3). Three coupled outputs from one pass plus an optional
//! rasterization:
//!
//! - `a2ui` — the A2UI v0.9 layout document (KPI/table/text/controls);
//! - `anofox_specs` — the anofox panel specs (rows inline) the layout's
//!   `kind: anofox` components embed (kept separable from layout, FR-V-1);
//! - `vega_specs` — minimal Vega-Lite DERIVED from the chartable panels, a
//!   compatibility output for native-chart consumers (Triton's Teams/GE).
//!   **Deprecated** (triton#375): surfaces now ship the renderer-neutral
//!   chart summary `_meta.chart` (`peacock_core::chart::chart_summary`);
//!   `vega_specs` is emitted for one more release, then removed;
//! - `structured_content` — typed rows + the parameter schema **and the
//!   current resolved parameter values** (the view state, FR-X-1);
//! - `png` / `pngs` — on-demand chart rasterizations (chat path, FR-V-2).

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
    /// Set when a row cap clipped a view on the way here (#46) — escurel's
    /// server cap or peacock's own `max_rows`. The artifact then shows partial
    /// data, and every surface must say so. Absent when nothing was capped,
    /// so untruncated artifacts stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<Truncation>,
}

/// Which view lost rows to a cap, and how many it kept (#46). Wire shape:
/// `{ rows_shown, rows_total?, limit, view }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Truncation {
    /// Rows the artifact carries for that view.
    pub rows_shown: usize,
    /// Rows the view had before the cap, when known: peacock's own cap knows
    /// it; escurel's server cap reports only that it clipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows_total: Option<usize>,
    /// The cap that applied.
    pub limit: usize,
    /// The report's data alias (or a dashboard's query) that was capped. When
    /// several were, the primary view wins, else the first in data order.
    pub view: String,
}

/// One render's output, shared verbatim across every surface (FR-R-1/2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub a2ui: Value,
    /// Derived Vega-Lite (compatibility output) for the chartable panels —
    /// bar / line / area / pie / donut. Other chart kinds have none.
    /// Deprecated in favour of the `_meta.chart` summary (triton#375); kept
    /// for one more release.
    pub vega_specs: Vec<Value>,
    /// The anofox panel specs (`{rows, roles, …}`), one per chart, in layout
    /// order. The painted SVG rides on each `kind: anofox` component.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anofox_specs: Vec<Value>,
    pub structured_content: StructuredContent,
    /// The preview PNG (the first chart, else the instance card), present
    /// only when a surface asked for rasterization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub png: Option<Vec<u8>>,
    /// One PNG per chart, in layout order (rasterized surfaces only). Not
    /// serialized — surfaces encode what they ship.
    #[serde(skip)]
    pub pngs: Vec<Vec<u8>>,
}

impl Artifact {
    /// The painted SVG of every chart — and of a whole compiled dashboard —
    /// in layout order (empty until the render step paints them).
    pub fn chart_svgs(&self) -> Vec<&str> {
        self.a2ui
            .get("components")
            .and_then(Value::as_array)
            .map(|cs| {
                cs.iter()
                    .filter(|c| {
                        matches!(
                            c.get("kind").and_then(Value::as_str),
                            Some("anofox" | "dashboard")
                        )
                    })
                    .filter_map(|c| c.get("svg").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default()
    }
}
