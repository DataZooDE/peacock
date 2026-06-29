//! Report parameter schema + typed values (FR-R-4, FR-D-6, FR-X-1).
//!
//! A report skill declares a `params` block (`{name: {type, default}}`).
//! peacock validates an incoming param vector against it **before** any
//! escurel read (defense in depth with escurel's binding), rejecting a value
//! whose JSON type does not match the declared scalar type. Validated values
//! travel to escurel as typed `ParamValue`s — peacock builds no SQL.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Error;

/// The scalar types a report parameter may declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    Date,
    String,
    Number,
    Bool,
}

/// One declared parameter: its type and optional default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    #[serde(rename = "type")]
    pub ty: ParamType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

impl ParamSpec {
    pub fn new(ty: ParamType) -> Self {
        Self { ty, default: None }
    }
    pub fn with_default(mut self, default: Value) -> Self {
        self.default = Some(default);
        self
    }
}

/// A typed parameter value handed to the escurel reader. Newtype over
/// `serde_json::Value` so the data path is explicitly "typed values, no SQL".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamValue(pub Value);

impl From<Value> for ParamValue {
    fn from(v: Value) -> Self {
        ParamValue(v)
    }
}

/// The report's declared parameter schema (ordered by name for determinism).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct ParamSchema(pub BTreeMap<String, ParamSpec>);

impl ParamSchema {
    /// Build from `(name, spec)` pairs.
    pub fn from_specs<I, S>(specs: I) -> Self
    where
        I: IntoIterator<Item = (S, ParamSpec)>,
        S: Into<String>,
    {
        ParamSchema(specs.into_iter().map(|(n, s)| (n.into(), s)).collect())
    }

    fn type_matches(ty: ParamType, v: &Value) -> bool {
        match ty {
            // Dates arrive as ISO strings on the JSON wire.
            ParamType::Date | ParamType::String => v.is_string(),
            ParamType::Number => v.is_number(),
            ParamType::Bool => v.is_boolean(),
        }
    }

    /// Validate a param vector against the schema: every supplied value must
    /// match its declared type; unknown names are rejected. Does **not** fill
    /// defaults (use [`ParamSchema::validate_and_default`]).
    pub fn validate(&self, params: &Value) -> Result<(), Error> {
        let obj = params
            .as_object()
            .ok_or_else(|| Error::validation("params must be a JSON object"))?;
        for (name, val) in obj {
            let spec = self
                .0
                .get(name)
                .ok_or_else(|| Error::validation(format!("unknown parameter `{name}`")))?;
            if !val.is_null() && !Self::type_matches(spec.ty, val) {
                return Err(Error::validation(format!(
                    "parameter `{name}` expects {:?}, got {val}",
                    spec.ty
                )));
            }
        }
        Ok(())
    }

    /// Validate, then return the resolved (absolute) param vector with
    /// declared defaults filled for any name the caller omitted. A required
    /// param (no default, not supplied) is a `Validation` error.
    pub fn validate_and_default(
        &self,
        params: &Value,
    ) -> Result<BTreeMap<String, ParamValue>, Error> {
        self.validate(params)?;
        let obj = params.as_object().expect("validated above");
        let mut out = BTreeMap::new();
        for (name, spec) in &self.0 {
            let val = match obj.get(name) {
                Some(v) if !v.is_null() => v.clone(),
                _ => spec.default.clone().ok_or_else(|| {
                    Error::validation(format!("missing required parameter `{name}`"))
                })?,
            };
            out.insert(name.clone(), ParamValue(val));
        }
        Ok(out)
    }
}
