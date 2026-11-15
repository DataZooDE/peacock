//! ACC-2 / ACC-11 static guard: peacock's production crates hold **no**
//! database driver, credential, DSN, or SQL-construction surface. The only
//! sanctioned data path is the typed `escurel-client` (FR-D-3, NFR-S-1).
//!
//! This walks the production crate sources (not tests, not the embedded
//! Northwind fixture which legitimately authors query SQL) and fails if any
//! high-signal forbidden token appears.

use std::path::{Path, PathBuf};

/// Production crates that must stay credential- and SQL-free.
const PROD_CRATES: &[&str] = &[
    "peacock-types",
    "peacock-core",
    "peacock-server",
    "peacock-bin",
];

/// High-signal substrings that would betray a credential/driver/SQL surface.
/// Kept narrow to avoid false positives; the behavioural injection test
/// (`data_northwind::injection_value_is_bound_not_executed`) covers the rest.
const FORBIDDEN: &[&str] = &[
    "duckdb",
    "tokio_postgres",
    "postgres://",
    "mysql://",
    "read_parquet",
    "ATTACH ",
    "libpq",
    "sqlx",
];

fn workspace_root() -> PathBuf {
    // crates/peacock-core → workspace root is two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn scan_dir(dir: &Path, hits: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, hits);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            for needle in FORBIDDEN {
                if body.contains(needle) {
                    hits.push(format!("{} contains forbidden `{needle}`", path.display()));
                }
            }
        }
    }
}

#[test]
fn production_crates_have_no_credential_or_sql_surface() {
    let root = workspace_root();
    let mut hits = Vec::new();
    for crate_name in PROD_CRATES {
        scan_dir(&root.join("crates").join(crate_name).join("src"), &mut hits);
    }
    assert!(
        hits.is_empty(),
        "credential/SQL surface detected:\n{}",
        hits.join("\n")
    );
}

/// Issue #44: dashboard SQL is compiled into escurel query pages at AUTHOR
/// time (`peacock author compile`, in `peacock-bin`). The RENDER crates only
/// hand an authored script to anofox's pure renderer and answer its reads
/// from compiled query pages by hash — they never plan, rewrite or build a
/// statement themselves.
#[test]
fn render_crates_never_plan_or_build_sql() {
    const RENDER_CRATES: &[&str] = &["peacock-types", "peacock-core", "peacock-server"];
    const SQL_BUILDING: &[&str] = &[
        "sql::plan",
        "lint::check",
        "getvariable",
        "\"SELECT ",
        "format!(\"SELECT",
        "\"WITH ",
    ];
    let root = workspace_root();
    let mut hits = Vec::new();
    for crate_name in RENDER_CRATES {
        let mut stack = vec![root.join("crates").join(crate_name).join("src")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    let body = std::fs::read_to_string(&path).unwrap_or_default();
                    for needle in SQL_BUILDING {
                        if body.contains(needle) {
                            hits.push(format!("{} contains `{needle}`", path.display()));
                        }
                    }
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "SQL planning/building in a render crate:\n{}",
        hits.join("\n")
    );
}
