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
pub mod skill;

pub use data::{Column, EscurelData, ReportData, RowSet};
pub use render::{RenderOpts, render, render_a2ui_to_png, view_state_record};
pub use skill::{Agg, ReportSkill, ReportSkills, ViewSpec};
