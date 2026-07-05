//! The render core: the single stateless pivot every surface funnels through
//! (FR-R-1). Pure with respect to `(report skill, params, rows)` (FR-R-2):
//! resolve the report skill, validate params, read each view's rows from
//! escurel with the params bound, and compose the artifact.

use std::collections::BTreeMap;

use peacock_types::{Artifact, Principal, Result, SharedSelection};
use serde_json::{Value, json};

use crate::compose::{DEFAULT_MAX_ROWS, compose};
use crate::data::{ReportData, RowSet};
use crate::instance::{InstanceData, InstancePage};
use crate::skill::ReportSkills;

/// Knobs for one render.
#[derive(Debug, Clone)]
pub struct RenderOpts {
    /// Per-view row cap (NFR-P-3).
    pub max_rows: usize,
    /// Rasterize the first chart to PNG and attach it to the artifact
    /// (the chat surface / embedded preview path, FR-C-2/FR-E-2). `None`
    /// skips rasterization; `Some(scale)` renders at `scale` ≥ 1.0.
    pub png_scale: Option<f32>,
    /// Optional theme applied to the rasterized chart (corporate identity ⊕
    /// host look). `None` renders peacock's stock palette. The matching CSS for
    /// the web surfaces is attached at the service boundary, not here.
    pub theme: Option<peacock_rasterizer::ThemeTokens>,
    /// Optional sink capturing the **real** escurel wire payloads this render
    /// issues (resolve + each query_instance, request and response). Used by the
    /// demo's "under the hood" inspector to show genuine — not reconstructed —
    /// traffic. `None` records nothing.
    pub trace: Option<crate::TraceSink>,
    /// Optional shared exploration selection the conversation context holds
    /// (FR-X-6 / OQ-5). When set and the report's param schema declares a param
    /// named after the selection's `dimension`, the report **inherits** that
    /// value ("now show me this in the other chart"). A report that does not
    /// declare the dimension silently ignores it. An absolute param the caller
    /// supplies always wins over the selection (the projections cannot drift,
    /// HLD §state-sync). peacock holds none of this — it is an input only.
    pub selection: Option<SharedSelection>,
    /// Opt-in **Mosaic mode** threshold (BRD §7 deferred big-data cross-filter).
    /// `None` (default) keeps the inline-data + re-render model for every view.
    /// When `Some(n)` and a chart view's row count exceeds `n`, that view is
    /// emitted as a Mosaic-mode artifact: a vgplot/Mosaic spec plus an
    /// escurel-owned data-**source** reference (`query_ref` + bound params), so
    /// the Mosaic client streams from escurel rather than peacock inlining the
    /// oversized rows. This is below `max_rows` (the hard render cap): a view
    /// over `max_rows` and without a mosaic threshold still errors (NFR-P-3).
    pub mosaic_threshold: Option<usize>,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            max_rows: DEFAULT_MAX_ROWS,
            png_scale: None,
            theme: None,
            trace: None,
            selection: None,
            mosaic_threshold: None,
        }
    }
}

/// The reserved pseudo-report id: `render("document", {skill, id})` renders
/// ONE escurel instance page as a document — the chat-reply "Sources"
/// target. It shadows any authored report skill of the same name.
pub const DOCUMENT_REPORT_ID: &str = "document";

