//! FR-X-6 / OQ-5 no-mock end-to-end: a committed drill on one report is
//! promoted to a **shared, named selection** in the conversation context, and
//! a *different* report that also accepts that dimension **inherits** it on its
//! next render — without peacock holding any state (the selection travels in
//! `RenderOpts`, never stored). A report that does not declare the dimension
//! ignores the selection (no error). Spec: BRD §5.6 FR-X-6, §9 OQ-5; HLD
//! §state-sync.

use peacock_core::{EscurelData, RenderOpts, promotable_selection, render, view_state_record};
use peacock_test_support::{NW_REPORT, NW_REPORT_PRODUCTS, NW_REPORT_SEASON, NorthwindEscurel};
use serde_json::json;

#[tokio::test]
async fn a_committed_drill_promotes_to_a_shared_selection_other_reports_inherit() {
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();
    let opts = RenderOpts::default();

    // 1. Render report A with a committed drill `category=Beverages`.
    let art_a = render(
        NW_REPORT,
        &json!({ "category": "Beverages" }),
        &principal,
        &escurel,
        &opts,
    )
    .await
    .expect("render report A drilled to Beverages");

    // 2. Promote that committed drill to a shared, named selection (a compact
    //    view-state projection — the non-default dimension value).
    let selection = promotable_selection(&art_a).expect("Beverages is a non-default selection");
    assert_eq!(selection.dimension, "category");
    assert_eq!(selection.value, json!("Beverages"));

    // It also rides along in the compact view-state record (no rows).
    let rec = view_state_record(NW_REPORT, &art_a, "Beverages, full year 1997");
    assert_eq!(rec["selection"]["dimension"], "category");
    assert_eq!(rec["selection"]["value"], "Beverages");
    assert!(rec.get("rows").is_none());

    // 3. Render a DIFFERENT report (seasonality) that also accepts `category`,
    //    passing the shared selection in — it inherits Beverages.
    let opts_inherit = RenderOpts {
        selection: Some(selection.clone()),
        ..Default::default()
    };
    let art_b = render(
        NW_REPORT_SEASON,
        &json!({}), // caller supplies no category → the shared selection wins
        &principal,
        &escurel,
        &opts_inherit,
    )
    .await
    .expect("render report B inheriting the shared selection");

    assert_eq!(
        art_b.structured_content.current_params["category"], "Beverages",
        "the second report inherited the shared selection's dimension value"
    );
    // Rows are filtered to Beverages: every row's category is Beverages.
    let rows = art_b.structured_content.rows.as_array().unwrap();
    assert!(!rows.is_empty(), "seasonality returned Beverages rows");
    assert!(
        rows.iter().all(|r| r["category"] == json!("Beverages")),
        "every row is the Beverages category"
    );

    // 4. A report WITHOUT a `category` param (top-products) ignores the
    //    selection: no error, and it renders ALL products (not Beverages-only).
    let art_c = render(
        NW_REPORT_PRODUCTS,
        &json!({}),
        &principal,
        &escurel,
        &opts_inherit,
    )
    .await
    .expect("a report lacking the dimension ignores the selection (no error)");
    assert!(
        art_c
            .structured_content
            .current_params
            .get("category")
            .is_none(),
        "top-products declares no category param; the selection is ignored"
    );
    let prod_rows = art_c.structured_content.rows.as_array().unwrap();
    assert!(
        prod_rows.len() > 1,
        "all products rendered, not filtered to one category"
    );

    nw.shutdown().await;
}

#[tokio::test]
async fn caller_supplied_absolute_param_wins_over_the_shared_selection() {
    // Statelessness + absolute-params invariant: an explicit param the caller
    // passes beats the inherited shared selection (no drift, HLD §state-sync).
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let selection = peacock_types::SharedSelection {
        name: "focus".into(),
        dimension: "category".into(),
        value: json!("Beverages"),
    };
    let opts = RenderOpts {
        selection: Some(selection),
        ..Default::default()
    };
    let art = render(
        NW_REPORT_SEASON,
        &json!({ "category": "Seafood" }), // caller is explicit
        &principal,
        &escurel,
        &opts,
    )
    .await
    .expect("render with both an explicit param and a shared selection");

    assert_eq!(
        art.structured_content.current_params["category"], "Seafood",
        "the caller's absolute param wins over the shared selection"
    );

    nw.shutdown().await;
}
