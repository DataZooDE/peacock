//! Phase 4 no-mock end-to-end render: the full render core against a **real**
//! escurel rendering the **real** Northwind report skill (FR-R-1..3, FR-V-1/3,
//! FR-X-1). This is the paper's running example produced as one artifact.

use peacock_core::{EscurelData, RenderOpts, render, view_state_record};
use peacock_test_support::{NW_REPORT, NorthwindEscurel};
use serde_json::json;

#[tokio::test]
async fn renders_the_northwind_report_as_one_artifact() {
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    // Default view: peacock fills the report's declared defaults (full 1997).
    let art = render(
        NW_REPORT,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render the Northwind report");

    // A2UI v0.9 with kpi + vega + table + narrative text.
    assert_eq!(art.a2ui["version"], "0.9");
    let comps = art.a2ui["components"].as_array().unwrap();
    assert_eq!(comps.iter().filter(|c| c["kind"] == "kpi").count(), 1);
    assert_eq!(comps.iter().filter(|c| c["kind"] == "vega").count(), 1);
    assert_eq!(comps.iter().filter(|c| c["kind"] == "table").count(), 1);

    // KPI = total 1997 revenue across the real Parquet data.
    let kpi = comps.iter().find(|c| c["kind"] == "kpi").unwrap();
    assert_eq!(kpi["value"].as_f64().unwrap(), 2986.0);

    // The chart carries the escurel rows INLINE (no remote URL) — 16 groups.
    let vega = comps.iter().find(|c| c["kind"] == "vega").unwrap();
    assert_eq!(vega["spec"]["data"]["values"].as_array().unwrap().len(), 16);
    assert!(vega["spec"]["data"].get("url").is_none());

    // structuredContent carries rows + the resolved (defaulted) params (FR-X-1).
    assert_eq!(art.structured_content.rows.as_array().unwrap().len(), 16);
    assert_eq!(art.structured_content.current_params["from"], "1997-01-01");
    assert_eq!(art.structured_content.current_params["category"], "ALL");

    nw.shutdown().await;
}

#[tokio::test]
async fn renders_a_real_png_for_the_chat_surface() {
    // FR-C-2 / FR-V-2: with png_scale set, the artifact carries a real PNG of
    // the Northwind chart (pure-Rust rasterizer, no Node/Deno/network).
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let opts = peacock_core::RenderOpts {
        png_scale: Some(2.0),
        ..Default::default()
    };
    let art = render(
        NW_REPORT,
        &json!({}),
        &nw.sales_principal(),
        &escurel,
        &opts,
    )
    .await
    .expect("render with png");
    let png = art.png.expect("artifact has a PNG");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(png.len() > 1000);

    nw.shutdown().await;
}

#[tokio::test]
async fn a_drill_is_a_fresh_render_with_absolute_params() {
    // FR-M-3 / FR-X-2 parity at the core: a drill = the same render with new
    // absolute params; a committed drill yields the compact view-state record.
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let drilled = json!({ "category": "Beverages" }); // from/to default to 1997
    let art = render(
        NW_REPORT,
        &drilled,
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("drill render");

    let kpi = art.a2ui["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "kpi")
        .unwrap();
    assert_eq!(kpi["value"].as_f64().unwrap(), 801.0); // Beverages-only 1997

    // The compact record pushed to the model on commit carries params, NOT rows.
    let rec = view_state_record(NW_REPORT, &art, "Beverages, full year 1997");
    assert_eq!(rec["report_id"], NW_REPORT);
    assert_eq!(rec["params"]["category"], "Beverages");
    assert!(rec.get("rows").is_none());

    nw.shutdown().await;
}

#[tokio::test]
async fn stateless_re_render_reproduces_the_artifact() {
    // ACC-8 / FR-R-2: two identical calls reproduce the same artifact; a drill
    // then reverting the param returns to the original.
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let p = nw.sales_principal();
    let opts = RenderOpts::default();

    let a = render(NW_REPORT, &json!({}), &p, &escurel, &opts)
        .await
        .unwrap();
    let _drill = render(
        NW_REPORT,
        &json!({ "category": "Seafood" }),
        &p,
        &escurel,
        &opts,
    )
    .await
    .unwrap();
    let b = render(NW_REPORT, &json!({}), &p, &escurel, &opts)
        .await
        .unwrap();
    assert_eq!(
        a, b,
        "no server-side state: identical inputs reproduce the artifact"
    );

    nw.shutdown().await;
}
