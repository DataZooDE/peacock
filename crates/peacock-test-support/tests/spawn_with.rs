//! `NorthwindEscurel::spawn_with` — the consumer knobs: extra fixtures
//! merged into the Northwind seed + escurel `ConfigOverrides` passthrough
//! (here: a custom `groups_claim`, the Triton-fronted trust shape). Real
//! escurel, real Parquet; no mocks.

use escurel_client::ResolveRequest;
use escurel_test_support::ConfigOverrides;
use peacock_test_support::{NorthwindEscurel, NorthwindOpts};

const ACCOUNT_SKILL: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n---\n# account\n";
const ACCOUNT: &str = "---\ntype: instance\nskill: account\nid: beverages-gmbh\n\
    name: Beverages GmbH\n---\n# Beverages GmbH\n";

#[tokio::test]
async fn spawn_with_merges_fixtures_and_forwards_overrides() {
    let nw = NorthwindEscurel::spawn_with(NorthwindOpts {
        extra_skills: vec![("account".to_owned(), ACCOUNT_SKILL.to_owned())],
        extra_instances: vec![(
            "account".to_owned(),
            "beverages-gmbh".to_owned(),
            ACCOUNT.to_owned(),
        )],
        config_overrides: ConfigOverrides {
            // The Triton-fronted claim; the claim-aware TestIssuer keeps the
            // sales principal's group working under it.
            groups_claim: Some("triton_sender_groups".to_owned()),
            ..Default::default()
        },
    })
    .await;

    // The extra fixture is seeded alongside the Northwind world.
    let client = nw.sales_client().await;
    let resolved = client
        .resolve(ResolveRequest {
            wikilink: "[[account::beverages-gmbh]]".to_owned(),
            ..Default::default()
        })
        .await
        .expect("resolve extra instance");
    assert!(
        resolved.page.is_some(),
        "the consumer-supplied account instance is present"
    );

    // The sales principal still passes the `read: [sales]` group ACL with the
    // custom groups claim configured — expand of the ACL'd sql_view instance
    // returns the overlay (fail-closed would hide it).
    let page = client
        .call_raw(
            "expand",
            serde_json::json!({ "page_id": "markdown/instances/nw_order_lines/eu.md" }),
        )
        .await
        .expect("expand ACL'd view page");
    assert!(
        page["frontmatter"]["backend_ref"]["kind"] == "sql_view",
        "sales principal reads the group-ACL'd view under the custom claim: {page}"
    );

    // A consumer-minted service principal (the follow-up worker's shape).
    let worker = nw.mint_token_with_groups("acme", "follow-up-worker", &["sales"], false);
    assert!(!worker.is_empty());

    nw.shutdown().await;
}
