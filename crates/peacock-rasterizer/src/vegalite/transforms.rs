//! Declarative data transforms computed in Rust — never JS eval.
//!
//! Supported `transform` entries: `filter` (field predicates equal / range /
//! oneOf), `fold`, `aggregate`, `bin`, and inline-encoding-driven `aggregate`
//! / `bin`. `calculate` is intentionally rejected by the guardrail.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::RasterError;

/// A row of inline data.
pub type Row = Map<String, Value>;

/// Pull inline `data.values` rows from the spec.
pub fn inline_rows(spec: &Value) -> Result<Vec<Row>, RasterError> {
    let arr = spec
        .get("data")
        .and_then(|d| d.get("values"))
        .and_then(Value::as_array)
        .ok_or_else(|| RasterError::new("spec has no inline data.values"))?;
    Ok(arr.iter().filter_map(|v| v.as_object().cloned()).collect())
}

/// Apply a `transform` array to rows in order.
pub fn apply_transforms(rows: Vec<Row>, spec: &Value) -> Result<Vec<Row>, RasterError> {
    let mut rows = rows;
    if let Some(tx) = spec.get("transform").and_then(Value::as_array) {
        for t in tx {
            rows = apply_one(rows, t)?;
        }
    }
    Ok(rows)
}

fn apply_one(rows: Vec<Row>, t: &Value) -> Result<Vec<Row>, RasterError> {
    if let Some(f) = t.get("filter") {
        return Ok(apply_filter(rows, f));
    }
    if let Some(fold) = t.get("fold").and_then(Value::as_array) {
        return Ok(apply_fold(rows, t, fold));
    }
    if t.get("aggregate").is_some() {
        return apply_aggregate(rows, t);
    }
    if let Some(bin_field) = t.get("bin") {
        return Ok(apply_bin_transform(rows, t, bin_field));
    }
    // Unknown transform — pass through (e.g. `calculate` is already rejected
    // by the guardrail before we get here).
    Ok(rows)
}

// ---------------------------------------------------------------------------
// filter
// ---------------------------------------------------------------------------

fn apply_filter(rows: Vec<Row>, f: &Value) -> Vec<Row> {
    rows.into_iter().filter(|r| matches_pred(r, f)).collect()
}

fn matches_pred(row: &Row, pred: &Value) -> bool {
    let obj = match pred.as_object() {
        Some(o) => o,
        None => return true, // string exprs are forbidden by guardrail
    };
    let field = match obj.get("field").and_then(Value::as_str) {
        Some(f) => f,
        None => return true,
    };
    let cell = row.get(field);
    if let Some(eq) = obj.get("equal") {
        return cell.map(|c| values_equal(c, eq)).unwrap_or(false);
    }
    if let Some(set) = obj.get("oneOf").and_then(Value::as_array) {
        return cell
            .map(|c| set.iter().any(|s| values_equal(c, s)))
            .unwrap_or(false);
    }
    let num = cell.and_then(Value::as_f64);
    if let Some(range) = obj.get("range").and_then(Value::as_array)
        && let (Some(lo), Some(hi)) = (
            range.first().and_then(Value::as_f64),
            range.get(1).and_then(Value::as_f64),
        )
    {
        return num.map(|n| n >= lo && n <= hi).unwrap_or(false);
    }
    if let Some(v) = obj.get("lt").and_then(Value::as_f64) {
        return num.map(|n| n < v).unwrap_or(false);
    }
    if let Some(v) = obj.get("lte").and_then(Value::as_f64) {
        return num.map(|n| n <= v).unwrap_or(false);
    }
    if let Some(v) = obj.get("gt").and_then(Value::as_f64) {
        return num.map(|n| n > v).unwrap_or(false);
    }
    if let Some(v) = obj.get("gte").and_then(Value::as_f64) {
        return num.map(|n| n >= v).unwrap_or(false);
    }
    true
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .zip(y.as_f64())
            .map(|(p, q)| (p - q).abs() < 1e-9)
            .unwrap_or(false),
        _ => a == b,
    }
}

// ---------------------------------------------------------------------------
// fold
// ---------------------------------------------------------------------------

