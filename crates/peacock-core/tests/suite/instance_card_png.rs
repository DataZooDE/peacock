//! The chat surface for INSTANCE reports: with `png_scale` set and no chart
//! in the report, the artifact carries a rasterized **instance card**
//! (title + facts + body + activity) — the same pure-Rust path as the
//! dashboard raster, themed via `RenderOpts.theme`. Real escurel.

use escurel_client::{AssignEventRequest, CaptureEventRequest};
use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{NorthwindEscurel, NorthwindOpts};
use serde_json::json;

const ACCOUNT_SKILL: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n---\n# account\n";

const BEVERAGES_GMBH: &str = "---\ntype: instance\nskill: account\nid: beverages-gmbh\n\
    name: Beverages GmbH\nstatus: follow_up\n---\n# Beverages GmbH\n\n\
    EU beverages distributor; renewal due in Q3.\n";

const CUSTOMER_REPORT: &str = "---\ntype: skill\nid: customer-report\nrender: a2ui\n\
    description: One customer account as a card.\n\
    params:\n  account: { type: string }\n\
    instances:\n  acct: \"[[account::{account}]]\"\n\
    views:\n\
      - { kind: frontmatter, instance: acct, keys: [name, status], label: Account }\n\
      - { kind: markdown, instance: acct }\n\
      - { kind: timeline, instance: acct, limit: 5 }\n\
    ---\n";

#[tokio::test]
async fn instance_report_renders_a_card_png() {
    let nw = NorthwindEscurel::spawn_with(NorthwindOpts {
        extra_skills: vec![
            ("account".to_owned(), ACCOUNT_SKILL.to_owned()),
            ("customer-report".to_owned(), CUSTOMER_REPORT.to_owned()),
        ],
        extra_instances: vec![(
            "account".to_owned(),
            "beverages-gmbh".to_owned(),
            BEVERAGES_GMBH.to_owned(),
        )],
        ..Default::default()
    })
    .await;
    // One assigned event so the card carries an activity line too.
    let client = nw.sales_client().await;
    let ev = client
        .capture_event(CaptureEventRequest {
            source: "mail".to_owned(),
            mime: "text/plain".to_owned(),
            label_skill: "account_activity".to_owned(),
            instance_page_id: "markdown/instances/account/beverages-gmbh.md".to_owned(),
            title: "Email filed".to_owned(),
            body: "Renewal question".to_owned(),
            ..Default::default()
        })
        .await
        .expect("capture");
    client
        .assign_event(AssignEventRequest {
            event_id: ev.event_id,
            instance_page_id: "markdown/instances/account/beverages-gmbh.md".to_owned(),
        })
        .await
        .expect("assign");

    let escurel = EscurelData::new(nw.endpoint());
    let opts = RenderOpts {
        png_scale: Some(2.0),
        ..Default::default()
    };
    let art = render(
        "customer-report",
        &json!({ "account": "beverages-gmbh" }),
        &nw.sales_principal(),
        &escurel,
        &opts,
    )
    .await
    .expect("render");

    // No chart in this report — the PNG is the instance CARD.
    assert!(art.vega_specs.is_empty());
    let png = art.png.as_ref().expect("an instance-card png");
    assert!(png.len() > 1000, "a real PNG, got {} bytes", png.len());
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic");

    // Purity: the same render yields the same bytes (stateless re-render).
    let again = render(
        "customer-report",
        &json!({ "account": "beverages-gmbh" }),
        &nw.sales_principal(),
        &escurel,
        &opts,
    )
    .await
    .expect("re-render");
    assert_eq!(art.png, again.png, "byte-reproducible card raster");

    nw.shutdown().await;
}
