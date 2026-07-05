//! The RowSet→ggplot data adapter (issue #8): columns are typed from the
//! escurel-reported SCHEMA type names (numeric vs categorical vs temporal),
//! never sniffed from the JSON values. Mismatches between the schema and the
//! rows — or between the schema and what the geom needs — are clear errors,
//! never a silently garbled chart.

use peacock_ggplot::{ColumnKind, ColumnSchema, render_stat_to_png};
use serde_json::{Value, json};

fn schema(cols: &[(&str, &str)]) -> Vec<ColumnSchema> {
    cols.iter()
        .map(|(name, type_name)| ColumnSchema {
            name: (*name).to_owned(),
            type_name: (*type_name).to_owned(),
        })
        .collect()
}

fn render(spec: Value, rows: Value, schema: &[ColumnSchema]) -> Result<Vec<u8>, String> {
    render_stat_to_png(&spec, &rows, schema, None, 1.0).map_err(|e| e.to_string())
}

fn assert_png(bytes: &[u8]) {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG magic header");
    assert!(bytes.len() > 500, "a real chart ({} bytes)", bytes.len());
}

// ── the type-name classifier ─────────────────────────────────────────────────

#[test]
fn schema_type_names_classify_columns() {
    for t in [
        // DuckDB SQL spellings.
        "DOUBLE",
        "FLOAT",
        "REAL",
        "INTEGER",
        "INT",
        "BIGINT",
        "HUGEINT",
        "SMALLINT",
        "TINYINT",
        "UBIGINT",
        "UINTEGER",
        "DECIMAL(18,3)",
        "NUMERIC",
        "double",
        // Arrow `DataType` Debug spellings — escurel's real wire form.
        "Float64",
        "Float32",
        "Int64",
        "Int32",
        "Int16",
        "Int8",
        "UInt64",
        "UInt32",
        "Decimal128(18, 3)",
    ] {
        assert_eq!(
            ColumnKind::from_type_name(t),
            ColumnKind::Numeric,
            "`{t}` is numeric"
        );
    }
    for t in [
        "DATE",
        "TIMESTAMP",
        "TIMESTAMP WITH TIME ZONE",
        "TIMESTAMPTZ",
        "DATETIME",
        "date",
        // Arrow `DataType` Debug spellings.
        "Date32",
        "Date64",
        "Timestamp(Microsecond, None)",
        "Timestamp(Nanosecond, Some(\"UTC\"))",
    ] {
        assert_eq!(
            ColumnKind::from_type_name(t),
            ColumnKind::Temporal,
            "`{t}` is temporal"
        );
    }
    for t in [
        "VARCHAR",
        "TEXT",
        "BOOLEAN",
        "ENUM('a','b')",
        "UUID",
        "BLOB",
        "Utf8",
        "LargeUtf8",
        "Boolean",
        "Binary",
    ] {
        assert_eq!(
            ColumnKind::from_type_name(t),
            ColumnKind::Categorical,
            "`{t}` is categorical"
        );
    }
}

// ── numeric / categorical / temporal columns via the schema ─────────────────

#[test]
fn numeric_column_typed_by_schema_renders() {
    let rows = json!(
        (0..60)
            .map(|i| json!({ "lead_days": 5.0 + f64::from(i % 13) }))
            .collect::<Vec<_>>()
    );
    let png = render(
        json!({ "geom": "density", "x": "lead_days" }),
        rows,
        &schema(&[("lead_days", "DOUBLE")]),
    )
    .expect("a DOUBLE column renders as the density's value axis");
    assert_png(&png);
}

#[test]
fn temporal_column_typed_by_schema_renders() {
    // escurel serializes DATE as an ISO-8601 string; the schema names it DATE,
    // so the adapter types it temporally — no JSON sniffing.
    let rows =
        json!((0..60)
        .map(|i| json!({ "delivered_on": format!("1997-{:02}-{:02}", 1 + i % 12, 1 + i % 28) }))
        .collect::<Vec<_>>());
    let png = render(
        json!({ "geom": "histogram", "x": "delivered_on" }),
        rows,
        &schema(&[("delivered_on", "DATE")]),
    )
    .expect("a DATE column renders as a temporal value axis");
    assert_png(&png);
}

#[test]
fn timestamp_values_parse_too() {
    let rows = json!(
        (0..40)
            .map(|i| json!({ "at": format!("1997-06-{:02}T{:02}:30:00", 1 + i % 28, i % 24) }))
            .collect::<Vec<_>>()
    );
    let png = render(
        json!({ "geom": "ecdf", "x": "at" }),
        rows,
        &schema(&[("at", "TIMESTAMP")]),
    )
    .expect("a TIMESTAMP column renders as a temporal value axis");
    assert_png(&png);
}

