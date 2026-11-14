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
    // A chart view names `rev_bar` but `specs:` only defines `other`.
    let bad = r#"---
kind: skill
id: broken-missing-spec
render: a2ui
data:
  rev: "[[query::nw_revenue_by_category]]"
views:
  - { kind: anofox, data: rev, spec: rev_bar }
specs:
  other:
    roles: [[month, XAXIS], [revenue, BARCHART]]
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
    // A spec that brings its own `url` violates the inline-data-only guardrail.
    let bad = r#"---
kind: skill
id: broken-remote-url
render: a2ui
data:
  rev: "[[query::nw_revenue_by_category]]"
views:
  - { kind: anofox, data: rev, spec: evil }
specs:
  evil:
    url: "https://example.com/x.json"
    roles: [[month, XAXIS], [revenue, BARCHART]]
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
fn validate_accepts_a_statistical_anofox_spec() {
    // A density with a contract reference line — the statistical chart the
    // removed `geom:` dialect used to express.
    let good = r#"---
kind: skill
id: supplier-lead-times
render: a2ui
data:
  deliveries: "[[query::supplier_deliveries]]"
views:
  - { kind: anofox, data: deliveries, spec: leadtime_density }
specs:
  leadtime_density:
    title: Lead times vs contract
    roles:
      - [lead_days, DENSITY, "Lead time (days)"]
      - [sla_days, XLINE, contract]
---
Per-supplier lead-time distribution.
"#;
    let path = write_temp("density_ok.md", good);
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("run validate");
    assert!(
        out.status.success(),
        "a valid anofox density skill must validate; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn validate_rejects_a_removed_kind_vega_view_with_the_migration() {
    let bad = r#"---
kind: skill
id: old-report
render: a2ui
data:
  rev: "[[query::nw_revenue_by_category]]"
views:
  - { kind: vega, data: rev, spec: rev_bar }
specs:
  rev_bar: { mark: bar }
---
old
"#;
    let path = write_temp("old_vega.md", bad);
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("run validate");
    assert!(!out.status.success(), "kind: vega must fail validation");
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        msg.contains("kind: anofox"),
        "error should name the migration; got: {msg}"
    );
}

#[test]
fn validate_rejects_an_unknown_anofox_role() {
    let bad = r#"---
kind: skill
id: broken-role
render: a2ui
data:
  rev: "[[query::nw_revenue_by_category]]"
views:
  - { kind: anofox, data: rev, spec: c }
specs:
  c: { roles: [[month, XAXIS], [revenue, VIOLINCHART]] }
---
broken
"#;
    let path = write_temp("role_bad.md", bad);
    let out = Command::new(binary())
        .args(["author", "validate"])
        .arg(&path)
        .output()
        .expect("run validate");
    assert!(!out.status.success(), "unknown role must fail validation");
    let msg =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(msg.contains("VIOLINCHART"), "got: {msg}");
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
        stdout.contains("anofox"),
        "reports the anofox chart component: {stdout}"
    );
    assert!(
        stdout.to_lowercase().contains("row"),
        "reports a row count: {stdout}"
    );
    assert_eq!(out.status.code(), Some(0));

    let _ = NW_REPORT; // the previewed report id (asserted via render success)
    nw.shutdown().await;
}

#[tokio::test]
async fn scaffold_output_is_accepted_by_the_real_engine_as_a_kind_page() {
    // The scaffold is the template every new report starts from. escurel's
    // hard cut refuses `kind: skill` (`frontmatter_type_removed`); ask the
    // REAL engine (no mock) whether the scaffold's frontmatter is acceptable.
    let scaffolded = Command::new(binary())
        .args(["author", "scaffold", "my-new-report"])
        .output()
        .expect("run scaffold");
    assert!(scaffolded.status.success(), "scaffold must succeed");
    let md = String::from_utf8(scaffolded.stdout).unwrap();

    let nw = NorthwindEscurel::spawn().await;
    let client = nw.sales_client().await;
    let verdict = client
        .validate(escurel_client::ValidateRequest {
            content: md.clone(),
            as_page_id: "markdown/skills/my-new-report.md".to_owned(),
        })
        .await
        .expect("real validate call");
    nw.shutdown().await;

    let kind_problems: Vec<_> = verdict
        .issues
        .iter()
        .filter(|i| {
            i.location == "frontmatter.type"
                || i.code == "frontmatter_type_removed"
                || i.code == "frontmatter_kind_missing"
        })
        .collect();
    assert!(
        kind_problems.is_empty(),
        "the real engine refused the scaffold's page kind: {kind_problems:?}\n--- scaffold ---\n{md}"
    );
}

// ── `peacock author compile` (issue #44, Phase 3) ──────────────────────────

const NW_DASH: &str = "nw-dashboard";

