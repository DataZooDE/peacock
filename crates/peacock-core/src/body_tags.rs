//! Inline report tags — the "markdown body + tags" page format.
//!
//! A report skill's BODY may interleave narrative markdown with inline tags
//! that place views exactly where the author wants them, instead of the
//! frontmatter `views:` list rendering as one block followed by the narrative.
//! This is what lets a per-instance result page read like a document —
//! a summary, a sentence, a chart, a sentence, a table, then follow-up buttons.
//!
//! Syntax (deliberately small + forgiving): `{{ name (: primary)? (key=value)* }}`
//!
//! ```text
//! {{summary}}                                   a KPI/frontmatter header block
//! {{chart: rows spec=convergence}}              a `vega` view over data alias `rows`
//! {{table: rows}}                               a `table` view
//! {{kpi: rows field=best_score agg=min label="Best score"}}
//! {{followups}}                                 the follow-up buttons
//! ```
//!
//! `primary` is the leading bareword after the colon (usually a data/instance
//! alias); `value` may be bare or "double-quoted" (to allow spaces). A `{{` with
//! no matching `}}` is left as literal text — a report body that happens to
//! contain braces never fails to parse.
//!
//! This module is PURE (no escurel, no rendering) so the split is unit-tested in
//! isolation; `compose` consumes [`parse_body`] to interleave the components.

use std::collections::BTreeMap;

/// One piece of a parsed report body, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodySegment {
    /// A run of literal markdown between tags (may be empty; callers skip blanks).
    Markdown(String),
    /// An inline tag: a view/directive to expand at this position.
    Tag(BodyTag),
}

/// A parsed inline tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyTag {
    /// The directive name, lowercased: `summary`, `chart`, `table`, `kpi`,
    /// `markdown`, `frontmatter`, `timeline`, `followups`.
    pub name: String,
    /// The leading bareword after `:` (a data/instance alias), if any.
    pub primary: Option<String>,
    /// `key=value` arguments (values unquoted).
    pub args: BTreeMap<String, String>,
}

impl BodyTag {
    /// A convenience accessor for a positional/keyed alias: the primary
    /// bareword, else the `data`/`instance` arg — whichever a view kind wants.
    pub fn alias(&self) -> Option<&str> {
        self.primary
            .as_deref()
            .or_else(|| self.args.get("data").map(String::as_str))
            .or_else(|| self.args.get("instance").map(String::as_str))
    }
}

/// Split a report body into ordered markdown/tag segments. Never fails: an
/// unterminated `{{` is treated as literal text.
pub fn parse_body(body: &str) -> Vec<BodySegment> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        // Everything before the `{{` is literal markdown.
        let (before, after_open) = rest.split_at(open);
        // Find the matching `}}` in the remainder after the `{{`.
        let inner_start = &after_open[2..];
        match inner_start.find("}}") {
            Some(close) => {
                if !before.is_empty() {
                    out.push(BodySegment::Markdown(before.to_string()));
                }
                let raw = &inner_start[..close];
                match parse_tag(raw) {
                    Some(tag) => out.push(BodySegment::Tag(tag)),
                    // A `{{ }}` that isn't a recognizable tag (empty/garbage) is
                    // kept verbatim rather than silently dropped.
                    None => out.push(BodySegment::Markdown(format!("{{{{{raw}}}}}"))),
                }
                rest = &inner_start[close + 2..];
            }
            None => {
                // No closing `}}` anywhere — the rest is all literal.
                break;
            }
        }
    }
    if !rest.is_empty() {
        out.push(BodySegment::Markdown(rest.to_string()));
    }
    out
}

/// Parse the inside of a `{{ … }}` into a [`BodyTag`], or `None` when there is
/// no name.
fn parse_tag(raw: &str) -> Option<BodyTag> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Split off the name (and optional `:primary`) from the `key=value` tail.
    // The name is the first token; `:` (with or without surrounding space)
    // introduces the primary bareword.
    let (head, tail) = split_head(raw);
    let (name, primary) = match head.split_once(':') {
        Some((n, p)) => {
            let p = p.trim();
            (
                n.trim(),
                if p.is_empty() {
                    None
                } else {
                    Some(p.to_string())
                },
            )
        }
        None => (head.trim(), None),
    };
    if name.is_empty() {
        return None;
    }
    Some(BodyTag {
        name: name.to_ascii_lowercase(),
        primary,
        args: parse_args(tail),
    })
}

