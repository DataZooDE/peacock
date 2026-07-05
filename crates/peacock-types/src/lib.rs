//! peacock shared types: the render artifact, parameter schema, principal,
//! and the four-variant error model.
//!
//! This crate is dependency-light on purpose (serde only) so every other
//! crate and every surface shell can speak the same vocabulary. See the
//! spec under `doc/` (BRD §2, §5; HLD §8.3).

mod artifact;
mod error;
mod params;
mod principal;
mod selection;
mod stat;

pub use artifact::{Artifact, StructuredContent};
pub use error::{Error, Result};
pub use params::{ParamSchema, ParamSpec, ParamType, ParamValue};
pub use principal::Principal;
pub use selection::SharedSelection;
pub use stat::{StatAnnotation, StatGeom, StatSpec};

/// Crate version, surfaced by `/version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_reported() {
        assert!(!VERSION.is_empty());
    }
}
