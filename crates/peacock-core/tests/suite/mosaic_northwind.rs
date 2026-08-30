//! No-mock Mosaic-mode render against a **real** escurel (BRD §7 deferred
//! "Mosaic/vgplot live big-data cross-filter", NFR-P-3). When a view's row
//! count exceeds `RenderOpts.mosaic_threshold`, peacock emits a Mosaic-mode
//! artifact that references the escurel query as the data SOURCE (the single
//! allow-listed non-inline source) rather than inlining the big rows. The
//! Flutter/web Mosaic client runtime that consumes it is out of scope here.

use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{NW_QUERY_LINES, NW_REPORT_DISCOUNT, NorthwindEscurel};
use serde_json::json;

#[tokio::test]
async fn oversize_view_renders_in_mosaic_mode_referencing_the_escurel_query() {
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    // The discount report reads raw order lines (one row per order detail) —
    // well above a deliberately tiny threshold. With the threshold set, the
    // chart view must switch to Mosaic mode instead of inlining the rows.
    let opts = RenderOpts {
        mosaic_threshold: Some(5),
        ..Default::default()
    };
    let art = render(NW_REPORT_DISCOUNT, &json!({}), &principal, &escurel, &opts)
        .await
        .expect("render the discount report in mosaic mode");

    let comps = art.a2ui["components"].as_array().unwrap();

    // (a) the artifact carries a Mosaic component referencing the escurel
    //     query_ref and the bound params — NOT inlined rows.
    let mosaic = comps
        .iter()
        .find(|c| c["kind"] == "mosaic")
        .expect("a mosaic component is present for the oversize chart view");
    let source = &mosaic["artifact"]["source"];
    assert_eq!(source["connector"], "escurel");
    assert_eq!(source["query_ref"], NW_QUERY_LINES);
    // The bound (defaulted) params travel with the source ref, not SQL.
    assert_eq!(source["params"]["from"], "1997-01-01");
    assert_eq!(source["params"]["to"], "1997-12-31");

    // The Mosaic spec carries the vgplot mark + encodings derived from the
    // report's Vega-Lite spec (a scatter/point over discount vs. revenue).
    let spec = &mosaic["artifact"]["spec"];
    assert_eq!(spec["mark"], "point");
    assert!(spec["encoding"]["x"]["field"].is_string());

    // (b) the big rows are NOT inlined anywhere in the Mosaic component.
    assert!(
        mosaic["artifact"]["spec"]["data"].get("values").is_none(),
        "mosaic spec must not inline the big rows"
    );
    assert!(
        mosaic.get("rows").is_none(),
        "mosaic component must not carry inlined rows"
    );

    // (c) the guardrail passed — an escurel-owned source reference is the one
    //     allow-listed non-inline source; the render did not error.

    // The structuredContent summary is preserved (current params + a row-count
    // summary) but the big rows are not inlined into it.
    assert!(art.structured_content.rows.as_array().unwrap().is_empty());
    assert_eq!(art.structured_content.current_params["from"], "1997-01-01");

    nw.shutdown().await;
}

#[tokio::test]
async fn no_threshold_still_renders_the_normal_inline_chart() {
    // (d) with mosaic_threshold = None the same report renders the normal
    //     inline-data chart (the default model is unchanged).
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();

    let art = render(
        NW_REPORT_DISCOUNT,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts::default(),
    )
    .await
    .expect("render the discount report inline");

    let comps = art.a2ui["components"].as_array().unwrap();
    assert!(
        comps.iter().all(|c| c["kind"] != "mosaic"),
        "no mosaic component without a threshold"
    );
    let vega = comps
        .iter()
        .find(|c| c["kind"] == "vega")
        .expect("the normal inline chart is present");
    assert!(
        vega["spec"]["data"]["values"].as_array().unwrap().len() > 5,
        "rows are inlined in the normal chart"
    );

    nw.shutdown().await;
}
