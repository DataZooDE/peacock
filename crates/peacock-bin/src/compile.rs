//! `peacock author compile` (issue #44, Phase 3): an anofox `dashboard.sql`
//! → escurel pages, at **author time**.
//!
//! This is the one place peacock handles dashboard SQL text, and it only
//! rewrites what the author wrote; the render path never plans, builds or
//! sends a statement (guarded by `no_credential_surface`). The steps:
//!
//! 1. **Lint as a hard gate** — anofox's own linter, statically (no
//!    database): a layout role inside a data statement (the silent panel
//!    drop), an unknown role, a bad theme … fail the compile.
//! 2. **Plan** — anofox splits the script and rewrites each statement's
//!    `::ROLE` casts to `AS c{i}` aliases.
//! 3. **Emit one escurel query page per statement**: SELECT-only (escurel's
//!    `query_instance` does not check the statement type itself), the
//!    source table → `{{target}}` (the allow-listed managed view), and
//!    `getvariable('x')` → `:x` (bound by escurel as a prepared parameter).
//! 4. **Emit the report skill**: the params, the script, and the manifest
//!    mapping each planned statement's content hash to its query page.

use std::collections::BTreeSet;
use std::path::Path;

use anofox_visualization::lint::{LintOptions, Severity};

/// Lint codes that need a database to mean anything. Compile lints without
/// one (every read returns no rows), so these are not the author's mistake.
const DATA_DEPENDENT: &[&str] = &[
    "sql-error",
    "empty-panel",
    "render-error",
    "render-warning",
    "silent-setup",
];

/// One `--param name:type=default`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDecl {
    pub name: String,
    /// escurel query-page type (`date`, `text`, `number`, …).
    pub ty: String,
    pub default: Option<String>,
}

impl ParamDecl {
    pub fn parse(s: &str) -> Result<Self, String> {
        let (decl, default) = match s.split_once('=') {
            Some((d, v)) => (d, Some(v.to_owned())),
            None => (s, None),
        };
        let (name, ty) = decl.split_once(':').unwrap_or((decl, "text"));
        if !is_ident(name) {
            return Err(format!("bad param name `{name}` in `--param {s}`"));
        }
        Ok(ParamDecl {
            name: name.to_owned(),
            ty: ty.to_owned(),
            default,
        })
    }

    /// The report-skill (peacock) param type for this escurel type.
    fn report_type(&self) -> &'static str {
        match self.ty.as_str() {
            "date" => "date",
            "number" | "double" | "integer" | "int" => "number",
            "bool" | "boolean" => "boolean",
            _ => "string",
        }
    }
}

pub struct CompileOpts<'a> {
    pub report: &'a str,
    /// The table name the script reads (becomes `{{target}}`).
    pub source: &'a str,
    /// The managed view wikilink, e.g. `[[nw_order_lines::eu]]`.
    pub target: &'a str,
    pub params: &'a [ParamDecl],
}

/// The compiled pages.
pub struct Compiled {
    /// `(id, markdown)` of the report skill.
    pub report: (String, String),
    /// `(id, markdown)` per query page, in statement order.
    pub queries: Vec<(String, String)>,
}

/// Compile `script`. Every problem is returned, each naming its statement.
pub fn compile(script: &str, opts: &CompileOpts) -> Result<Compiled, Vec<String>> {
    if !peacock_core::is_slug(opts.report) {
        return Err(vec![format!(
            "`--report {}` is not a valid id",
            opts.report
        )]);
    }
    if !is_ident(opts.source) {
        return Err(vec![format!(
            "`--source {}` is not a table name",
            opts.source
        )]);
    }

    // 1. Lint (static).
    let lint_opts = LintOptions {
        design: false,
        ..Default::default()
    };
    let mut errors: Vec<String> =
        anofox_visualization::lint::check_opts(script, |_| Ok(Vec::new()), lint_opts)
            .into_iter()
            .filter(|d| d.severity == Severity::Error && !DATA_DEPENDENT.contains(&&*d.code))
            .map(|d| format!("statement {}: {}: {}", d.stmt, d.code, d.message))
            .collect();

    // 2. Plan; 3. one query page per statement.
    let declared: BTreeSet<&str> = opts.params.iter().map(|p| p.name.as_str()).collect();
    let mut queries = Vec::new();
    let mut manifest = Vec::new();
    for (i, panel) in anofox_visualization::sql::plan(script)
        .into_iter()
        .enumerate()
    {
        let n = i + 1;
        let first = first_keyword(&panel.sql);
        if panel.setup || !matches!(first.as_str(), "SELECT" | "WITH") {
            errors.push(format!(
                "statement {n}: `{first} …` — only SELECT statements compile to query pages; \
                 setup (CREATE/INSERT/SET/COPY/…) belongs in the escurel view, not the dashboard"
            ));
            continue;
        }
        let (sql, used) = rewrite(&panel.sql, opts.source);
        let sql = match target_bound(&sql, &panel.sql) {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("statement {n}: {e}"));
                continue;
            }
        };
        for p in &used {
            if !declared.contains(p.as_str()) {
                errors.push(format!(
                    "statement {n}: reads getvariable('{p}') — declare it with `--param {p}:<type>=<default>`"
                ));
            }
        }
        let id = format!("{}-q{n}", opts.report);
        let params: Vec<&ParamDecl> = opts
            .params
            .iter()
            .filter(|p| used.contains(&p.name))
            .collect();
        queries.push((id.clone(), query_page(&id, opts.target, &params, &sql, n)));
        manifest.push(serde_json::json!({
            "key": peacock_core::dashboard::statement_key(&panel.sql),
            "query": format!("[[query::{id}]]"),
            "params": params.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
            "draws": !anofox_visualization::roles::is_directive_panel(&panel.roles),
        }));
    }
    if manifest.is_empty() && errors.is_empty() {
        errors.push("the script has no statements".to_owned());
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Compiled {
        report: (opts.report.to_owned(), report_page(opts, script, &manifest)),
        queries,
    })
}

