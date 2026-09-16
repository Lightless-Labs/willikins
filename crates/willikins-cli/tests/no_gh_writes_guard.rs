//! Nothing in this workspace may reach the operator's GitHub CLI.
//!
//! The teardown script was written (task 12) to delete the smoke run's
//! repository through `gh api`, which runs as the operator's own GitHub
//! credential -- one that can administer every repository they can touch,
//! in every organization they belong to. The operator had already
//! provided a *fine-grained* PAT scoped to the throwaway organization
//! `Willikins-Test`, in `~/.config/willikins/sandbox.env`, for exactly
//! this reason, in their own words: "I already provided a pat, scoped to
//! the test org. On purpose. So a mistake could *not* wreck anything. I
//! *never* allowed using my github cli set up to create or delete
//! repositories."
//!
//! Every provider call this workspace makes -- read or write, in a tool,
//! a live test, or the teardown script -- therefore authenticates as a
//! credential the *project* holds, never as the operator's own
//! (`WILLIKINS_GITHUB_TOKEN`, through `curl --config -` on stdin, exactly
//! as `deploy/teardown.sh` does since the 2026-09-16 credential-boundary
//! addendum). This test is what keeps that checkable after the people
//! who remember the reason have moved on: it walks the whole tree and
//! fails on any `gh` invocation in a file that runs.
//!
//! Scope, and why it is the whole tree rather than the destructive
//! calls only. A read-only `gh api` is harmless today and one edit away
//! from a write tomorrow, and the distinction cannot be drawn by
//! grepping: `gh api repos/o/r` is a read, `gh api -X DELETE repos/o/r`
//! is not, and they differ by two characters. Documentation may discuss
//! `gh` freely (this test reads no `.md` file); a human at a terminal
//! may use it for anything they like. What may not happen is a file in
//! this repository spending the operator's credential without them
//! there.
//!
//! Adversarial pass 2 note: this guard is the same shape as
//! `willikins-core/tests/expose_secret_guard.rs`, which pins the single-
//! credential-site rule, and as `willikins-core/tests/secret_literal_guard.rs`,
//! landed alongside it. All three are string scans over the tree, and all
//! three are weaker than the invariant they protect -- a shell script can
//! spell `gh` as `$G` and a Rust file can build the string at runtime.
//! They catch the honest mistake, which is the one that actually
//! happened.

use std::path::{Path, PathBuf};

/// The repository root, from this test's own manifest directory.
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("crates/<crate>/ has a grandparent")
        .to_path_buf()
}

/// Directories that hold no file this workspace runs.
const SKIPPED_DIRS: &[&str] = &["target", ".git", "docs", "todos", "node_modules", ".claude"];

/// Files that may name the tool because they *are* this rule.
fn is_exempt(relative: &str) -> bool {
    relative == "crates/willikins-cli/tests/no_gh_writes_guard.rs" || relative == "CLAUDE.md"
}

/// Every runnable file in the tree, repository-relative, `/`-separated.
fn runnable_files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("readable directory entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{prefix}{name}");
        if entry.file_type().expect("file type").is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                runnable_files(&entry.path(), &format!("{relative}/"), out);
            }
            continue;
        }
        let runnable = matches!(
            Path::new(&name).extension().and_then(|e| e.to_str()),
            Some("rs" | "sh" | "bash" | "toml" | "yaml" | "yml")
        );
        if runnable && !is_exempt(&relative) {
            out.push(relative);
        }
    }
}

/// `line` up to (not including) a trailing `//` or `#` comment marker, if
/// either appears; `line` unchanged otherwise.
///
/// Not string-literal-aware: a `#` or `//` inside a quoted string is
/// still treated as a comment start, which can only ever hide a real
/// invocation from this scan (a false *negative*), never manufacture a
/// false positive. That is the direction a security guard should err in
/// -- a guard that occasionally blocks an innocent commit gets disabled,
/// which is worse than the honest mistake it exists to catch.
fn before_trailing_comment(line: &str) -> &str {
    let slash = line.find("//");
    let hash = line.find('#');
    match (slash, hash) {
        (Some(s), Some(h)) => &line[..s.min(h)],
        (Some(s), None) => &line[..s],
        (None, Some(h)) => &line[..h],
        (None, None) => line,
    }
}

