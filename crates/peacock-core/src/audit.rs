//! One structured audit line per render on stdout (FR-O-4, HLD §8.4, #46).
//!
//! Two stdout channels share the process output, told apart by JSON `kind`:
//! the diagnostic `tracing` log, and these `kind: "audit"` lines, which the
//! substrate collector ships (peacock never ships logs itself, ADR-P12).
//! Mirrors Triton's audit emitter (`triton-core/src/audit.rs`).
//!
//! The line records the render's OUTCOME only: report id, surface, host
//! flavor, tenant + subject, the caller's param NAMES, the result tag,
//! duration, the primary row count and the truncation when a view was capped.
//! Rows, cell values, param values, tokens and error messages never appear:
//! the schema has no field that could carry them, and the [`Principal`]'s
//! bearer is never read here.

use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use peacock_types::{Artifact, Error, Principal, Truncation};
use serde::Serialize;
use serde_json::Value;

/// The surface a render arrived on (`surface` in the audit line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// peacock's own MCP endpoint (`POST /mcp`, `tools/call render_report`).
    Mcp,
    /// The structured HTTP face (`POST /v1/render_report`).
    Http,
    /// The Triton upstream entry point (`POST /` + `X-Triton-Tool`).
    Triton,
    /// A server-seeded document page (the agent's `/docs/{token}` view).
    DocumentPage,
    /// A Rust caller linking `peacock-core` in-process (FR-E-1).
    Embedded,
}

impl Surface {
    /// The stable lowercase tag written to the audit line.
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Mcp => "mcp",
            Surface::Http => "http",
            Surface::Triton => "triton",
            Surface::DocumentPage => "document_page",
            Surface::Embedded => "embedded",
        }
    }
}

/// Where a render's audit line says it came from. Set on
/// [`crate::RenderOpts::audit`]; the default is an embedded render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditScope {
    pub surface: Surface,
    /// The host flavor the render was themed for (`copilot`, `teams`, …),
    /// when it is a KNOWN flavor. Callers pass only registry names, never a
    /// raw header, so the field stays a closed, low-cardinality set.
    pub host: Option<String>,
}

impl AuditScope {
    /// A scope for `surface` with no host flavor.
    pub fn new(surface: Surface) -> Self {
        Self {
            surface,
            host: None,
        }
    }

    /// The same scope with a host flavor.
    pub fn with_host(mut self, host: Option<String>) -> Self {
        self.host = host;
        self
    }
}

impl Default for AuditScope {
    fn default() -> Self {
        Self::new(Surface::Embedded)
    }
}

/// The audit record, as serialized. Every field is an identifier, a count or
/// a closed tag — nothing a row, a param value or a token could ride in.
#[derive(Debug, Serialize)]
pub struct AuditLine<'a> {
    /// Always `"audit"`: separates these lines from the `tracing` log.
    pub kind: &'static str,
    /// Always `"render"`.
    pub event: &'static str,
    /// RFC 3339 UTC, millisecond precision.
    pub when: String,
    pub surface: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<&'a str>,
    pub tenant: &'a str,
    /// The principal's subject (FR-O-4 `who`).
    pub who: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<&'a str>,
    /// Absent only when the request named no report at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_id: Option<&'a str>,
    /// The caller's param NAMES, sorted. Values are never recorded.
    pub param_names: Vec<&'a str>,
    /// `ok`, or `error:<tag>` with the [`Error::kind`] tag.
    pub result: String,
    pub duration_ms: u64,
    /// The primary view's row count (successful renders only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<usize>,
    /// The cut, when a view was capped (#46).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<&'a Truncation>,
}

impl<'a> AuditLine<'a> {
    /// Build the record for one render's outcome.
    pub fn new(
        scope: &'a AuditScope,
        principal: &'a Principal,
        report_id: Option<&'a str>,
        params: &'a Value,
        outcome: std::result::Result<&'a Artifact, &Error>,
        elapsed: Duration,
    ) -> Self {
        let mut param_names: Vec<&str> = params
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        param_names.sort_unstable();
        let (result, rows, truncated) = match outcome {
            Ok(art) => (
                "ok".to_owned(),
                Some(
                    art.structured_content
                        .rows
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or(0),
                ),
                art.structured_content.truncated.as_ref(),
            ),
            Err(e) => (format!("error:{}", e.kind()), None, None),
        };
        Self {
            kind: "audit",
            event: "render",
            when: rfc3339_utc(SystemTime::now()),
            surface: scope.surface.as_str(),
            host: scope.host.as_deref().filter(|h| !h.is_empty()),
            tenant: &principal.tenant,
            who: &principal.sub,
            trace_id: Some(principal.trace_id.as_str()).filter(|t| !t.is_empty()),
            report_id,
            param_names,
            result,
            duration_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
            rows,
            truncated,
        }
    }
}

/// Write one audit line to stdout. Locked + flushed so concurrent renders
/// never interleave mid-line with each other or with the `tracing` log.
/// Best-effort: a failed stdout write cannot be reported anywhere better.
pub fn emit(line: &AuditLine<'_>) {
    let Ok(mut buf) = serde_json::to_vec(line) else {
        return;
    };
    buf.push(b'\n');
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(&buf);
    let _ = handle.flush();
}

/// Audit a render that failed before it reached the render core (a missing
/// `report_id`, a bad look argument), so a refused request is not silent.
pub fn emit_refused(
    scope: &AuditScope,
    principal: &Principal,
    report_id: Option<&str>,
    params: &Value,
    err: &Error,
) {
    emit(&AuditLine::new(
        scope,
        principal,
        report_id,
        params,
        Err(err),
        Duration::ZERO,
    ));
}

/// `SystemTime` → `YYYY-MM-DDTHH:MM:SS.mmmZ` (UTC), without a date crate.
pub fn rfc3339_utc(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, day) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian
/// (H. Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}
