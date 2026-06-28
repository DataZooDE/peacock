//! A registry of host-flavor and brand CSS, resolving `(brand, host)` to a
//! composed [`Theme`]. Built-in host flavors ship with peacock; brand CSS can
//! be registered at runtime (e.g. agent-authored corporate identities).

use std::collections::BTreeMap;

use crate::Theme;

/// Holds host-flavor and brand CSS, keyed by name.
#[derive(Debug, Clone)]
pub struct ThemeRegistry {
    hosts: BTreeMap<String, String>,
    brands: BTreeMap<String, String>,
}

impl ThemeRegistry {
    /// The built-in registry: host flavors (`copilot`, `whatsapp`, `gemini`)
    /// and demo brands (`company-a`, `company-b`, plus the empty `default`).
    pub fn builtin() -> Self {
        let mut hosts = BTreeMap::new();
        hosts.insert(
            "copilot".into(),
            include_str!("../assets/hosts/copilot.css").to_string(),
        );
        hosts.insert(
            "whatsapp".into(),
            include_str!("../assets/hosts/whatsapp.css").to_string(),
        );
        hosts.insert(
            "gemini".into(),
            include_str!("../assets/hosts/gemini.css").to_string(),
        );

        let mut brands = BTreeMap::new();
        brands.insert("default".into(), String::new());
        brands.insert(
            "company-a".into(),
            include_str!("../assets/brands/company-a.css").to_string(),
        );
        brands.insert(
            "company-b".into(),
            include_str!("../assets/brands/company-b.css").to_string(),
        );

        Self { hosts, brands }
    }

    /// Register (or replace) a brand's CSS — the hook an authoring agent uses
    /// to install a generated corporate identity.
    pub fn register_brand(&mut self, name: impl Into<String>, css: impl Into<String>) {
        self.brands.insert(name.into(), css.into());
    }

    /// Register (or replace) a host flavor's CSS.
    pub fn register_host(&mut self, name: impl Into<String>, css: impl Into<String>) {
        self.hosts.insert(name.into(), css.into());
    }

    /// Resolve `(brand, host)` to a composed theme. Unknown names fall back to
    /// empty CSS (i.e. peacock's stock defaults).
    pub fn resolve(&self, brand: &str, host: &str) -> Theme {
        let host_css = self.hosts.get(host).map(String::as_str).unwrap_or("");
        let brand_css = self.brands.get(brand).map(String::as_str).unwrap_or("");
        Theme::from_css(host, brand, host_css, brand_css)
    }

    /// Known host-flavor names.
    pub fn hosts(&self) -> impl Iterator<Item = &str> {
        self.hosts.keys().map(String::as_str)
    }

    /// Known brand names.
    pub fn brands(&self) -> impl Iterator<Item = &str> {
        self.brands.keys().map(String::as_str)
    }
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}