/// A line invokes the GitHub CLI if `gh` appears as a bare command word:
/// at the start of a line, after a shell separator, or as the program of
/// a `Command`. A word merely containing the letters (`weigh`, `gh_api`)
/// does not count, which is why the character before and after matter --
/// checked on the line with any trailing `//`/`#` comment already
/// stripped, and skipped outright when the whole line is a comment (a
/// leading `#`, `//`, or `*`, the last for a block-comment continuation).
fn invokes_gh(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with('*') {
        return false;
    }
    let line = before_trailing_comment(line);
    let bytes = line.as_bytes();
    let mut index = 0;
    while let Some(found) = line[index..].find("gh") {
        let at = index + found;
        let before_ok = at == 0
            || matches!(
                bytes[at - 1],
                b' ' | b'\t' | b'(' | b'"' | b'\'' | b'`' | b'|' | b'&' | b';' | b'='
            );
        // `None` is end-of-line (`command -v gh`); the rest are the
        // shell/Rust punctuation that can immediately follow a bare `gh`
        // that is not the start of a longer identifier.
        let after = bytes.get(at + 2).copied();
        let after_ok = matches!(
            after,
            None | Some(b' ' | b'"' | b'\'' | b'`' | b')' | b';' | b'|' | b'\t')
        );
        if before_ok && after_ok {
            return true;
        }
        index = at + 2;
    }
    false
}

#[test]
fn no_file_in_this_workspace_invokes_the_operators_github_cli() {
    let root = repo_root();
    let mut files = Vec::new();
    runnable_files(&root, "", &mut files);
    assert!(
        files.len() > 50,
        "the walk found only {} files; it is not reading the tree",
        files.len()
    );

    let mut offenders = Vec::new();
    for relative in &files {
        let Ok(text) = std::fs::read_to_string(root.join(relative)) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if invokes_gh(line) {
                offenders.push(format!("{relative}:{}: {}", number + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these lines invoke the operator's GitHub CLI; authenticate as the project's own \
         scoped credential instead (`WILLIKINS_GITHUB_TOKEN`, through `curl --config -` on \
         stdin, as `deploy/teardown.sh` does):\n{}",
        offenders.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_recognises_an_invocation_and_ignores_a_word_that_merely_contains_it() {
        assert!(invokes_gh("gh api repos/o/r"));
        assert!(invokes_gh("  gh auth status"));
        assert!(invokes_gh("output=$(gh api user)"));
        assert!(invokes_gh("Command::new(\"gh\")"));
        assert!(invokes_gh("something | gh api -X DELETE repos/o/r"));
        assert!(invokes_gh("if command -v gh"), "end-of-line after gh");
        assert!(invokes_gh("(gh api user)"), "gh immediately before )");
        assert!(invokes_gh("gh; echo done"), "gh immediately before ;");
        assert!(invokes_gh("gh | cat"), "gh immediately before |");

        assert!(!invokes_gh("# gh api is fine in a comment"));
        assert!(!invokes_gh("// gh api is fine in a comment"));
        assert!(!invokes_gh(" * gh api in a block-comment continuation"));
        assert!(!invokes_gh("let weight = 1;"));
        assert!(!invokes_gh("let gh_status = 404;"));
        assert!(!invokes_gh("through.high.ghost"));
    }

    #[test]
    fn a_trailing_comment_does_not_trigger_the_scan() {
        assert!(!invokes_gh("let x = 1; // gh api left here as an example"));
        assert!(!invokes_gh(
            "value = 1 # gh api, same idea in a shell comment"
        ));
        // But real code before the comment marker is still caught.
        assert!(invokes_gh("gh api repos/o/r # this one is real"));
    }

    #[test]
    fn the_exempt_files_are_the_two_guards_that_must_say_gh_by_name() {
        assert!(is_exempt(
            "crates/willikins-cli/tests/no_gh_writes_guard.rs"
        ));
        assert!(is_exempt("CLAUDE.md"));
        assert!(!is_exempt("deploy/teardown.sh"));
    }
}