/// Run `peacock author compile` on `script`; return (exit ok, stdout+stderr,
/// out dir).
fn compile(script: &str, tag: &str) -> (bool, String, PathBuf) {
    let src = write_temp(&format!("{tag}.sql"), script);
    let out_dir =
        std::env::temp_dir().join(format!("peacock-compile-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = Command::new(binary())
        .args(["author", "compile"])
        .arg(&src)
        .args(["--report", NW_DASH])
        .args(["--source", "order_lines"])
        .args(["--target", peacock_test_support::NW_DASHBOARD_TARGET])
        .args(["--param", "from:date=1997-01-01"])
        .args(["--param", "to:date=1997-12-31"])
        .arg("--out")
        .arg(&out_dir)
        .output()
        .expect("run peacock author compile");
    let msg =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), msg, out_dir)
}

/// The emitted report skill + query pages, ready to seed into escurel.
fn compiled_pages(out_dir: &std::path::Path) -> (String, Vec<(String, String)>) {
    let report = std::fs::read_to_string(out_dir.join(format!("{NW_DASH}.md"))).unwrap();
    let mut queries: Vec<(String, String)> = std::fs::read_dir(out_dir.join("query"))
        .unwrap()
        .flatten()
        .map(|e| {
            let id = e.path().file_stem().unwrap().to_string_lossy().to_string();
            (id, std::fs::read_to_string(e.path()).unwrap())
        })
        .collect();
    queries.sort();
    (report, queries)
}

#[test]
fn compile_emits_one_query_page_per_statement() {
    let (ok, msg, out_dir) = compile(&peacock_test_support::northwind_dashboard_sql(), "ok");
    assert!(ok, "compile must succeed: {msg}");
    let (report, queries) = compiled_pages(&out_dir);
    let ids: Vec<&str> = queries.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "nw-dashboard-q1",
            "nw-dashboard-q2",
            "nw-dashboard-q3",
            "nw-dashboard-q4"
        ]
    );
    for (id, md) in &queries {
        assert!(md.contains("skill: query"), "{id}: a query page");
        assert!(
            md.contains("target: \"[[nw_order_lines::eu]]\""),
            "{id}: {md}"
        );
        assert!(
            !md.contains("getvariable"),
            "{id}: params bound by escurel, not read from session state: {md}"
        );
        assert!(
            !md.contains("order_lines WHERE") && !md.contains("FROM order_lines"),
            "{id}: source table → {{{{target}}}}: {md}"
        );
    }
    let kpi = &queries[1].1;
    assert!(
        kpi.contains("FROM {{target}}") && kpi.contains(":from") && kpi.contains(":to"),
        "{kpi}"
    );
    assert!(
        kpi.contains("{name: from, type: date"),
        "the page declares only the params it binds: {kpi}"
    );
    // The heading binds nothing.
    assert!(!queries[0].1.contains("name: from"), "{}", queries[0].1);

    // The report skill: the params, the script and the compiled manifest.
    assert!(report.contains("id: nw-dashboard"), "{report}");
    assert!(report.contains("dashboard:"), "{report}");
    assert!(report.contains("[[query::nw-dashboard-q3]]"), "{report}");
    assert!(report.contains("1997-01-01"), "param default: {report}");
    let errs = peacock_core::validate_skill_markdown(&report);
    assert!(errs.is_empty(), "the compiled report validates: {errs:?}");
}

#[test]
fn compile_rejects_a_layout_role_inside_a_data_statement() {
    // The spike's silent-drop slip: `8::COL` inside the chart SELECT makes
    // anofox treat the whole statement as layout and draw nothing — no error.
    // Compile is the hard gate that turns it into one.
    let slipped = peacock_test_support::northwind_dashboard_sql()
        .replace("AS \"Revenue\"::DONUT", "AS \"Revenue\"::DONUT, 8::COL");
    let (ok, msg, _) = compile(&slipped, "slip");
    assert!(
        !ok,
        "a layout role inside a data statement must fail compile"
    );
    assert!(
        msg.contains("inline-layout-role") && msg.contains("statement 4"),
        "{msg}"
    );
}

#[test]
fn compile_rejects_setup_and_non_select_statements() {
    for (bad, what) in [
        ("CREATE TABLE t AS SELECT 1 AS x;\n", "setup"),
        ("DELETE FROM order_lines;\n", "setup"),
        ("COPY order_lines TO 'x.csv';\n", "setup"),
    ] {
        let script = format!("{bad}{}", peacock_test_support::northwind_dashboard_sql());
        let (ok, msg, _) = compile(&script, "setup");
        assert!(!ok, "{bad} must fail compile");
        assert!(
            msg.contains(what) && msg.contains("statement 1"),
            "{bad}: {msg}"
        );
    }
}

