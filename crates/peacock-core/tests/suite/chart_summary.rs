//! The neutral chart summary (`_meta.chart` v1, Triton contract §4b, triton#375):
//! the primary chart described without any renderer, derived from the anofox
//! role spec, so Triton can map it to a channel's native chart. Pure — an
//! authored spec plus rows through `panel_spec`, exactly as a render builds it.

use peacock_core::chart::{CHART_SUMMARY_MAX_ROWS, chart_summary, panel_spec};
use peacock_core::data::{Column, RowSet};
use serde_json::{Value, json};

fn rows(cols: &[(&str, &str)], rows: Value) -> RowSet {
    RowSet {
        rows,
        schema: cols
            .iter()
            .map(|(n, t)| Column {
                name: (*n).into(),
                type_name: (*t).into(),
            })
            .collect(),
        truncated: false,
    }
}

fn summary(spec: Value, rs: &RowSet) -> Option<Value> {
    chart_summary(&panel_spec(&spec, rs).expect("a valid spec"))
}

fn nw() -> RowSet {
    rows(
        &[
            ("month", "DATE"),
            ("category", "VARCHAR"),
            ("revenue", "DOUBLE"),
        ],
        json!([
            { "month": "1997-01-01", "category": "Beverages", "revenue": 180.5 },
            { "month": "1997-02-01", "category": "Beverages", "revenue": 81.0 },
            { "month": "1997-01-01", "category": "Condiments", "revenue": 110.0 }
        ]),
    )
}

#[test]
fn a_stacked_bar_with_series_summarises_every_contract_key() {
    let s = summary(
        json!({ "title": "Revenue by category", "roles": [
            ["month", "XAXIS"], ["category", "CATEGORY"], ["revenue", "BARCHART_STACKED", "Revenue"]
        ]}),
        &nw(),
    )
    .expect("a bar summarises");
    assert_eq!(s["kind"], "bar");
    assert_eq!(s["title"], "Revenue by category");
    assert_eq!(s["x"], "month");
    assert_eq!(s["y"], "Revenue");
    assert_eq!(s["series"], "category");
    assert_eq!(s["stacked"], true);
    assert_eq!(s["orientation"], "vertical");
    assert_eq!(
        s["rows"][0],
        json!({ "month": "1997-01-01", "category": "Beverages", "Revenue": 180.5 })
    );
    assert_eq!(s["rows"].as_array().unwrap().len(), 3);
}

#[test]
fn a_grouped_bar_is_not_stacked_and_a_line_has_no_bar_keys() {
    let spec = |kind: &str| json!({ "roles": [["month", "XAXIS"], ["category", "CATEGORY"], ["revenue", kind, "Revenue"]] });
    let bar = summary(spec("BARCHART"), &nw()).unwrap();
    assert_eq!(bar["stacked"], false);

    let line = summary(spec("LINECHART"), &nw()).unwrap();
    assert_eq!(line["kind"], "line");
    assert_eq!(line["series"], "category");
    assert!(line.get("stacked").is_none() && line.get("orientation").is_none());
    assert!(line.get("title").is_none(), "no TITLE role, no title key");

    let area = summary(spec("AREACHART_STACKED"), &nw()).unwrap();
    assert_eq!(area["kind"], "area");
}

#[test]
fn a_single_series_bar_carries_no_stacked_key() {
    let rs = rows(
        &[("product", "VARCHAR"), ("revenue", "DOUBLE")],
        json!([{ "product": "Chai", "revenue": 10.0 }, { "product": "Tofu", "revenue": 4.0 }]),
    );
    let s = summary(
        json!({ "roles": [["product", "XAXIS"], ["revenue", "BARCHART", "Revenue"]] }),
        &rs,
    )
    .unwrap();
    assert!(s.get("series").is_none() && s.get("stacked").is_none());
}

#[test]
fn flip_makes_a_horizontal_bar_and_modifiers_supply_unit_and_axis_titles() {
    let rs = rows(
        &[
            ("product", "VARCHAR"),
            ("revenue", "DOUBLE"),
            ("flip", "BOOLEAN"),
            ("fmt", "VARCHAR"),
            ("xt", "VARCHAR"),
            ("yt", "VARCHAR"),
        ],
        json!([
            { "product": "Chai", "revenue": 10.0, "flip": true, "fmt": "€", "xt": "Product", "yt": "Net revenue" },
            { "product": "Tofu", "revenue": 4.0, "flip": true, "fmt": "€", "xt": "Product", "yt": "Net revenue" }
        ]),
    );
    let s = summary(
        json!({ "roles": [
            ["product", "XAXIS"], ["revenue", "BARCHART", "Revenue"],
            ["flip", "FLIP"], ["fmt", "YFORMAT"], ["xt", "XTITLE"], ["yt", "YTITLE"]
        ]}),
        &rs,
    )
    .unwrap();
    assert_eq!(s["orientation"], "horizontal");
    assert_eq!(s["unit"], "€");
    assert_eq!(s["x_title"], "Product");
    assert_eq!(s["y_title"], "Net revenue");
    // Modifier columns never leak into the summary rows.
    assert_eq!(s["rows"][0], json!({ "product": "Chai", "Revenue": 10.0 }));
}

#[test]
fn the_unit_comes_from_the_value_format_or_the_currency_code() {
    let run = |role: &str, value: &str| {
        let rs = rows(
            &[("p", "VARCHAR"), ("v", "DOUBLE"), ("m", "VARCHAR")],
            json!([{ "p": "a", "v": 1.0, "m": value }]),
        );
        summary(
            json!({ "roles": [["p", "XAXIS"], ["v", "BARCHART"], ["m", role]] }),
            &rs,
        )
        .unwrap()
    };
    assert_eq!(run("YFORMAT", " kg")["unit"], "kg");
    assert_eq!(run("YFORMAT", "percent")["unit"], "%");
    assert!(
        run("YFORMAT", "comma").get("unit").is_none(),
        "a pure format"
    );
    assert_eq!(run("CURRENCY_CODE", "EUR")["unit"], "EUR");
}

