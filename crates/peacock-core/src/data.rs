//! The data reader: peacock's **only** path to report rows (FR-D-1..6).
//!
//! peacock resolves a report's structured-data-view reference to an escurel
//! `query_instance(ref, params)` call, forwarding the caller principal so
//! escurel's fail-closed ACL applies (FR-D-2). Parameters travel as typed
//! JSON values; escurel binds them as prepared-statement parameters. **peacock
//! constructs no query text and holds no database credential** (FR-D-3,
//! NFR-S-1): the reader speaks only the typed `escurel-client` surface.

use async_trait::async_trait;
use escurel_client::{Client, Error as EscError, QueryInstanceRequest};
use peacock_types::{Error, Principal, Result};
use secrecy::SecretString;
use serde_json::{Value, json};

use crate::TraceSink;

/// One column's name and escurel-reported type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub type_name: String,
}

/// The access-checked, already-aggregated rows escurel released for one view
/// read, plus a `truncated` flag when the server row cap clipped the set.
#[derive(Debug, Clone, PartialEq)]
pub struct RowSet {
    pub rows: Value,
    pub schema: Vec<Column>,
    pub truncated: bool,
}

/// The report data port. The render core depends on this trait, not on
/// escurel directly, so the embedded face can supply its own escurel binding
/// (FR-E-3) and the single render path is preserved (FR-R-1).
#[async_trait]
pub trait ReportData: Send + Sync {
    /// Read a structured data view by its query ref with the given typed
    /// params, as the given principal. When `trace` is `Some`, the **real**
    /// request and response that crossed the escurel wire are recorded into it.
    async fn query_view(
        &self,
        query_ref: &str,
        params: &Value,
        principal: &Principal,
        trace: Option<&TraceSink>,
    ) -> Result<RowSet>;
}

/// The escurel-backed reader. Holds only the escurel endpoint — no token, no
/// credential. The bearer is taken from the per-request principal and
/// forwarded, so rows are never cached across principals (FR-D-2).
#[derive(Debug, Clone)]
pub struct EscurelData {
    endpoint: String,
}

impl EscurelData {
    /// Bind to an escurel endpoint (e.g. `http://escurel.tailnet:8080`).
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    /// The bound escurel endpoint (shared with the skill-resolution port).
    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[async_trait]
impl ReportData for EscurelData {
    async fn query_view(
        &self,
        query_ref: &str,
        params: &Value,
        principal: &Principal,
        trace: Option<&TraceSink>,
    ) -> Result<RowSet> {
        // Forward the caller's bearer per request (no ambient credential).
        let token = SecretString::from(principal.raw_token.clone());
        let client = Client::connect(&self.endpoint, token)
            .await
            .map_err(map_err)?;

        let resp = client
            .query_instance(QueryInstanceRequest {
                query_ref: query_ref.to_owned(),
                params: params.clone(),
            })
            .await
            .map_err(map_err)?;

        let schema: Vec<Column> = resp
            .schema
            .into_iter()
            .map(|c| Column {
                name: c.name,
                type_name: c.type_name,
            })
            .collect();

        // Record the genuine wire payloads (the request peacock sent and the
        // response escurel returned), for the inspector to show verbatim.
        crate::record(
            trace,
            json!({
                "hop": "peacock→escurel",
                "method": "query_instance",
                "request": { "query_ref": query_ref, "params": params },
                "response": {
                    "schema": schema.iter().map(|c| json!({ "name": c.name, "type": c.type_name })).collect::<Vec<_>>(),
                    "row_count": resp.rows.as_array().map(Vec::len).unwrap_or(0),
                    "rows": resp.rows,
                    "truncated": resp.truncated
                }
            }),
        );

        Ok(RowSet {
            rows: resp.rows,
            schema,
            truncated: resp.truncated,
        })
    }
}

/// Map an `escurel-client` error onto peacock's typed model (FR-D-5). An
/// identity rejection is `Auth`; every other escurel-read failure (ACL
/// denial, timeout, drift, decode) is `Data` — never a partial artifact.
/// The variant is decided here, never by a surface inspecting the message.
pub(crate) fn map_err(e: EscError) -> Error {
    match e {
        EscError::InvalidToken => Error::auth("invalid bearer for escurel"),
        EscError::Http { status: 401, .. } => Error::auth("escurel rejected the principal (401)"),
        // -32001 is escurel's admin/identity gate code.
        EscError::JsonRpc { code: -32001, .. } => {
            Error::auth("escurel denied the principal (jsonrpc -32001)")
        }
        other => Error::data(format!("escurel read failed: {other}")),
    }
}
