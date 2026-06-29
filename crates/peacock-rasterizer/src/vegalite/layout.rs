//! View composition: `layer`, `facet` / `row`+`column`, `hconcat`, `vconcat`.
//! Each composition recurses into [`render_view`]; leaf (unit) specs render via
//! [`super::unit::render_unit`].

use std::fmt::Write as _;

use serde_json::Value;

use super::transforms::Row;
use super::{svgutil, unit};
use crate::RasterError;

/// A rendered fragment plus its measured outer size.
pub struct Rendered {
    pub body: String,
    pub width: f64,
    pub height: f64,
}

/// Render any view spec (composite or unit). `inherited` supplies data rows
/// when a parent (facet/concat) has already resolved them.
pub fn render_view(spec: &Value, inherited: Option<&[Row]>) -> Result<Rendered, RasterError> {
    if let Some(layers) = spec.get("layer").and_then(Value::as_array) {
        return render_layer(spec, layers, inherited);
    }
    if let Some(specs) = spec.get("hconcat").and_then(Value::as_array) {
        return render_concat(specs, inherited, true);
    }
    if let Some(specs) = spec.get("vconcat").and_then(Value::as_array) {
        return render_concat(specs, inherited, false);
    }
    if let Some(specs) = spec.get("concat").and_then(Value::as_array) {
        // default concat flows as columns; treat as horizontal.
        return render_concat(specs, inherited, true);
    }
    // facet via row/column encoding channel, or explicit `facet` operator.
    if let Some(r) = facet_spec(spec) {
        return render_facet(spec, r, inherited);
    }
    // Unit view.
    let rows = resolve_rows(spec, inherited)?;
    unit::render_unit(spec, &rows, None)
}

/// Resolve the data rows for a view: inline `data.values` (after transforms),
/// or inherited rows from a parent composition.
fn resolve_rows(spec: &Value, inherited: Option<&[Row]>) -> Result<Vec<Row>, RasterError> {
    let base = match super::transforms::inline_rows(spec) {
        Ok(r) => r,
        Err(_) => match inherited {
            Some(r) => r.to_vec(),
            None => return Err(RasterError::new("spec has no inline data.values")),
        },
    };
    super::transforms::apply_transforms(base, spec)
}

// ---------------------------------------------------------------------------
// layer
// ---------------------------------------------------------------------------

fn render_layer(
    spec: &Value,
    layers: &[Value],
    inherited: Option<&[Row]>,
) -> Result<Rendered, RasterError> {
    let rows = resolve_rows(spec, inherited)?;
    // Layers share scales: compute a combined frame from all layer encodings
    // merged with the parent's shared encoding.
    let shared_enc = spec.get("encoding");
    let mut body = String::new();
    let mut width = super::DEFAULT_W;
    let mut height = super::DEFAULT_H;
    for (i, layer) in layers.iter().enumerate() {
        let merged = merge_encoding(layer, shared_enc);
        let layer_rows = super::transforms::apply_transforms(rows.clone(), layer)?;
        let r = unit::render_unit(&merged, &layer_rows, Some(i > 0))?;
        width = r.width;
        height = r.height;
        body.push_str(&r.body);
    }
    Ok(Rendered {
        body,
        width,
        height,
    })
}

/// Merge a parent `encoding` into a child spec's encoding (child wins).
fn merge_encoding(child: &Value, parent: Option<&Value>) -> Value {
    let mut out = child.clone();
    if let (Some(p), Some(obj)) = (parent, out.as_object_mut()) {
        let child_enc = obj.get("encoding").cloned();
        let mut enc = p.clone();
        if let (Some(e), Some(c)) = (
            enc.as_object_mut(),
            child_enc.as_ref().and_then(Value::as_object),
        ) {
            for (k, v) in c {
                e.insert(k.clone(), v.clone());
            }
        }
        obj.insert("encoding".to_owned(), enc);
    }
    out
}

// ---------------------------------------------------------------------------
// concat
// ---------------------------------------------------------------------------

