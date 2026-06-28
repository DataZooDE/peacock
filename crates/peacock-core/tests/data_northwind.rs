//! Phase 3 no-mock integration tests: peacock's data reader against a **real**
//! escurel reading the **real** Northwind Parquet fixture (FR-D-1..6, ACC-2,
//! ACC-3, ACC-11). No mocks — a real escurel process, real DuckDB, real
//! Parquet. The paper's running example (monthly revenue by category) is the
//! demonstration thread.

use peacock_core::{EscurelData, ReportData};
use peacock_test_support::{NW_QUERY_REF, NorthwindEscurel};
use serde_json::json;

/// Full-year 1997 params, all categories (the report's default view).
fn params_1997_all() -> serde_json::Value {
    json!({ "from": "1997-01-01", "to": "1997-12-31", "category": "ALL" })
}

fn revenue_sum(rows: &serde_json::Value) -> f64 {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| r["revenue"].as_f64().unwrap())
        .sum()
}

#[tokio::test]
async fn reads_aggregated_revenue_by_category_for_1997() {
    let nw = NorthwindEscurel::spawn().await;
    let data = EscurelData::new(nw.endpoint());

    let rs = data
        .query_view(
            NW_QUERY_REF,
            &params_1997_all(),
            &nw.sales_principal(),
            None,
        )
        .await
        .expect("query_instance returns rows");

    let rows = rs.rows.as_array().expect("rows is an array");
    // 16 month×category groups across 1997; the 1996 lines are excluded by
    // the bound :from/:to date range.
    assert_eq!(rows.len(), 16, "rows: {:#?}", rows);
    // Grand total revenue for 1997 (KPI cross-check).
    assert_eq!(revenue_sum(&rs.rows), 2986.0);
    // First group is January Beverages = 18*10 = 180.
    assert_eq!(rows[0]["month"], "1997-01-01");
    assert_eq!(rows[0]["category"], "Beverages");
    assert_eq!(rows[0]["revenue"].as_f64().unwrap(), 180.0);
    // Schema carries the projected columns escurel reported.
    assert!(rs.schema.iter().any(|c| c.name == "revenue"));
    assert!(!rs.truncated);

    nw.shutdown().await;
}

#[tokio::test]
async fn drill_to_one_category_is_just_different_bound_params() {
    let nw = NorthwindEscurel::spawn().await;
    let data = EscurelData::new(nw.endpoint());

    let drilled = json!({ "from": "1997-01-01", "to": "1997-12-31", "category": "Beverages" });
    let rs = data
        .query_view(NW_QUERY_REF, &drilled, &nw.sales_principal(), None)
        .await
        .expect("drilled query");

    let rows = rs.rows.as_array().unwrap();
    assert!(rows.iter().all(|r| r["category"] == "Beverages"));
    // Beverages-only 1997 revenue.
    assert_eq!(revenue_sum(&rs.rows), 801.0);

    nw.shutdown().await;
}

#[tokio::test]
async fn injection_value_is_bound_not_executed() {
    // ACC-11: a category value carrying SQL metacharacters changes only the
    // bound value — escurel runs it as a prepared-statement parameter
    // (matching nothing), the `pages` table is untouched, peacock emits no SQL.
    let nw = NorthwindEscurel::spawn().await;
    let data = EscurelData::new(nw.endpoint());

    let evil = json!({
        "from": "1997-01-01",
        "to": "1997-12-31",
        "category": "Beverages'; DROP TABLE pages; --"
    });
    let rs = data
        .query_view(NW_QUERY_REF, &evil, &nw.sales_principal(), None)
        .await
        .expect("injection value binds, does not error");
    assert!(
        rs.rows.as_array().unwrap().is_empty(),
        "no category equals the injection literal"
    );

    // A legitimate value still works afterwards — escurel survived intact.
    let ok = data
        .query_view(
            NW_QUERY_REF,
            &params_1997_all(),
            &nw.sales_principal(),
            None,
        )
        .await
        .expect("escurel intact");
    assert_eq!(ok.rows.as_array().unwrap().len(), 16);

    nw.shutdown().await;
}

#[tokio::test]
async fn acl_denial_is_a_typed_error_not_a_partial_read() {
    // ACC-3 / FR-D-5: a principal lacking the view's `read` group gets a typed
    // error (Data or Auth), never rows.
    let nw = NorthwindEscurel::spawn().await;
    let data = EscurelData::new(nw.endpoint());

    let err = data
        .query_view(
            NW_QUERY_REF,
            &params_1997_all(),
            &nw.no_sales_principal(),
            None,
        )
        .await
        .expect_err("ACL must deny a non-sales principal");

    let kind = err.kind();
    assert!(
        kind == "data" || kind == "auth",
        "expected typed data/auth error, got {kind}: {err}"
    );

    nw.shutdown().await;
}
