//! Seed a real escurel with the paper's Northwind report.

use std::path::PathBuf;

use escurel_test_support::{AuthMode, EscurelProcess, FixtureBuilder, Opts, Role};
use peacock_types::Principal;
use secrecy::SecretString;
use serde_json::json;

/// Default tenant every fixture is seeded under.
pub const TENANT: &str = "acme";
/// The group the `nw_order_lines` view requires for read (ACL test).
pub const SALES_GROUP: &str = "sales";
/// The query-instance ref peacock reads the report's rows from.
pub const NW_QUERY_REF: &str = "nw_revenue_by_category";
/// The report skill id peacock renders (the paper's running example).
pub const NW_REPORT: &str = "northwind-monthly-revenue";

/// Absolute path to the committed Northwind order-lines Parquet directory.
fn order_lines_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/northwind/order_lines")
        .canonicalize()
        .expect("northwind fixture dir exists (run `duckdb < fixtures/northwind/gen.sql`)")
}

/// The `query` meta-skill (so query instances validate).
fn skill_query() -> String {
    "---\ntype: skill\nid: query\ndescription: Reusable parameterised reads.\n---\n# query\n"
        .to_owned()
}

/// The `nw_order_lines` sql_view skill, group-ACL `read: [sales]`, bound to
/// the offline Parquet directory via the credential-free `parquet_dir`
/// connector.
fn skill_order_lines(relation: &str) -> String {
    format!(
        "---\n\
         type: skill\n\
         id: nw_order_lines\n\
         description: Northwind order lines, mirrored read-only from Parquet.\n\
         backend:\n  kind: sql_view\n  source: {{ connector: parquet_dir, relation: {relation} }}\n\
         search_text: [category]\n\
         acl: {{ read: [{SALES_GROUP}] }}\n\
         ---\n\
         # nw_order_lines\n\
         EMEA Northwind order lines (one row per order detail), virtualized read-only.\n"
    )
}

/// The `nw_revenue_by_category` query page: the real revenue aggregation,
/// `:from`/`:to`/`:category` bound as prepared-statement params, reading the
/// `{{target}}` managed view.
fn query_revenue_by_category() -> String {
    "---\n\
     type: instance\n\
     skill: query\n\
     id: nw_revenue_by_category\n\
     target: \"[[nw_order_lines::eu]]\"\n\
     params:\n\
     \x20 - {name: from, type: date, required: true}\n\
     \x20 - {name: to, type: date, required: true}\n\
     \x20 - {name: category, type: text, required: true}\n\
     sql: \"SELECT date_trunc('month', order_date)::DATE AS month, category AS category, \
     sum(unit_price * quantity * (1 - discount))::DOUBLE AS revenue FROM {{target}} \
     WHERE order_date BETWEEN :from AND :to AND (:category = 'ALL' OR category = :category) \
     GROUP BY 1, 2 ORDER BY 1, 2\"\n\
     ---\n\
     # nw_revenue_by_category\n\
     Revenue is net of line discount; EMEA orders only.\n"
        .to_owned()
}

/// The `northwind-monthly-revenue` report skill (the paper's example):
/// render params, data bound by reference to the query page, the view layout,
/// and the named Vega-Lite chart spec.
fn skill_report() -> String {
    r#"---
type: skill
id: northwind-monthly-revenue
render: a2ui
description: Northwind monthly revenue by product category (EMEA).
params:
  from:     { type: date,   default: "1997-01-01" }
  to:       { type: date,   default: "1997-12-31" }
  category: { type: string, default: "ALL" }
data:
  rev_by_cat: "[[query::nw_revenue_by_category]]"
views:
  - { kind: kpi,   data: rev_by_cat, agg: sum, field: revenue, label: "Total revenue" }
  - { kind: vega,  data: rev_by_cat, spec: rev_bar, spec_single: rev_line }
  - { kind: table, data: rev_by_cat }
specs:
  rev_bar:
    mark: bar
    encoding:
      x:     { field: month,    type: ordinal,      title: Month }
      y:     { field: revenue,  type: quantitative, aggregate: sum, title: Revenue }
      color: { field: category, type: nominal }
  rev_line:
    mark: line
    encoding:
      x:     { field: month,    type: temporal,     title: Month }
      y:     { field: revenue,  type: quantitative, aggregate: sum, title: Revenue }
      color: { field: category, type: nominal }
---
Revenue is recognised at order date, net of line discount. EMEA orders only.
"#
    .to_owned()
}

/// A running real escurel seeded with the Northwind report.
pub struct NorthwindEscurel {
    process: EscurelProcess,
}

impl NorthwindEscurel {
    /// Spawn real escurel, seed the skills + query page, then materialise the
    /// `nw_order_lines::eu` sql_view instance over the Parquet directory
    /// (admin-gated `create_sql_instance`).
    pub async fn spawn() -> Self {
        let relation = order_lines_dir();
        let relation = relation.to_str().expect("utf-8 path");

        let fixtures = FixtureBuilder::new()
            .tenant(TENANT)
            .skill("query", skill_query())
            .skill("nw_order_lines", skill_order_lines(relation))
            .skill(NW_REPORT, skill_report())
            .instance("query", NW_QUERY_REF, query_revenue_by_category())
            .done();

        let process = EscurelProcess::spawn(Opts {
            auth: AuthMode::TestIssuer,
            fixtures: Some(fixtures),
            ..Default::default()
        })
        .await;

        // Materialise the sql_view instance (admin) — this CREATE-VIEWs the
        // managed `vw_…` over `read_parquet(...)` that `query_instance` reads.
        let admin = process.client_for(TENANT, Role::Admin).await;
        admin
            .call_raw(
                "create_sql_instance",
                json!({ "skill": "nw_order_lines", "id": "eu" }),
            )
            .await
            .expect("materialise nw_order_lines::eu");

        Self { process }
    }

    /// The escurel base URL (the "escurel binding" the embedded face needs).
    pub fn endpoint(&self) -> &str {
        self.process.base_url()
    }

    /// An `escurel-client` already bearing a `sales`-group token (cheap to
    /// build), for assertions that bypass peacock's reader.
    pub async fn sales_client(&self) -> escurel_client::Client {
        escurel_client::Client::connect(self.endpoint(), SecretString::from(self.sales_token()))
            .await
            .expect("connect sales client")
    }

    fn sales_token(&self) -> String {
        self.process
            .mint_token_with_groups(TENANT, "analyst-1", &[SALES_GROUP], false)
    }

    fn no_sales_token(&self) -> String {
        self.process
            .mint_token_with_groups(TENANT, "outsider-1", &[], false)
    }

    /// A principal that **may** read the view (`sales` group).
    pub fn sales_principal(&self) -> Principal {
        Principal {
            sub: "analyst-1".into(),
            scopes: vec![],
            groups: vec![SALES_GROUP.into()],
            tenant: TENANT.into(),
            raw_token: self.sales_token(),
            trace_id: "test-trace".into(),
        }
    }

    /// A principal that may **not** read the view (no `sales` group) — drives
    /// the fail-closed ACL test (ACC-3).
    pub fn no_sales_principal(&self) -> Principal {
        Principal {
            sub: "outsider-1".into(),
            scopes: vec![],
            groups: vec![],
            tenant: TENANT.into(),
            raw_token: self.no_sales_token(),
            trace_id: "test-trace".into(),
        }
    }

    /// Graceful shutdown of the underlying escurel process.
    pub async fn shutdown(self) {
        self.process.shutdown().await;
    }
}
