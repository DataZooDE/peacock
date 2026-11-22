//! Phase 2 red→green contract tests for `peacock-types`.
//!
//! These pin the four-variant error model (HLD §8.3, FR-R-5), the
//! Triton-wire-identical `Principal` (FR-I-1), and the `Artifact` /
//! `ParamSchema` serde shapes (FR-R-3, FR-X-1).

use peacock_types::{
    Artifact, Error, ParamSchema, ParamSpec, ParamType, ParamValue, Principal, StructuredContent,
    Truncation,
};
use serde_json::{Value, json};

// ── error model: the variant decides the mapping, never the message ──

#[test]
fn error_variants_map_to_stable_jsonrpc_codes() {
    // Mirrors Triton's codes: auth -32001, validation -32602, server -32000.
    assert_eq!(Error::auth("nope").jsonrpc_code(), -32001);
    assert_eq!(Error::validation("bad param").jsonrpc_code(), -32602);
    assert_eq!(Error::data("escurel timeout").jsonrpc_code(), -32000);
    assert_eq!(Error::render("remote data url").jsonrpc_code(), -32000);
}

#[test]
fn error_kind_is_decided_by_variant_not_message() {
    // Two errors with identical messages but different variants must map
    // differently — surfaces switch on the variant, never the string.
    let a = Error::data("boom");
    let b = Error::render("boom");
    assert_eq!(a.kind(), "data");
    assert_eq!(b.kind(), "render");
    assert_ne!(a.kind(), b.kind());
    assert_eq!(Error::auth("x").kind(), "auth");
    assert_eq!(Error::validation("x").kind(), "validation");
}

#[test]
fn error_is_std_error_and_displays_with_kind_prefix() {
    let e = Error::validation("param `from` is not a date");
    // Display carries the kind prefix (audit/log friendly), like Triton.
    assert!(e.to_string().starts_with("validation:"));
    let _: &dyn std::error::Error = &e;
}

// ── Principal: wire-identical to Triton's `{sub,scopes,groups,tenant,trace_id}` ──

#[test]
fn principal_serializes_triton_shape_and_redacts_token() {
    let p = Principal {
        sub: "user-1".into(),
        scopes: vec!["read".into()],
        groups: vec!["sales".into()],
        tenant: "acme".into(),
        raw_token: "super-secret-bearer".into(),
        trace_id: "trace-xyz".into(),
    };
    let v: Value = serde_json::to_value(&p).unwrap();
    assert_eq!(v["sub"], "user-1");
    assert_eq!(v["scopes"], json!(["read"]));
    assert_eq!(v["groups"], json!(["sales"]));
    assert_eq!(v["tenant"], "acme");
    assert_eq!(v["trace_id"], "trace-xyz");
    // The bearer must NEVER appear in the serialized form (NFR-S-4).
    assert!(v.get("raw_token").is_none());
    assert!(
        !serde_json::to_string(&p)
            .unwrap()
            .contains("super-secret-bearer")
    );
}

#[test]
fn principal_debug_does_not_leak_token() {
    let p = Principal {
        sub: "u".into(),
        scopes: vec![],
        groups: vec![],
        tenant: "t".into(),
        raw_token: "leaky-token-value".into(),
        trace_id: "tr".into(),
    };
    assert!(!format!("{p:?}").contains("leaky-token-value"));
}

// ── ParamSchema / ParamValue (FR-R-4, FR-D-6 type checking) ──