/// Write the compiled pages under `out` (`<report>.md`, `query/<id>.md`).
pub fn write(compiled: &Compiled, out: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out.join("query"))?;
    std::fs::write(
        out.join(format!("{}.md", compiled.report.0)),
        &compiled.report.1,
    )?;
    for (id, md) in &compiled.queries {
        std::fs::write(out.join("query").join(format!("{id}.md")), md)?;
    }
    Ok(())
}

fn query_page(id: &str, target: &str, params: &[&ParamDecl], sql: &str, n: usize) -> String {
    let mut fm = format!("---\nkind: instance\nskill: query\nid: {id}\ntarget: \"{target}\"\n");
    if !params.is_empty() {
        fm.push_str("params:\n");
        for p in params {
            fm.push_str(&format!(
                "  - {{name: {}, type: {}, required: true}}\n",
                p.name, p.ty
            ));
        }
    }
    let sql_json = serde_json::to_string(sql).expect("a string serializes");
    format!(
        "{fm}sql: {sql_json}\n---\n# {id}\nStatement {n} of a compiled anofox dashboard \
         (`peacock author compile`). Do not edit — recompile the dashboard.\n"
    )
}

fn report_page(opts: &CompileOpts, script: &str, manifest: &[serde_json::Value]) -> String {
    let mut fm = format!(
        "---\nkind: skill\nid: {}\nrender: a2ui\ndescription: Compiled anofox dashboard.\n",
        opts.report
    );
    if !opts.params.is_empty() {
        fm.push_str("params:\n");
        for p in opts.params {
            let default = p
                .default
                .as_ref()
                .map(|d| format!(", default: {}", serde_json::to_string(d).unwrap()))
                .unwrap_or_default();
            fm.push_str(&format!(
                "  {}: {{ type: {}{default} }}\n",
                p.name,
                p.report_type()
            ));
        }
    }
    fm.push_str("dashboard:\n  script: |\n");
    for line in script.trim_end().lines() {
        fm.push_str("    ");
        fm.push_str(line);
        fm.push('\n');
    }
    fm.push_str("  queries:\n");
    for q in manifest {
        fm.push_str(&format!("    - {q}\n"));
    }
    format!(
        "{fm}---\nCompiled from an anofox dashboard by `peacock author compile`; recompile \
         after editing the script.\n"
    )
}

/// The statement's first keyword (comments skipped), upper-cased.
fn first_keyword(sql: &str) -> String {
    let mut rest = sql.trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("--") {
            rest = r
                .split_once('\n')
                .map(|(_, t)| t)
                .unwrap_or("")
                .trim_start();
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r
                .split_once("*/")
                .map(|(_, t)| t)
                .unwrap_or("")
                .trim_start();
        } else {
            break;
        }
    }
    rest.split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("")
        .to_ascii_uppercase()
}