fn apply_fold(rows: Vec<Row>, t: &Value, fields: &[Value]) -> Vec<Row> {
    let names = t.get("as").and_then(Value::as_array);
    let key_name = names
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or("key")
        .to_owned();
    let value_name = names
        .and_then(|a| a.get(1))
        .and_then(Value::as_str)
        .unwrap_or("value")
        .to_owned();
    let fields: Vec<String> = fields
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut out = Vec::new();
    for row in rows {
        for f in &fields {
            let mut nr = row.clone();
            let v = row.get(f).cloned().unwrap_or(Value::Null);
            nr.insert(key_name.clone(), Value::String(f.clone()));
            nr.insert(value_name.clone(), v);
            out.push(nr);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// aggregate
// ---------------------------------------------------------------------------

fn apply_aggregate(rows: Vec<Row>, t: &Value) -> Result<Vec<Row>, RasterError> {
    let aggs = t
        .get("aggregate")
        .and_then(Value::as_array)
        .ok_or_else(|| RasterError::new("aggregate transform needs an array"))?;
    let groupby: Vec<String> = t
        .get("groupby")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    // group rows by the groupby key tuple (preserving first-seen order).
    let mut order: Vec<Vec<String>> = Vec::new();
    let mut groups: BTreeMap<Vec<String>, Vec<Row>> = BTreeMap::new();
    for row in rows {
        let key: Vec<String> = groupby
            .iter()
            .map(|g| crate::vegalite::data::cell_string(row.get(g)))
            .collect();
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(row);
    }

    let mut out = Vec::new();
    for key in order {
        let grp = &groups[&key];
        let mut nr = Row::new();
        for (g, kv) in groupby.iter().zip(key.iter()) {
            // preserve original typed value where possible
            let orig = grp.first().and_then(|r| r.get(g)).cloned();
            nr.insert(g.clone(), orig.unwrap_or(Value::String(kv.clone())));
        }
        for a in aggs {
            let op = a.get("op").and_then(Value::as_str).unwrap_or("count");
            let field = a.get("field").and_then(Value::as_str);
            let out_name = a
                .get("as")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| match field {
                    Some(f) => format!("{op}_{f}"),
                    None => op.to_owned(),
                });
            let nums: Vec<f64> = match field {
                Some(f) => grp
                    .iter()
                    .filter_map(|r| r.get(f).and_then(Value::as_f64))
                    .collect(),
                None => Vec::new(),
            };
            let val = compute_agg(op, &nums, grp.len());
            nr.insert(out_name, json_num(val));
        }
        out.push(nr);
    }
    Ok(out)
}

/// Compute an aggregate op over numbers.
pub fn compute_agg(op: &str, nums: &[f64], count: usize) -> f64 {
    match op {
        "count" => count as f64,
        "sum" => nums.iter().sum(),
        "mean" | "average" => {
            if nums.is_empty() {
                0.0
            } else {
                nums.iter().sum::<f64>() / nums.len() as f64
            }
        }
        "min" => nums.iter().cloned().fold(f64::INFINITY, f64::min),
        "max" => nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        "median" => median(nums),
        _ => nums.iter().sum(),
    }
}

fn median(nums: &[f64]) -> f64 {
    if nums.is_empty() {
        return 0.0;
    }
    let mut v = nums.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn json_num(v: f64) -> Value {
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// bin (transform form)
// ---------------------------------------------------------------------------

fn apply_bin_transform(rows: Vec<Row>, t: &Value, _bin: &Value) -> Vec<Row> {
    let field = match t.get("field").and_then(Value::as_str) {
        Some(f) => f.to_owned(),
        None => return rows,
    };
    let names = t.get("as").and_then(Value::as_array);
    let start_name = names
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or("bin_start")
        .to_owned();
    let end_name = names
        .and_then(|a| a.get(1))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let maxbins = t
        .get("bin")
        .and_then(|b| b.get("maxbins"))
        .and_then(Value::as_f64)
        .unwrap_or(10.0);
    let values: Vec<f64> = rows
        .iter()
        .filter_map(|r| r.get(&field).and_then(Value::as_f64))
        .collect();
    let (lo, hi, width) = bin_params(&values, maxbins);
    rows.into_iter()
        .map(|mut r| {
            if let Some(v) = r.get(&field).and_then(Value::as_f64) {
                let idx = (((v - lo) / width).floor()).max(0.0);
                let start = lo + idx * width;
                let _ = hi;
                r.insert(start_name.clone(), json_num(start));
                if let Some(en) = &end_name {
                    r.insert(en.clone(), json_num(start + width));
                }
            }
            r
        })
        .collect()
}

/// Compute nice bin (lo, hi, width) for a maxbins target.
pub fn bin_params(values: &[f64], maxbins: f64) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 1.0, 1.0);
    }
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if (max - min).abs() < f64::EPSILON {
        return (min, min + 1.0, 1.0);
    }
    let span = max - min;
    let raw = span / maxbins.max(1.0);
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    } * mag;
    let lo = (min / step).floor() * step;
    let hi = (max / step).ceil() * step;
    (lo, hi, step)
}