/// Render `(report_id, params)` for `principal` against an escurel binding.
/// `escurel` supplies both the report-skill resolution and the row reads, so
/// the embedded face and the service share this exact path (FR-R-1, FR-E-1).
pub async fn render<E>(
    report_id: &str,
    params: &Value,
    principal: &Principal,
    escurel: &E,
    opts: &RenderOpts,
) -> Result<Artifact>
where
    E: ReportData + ReportSkills + InstanceData,
{
    // 0. The reserved `document` pseudo-report intercepts BEFORE any
    //    authored-skill resolution (an imposter page never shadows it).
    if report_id == DOCUMENT_REPORT_ID {
        return render_document(params, principal, escurel, opts).await;
    }

    // 1. Resolve the report skill (escurel resolve/expand).
    let skill = escurel
        .resolve_report(report_id, principal, opts.trace.as_ref())
        .await?;

    // 2. Inherit the shared exploration selection when the report declares the
    //    selection's dimension and the caller left it unset (FR-X-6 / OQ-5).
    //    A report without that param ignores the selection; a caller-supplied
    //    absolute param always wins (no drift, HLD §state-sync).
    let params = apply_selection(params, opts.selection.as_ref(), &skill.params);

    // 3. Validate params against the declared schema and fill defaults — the
    //    absolute parameter vector (FR-R-4, FR-X-2). Type mismatch ⇒ Validation.
    let absolute = skill.params.validate_and_default(&params)?;
    let bound: Value = Value::Object(
        absolute
            .iter()
            .map(|(k, v)| (k.clone(), v.0.clone()))
            .collect(),
    );

    // 4. Read each referenced view with the params bound (escurel binds them
    //    as prepared-statement parameters; peacock builds no SQL).
    let mut rows: BTreeMap<String, RowSet> = BTreeMap::new();
    for (alias, query_ref) in &skill.data {
        let rs = escurel
            .query_view(query_ref, &bound, principal, opts.trace.as_ref())
            .await?;
        rows.insert(alias.clone(), rs);
    }

    // 4b. Resolve each instance-page alias (`instances:`) — the record the
    //     report renders (a customer account, a briefing). The `{param}`
    //     placeholders substitute from the absolute vector and the resulting
    //     id is slug-validated BEFORE any escurel read.
    let mut pages: BTreeMap<String, InstancePage> = BTreeMap::new();
    for (alias, iref) in &skill.instances {
        let id = iref.resolve_id(&absolute)?;
        let mut page = escurel
            .read_instance(&iref.skill, &id, principal, opts.trace.as_ref())
            .await?;
        // Fetch the event history only when a timeline view shows it (the
        // largest limit wins if several timelines share the alias).
        let timeline_limit = skill
            .views
            .iter()
            .filter_map(|v| match v {
                crate::skill::ViewSpec::Timeline { instance, limit } if instance == alias => {
                    Some(*limit)
                }
                _ => None,
            })
            .max();
        if let Some(limit) = timeline_limit {
            page.events = escurel
                .instance_events(&page.page_id, limit, principal, opts.trace.as_ref())
                .await?;
        }
        pages.insert(alias.clone(), page);
    }

    // 5. Compose the one artifact (FR-R-3). `bound` (the absolute param vector
    //    as JSON) travels into compose so a Mosaic-mode view can carry the
    //    escurel-owned data-source reference (`query_ref` + bound params).
    let mut artifact = compose(
        &skill,
        &absolute,
        &bound,
        &rows,
        &pages,
        opts.max_rows,
        opts.mosaic_threshold,
    )?;

    // 6. Optionally rasterize to PNG (chat / embedded preview), themed with
    //    the resolved corporate identity ⊕ host look when set: the first
    //    chart when the report has one, else the INSTANCE CARD (title +
    //    facts + body + activity) for a chartless instance report — every
    //    surface Triton fronts gets a usable image.
    attach_png(&mut artifact, &skill, &pages, opts)?;

    Ok(artifact)
}

