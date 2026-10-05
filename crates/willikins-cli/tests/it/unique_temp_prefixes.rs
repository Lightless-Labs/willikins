//! Acceptance 5 (milestone 3k,
//! `docs/plans/2026-10-05-milestone-3k-faster-gates.md`, decision (d9)):
//! every `std::env::temp_dir()`-based prefix this crate's tests build
//! must be unique across `tests/it/`. Two files that joined
//! `temp_dir()` with the same prefix never ran at the same time before
//! this milestone -- `cargo test` built each `tests/<stem>.rs` as its
//! own process. Now every file in `tests/it/` shares one binary and one
//! pid (`RUST_TEST_THREADS=2`), so a shared prefix is a shared
//! directory: one test's `remove_dir_all` can race another's writes.
//! `willikins-cli-test-` collided between `cli.rs` and
//! `prerendered_identifier_guards.rs` until the latter renamed to
//! `willikins-cli-prerendered-guard-`; this test is what keeps a future
//! collision from being silent.

use std::path::{Path, PathBuf};

/// This file's own repository-relative path.
///
/// Exempt from the scan below for the same reason every other guard in
/// this workspace exempts itself (decision (d6)): the doc comment above
/// and the fixtures in `mod tests` below both write out
/// `temp_dir()`-shaped text and fictitious `"willikins-...-{"` literals
/// on purpose, to prove [`prefixes_in`] works -- scanning this file for
/// real would turn its own test fixtures into violations of the very
/// rule they exist to exercise.
const SELF: &str = "crates/willikins-cli/tests/it/unique_temp_prefixes.rs";

/// `tests/it/`, from this crate's own manifest directory.
fn it_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/it")
}

/// Every `tests/it/<name>.rs` prefix [`prefixes_in`] finds near a
/// `temp_dir()` call, paired with the file it came from. `main.rs` and
/// [`SELF`] are never read.
fn prefixes_by_file() -> Vec<(String, String)> {
    let dir = it_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect();
    files.sort();

    let mut out = Vec::new();
    for path in files {
        let file_name = path
            .file_name()
            .expect("a file has a name")
            .to_string_lossy()
            .into_owned();
        if file_name == "main.rs" {
            continue;
        }
        let relative = format!("crates/willikins-cli/tests/it/{file_name}");
        if relative == SELF {
            continue;
        }
        let source =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for prefix in prefixes_in(&source) {
            out.push((prefix, relative.clone()));
        }
    }
    out
}

/// How many lines after (and including) a `temp_dir()` call
/// [`prefixes_in`] looks at for the call's own prefix literal. Every
/// real call site in this crate writes the prefix on the very next
/// line (`temp_dir().join(format!(` on one line, the string literal on
/// the next), so 4 lines is generous headroom without reaching into an
/// unrelated, later call.
const WINDOW_LINES: usize = 4;

/// Every `"willikins-[a-z0-9-]+-{"`-shaped prefix (the text up to and
/// including the trailing `-` that immediately precedes `{`, per
/// acceptance 5's own wording) that appears within [`WINDOW_LINES`]
/// lines of a `std::env::temp_dir()` call in `source`. Bounding the
/// search is what keeps a prefix mentioned only in a doc comment, well
/// away from any real call site, from counting as a collision.
fn prefixes_in(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.contains("temp_dir(") {
            continue;
        }
        let end = (index + WINDOW_LINES).min(lines.len());
        out.extend(prefix_literals(&lines[index..end].join("\n")));
    }
    out
}

/// Every `"willikins-[a-z0-9-]+-{"`-shaped literal prefix in `text`,
/// with no awareness of where a string literal starts or ends: it looks
/// for the byte sequence `willikins-`, consumes `[a-z0-9-]` greedily,
/// and keeps the match only when the very next byte is `{` -- which,
/// since `-` is itself one of the consumed bytes, also guarantees the
/// match ends in `-`, exactly as `"willikins-[a-z0-9-]+-\{"` requires.
fn prefix_literals(text: &str) -> Vec<String> {
    const NEEDLE: &str = "willikins-";
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(found) = text[search_from..].find(NEEDLE) {
        let start = search_from + found;
        let mut end = start;
        while end < bytes.len() && matches!(bytes[end], b'a'..=b'z' | b'0'..=b'9' | b'-') {
            end += 1;
        }
        if end < bytes.len() && bytes[end] == b'{' && end > start && bytes[end - 1] == b'-' {
            out.push(text[start..end].to_string());
        }
        search_from = end.max(start + NEEDLE.len());
    }
    out
}

