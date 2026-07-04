//! Report-authoring tooling (BRD §7 "authoring tooling" deferral; HLD §170
//! "embedded library face … authoring preview").
//!
//! An author writes a report skill as escurel markdown — YAML front matter
//! (`params` / `data` / `views` / `specs`) + a narrative body — and publishes
//! it to escurel. This module lets them catch errors *before* publishing, by
//! reusing peacock's **own** parser ([`ReportSkill::from_frontmatter`]) and the
//! render guardrail ([`crate::guardrail::check_vega_spec`]) so the rules the
//! renderer enforces are the rules the author is checked against — no second,
//! drifting implementation.
//!
//! Three operations back the `peacock author` subcommands:
//! - [`validate_skill_markdown`] — split + parse + guardrail + cross-reference
//!   checks, returning every problem found (not just the first), each with the
//!   1-based source line where it can be located;
//! - [`scaffold`] — emit a minimal valid template that [`validate_skill_markdown`]
//!   accepts;
//! - the preview path reuses [`crate::render`] directly (it needs a real
//!   escurel binding, so it lives in the binary).
//!
//! No credential, no SQL, no escurel needed for validate/scaffold (the trust
//! boundary holds: this is pure parsing of author-supplied text).

use serde_json::Value;

use crate::guardrail::check_vega_spec;
use crate::skill::{ReportSkill, ViewSpec};

/// One author-facing problem, with the 1-based source line it can be located
/// at when known (`0` ⇒ no specific line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorError {
    pub line: usize,
    pub message: String,
}

impl AuthorError {
    fn at(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
    fn nowhere(message: impl Into<String>) -> Self {
        Self::at(0, message)
    }
}

impl std::fmt::Display for AuthorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

/// A skill split into its raw front matter (YAML) and narrative body, plus the
/// 1-based line on which the front matter starts (for error reporting).
pub struct SplitSkill<'a> {
    pub frontmatter: &'a str,
    pub body: &'a str,
    /// Source line of the first front-matter content line (after the opening
    /// `---`).
    pub frontmatter_start_line: usize,
}

/// Split an escurel skill markdown into `---`-fenced YAML front matter and the
/// body. Mirrors how escurel itself fences a page; the leading `---` may be
/// preceded only by whitespace.
pub fn split_frontmatter(text: &str) -> Result<SplitSkill<'_>, AuthorError> {
    // Tolerate a UTF-8 BOM and blank lines before the opening fence.
    let mut rest = text.trim_start_matches('\u{feff}');
    let mut skipped_lines = 0usize;
    while let Some(stripped) = rest.strip_prefix('\n') {
        rest = stripped;
        skipped_lines += 1;
    }

    let after_open = rest
        .strip_prefix("---\n")
        .or_else(|| rest.strip_prefix("---\r\n"))
        .ok_or_else(|| {
            AuthorError::at(
                1,
                "missing opening `---` front-matter fence (a report skill is YAML \
                 front matter then a markdown body)",
            )
        })?;

    // The opening fence sits on the line after any skipped blanks.
    let fence_line = skipped_lines + 1;

    // Find the closing `---` on its own line.
    let close_rel = find_closing_fence(after_open)
        .ok_or_else(|| AuthorError::at(fence_line, "missing closing `---` front-matter fence"))?;
    let frontmatter = &after_open[..close_rel.start];
    let body = &after_open[close_rel.end..];

    Ok(SplitSkill {
        frontmatter,
        body,
        frontmatter_start_line: fence_line + 1,
    })
}

/// Byte range of the closing `---` fence line within `s` (which begins just
/// after the opening fence).
struct FenceSpan {
    start: usize,
    end: usize,
}

fn find_closing_fence(s: &str) -> Option<FenceSpan> {
    let mut offset = 0usize;
    for line in s.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        if content == "---" {
            return Some(FenceSpan {
                start: offset,
                end: offset + line.len(),
            });
        }
        offset += line.len();
    }
    None
}

/// Parse YAML front matter into a `serde_json::Value` so it can feed
/// [`ReportSkill::from_frontmatter`] (peacock's canonical parser).
pub fn frontmatter_to_json(frontmatter: &str) -> Result<Value, AuthorError> {
    let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(frontmatter)
        .map_err(|e| AuthorError::nowhere(format!("front matter is not valid YAML: {e}")))?;
    serde_json::to_value(yaml).map_err(|e| {
        AuthorError::nowhere(format!("front matter cannot be represented as JSON: {e}"))
    })
}

