//! No-mock end-to-end saved/shared report instances (BRD §7) against a
//! **real** escurel. peacock stays stateless: the bookmark lives in escurel,
//! written via `escurel-client` `update_page` and read back via
//! `resolve`+`expand` — the caller principal is forwarded so escurel's
//! fail-closed owner ACL gates both. A saved render must reproduce a direct
//! `render(report, params)` artifact (FR-R-1, FR-X reproducibility).

use peacock_core::{
    EscurelData, RenderOpts, render, render_saved, resolve_saved_instance, save_instance,
};
use peacock_test_support::{NW_REPORT, NorthwindEscurel};
use serde_json::{Value, json};

/// The absolute parameter vector the bookmark pins: Beverages, full-year 1997.
fn beverages_1997() -> Value {
    json!({ "from": "1997-01-01", "to": "1997-12-31", "category": "Beverages" })
}

#[tokio::test]
async fn saved_render_reproduces_a_direct_render() {
    let nw = NorthwindEscurel::spawn().await;
    let escurel = EscurelData::new(nw.endpoint());
    let principal = nw.sales_principal();
    let opts = RenderOpts::default();

    let params = beverages_1997();

    // Persist the parameterized render as an escurel saved instance (the real
    // `update_page` write path, forwarding the caller principal).
    let saved = save_instance(
        &escurel,
        &principal,
        "nw-1997-beverages",
        NW_REPORT,
        &params,
    )
    .await
    .expect("save the parameterized render as an escurel instance");

    // Read it back: exactly the (report_id, params) we saved.
    let (report_id, read_params) = resolve_saved_instance(&escurel, &principal, &saved)
        .await
        .expect("resolve the saved instance");
    assert_eq!(report_id, NW_REPORT);
    assert_eq!(read_params, params);

    // A saved render funnels through the one render core → byte-reproducible
    // against a direct render of the same (report, params).
    let from_saved = render_saved(&escurel, &principal, &saved, &opts)
        .await
        .expect("render from the saved instance");
    let direct = render(NW_REPORT, &params, &principal, &escurel, &opts)
        .await
        .expect("direct render of the same (report, params)");

    assert_eq!(
        from_saved, direct,
        "a saved render reproduces a direct render byte-for-byte"
    );

    // And it really rendered the Beverages-only slice (KPI = 801 for 1997).
    let kpi = from_saved.a2ui["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "kpi")
        .unwrap();
    assert_eq!(kpi["value"].as_f64().unwrap(), 801.0);
    assert_eq!(
        from_saved.structured_content.current_params["category"],
        "Beverages"
    );

    nw.shutdown().await;
}
