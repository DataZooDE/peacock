//! The `ggplot` feature OFF (the base tree, issue #6): composition of a
//! STATISTICAL spec is backend-independent — the artifact still carries
//! `stat_specs` + inline rows + structuredContent — but asking for a PNG is a
//! clear error naming the missing feature, never a silent skip. No mocks: a
//! real escurel serves the report skill and the real Parquet-backed rows.

#![cfg(not(feature = "ggplot"))]

use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{
    NW_REPORT_DISTRIBUTION, NorthwindEscurel, NorthwindOpts, skill_report_distribution,
};
use serde_json::json;

async fn spawn_with_distribution_report() -> NorthwindEscurel {
    NorthwindEscurel::spawn_with(NorthwindOpts {
        extra_skills: vec![(
            NW_REPORT_DISTRIBUTION.to_owned(),
            skill_report_distribution(),
        )],
        ..Default::default()
    })
    .await
}

#[tokio::test]
async fn stat_report_composes_without_the_feature_but_png_errors_clearly() {
    let nw = spawn_with_distribution_report().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    // Without rasterization the render succeeds: structuredContent and the
    // inline-rows stat spec are backend-independent by construction.
    let art = render(
        NW_REPORT_DISTRIBUTION,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("a stat report composes with the feature off");
    assert_eq!(art.stat_specs.len(), 1);
    assert!(art.stat_specs[0]["data"]["values"].is_array());
    assert!(art.png.is_none());

    // Asking for the PNG is a CLEAR error naming the feature — not a skip.
    let err = render(
        NW_REPORT_DISTRIBUTION,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts {
            png_scale: Some(2.0),
            ..Default::default()
        },
    )
    .await
    .expect_err("png of a stat spec without the ggplot feature must error");
    assert!(
        err.to_string().contains("ggplot"),
        "the error names the missing feature: {err}"
    );

    nw.shutdown().await;
}