/// Split `raw` into the head (name + optional `:primary`, up to the first
/// `key=value`) and the argument tail. The head runs until the first token that
/// contains `=`.
fn split_head(raw: &str) -> (&str, &str) {
    // Walk tokens; the head ends at the first `key=value` token.
    let bytes = raw.as_bytes();
    let mut i = 0;
    let mut in_quote = false;
    // Find the byte index where the first `=`-bearing token starts.
    let mut token_start = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '"' {
            in_quote = !in_quote;
        } else if !in_quote && c.is_whitespace() {
            token_start = i + 1;
        } else if !in_quote && c == '=' {
            // The token beginning at `token_start` is a key=value arg.
            return (raw[..token_start].trim_end(), &raw[token_start..]);
        }
        i += 1;
    }
    (raw, "")
}

/// Parse `key=value key2="a b"` into a map (values unquoted).
fn parse_args(tail: &str) -> BTreeMap<String, String> {
    let mut args = BTreeMap::new();
    let bytes = tail.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace between args.
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        // Read the key up to `=`.
        let key_start = i;
        while i < bytes.len() && bytes[i] as char != '=' && !(bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] as char != '=' {
            break; // no `=`; stop (trailing garbage)
        }
        let key = tail[key_start..i].trim().to_string();
        i += 1; // skip '='
        // Read the value: quoted (to next `"`) or bare (to next whitespace).
        let value = if i < bytes.len() && bytes[i] as char == '"' {
            i += 1;
            let vs = i;
            while i < bytes.len() && bytes[i] as char != '"' {
                i += 1;
            }
            let v = tail[vs..i.min(bytes.len())].to_string();
            if i < bytes.len() {
                i += 1; // closing quote
            }
            v
        } else {
            let vs = i;
            while i < bytes.len() && !(bytes[i] as char).is_whitespace() {
                i += 1;
            }
            tail[vs..i].to_string()
        };
        if !key.is_empty() {
            args.insert(key, value);
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(segs: &[BodySegment], idx: usize) -> &BodyTag {
        match &segs[idx] {
            BodySegment::Tag(t) => t,
            other => panic!("segment {idx} is not a tag: {other:?}"),
        }
    }

    #[test]
    fn plain_body_is_one_markdown_segment() {
        let s = parse_body("just narrative, no tags.");
        assert_eq!(
            s,
            vec![BodySegment::Markdown("just narrative, no tags.".into())]
        );
    }

    #[test]
    fn interleaves_markdown_and_tags_in_order() {
        let s = parse_body("Intro.\n\n{{summary}}\n\nBody {{chart: rows spec=conv}} end.");
        assert_eq!(s.len(), 5);
        assert!(matches!(&s[0], BodySegment::Markdown(m) if m.contains("Intro.")));
        assert_eq!(tag(&s, 1).name, "summary");
        assert!(matches!(&s[2], BodySegment::Markdown(m) if m.contains("Body")));
        let chart = tag(&s, 3);
        assert_eq!(chart.name, "chart");
        assert_eq!(chart.primary.as_deref(), Some("rows"));
        assert_eq!(chart.args.get("spec").map(String::as_str), Some("conv"));
        assert!(matches!(&s[4], BodySegment::Markdown(m) if m.contains("end.")));
    }

    #[test]
    fn parses_primary_alias_and_quoted_value() {
        let s = parse_body(r#"{{kpi: rows field=best_score agg=min label="Best score"}}"#);
        let t = tag(&s, 0);
        assert_eq!(t.name, "kpi");
        assert_eq!(t.alias(), Some("rows"));
        assert_eq!(t.args.get("field").map(String::as_str), Some("best_score"));
        assert_eq!(t.args.get("agg").map(String::as_str), Some("min"));
        assert_eq!(t.args.get("label").map(String::as_str), Some("Best score"));
    }

    #[test]
    fn bare_name_no_args() {
        let t = tag(&parse_body("{{followups}}"), 0).clone();
        assert_eq!(t.name, "followups");
        assert!(t.primary.is_none());
        assert!(t.args.is_empty());
    }

    #[test]
    fn name_lowercased_and_alias_from_data_arg() {
        let t = tag(&parse_body("{{Table data=rows}}"), 0).clone();
        assert_eq!(t.name, "table");
        assert_eq!(t.alias(), Some("rows"));
    }

    #[test]
    fn unterminated_open_is_literal_text() {
        let s = parse_body("text {{ not closed");
        assert_eq!(s, vec![BodySegment::Markdown("text {{ not closed".into())]);
    }

    #[test]
    fn empty_tag_is_kept_verbatim_not_dropped() {
        let s = parse_body("a {{}} b");
        // "a ", literal "{{}}", " b"
        assert!(
            s.iter()
                .any(|seg| matches!(seg, BodySegment::Markdown(m) if m == "{{}}"))
        );
    }
}
