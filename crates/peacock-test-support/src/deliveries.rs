//! Seed a real escurel with the SUPPLIER-DELIVERIES world — the ggplot
//! statistical backend's acceptance scenario (issue #8): a lead-time
//! DENSITY with the contracted SLA as a `vline` and the computed `p90`
//! marker, over a committed deterministic Parquet fixture
//! (`fixtures/deliveries/`, regenerate with
//! `duckdb < fixtures/deliveries/gen.sql`). Mirrors [`crate::NorthwindEscurel`]:
//! a credential-free offline `parquet_dir` sql_view, a `query` page binding
//! `:from`/`:to` as prepared statements, and the report skill — all authored
//! in escurel markdown. This is the shape the datazoo-agent-template's
//! supplier-reliability report (template #56) renders through.

use std::path::PathBuf;

use escurel_test_support::{AuthMode, EscurelProcess, FixtureBuilder, Opts, Role};
use peacock_types::Principal;
use serde_json::json;

/// The tenant the deliveries fixture is seeded under.
pub const DELIVERIES_TENANT: &str = "acme";
/// The group the `supplier_deliveries` view requires for read.
pub const LOGISTICS_GROUP: &str = "logistics";
/// The query-instance ref peacock reads the lead-time rows from.
pub const SD_QUERY_REF: &str = "sd_lead_times";
/// The report skill id: the lead-time distribution vs the contracted SLA.
pub const SD_REPORT: &str = "supplier-lead-times";

/// Absolute path to the committed supplier-deliveries Parquet directory.
fn deliveries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/deliveries/supplier_deliveries")
        .canonicalize()
        .expect("deliveries fixture dir exists (run `duckdb < fixtures/deliveries/gen.sql`)")
}

/// The `query` meta-skill (so query instances validate).
fn skill_query() -> String {
    "---\ntype: skill\nid: query\ndescription: Reusable parameterised reads.\n---\n# query\n"
        .to_owned()
}

/// The `supplier_deliveries` sql_view skill, group-ACL `read: [logistics]`,
/// bound to the offline Parquet directory via the credential-free
/// `parquet_dir` connector.
fn skill_supplier_deliveries(relation: &str) -> String {
    format!(
        "---\n\
         type: skill\n\
         id: supplier_deliveries\n\
         description: Completed inbound deliveries with realised lead times, mirrored read-only from Parquet.\n\
         backend:\n  kind: sql_view\n  source: {{ connector: parquet_dir, relation: {relation} }}\n  search_text: [supplier]\n\
         acl: {{ read: [{LOGISTICS_GROUP}] }}\n\
         ---\n\
         # supplier_deliveries\n\
         One row per completed delivery: supplier, delivery date, actual lead-time days.\n"
    )
}

/// The `sd_lead_times` query page: the granular lead-time read,
/// `:from`/`:to` bound as prepared-statement params, reading the
/// `{{target}}` managed view.
fn query_lead_times() -> String {
    "---\n\
     type: instance\n\
     skill: query\n\
     id: sd_lead_times\n\
     target: \"[[supplier_deliveries::inbound]]\"\n\
     params:\n\
     \x20 - {name: from, type: date, required: true}\n\
     \x20 - {name: to, type: date, required: true}\n\
     sql: \"SELECT supplier AS supplier, actual_days::DOUBLE AS actual_days FROM {{target}} \
     WHERE delivered_on BETWEEN :from AND :to ORDER BY delivered_on, supplier\"\n\
     ---\n\
     # sd_lead_times\n\
     One row per delivery in the window: the supplier and the realised lead time in days.\n"
        .to_owned()
}

/// The `supplier-lead-times` report skill: the lead-time DENSITY with the
/// contracted 14-day SLA `vline` and the computed `p90` marker — authored
/// entirely in escurel markdown, rendered by the ggplot backend.
pub fn skill_report_lead_times() -> String {
    r#"---
type: skill
id: supplier-lead-times
render: a2ui
description: Distribution of realised supplier lead times vs the contracted SLA.
params:
  from: { type: date, default: "1997-01-01" }
  to:   { type: date, default: "1997-12-31" }
data:
  deliveries: "[[query::sd_lead_times]]"
views:
  - { kind: vega,  data: deliveries, spec: leadtime_density }
  - { kind: table, data: deliveries }
specs:
  leadtime_density:
    geom: density
    x: actual_days
    title: Supplier lead times vs contract
    annotations:
      - { kind: vline, at: 14.0, label: contract }
      - { kind: p90 }
---
Realised lead time per completed delivery, all suppliers. The dashed line is
the contracted 14-day SLA; the dotted line is the observed 90th percentile.
"#
    .to_owned()
}

/// A running real escurel seeded with the supplier-deliveries world.
pub struct DeliveriesEscurel {
    process: EscurelProcess,
}

impl DeliveriesEscurel {
    /// Spawn real escurel, seed the skills + query page, then materialise the
    /// `supplier_deliveries::inbound` sql_view instance over the Parquet
    /// directory (admin-gated `create_sql_instance`).
    pub async fn spawn() -> Self {
        let relation = deliveries_dir();
        let relation = relation.to_str().expect("utf-8 path");

        let fixtures = FixtureBuilder::new()
            .tenant(DELIVERIES_TENANT)
            .skill("query", skill_query())
            .skill("supplier_deliveries", skill_supplier_deliveries(relation))
            .skill(SD_REPORT, skill_report_lead_times())
            .instance("query", SD_QUERY_REF, query_lead_times())
            .done();

        let process = EscurelProcess::spawn(Opts {
            auth: AuthMode::TestIssuer,
            fixtures: Some(fixtures),
            config_overrides: Default::default(),
        })
        .await;

        let admin = process.client_for(DELIVERIES_TENANT, Role::Admin).await;
        admin
            .call_raw(
                "create_sql_instance",
                json!({ "skill": "supplier_deliveries", "id": "inbound" }),
            )
            .await
            .expect("materialise supplier_deliveries::inbound");

        Self { process }
    }

    /// The escurel base URL (the "escurel binding" the embedded face needs).
    pub fn endpoint(&self) -> &str {
        self.process.base_url()
    }

    /// A principal that may read the view (`logistics` group).
    pub fn logistics_principal(&self) -> Principal {
        Principal {
            sub: "planner-1".into(),
            scopes: vec![],
            groups: vec![LOGISTICS_GROUP.into()],
            tenant: DELIVERIES_TENANT.into(),
            raw_token: self.process.mint_token_with_groups(
                DELIVERIES_TENANT,
                "planner-1",
                &[LOGISTICS_GROUP],
                false,
            ),
            trace_id: "test-trace".into(),
        }
    }

    /// Graceful shutdown of the underlying escurel process.
    pub async fn shutdown(self) {
        self.process.shutdown().await;
    }
}
