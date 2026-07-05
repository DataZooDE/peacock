//! The RowSet→ggplot data adapter (issue #8): peacock's ACL-checked escurel
//! rows (a JSON array) plus their escurel-reported SCHEMA become row-aligned,
//! **typed** ggplot columns. The schema's type names decide numeric vs
//! categorical vs temporal — the JSON values are never sniffed — so a
//! mismatch between schema and rows (or between the schema and what a geom
//! needs) is a clear structured error, never a silently garbled chart.
//!
//! No polars, no arrow: the target is ggplot-rs's plain
//! `Vec<(String, Vec<Value>)>` `GGData` input, which is already columnar and
//! typed — pulling the Arrow dependency graph in to build `RecordBatch`es
//! from JSON would duplicate this conversion, not remove it.

use ggplot_rs::data::Value as GgValue;
use peacock_types::StatGeom;
use serde_json::Value;

use crate::GgplotError;

/// One column's name and escurel-reported type — the shape of
/// `peacock-core`'s `RowSet` schema entries, mirrored here so the adapter
/// stays dependency-free (peacock-core depends on this crate, not vice
/// versa).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSchema {
    pub name: String,
    pub type_name: String,
}

/// How a schema type name renders: on a value axis as a number, on a value
/// axis as a point in time, or as a grouping label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Numeric,
    Temporal,
    Categorical,
}

impl ColumnKind {
    /// Classify a column type name. escurel reports the column type as the
    /// `Debug` form of DuckDB's returned **Arrow** `DataType` (`Float64`,
    /// `Utf8`, `Date32`, `Timestamp(Microsecond, None)`, `Decimal128(18,3)`),
    /// so the Arrow spellings are the ones that reach a real render; the
    /// DuckDB SQL spellings (`DOUBLE`, `VARCHAR`, `TIMESTAMP`) are recognised
    /// too so a hand-authored schema classifies the same. Parameterised types
    /// (`DECIMAL(18,3)`, `Timestamp(Microsecond, None)`) classify by their
    /// base name; unknown types are categorical (they can always group, never
    /// fake a number).
    pub fn from_type_name(type_name: &str) -> Self {
        let base = type_name
            .split('(')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        match base.as_str() {
            // DuckDB SQL spellings.
            "TINYINT" | "SMALLINT" | "INTEGER" | "INT" | "BIGINT" | "HUGEINT" | "UTINYINT"
            | "USMALLINT" | "UINTEGER" | "UBIGINT" | "UHUGEINT" | "FLOAT" | "REAL" | "DOUBLE"
            | "DECIMAL" | "NUMERIC"
            // Arrow `DataType` Debug spellings (escurel's wire form).
            | "INT8" | "INT16" | "INT32" | "INT64" | "UINT8" | "UINT16" | "UINT32" | "UINT64"
            | "FLOAT16" | "FLOAT32" | "FLOAT64" | "DECIMAL128" | "DECIMAL256" => {
                ColumnKind::Numeric
            }
            // DuckDB SQL spellings.
            "DATE" | "DATETIME" | "TIMESTAMP" | "TIMESTAMPTZ" | "TIMESTAMP WITH TIME ZONE"
            | "TIMESTAMP_S" | "TIMESTAMP_MS" | "TIMESTAMP_NS"
            // Arrow `DataType` Debug spellings.
            | "DATE32" | "DATE64" => ColumnKind::Temporal,
            _ => ColumnKind::Categorical,
        }
    }
}

/// The role a column plays in the chart: a VALUE column feeds a numeric axis
/// (and must be schema-typed numeric or temporal); a GROUP column labels a
/// series / facet / boxplot category (any schema type groups).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Value,
    Group,
}

/// One column the chart needs out of the rows.
pub(crate) struct NeededColumn<'a> {
    pub(crate) name: &'a str,
    pub(crate) role: Role,
}

impl<'a> NeededColumn<'a> {
    pub(crate) fn value(name: &'a str) -> Self {
        NeededColumn {
            name,
            role: Role::Value,
        }
    }
    pub(crate) fn group(name: &'a str) -> Self {
        NeededColumn {
            name,
            role: Role::Group,
        }
    }
}

