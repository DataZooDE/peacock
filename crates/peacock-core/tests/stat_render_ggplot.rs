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
async fn stat_report_renders_a_density_chart_with_contract_and_p90_markers() {
    // Issue #7 acceptance: a report skill declares a density chart with a
    // contract `vline` + `p90` marker — authored entirely in escurel
    // markdown — and peacock parses it into the ggplot backend's plot with
    // no Rust changes per-chart.
    let density_skill = r#"---
type: skill
id: northwind-revenue-density
render: a2ui
description: Density of Northwind order-line revenue with contract + p90 markers.
params:
  from: { type: date, default: "1997-01-01" }
  to:   { type: date, default: "1997-12-31" }
data:
  line_values: "[[query::nw_order_line_values]]"
views:
  - { kind: vega, data: line_values, spec: revenue_density }
specs:
  revenue_density:
    geom: density
    x: revenue
    title: Order-line revenue density
    annotations:
      - { kind: vline, at: 500.0, label: contract }
      - { kind: p90 }
---
Density of individual order-line revenue; EMEA orders only.
"#;
    let nw = NorthwindEscurel::spawn_with(NorthwindOpts {
        extra_skills: vec![(
            "northwind-revenue-density".to_owned(),
            density_skill.to_owned(),
        )],
        ..Default::default()
    })
    .await;
    let escurel = EscurelData::new(nw.endpoint());
    let opts = RenderOpts {
        png_scale: Some(1.0),
        ..Default::default()
    };

    let art = render(
        "northwind-revenue-density",
        &json!({}),
        &nw.sales_principal(),
        &escurel,
        &opts,
    )
    .await
    .expect("render the density report with annotations");

    assert_eq!(art.stat_specs.len(), 1);
    assert_eq!(art.stat_specs[0]["geom"], "density");
    assert_eq!(
        art.stat_specs[0]["annotations"].as_array().unwrap().len(),
        2,
        "the composed spec carries the contract vline and the p90 marker"
    );

    let png = art.png.expect("artifact carries the density PNG");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(png.len() > 1000);

    nw.shutdown().await;
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
