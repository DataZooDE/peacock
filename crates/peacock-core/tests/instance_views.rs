//! Instance rendering, no-mock: a report skill whose views render an escurel
//! **instance page** (frontmatter facts + markdown body) instead of query
//! rows — the "customer report" the agent delegates to peacock. Real escurel,
//! real fixtures; the caller principal is forwarded (fail-closed ACL).
//!
//! The report skill declares `instances:` (alias → `[[skill::{param}]]`,
//! placeholders filled from the ABSOLUTE param vector) and lays out
//! `frontmatter` / `markdown` views over the resolved page.

use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{NorthwindEscurel, NorthwindOpts};
use serde_json::json;

const ACCOUNT_SKILL: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n\
    optional_frontmatter: [status, category, email]\n---\n# account\n";

const BEVERAGES_GMBH: &str = "---\ntype: instance\nskill: account\nid: beverages-gmbh\n\
    name: Beverages GmbH\nstatus: follow_up\ncategory: Beverages\n\
    email: maria@beverages.example\n---\n# Beverages GmbH\n\n\
    EU beverages distributor; renewal due in Q3.\n\n\
    Follow-up scheduled: renewal at risk.\n";

/// A group-gated account skill: only `sales` may read its instances.
const GATED_ACCOUNT_SKILL: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n\
    acl: { read: [sales] }\n---\n# account\n";

/// The customer report: one instance alias, facts + body views. `account` is
/// a REQUIRED string param (no default) substituted into the instance ref.
const CUSTOMER_REPORT: &str = "---\ntype: skill\nid: customer-report\nrender: a2ui\n\
    description: One customer account as a card.\n\
    params:\n  account: { type: string }\n\
    instances:\n  acct: \"[[account::{account}]]\"\n\
    views:\n\
      - { kind: frontmatter, instance: acct, keys: [name, status, category, email], label: Account }\n\
      - { kind: markdown, instance: acct }\n\
    ---\nThe account record, rendered.\n";