/// Extract `columns` from the escurel rows as row-aligned, schema-typed
/// ggplot columns (grouping/facet values must stay aligned with the
/// measure). A row whose FIRST requested column (the geometry's driving
/// column) is `null` is skipped whole (NA), as is a row with an NA in any
/// secondary value column; a cell that contradicts the schema is a clear
/// error — never a silently garbled chart.
pub(crate) fn aligned_columns(
    rows: &Value,
    schema: &[ColumnSchema],
    columns: &[NeededColumn<'_>],
    geom: StatGeom,
) -> Result<Vec<(String, Vec<GgValue>)>, GgplotError> {
    let arr = rows
        .as_array()
        .ok_or_else(|| GgplotError::new("rows must be a JSON array of objects"))?;

    // Resolve each needed column against the schema up front: kind decides
    // the conversion, and a VALUE role rejects categorical types here —
    // before any row is touched.
    let mut kinds = Vec::with_capacity(columns.len());
    for col in columns {
        let entry = schema.iter().find(|c| c.name == col.name).ok_or_else(|| {
            GgplotError::new(format!(
                "the rows' schema has no column `{}` (schema: {:?})",
                col.name,
                schema.iter().map(|c| c.name.as_str()).collect::<Vec<_>>()
            ))
        })?;
        let kind = ColumnKind::from_type_name(&entry.type_name);
        if col.role == Role::Value && kind == ColumnKind::Categorical {
            return Err(GgplotError::new(format!(
                "column `{}` is {} — geom `{}` needs a numeric or date/timestamp value column",
                col.name,
                entry.type_name,
                geom.as_str()
            )));
        }
        kinds.push((kind, entry.type_name.clone()));
    }

    let mut out: Vec<(String, Vec<GgValue>)> = columns
        .iter()
        .map(|c| (c.name.to_owned(), Vec::with_capacity(arr.len())))
        .collect();

    'row: for row in arr {
        // Drop the row when the driving column is NA — checked first so no
        // partial row is pushed.
        if matches!(row.get(columns[0].name), None | Some(Value::Null)) {
            continue;
        }
        let mut converted = Vec::with_capacity(columns.len());
        for (col, (kind, type_name)) in columns.iter().zip(&kinds) {
            let v = row.get(col.name).unwrap_or(&Value::Null);
            match col.role {
                Role::Group => converted.push(categorical(v)),
                Role::Value if v.is_null() => continue 'row, // NA in a secondary value column
                Role::Value => converted.push(typed_value(v, *kind, col.name, type_name)?),
            }
        }
        for (slot, value) in out.iter_mut().zip(converted) {
            slot.1.push(value);
        }
    }

    if out[0].1.is_empty() {
        return Err(GgplotError::new(format!(
            "column `{}` has no values to plot",
            columns[0].name
        )));
    }
    Ok(out)
}

/// Convert one VALUE-role cell per its schema kind. The schema is the
/// contract: a cell that contradicts it is a mismatch error naming both.
fn typed_value(
    v: &Value,
    kind: ColumnKind,
    name: &str,
    type_name: &str,
) -> Result<GgValue, GgplotError> {
    match kind {
        ColumnKind::Numeric => v.as_f64().map(GgValue::Float).ok_or_else(|| {
            GgplotError::new(format!(
                "column `{name}` has a non-numeric value ({v}) but the schema declares \
                 {type_name} — the rows do not match the schema"
            ))
        }),
        ColumnKind::Temporal => v
            .as_str()
            .and_then(iso_to_epoch_secs)
            .map(GgValue::DateTime)
            .ok_or_else(|| {
                GgplotError::new(format!(
                    "column `{name}` has a value ({v}) that is not an ISO-8601 date/time but \
                     the schema declares {type_name} — the rows do not match the schema"
                ))
            }),
        ColumnKind::Categorical => unreachable!("VALUE roles reject categorical kinds up front"),
    }
}

/// A categorical cell: strings pass through; numbers/bools become their text
/// form; `null` groups as `"NA"`.
fn categorical(v: &Value) -> GgValue {
    match v {
        Value::String(s) => GgValue::Str(s.clone()),
        Value::Null => GgValue::Str("NA".to_owned()),
        other => GgValue::Str(other.to_string()),
    }
}

/// Parse an ISO-8601 date (`1997-06-01`) or date-time
/// (`1997-06-01T12:30:00`, space separator and fractional seconds / `Z` /
/// numeric offsets tolerated but the offset ignored) into seconds since the
/// Unix epoch — escurel's wire form for DATE / TIMESTAMP columns.
fn iso_to_epoch_secs(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = match s.find(['T', ' ']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(y, m, d) * 86_400;
    if let Some(t) = time {
        // Cut fractional seconds and any numeric offset (`+02:00` / `-05:00`).
        let t = t.split(['.', '+', '-']).next().unwrap_or(t);
        let mut parts = t.split(':');
        let h: i64 = parts.next()?.parse().ok()?;
        let mi: i64 = parts.next().unwrap_or("0").parse().ok()?;
        let sec: i64 = parts.next().unwrap_or("0").parse().ok()?;
        if !(0..24).contains(&h) || !(0..60).contains(&mi) || !(0..60).contains(&sec) {
            return None;
        }
        secs += h * 3600 + mi * 60 + sec;
    }
    Some(secs)
}

/// Days from 1970-01-01 for a proleptic-Gregorian civil date (Howard
/// Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_parsing_matches_known_dates() {
        assert_eq!(iso_to_epoch_secs("1970-01-01"), Some(0));
        assert_eq!(iso_to_epoch_secs("1997-01-01"), Some(9862 * 86_400));
        assert_eq!(
            iso_to_epoch_secs("1997-01-01T06:30:15"),
            Some(9862 * 86_400 + 6 * 3600 + 30 * 60 + 15)
        );
        assert_eq!(
            iso_to_epoch_secs("1997-01-01 06:30:15.250Z"),
            Some(9862 * 86_400 + 6 * 3600 + 30 * 60 + 15)
        );
        assert_eq!(iso_to_epoch_secs("not-a-date"), None);
        assert_eq!(iso_to_epoch_secs("1997-13-01"), None);
    }
}