/// Rewrite a planned statement for escurel: outside string literals and
/// quoted identifiers, `getvariable('x')` → `:x` and the bare `source`
/// identifier → `{{target}}`. Returns the statement and the params it reads.
fn rewrite(sql: &str, source: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(sql.len());
    let mut used: Vec<String> = Vec::new();
    let b = sql.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c == '\'' || c == '"' {
            // Copy a literal / quoted identifier verbatim ('' / "" escapes).
            let q = c;
            out.push(q);
            i += 1;
            while i < b.len() {
                let d = b[i] as char;
                out.push(d);
                i += 1;
                if d == q {
                    if i < b.len() && b[i] as char == q {
                        out.push(q);
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &sql[start..i];
            let prev_dot = start > 0 && b[start - 1] == b'.';
            if word.eq_ignore_ascii_case("getvariable")
                && let Some((name, end)) = getvariable_arg(sql, i)
            {
                out.push(':');
                out.push_str(&name);
                if !used.contains(&name) {
                    used.push(name);
                }
                i = end;
                continue;
            }
            if word == source && !prev_dot {
                out.push_str("{{target}}");
            } else {
                out.push_str(word);
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    (out, used)
}

/// escurel runs a query page only when its SQL names the `{{target}}` it is
/// authorised against. A constant statement (a heading, no FROM) reads one
/// row of the target; a statement reading some other table is refused.
fn target_bound(rewritten: &str, planned: &str) -> Result<String, String> {
    if rewritten.contains("{{target}}") {
        return Ok(rewritten.to_owned());
    }
    let has_from = words_outside_literals(planned)
        .iter()
        .any(|w| w.eq_ignore_ascii_case("FROM"));
    if has_from {
        return Err(format!(
            "it reads a table other than `--source`; a compiled query page may only read the \
             managed view (`{{{{target}}}}`): {planned}"
        ));
    }
    Ok(format!(
        "{} FROM {{{{target}}}} LIMIT 1",
        rewritten.trim_end()
    ))
}

/// The bare words of a statement, skipping string literals / quoted names.
fn words_outside_literals(sql: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let b = sql.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'\'' || c == b'"' {
            i += 1;
            while i < b.len() && b[i] != c {
                i += 1;
            }
            i += 1;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            words.push(&sql[start..i]);
        } else {
            i += 1;
        }
    }
    words
}

/// After `getvariable` at byte `i`: `( 'name' )` → (name, byte after `)`).
fn getvariable_arg(sql: &str, i: usize) -> Option<(String, usize)> {
    let rest = &sql[i..];
    let open = rest.find('(')?;
    if !rest[..open].trim().is_empty() {
        return None;
    }
    let inner_start = i + open + 1;
    let close = sql[inner_start..].find(')')? + inner_start;
    let arg = sql[inner_start..close].trim();
    let name = arg.strip_prefix('\'')?.strip_suffix('\'')?;
    is_ident(name).then(|| (name.to_owned(), close + 1))
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_binds_variables_and_targets_the_view() {
        let (sql, used) = rewrite(
            "SELECT 'order_lines' AS c0, x AS c1 FROM order_lines o WHERE d BETWEEN \
             getvariable('from') AND getvariable( 'to' ) AND o.order_lines = 1",
            "order_lines",
        );
        assert_eq!(
            sql,
            "SELECT 'order_lines' AS c0, x AS c1 FROM {{target}} o WHERE d BETWEEN :from AND :to \
             AND o.order_lines = 1"
        );
        assert_eq!(used, vec!["from", "to"]);
    }

    #[test]
    fn a_constant_statement_reads_one_row_of_the_target() {
        // escurel refuses a query page whose SQL never names `{{target}}`.
        assert_eq!(
            target_bound("SELECT 'Northwind' AS c0", "SELECT 'Northwind' AS c0").unwrap(),
            "SELECT 'Northwind' AS c0 FROM {{target}} LIMIT 1"
        );
        // A statement reading some other table cannot be authorised.
        assert!(
            target_bound(
                "SELECT x AS c0 FROM other_table",
                "SELECT x AS c0 FROM other_table"
            )
            .is_err()
        );
        // A statement that reads the target passes through.
        assert_eq!(
            target_bound("SELECT x AS c0 FROM {{target}}", "-").unwrap(),
            "SELECT x AS c0 FROM {{target}}"
        );
    }

    #[test]
    fn first_keyword_skips_comments() {
        assert_eq!(first_keyword("-- hi\n /* x */ select 1"), "SELECT");
        assert_eq!(first_keyword("CREATE TABLE t"), "CREATE");
    }

    #[test]
    fn param_decl_parses() {
        assert_eq!(
            ParamDecl::parse("from:date=1997-01-01").unwrap(),
            ParamDecl {
                name: "from".into(),
                ty: "date".into(),
                default: Some("1997-01-01".into())
            }
        );
        assert!(ParamDecl::parse("bad name:text").is_err());
    }
}
