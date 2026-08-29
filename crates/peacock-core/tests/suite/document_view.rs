//! The reserved `document` pseudo-report, no-mock: `render("document",
//! {skill, id})` renders ONE escurel instance page as a document — the
//! chat-reply "Sources" target. The target's SKILL page is the contract:
//! an optional `viewer:` delegates to a richer authored report; an
//! `actions:` list declares what a reader may do from the document
//! (`prompt` back to the chat, `event` into escurel). Real escurel, real
//! fixtures; the caller principal is forwarded (fail-closed ACL).

use peacock_core::skill::ReportSkill;
use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{NorthwindEscurel, NorthwindOpts};
use serde_json::json;

/// The account skill declares actions but NO viewer — the generic
/// document view renders it.
const ACCOUNT_SKILL_ACTIONS: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n\
    optional_frontmatter: [status, category, email]\n\
    actions:\n\
    \x20 - name: propose-nba\n\
    \x20   kind: prompt\n\
    \x20   label: Propose next best action\n\
    \x20   prompt: \"whats the next best action for {id}?\"\n\
    \x20 - name: renewal-at-risk\n\
    \x20   kind: event\n\
    \x20   label: Flag renewal at risk\n\
    \x20   event: follow_up\n\
    \x20   title: \"{frontmatter.name}\"\n\
    \x20   body: \"renewal at risk (flagged from the document)\"\n\
    ---\n# account\n";

/// The same skill WITH a viewer: the document view delegates to the
/// authored customer-report.
const ACCOUNT_SKILL_VIEWER: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n\
    optional_frontmatter: [status, category, email]\n\
    viewer: { report: customer-report, param: account }\n\
    actions:\n\
    \x20 - name: propose-nba\n\
    \x20   kind: prompt\n\
    \x20   label: Propose next best action\n\
    \x20   prompt: \"whats the next best action for {id}?\"\n\
    ---\n# account\n";

const BEVERAGES_GMBH: &str = "---\ntype: instance\nskill: account\nid: beverages-gmbh\n\
    name: Beverages GmbH\nstatus: follow_up\ncategory: Beverages\n\
    email: maria@beverages.example\n---\n# Beverages GmbH\n\n\
    EU beverages distributor; renewal due in Q3.\n\n\
    See [[email::mail-1]] for the renewal thread.\n";

const CUSTOMER_REPORT: &str = "---\ntype: skill\nid: customer-report\nrender: a2ui\n\
    description: One customer account as a card.\n\
    params:\n  account: { type: string }\n\
    instances:\n  acct: \"[[account::{account}]]\"\n\
    views:\n\
      - { kind: frontmatter, instance: acct, keys: [name, status], label: Account }\n\
      - { kind: markdown, instance: acct }\n\
    ---\nThe account record, rendered.\n";

fn doc_opts(account_skill: &str) -> NorthwindOpts {
    NorthwindOpts {
        extra_skills: vec![
            ("account".to_owned(), account_skill.to_owned()),
            ("customer-report".to_owned(), CUSTOMER_REPORT.to_owned()),
        ],
        extra_instances: vec![(
            "account".to_owned(),
            "beverages-gmbh".to_owned(),
            BEVERAGES_GMBH.to_owned(),
        )],
        ..Default::default()
    }
}

