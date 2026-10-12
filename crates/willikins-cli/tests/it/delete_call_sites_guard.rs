//! Milestone 3m task P8 (acceptance 14,
//! `docs/plans/2026-10-05-milestone-3m-safe-rollback.md`): an inventory,
//! not a prohibition. Every other guard in this file's neighbourhood
//! (`no_gh_writes_guard.rs`, `willikins-core`'s `sink_token_guard.rs` and
//! `teardown_token_guard.rs`) exists because the call it finds must never
//! happen outside one named place. A `.delete(` or `.delete_with_body(`
//! call in a provider crate is the opposite: this milestone's whole point
//! is that some of them now exist on purpose (P1-P5's inverses,
//! `delete_repo`/`delete_project`/`delete_pipeline`/`delete_profile` and
//! friends), and more will arrive as live cycles (L1-L3) and later
//! providers add their own. So this test pins what exists today, by file
//! and enclosing function, and fails on any difference — added or
//! removed — until a reviewer reads the new line and extends
//! [`INVENTORY`] in the same commit. That review is the point: a stray
//! delete call reachable from somewhere nobody meant to put one is caught
//! here, before it is caught by a tool with no `inverse()` to its name.
//!
//! **Scope.** Every `.rs` file under every `crates/willikins-providers-*`
//! tree — `src/` and `tests/` alike, `target/` skipped — because
//! acceptance 14 says "every ... call site in the provider crates", not
//! "every production call site": `willikins-providers-http`'s own
//! `self.agent.delete(` (the one real site both `Http::delete` and
//! `Http::delete_with_body` share), the live cycles' by-hand cleanup
//! deletes (acceptance 16), and the fake catalog's mock `inverse.delete(`
//! calls are all exactly what a reviewer wants visible, not noise to
//! filter out. A text scan cannot tell `#[cfg(test)]` apart from
//! production code without parsing the file (the same tradeoff
//! `no_gh_writes_guard.rs` makes), so this does not try; the pin already
//! includes every test call site that exists today.
//!
//! **Patterns.** Literally the two acceptance 14 names, `.delete(` and
//! `.delete_with_body(`, each requiring the leading `.` so a method call
//! (`client.delete(...)`) is never confused with a function's own
//! definition (`fn delete(...)`, no dot before `delete`) — except
//! `delete_with_body(` itself: `willikins-providers-http/src/http.rs`
//! defines `Http::delete_with_body`, and that definition line *is* an
//! occurrence of the pattern `.delete_with_body(`... no, it is not: the
//! definition reads `pub fn delete_with_body(&self, ...)`, with no `.`
//! before `delete_with_body`, so the dot-anchored pattern already leaves
//! it out. (The one place a defining line *would* match is if some day a
//! method is defined as `fn x(&self) { self.delete_with_body(...) }` on
//! the same physical line as its own signature; nothing in this tree
//! does that today.) Matched on comment-stripped, non-whole-line-comment
//! text exactly as `no_gh_writes_guard.rs` strips it, so a mention of
//! either pattern in a doc comment or prose is not a site.
//!
//! **Enclosing function.** The last `fn <ident>` token seen on a
//! non-comment line at or above the hit, read top to bottom through the
//! file — not a scoped, brace-depth-aware read. This is approximate (it
//! would misattribute a call written after the last `fn` in a file, at
//! module scope) but every file this walk reaches today is a flat
//! sequence of `impl` methods with no such code, and a closure passed to
//! another call (`willikins-providers-github/src/client.rs`'s
//! `self.retry_secondary_limit(|| self.http.delete(&path))`) has no `fn`
//! of its own, so it correctly attributes to the method around it
//! (`delete_repo`). Multi-line signatures are not a problem either way:
//! the name always sits on the same line as the `fn` keyword.

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