fn render_concat(
    specs: &[Value],
    inherited: Option<&[Row]>,
    horizontal: bool,
) -> Result<Rendered, RasterError> {
    let pad = 18.0;
    let mut pieces = Vec::new();
    for s in specs {
        pieces.push(render_view(s, inherited)?);
    }
    let mut body = String::new();
    let (mut x, mut y) = (0.0_f64, 0.0_f64);
    let mut total_w = 0.0_f64;
    let mut total_h = 0.0_f64;
    for p in &pieces {
        let _ = write!(body, r##"<g transform="translate({x:.1},{y:.1})">"##);
        body.push_str(&p.body);
        body.push_str("</g>");
        if horizontal {
            x += p.width + pad;
            total_w = x - pad;
            total_h = total_h.max(p.height);
        } else {
            y += p.height + pad;
            total_h = y - pad;
            total_w = total_w.max(p.width);
        }
    }
    Ok(Rendered {
        body,
        width: total_w.max(1.0),
        height: total_h.max(1.0),
    })
}

// ---------------------------------------------------------------------------
// facet (row / column small multiples)
// ---------------------------------------------------------------------------

struct FacetSpec {
    row_field: Option<String>,
    col_field: Option<String>,
    inner: Value,
}

fn facet_spec(spec: &Value) -> Option<FacetSpec> {
    // explicit `facet` operator with a `spec` child.
    if let Some(f) = spec.get("facet") {
        let inner = spec.get("spec").cloned().unwrap_or(Value::Null);
        if inner.is_null() {
            return None;
        }
        let row_field = f.get("row").and_then(field_of).or_else(|| field_of(f));
        let col_field = f.get("column").and_then(field_of);
        return Some(FacetSpec {
            row_field: if f.get("row").is_some() || f.get("column").is_none() {
                row_field
            } else {
                None
            },
            col_field,
            inner,
        });
    }
    // row / column facet channels in the encoding.
    let enc = spec.get("encoding")?;
    let row_field = enc.get("row").and_then(field_of);
    let col_field = enc.get("column").and_then(field_of);
    if row_field.is_none() && col_field.is_none() {
        return None;
    }
    // strip the facet channels for the inner unit spec.
    let mut inner = spec.clone();
    if let Some(e) = inner.get_mut("encoding").and_then(Value::as_object_mut) {
        e.remove("row");
        e.remove("column");
    }
    Some(FacetSpec {
        row_field,
        col_field,
        inner,
    })
}

fn field_of(v: &Value) -> Option<String> {
    v.get("field").and_then(Value::as_str).map(str::to_owned)
}

fn render_facet(
    spec: &Value,
    f: FacetSpec,
    inherited: Option<&[Row]>,
) -> Result<Rendered, RasterError> {
    let rows = resolve_rows(spec, inherited)?;
    let facet_field = f.col_field.clone().or_else(|| f.row_field.clone());
    let by_column = f.col_field.is_some();
    let field = match facet_field {
        Some(f) => f,
        None => return unit::render_unit(&f.inner, &rows, None),
    };

    // distinct facet values, first-seen order.
    let mut keys: Vec<String> = Vec::new();
    for r in &rows {
        let k = super::data::cell_string(r.get(&field));
        super::data::index_of(&mut keys, &k);
    }
    super::data::sort_categories(&mut keys);

    let pad = 22.0;
    let title_h = 18.0;
    let mut pieces: Vec<(String, Rendered)> = Vec::new();
    for key in &keys {
        let sub: Vec<Row> = rows
            .iter()
            .filter(|r| &super::data::cell_string(r.get(&field)) == key)
            .cloned()
            .collect();
        // scale each facet down a bit for small multiples.
        let inner = scaled_inner(&f.inner, 0.62);
        let r = unit::render_unit(&inner, &sub, None)?;
        pieces.push((format!("{field} = {key}"), r));
    }

    let mut body = String::new();
    let (mut x, mut y) = (0.0_f64, 0.0_f64);
    let mut total_w = 0.0_f64;
    let mut total_h = 0.0_f64;
    for (label, p) in &pieces {
        let _ = write!(body, r##"<g transform="translate({x:.1},{y:.1})">"##);
        // per-facet header label
        let _ = write!(
            body,
            r##"<text x="{:.1}" y="13" font-size="12" font-weight="bold" text-anchor="middle" fill="#333">{}</text>"##,
            p.width / 2.0,
            svgutil::escape(label)
        );
        let _ = write!(body, r##"<g transform="translate(0,{title_h})">"##);
        body.push_str(&p.body);
        body.push_str("</g></g>");
        let panel_h = p.height + title_h;
        if by_column {
            x += p.width + pad;
            total_w = x - pad;
            total_h = total_h.max(panel_h);
        } else {
            y += panel_h + pad;
            total_h = y - pad;
            total_w = total_w.max(p.width);
        }
    }
    Ok(Rendered {
        body,
        width: total_w.max(1.0),
        height: total_h.max(1.0),
    })
}

/// Apply width/height overrides (as a fraction of the default) to a unit spec.
fn scaled_inner(inner: &Value, frac: f64) -> Value {
    let mut s = inner.clone();
    if let Some(o) = s.as_object_mut() {
        let w = inner
            .get("width")
            .and_then(Value::as_f64)
            .unwrap_or(super::DEFAULT_W - 214.0);
        let h = inner
            .get("height")
            .and_then(Value::as_f64)
            .unwrap_or(super::DEFAULT_H - 84.0);
        o.insert("width".to_owned(), Value::from((w * frac).round() as i64));
        o.insert("height".to_owned(), Value::from((h * frac).round() as i64));
    }
    s
}
