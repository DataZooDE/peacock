//! Phase 7 lifecycle / observability against the **real** `peacock` binary +
//! **real** escurel (FR-L, FR-O, ACC-9/10). No mocks.

use std::collections::HashMap;

use peacock_test_support::{NW_REPORT, NorthwindEscurel, PeacockProcess};
use serde_json::Value;

fn env_for(nw: &NorthwindEscurel) -> HashMap<String, String> {
    let p = nw.sales_principal();
    HashMap::from([
        ("PEACOCK_ESCUREL_URL".into(), nw.endpoint().to_string()),
        ("PEACOCK_ESCUREL_TOKEN".into(), p.raw_token.clone()),
        ("PEACOCK_TENANT".into(), p.tenant.clone()),
        ("PEACOCK_SUB".into(), p.sub.clone()),
    ])
}

#[tokio::test]
async fn cold_start_healthz_version_and_render() {
    // ACC-9: a fresh process binds, passes /healthz, /version reports SHAs,
    // and renders the Northwind report end-to-end against real escurel.
    let nw = NorthwindEscurel::spawn().await;
    let peacock = PeacockProcess::spawn(env_for(&nw)).await;
    let http = reqwest::Client::new();

    let h = http
        .get(format!("{}/healthz", peacock.base_url()))
        .send()
        .await
        .unwrap();
    assert!(h.status().is_success());

    let v: Value = http
        .get(format!("{}/version", peacock.base_url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["name"], "peacock");
    assert!(v.get("bundle_sha").is_some());

    let body: Value = http
        .post(format!("{}/v1/render_report", peacock.base_url()))
        .json(&serde_json::json!({ "report_id": NW_REPORT }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["a2ui"]["version"], "0.9");

    // FR-L-2: SIGTERM drains and exits 0.
    let status = peacock.terminate();
    assert!(status.success(), "clean SIGTERM exit: {status:?}");
    nw.shutdown().await;
}

#[tokio::test]
async fn boot_refuses_on_unknown_manifest_enum() {
    // ACC-10: an unknown manifest component refuses boot (non-zero exit).
    let nw = NorthwindEscurel::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path().join("bad.toml");
    std::fs::write(
        &manifest,
        "[render]\npolicy = \"strict\"\n[components]\ncatalog = [\"kpi\", \"hologram\"]\n",
    )
    .unwrap();

    let mut env = env_for(&nw);
    env.insert(
        "PEACOCK_MANIFEST".into(),
        manifest.to_str().unwrap().to_string(),
    );

    match PeacockProcess::try_spawn(env).await {
        Err(status) => assert!(!status.success(), "boot must refuse: {status:?}"),
        Ok(_) => panic!("peacock booted despite an unknown manifest component"),
    }
    nw.shutdown().await;
}
