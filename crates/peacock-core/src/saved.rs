//! Saved / shared report instances (BRD §7) — peacock stays **stateless**.
//!
//! Bookmarking a parameterized render is an *escurel* concern: the saved
//! instance is an ordinary escurel page (`type: instance, skill:
//! report_bookmark`) carrying the report wikilink and the absolute parameter
//! vector. peacock writes it via `escurel-client` `update_page` and reads it
//! back via `resolve` + `expand`, forwarding the caller principal so escurel's
//! fail-closed ACL gates both the write and the read. peacock holds no DB, no
//! credential, and constructs no SQL — it only asks escurel to persist/read a
//! markdown page (FR-R-2, NFR-S-1).
//!
//! A saved render is **byte-reproducible** against a direct
//! `render(report_id, params)` call, because `render_saved` resolves the saved
//! instance to exactly `(report_id, params)` and funnels through the same
//! render core (FR-R-1, FR-X reproducibility).

use escurel_client::{Client, ExpandRequest, ResolveRequest, UpdatePageRequest};
use peacock_types::{Artifact, Error, Principal, Result};
use secrecy::SecretString;
use serde_json::Value;

use crate::data::{EscurelData, map_err};
use crate::render::{RenderOpts, render};

/// The escurel skill a saved render-instance is filed under. Its instances
/// live at `markdown/instances/report_bookmark/<name>.md` and resolve via the
/// `[[report_bookmark::<name>]]` wikilink.
pub const BOOKMARK_SKILL: &str = "report_bookmark";

/// A handle to a persisted saved instance — the name that resolves it back.
/// Opaque to surfaces: pass it to [`render_saved`] /
/// [`resolve_saved_instance`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedRef {
    /// The saved instance's stable name (its escurel instance id).
    pub name: String,
}

impl SavedRef {
    /// The `[[report_bookmark::<name>]]` wikilink escurel resolves it by.
    pub fn wikilink(&self) -> String {
        format!("[[{BOOKMARK_SKILL}::{}]]", self.name)
    }

    /// The escurel page id the instance is persisted at.
    fn page_id(&self) -> String {
        format!("markdown/instances/{BOOKMARK_SKILL}/{}.md", self.name)
    }
}

/// Persist a parameterized render as an escurel saved instance and return the
/// [`SavedRef`] that resolves it back. The instance carries `report:
/// "[[skill::<report_id>]]"` and the absolute `params:` vector; `owner` is
/// stamped with the caller's `sub` so escurel's owner ACL lets the caller
/// (and only the caller) read it back. peacock writes nothing locally — it
/// forwards the page to escurel's `update_page` as the caller (FR-R-2).
pub async fn save_instance(
    escurel: &EscurelData,
    principal: &Principal,
    name: &str,
    report_id: &str,
    params: &Value,
) -> Result<SavedRef> {
    let saved = SavedRef {
        name: name.to_owned(),
    };

    let token = SecretString::from(principal.raw_token.clone());
    let client = Client::connect(escurel.endpoint(), token)
        .await
        .map_err(map_err)?;

    let content = bookmark_markdown(name, report_id, params, &principal.sub)?;
    let resp = client
        .update_page(UpdatePageRequest {
            page_id: saved.page_id(),
            content,
        })
        .await
        .map_err(map_err)?;

    if !resp.ok {
        return Err(Error::data(format!(
            "escurel rejected the saved instance `{name}`: {:?}",
            resp.issues
        )));
    }

    Ok(saved)
}

/// Read a saved instance back to the `(report_id, params)` it bookmarked,
/// via escurel `resolve` + `expand` as the caller (the owner ACL applies).
pub async fn resolve_saved_instance(
    escurel: &EscurelData,
    principal: &Principal,
    saved: &SavedRef,
) -> Result<(String, Value)> {
    let token = SecretString::from(principal.raw_token.clone());
    let client = Client::connect(escurel.endpoint(), token)
        .await
        .map_err(map_err)?;

    let resolved = client
        .resolve(ResolveRequest {
            wikilink: saved.wikilink(),
            scenario: String::new(),
        })
        .await
        .map_err(map_err)?;
    let page = resolved
        .page
        .ok_or_else(|| Error::data(format!("saved instance `{}` not found", saved.name)))?;

    let expanded = client
        .expand(ExpandRequest {
            page_id: page.page_id,
            ..Default::default()
        })
        .await
        .map_err(map_err)?;

    let report_id = expanded
        .frontmatter
        .get("report")
        .and_then(Value::as_str)
        .map(report_ref_of)
        .ok_or_else(|| {
            Error::data(format!(
                "saved instance `{}` carries no `report` link",
                saved.name
            ))
        })?;

    let params = expanded
        .frontmatter
        .get("params")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));

    Ok((report_id, params))
}

/// Render a saved instance: resolve it to `(report_id, params)` then funnel
/// through the one render core — so a saved render reproduces a direct
/// `render(report_id, params)` artifact byte-for-byte (FR-R-1, FR-X).
pub async fn render_saved(
    escurel: &EscurelData,
    principal: &Principal,
    saved: &SavedRef,
    opts: &RenderOpts,
) -> Result<Artifact> {
    let (report_id, params) = resolve_saved_instance(escurel, principal, saved).await?;
    render(&report_id, &params, principal, escurel, opts).await
}

/// The markdown a saved instance is persisted as: `type: instance`, filed
/// under the [`BOOKMARK_SKILL`], carrying the report wikilink, the absolute
/// params vector, and the owner subject.
fn bookmark_markdown(
    name: &str,
    report_id: &str,
    params: &Value,
    owner_sub: &str,
) -> Result<String> {
    // Front matter is YAML; serialize the params object as JSON, which is a
    // valid YAML flow mapping — escurel parses it back into the same value.
    let params_json = serde_json::to_string(params)
        .map_err(|e| Error::render(format!("saved instance `{name}`: bad params: {e}")))?;
    Ok(format!(
        "---\n\
         type: instance\n\
         skill: {BOOKMARK_SKILL}\n\
         id: {name}\n\
         visibility: owner\n\
         owner: {owner_sub}\n\
         report: \"[[skill::{report_id}]]\"\n\
         params: {params_json}\n\
         ---\n\
         # {name}\n\
         Saved render of [[skill::{report_id}]].\n"
    ))
}

/// Extract the report id from a `report:` wikilink: `[[skill::nw]]` → `nw`,
/// `[[nw]]` → `nw`, plain `nw` → `nw` (mirrors `skill::query_ref_of`).
fn report_ref_of(link: &str) -> String {
    let inner = link.trim().trim_start_matches("[[").trim_end_matches("]]");
    match inner.rsplit_once("::") {
        Some((_, id)) => id.to_owned(),
        None => inner.to_owned(),
    }
}