#[test]
fn param_schema_round_trips_and_typechecks_values() {
    let schema = ParamSchema::from_specs([
        (
            "from",
            ParamSpec::new(ParamType::Date).with_default(json!("1997-01-01")),
        ),
        (
            "to",
            ParamSpec::new(ParamType::Date).with_default(json!("1997-12-31")),
        ),
        (
            "category",
            ParamSpec::new(ParamType::String).with_default(json!("ALL")),
        ),
    ]);

    // serde round-trip preserves the schema.
    let back: ParamSchema = serde_json::from_value(serde_json::to_value(&schema).unwrap()).unwrap();
    assert_eq!(back, schema);

    // A string for a date param is rejected pre-call (FR-D-6 defense in depth).
    let bad = schema.validate(&json!({ "from": 42, "to": "1997-12-31", "category": "ALL" }));
    assert!(matches!(bad, Err(Error::Validation(_))));

    // A well-typed, defaulted param vector validates and fills defaults.
    let ok = schema
        .validate_and_default(&json!({ "category": "Beverages" }))
        .expect("valid params");
    assert_eq!(
        ok.get("from").unwrap(),
        &ParamValue::from(json!("1997-01-01"))
    );
    assert_eq!(
        ok.get("category").unwrap(),
        &ParamValue::from(json!("Beverages"))
    );
}

// ── Artifact / StructuredContent (FR-R-3, FR-X-1) ──

#[test]
fn artifact_carries_a2ui_charts_structured_and_optional_png() {
    let sc = StructuredContent {
        rows: json!([{ "month": "1997-01-01", "category": "Beverages", "revenue": 100 }]),
        param_schema: json!({ "category": { "type": "string" } }),
        current_params: json!({ "category": "Beverages", "from": "1997-01-01" }),
        instances: None,
        document: None,
        truncated: None,
    };
    let panel = json!({ "rows": [{ "c0": "1997-01", "c1": 100 }], "roles": [[0, "XAXIS", "month"], [1, "LINECHART", "revenue"]] });
    let art = Artifact {
        a2ui: json!({ "version": "0.9", "components": [
            { "kind": "anofox", "spec": panel, "svg": "<svg/>" },
            { "kind": "table", "rows": [] }
        ] }),
        anofox_specs: vec![panel],
        structured_content: sc,
        png: None,
        pngs: Vec::new(),
    };
    let v = serde_json::to_value(&art).unwrap();
    assert_eq!(v["a2ui"]["version"], "0.9");
    // The derived Vega-Lite is gone (triton#382).
    assert!(v.get("vega_specs").is_none());
    assert_eq!(v["anofox_specs"][0]["roles"][1][1], "LINECHART");
    // Per-chart PNGs never ride the serialized artifact.
    assert!(v.get("pngs").is_none());
    assert_eq!(art.chart_svgs(), vec!["<svg/>"]);
    // The current resolved params ride structuredContent (FR-X-1).
    assert_eq!(
        v["structured_content"]["current_params"]["category"],
        "Beverages"
    );
    assert!(art.png.is_none());
}

// ── row truncation (#46): the artifact says when rows were capped ──

#[test]
fn structured_content_reports_truncation_only_when_rows_were_capped() {
    let mut sc = StructuredContent {
        rows: json!([]),
        param_schema: json!({}),
        current_params: json!({}),
        instances: None,
        document: None,
        truncated: None,
    };
    // Untruncated: no key at all (existing artifacts stay byte-identical).
    assert!(
        serde_json::to_value(&sc)
            .unwrap()
            .get("truncated")
            .is_none()
    );

    // peacock's own cap knows the total.
    sc.truncated = Some(Truncation {
        rows_shown: 10_000,
        rows_total: Some(12_345),
        limit: 10_000,
        view: "lines".into(),
    });
    assert_eq!(
        serde_json::to_value(&sc).unwrap()["truncated"],
        json!({ "rows_shown": 10_000, "rows_total": 12_345, "limit": 10_000, "view": "lines" })
    );

    // escurel's server cap does not report a total: the key is omitted.
    sc.truncated = Some(Truncation {
        rows_shown: 10_000,
        rows_total: None,
        limit: 10_000,
        view: "lines".into(),
    });
    let v = serde_json::to_value(&sc).unwrap();
    assert!(v["truncated"].get("rows_total").is_none());
    let back: StructuredContent = serde_json::from_value(v).unwrap();
    assert_eq!(back, sc);
}
