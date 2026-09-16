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

/// Whether `name` is a file something in this repository executes.
///
/// Extension is the wrong question for two of them, which is why this is
/// a function and not a `matches!` on `Path::extension`. `Dockerfile` has
/// no extension at all and is what Railway builds the deployed image
/// from, so a `RUN gh api ...` line in it runs on every deploy;
/// `.railway/railway.ts` is the infrastructure-as-code document
/// `railway config apply` evaluates. Both were outside the first version
/// of this walk. `.json` earns its place through `package.json`, whose
/// `scripts` are shell commands npm runs -- it declares none today, and
/// the other three dozen `.json` files in the tree are recorded API
/// responses that execute nothing, so including the extension costs
/// nothing and closes the one place a command could hide.
fn is_runnable(name: &str) -> bool {
    if name == "Dockerfile" {
        return true;
    }
    matches!(
        Path::new(name).extension().and_then(|e| e.to_str()),
        Some("rs" | "sh" | "bash" | "toml" | "yaml" | "yml" | "ts" | "js" | "json")
    )
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
        if is_runnable(&name) && !is_exempt(&relative) {
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
    // Either marker opens a comment only at the start of the line or
    // after whitespace, which is how both languages actually write one.
    // Taking the *first* occurrence anywhere swallowed the rest of a line
    // that merely contained the characters, and everything after it
    // including a real invocation: `https://api.github.com` carries a
    // `//` and `${name#prefix}` a `#`, and either one hid what followed.
    // See [`a_marker_inside_a_word_does_not_open_a_comment`].
    let opens_comment =
        |index: usize| index == 0 || matches!(line.as_bytes()[index - 1], b' ' | b'\t');
    let slash = line
        .match_indices("//")
        .map(|(index, _)| index)
        .find(|&index| opens_comment(index));
    // `#[` is a Rust attribute, not a comment, so it does not truncate
    // the item that may follow it on the same line -- the same exception
    // [`is_whole_line_comment`] makes, which would otherwise be undone
    // here.
    let hash = line
        .match_indices('#')
        .map(|(index, _)| index)
        .find(|&index| opens_comment(index) && !line[index..].starts_with("#["));
    match (slash, hash) {
        (Some(s), Some(h)) => &line[..s.min(h)],
        (Some(s), None) => &line[..s],
        (None, Some(h)) => &line[..h],
        (None, None) => line,
    }
}

/// Whether `trimmed` (a line with its leading whitespace removed) is
/// wholly a comment, and so cannot invoke anything.
///
/// The `*` arm is the narrow one, and it is narrow because the wide one
/// was a bypass. A Rust block-comment continuation is `*` alone or `*`
/// then a space; a shell `case` arm is `*)` or `*pattern)`, and
/// `deploy/teardown.sh` -- the very script this guard exists for -- has
/// one at its argument parser, as does `deploy/teardown_test.sh` three
/// times over. Skipping every line that merely *starts* with `*` made
/// the idiomatic one-line arm `*) gh api -X DELETE repos/o/r ;;`
/// invisible to this scan. See
/// [`a_shell_case_arm_is_not_a_comment`].
///
/// `#[` is likewise excluded from the `#` arm: a Rust attribute is not a
/// comment, and an item written on one line after it would otherwise be
/// skipped whole. A shebang stays a comment -- it names an interpreter,
/// not a command this repository issues.
fn is_whole_line_comment(trimmed: &str) -> bool {
    if trimmed.starts_with("//") {
        return true;
    }
    if trimmed.starts_with('#') && !trimmed.starts_with("#[") {
        return true;
    }
    trimmed == "*" || trimmed.starts_with("* ")
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
    if is_whole_line_comment(trimmed) {
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

    /// A shell `case` arm starts with `*` and is not a comment. This is
    /// the bypass this guard had: `deploy/teardown.sh`'s own argument
    /// parser ends in `*)`, and an invocation written on that line went
    /// unseen. Proved by mutation before it was fixed -- the arm was
    /// planted in the real script and the guard stayed green.
    #[test]
    fn a_shell_case_arm_is_not_a_comment() {
        assert!(invokes_gh("    *) gh api -X DELETE repos/o/r ;;"));
        assert!(invokes_gh("  *api.github.com/repos/*) gh api user ;;"));
        // A genuine block-comment continuation is still a comment.
        assert!(!invokes_gh(" * gh api in a block-comment continuation"));
        assert!(!is_whole_line_comment("*)"));
        assert!(is_whole_line_comment("* still a comment"));
        assert!(is_whole_line_comment("*"));
    }

    /// A `#` or `//` that is part of a word -- a shell parameter
    /// expansion, a URL's scheme separator or fragment -- does not open a
    /// comment, so it must not hide what follows it on the line. The
    /// `https://` case is the one that bit: `deploy/teardown.sh` and
    /// `deploy/teardown_test.sh` both build API base URLs, and every line
    /// carrying one was truncated at the scheme's own slashes.
    #[test]
    fn a_marker_inside_a_word_does_not_open_a_comment() {
        assert!(invokes_gh(
            "repo=\"${slug#owner/}\"; gh api \"repos/$repo\""
        ));
        assert!(invokes_gh("url=\"https://x/y#frag\" && gh api user"));
        assert!(invokes_gh(
            "base=\"https://api.github.com\"; gh api \"$base/user\""
        ));
        // A Rust attribute is not a comment either.
        assert!(invokes_gh("#[allow(dead_code)] fn f() { run(\"gh\"); }"));
        // A real trailing comment still is one, and so is a shebang.
        assert!(!invokes_gh(
            "value = 1 # gh api, same idea in a shell comment"
        ));
        assert!(!invokes_gh("#!/bin/sh -- gh api would be a comment here"));
    }

    /// The walk reads what this repository executes, which is not the
    /// same set as "files with a source-code extension": the `Dockerfile`
    /// Railway builds from and the `.railway/railway.ts` it applies both
    /// run, and neither was scanned by the first version of this guard.
    #[test]
    fn the_walk_reads_every_kind_of_file_this_repository_executes() {
        for name in [
            "teardown.sh",
            "main.rs",
            "Cargo.toml",
            "railway.ts",
            "package.json",
            "Dockerfile",
        ] {
            assert!(is_runnable(name), "{name} is not scanned");
        }
        for name in ["HANDOFF.md", "snapshot.snap", "LICENSE"] {
            assert!(!is_runnable(name), "{name} should not be scanned");
        }

        // And the walk actually finds them on disk, so the extension
        // list above is not describing a set the traversal never reaches.
        let root = repo_root();
        let mut files = Vec::new();
        runnable_files(&root, "", &mut files);
        for expected in ["deploy/teardown.sh", "Dockerfile", ".railway/railway.ts"] {
            assert!(
                files.iter().any(|f| f == expected),
                "the walk never reached {expected}"
            );
        }
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
