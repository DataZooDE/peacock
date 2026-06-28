//! peacock render core: the single stateless pivot
//! `(report skill, params, rows) → artifact` (FR-R-1).
//!
//! - `data` — the escurel row reader (FR-D);
//! - `skill` — the report-skill model + escurel resolution (FR-D-1);
//! - `guardrail` — the safe Vega-Lite subset (FR-V-4);
//! - `compose` — A2UI v0.9 + kind:vega + structuredContent (FR-R-3, FR-V);
//! - `render` — the orchestration every surface funnels through.

pub mod compose;
pub mod data;
pub mod guardrail;
pub mod render;
pub mod saved;
pub mod skill;

pub use data::{Column, EscurelData, ReportData, RowSet};
pub use render::{RenderOpts, render, render_a2ui_to_png, view_state_record};
pub use saved::{BOOKMARK_SKILL, SavedRef, render_saved, resolve_saved_instance, save_instance};
pub use skill::{Agg, ReportSkill, ReportSkills, ViewSpec};

/// A sink that captures the **real** escurel-client wire payloads a render
/// issues (resolve + query_instance, request and response), so a surface can
/// show exactly what crossed the wire — not a reconstruction. Optional; when
/// `None` the data path records nothing. Each entry is a self-describing JSON
/// event (`{ "hop", "request", "response", … }`).
pub type TraceSink = std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>;

/// Push one wire event into a sink, ignoring a poisoned lock (a trace is
/// best-effort observability, never load-bearing).
pub(crate) fn record(sink: Option<&TraceSink>, event: serde_json::Value) {
    if let Some(s) = sink
        && let Ok(mut v) = s.lock()
    {
        v.push(event);
    }
}