#[tokio::test]
async fn generic_document_view_renders_facts_markdown_timeline_and_actions() {
    let nw = NorthwindEscurel::spawn_with(doc_opts(ACCOUNT_SKILL_ACTIONS)).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let art = render(
        "document",
        &json!({ "skill": "account", "id": "beverages-gmbh" }),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render the generic document view");

    let comps = art.a2ui["components"].as_array().unwrap();
    // Facts from the page's OWN frontmatter (structural keys excluded).
    let facts = comps
        .iter()
        .find(|c| c["kind"] == "frontmatter")
        .expect("a frontmatter component");
    let keys: Vec<&str> = facts["facts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect();
    assert!(
        keys.contains(&"name") && keys.contains(&"status"),
        "{keys:?}"
    );
    assert!(
        !keys.contains(&"type") && !keys.contains(&"skill") && !keys.contains(&"id"),
        "structural keys are not facts: {keys:?}"
    );
    // The body rides a markdown view (wikilinks intact — the runtime encodes).
    let md = comps
        .iter()
        .find(|c| c["kind"] == "markdown")
        .expect("a markdown component");
    assert!(md["value"].as_str().unwrap().contains("[[email::mail-1]]"));
    // A timeline view is always composed (empty history is fine).
    assert!(comps.iter().any(|c| c["kind"] == "timeline"));

    // The typed document contract: identity + resolved actions. Prompt
    // templates are substituted server-side; event actions carry NO
    // title/body (server-side only — the client sends back the name).
    let doc = art
        .structured_content
        .document
        .as_ref()
        .expect("document in structuredContent");
    assert_eq!(doc["skill"], "account");
    assert_eq!(doc["id"], "beverages-gmbh");
    let actions = doc["actions"].as_array().expect("actions");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["name"], "propose-nba");
    assert_eq!(actions[0]["kind"], "prompt");
    assert_eq!(
        actions[0]["prompt"],
        "whats the next best action for beverages-gmbh?"
    );
    assert_eq!(actions[1]["name"], "renewal-at-risk");
    assert_eq!(actions[1]["kind"], "event");
    assert!(actions[1].get("title").is_none() && actions[1].get("body").is_none());
    // The instances contract also rides along for the runtime.
    assert!(art.structured_content.instances.is_some());

    nw.shutdown().await;
}

#[tokio::test]
async fn viewer_delegation_renders_the_declared_report_with_actions() {
    let nw = NorthwindEscurel::spawn_with(doc_opts(ACCOUNT_SKILL_VIEWER)).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let art = render(
        "document",
        &json!({ "skill": "account", "id": "beverages-gmbh" }),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("delegate to the viewer report");

    // The customer-report's views rendered (its frontmatter view label).
    let comps = art.a2ui["components"].as_array().unwrap();
    let facts = comps
        .iter()
        .find(|c| c["kind"] == "frontmatter")
        .expect("the viewer's frontmatter view");
    assert_eq!(facts["label"], "Account");

    // The document contract still rides the artifact (actions from the
    // TARGET's skill page, not the viewer report).
    let doc = art.structured_content.document.as_ref().expect("document");
    assert_eq!(doc["skill"], "account");
    assert_eq!(doc["actions"][0]["name"], "propose-nba");

    nw.shutdown().await;
}

#[tokio::test]
async fn frontmatter_placeholder_substitutes_and_missing_key_fails() {
    let nw = NorthwindEscurel::spawn_with(doc_opts(ACCOUNT_SKILL_ACTIONS)).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    // `{frontmatter.name}` in the event action's title substituted from the
    // page — proven indirectly: the render succeeds (substitution happens
    // server-side even though title never ships to the client).
    render(
        "document",
        &json!({ "skill": "account", "id": "beverages-gmbh" }),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("substitution over present keys renders");

    // A skill page naming an ABSENT frontmatter key is an author error.
    const BAD_KEY_SKILL: &str = "---\ntype: skill\nid: account\n\
        description: A customer account.\nrequired_frontmatter: [id, name]\n\
        actions:\n\
        \x20 - name: bad\n\
        \x20   kind: prompt\n\
        \x20   label: Bad\n\
        \x20   prompt: \"about {frontmatter.nope}\"\n\
        ---\n# account\n";
    let nw2 = NorthwindEscurel::spawn_with(doc_opts(BAD_KEY_SKILL)).await;
    let escurel2 = EscurelData::new(nw2.endpoint());
    let err = render(
        "document",
        &json!({ "skill": "account", "id": "beverages-gmbh" }),
        &nw2.sales_principal(),
        &escurel2,
        &RenderOpts::default(),
    )
    .await
    .expect_err("a missing frontmatter key must fail the render");
    assert!(
        matches!(err, peacock_types::Error::Render(_)),
        "render (author) error, got: {err:?}"
    );

    nw.shutdown().await;
    nw2.shutdown().await;
}

#[tokio::test]
async fn smuggled_document_params_fail_closed() {
    let nw = NorthwindEscurel::spawn_with(doc_opts(ACCOUNT_SKILL_ACTIONS)).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    for (skill, id) in [
        ("../../etc", "beverages-gmbh"),
        ("account", "x]] [[y"),
        ("skill::evil", "beverages-gmbh"),
        ("account", "../secret"),
        ("", "beverages-gmbh"),
        ("account", ""),
    ] {
        let res = render(
            "document",
            &json!({ "skill": skill, "id": id }),
            &principal,
            &escurel,
            &RenderOpts::default(),
        )
        .await;
        match res {
            Err(peacock_types::Error::Validation(_)) => {}
            other => panic!("`{skill}::{id}` must be a validation rejection, got: {other:?}"),
        }
    }
    // Missing params entirely: validation, not a phantom render.
    let err = render(
        "document",
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect_err("skill+id are required");
    assert!(matches!(err, peacock_types::Error::Validation(_)));

    nw.shutdown().await;
}

#[tokio::test]
async fn reserved_id_shadows_an_authored_document_report() {
    // An authored report skill named `document` is never resolved — the
    // reserved pseudo-report intercepts first (documented contract).
    const IMPOSTER: &str = "---\ntype: skill\nid: document\nrender: a2ui\n\
        description: An imposter.\nparams: {}\ndata: {}\nviews: []\n---\n";
    let mut opts = doc_opts(ACCOUNT_SKILL_ACTIONS);
    opts.extra_skills
        .push(("document".to_owned(), IMPOSTER.to_owned()));
    let nw = NorthwindEscurel::spawn_with(opts).await;
    let escurel = EscurelData::new(nw.endpoint());

    // With the pseudo-report intercepting, a call WITHOUT skill/id params is
    // a validation error — the imposter (which needs none) never renders.
    let err = render(
        "document",
        &json!({}),
        &nw.sales_principal(),
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect_err("the reserved id must not fall through to the authored page");
    assert!(matches!(err, peacock_types::Error::Validation(_)));

    nw.shutdown().await;
}

#[tokio::test]
async fn group_gated_document_fails_closed() {
    const GATED: &str = "---\ntype: skill\nid: account\n\
        description: A customer account.\nrequired_frontmatter: [id, name]\n\
        acl: { read: [sales] }\n\
        actions:\n\
        \x20 - name: propose-nba\n\
        \x20   kind: prompt\n\
        \x20   label: Propose next best action\n\
        \x20   prompt: \"next for {id}?\"\n\
        ---\n# account\n";
    let nw = NorthwindEscurel::spawn_with(doc_opts(GATED)).await;
    let escurel = EscurelData::new(nw.endpoint());

    render(
        "document",
        &json!({ "skill": "account", "id": "beverages-gmbh" }),
        &nw.sales_principal(),
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("sales may read the gated document");

    render(
        "document",
        &json!({ "skill": "account", "id": "beverages-gmbh" }),
        &nw.no_sales_principal(),
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect_err("no `sales` group must fail closed");

    nw.shutdown().await;
}

// ---- frontmatter parse validation (author errors caught at parse time) ----

fn parse(fm_yaml: &str) -> peacock_types::Result<ReportSkill> {
    let fm: serde_json::Value = serde_yaml_ng::from_str(fm_yaml).unwrap();
    ReportSkill::from_frontmatter("account", &fm, "")
}

#[test]
fn action_parse_validation() {
    // A well-formed pair parses.
    let ok = parse(
        "actions:\n- {name: a, kind: prompt, label: L, prompt: p}\n- {name: b, kind: event, label: E, event: follow_up}\n",
    )
    .expect("well-formed actions parse");
    assert_eq!(ok.actions.len(), 2);

    // Unknown kind.
    assert!(parse("actions:\n- {name: a, kind: dance, label: L}\n").is_err());
    // Non-slug name.
    assert!(parse("actions:\n- {name: 'a b', kind: prompt, label: L, prompt: p}\n").is_err());
    // Prompt action without a template.
    assert!(parse("actions:\n- {name: a, kind: prompt, label: L}\n").is_err());
    // Event action without an event label skill.
    assert!(parse("actions:\n- {name: a, kind: event, label: L}\n").is_err());
    // Non-slug event label skill.
    assert!(parse("actions:\n- {name: a, kind: event, label: L, event: 'x y'}\n").is_err());
    // Empty label.
    assert!(parse("actions:\n- {name: a, kind: prompt, label: '', prompt: p}\n").is_err());

    // Viewer: well-formed + malformed.
    let v = parse("viewer: { report: customer-report, param: account }\n").expect("viewer parses");
    let viewer = v.viewer.expect("viewer present");
    assert_eq!(viewer.report, "customer-report");
    assert_eq!(viewer.param, "account");
    assert!(parse("viewer: { report: 'a b', param: account }\n").is_err());
    assert!(parse("viewer: { report: document, param: account }\n").is_err());
    assert!(parse("viewer: { report: r }\n").is_err());
}
