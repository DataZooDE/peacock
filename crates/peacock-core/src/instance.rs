//! The instance-page reader: how a report renders an escurel **record**
//! (a customer account, a briefing) instead of query rows.
//!
//! Same trust shape as the row reader (`data.rs`): the caller principal's
//! bearer is forwarded per request so escurel's fail-closed ACL applies; the
//! read is `resolve` + `expand` of `[[skill::id]]` — the `saved.rs`
//! bookmark-read precedent, generalized. No SQL, no credential.

use async_trait::async_trait;
use escurel_client::{Client, ExpandRequest, ResolveRequest};
use peacock_types::{Error, Principal, Result};
use secrecy::SecretString;
use serde_json::{Value, json};

use crate::TraceSink;
use crate::data::map_err;

/// One resolved instance page: identity + the frontmatter/body the views
/// select from.
#[derive(Debug, Clone, PartialEq)]
pub struct InstancePage {
    pub page_id: String,
    pub skill: String,
    pub id: String,
    pub frontmatter: Value,
    pub body: String,
}

/// The instance-page port. The render core depends on this trait, not on
/// escurel directly, so the embedded face can supply its own binding —
/// exactly the `ReportData` seam (FR-R-1, FR-E-3).
#[async_trait]
pub trait InstanceData: Send + Sync {
    /// Read one instance page (`[[skill::id]]`) as the given principal.
    async fn read_instance(
        &self,
        skill: &str,
        id: &str,
        principal: &Principal,
        trace: Option<&TraceSink>,
    ) -> Result<InstancePage>;
}

#[async_trait]
impl InstanceData for crate::data::EscurelData {
    async fn read_instance(
        &self,
        skill: &str,
        id: &str,
        principal: &Principal,
        trace: Option<&TraceSink>,
    ) -> Result<InstancePage> {
        // Forward the caller's bearer per request (no ambient credential).
        let token = SecretString::from(principal.raw_token.clone());
        let client = Client::connect(self.endpoint(), token)
            .await
            .map_err(map_err)?;

        let wikilink = format!("[[{skill}::{id}]]");
        let resolved = client
            .resolve(ResolveRequest {
                wikilink: wikilink.clone(),
                scenario: String::new(),
            })
            .await
            .map_err(map_err)?;
        let page = resolved
            .page
            .ok_or_else(|| Error::data(format!("instance `{skill}::{id}` not found")))?;

        let expanded = client
            .expand(ExpandRequest {
                page_id: page.page_id.clone(),
                ..Default::default()
            })
            .await
            .map_err(map_err)?;

        // Record the genuine wire exchange for the inspector, like every
        // other escurel hop.
        crate::record(
            trace,
            json!({
                "hop": "peacock→escurel",
                "method": "resolve + expand (instance)",
                "request": { "wikilink": wikilink, "scenario": "" },
                "response": {
                    "page_id": page.page_id,
                    "frontmatter": expanded.frontmatter,
                    "body_len": expanded.body.len(),
                }
            }),
        );

        Ok(InstancePage {
            page_id: page.page_id,
            skill: skill.to_owned(),
            id: id.to_owned(),
            frontmatter: expanded.frontmatter,
            body: expanded.body,
        })
    }
}
