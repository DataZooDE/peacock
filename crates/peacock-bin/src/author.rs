//! The `peacock author` subcommand: scaffold, validate, and preview report
//! skills (BRD §7 authoring-tooling deferral; HLD §170 authoring preview).
//!
//! All three reuse peacock-core — there is no second parser or guardrail here:
//! - `validate` and `scaffold` are pure (no escurel, no credential);
//! - `preview` renders against a **real** escurel via the embedded library face
//!   (`peacock_core::render`), the same render core every surface funnels
//!   through, and prints a summary (component kinds, row count, chart rasterized).
//!
//! Each subcommand returns a process exit code so the binary can propagate it.

use clap::Subcommand;
use peacock_core::{EscurelData, RenderOpts, render, scaffold, validate_skill_markdown};
use peacock_types::Principal;
use serde_json::Value;

/// `peacock author …` operations.
#[derive(Subcommand, Debug)]
pub enum AuthorCmd {
    /// Validate a report-skill markdown file (front matter + body). Prints any
    /// problems with their source lines; exits non-zero on failure.
    Validate {
        /// Path to the report-skill `.md` file.
        file: String,
    },
    /// Print a minimal valid report-skill template for `report_id` to stdout.
    Scaffold {
        /// The report id to embed in the scaffold.
        report_id: String,
    },
    /// Render a report-skill file against a real escurel and print a summary.
    Preview {
        /// Path to the report-skill `.md` file.
        file: String,
        /// escurel endpoint base URL.
        #[arg(long)]
        escurel: String,
        /// escurel bearer token.
        #[arg(long, default_value = "")]
        token: String,
        /// Tenant forwarded to escurel.
        #[arg(long, default_value = "acme")]
        tenant: String,
        /// Subject forwarded to escurel.
        #[arg(long, default_value = "peacock")]
        sub: String,
        /// Comma-separated groups for the principal (ACL).
        #[arg(long, default_value = "")]
        groups: String,
        /// Override params as `k=v,k=v` (values parsed as JSON, else string).
        #[arg(long)]
        params: Option<String>,
    },
}

/// Run an `author` subcommand, returning the process exit code.
pub async fn run(cmd: AuthorCmd) -> i32 {
    match cmd {
        AuthorCmd::Validate { file } => validate(&file),
        AuthorCmd::Scaffold { report_id } => {
            print!("{}", scaffold(&report_id));
            0
        }
        AuthorCmd::Preview {
            file,
            escurel,
            token,
            tenant,
            sub,
            groups,
            params,
        } => {
            preview(
                &file,
                &escurel,
                &token,
                &tenant,
                &sub,
                &groups,
                params.as_deref(),
            )
            .await
        }
    }
}

/// `peacock author validate <file>` — parse + guardrail + cross-reference the
/// skill, printing every problem found. Exit 0 only when clean.
fn validate(file: &str) -> i32 {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("author: cannot read {file}: {e}");
            return 1;
        }
    };
    let errors = validate_skill_markdown(&text);
    if errors.is_empty() {
        println!("{file}: OK — report skill is valid.");
        return 0;
    }
    eprintln!("{file}: {} problem(s):", errors.len());
    for e in &errors {
        eprintln!("  {e}");
    }
    1
}

/// `peacock author preview <file> --escurel <url> …` — render the skill against
/// a real escurel and print a summary of the resulting artifact.
#[allow(clippy::too_many_arguments)]
async fn preview(
    file: &str,
    escurel: &str,
    token: &str,
    tenant: &str,
    sub: &str,
    groups: &str,
    params: Option<&str>,
) -> i32 {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("author: cannot read {file}: {e}");
            return 1;
        }
    };

    // Validate first — a clear local error beats an opaque render failure.
    let errors = validate_skill_markdown(&text);
    if !errors.is_empty() {
        eprintln!("author: {file} is invalid ({} problem(s)):", errors.len());
        for e in &errors {
            eprintln!("  {e}");
        }
        return 1;
    }

    let skill = match peacock_core::parse_skill_markdown(&text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("author: {e}");
            return 1;
        }
    };

    let param_vec = match parse_params(params) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("author: bad --params: {e}");
            return 1;
        }
    };

    let principal = Principal {
        sub: sub.to_owned(),
        scopes: Vec::new(),
        groups: groups
            .split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_owned)
            .collect(),
        tenant: tenant.to_owned(),
        raw_token: token.to_owned(),
        trace_id: String::new(),
    };

    // Render through the embedded library face — the same core every surface
    // funnels through (FR-R-1). Rasterize the first chart so the author sees
    // whether it renders to PNG.
    let escurel_data = EscurelData::new(escurel.to_owned());
    let opts = RenderOpts {
        png_scale: Some(2.0),
        ..Default::default()
    };
    let artifact = match render(&skill.id, &param_vec, &principal, &escurel_data, &opts).await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("author: preview render failed: {e}");
            return 1;
        }
    };

    print_summary(&skill.id, &artifact);
    0
}

/// Parse `--params k=v,k=v` into a JSON object. Each value is parsed as JSON
/// when possible (so `42`, `true`, `"x"` keep their type); otherwise it is kept
/// as a bare string.
fn parse_params(params: Option<&str>) -> Result<Value, String> {
    let mut obj = serde_json::Map::new();
    if let Some(raw) = params {
        for pair in raw.split(',').filter(|p| !p.trim().is_empty()) {
            let (k, v) = pair
                .split_once('=')
                .ok_or_else(|| format!("expected `k=v`, got `{pair}`"))?;
            let value = serde_json::from_str::<Value>(v.trim())
                .unwrap_or_else(|_| Value::String(v.trim().to_owned()));
            obj.insert(k.trim().to_owned(), value);
        }
    }
    Ok(Value::Object(obj))
}

/// Print the authoring preview summary: component kinds, the per-alias row
/// counts, and whether a chart rasterized.
fn print_summary(report_id: &str, artifact: &peacock_types::Artifact) {
    println!("preview of `{report_id}` (rendered against real escurel):");

    let kinds: Vec<String> = artifact
        .a2ui
        .get("components")
        .and_then(Value::as_array)
        .map(|cs| {
            cs.iter()
                .filter_map(|c| c.get("kind").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    println!("  components: [{}]", kinds.join(", "));

    // `structured_content.rows` is the primary view's row array.
    let row_count = artifact
        .structured_content
        .rows
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    println!("  rows: {row_count}");

    println!("  vega specs: {}", artifact.vega_specs.len());
    println!(
        "  chart rasterized: {}",
        if artifact.png.is_some() { "yes" } else { "no" }
    );
}