#[test]
fn categorical_typed_value_column_errors_clearly() {
    // The values LOOK numeric, but the schema says VARCHAR — schema wins,
    // and the geom's value axis needs a numeric/temporal column.
    let rows = json!([{ "code": "1" }, { "code": "2" }]);
    let err = render(
        json!({ "geom": "density", "x": "code" }),
        rows,
        &schema(&[("code", "VARCHAR")]),
    )
    .expect_err("a VARCHAR value column must error, not sniff the strings");
    assert!(
        err.contains("code") && err.contains("VARCHAR") && err.contains("density"),
        "error names the column, its schema type and the geom: {err}"
    );
}

// ── schema/rows mismatches ───────────────────────────────────────────────────

#[test]
fn value_contradicting_the_schema_errors_clearly() {
    let rows = json!([{ "lead_days": 5.0 }, { "lead_days": "n/a" }]);
    let err = render(
        json!({ "geom": "density", "x": "lead_days" }),
        rows,
        &schema(&[("lead_days", "DOUBLE")]),
    )
    .expect_err("a string in a DOUBLE column is a schema/rows mismatch");
    assert!(
        err.contains("lead_days") && err.contains("DOUBLE"),
        "error names the column and the declared type: {err}"
    );
}

#[test]
fn unparseable_temporal_value_errors_clearly() {
    let rows = json!([{ "delivered_on": "not-a-date" }]);
    let err = render(
        json!({ "geom": "density", "x": "delivered_on" }),
        rows,
        &schema(&[("delivered_on", "DATE")]),
    )
    .expect_err("garbage in a DATE column is a schema/rows mismatch");
    assert!(
        err.contains("delivered_on") && err.contains("DATE"),
        "error names the column and the declared type: {err}"
    );
}

#[test]
fn column_missing_from_the_schema_errors_clearly() {
    // The rows carry the column but the schema does not — the adapter trusts
    // the schema, so this is a clear mismatch error.
    let rows = json!([{ "lead_days": 5.0 }, { "lead_days": 7.0 }]);
    let err = render(
        json!({ "geom": "density", "x": "lead_days" }),
        rows,
        &schema(&[("something_else", "DOUBLE")]),
    )
    .expect_err("a column absent from the schema must error");
    assert!(
        err.contains("lead_days") && err.contains("schema"),
        "error names the missing column and blames the schema: {err}"
    );
}

// ── nulls and emptiness ──────────────────────────────────────────────────────

#[test]
fn empty_rows_error_clearly() {
    let err = render(
        json!({ "geom": "density", "x": "lead_days" }),
        json!([]),
        &schema(&[("lead_days", "DOUBLE")]),
    )
    .expect_err("no rows at all is a clear error, not an empty chart");
    assert!(err.contains("lead_days"), "error names the column: {err}");
}

#[test]
fn null_rows_are_skipped_as_na() {
    let dense: Vec<Value> = (0..40)
        .map(|i| json!({ "lead_days": 5.0 + f64::from(i % 11) }))
        .collect();
    let mut with_nulls = dense.clone();
    with_nulls.push(json!({ "lead_days": null }));
    with_nulls.push(json!({}));

    let clean = render(
        json!({ "geom": "density", "x": "lead_days" }),
        json!(dense),
        &schema(&[("lead_days", "DOUBLE")]),
    )
    .unwrap();
    let skipped = render(
        json!({ "geom": "density", "x": "lead_days" }),
        json!(with_nulls),
        &schema(&[("lead_days", "DOUBLE")]),
    )
    .expect("null cells are NA rows, not errors");
    assert_eq!(
        clean, skipped,
        "NA rows drop out without changing the chart"
    );
}

#[test]
fn all_null_column_errors_clearly() {
    let rows = json!([{ "lead_days": null }, { "lead_days": null }]);
    let err = render(
        json!({ "geom": "density", "x": "lead_days" }),
        rows,
        &schema(&[("lead_days", "DOUBLE")]),
    )
    .expect_err("a column with no usable values must error");
    assert!(err.contains("lead_days"), "error names the column: {err}");
}

#[test]
fn grouping_columns_stay_row_aligned_after_na_skips() {
    // A null in the value column drops the WHOLE row, so the color series
    // stays aligned; grouping by supplier still renders.
    let mut rows: Vec<Value> = (0..60)
        .map(|i| {
            json!({
                "lead_days": 5.0 + f64::from(i % 13),
                "supplier": if i % 2 == 0 { "acme" } else { "globex" }
            })
        })
        .collect();
    rows.insert(10, json!({ "lead_days": null, "supplier": "acme" }));
    let png = render(
        json!({ "geom": "density", "x": "lead_days", "color": "supplier" }),
        json!(rows),
        &schema(&[("lead_days", "DOUBLE"), ("supplier", "VARCHAR")]),
    )
    .expect("grouped density renders across NA skips");
    assert_png(&png);
}
