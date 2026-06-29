//! Row/value helpers shared by transforms and the renderer.

use serde_json::Value;

/// Stringify a cell for use as a category key / label.
pub fn cell_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Numeric value of a cell (0.0 when absent / non-numeric).
pub fn cell_num(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(0.0)
}

/// Append `s` to `v` if absent, returning its index (first-seen order).
pub fn index_of(v: &mut Vec<String>, s: &str) -> usize {
    if let Some(i) = v.iter().position(|e| e == s) {
        i
    } else {
        v.push(s.to_owned());
        v.len() - 1
    }
}

/// Sort categories chronologically/numerically (ISO dates & numbers compare
/// correctly), falling back to lexical order.
pub fn sort_categories(cats: &mut [String]) {
    cats.sort_by(|a, b| match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.cmp(b),
    });
}