/// Parse a skill markdown string into a typed [`ReportSkill`] using peacock's
/// own parser, surfacing the failure as an [`AuthorError`].
pub fn parse_skill_markdown(text: &str) -> Result<ReportSkill, AuthorError> {
    let split = split_frontmatter(text)?;
    let fm = frontmatter_to_json(split.frontmatter)?;
    let id = fm
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
        .to_owned();
    ReportSkill::from_frontmatter(&id, &fm, split.body)
        .map_err(|e| AuthorError::at(split.frontmatter_start_line, e.to_string()))
}

/// Validate a report-skill markdown end to end and return **every** problem
/// found (empty ⇒ valid). The checks, in order:
///
/// 1. front-matter split + parse via [`ReportSkill::from_frontmatter`];
/// 2. the inline-data-only Vega-Lite guardrail against each named spec;
/// 3. cross-references: every `data:` alias is used by some view, every view's
///    `data` names a declared alias, and every view `spec` / `spec_single`
///    exists in `specs:`;
/// 4. params sanity: a declared default must match its declared type.
pub fn validate_skill_markdown(text: &str) -> Vec<AuthorError> {
    let split = match split_frontmatter(text) {
        Ok(s) => s,
        Err(e) => return vec![e],
    };
    let line0 = split.frontmatter_start_line;

    let fm = match frontmatter_to_json(split.frontmatter) {
        Ok(v) => v,
        Err(e) => return vec![e],
    };
    let id = fm
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
        .to_owned();

    let skill = match ReportSkill::from_frontmatter(&id, &fm, split.body) {
        Ok(s) => s,
        // The structural parser stops at the first error; report it on the
        // front-matter line and bail (later checks would all be noise).
        Err(e) => return vec![AuthorError::at(line0, e.to_string())],
    };

    let mut errors = Vec::new();

    // 2. Guardrail each named spec (the exact check the renderer runs).
    for (name, spec) in &skill.specs {
        if let Err(e) = check_vega_spec(spec) {
            errors.push(AuthorError::at(
                spec_line(&fm, text, name),
                format!("spec `{name}`: {e}"),
            ));
        }
    }

    // 3. Cross-references between data / instances / views / specs.
    let declared_aliases: std::collections::BTreeSet<&str> =
        skill.data.keys().map(String::as_str).collect();
    let declared_instances: std::collections::BTreeSet<&str> =
        skill.instances.keys().map(String::as_str).collect();
    let mut used_aliases = std::collections::BTreeSet::new();
    let mut used_instances = std::collections::BTreeSet::new();

    for view in &skill.views {
        let (data, specs) = view_refs(view);
        used_aliases.insert(data);
        if !data.is_empty() && !declared_aliases.contains(data) {
            errors.push(AuthorError::at(
                line0,
                format!("a view references data alias `{data}`, which is not declared in `data:`"),
            ));
        }
        if let Some(instance) = view_instance_ref(view) {
            used_instances.insert(instance);
            if !declared_instances.contains(instance) {
                errors.push(AuthorError::at(
                    line0,
                    format!(
                        "a view references instance alias `{instance}`, \
                         which is not declared in `instances:`"
                    ),
                ));
            }
        }
        for spec_name in specs {
            if !skill.specs.contains_key(spec_name) {
                errors.push(AuthorError::at(
                    line0,
                    format!("a view names spec `{spec_name}`, which is not defined in `specs:`"),
                ));
            }
        }
    }

    for alias in &declared_aliases {
        if !used_aliases.contains(*alias) {
            errors.push(AuthorError::at(
                line0,
                format!("data alias `{alias}` is declared but no view references it"),
            ));
        }
    }
    for alias in &declared_instances {
        if !used_instances.contains(*alias) {
            errors.push(AuthorError::at(
                line0,
                format!("instance alias `{alias}` is declared but no view references it"),
            ));
        }
    }

    // 4. Params: a declared default must satisfy its declared type.
    for (name, spec) in &skill.params.0 {
        if let Some(default) = &spec.default {
            let single = serde_json::json!({ name.clone(): default.clone() });
            if let Err(e) = skill.params.validate(&single) {
                errors.push(AuthorError::at(
                    line0,
                    format!("param `{name}` default is invalid: {e}"),
                ));
            }
        }
    }

    errors
}