/// Rasterize the artifact's preview PNG in place: the first chart when the
/// report has one, else the instance card for a chartless instance report.
/// No-op when the surface didn't ask for rasterization.
fn attach_png(
    artifact: &mut Artifact,
    skill: &crate::skill::ReportSkill,
    pages: &BTreeMap<String, InstancePage>,
    opts: &RenderOpts,
) -> Result<()> {
    let Some(scale) = opts.png_scale else {
        return Ok(());
    };
    if let Some(spec) = artifact.vega_specs.first() {
        let png = match &opts.theme {
            Some(theme) => peacock_rasterizer::render_vega_to_png_themed(spec, scale, theme)?,
            None => peacock_rasterizer::render_vega_to_png(spec, scale)?,
        };
        artifact.png = Some(png);
    } else if let Some(spec) = artifact.stat_specs.first() {
        // The backend selector (issue #6): a STATISTICAL spec (top-level
        // `geom`) rasterizes through the pluggable ggplot backend. Only this
        // PNG step is feature-gated — the spec itself composed above either
        // way — and with the feature off it is a CLEAR error, never a skip.
        artifact.png = Some(render_stat_png(spec, opts, scale)?);
    } else if let Some(req) = instance_card_request(skill, pages) {
        let png = match &opts.theme {
            Some(theme) => {
                peacock_rasterizer::render_instance_card_to_png_themed(&req, scale, theme)?
            }
            None => peacock_rasterizer::render_instance_card_to_png(&req, scale)?,
        };
        artifact.png = Some(png);
    }
    Ok(())
}