fn customer_report_opts(account_skill: &str) -> NorthwindOpts {
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
async fn renders_markdown_and_frontmatter_views_from_an_instance() {
    let nw = NorthwindEscurel::spawn_with(customer_report_opts(ACCOUNT_SKILL)).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let art = render(
        "customer-report",
        &json!({ "account": "beverages-gmbh" }),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render the customer report");

    let comps = art.a2ui["components"].as_array().unwrap();

    // The facts view: declared keys present on the page, in declared order.
    let facts = comps
        .iter()
        .find(|c| c["kind"] == "frontmatter")
        .expect("a frontmatter component");
    assert_eq!(facts["label"], "Account");
    let pairs: Vec<(&str, &str)> = facts["facts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["key"].as_str().unwrap(), f["value"].as_str().unwrap()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("name", "Beverages GmbH"),
            ("status", "follow_up"),
            ("category", "Beverages"),
            ("email", "maria@beverages.example"),
        ]
    );

    // The markdown view: the page BODY, raw (encoding is the renderer's job).
    let md = comps
        .iter()
        .find(|c| c["kind"] == "markdown")
        .expect("a markdown component");
    let body = md["value"].as_str().unwrap();
    assert!(
        body.contains("EU beverages distributor")
            && body.contains("Follow-up scheduled: renewal at risk."),
        "the markdown carries the page body: {body}"
    );

    // structuredContent: no query rows, but the instance contract for
    // programmatic consumers (facts + markdown, keyed by alias).
    assert_eq!(art.structured_content.rows, json!([]));
    let instances = art
        .structured_content
        .instances
        .as_ref()
        .expect("instances in structuredContent");
    let acct = &instances["acct"];
    assert_eq!(acct["skill"], "account");
    assert_eq!(acct["id"], "beverages-gmbh");
    assert!(
        acct["page_id"].as_str().unwrap().contains("beverages-gmbh"),
        "{acct}"
    );
    assert_eq!(acct["facts"][0]["key"], "name");
    assert!(
        acct["markdown"]
            .as_str()
            .unwrap()
            .contains("EU beverages distributor")
    );
    // Params resolved as usual (FR-X-1).
    assert_eq!(
        art.structured_content.current_params["account"],
        "beverages-gmbh"
    );

    nw.shutdown().await;
}

#[tokio::test]
async fn missing_account_param_is_a_validation_error() {
    // `account` declares no default — omitting it must be a Validation error,
    // not a render of some phantom page.
    let nw = NorthwindEscurel::spawn_with(customer_report_opts(ACCOUNT_SKILL)).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let err = render(
        "customer-report",
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect_err("missing required param must fail");
    assert!(
        matches!(err, peacock_types::Error::Validation(_)),
        "validation error, got: {err:?}"
    );

    nw.shutdown().await;
}

#[tokio::test]
async fn placeholder_smuggling_is_rejected() {
    // The substituted instance id is validated against a strict slug charset —
    // path/wikilink/namespace smuggling never reaches escurel.
    let nw = NorthwindEscurel::spawn_with(customer_report_opts(ACCOUNT_SKILL)).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    for evil in ["../../etc", "x]] [[y", "a:b", "a/b", "", " "] {
        let res = render(
            "customer-report",
            &json!({ "account": evil }),
            &principal,
            &escurel,
            &RenderOpts::default(),
        )
        .await;
        match res {
            Err(peacock_types::Error::Validation(_)) => {}
            other => panic!("`{evil}` must be a validation rejection, got: {other:?}"),
        }
    }

    nw.shutdown().await;
}

#[tokio::test]
async fn missing_instance_page_fails_closed() {
    // A well-formed id with no page behind it: Error::Data, never a partial
    // artifact.
    let nw = NorthwindEscurel::spawn_with(customer_report_opts(ACCOUNT_SKILL)).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let err = render(
        "customer-report",
        &json!({ "account": "ghost-corp" }),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect_err("a missing instance page must fail the whole render");
    assert!(
        matches!(err, peacock_types::Error::Data(_)),
        "data error, got: {err:?}"
    );

    nw.shutdown().await;
}

#[tokio::test]
async fn group_gated_instance_page_fails_closed() {
    // The account skill gates reads to `sales`; a principal without the group
    // gets NO artifact (escurel's fail-closed ACL travels through the
    // forwarded per-request principal).
    let nw = NorthwindEscurel::spawn_with(customer_report_opts(GATED_ACCOUNT_SKILL)).await;
    let escurel = EscurelData::new(nw.endpoint());

    // The sales principal renders fine…
    render(
        "customer-report",
        &json!({ "account": "beverages-gmbh" }),
        &nw.sales_principal(),
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("sales may read the gated account");

    // …the outsider gets an error, not a partial artifact.
    render(
        "customer-report",
        &json!({ "account": "beverages-gmbh" }),
        &nw.no_sales_principal(),
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect_err("no `sales` group must fail closed");

    nw.shutdown().await;
}

#[tokio::test]
async fn data_and_instances_coexist() {
    // A report may mix query rows and an instance page: revenue rows FOR the
    // account next to the account card. The whole absolute param vector binds
    // to every query (escurel rejects undeclared params fail-closed), so the
    // mixed report's query page declares `account` too — the natural shape:
    // the record and the numbers share the drill dimension.
    // (Rust's `\` line continuation strips leading whitespace — the YAML
    // indentation must ride inside the escapes.)
    const MIXED_QUERY: &str = "---\n\
        type: instance\n\
        skill: query\n\
        id: q_account_revenue\n\
        target: \"[[nw_order_lines::eu]]\"\n\
        params:\n\
        \x20 - {name: account, type: text, required: true}\n\
        sql: \"SELECT :account AS account, category AS category, \
        sum(unit_price * quantity)::DOUBLE AS revenue FROM {{target}} \
        GROUP BY 1, 2 ORDER BY 2\"\n\
        ---\n\
        # q_account_revenue\n";
    const MIXED_REPORT: &str = "---\ntype: skill\nid: mixed-report\nrender: a2ui\n\
        description: Rows and a record in one report.\n\
        params:\n  account: { type: string }\n\
        data:\n  rows: \"[[query::q_account_revenue]]\"\n\
        instances:\n  acct: \"[[account::{account}]]\"\n\
        views:\n\
        - { kind: table, data: rows }\n\
        - { kind: frontmatter, instance: acct, keys: [name], label: Account }\n\
        ---\n";
    let mut opts = customer_report_opts(ACCOUNT_SKILL);
    opts.extra_skills
        .push(("mixed-report".to_owned(), MIXED_REPORT.to_owned()));
    opts.extra_instances.push((
        "query".to_owned(),
        "q_account_revenue".to_owned(),
        MIXED_QUERY.to_owned(),
    ));
    let nw = NorthwindEscurel::spawn_with(opts).await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let art = render(
        "mixed-report",
        &json!({ "account": "beverages-gmbh" }),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("mixed report renders");
    let comps = art.a2ui["components"].as_array().unwrap();
    assert!(comps.iter().any(|c| c["kind"] == "table"));
    assert!(comps.iter().any(|c| c["kind"] == "frontmatter"));
    // Query rows stay the primary structuredContent rows.
    assert!(!art.structured_content.rows.as_array().unwrap().is_empty());
    assert!(art.structured_content.instances.is_some());

    nw.shutdown().await;
}
