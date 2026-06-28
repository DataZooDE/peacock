//! peacock test harness. No mocks — see CLAUDE.md principle 2.
//!
//! [`NorthwindEscurel`] spawns a **real** escurel (`EscurelProcess`) seeded
//! with the paper's running example: an offline Parquet `sql_view`
//! (`nw_order_lines`, group-ACL `read: [sales]`) and a `query_instance`
//! query page (`nw_revenue_by_category`) that does the real revenue
//! aggregation with `:from`/`:to`/`:category` bound as prepared-statement
//! parameters. `PeacockProcess` (the real `peacock` binary) lands in Phase 7.

mod northwind;
mod process;

pub use northwind::{
    NW_QUERY_REF, NW_REPORT, NW_REPORT_PRODUCTS, NW_REPORT_SEASON, NorthwindEscurel, SALES_GROUP,
    TENANT,
};
pub use process::PeacockProcess;
