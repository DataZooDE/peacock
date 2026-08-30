//! The sales-manager leaderboard: revenue per **salesperson**, ranked.
//!
//! The Northwind fixture has carried a `salesperson` column since day one
//! with no report over it; this is the natural "who sold the most?"
//! question a sales manager asks. Real escurel, real Parquet, real
//! `query_instance` — no mocks (CLAUDE principle 2). The chart exercises
//! the rasterizer's horizontal-bar + `sort: {field, order}` features
//! (#4/#5): salespeople on y, revenue on x, best first.

use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{NW_REPORT_LEADERBOARD, NorthwindEscurel};
use serde_json::json;

#[tokio::test]
async fn leaderboard_ranks_salespeople_by_revenue() {
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let art = render(
        NW_REPORT_LEADERBOARD,
        &json!({}), // defaults: full 1997
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render the salesperson leaderboard");

    // One row per salesperson, revenue descending (the query orders it).
    let rows = art.structured_content.rows.as_array().unwrap();
    assert!(rows.len() >= 3, "several salespeople in 1997: {rows:?}");
    let revenues: Vec<f64> = rows
        .iter()
        .map(|r| r["revenue"].as_f64().expect("numeric revenue"))
        .collect();
    assert!(
        revenues.windows(2).all(|w| w[0] >= w[1]),
        "leaderboard must be ranked best-first: {revenues:?}"
    );
    assert!(
        rows.iter().all(|r| r["salesperson"].is_string()),
        "each row names its salesperson"
    );

    // The date params bind: a window covering only part of 1997 shrinks
    // total revenue versus the full year.
    let full: f64 = revenues.iter().sum();
    let art_q1 = render(
        NW_REPORT_LEADERBOARD,
        &json!({ "from": "1997-01-01", "to": "1997-03-31" }),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render Q1 window");
    let q1: f64 = art_q1
        .structured_content
        .rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["revenue"].as_f64().unwrap())
        .sum();
    assert!(
        q1 > 0.0 && q1 < full,
        "Q1 revenue ({q1}) must be a strict, non-empty subset of 1997 ({full})"
    );

    nw.shutdown().await;
}
