//! The four-variant peacock error model (FR-R-5, HLD §8.3).
//!
//! `Auth | Validation | Data | Render`. Each surface (MCP JSON-RPC, chat,
//! library `Result`) maps **on the variant, never by inspecting the inner
//! message**. The mapping helpers ([`Error::jsonrpc_code`], [`Error::kind`])
//! are the single source of truth so surfaces cannot diverge.

use thiserror::Error as ThisError;

/// A peacock render error. The variant carries a human-readable message for
/// logs/audit; surfaces switch on the variant alone.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum Error {
    /// Identity/authorization failure (forwarded principal rejected, escurel
    /// ACL denial surfaced as auth). MCP JSON-RPC `-32001`.
    #[error("auth: {0}")]
    Auth(String),

    /// Parameters did not validate against the report skill's declared schema
    /// (FR-R-4, FR-D-6 pre-call type check). MCP JSON-RPC `-32602`.
    #[error("validation: {0}")]
    Validation(String),

    /// escurel read failure (timeout, ACL denial as data, schema drift). No
    /// partial artifact is emitted (FR-D-5). MCP JSON-RPC `-32000`.
    #[error("data: {0}")]
    Data(String),

    /// Composition/guardrail/rasterization failure (remote-data spec,
    /// disallowed Vega expr, oversized result set). MCP JSON-RPC `-32000`.
    #[error("render: {0}")]
    Render(String),
}

impl Error {
    /// Construct an [`Error::Auth`].
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }
    /// Construct an [`Error::Validation`].
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }
    /// Construct an [`Error::Data`].
    pub fn data(msg: impl Into<String>) -> Self {
        Self::Data(msg.into())
    }
    /// Construct an [`Error::Render`].
    pub fn render(msg: impl Into<String>) -> Self {
        Self::Render(msg.into())
    }

    /// Stable lowercase tag, decided by the variant. Used in the audit
    /// `result` field and the chat/A2A `metadata.error` projection.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Auth(_) => "auth",
            Error::Validation(_) => "validation",
            Error::Data(_) => "data",
            Error::Render(_) => "render",
        }
    }

    /// MCP JSON-RPC error code, mirroring Triton's mapping
    /// (auth `-32001`, validation `-32602`, server-side `-32000`).
    pub fn jsonrpc_code(&self) -> i32 {
        match self {
            Error::Auth(_) => -32001,
            Error::Validation(_) => -32602,
            Error::Data(_) | Error::Render(_) => -32000,
        }
    }
}

/// Convenience alias for peacock results.
pub type Result<T> = std::result::Result<T, Error>;
