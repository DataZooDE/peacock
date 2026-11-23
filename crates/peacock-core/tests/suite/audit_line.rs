//! The audit record's shape (FR-O-4, #46): unit-level checks of the schema
//! the real-binary test (`peacock-bin` `audit.rs`) exercises end to end.

use std::time::{Duration, UNIX_EPOCH};

use peacock_core::audit::{AuditLine, AuditScope, Surface, rfc3339_utc};
use peacock_types::{Error, Principal};
use serde_json::json;

fn principal() -> Principal {
    Principal {
        sub: "alice".into(),
        scopes: vec![],
        groups: vec![],
        tenant: "acme".into(),
        raw_token: "SECRET-BEARER".into(),
        trace_id: String::new(),
    }
}

#[test]
fn timestamps_are_rfc3339_utc_millis() {
    assert_eq!(rfc3339_utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    let t = UNIX_EPOCH + Duration::from_millis(1_795_000_000_123);
    assert_eq!(rfc3339_utc(t), "2026-11-18T11:06:40.123Z");
    // A leap day.
    let t = UNIX_EPOCH + Duration::from_secs(951_782_400);
    assert_eq!(rfc3339_utc(t), "2000-02-29T00:00:00.000Z");
}

#[test]
fn a_failed_render_records_names_and_the_tag_only() {
    let scope = AuditScope::new(Surface::Triton).with_host(Some("teams".into()));
    let p = principal();
    let params = json!({ "year": 1997, "customer": "Jane Doe" });
    let err = Error::data("escurel said: customer Jane Doe not visible");
    let line = AuditLine::new(
        &scope,
        &p,
        Some("sales"),
        &params,
        Err(&err),
        Duration::from_millis(42),
    );
    let mut v = serde_json::to_value(&line).unwrap();
    assert!(v["when"].as_str().unwrap().ends_with('Z'));
    v.as_object_mut().unwrap().remove("when");
    assert_eq!(
        v,
        json!({
            "kind": "audit", "event": "render", "surface": "triton", "host": "teams",
            "tenant": "acme", "who": "alice", "report_id": "sales",
            "param_names": ["customer", "year"], "result": "error:data",
            "duration_ms": 42
        })
    );
    let s = v.to_string();
    assert!(!s.contains("Jane") && !s.contains("SECRET") && !s.contains("1997"));
}
