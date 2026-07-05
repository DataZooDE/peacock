//! The statistical report-spec DIALECT (issue #7): the typed, declarative
//! surface a report-skill author writes under `specs:` for a statistical
//! chart — no Rust, no chart code in the customer's hands.
//!
//! A named spec is STATISTICAL when its JSON carries a top-level `geom` key
//! (the Vega-Lite subset uses `mark`, never `geom`). The grammar:
//!
//! ```yaml
//! specs:
//!   leadtime_density:
//!     geom: density            # histogram | density | boxplot | ecdf
//!     x: lead_days             # required — a RowSet column
//!     y: lead_days             # boxplot only: the value axis (x is the group)
//!     color: supplier          # optional series column
//!     facet_wrap: supplier     # optional small-multiples column
//!     bins: 30                 # histogram only
//!     title: "Lead times"      # optional
//!     annotations:
//!       - { kind: vline, at: 14.0, label: contract }   # line at a value
//!       - { kind: p90, label: p90 }                    # 90th percentile of the data
//! ```
//!
//! Parsing is strict (`deny_unknown_fields`, typed annotation payloads) so an
//! author's typo is a clear error at compose / `peacock author validate`
//! time, never a silently dropped field. Geometry-conditional shape rules
//! ([`StatSpec::parse`]) and the RowSet column cross-check
//! ([`StatSpec::check_columns`]) live here so the composer's guardrail, the
//! author tooling and the ggplot render backend all enforce the *same*
//! dialect — one parser, no drift.

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};

/// The statistical geoms the dialect declares (issue #7; extend later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatGeom {
    Histogram,
    Density,
    Boxplot,
    Ecdf,
}

impl StatGeom {
    /// Every declared geom, in dialect spelling — the allow-list error
    /// messages cite.
    pub const ALL: &'static [&'static str] = &["histogram", "density", "boxplot", "ecdf"];

    /// The dialect spelling (`geom: <this>`).
    pub fn as_str(self) -> &'static str {
        match self {
            StatGeom::Histogram => "histogram",
            StatGeom::Density => "density",
            StatGeom::Boxplot => "boxplot",
            StatGeom::Ecdf => "ecdf",
        }
    }
}

/// An annotation layer over a statistical chart — the marks the reliability
/// chart needs (issue #7): a reference line at a contracted value, and the
/// 90th-percentile marker computed from the data.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum StatAnnotation {
    /// A reference line at a fixed value on the value axis (e.g. the
    /// contracted lead time).
    Vline {
        at: f64,
        #[serde(default)]
        label: Option<String>,
    },
    /// The 90th-percentile marker: peacock computes the p90 quantile of the
    /// value column and draws a reference line there.
    P90 {
        #[serde(default)]
        label: Option<String>,
    },
}

/// A parsed statistical chart spec — the typed form of the dialect above.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatSpec {
    pub geom: StatGeom,
    /// The x aesthetic: the value column for `histogram`/`density`/`ecdf`,
    /// the grouping category for `boxplot`. Always a RowSet column.
    pub x: String,
    /// The value column for `boxplot` (required there, meaningless — and
    /// rejected — elsewhere).
    #[serde(default)]
    pub y: Option<String>,
    /// Optional series column (one colored series per distinct value).
    #[serde(default)]
    pub color: Option<String>,
    /// Optional small-multiples column (`facet_wrap` by this field).
    #[serde(default)]
    pub facet_wrap: Option<String>,
    /// Histogram bin count (histogram only).
    #[serde(default)]
    pub bins: Option<u32>,
    /// Optional chart title.
    #[serde(default)]
    pub title: Option<String>,
    /// Annotation layers (empty ⇒ none).
    #[serde(default)]
    pub annotations: Vec<StatAnnotation>,
    /// The composer-injected inline rows (`data.values`). Never authored —
    /// tolerated here so the *composed* spec round-trips through the same
    /// parser; escape hatches inside it are still caught by the guardrail
    /// walk.
    #[serde(default)]
    pub data: Option<Value>,
}

impl StatSpec {
    /// Parse a statistical spec from its JSON form, enforcing the dialect:
    /// known geom, known fields only, typed annotations, and the
    /// geometry-conditional shape rules (`y` required for `boxplot` and
    /// rejected elsewhere; `bins` only on `histogram`). Column existence is
    /// [`StatSpec::check_columns`] — it needs the RowSet.
    pub fn parse(spec: &Value) -> Result<Self> {
        let Value::Object(map) = spec else {
            return Err(Error::render(
                "statistical spec must be a JSON object".to_owned(),
            ));
        };

        // A bespoke geom check first: serde's unknown-variant message would
        // not name the `geom` field, and this error is the author's most
        // common one.
        let geom = map.get("geom").and_then(Value::as_str).unwrap_or_default();
        if !StatGeom::ALL.contains(&geom) {
            return Err(Error::render(format!(
                "statistical spec names unknown geom `{geom}` (one of {:?})",
                StatGeom::ALL
            )));
        }

        let parsed: StatSpec = serde_json::from_value(spec.clone())
            .map_err(|e| Error::render(format!("statistical spec: {e}")))?;

        // Geometry-conditional shape rules.
        match parsed.geom {
            StatGeom::Boxplot => {
                if parsed.y.is_none() {
                    return Err(Error::render(
                        "statistical spec: geom `boxplot` requires a `y` value column \
                         (`x` is the grouping category)"
                            .to_owned(),
                    ));
                }
            }
            _ => {
                if let Some(y) = &parsed.y {
                    return Err(Error::render(format!(
                        "statistical spec: `y` (`{y}`) is not meaningful for geom `{}` \
                         (the y axis is computed); only `boxplot` takes a `y`",
                        parsed.geom.as_str()
                    )));
                }
            }
        }
        if parsed.bins.is_some() && parsed.geom != StatGeom::Histogram {
            return Err(Error::render(format!(
                "statistical spec: `bins` is only meaningful for geom `histogram`, not `{}`",
                parsed.geom.as_str()
            )));
        }

        Ok(parsed)
    }

    /// Cross-check every aesthetic against the RowSet's column names — the
    /// same contract the pre-#7 `x` check enforced, extended to the whole
    /// aes mapping.
    pub fn check_columns(&self, columns: &[&str]) -> Result<()> {
        let check = |aes: &str, col: &str| -> Result<()> {
            if columns.contains(&col) {
                Ok(())
            } else {
                Err(Error::render(format!(
                    "statistical spec's {aes} `{col}` is not a column of the view's rows \
                     ({columns:?})"
                )))
            }
        };
        check("x", &self.x)?;
        if let Some(y) = &self.y {
            check("y", y)?;
        }
        if let Some(color) = &self.color {
            check("color", color)?;
        }
        if let Some(facet) = &self.facet_wrap {
            check("facet_wrap", facet)?;
        }
        Ok(())
    }
}
