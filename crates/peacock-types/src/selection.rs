//! Shared exploration selection (FR-X-6 / OQ-5).
//!
//! A **committed** drill on one report can be promoted to a shared, named
//! selection the conversation context holds and that *other* reports inherit
//! ("now show me X for *this*"). It is a tiny projection of view state — a
//! `(dimension, value)` pair plus a human-facing name — never rows. peacock
//! stays stateless: the selection is an **input** to a render (carried in
//! `RenderOpts`), never persisted server-side. A report inherits it only when
//! its declared param schema names the selection's `dimension`; otherwise the
//! selection is silently ignored. Absolute params the caller supplies always
//! win (HLD §state-sync: drills carry the absolute vector, so the projections
//! cannot drift).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A committed drill promoted to a reusable, named selection that other
/// visualizations inherit (e.g. `dimension = "category"`, `value =
/// "Beverages"`). The recommended default for multi-visualization follow-ups
/// (BRD §5.6 FR-X-6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedSelection {
    /// A human-facing name for the selection in the conversation context.
    pub name: String,
    /// The dimension (report-param name) the selection binds.
    pub dimension: String,
    /// The selected value, typed as it travels to escurel.
    pub value: Value,
}

impl SharedSelection {
    /// Build a selection over `dimension = value`, named `name`.
    pub fn new(name: impl Into<String>, dimension: impl Into<String>, value: Value) -> Self {
        Self {
            name: name.into(),
            dimension: dimension.into(),
            value,
        }
    }
}
