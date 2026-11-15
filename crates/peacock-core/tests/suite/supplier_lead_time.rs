//! The statistical-chart ACCEPTANCE render: the supplier lead-time
//! distribution — an anofox DENSITY over granular `supplier_deliveries` rows
//! with the contracted SLA as an `XLINE` — renders END-TO-END through a real
//! escurel (parquet_dir sql_view → query page → report skill, all authored
//! markdown), the compose path and anofox, themed with a brand CSS. No mocks.
//! This is the proof the datazoo-agent-template's supplier-reliability report
//! page (template #56) renders.

use peacock_core::{EscurelData, RenderOpts, render};
use peacock_test_support::{DeliveriesEscurel, SD_REPORT};
use peacock_theme::ThemeRegistry;
use serde_json::json;

#[tokio::test]
async fn supplier_lead_time_distribution_renders_in_brand() {
    let world = DeliveriesEscurel::spawn().await;
    let escurel = EscurelData::new(world.endpoint());
    let principal = world.logistics_principal();

    // The deployment brand: a real brand CSS composed through peacock-theme
    // (the same `--pk-*` one-source path the server surfaces use).
    let brand = ThemeRegistry::builtin().resolve("company-a", "copilot");
    let branded_opts = RenderOpts {
        png_scale: Some(2.0),
        theme: Some(brand.tokens.clone()),
        ..Default::default()
    };

    let art = render(SD_REPORT, &json!({}), &principal, &escurel, &branded_opts)
        .await
        .expect("the lead-time distribution renders end-to-end");

    // The composed artifact carries the anofox panel: the density over all 36
    // deliveries plus the SLA reference line. A density has no minimal
    // Vega-Lite form, so no derived spec (Triton shows the PNG).
    assert_eq!(art.anofox_specs.len(), 1);
    assert!(art.vega_specs.is_empty());
    let spec = &art.anofox_specs[0];
    assert_eq!(spec["roles"][0][1], "DENSITY");
    assert_eq!(spec["roles"][1][1], "XLINE");
    assert_eq!(spec["rows"].as_array().unwrap().len(), 36);
    assert_eq!(spec["rows"][0]["c1"], 14.0);

    // A real branded PNG.
    let branded = art.png.expect("the artifact carries the density PNG");
    assert_eq!(&branded[..8], b"\x89PNG\r\n\x1a\n");
    assert!(branded.len() > 1000);

    // Same brand ⇒ same bytes (statelessness / reproducibility, ADR-P7).
    let again = render(SD_REPORT, &json!({}), &principal, &escurel, &branded_opts)
        .await
        .unwrap()
        .png
        .unwrap();
    assert_eq!(branded, again, "one brand always renders the same bytes");

    // Different corporate identity ⇒ different bytes (the brand actually
    // paints the chart — mirrors http_surface's two-brand assertion).
    let other = ThemeRegistry::builtin().resolve("company-b", "gemini");
    let other_png = render(
        SD_REPORT,
        &json!({}),
        &principal,
        &escurel,
        &RenderOpts {
            png_scale: Some(2.0),
            theme: Some(other.tokens.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .png
    .unwrap();
    assert_ne!(
        branded, other_png,
        "two corporate identities render different charts"
    );

    // The params bind server-side: a narrower window drops rows (prepared
    // statements, not string interpolation).
    let q1 = render(
        SD_REPORT,
        &json!({ "from": "1997-01-01", "to": "1997-03-31" }),
        &principal,
        &escurel,
        &branded_opts,
    )
    .await
    .expect("a drilled window renders");
    assert_eq!(q1.anofox_specs[0]["rows"].as_array().unwrap().len(), 9);
    assert_ne!(
        q1.png.unwrap(),
        branded,
        "the drilled window renders a different chart"
    );

    world.shutdown().await;
}
