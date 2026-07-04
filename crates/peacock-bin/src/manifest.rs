//! The peacock boot manifest (FR-L-3, ADR-P11): a lean declaration of the A2UI
//! component catalog, the render policy/guardrail mode, and the escurel
//! binding. Every enumerated value is **closed-checked at boot** — an unknown
//! enum refuses boot with a named error (ACC-10). In production, every secret
//! field MUST be a `vault://` reference (a literal refuses boot).

use serde::Deserialize;

/// The render guardrail policy (closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderPolicy {
    /// inline-data-only + no Vega expressions (the v1 default).
    Strict,
    /// reserved for a documented, widened safe subset.
    Lenient,
}

/// The component-catalog kinds peacock will compose (closed set).
const ALLOWED_COMPONENTS: &[&str] = &[
    "kpi",
    "vega",
    "table",
    "text",
    "controls",
    "markdown",
    "frontmatter",
    "timeline",
];

/// A parsed, validated manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub policy: RenderPolicy,
    pub catalog: Vec<String>,
    /// The escurel binding (endpoint), if declared here rather than via env.
    pub escurel_url: Option<String>,
}

/// The on-disk shape (TOML).
#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    render: RawRender,
    #[serde(default)]
    components: RawComponents,
    #[serde(default)]
    escurel: RawEscurel,
}

#[derive(Debug, Default, Deserialize)]
struct RawRender {
    /// Unknown values are rejected by the enum's serde (closed set).
    policy: Option<RenderPolicy>,
}
#[derive(Debug, Default, Deserialize)]
struct RawComponents {
    #[serde(default)]
    catalog: Vec<String>,
}
#[derive(Debug, Default, Deserialize)]
struct RawEscurel {
    url: Option<String>,
    /// In production this MUST be a `vault://…` ref, not a literal.
    secret: Option<String>,
}

/// Boot-validation failure (named, ACC-10).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("manifest parse error: {0}")]
    Parse(String),
    #[error(
        "manifest: unknown component `{0}` (allowed: kpi, vega, table, text, controls, \
         markdown, frontmatter, timeline)"
    )]
    UnknownComponent(String),
    #[error("manifest: `render.policy` is required")]
    MissingPolicy,
    #[error("manifest: secret `{0}` must be a vault:// reference in production, not a literal")]
    LiteralSecretInProd(String),
}

impl Manifest {
    /// Parse + closed-check a manifest from TOML text. `production` enables the
    /// Vault-ref-only secret rule.
    pub fn parse(text: &str, production: bool) -> Result<Self, ManifestError> {
        let raw: RawManifest =
            toml::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;

        // Unknown render.policy values already fail at the serde layer (the
        // enum is closed); a missing one is a named error.
        let policy = raw.render.policy.ok_or(ManifestError::MissingPolicy)?;

        // Closed-check every catalog component.
        for c in &raw.components.catalog {
            if !ALLOWED_COMPONENTS.contains(&c.as_str()) {
                return Err(ManifestError::UnknownComponent(c.clone()));
            }
        }

        // Production: secrets must be Vault references.
        if production
            && let Some(secret) = &raw.escurel.secret
            && !secret.starts_with("vault://")
        {
            return Err(ManifestError::LiteralSecretInProd(secret.clone()));
        }

        Ok(Manifest {
            policy,
            catalog: raw.components.catalog,
            escurel_url: raw.escurel.url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
        [render]
        policy = "strict"
        [components]
        catalog = ["kpi", "vega", "table", "text"]
        [escurel]
        url = "http://escurel.tailnet:8080"
    "#;

    #[test]
    fn parses_a_valid_manifest() {
        let m = Manifest::parse(GOOD, false).unwrap();
        assert_eq!(m.policy, RenderPolicy::Strict);
        assert!(m.catalog.contains(&"vega".to_string()));
    }

    #[test]
    fn rejects_unknown_component() {
        let bad = r#"
            [render]
            policy = "strict"
            [components]
            catalog = ["kpi", "hologram"]
        "#;
        assert_eq!(
            Manifest::parse(bad, false),
            Err(ManifestError::UnknownComponent("hologram".into()))
        );
    }

    #[test]
    fn rejects_unknown_policy_enum() {
        let bad = r#"[render]
            policy = "yolo"
        "#;
        assert!(matches!(
            Manifest::parse(bad, false),
            Err(ManifestError::Parse(_))
        ));
    }

    #[test]
    fn production_refuses_a_literal_secret() {
        let bad = r#"
            [render]
            policy = "strict"
            [escurel]
            secret = "super-secret-literal"
        "#;
        assert!(matches!(
            Manifest::parse(bad, true),
            Err(ManifestError::LiteralSecretInProd(_))
        ));
        // The same literal is fine in dev.
        assert!(Manifest::parse(bad, false).is_ok());
    }
}