#[test]
fn every_temp_dir_prefix_in_tests_it_is_unique() {
    let found = prefixes_by_file();

    // Non-vacuity: the scan must really be reading real call sites, not
    // silently finding nothing and passing by default.
    let files_with_a_prefix: std::collections::BTreeSet<&str> =
        found.iter().map(|(_, file)| file.as_str()).collect();
    assert!(
        files_with_a_prefix.len() >= 8,
        "expected at least 8 files under tests/it/ to call temp_dir() with a \
         `willikins-...-{{` prefix; found only {}: {found:?}",
        files_with_a_prefix.len()
    );
    assert!(
        found
            .iter()
            .any(|(prefix, _)| prefix == "willikins-cli-test-"),
        "expected to find cli.rs's own `willikins-cli-test-` prefix; the scan \
         is not reading real call sites: {found:?}"
    );

    let mut by_prefix: std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>> =
        std::collections::BTreeMap::new();
    for (prefix, file) in &found {
        by_prefix.entry(prefix).or_default().insert(file);
    }

    let offenders: Vec<String> = by_prefix
        .iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(prefix, files)| {
            format!(
                "{prefix} is used by more than one file: {}",
                files.iter().copied().collect::<Vec<_>>().join(", ")
            )
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these temp_dir() prefixes collide across files that now share one \
         process and one pid (RUST_TEST_THREADS=2 under tests/it/main.rs); \
         rename one of each pair so every prefix is unique:\n{}",
        offenders.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fictitious_prefix_next_to_a_temp_dir_call_is_found() {
        let source = r#"
fn helper() {
    let dir = std::env::temp_dir().join(format!(
        "willikins-fictitious-example-{}-{label}",
        std::process::id()
    ));
}
"#;
        assert_eq!(
            prefixes_in(source),
            vec!["willikins-fictitious-example-".to_string()]
        );
    }

    #[test]
    fn two_distinct_prefixes_in_two_different_calls_are_both_found() {
        // The two calls sit more than `WINDOW_LINES` apart, so each
        // call's own window never reaches the other's literal -- a
        // regression here would otherwise double-count one of them.
        let source = r#"
fn one() {
    let a = std::env::temp_dir().join(format!("willikins-alpha-{}-x", 1));
}
// filler
// filler
// filler
// filler
fn two() {
    let b = std::env::temp_dir().join(format!("willikins-beta-{}-y", 2));
}
"#;
        assert_eq!(
            prefixes_in(source),
            vec![
                "willikins-alpha-".to_string(),
                "willikins-beta-".to_string()
            ]
        );
    }

    #[test]
    fn a_literal_with_no_brace_is_not_a_prefix() {
        let source = r#"
fn helper() {
    let dir = std::env::temp_dir().join("willikins-smoke");
}
"#;
        assert_eq!(prefixes_in(source), Vec::<String>::new());
    }

    #[test]
    fn a_matching_literal_far_from_any_temp_dir_call_is_not_counted() {
        // More than six lines away from the `temp_dir()` call: outside
        // the window this guard actually scans, exactly like a doc
        // comment at the top of a file would be.
        let source = r#"
fn helper() {
    let dir = std::env::temp_dir();
    let _ = 1;
    let _ = 2;
    let _ = 3;
    let _ = 4;
    let _ = 5;
    let _ = dir.join(format!("willikins-far-away-{}-x", 1));
}
"#;
        assert_eq!(prefixes_in(source), Vec::<String>::new());
    }

    #[test]
    fn this_files_own_path_is_the_declared_exemption() {
        assert_eq!(
            SELF,
            "crates/willikins-cli/tests/it/unique_temp_prefixes.rs"
        );
    }

    /// Proves the exemption is not vacuous: this file's own fixtures
    /// above really do contain `temp_dir()`-shaped text and
    /// `"willikins-...-{"` literals that [`prefixes_in`] would otherwise
    /// flag, so [`prefixes_by_file`] skipping [`SELF`] is load-bearing,
    /// not a no-op.
    #[test]
    fn this_files_own_source_contains_fixture_prefixes_that_would_otherwise_match() {
        let this_file =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/unique_temp_prefixes.rs");
        let source = std::fs::read_to_string(&this_file).expect("this file exists on disk");
        assert!(
            !prefixes_in(&source).is_empty(),
            "expected this file's own fixtures to contain at least one \
             `willikins-...-{{`-shaped literal near a `temp_dir()` mention"
        );
    }
}
