# Milestone 3g: adversarial pass — render and write (T1, G1, G2)

**Date:** 2026-09-30
**Task:** the independent attacker's pass over the "Render and write" group of
`docs/plans/2026-09-30-milestone-3g-file-writing.md`. The attacker wrote none of it.
**Subject:** T1 — `b4694fc`, `5d6915c` (`repo.file.render`). G1 — `ff8ac77`, `e3e9410`, `6fc0b5f`
(the GitHub client's git-database reads, local blob sha, `createCommitOnBranch`). G2 — `b2cd1dc`,
`3a44741`, `20d8f3f`, `0cd6791` (`github.scaffold.ensure`, its fake twin, catalog registration).
**Method:** read the plan in full, including its addenda, then the code: `willikins-tools`
`repo_file_render.rs` and `template_render.rs`; `willikins-providers-github` `client.rs`,
`tools/scaffold_ensure.rs`, `tests/scaffold_ensure_mock.rs`, `tests/scaffold_fake_agrees_with_live.rs`;
`willikins-providers-fake` `tools/github_scaffold_ensure.rs` and `state.rs`; `willikins-types` `repo.rs`
and `text.rs`; `willikins-core` `plan.rs`'s `plan_one`. Then mutations. Every mutation followed the same
steps: copy the file into the pass's scratchpad, apply one exact-text replacement, run the narrowest test
target, copy the saved file back, `touch` it, and confirm byte identity with `cmp` (exit 0 every time).
Never `git checkout`, `reset` or `stash`. **No live call and no provider API was made.** L1 is not
written yet, and B1 is not in this group.

## 0. The state the pass started from

`main` at `0cd6791`, clean apart from `goal.txt` and the host-maintenance todo (both left alone).
Nothing uncommitted, nothing red. Baseline, scoped: `cargo test -p willikins-tools -p
willikins-providers-github -p willikins-providers-fake` (every suite green).

**One deliverable of this group was missing.** E2's addendum deferred six negative fixtures to the tasks
that land their tools: `secret-into-repo-file.yaml` to T1, a `RepoFile` literal on a list element to G2,
and the four tool-free `TemplateSource`/`RepoFile` input and default fixtures to either. Neither T1 nor
G2 added them, so acceptance 4's "negative fixtures … each exactly one error" was unmet at document
level (the refusals themselves were proved in `willikins-core/tests/check.rs`). This pass adds all six
(`3e3d766`).

## 1. The questions, and what the evidence says

### Can a secret or a `Text` reach a committed file? No

The only producer of `RepoFile` in either catalog is `repo.file.render`
(`grep` over every `src/`: the one `scalar("RepoFile")` output). Its inputs are `RepoPath`,
`TemplateSource` and `list<TemplateValue>`, all public; the only conversion row into any of the new
types is `AppleBundleIdentifier => TemplateValue`; no row targets `TemplateSource` or `RepoFile`, and
none may (the engine pass pinned it). `TemplateSource` and `RepoFile` cannot be inputs or defaults, and
a `RepoFile` cannot be a literal. `github.scaffold.ensure` writes exactly `files` plus the marker, and the
marker's bytes are the fixed header plus the blob shas and paths of `files`. The commit body is
`Seeded by willikins. Marker: <marker>.`; the headline is a `CommitHeadline` (public, one line, 72
characters), so a caller may choose the commit *message* but no file byte. The six fixtures of `3e3d766`
now prove each wall at document level.

### Can a `TemplateValue` break out of YAML, JSON, plist, Starlark or bash? Not by its grammar

`[A-Za-z0-9_][A-Za-z0-9._/-]*`, at most 255 characters, with no `{`, so a value cannot inject a
placeholder either (the scanner runs over the template, never over the rendered text, and a value cannot
contain `{{`). In an XML comment a value may hold `--` (malformed, not an escape: `>` is refused). The
engine pass's placement caveat stands unchanged: a value in a command position, a path position
(`x/../../y`) or an unquoted YAML plain scalar (`null`, `true`, `1e3`) is chosen by the caller. That is
W1's to review in its rendered snapshots.

### Can the scaffold overwrite or delete anything? It could, in two ways; both are now refused

Decision (c) says "overwriting is never silent, because it never happens". The read table decides
`Absent` from a non-recursive tree walk, and `Absent` becomes a `createCommitOnBranch` whose additions
land at every declared path. Two routes turned an occupied path into `Absent`:

1. **A path beneath an existing file.** `resolve_one_path` returned `PathEntry::Absent` when an
   intermediate segment existed but was not a tree (G1's addendum flagged it; G2 left it as is). A git
   tree cannot hold a blob `ios` and a directory `ios/`, so a commit adding `ios/BUILD.bazel` can only
   fail, or succeed by replacing the file `ios` with a directory — a silent deletion. GitHub's actual
   behaviour is unprobed, and it does not need probing: refusing is safe whichever way GitHub behaves.
   Fixed: `PathEntry::UnderNonDirectory`, which the tool's existing arms turn into `Conflict` naming the
   seed path, or `Foreign` for the marker (`9422d3e`).
2. **A truncated tree listing.** GitHub's `git-tree` body carries a required `truncated` flag; the
   client never read it. A truncated listing can omit the entry a declared path names, which read as
   `Absent`. Fixed: a truncated listing is a body-free `Provider` failure (`281e0ef`).

The fake had the matching gap and one more of its own: as a flat path-to-content map it knew no
directories, so a seed path that is an existing directory — `Conflict` on the live tool, since a tree is
a non-blob — planned `Create` on the fake. That broke acceptance 8's "the fake twin agrees" before this
pass. The fake now treats a path with a key beneath it as a directory and a path beneath a key as under
a file (`9422d3e`).

A third, lesser shape problem: declared paths nested under one another (`a` and `a/b`, or a file and
the marker either way round). Knowable from inputs alone, so decision (b) puts it "before any request";
instead the live tool sent the commit and left GitHub to refuse it (a `Provider` failure after the
retry loop, never destructive), and the fake wrote both paths into its map and reported success.
Fixed on both sides as `Invalid` before any request (`3b4120e`).

Otherwise the write path holds: the marker is checked first and a `Present` scaffold never reads or
writes a seed path; a differing seed, a `100755` mode, a symlink or submodule is a `Conflict` naming
paths only; `additions` exclude byte-equal seeds and never name a path the read did not see as absent
or equal; the tool never deletes (`createCommitOnBranch` is sent `additions` only).

### Can a moved head cause a duplicate or a lost commit? No

`expectedHeadOid` is the head the decision was made against, so a commit lands only on exactly the tree
that was read. Every failure is resolved by a re-read, never by the error body: `Present` → unchanged
(someone, perhaps this very call behind a lost response, landed it; no second commit); same head →
the original failure (nothing landed, and retrying would fail the same way; mutation M3); moved head and
still `Absent` → retry against the new head, at most three attempts (mutation M6). A concurrent second
run cannot duplicate: whichever commit lands first makes the other's compare-and-swap fail and its
re-read `Present`. The one imprecision is by design: a commit that landed behind a lost response
reports `changed: false`.

### Does the stored pipeline configuration leak from the gate?

Out of this group: the bootstrap gate is B1's and has not landed. Nothing in T1, G1 or G2 reads
Buildkite.

### Plan-to-apply drift, and every existing document

Outputs are pass-through only (`repo`, `branch`, `marker`), so a busy `main` moving between plan and
apply changes no fingerprint; `ensure` re-reads at the current head. `repo.file.render` is pure and
deterministic. **Every existing document plans byte-identically**: the characterization snapshot's diff
over this pass is 24 added lines (the six new fixtures) and nothing else, and `git diff 0cd6791` touches
no existing document under `workflows/`.

## 2. Mutations

| # | File | Mutation | Result | Killed by |
| --- | --- | --- | --- | --- |
| M1 | `willikins-tools/src/repo_file_render.rs` | render bound `>` → `>=` | killed (T1's fix-commit claim verified) | `the_projected_bound_is_exactly_the_bound_repo_file_itself_enforces` |
| M2 | `willikins-tools/src/repo_file_render.rs` | the closing space after the index made optional | **survived** all 15 tests: every malformed case failed earlier in the scan | new: `refuses_a_placeholder_missing_its_closing_space` (`eac5c41`) |
| M3 | `willikins-providers-github/src/tools/scaffold_ensure.rs` | the same-head guard removed, so a same-head failure retries | killed | `ensure_a_same_head_failure_is_reported_as_provider_with_one_attempt` |
| M4 | `…/scaffold_ensure.rs` | marker first line `==` header → `starts_with(header)` | **survived** the whole crate: the only foreign first line tested was `not ours` | new: `read_reports_foreign_when_the_markers_first_line_only_starts_with_the_header`, `agrees_on_a_marker_whose_first_line_only_starts_with_the_header` (`9dec0ce`) |
| M5 | `willikins-providers-github/src/client.rs` | `create_commit_on_branch` no longer sorts `additions` | killed | `create_commit_on_branch_sends_the_pinned_body_sorted_by_path_and_returns_the_oid`, `ensure_commits_only_the_non_equal_files_plus_a_marker_in_the_shared_values_format` |
| M6 | `…/scaffold_ensure.rs` | `MAX_COMMIT_ATTEMPTS` 3 → 4 | killed | `ensure_gives_up_after_three_attempts_on_a_persistently_moving_head` |
| M7 | `…/client.rs` (after the fix) | a path under a non-directory reads `Absent` again (the shipped G1 behaviour) | killed by five tests: the client's resolve test, two mock tests, two parity tests | `a_seed_path_beneath_an_existing_file_conflicts_and_is_never_committed` and others (`9422d3e`) |
| M8 | `willikins-providers-fake/src/tools/github_scaffold_ensure.rs` (after the fix) | the fake's occupancy check always `false` (the shipped G2 fake) | killed by three parity tests, including the directory case that predates this pass | `agrees_on_a_seed_path_that_is_an_existing_directory` and others (`9422d3e`) |

M2 and M4 were re-run with their new tests in place; each new test failed under the mutation and passed
once it was restored. The truncated-tree and nested-path tests were run red against the shipped code
before their fixes (`a_truncated_tree_is_a_provider_failure_never_absent`: `read` returned `Ok`;
`agrees_on_declared_paths_nested_under_one_another`: the fake's `ensure` succeeded).

## 3. Findings

**Real defects, fixed test-first:**

1. A seed path or the marker beneath an existing file read `Absent` and would be committed, replacing
   that file with a directory or failing. Now `Conflict`/`Foreign` (`9422d3e`).
2. A truncated tree listing read as complete. Now a `Provider` failure (`281e0ef`).
3. The fake planned `Create` for a seed path that is an existing directory, where the live tool refuses
   (acceptance 8 parity). Fixed with 1 (`9422d3e`).
4. Nested declared paths reached GitHub (live) or were accepted (fake). Now `Invalid` before any
   request, both sides (`3b4120e`).
5. Acceptance 4's six negative fixtures, deferred by E2 to T1 and G2, were never added. Added
   (`3e3d766`).

**Test gaps, closed:** M2 (`eac5c41`), M4 (`9dec0ce`).

**Recorded, not changed:**

- `{{ 01 }}` and `{{ 1 }}` both name index 1. Harmless — the value is still grammar-checked and the
  index still range-checked and matched — so it stays; pinning a canonical form would only reject
  templates that render identically.
- `repo.file.render`, like `template.render`, fails `plan` with `Invalid` when an input is `Unknown`,
  rather than predicting an unknown file. Walter's renders take literals and input identifiers, so it
  never bites today; W1 should keep every render's inputs known at plan (the engine pass's item 4).
- `get_blob` fetches the marker's blob without a size bound. A foreign multi-megabyte file at the marker
  path costs memory once per read. Low severity; a `size` check on the tree entry would bound it.
- The fake still has no notion of a missing branch: the live tool answers `NotFound` for one, the fake
  plans `Create`. Walter names `main`, which exists; L1 or W1 may want a fake-state switch for it.
- A `CommitHeadline` may be a workflow input, so a caller may choose the commit message (never a file
  byte). Consistent with decision (e) as written.

## 4. Commits

- `eac5c41` Pin that repo.file.render refuses a placeholder missing its closing space (M2).
- `9dec0ce` Pin that a scaffold marker's first line must equal the header exactly (M4).
- `281e0ef` Fix github.scaffold.ensure: a truncated tree listing is a failure, never Absent.
- `9422d3e` Fix github.scaffold.ensure: a path beneath an existing file is occupied, not absent (live and
  fake).
- `3b4120e` Fix github.scaffold.ensure: refuse declared paths nested under one another (live and fake).
- `3e3d766` The six acceptance-4 fixtures (`secret-into-repo-file.yaml`, `repo-file-literal-in-scaffold.yaml`,
  `template-source-input.yaml`, `template-source-default.yaml`, `repo-file-input.yaml`,
  `repo-file-default.yaml`) with their acceptance tests and characterization entries (additions only).
- This record and the plan's dated addendum.

**Gates run, all scoped and green:** `cargo fmt --all --check`; `cargo clippy --all-targets -D warnings`
on `willikins-tools`, `willikins-providers-github`, `willikins-providers-fake`, `willikins-dsl`;
`cargo test` on `willikins-providers-github` and `willikins-providers-fake` (whole crates),
`willikins-tools` (`repo_file_render`), `willikins-dsl --test acceptance`, `willikins-cli --test
adversarial --test acceptance_11_mcp_parity` (both sweep every document); `cargo check -p
willikins-types`. The full workspace gate, and its guards over the new fixtures, are the coordinator's.