/// The one file allowed to say either pattern outside the pinned
/// inventory: this guard's own module doc and test fixtures say
/// `.delete(` and `.delete_with_body(` in prose and in strings. A
/// providers-only walk never reaches `crates/willikins-cli/`, so this
/// exemption is inert today — named anyway, the same way
/// `no_gh_writes_guard.rs` names itself, in case the walk is ever widened
/// to the whole tree.
fn is_exempt(relative: &str) -> bool {
    relative == "crates/willikins-cli/tests/it/delete_call_sites_guard.rs"
}

/// Every provider crate's directory: `crates/willikins-providers-*`.
fn provider_crate_dirs(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root.join("crates"))
        .expect("crates/ is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("willikins-providers-"))
        })
        .collect()
}

/// Every `.rs` file under `dir`, repository-relative and `/`-separated,
/// skipping `target/`.
fn rs_files(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name == "target" {
                continue;
            }
            rs_files(&path, root, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let relative = path
                .strip_prefix(root)
                .expect("every walked path is under root")
                .to_string_lossy()
                .replace('\\', "/");
            out.push(relative);
        }
    }
}

/// `line` up to (not including) a trailing `//` comment marker, if one
/// appears. Copied from `no_gh_writes_guard.rs`'s own
/// `before_trailing_comment`, minus the shell `#`-comment half: this
/// guard reads only `.rs` files, which have no shell comments, and a bare
/// `#` is a Rust attribute opener (`#[...]`) or part of a macro, never a
/// comment.
///
/// Not string-literal-aware, same reasoning as the original: a `//`
/// inside a quoted string can only hide a real call from this scan (a
/// false negative, the safe direction for a guard whose job is raising
/// visibility, not blocking a push outright).
fn before_trailing_comment(line: &str) -> &str {
    let opens_comment =
        |index: usize| index == 0 || matches!(line.as_bytes()[index - 1], b' ' | b'\t');
    let slash = line
        .match_indices("//")
        .map(|(index, _)| index)
        .find(|&index| opens_comment(index));
    match slash {
        Some(s) => &line[..s],
        None => line,
    }
}

/// Whether `trimmed` (a line with its leading whitespace removed) is
/// wholly a comment: a `//` line comment (which also catches `///` doc
/// comments and `//!` module docs) or a block-comment continuation line
/// (`*` alone, or `*` then a space — never a shell `case` arm, which this
/// guard never reads).
fn is_whole_line_comment(trimmed: &str) -> bool {
    trimmed.starts_with("//") || trimmed == "*" || trimmed.starts_with("* ")
}

