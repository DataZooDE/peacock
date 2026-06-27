//! `Principal` — the caller identity peacock accepts (forwarded by Triton)
//! and forwards to escurel (FR-I-1, FR-D-2). Wire-identical to Triton's
//! `Principal`: `{sub, scopes, groups, tenant, trace_id}`, with the bearer
//! token never serialized and never shown in `Debug` (NFR-S-4).

use serde::{Deserialize, Serialize};

/// A caller identity. Field-for-field the same shape as
/// `triton_core::principal::Principal`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Principal {
    pub sub: String,
    pub scopes: Vec<String>,
    /// RBAC groups (escurel derives ACL membership from these).
    pub groups: Vec<String>,
    pub tenant: String,
    /// The forwarded bearer. **Never serialized, never logged** — the
    /// lethal-trifecta cut (NFR-S-4). Default empty for the embedded face.
    #[serde(skip)]
    pub raw_token: String,
    pub trace_id: String,
}

impl Principal {
    /// A minimal principal for the embedded library face / tests.
    pub fn new(sub: impl Into<String>, tenant: impl Into<String>) -> Self {
        Self {
            sub: sub.into(),
            scopes: Vec::new(),
            groups: Vec::new(),
            tenant: tenant.into(),
            raw_token: String::new(),
            trace_id: String::new(),
        }
    }
}

/// Hand-rolled so the bearer can never leak through `{:?}` (Triton pattern).
impl std::fmt::Debug for Principal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Principal")
            .field("sub", &self.sub)
            .field("scopes", &self.scopes)
            .field("groups", &self.groups)
            .field("tenant", &self.tenant)
            .field("raw_token", &"<redacted>")
            .field("trace_id", &self.trace_id)
            .finish()
    }
}
