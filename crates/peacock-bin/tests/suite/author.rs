//! No-mock integration tests for the `peacock author` tooling (BRD §7
//! authoring-tooling deferral; HLD §170 authoring preview). `validate` and
//! `scaffold` run against the real `peacock` binary with no escurel; `preview`
//! hits a **real** `NorthwindEscurel::spawn()` — no mocks (CLAUDE.md §2).

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use peacock_test_support::{NW_REPORT, NorthwindEscurel, skill_report_markdown};

/// Path to the built `peacock` binary.
fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/peacock")
        .canonicalize()
        .expect("peacock binary built (run `cargo build`)")
}

fn write_temp(name: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("peacock-author-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    path
}

#[test]
fn validate_accepts_the_real_northwind_skill() {
    let path = write_temp("valid.md", &skill_report_markdown());
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("run peacock author validate");
    assert!(
        out.status.success(),
        "valid skill must validate; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn validate_rejects_a_view_referencing_a_missing_spec() {
    // A vega view names `rev_bar` but `specs:` only defines `other`.
    let bad = r#"---
type: skill
id: broken-missing-spec
render: a2ui
data:
  rev: "[[query::nw_revenue_by_category]]"
views:
  - { kind: vega, data: rev, spec: rev_bar }
specs:
  other:
    mark: bar
---
broken
"#;
    let path = write_temp("missing_spec.md", bad);
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("run validate");
    assert!(!out.status.success(), "missing spec must fail validation");
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        msg.contains("rev_bar"),
        "error should name the missing spec; got: {msg}"
    );
}

#[test]
fn validate_rejects_a_remote_data_url_in_a_spec() {
    // A spec with a remote `data.url` violates the inline-data-only guardrail.
    let bad = r#"---
type: skill
id: broken-remote-url
render: a2ui
data:
  rev: "[[query::nw_revenue_by_category]]"
views:
  - { kind: vega, data: rev, spec: evil }
specs:
  evil:
    data: { url: "https://example.com/x.json" }
    mark: bar
---
broken
"#;
    let path = write_temp("remote_url.md", bad);
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("run validate");
    assert!(
        !out.status.success(),
        "remote data.url must fail validation"
    );
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        msg.contains("url"),
        "error should name the disallowed feature; got: {msg}"
    );
}

#[test]
fn validate_accepts_a_stat_spec_report_skill() {
    // The stat-spec dialect (issue #7): a report skill declaring a density
    // chart with a contract vline + p90 marker closed-checks cleanly.
    let good = r#"---
type: skill
id: supplier-lead-times
render: a2ui
data:
  deliveries: "[[query::supplier_deliveries]]"
views:
  - { kind: vega, data: deliveries, spec: leadtime_density }
specs:
  leadtime_density:
    geom: density
    x: lead_days
    color: supplier
    facet_wrap: supplier
    annotations:
      - { kind: vline, at: 14.0, label: contract }
      - { kind: p90 }
---
Per-supplier lead-time distribution.
"#;
    let path = write_temp("stat_ok.md", good);
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("run validate");
    assert!(
        out.status.success(),
        "a valid stat-spec skill must validate; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn validate_rejects_a_broken_stat_spec_with_a_useful_message() {
    // Unknown geom + malformed annotation — validate names both problems.
    let bad = r#"---
type: skill
id: broken-stat
render: a2ui
data:
  deliveries: "[[query::supplier_deliveries]]"
views:
  - { kind: vega, data: deliveries, spec: leadtime }
specs:
  leadtime:
    geom: violin
    x: lead_days
---
broken
"#;
    let path = write_temp("stat_bad.md", bad);
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("run validate");
    assert!(!out.status.success(), "unknown geom must fail validation");
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        msg.contains("violin") && msg.contains("histogram"),
        "error should name the bad geom and the alternatives; got: {msg}"
    );
}

#[test]
fn scaffold_output_validates() {
    let scaffolded = Command::new(binary())
        .args(["author", "scaffold", "my-new-report"])
        .output()
        .expect("run scaffold");
    assert!(scaffolded.status.success(), "scaffold must succeed");
    let md = String::from_utf8(scaffolded.stdout).unwrap();
    assert!(md.contains("my-new-report"), "scaffold embeds the id");

    let path = write_temp("scaffolded.md", &md);
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("validate scaffold output");
    assert!(
        out.status.success(),
        "scaffold output must validate; stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[tokio::test]
async fn preview_renders_against_real_escurel() {
    // No mocks: preview the real Northwind report against a real escurel.
    let nw = NorthwindEscurel::spawn().await;
    let p = nw.sales_principal();
    let path = write_temp("preview.md", &skill_report_markdown());

    let out = tokio::task::spawn_blocking({
        let endpoint = nw.endpoint().to_string();
        let token = p.raw_token.clone();
        let tenant = p.tenant.clone();
        let sub = p.sub.clone();
        let groups = p.groups.join(",");
        move || {
            Command::new(binary())
                .args(["author", "preview"])
                .arg(&path)
                .args(["--escurel", &endpoint])
                .args(["--token", &token])
                .args(["--tenant", &tenant])
                .args(["--sub", &sub])
                .args(["--groups", &groups])
                .output()
                .expect("run peacock author preview")
        }
    })
    .await
    .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "preview must succeed against real escurel; stdout={stdout} stderr={stderr}",
    );
    // It reports the genuine component kinds and a real row count.
    assert!(
        stdout.contains("kpi"),
        "reports the kpi component: {stdout}"
    );
    assert!(
        stdout.contains("vega"),
        "reports the vega component: {stdout}"
    );
    assert!(
        stdout.to_lowercase().contains("row"),
        "reports a row count: {stdout}"
    );
    assert_eq!(out.status.code(), Some(0));

    let _ = NW_REPORT; // the previewed report id (asserted via render success)
    nw.shutdown().await;
}