/// The function name following a bare `fn ` token in `line`, if any — the
/// first one found, read left to right. `fn` must sit at a word boundary
/// (not part of a longer identifier, so `infn_helper()` does not match)
/// and be followed by at least one identifier character (so a trailing
/// `fn ` with nothing after it, which does not parse as Rust anyway,
/// yields `None` rather than an empty name).
fn extract_fn_name(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut search_from = 0;
    while let Some(found) = line[search_from..].find("fn ") {
        let at = search_from + found;
        let before_ok =
            at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
        if before_ok {
            let rest = &line[at + 3..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
        search_from = at + 2;
    }
    None
}

/// Every non-overlapping occurrence of `pattern` in `line`.
fn occurrences(line: &str, pattern: &str) -> usize {
    let mut count = 0;
    let mut search_from = 0;
    while let Some(found) = line[search_from..].find(pattern) {
        count += 1;
        search_from += found + pattern.len();
    }
    count
}

/// The two call shapes acceptance 14 names, in the fixed order every
/// entry in [`INVENTORY`] is checked against.
const PATTERNS: &[&str] = &[".delete(", ".delete_with_body("];

/// One label per hit: `<file>::<enclosing fn, or "<module scope>"> <pattern>`.
/// A line with two occurrences of the same pattern (never seen in this
/// tree today) yields the label twice, so a second call added to an
/// already-listed function still changes the inventory.
fn call_site_labels(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for dir in provider_crate_dirs(root) {
        rs_files(&dir, root, &mut files);
    }

    let mut labels = Vec::new();
    for relative in &files {
        if is_exempt(relative) {
            continue;
        }
        let text = std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|e| panic!("{relative}: {e}"));
        let mut current_fn: Option<String> = None;
        for raw_line in text.lines() {
            let trimmed = raw_line.trim_start();
            if is_whole_line_comment(trimmed) {
                continue;
            }
            let line = before_trailing_comment(raw_line);
            if let Some(name) = extract_fn_name(line) {
                current_fn = Some(name);
            }
            let label_fn = current_fn.as_deref().unwrap_or("<module scope>");
            for pattern in PATTERNS {
                for _ in 0..occurrences(line, pattern) {
                    labels.push(format!("{relative}::{label_fn} {pattern}"));
                }
            }
        }
    }
    labels
}

/// Every `.delete(`/`.delete_with_body(` call site this workspace held
/// when this guard last ran, by file and enclosing function. Reviewed
/// here, not inferred: a line added to or removed from this list is a
/// line a reviewer read and accepted into the inventory, in the same
/// commit that changed the call site itself.
const INVENTORY: &[&str] = &[
    "crates/willikins-providers-appstore/src/client.rs::delete_bundle_id .delete(",
    "crates/willikins-providers-appstore/src/client.rs::delete_profile .delete(",
    "crates/willikins-providers-appstore/tests/it/no_certificate_writes_guard.rs::a_block_comment_mentioning_a_certificate_delete_is_not_flagged .delete(",
    "crates/willikins-providers-appstore/tests/it/no_certificate_writes_guard.rs::a_comment_mentioning_a_certificate_delete_is_not_flagged .delete(",
    "crates/willikins-providers-appstore/tests/it/no_certificate_writes_guard.rs::revoke .delete(",
    "crates/willikins-providers-appstore/tests/it/no_certificate_writes_guard.rs::revoke .delete(",
    "crates/willikins-providers-appstore/tests/it/profile_inverse_mock.rs::delete_is_a_noop_when_already_gone .delete(",
    "crates/willikins-providers-appstore/tests/it/profile_inverse_mock.rs::delete_pins_the_delete_profiles_request .delete(",
    "crates/willikins-providers-appstore/tests/it/profile_inverse_mock.rs::delete_refuses_as_a_conflict_when_not_ours_and_never_deletes .delete(",
    "crates/willikins-providers-appstore/tests/live_capability_cycle.rs::appstore_live_capability_cycle .delete(",
    "crates/willikins-providers-appstore/tests/live_capability_cycle.rs::drop .delete(",
    "crates/willikins-providers-buildkite/src/client.rs::delete_pipeline .delete(",
    "crates/willikins-providers-buildkite/tests/it/pipeline_inverse_mock.rs::delete_deletes_when_as_created_with_no_builds .delete(",
    "crates/willikins-providers-buildkite/tests/it/pipeline_inverse_mock.rs::delete_deletes_when_the_build_loss_is_accepted .delete(",
    "crates/willikins-providers-buildkite/tests/it/pipeline_inverse_mock.rs::delete_is_a_noop_when_already_gone .delete(",
    "crates/willikins-providers-buildkite/tests/it/pipeline_inverse_mock.rs::delete_refuses_as_a_conflict_when_a_build_exists_and_is_not_accepted .delete(",
    "crates/willikins-providers-buildkite/tests/it/pipeline_inverse_mock.rs::delete_refuses_as_a_conflict_when_not_ours .delete(",
    "crates/willikins-providers-doppler/src/client.rs::delete_config .delete(",
    "crates/willikins-providers-doppler/src/client.rs::delete_environment .delete(",
    "crates/willikins-providers-doppler/src/client.rs::delete_project .delete_with_body(",
    "crates/willikins-providers-doppler/src/client.rs::delete_service_token .delete_with_body(",
    "crates/willikins-providers-doppler/tests/it/branch_config_inverse_mock.rs::delete_deletes_when_as_created .delete(",
    "crates/willikins-providers-doppler/tests/it/branch_config_inverse_mock.rs::delete_is_a_noop_when_already_gone .delete(",
    "crates/willikins-providers-doppler/tests/it/branch_config_inverse_mock.rs::delete_refuses_as_a_conflict_when_drifted .delete(",
    "crates/willikins-providers-doppler/tests/it/config_inverse_mock.rs::delete_deletes_when_as_created .delete(",
    "crates/willikins-providers-doppler/tests/it/config_inverse_mock.rs::delete_is_a_noop_when_already_gone .delete(",
    "crates/willikins-providers-doppler/tests/it/config_inverse_mock.rs::delete_refuses_as_a_conflict_when_drifted .delete(",
    "crates/willikins-providers-doppler/tests/it/project_inverse_mock.rs::delete_deletes_when_as_created .delete(",
    "crates/willikins-providers-doppler/tests/it/project_inverse_mock.rs::delete_is_a_noop_when_already_gone .delete(",
    "crates/willikins-providers-doppler/tests/it/project_inverse_mock.rs::delete_refuses_as_a_conflict_when_drifted .delete(",
    "crates/willikins-providers-doppler/tests/it/service_token_inverse_mock.rs::delete_deletes_the_matching_slug_when_as_created .delete(",
    "crates/willikins-providers-doppler/tests/it/service_token_inverse_mock.rs::delete_is_a_noop_when_already_gone .delete(",
    "crates/willikins-providers-doppler/tests/it/service_token_inverse_mock.rs::delete_refuses_as_a_conflict_when_drifted .delete(",
    "crates/willikins-providers-doppler/tests/live_project_member_cycle.rs::delete_project .delete_with_body(",
    "crates/willikins-providers-doppler/tests/live_project_member_cycle.rs::delete_service_account .delete(",
    "crates/willikins-providers-doppler/tests/live_secret_name_gate_cycle.rs::delete .delete_with_body(",
    "crates/willikins-providers-doppler/tests/live_write_cycle.rs::delete .delete_with_body(",
    "crates/willikins-providers-doppler/tests/live_write_cycle.rs::step_9a_underscore_environment .delete(",
    "crates/willikins-providers-fake/src/tools/appstore_profile_ensure.rs::delete_on_an_already_gone_profile_reports_already_gone .delete(",
    "crates/willikins-providers-fake/src/tools/appstore_profile_ensure.rs::delete_on_an_already_gone_profile_reports_already_gone .delete(",
    "crates/willikins-providers-fake/src/tools/appstore_profile_ensure.rs::standing_as_created_then_delete_removes_the_profile .delete(",
    "crates/willikins-providers-fake/src/tools/buildkite_pipeline_ensure.rs::delete_refuses_an_unaccepted_build_loss .delete(",
    "crates/willikins-providers-fake/src/tools/buildkite_pipeline_ensure.rs::delete_removes_the_pipeline_and_its_build_count .delete(",
    "crates/willikins-providers-fake/src/tools/doppler_branch_config_ensure.rs::standing_as_created_then_delete_removes_the_branch_config .delete(",
    "crates/willikins-providers-fake/src/tools/doppler_config_ensure.rs::delete_removes_the_config_and_its_secrets .delete(",
    "crates/willikins-providers-fake/src/tools/doppler_project_ensure.rs::delete_on_an_already_gone_project_reports_already_gone .delete(",
    "crates/willikins-providers-fake/src/tools/doppler_project_ensure.rs::delete_refuses_a_drifted_project_and_changes_nothing .delete(",
    "crates/willikins-providers-fake/src/tools/doppler_project_ensure.rs::delete_removes_the_project_and_its_configs_and_secrets .delete(",
    "crates/willikins-providers-fake/src/tools/doppler_service_token_ensure.rs::delete_on_an_already_gone_token_reports_already_gone .delete(",
    "crates/willikins-providers-fake/src/tools/doppler_service_token_ensure.rs::standing_as_created_then_delete_removes_the_token .delete(",
    "crates/willikins-providers-fake/src/tools/github_repo_ensure.rs::delete_on_an_already_gone_repo_reports_already_gone .delete(",
    "crates/willikins-providers-fake/src/tools/github_repo_ensure.rs::delete_refuses_a_drifted_repo_and_changes_nothing .delete(",
    "crates/willikins-providers-fake/src/tools/github_repo_ensure.rs::delete_removes_the_repo_and_its_actions_secrets_and_scaffolds .delete(",
    "crates/willikins-providers-github/src/client.rs::delete_repo .delete(",
    "crates/willikins-providers-github/tests/it/live_write_cycle.rs::drop .delete(",
    "crates/willikins-providers-github/tests/it/live_write_cycle.rs::step_8_delete .delete(",
    "crates/willikins-providers-github/tests/it/repo_inverse_mock.rs::delete_deletes_when_as_created .delete(",
    "crates/willikins-providers-github/tests/it/repo_inverse_mock.rs::delete_is_a_noop_when_already_gone .delete(",
    "crates/willikins-providers-github/tests/it/repo_inverse_mock.rs::delete_refuses_as_a_conflict_when_drifted .delete(",
    "crates/willikins-providers-github/tests/live_new_repository_cycle.rs::drop .delete(",
    "crates/willikins-providers-github/tests/live_new_repository_cycle.rs::step_11_delete_and_recount .delete(",
    "crates/willikins-providers-github/tests/live_scaffold_cycle.rs::drop .delete(",
    "crates/willikins-providers-github/tests/live_scaffold_cycle.rs::step_9_delete_and_recount .delete(",
    "crates/willikins-providers-http/src/http.rs::delete .delete(",
    "crates/willikins-providers-http/src/http.rs::delete_with_body .delete(",
    "crates/willikins-providers-http/src/http.rs::delete_with_body_fails_with_the_provider_error_on_a_client_error .delete_with_body(",
    "crates/willikins-providers-http/src/http.rs::delete_with_body_is_retried_on_503_then_succeeds .delete_with_body(",
    "crates/willikins-providers-http/src/http.rs::delete_with_body_sends_the_body_and_succeeds_on_204 .delete_with_body(",
    "crates/willikins-providers-http/src/http.rs::put_and_delete_are_retried_like_get .delete(",
    "crates/willikins-providers-http/tests/it/adversarial_retry.rs::a_delete_is_attempted_exactly_four_times .delete(",
    "crates/willikins-providers-signoz/src/client.rs::delete_ingestion_key .delete(",
];

/// Additions (in `found` but not `expected`) and removals (in `expected`
/// but not `found`), each reported once per duplicate, since a plain set
/// difference would hide "the same label now appears twice".
fn diff(found: &[String], expected: &[String]) -> (Vec<String>, Vec<String>) {
    let mut found_remaining = found.to_vec();
    let mut expected_remaining = expected.to_vec();
    let mut i = 0;
    while i < found_remaining.len() {
        if let Some(pos) = expected_remaining
            .iter()
            .position(|e| e == &found_remaining[i])
        {
            expected_remaining.remove(pos);
            found_remaining.remove(i);
        } else {
            i += 1;
        }
    }
    (found_remaining, expected_remaining)
}

#[test]
fn delete_call_sites_in_provider_crates_match_the_pinned_inventory() {
    let root = repo_root();
    let mut found = call_site_labels(&root);
    found.sort();

    assert!(
        found.len() > 20,
        "the walk found only {} call site(s); it is not reading the provider crates",
        found.len()
    );
    assert!(
        found
            .iter()
            .any(|l| l.starts_with("crates/willikins-providers-github/src/client.rs")),
        "the walk never reached willikins-providers-github/src/client.rs"
    );

    let mut expected: Vec<String> = INVENTORY.iter().map(ToString::to_string).collect();
    expected.sort();

    let (additions, removals) = diff(&found, &expected);
    assert!(
        additions.is_empty() && removals.is_empty(),
        "the live `.delete(`/`.delete_with_body(` call sites in the provider crates no longer \
         match this guard's pinned INVENTORY. Review each line below, then edit INVENTORY in \
         the same commit that changed the call site.\n\
         Added (now live, not yet in INVENTORY):\n{}\n\
         Removed (in INVENTORY, no longer live):\n{}",
        additions.join("\n"),
        removals.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_doc_comment_mentioning_either_pattern_is_not_a_site() {
        assert!(is_whole_line_comment("/// calls `.delete(` on the client"));
        assert!(is_whole_line_comment(
            "// also mentions .delete_with_body( in prose"
        ));
    }

    #[test]
    fn a_trailing_comment_does_not_trigger_the_scan() {
        let line = before_trailing_comment("foo() // not real: .delete(bar)");
        assert_eq!(occurrences(line, ".delete("), 0);
        let line = before_trailing_comment("real.delete(bar) // and a mention too: .delete(baz)");
        assert_eq!(
            occurrences(line, ".delete("),
            1,
            "only the code before the comment marker counts"
        );
    }

    #[test]
    fn deleted_and_delete_all_are_not_the_pattern() {
        assert_eq!(occurrences("thing.deleted(x)", ".delete("), 0);
        assert_eq!(occurrences("thing.delete_all(x)", ".delete("), 0);
    }

    #[test]
    fn the_delete_with_body_definition_has_no_leading_dot() {
        let line = "    pub fn delete_with_body(&self, path: &str, body: &impl Serialize) {";
        assert_eq!(occurrences(line, ".delete_with_body("), 0);
        assert_eq!(extract_fn_name(line), Some("delete_with_body".to_string()));
    }

    #[test]
    fn a_closure_attributes_to_its_enclosing_named_function() {
        let text = "\
pub(crate) fn delete_repo(&self, repo: &GitHubRepo) -> Result<(), ProviderError> {
    self.retry_secondary_limit(|| self.http.delete(&path))
}
";
        let mut current_fn: Option<String> = None;
        let mut label = None;
        for line in text.lines() {
            if let Some(name) = extract_fn_name(line) {
                current_fn = Some(name);
            }
            if occurrences(line, ".delete(") > 0 {
                label = current_fn.clone();
            }
        }
        assert_eq!(label, Some("delete_repo".to_string()));
    }

    #[test]
    fn multi_line_signatures_still_name_the_function_on_the_fn_line() {
        assert_eq!(
            extract_fn_name("    pub fn delete_pipeline("),
            Some("delete_pipeline".to_string())
        );
    }

    #[test]
    fn infn_helper_is_not_mistaken_for_a_function_named_helper() {
        assert_eq!(extract_fn_name("let infn_helper = 1;"), None);
    }

    #[test]
    fn the_only_exempt_file_is_this_guard_itself() {
        assert!(is_exempt(
            "crates/willikins-cli/tests/it/delete_call_sites_guard.rs"
        ));
        assert!(!is_exempt(
            "crates/willikins-providers-github/src/client.rs"
        ));
    }

    #[test]
    fn diff_reports_an_addition_and_a_removal_separately() {
        let found = vec!["a".to_string(), "b".to_string()];
        let expected = vec!["a".to_string(), "c".to_string()];
        let (additions, removals) = diff(&found, &expected);
        assert_eq!(additions, vec!["b".to_string()]);
        assert_eq!(removals, vec!["c".to_string()]);
    }

    #[test]
    fn diff_reports_a_duplicate_as_a_second_addition() {
        let found = vec!["a".to_string(), "a".to_string()];
        let expected = vec!["a".to_string()];
        let (additions, removals) = diff(&found, &expected);
        assert_eq!(additions, vec!["a".to_string()]);
        assert!(removals.is_empty());
    }
}
