//! The `timeline` view, no-mock: an instance report renders the page's
//! escurel EVENT HISTORY (the filed emails, the flags — whatever the
//! platform captured against the record). Real escurel; events are captured
//! **and assigned** in the test because escurel's `list_events` returns only
//! `processed` events, oldest first — a captured-but-unassigned inbox event
//! is invisible to a timeline (by design: the inbox is a work queue, the
//! history is the folded outcome).

use escurel_client::{AssignEventRequest, CaptureEventRequest};
use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{NorthwindEscurel, NorthwindOpts};
use serde_json::json;

const ACCOUNT_SKILL: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n---\n# account\n";

const BEVERAGES_GMBH: &str = "---\ntype: instance\nskill: account\nid: beverages-gmbh\n\
    name: Beverages GmbH\n---\n# Beverages GmbH\n\nEU distributor.\n";

const ACCOUNT_PAGE: &str = "markdown/instances/account/beverages-gmbh.md";

/// A customer report with an activity timeline (limit 2 pins the cap).
const TIMELINE_REPORT: &str = "---\ntype: skill\nid: timeline-report\nrender: a2ui\n\
    description: One customer's recent activity.\n\
    params:\n  account: { type: string }\n\
    instances:\n  acct: \"[[account::{account}]]\"\n\
    views:\n\
      - { kind: timeline, instance: acct, limit: 2 }\n\
    ---\n";

fn opts() -> NorthwindOpts {
    NorthwindOpts {
        extra_skills: vec![
            ("account".to_owned(), ACCOUNT_SKILL.to_owned()),
            ("timeline-report".to_owned(), TIMELINE_REPORT.to_owned()),
        ],
        extra_instances: vec![(
            "account".to_owned(),
            "beverages-gmbh".to_owned(),
            BEVERAGES_GMBH.to_owned(),
        )],
        ..Default::default()
    }
}

/// Capture one event against the account page and ASSIGN it (only processed
/// events are history).
async fn account_event(client: &escurel_client::Client, title: &str, body: &str) {
    let ev = client
        .capture_event(CaptureEventRequest {
            source: "mail".to_owned(),
            mime: "text/plain".to_owned(),
            label_skill: "account_activity".to_owned(),
            instance_page_id: ACCOUNT_PAGE.to_owned(),
            title: title.to_owned(),
            body: body.to_owned(),
            ..Default::default()
        })
        .await
        .expect("capture");
    client
        .assign_event(AssignEventRequest {
            event_id: ev.event_id,
            instance_page_id: ACCOUNT_PAGE.to_owned(),
        })
        .await
        .expect("assign");
}

#[tokio::test]
async fn timeline_renders_the_instances_processed_events() {
    let nw = NorthwindEscurel::spawn_with(opts()).await;
    let client = nw.sales_client().await;
    account_event(&client, "Email filed", "Renewal question").await;
    account_event(&client, "Flagged", "renewal at risk").await;

    let escurel = EscurelData::new(nw.endpoint());
    let art = render(
        "timeline-report",
        &json!({ "account": "beverages-gmbh" }),
        &nw.sales_principal(),
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render the timeline report");

    let comps = art.a2ui["components"].as_array().unwrap();
    let timeline = comps
        .iter()
        .find(|c| c["kind"] == "timeline")
        .expect("a timeline component");
    let events = timeline["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    // Oldest first — escurel's folded-history order is kept.
    assert_eq!(events[0]["title"], "Email filed");
    assert_eq!(events[0]["body"], "Renewal question");
    assert_eq!(events[1]["title"], "Flagged");
    // `at` is always present but may be empty — escurel stores no timestamp
    // when the capturer omits one (the renderer shows what history there is).
    assert!(events[0]["at"].is_string());

    // The typed contract mirrors it.
    let inst = art.structured_content.instances.as_ref().unwrap();
    assert_eq!(inst["acct"]["events"].as_array().unwrap().len(), 2);

    nw.shutdown().await;
}

#[tokio::test]
async fn timeline_limit_caps_and_unassigned_events_are_invisible() {
    let nw = NorthwindEscurel::spawn_with(opts()).await;
    let client = nw.sales_client().await;
    account_event(&client, "one", "1").await;
    account_event(&client, "two", "2").await;
    account_event(&client, "three", "3").await;
    // Captured but NOT assigned: still in the inbox → not history.
    client
        .capture_event(CaptureEventRequest {
            source: "mail".to_owned(),
            mime: "text/plain".to_owned(),
            label_skill: "account_activity".to_owned(),
            instance_page_id: ACCOUNT_PAGE.to_owned(),
            title: "pending".to_owned(),
            body: "unassigned".to_owned(),
            ..Default::default()
        })
        .await
        .expect("capture unassigned");

    let escurel = EscurelData::new(nw.endpoint());
    let art = render(
        "timeline-report",
        &json!({ "account": "beverages-gmbh" }),
        &nw.sales_principal(),
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render");
    let comps = art.a2ui["components"].as_array().unwrap();
    let events = comps.iter().find(|c| c["kind"] == "timeline").unwrap()["events"]
        .as_array()
        .unwrap()
        .clone();
    // limit: 2 caps the three processed events; the unassigned one never shows.
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e["title"] != "pending"));

    nw.shutdown().await;
}

#[tokio::test]
async fn empty_history_still_emits_the_component() {
    // Deterministic layout: no activity yet → `events: []`, never a missing
    // component (ADR-P7 reproducibility; the iframe shows "No activity yet").
    let nw = NorthwindEscurel::spawn_with(opts()).await;
    let escurel = EscurelData::new(nw.endpoint());
    let art = render(
        "timeline-report",
        &json!({ "account": "beverages-gmbh" }),
        &nw.sales_principal(),
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render");
    let comps = art.a2ui["components"].as_array().unwrap();
    let timeline = comps.iter().find(|c| c["kind"] == "timeline").unwrap();
    assert_eq!(timeline["events"], json!([]));

    nw.shutdown().await;
}