#[test]
fn pie_and_donut_take_the_category_as_x_and_carry_no_series() {
    let rs = rows(
        &[("country", "VARCHAR"), ("revenue", "DOUBLE")],
        json!([{ "country": "Germany", "revenue": 50.0 }, { "country": "France", "revenue": 20.0 }]),
    );
    let s = summary(
        json!({ "title": "Share", "roles": [["country", "CATEGORY"], ["revenue", "DONUT", "Revenue"]] }),
        &rs,
    )
    .unwrap();
    assert_eq!(s["kind"], "donut");
    assert_eq!(s["x"], "country");
    assert_eq!(s["y"], "Revenue");
    assert!(s.get("series").is_none() && s.get("orientation").is_none());
    let pie = summary(
        json!({ "roles": [["country", "CATEGORY"], ["revenue", "PIE"]] }),
        &rs,
    )
    .unwrap();
    assert_eq!(pie["kind"], "pie");
}

#[test]
fn a_scatter_needs_numeric_x_and_ignores_the_size_channel() {
    let rs = rows(
        &[
            ("d", "DOUBLE"),
            ("rev", "DOUBLE"),
            ("q", "INTEGER"),
            ("c", "VARCHAR"),
        ],
        json!([
            { "d": 5.0, "rev": 100.0, "q": 3, "c": "Beverages" },
            { "d": "n/a", "rev": 50.0, "q": 1, "c": "Produce" },
            { "d": "15", "rev": 70.0, "q": 2, "c": "Produce" }
        ]),
    );
    let s = summary(
        json!({ "roles": [["d", "XAXIS", "Discount %"], ["rev", "SCATTER", "Line revenue"],
                          ["q", "SIZE", "Quantity"], ["c", "CATEGORY"]] }),
        &rs,
    )
    .unwrap();
    assert_eq!(s["kind"], "scatter");
    assert_eq!(s["series"], "c");
    let pts = s["rows"].as_array().unwrap();
    assert_eq!(pts.len(), 2, "the non-numeric x is dropped: {pts:?}");
    assert_eq!(pts[1]["Discount %"], 15.0, "a numeric string x is a number");
    assert!(pts[0].get("Quantity").is_none());
}

#[test]
fn null_and_non_numeric_points_are_dropped_and_rows_cap_at_200() {
    let mut data: Vec<Value> = (0..250)
        .map(|i| json!({ "month": format!("m{i:03}"), "category": "A", "revenue": i as f64 }))
        .collect();
    data[0]["revenue"] = Value::Null;
    data[1]["revenue"] = json!("not a number");
    data[2]["month"] = Value::Null;
    data[3]["category"] = Value::Null;
    data[4]["revenue"] = json!("12.5");
    let rs = rows(
        &[
            ("month", "VARCHAR"),
            ("category", "VARCHAR"),
            ("revenue", "DOUBLE"),
        ],
        Value::Array(data),
    );
    let s = summary(
        json!({ "roles": [["month", "XAXIS"], ["category", "CATEGORY"], ["revenue", "LINECHART", "Revenue"]] }),
        &rs,
    )
    .unwrap();
    let pts = s["rows"].as_array().unwrap();
    assert_eq!(pts.len(), CHART_SUMMARY_MAX_ROWS);
    assert_eq!(CHART_SUMMARY_MAX_ROWS, 200);
    assert_eq!(pts[0]["month"], "m004", "the four broken points are gone");
    assert_eq!(pts[0]["Revenue"], 12.5, "a numeric string y is a number");
    assert!(
        pts.iter()
            .all(|p| p["Revenue"].is_number() && !p["month"].is_null() && !p["category"].is_null())
    );
}

#[test]
fn charts_the_contract_cannot_express_have_no_summary() {
    // A heatmap (second axis), a histogram (binned), a second measure.
    let heat = summary(
        json!({ "roles": [["month", "XAXIS"], ["category", "YAXIS"], ["revenue", "HEATMAP"]] }),
        &nw(),
    );
    assert!(heat.is_none());
    let hist = summary(json!({ "roles": [["revenue", "HISTOGRAM"]] }), &nw());
    assert!(hist.is_none());
    let two = rows(
        &[("p", "VARCHAR"), ("a", "DOUBLE"), ("b", "DOUBLE")],
        json!([{ "p": "x", "a": 1.0, "b": 2.0 }]),
    );
    let two_measures = summary(
        json!({ "roles": [["p", "XAXIS"], ["a", "BARCHART"], ["b", "LINECHART"]] }),
        &two,
    );
    assert!(two_measures.is_none());
    // No valid point at all: rows must hold 1–200 objects, so no summary.
    let empty = rows(
        &[("p", "VARCHAR"), ("v", "DOUBLE")],
        json!([{ "p": "x", "v": null }]),
    );
    assert!(
        summary(
            json!({ "roles": [["p", "XAXIS"], ["v", "BARCHART"]] }),
            &empty
        )
        .is_none()
    );
}

#[test]
fn colliding_field_names_have_no_summary() {
    // The x display name equals the value's: one row key cannot hold both.
    let s = summary(
        json!({ "roles": [["month", "XAXIS", "v"], ["revenue", "BARCHART", "v"]] }),
        &nw(),
    );
    assert!(s.is_none());
}
