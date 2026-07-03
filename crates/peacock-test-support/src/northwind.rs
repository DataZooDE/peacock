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

// --- Additional demo scenarios (each a distinct question + chart shape) ---
/// Top products by revenue (ranked bar).
pub const NW_REPORT_PRODUCTS: &str = "northwind-top-products";
pub const NW_QUERY_PRODUCTS: &str = "nw_revenue_by_product";
/// Revenue share by destination country (donut/pie).
pub const NW_REPORT_COUNTRY: &str = "northwind-sales-by-country";
pub const NW_QUERY_COUNTRY: &str = "nw_revenue_by_country";
/// Month × category revenue heatmap (reuses the category query).
pub const NW_REPORT_SEASON: &str = "northwind-seasonality";
/// Discount vs. line revenue (scatter/bubble over raw order lines).
pub const NW_REPORT_DISCOUNT: &str = "northwind-discount-vs-value";
pub const NW_QUERY_LINES: &str = "nw_order_line_values";
/// Revenue per salesperson, ranked best-first (horizontal leaderboard).
pub const NW_REPORT_LEADERBOARD: &str = "northwind-salesperson-leaderboard";
pub const NW_QUERY_LEADERBOARD: &str = "nw_revenue_by_salesperson";

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

/// The skill saved/shared report instances are filed under (BRD §7). Mirrors
/// `peacock_core::BOOKMARK_SKILL`. `visibility: owner` + `owner_field: owner`
/// makes a bookmark create-able and readable only by the principal who saved
/// it — peacock stamps the caller's `sub` into the instance's `owner` field.
pub const BOOKMARK_SKILL: &str = "report_bookmark";

