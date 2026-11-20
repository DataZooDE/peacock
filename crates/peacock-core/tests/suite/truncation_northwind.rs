//! Row truncation is surfaced, never silent (#46), against a **real** escurel.
//!
//! Two caps sit on the path: escurel's server row cap (10 000, it reports
//! `truncated` but no total) and peacock's own per-view `max_rows`. Either one
//! clipping a view must show up in `structuredContent.truncated`, and the
//! render must still succeed with the capped rows.

use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{
    NW_REPORT, NW_REPORT_OVERSIZED, NorthwindEscurel, NorthwindOpts, oversized_fixtures,
};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_rows_are_capped_and_truncation_is_surfaced() {
    let (extra_skills, extra_instances) = oversized_fixtures();
    let nw = NorthwindEscurel::spawn_with(NorthwindOpts {
        extra_skills,
        extra_instances,
        ..Default::default()
    })
    .await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    // 1. escurel's server cap clips the 104 976-row view to 10 000 rows.
    let art = render(
        NW_REPORT_OVERSIZED,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("an escurel-clipped view still renders");
    let t = art
        .structured_content
        .truncated
        .clone()
        .expect("escurel's clip is surfaced");
    assert_eq!(t.rows_shown, 10_000);
    assert_eq!(t.rows_total, None, "escurel reports no total");
    assert_eq!(t.limit, 10_000);
    assert_eq!(t.view, "lines");
    assert_eq!(
        art.structured_content.rows.as_array().unwrap().len(),
        10_000
    );

    // 2. peacock's own cap, below escurel's: capped, and the total is known.
    let opts = RenderOpts {
        max_rows: 10,
        ..Default::default()
    };
    let art = render(NW_REPORT, &json!({}), &principal, &escurel, &opts)
        .await
        .expect("a view over max_rows is capped, not refused");
    let t = art
        .structured_content
        .truncated
        .clone()
        .expect("peacock's cap is surfaced");
    assert_eq!((t.rows_shown, t.rows_total, t.limit), (10, Some(16), 10));
    assert_eq!(t.view, "rev_by_cat");
    let comps = art.a2ui["components"].as_array().unwrap();
    let chart = comps.iter().find(|c| c["kind"] == "anofox").unwrap();
    assert_eq!(chart["spec"]["rows"].as_array().unwrap().len(), 10);

    // 3. The same report under the default cap is whole and says nothing.
    let art = render(
        NW_REPORT,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .unwrap();
    assert!(art.structured_content.truncated.is_none());

    nw.shutdown().await;
}