/// Rasterize one STATISTICAL spec through the ggplot backend. The rows AND
/// the escurel column schema come back out of the spec's injected inline
/// `data` — exactly what the web surfaces see, so PNG/iframe parity holds by
/// construction, and the backend types every column from escurel's reported
/// type names (issue #8) instead of sniffing the JSON.
#[cfg(feature = "ggplot")]
fn render_stat_png(spec: &Value, opts: &RenderOpts, scale: f32) -> Result<Vec<u8>> {
    let data = spec.get("data");
    let rows = data
        .and_then(|d| d.get("values"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let schema: Vec<peacock_ggplot::ColumnSchema> = data
        .and_then(|d| d.get("schema"))
        .and_then(Value::as_array)
        .map(|cols| {
            cols.iter()
                .filter_map(|c| {
                    Some(peacock_ggplot::ColumnSchema {
                        name: c.get("name")?.as_str()?.to_owned(),
                        type_name: c.get("type")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(peacock_ggplot::render_stat_to_png(
        spec,
        &rows,
        &schema,
        opts.theme.as_ref(),
        scale,
    )?)
}

/// The feature-off selector arm: a statistical spec was authored but this
/// build cannot rasterize it — a clear error, never a silent skip. Only
/// reachable when a report actually carries a `geom:` spec, so the base
/// Vega-Lite path is untouched (issue #6 acceptance).
#[cfg(not(feature = "ggplot"))]
fn render_stat_png(_spec: &Value, _opts: &RenderOpts, _scale: f32) -> Result<Vec<u8>> {
    Err(peacock_types::Error::render(
        "statistical spec requires the `ggplot` feature (rebuild with `--features ggplot`)"
            .to_owned(),
    ))
}

/// The `document` pseudo-report: render one instance page (`{skill, id}`)
/// as a document. The target's SKILL page is the contract — its optional
/// `viewer:` delegates to a richer authored report; its `actions:` list
/// becomes the document's affordances (`structuredContent.document`).
async fn render_document<E>(
    params: &Value,
    principal: &Principal,
    escurel: &E,
    opts: &RenderOpts,
) -> Result<Artifact>
where
    E: ReportData + ReportSkills + InstanceData,
{
    use crate::skill::{ViewSpec, is_slug};

    // Caller input: both params are required slugs — path / wikilink /
    // namespace smuggling is rejected before ANY escurel read.
    let doc_skill = params
        .get("skill")
        .and_then(Value::as_str)
        .filter(|s| is_slug(s))
        .ok_or_else(|| {
            peacock_types::Error::validation("document: `skill` must be a slug param")
        })?;
    let doc_id = params
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| is_slug(s))
        .ok_or_else(|| peacock_types::Error::validation("document: `id` must be a slug param"))?;

    // The target's SKILL page: viewer + actions. A plain skill page parses
    // to an empty ReportSkill carrying just those declarations.
    let skill_page = escurel
        .resolve_report(doc_skill, principal, opts.trace.as_ref())
        .await?;

    // Read the instance ONCE up front — existence + ACL gate every path,
    // and the action templates substitute against its frontmatter.
    let mut page = escurel
        .read_instance(doc_skill, doc_id, principal, opts.trace.as_ref())
        .await?;
    let actions = resolve_actions(&skill_page.actions, &page)?;
    let document = json!({ "skill": doc_skill, "id": doc_id, "actions": actions });

    // Delegation: the skill page names the report that best renders one of
    // its instances. The pseudo-report id itself is rejected at parse time,
    // so this recursion is one level deep by construction.
    if let Some(viewer) = &skill_page.viewer {
        let viewer_params = json!({ viewer.param.clone(): doc_id });
        let mut artifact = Box::pin(render(
            &viewer.report,
            &viewer_params,
            principal,
            escurel,
            opts,
        ))
        .await?;
        artifact.structured_content.document = Some(document);
        return Ok(artifact);
    }

    // Generic fallback: facts (the page's own frontmatter, structural keys
    // excluded) + markdown body + event timeline.
    let fact_keys: Vec<String> = page
        .frontmatter
        .as_object()
        .map(|o| {
            o.keys()
                .filter(|k| !matches!(k.as_str(), "type" | "skill" | "id"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut views = Vec::new();
    if !fact_keys.is_empty() {
        views.push(ViewSpec::Frontmatter {
            instance: "doc".to_owned(),
            keys: fact_keys,
            label: doc_skill.to_owned(),
        });
    }
    views.push(ViewSpec::Markdown {
        instance: "doc".to_owned(),
    });
    views.push(ViewSpec::Timeline {
        instance: "doc".to_owned(),
        limit: 20,
    });
    page.events = escurel
        .instance_events(&page.page_id, 20, principal, opts.trace.as_ref())
        .await?;

    let mut instances = BTreeMap::new();
    instances.insert(
        "doc".to_owned(),
        crate::skill::InstanceRef {
            skill: doc_skill.to_owned(),
            id_template: doc_id.to_owned(),
        },
    );
    let synthesized = crate::skill::ReportSkill {
        id: DOCUMENT_REPORT_ID.to_owned(),
        params: doc_param_schema(),
        data: BTreeMap::new(),
        instances,
        views,
        specs: BTreeMap::new(),
        narrative: String::new(),
        viewer: None,
        actions: Vec::new(),
    };

    let absolute: BTreeMap<String, peacock_types::ParamValue> = [
        ("skill".to_owned(), Value::from(doc_skill).into()),
        ("id".to_owned(), Value::from(doc_id).into()),
    ]
    .into();
    let bound = json!({ "skill": doc_skill, "id": doc_id });
    let mut pages = BTreeMap::new();
    pages.insert("doc".to_owned(), page);

    let mut artifact = compose(
        &synthesized,
        &absolute,
        &bound,
        &BTreeMap::new(),
        &pages,
        opts.max_rows,
        opts.mosaic_threshold,
    )?;
    attach_png(&mut artifact, &synthesized, &pages, opts)?;
    artifact.structured_content.document = Some(document);
    Ok(artifact)
}

/// Execute a document's `event` action: validate the caller-named action
/// against the target's SKILL page (server-side — the client only ever sends
/// the action `name` back), substitute the event templates against the
/// instance, and capture the event as the CALLER (forwarded bearer, escurel
/// ACL gates the write). Returns the minted event id. A `prompt` action or
/// an undeclared name is a validation error — nothing is captured.
pub async fn emit_document_event<E>(
    doc_skill: &str,
    doc_id: &str,
    action_name: &str,
    principal: &Principal,
    escurel: &E,
    trace: Option<&crate::TraceSink>,
) -> Result<String>
where
    E: ReportSkills + InstanceData,
{
    use crate::skill::{ActionKind, is_slug};

    for (label, v) in [
        ("skill", doc_skill),
        ("id", doc_id),
        ("action", action_name),
    ] {
        if !is_slug(v) {
            return Err(peacock_types::Error::validation(format!(
                "emit_document_event: `{label}` must be a slug"
            )));
        }
    }

    let skill_page = escurel.resolve_report(doc_skill, principal, trace).await?;
    let spec = skill_page
        .actions
        .iter()
        .find(|a| a.name == action_name && a.kind == ActionKind::Event)
        .ok_or_else(|| {
            peacock_types::Error::validation(format!(
                "`{action_name}` is not an event action the `{doc_skill}` skill page declares"
            ))
        })?;
    let event = spec
        .event
        .as_ref()
        .expect("event actions carry an EventSpec by construction");

    // Existence + ACL gate: the instance is read as the caller before any
    // write; the templates substitute against what the caller may see.
    let page = escurel
        .read_instance(doc_skill, doc_id, principal, trace)
        .await?;
    let title = substitute_template(&event.title, &page)?;
    let body = substitute_template(&event.body, &page)?;

    escurel
        .capture_document_event(
            &event.label_skill,
            &page.page_id,
            &title,
            &body,
            principal,
            trace,
        )
        .await
}

/// The pseudo-report's param schema: two required strings.
fn doc_param_schema() -> peacock_types::ParamSchema {
    serde_json::from_value(json!({
        "skill": { "type": "string" },
        "id": { "type": "string" }
    }))
    .expect("static schema parses")
}

/// Substitute the skill page's action templates against the target
/// instance: `{id}` and `{frontmatter.<key>}` (string values only — a
/// missing or non-string key is an AUTHOR error naming the skill page).
/// Event actions ship only `{name, kind, label}` — their captured
/// title/body stay server-side.
fn resolve_actions(specs: &[crate::skill::ActionSpec], page: &InstancePage) -> Result<Vec<Value>> {
    use crate::skill::ActionKind;
    specs
        .iter()
        .map(|a| {
            Ok(match a.kind {
                ActionKind::Prompt => {
                    let prompt =
                        substitute_template(a.prompt.as_deref().unwrap_or_default(), page)?;
                    json!({ "name": a.name, "kind": "prompt", "label": a.label, "prompt": prompt })
                }
                ActionKind::Event => {
                    json!({ "name": a.name, "kind": "event", "label": a.label })
                }
            })
        })
        .collect()
}

/// `{id}` → the instance id; `{frontmatter.<key>}` → the page's string
/// frontmatter value. Anything else brace-shaped is an author error.
pub(crate) fn substitute_template(template: &str, page: &InstancePage) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            return Err(peacock_types::Error::render(format!(
                "action template `{template}`: unclosed placeholder"
            )));
        };
        let name = &rest[open + 1..open + close];
        if name == "id" {
            out.push_str(&page.id);
        } else if let Some(key) = name.strip_prefix("frontmatter.") {
            let value = page
                .frontmatter
                .get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    peacock_types::Error::render(format!(
                        "action template names `frontmatter.{key}` — not a string \
                         value on `{}::{}` (fix the `{}` skill page)",
                        page.skill, page.id, page.skill
                    ))
                })?;
            out.push_str(value);
        } else {
            return Err(peacock_types::Error::render(format!(
                "action template `{template}`: unknown placeholder `{{{name}}}`"
            )));
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Build the chat card from the report's FIRST instance alias (deterministic
/// via the ordered `instances` map), carrying only what the views selected —
/// facts from the first `frontmatter` view, body from the `markdown` view,
/// activity from the `timeline` view. `None` when the report has no
/// instances (a row report without a chart stays PNG-less, as before).
fn instance_card_request(
    skill: &crate::skill::ReportSkill,
    pages: &BTreeMap<String, InstancePage>,
) -> Option<peacock_rasterizer::InstanceCardRequest> {
    use crate::skill::ViewSpec;

    let (alias, page) = skill
        .instances
        .keys()
        .next()
        .and_then(|a| pages.get(a).map(|p| (a.clone(), p)))?;

    let display = |v: &Value| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let mut req = peacock_rasterizer::InstanceCardRequest {
        title: page
            .frontmatter
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&page.id)
            .to_owned(),
        subtitle: format!("{} · {}", page.skill, page.id),
        ..Default::default()
    };
    for view in &skill.views {
        match view {
            ViewSpec::Frontmatter { instance, keys, .. }
                if *instance == alias && req.facts.is_empty() =>
            {
                req.facts = keys
                    .iter()
                    .filter_map(|k| page.frontmatter.get(k).map(|v| (k.clone(), display(v))))
                    .collect();
            }
            ViewSpec::Markdown { instance } if *instance == alias => {
                req.body_lines = page
                    .body
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_owned)
                    .collect();
            }
            ViewSpec::Timeline { instance, limit } if *instance == alias => {
                req.events = page
                    .events
                    .iter()
                    .take(*limit as usize)
                    .map(|e| {
                        let body_first = e.body.lines().next().unwrap_or_default();
                        let when = if e.at.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", e.at)
                        };
                        (e.title.clone(), format!("{}{when} · {body_first}", e.label))
                    })
                    .collect();
            }
            _ => {}
        }
    }
    Some(req)
}

/// Rasterize a single Vega-Lite chart spec to PNG — the `render_a2ui_to_png`
/// capability Triton's chat surface delegates to (FR-V-2, FR-C-2).
pub fn render_a2ui_to_png(spec: &serde_json::Value, scale: f32) -> Result<Vec<u8>> {
    Ok(peacock_rasterizer::render_vega_to_png(spec, scale)?)
}

/// Bind a shared selection into the param vector when the report declares its
/// dimension and the caller did not supply that param (FR-X-6 / OQ-5). Caller
/// params win; a report without the dimension is returned unchanged. Returns an
/// owned `Value` so the caller's input is never mutated (statelessness).
fn apply_selection(
    params: &Value,
    selection: Option<&SharedSelection>,
    schema: &peacock_types::ParamSchema,
) -> Value {
    let Some(sel) = selection else {
        return params.clone();
    };
    // The report must declare the selection's dimension to inherit it.
    if !schema.0.contains_key(&sel.dimension) {
        return params.clone();
    }
    let mut obj = match params.as_object() {
        Some(o) => o.clone(),
        None => return params.clone(), // let validation report the bad shape
    };
    // The caller's absolute param wins; only fill an absent/null dimension.
    let unset = obj.get(&sel.dimension).map(Value::is_null).unwrap_or(true);
    if unset {
        obj.insert(sel.dimension.clone(), sel.value.clone());
    }
    Value::Object(obj)
}

/// Promote a committed drill to a shared, named selection (FR-X-6 / OQ-5): the
/// first param whose current value differs from its declared default is the
/// salient selection ("the thing the user drilled into"). Returns `None` when
/// every param sits at its default (nothing committed to promote). The
/// selection's `name` is its `dimension` — the conversation context may rename
/// it. Reads only the artifact's compact view state; never the rows.
pub fn promotable_selection(artifact: &Artifact) -> Option<SharedSelection> {
    let current = artifact.structured_content.current_params.as_object()?;
    let schema = artifact.structured_content.param_schema.as_object()?;
    for (name, value) in current {
        let default = schema
            .get(name)
            .and_then(|spec| spec.get("default"))
            .unwrap_or(&Value::Null);
        if value != default {
            return Some(SharedSelection::new(
                name.clone(),
                name.clone(),
                value.clone(),
            ));
        }
    }
    None
}

/// Build the compact view-state record pushed to the model on a committed
/// drill (FR-X-3, ACC-12): `{report_id, params, salient_summary}` — **never
/// rows**. When a param has been drilled off its default it also carries the
/// promotable shared `selection` (FR-X-6) other reports can inherit. Returned
/// to the surface shells so the MCP-App / chat paths can emit it via
/// `updateModelContext` / the signed token.
pub fn view_state_record(report_id: &str, artifact: &Artifact, summary: &str) -> Value {
    let mut rec = json!({
        "report_id": report_id,
        "params": artifact.structured_content.current_params,
        "salient_summary": summary,
    });
    if let Some(sel) = promotable_selection(artifact) {
        rec["selection"] = serde_json::to_value(sel).unwrap_or(Value::Null);
    }
    rec
}