/// The `report_bookmark` meta-skill (so saved-render instances validate and
/// the owner ACL gates them to their creator).
fn skill_bookmark() -> String {
    "---\n\
     type: skill\n\
     id: report_bookmark\n\
     description: A saved/shared parameterized render (bookmark).\n\
     visibility: owner\n\
     owner_field: owner\n\
     ---\n\
     # report_bookmark\n\
     A persisted (report, params) bookmark; renders transiently via peacock.\n"
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
///
/// Exposed (via the [`skill_report_markdown`] re-export) so authoring-tooling
/// tests can feed the canonical valid skill to `peacock author validate` /
/// `preview` without duplicating the markdown.
pub fn skill_report_markdown() -> String {
    skill_report()
}

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

// --- Scenario query pages (each a real GROUP BY over the same managed view) ---

/// Revenue per product, ranked — drives the "top products" bar.
fn query_revenue_by_product() -> String {
    "---\n\
     type: instance\n\
     skill: query\n\
     id: nw_revenue_by_product\n\
     target: \"[[nw_order_lines::eu]]\"\n\
     params:\n\
     \x20 - {name: from, type: date, required: true}\n\
     \x20 - {name: to, type: date, required: true}\n\
     sql: \"SELECT product AS product, \
     sum(unit_price * quantity * (1 - discount))::DOUBLE AS revenue FROM {{target}} \
     WHERE order_date BETWEEN :from AND :to GROUP BY 1 ORDER BY revenue DESC\"\n\
     ---\n\
     # nw_revenue_by_product\n\
     Net revenue per product; EMEA orders only.\n"
        .to_owned()
}

/// Revenue per destination country — drives the geography donut.
fn query_revenue_by_country() -> String {
    "---\n\
     type: instance\n\
     skill: query\n\
     id: nw_revenue_by_country\n\
     target: \"[[nw_order_lines::eu]]\"\n\
     params:\n\
     \x20 - {name: from, type: date, required: true}\n\
     \x20 - {name: to, type: date, required: true}\n\
     sql: \"SELECT country AS country, \
     sum(unit_price * quantity * (1 - discount))::DOUBLE AS revenue FROM {{target}} \
     WHERE order_date BETWEEN :from AND :to GROUP BY 1 ORDER BY revenue DESC\"\n\
     ---\n\
     # nw_revenue_by_country\n\
     Net revenue per destination country; EMEA orders only.\n"
        .to_owned()
}

/// Revenue per salesperson, ranked — drives the sales-manager leaderboard.
fn query_revenue_by_salesperson() -> String {
    "---\n\
     type: instance\n\
     skill: query\n\
     id: nw_revenue_by_salesperson\n\
     target: \"[[nw_order_lines::eu]]\"\n\
     params:\n\
     \x20 - {name: from, type: date, required: true}\n\
     \x20 - {name: to, type: date, required: true}\n\
     sql: \"SELECT salesperson AS salesperson, \
     sum(unit_price * quantity * (1 - discount))::DOUBLE AS revenue FROM {{target}} \
     WHERE order_date BETWEEN :from AND :to GROUP BY 1 ORDER BY revenue DESC\"\n\
     ---\n\
     # nw_revenue_by_salesperson\n\
     Net revenue per salesperson; EMEA orders only.\n"
        .to_owned()
}

/// Raw order lines with computed line revenue — drives the discount scatter.
fn query_order_line_values() -> String {
    "---\n\
     type: instance\n\
     skill: query\n\
     id: nw_order_line_values\n\
     target: \"[[nw_order_lines::eu]]\"\n\
     params:\n\
     \x20 - {name: from, type: date, required: true}\n\
     \x20 - {name: to, type: date, required: true}\n\
     sql: \"SELECT (discount * 100)::DOUBLE AS discount_pct, \
     (unit_price * quantity * (1 - discount))::DOUBLE AS revenue, \
     quantity::INTEGER AS quantity, category AS category FROM {{target}} \
     WHERE order_date BETWEEN :from AND :to ORDER BY revenue DESC\"\n\
     ---\n\
     # nw_order_line_values\n\
     One row per order line, with net line revenue; EMEA orders only.\n"
        .to_owned()
}

// --- Scenario report skills (one render core, four different shapes) ---

/// Top products by revenue — a ranked vertical bar (the query already orders
/// products by revenue desc, so the bars read as a leaderboard).
fn skill_report_products() -> String {
    r#"---
type: skill
id: northwind-top-products
render: a2ui
description: Northwind best-selling products by revenue (EMEA, 1997).
params:
  from: { type: date, default: "1997-01-01" }
  to:   { type: date, default: "1997-12-31" }
data:
  by_product: "[[query::nw_revenue_by_product]]"
views:
  - { kind: kpi,   data: by_product, agg: sum, field: revenue, label: "Total revenue" }
  - { kind: vega,  data: by_product, spec: prod_bar }
  - { kind: table, data: by_product }
specs:
  prod_bar:
    mark: bar
    encoding:
      x:     { field: product, type: ordinal,      title: Product }
      y:     { field: revenue, type: quantitative, title: Revenue }
      color: { field: product, type: nominal }
---
Best-selling products by net revenue, highest first. EMEA orders only.
"#
    .to_owned()
}

/// Revenue share by destination country — a donut.
fn skill_report_country() -> String {
    r#"---
type: skill
id: northwind-sales-by-country
render: a2ui
description: Northwind revenue share by destination country (EMEA, 1997).
params:
  from: { type: date, default: "1997-01-01" }
  to:   { type: date, default: "1997-12-31" }
data:
  by_country: "[[query::nw_revenue_by_country]]"
views:
  - { kind: kpi,   data: by_country, agg: sum, field: revenue, label: "Total revenue" }
  - { kind: vega,  data: by_country, spec: country_pie }
  - { kind: table, data: by_country }
specs:
  country_pie:
    mark: { type: arc, innerRadius: 60 }
    encoding:
      theta: { field: revenue, type: quantitative }
      color: { field: country, type: nominal }
---
Where the revenue comes from, by destination country. EMEA orders only.
"#
    .to_owned()
}

/// Month × category revenue heatmap — reuses the category query, renders rect.
fn skill_report_season() -> String {
    r#"---
type: skill
id: northwind-seasonality
render: a2ui
description: Northwind revenue seasonality — month × category heatmap (EMEA, 1997).
params:
  from:     { type: date,   default: "1997-01-01" }
  to:       { type: date,   default: "1997-12-31" }
  category: { type: string, default: "ALL" }
data:
  rev_by_cat: "[[query::nw_revenue_by_category]]"
views:
  - { kind: kpi,   data: rev_by_cat, agg: max, field: revenue, label: "Peak month revenue" }
  - { kind: vega,  data: rev_by_cat, spec: season_heat }
  - { kind: table, data: rev_by_cat }
specs:
  season_heat:
    mark: rect
    encoding:
      x:     { field: month,    type: ordinal, title: Month }
      y:     { field: category, type: nominal, title: Category }
      color: { field: revenue,  type: quantitative, title: Revenue }
---
Which categories peak in which months. Darker = more revenue. EMEA orders only.
"#
    .to_owned()
}

/// Discount vs. line value — a coloured, size-encoded scatter.
fn skill_report_discount() -> String {
    r#"---
type: skill
id: northwind-discount-vs-value
render: a2ui
description: Northwind discount vs. order-line value (EMEA, 1997).
params:
  from: { type: date, default: "1997-01-01" }
  to:   { type: date, default: "1997-12-31" }
data:
  lines: "[[query::nw_order_line_values]]"
views:
  - { kind: kpi,   data: lines, agg: avg, field: discount_pct, label: "Avg discount %" }
  - { kind: vega,  data: lines, spec: disc_scatter }
  - { kind: table, data: lines }
specs:
  disc_scatter:
    mark: point
    encoding:
      x:     { field: discount_pct, type: quantitative, title: "Discount %" }
      y:     { field: revenue,      type: quantitative, title: "Line revenue" }
      size:  { field: quantity,     type: quantitative, title: Quantity }
      color: { field: category,     type: nominal }
---
Does discounting drive bigger orders? Each point is one order line. EMEA only.
"#
    .to_owned()
}

/// Who sold the most — a horizontal, best-first leaderboard. Exercises the
/// rasterizer's horizontal bars (categories on y) + explicit `sort` by
/// another field (peacock #4/#5).
fn skill_report_leaderboard() -> String {
    r#"---
type: skill
id: northwind-salesperson-leaderboard
render: a2ui
description: Northwind revenue per salesperson, ranked (EMEA, 1997).
params:
  from: { type: date, default: "1997-01-01" }
  to:   { type: date, default: "1997-12-31" }
data:
  by_rep: "[[query::nw_revenue_by_salesperson]]"
views:
  - { kind: kpi,   data: by_rep, agg: max, field: revenue, label: "Top seller revenue" }
  - { kind: vega,  data: by_rep, spec: rep_bar }
  - { kind: table, data: by_rep }
specs:
  rep_bar:
    mark: bar
    encoding:
      y:     { field: salesperson, type: nominal, title: Salesperson, sort: { field: revenue, order: descending } }
      x:     { field: revenue,     type: quantitative, title: Revenue }
      color: { field: salesperson, type: nominal }
---
Who sold the most, best first. Net revenue, EMEA orders only.
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
            .skill(BOOKMARK_SKILL, skill_bookmark())
            .skill("nw_order_lines", skill_order_lines(relation))
            .skill(NW_REPORT, skill_report())
            .skill(NW_REPORT_PRODUCTS, skill_report_products())
            .skill(NW_REPORT_COUNTRY, skill_report_country())
            .skill(NW_REPORT_SEASON, skill_report_season())
            .skill(NW_REPORT_DISCOUNT, skill_report_discount())
            .skill(NW_REPORT_LEADERBOARD, skill_report_leaderboard())
            .instance("query", NW_QUERY_REF, query_revenue_by_category())
            .instance("query", NW_QUERY_PRODUCTS, query_revenue_by_product())
            .instance("query", NW_QUERY_COUNTRY, query_revenue_by_country())
            .instance("query", NW_QUERY_LINES, query_order_line_values())
            .instance(
                "query",
                NW_QUERY_LEADERBOARD,
                query_revenue_by_salesperson(),
            )
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