/// The `(data alias, [spec names])` a view references. Instance views carry
/// no data alias — theirs is [`view_instance_ref`].
fn view_refs(view: &ViewSpec) -> (&str, Vec<&str>) {
    match view {
        ViewSpec::Kpi { data, .. } => (data.as_str(), Vec::new()),
        ViewSpec::Table { data } => (data.as_str(), Vec::new()),
        ViewSpec::Vega {
            data,
            spec,
            spec_single,
        } => {
            let mut specs = vec![spec.as_str()];
            if let Some(s) = spec_single {
                specs.push(s.as_str());
            }
            (data.as_str(), specs)
        }
        ViewSpec::Markdown { .. } | ViewSpec::Frontmatter { .. } | ViewSpec::Timeline { .. } => {
            ("", Vec::new())
        }
    }
}

/// The `instances:` alias an instance view references, if any.
fn view_instance_ref(view: &ViewSpec) -> Option<&str> {
    match view {
        ViewSpec::Markdown { instance } => Some(instance.as_str()),
        ViewSpec::Frontmatter { instance, .. } => Some(instance.as_str()),
        ViewSpec::Timeline { instance, .. } => Some(instance.as_str()),
        _ => None,
    }
}

/// Best-effort source line of a `specs.<name>:` key, so a guardrail violation
/// points the author near the offending spec. Falls back to the front-matter
/// line when not found.
fn spec_line(fm: &Value, text: &str, name: &str) -> usize {
    if fm.get("specs").and_then(Value::as_object).is_none() {
        return 0;
    }
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with(&format!("{name}:")) || t.starts_with(&format!("{name} :")) {
            return i + 1;
        }
    }
    0
}

/// Emit a minimal, valid report-skill markdown template for `report_id` — a
/// single KPI + bar chart over one query, with one parameter. The output is
/// accepted by [`validate_skill_markdown`] (asserted by an integration test).
pub fn scaffold(report_id: &str) -> String {
    format!(
        r#"---
type: skill
id: {report_id}
render: a2ui
description: TODO one-line description of this report.
params:
  from: {{ type: date, default: "1997-01-01" }}
  to:   {{ type: date, default: "1997-12-31" }}
data:
  # Bind one alias to an escurel query page (a `[[query::<id>]]` instance).
  rows: "[[query::TODO_query_id]]"
views:
  - {{ kind: kpi,   data: rows, agg: sum, field: value, label: "Total" }}
  - {{ kind: vega,  data: rows, spec: main_chart }}
  - {{ kind: table, data: rows }}
specs:
  main_chart:
    mark: bar
    encoding:
      x: {{ field: label, type: ordinal,      title: Label }}
      y: {{ field: value, type: quantitative, title: Value }}
---
TODO: the agent-authored narrative for {report_id}.
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> &'static str {
        r#"---
type: skill
id: demo
render: a2ui
data:
  rows: "[[query::q]]"
views:
  - { kind: vega, data: rows, spec: chart }
specs:
  chart:
    mark: bar
    encoding:
      x: { field: a, type: ordinal }
      y: { field: b, type: quantitative }
---
narrative
"#
    }

    #[test]
    fn valid_skill_has_no_errors() {
        assert!(validate_skill_markdown(sample()).is_empty());
    }

    #[test]
    fn scaffold_round_trips_through_validate() {
        let md = scaffold("my-report");
        let errs = validate_skill_markdown(&md);
        assert!(errs.is_empty(), "scaffold should validate: {errs:?}");
        assert!(md.contains("my-report"));
    }

    #[test]
    fn missing_spec_is_reported() {
        let bad = sample().replace("spec: chart", "spec: nope");
        let errs = validate_skill_markdown(&bad);
        assert!(
            errs.iter().any(|e| e.message.contains("nope")),
            "got: {errs:?}"
        );
    }

    #[test]
    fn remote_url_in_spec_is_reported() {
        let bad = sample().replace("mark: bar", "data: { url: \"http://x\" }\n    mark: bar");
        let errs = validate_skill_markdown(&bad);
        assert!(
            errs.iter().any(|e| e.message.contains("url")),
            "got: {errs:?}"
        );
    }

    #[test]
    fn unused_data_alias_is_reported() {
        let bad = sample().replace(
            "  rows: \"[[query::q]]\"",
            "  rows: \"[[query::q]]\"\n  orphan: \"[[query::z]]\"",
        );
        let errs = validate_skill_markdown(&bad);
        assert!(
            errs.iter().any(|e| e.message.contains("orphan")),
            "got: {errs:?}"
        );
    }

    #[test]
    fn missing_open_fence_is_reported() {
        let errs = validate_skill_markdown("no front matter here\n");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("opening"));
    }
}
