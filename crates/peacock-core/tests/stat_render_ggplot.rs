//! The `ggplot` backend end-to-end (issue #6): with the feature ON, the
//! render core dispatches a STATISTICAL spec (top-level `geom`) to
//! `peacock-ggplot` and the artifact carries a real histogram PNG. No mocks:
//! a real escurel serves the report skill and the real Parquet-backed rows.

#![cfg(feature = "ggplot")]

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
async fn stat_report_renders_a_histogram_png_via_the_ggplot_backend() {
    let nw = spawn_with_distribution_report().await;
    let escurel = EscurelData::new(nw.endpoint());
    let opts = RenderOpts {
        png_scale: Some(2.0),
        ..Default::default()
    };

    let art = render(
        NW_REPORT_DISTRIBUTION,
        &json!({}),
        &nw.sales_principal(),
        &escurel,
        &opts,
    )
    .await
    .expect("render the distribution report with a PNG");

    // The selector routed by spec type: statistical, not Vega.
    assert_eq!(art.stat_specs.len(), 1);
    assert!(art.vega_specs.is_empty());
    assert_eq!(art.stat_specs[0]["geom"], "histogram");

    // A real PNG from the ggplot backend.
    let png = art.png.expect("artifact carries the histogram PNG");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(png.len() > 1000);

    nw.shutdown().await;
}
