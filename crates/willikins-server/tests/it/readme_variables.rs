//! Task 12 verification: the README's "Environment variables" table is
//! the only place an operator learns what to set on a deployment, so it
//! has to stay true as the code changes.
//!
//! Three ties, all read from the repository itself:
//!
//! 1. Every variable this crate reads is in the table. A new one that
//!    nobody documents is how a deployment refuses to start with a
//!    message about a variable the operator has never heard of.
//! 2. Every variable the table names exists as a string literal
//!    somewhere in `crates/`. That catches a typo and an invented row.
//! 3. Every default the table prints for an optional numeric variable is
//!    the constant the server would actually apply.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use willikins_server::ButlerConfig;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn readme() -> String {
    std::fs::read_to_string(repo_root().join("README.md")).expect("README.md")
}

/// One row of the README's variable table: its name and its documented
/// default, taken from the first two columns.
fn table_rows() -> Vec<(String, String)> {
    readme()
        .lines()
        .filter(|line| line.starts_with("| `"))
        .filter_map(|line| {
            let mut cells = line.trim_matches('|').split('|').map(str::trim);
            let name = cells.next()?.trim_matches('`').to_string();
            let default = cells.next()?.to_string();
            Some((name, default))
        })
        .collect()
}

fn documented_names() -> BTreeSet<String> {
    table_rows().into_iter().map(|(name, _)| name).collect()
}

/// Every `"WILLIKINS_*"` or `"PORT"` string literal in `path`'s tree.
/// Only literals: a name inside a doc comment is written in backticks
/// and is deliberately not counted, so prose about a variable another
/// crate owns does not make this crate look like its reader.
fn literal_variable_names(path: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(next) = stack.pop() {
        if next.is_dir() {
            for entry in std::fs::read_dir(&next).expect("readable directory") {
                stack.push(entry.expect("directory entry").path());
            }
            continue;
        }
        if next.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&next).expect("readable source file");
        for piece in text.split('"').skip(1).step_by(2) {
            if piece == "PORT" || (piece.starts_with("WILLIKINS_") && !piece.contains(' ')) {
                found.insert(piece.to_string());
            }
        }
    }
    found
}

/// The variables this crate reads but the README's table never names.
///
/// Two prefixes are excluded, neither of which a deployment ever sets:
/// `WILLIKINS_LIVE_*` belong to the provider crates' opt-in live tests,
/// which the README documents in its own sentence under the table; and
/// `WILLIKINS_TEST_*` are the labels this crate's own `#[cfg(test)]`
/// catalog gives `Credential::for_testing`, which reads no environment
/// variable at all.
#[test]
fn every_variable_the_server_reads_is_in_the_readme_table() {
    let documented = documented_names();
    let read_by_the_server = literal_variable_names(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .canonicalize()
            .expect("the crate's own src"),
    );

    let missing: Vec<&String> = read_by_the_server
        .iter()
        .filter(|name| !name.starts_with("WILLIKINS_LIVE_") && !name.starts_with("WILLIKINS_TEST_"))
        .filter(|name| !documented.contains(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "willikins-server reads these variables and the README documents none of them: {missing:?}"
    );
}

#[test]
fn every_variable_the_readme_table_names_exists_in_the_code() {
    let in_code = literal_variable_names(
        &repo_root()
            .join("crates")
            .canonicalize()
            .expect("crates directory"),
    );
    for name in documented_names() {
        assert!(
            in_code.contains(&name),
            "the README table names {name}, which no crate reads"
        );
    }
}

/// The defaults the table prints, against the constants the server
/// applies when the variable is unset.
#[test]
fn every_documented_default_is_the_constant_the_server_applies() {
    let rows = table_rows();
    let default_for = |name: &str| -> String {
        rows.iter()
            .find(|(row, _)| row == name)
            .unwrap_or_else(|| panic!("no README row for {name}"))
            .1
            .clone()
    };

    for (variable, expected) in [
        (
            "WILLIKINS_APPROVAL_WINDOW_SECONDS",
            ButlerConfig::DEFAULT_APPROVAL_WINDOW.as_secs().to_string(),
        ),
        (
            "WILLIKINS_PLAN_TTL_SECONDS",
            ButlerConfig::DEFAULT_APPLY_WINDOW.as_secs().to_string(),
        ),
        (
            "WILLIKINS_PLAN_RATE_PER_MINUTE",
            ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE.to_string(),
        ),
        (
            "WILLIKINS_READ_RATE_PER_MINUTE",
            ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE.to_string(),
        ),
    ] {
        let documented = default_for(variable);
        assert!(
            documented.contains(&format!("`{expected}`")),
            "{variable}: the README says {documented:?}, the server applies {expected}"
        );
    }

    // The two required paths have no default, and the table must not
    // invent one for them.
    for variable in ["WILLIKINS_WORKFLOWS_DIR", "WILLIKINS_JOURNAL_PATH"] {
        assert_eq!(default_for(variable), "none", "{variable}");
    }
}