/// J6: an author writes `dashboard.sql`, compiles it, publishes the pages to
/// escurel and opens the report. The WHOLE dashboard renders through the real
/// escurel over the real Parquet; escurel receives only query-page refs and
/// typed params — never a SQL string; a drill re-renders from new params.
#[tokio::test]
async fn compile_then_render_whole_dashboard() {
    let (ok, msg, out_dir) = compile(&peacock_test_support::northwind_dashboard_sql(), "j6");
    assert!(ok, "{msg}");
    let (report, queries) = compiled_pages(&out_dir);
    let nw = NorthwindEscurel::spawn_with(peacock_test_support::NorthwindOpts {
        extra_skills: vec![(NW_DASH.into(), report)],
        extra_instances: queries
            .into_iter()
            .map(|(id, md)| ("query".to_owned(), id, md))
            .collect(),
        ..Default::default()
    })
    .await;

    let escurel = peacock_core::EscurelData::new(nw.endpoint());
    let sink: peacock_core::TraceSink = Default::default();
    let art = peacock_core::render(
        NW_DASH,
        &serde_json::json!({}),
        &nw.sales_principal(),
        &escurel,
        &peacock_core::RenderOpts {
            png_scale: Some(1.0),
            trace: Some(sink.clone()),
            ..Default::default()
        },
    )
    .await
    .expect("the compiled dashboard renders");

    let comps = art.a2ui["components"].as_array().unwrap();
    let dash = comps
        .iter()
        .find(|c| c["kind"] == "dashboard")
        .expect("a dashboard component");
    let svg = dash["svg"].as_str().expect("the painted dashboard");
    assert!(svg.starts_with("<svg"));
    for text in [
        "Northwind revenue (EMEA)",
        "Beverages",
        "Seafood",
        "1997-01",
    ] {
        assert!(svg.contains(text), "the dashboard shows {text}");
    }
    // The KPI the analyst reads matches the fixture's 1997 total.
    assert!(
        svg.contains("2,986") || svg.contains("2.986") || svg.contains("2,99"),
        "KPI 2986"
    );
    assert_eq!(dash["panels"], 4, "every compiled statement drew");
    let png = art.png.expect("a PNG of the whole dashboard");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");

    // The trust boundary: what peacock SENT escurel is refs + typed params,
    // never SQL (the responses carry escurel's own pages, script included).
    let requests: Vec<serde_json::Value> = sink
        .lock()
        .unwrap()
        .iter()
        .map(|e| e["request"].clone())
        .collect();
    let wire = serde_json::to_string(&requests).unwrap();
    assert!(
        wire.contains("nw-dashboard-q3"),
        "the compiled query page was read"
    );
    for needle in ["SELECT", "getvariable", "order_lines WHERE"] {
        assert!(
            !wire.contains(needle),
            "peacock sent SQL to escurel ({needle}): {wire}"
        );
    }

    // Drill: Q1 only — a fresh render from absolute params.
    let q1 = peacock_core::render(
        NW_DASH,
        &serde_json::json!({ "from": "1997-01-01", "to": "1997-03-31" }),
        &nw.sales_principal(),
        &escurel,
        &peacock_core::RenderOpts::default(),
    )
    .await
    .expect("drilled render");
    let q1_svg = q1.a2ui["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "dashboard")
        .unwrap()["svg"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(q1_svg, svg, "a narrower window draws a different dashboard");
    assert_eq!(q1.structured_content.current_params["to"], "1997-03-31");

    nw.shutdown().await;
}

/// A compiled panel that anofox does not draw (the silent drop) fails the
/// render instead of shipping a dashboard with a hole in it.
#[tokio::test]
async fn render_fails_when_a_compiled_panel_is_not_drawn() {
    let (ok, msg, out_dir) = compile(&peacock_test_support::northwind_dashboard_sql(), "drop");
    assert!(ok, "{msg}");
    let (report, queries) = compiled_pages(&out_dir);
    // Simulate the drop: the published script loses the donut statement, the
    // manifest still expects it.
    let script_tail = "GROUP BY ALL ORDER BY 2 DESC;";
    let cut = report
        .find("SELECT category::CATEGORY")
        .expect("donut in the script");
    let end = report[cut..].find(script_tail).unwrap() + cut + script_tail.len();
    let tampered = format!("{}{}", &report[..cut], &report[end..]);
    let nw = NorthwindEscurel::spawn_with(peacock_test_support::NorthwindOpts {
        extra_skills: vec![(NW_DASH.into(), tampered)],
        extra_instances: queries
            .into_iter()
            .map(|(id, md)| ("query".to_owned(), id, md))
            .collect(),
        ..Default::default()
    })
    .await;
    let err = peacock_core::render(
        NW_DASH,
        &serde_json::json!({}),
        &nw.sales_principal(),
        &peacock_core::EscurelData::new(nw.endpoint()),
        &peacock_core::RenderOpts::default(),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("panel"), "{err}");
    nw.shutdown().await;
}
