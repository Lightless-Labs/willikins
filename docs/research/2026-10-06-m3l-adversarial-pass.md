# Milestone 3l, task X1: adversarial pass on the new-repository scaffold

**Date:** 2026-10-06
**Run as:** Sonnet 5. The plan's task table delegates X1 to opus; the harness dispatched this
session to run it anyway. The `Co-Authored-By` trailers on `5d96227` and `e0d9114` record the
same. Whether the coordinator wants a second, independent pass before marking the plan Completed
is theirs to decide.
**Scope:** `crates/willikins-providers-github/src/client.rs`,
`crates/willikins-providers-github/src/tools/scaffold_ensure.rs`, their mock tests under
`crates/willikins-providers-github/tests/it/`, and the fake's mirror in
`crates/willikins-providers-fake/src/tools/github_scaffold_ensure.rs`, against
`docs/plans/2026-10-05-milestone-3l-new-repositories.md` (decisions (a)-(f), SHARED VALUES,
trust boundaries 1-5) after tasks S1-S4, F1, D1, L1 had all landed.

## Method

Baseline (`cargo test -p willikins-providers-github --lib --test it`, 39 + 135 = 174 tests)
green before any mutation. Each mutation: edit the source in place, wait for a quiet host
(`pgrep -x cargo` and `pgrep -f 'cargo[-]sweep'` both empty for 3 consecutive seconds), run the
scoped test target, read the output body, then restore the file from a `cp`'d copy and confirm
byte-for-byte identity with `cmp` (and `git diff --quiet`) before moving to the next mutation. A
mutation that a test caught is recorded as closed; a mutation that survived would have needed a
new test -- none did, once the two gaps below were closed.

## Mutations, each restored byte-identical (`cmp`, confirmed)

| # | File : line | Mutation | Target named in the task | Caught by |
| --- | --- | --- | --- | --- |
| 1 | `client.rs`, `FirstFileBody` | Added a `sha: String` field to the first-file `PUT`'s request body | "a `PUT` carrying `sha` (an overwrite path)" | `create_first_file_puts_the_encoded_path_with_exactly_message_content_and_branch` (`json_body` rejects the extra key) |
| 2 | `scaffold_ensure.rs`, `observe_without_branch` | Deleted the `has_any_branch` check, treating every branchless-read repository as empty regardless of whether it actually has other branches | "a `PUT` on a repository that is not empty" | 14 tests, including the non-empty-missing-branch `NotFound` test, the `409`-non-empty `Provider` test, and a `scaffold_fake_agrees_with_live` parity row |
| 3 | `scaffold_ensure.rs`, `observe_without_branch`'s `GET /repos` match | Widened the `404`-only `RepositoryAbsent` guard to match any error (so a `301` reads `Absent` too) | "`Absent` returned where the read table says an error should be returned instead (e.g. a `301`...)" | `a_301_from_get_repos_fails_loudly_never_absent` |
| 4 | `scaffold_ensure.rs`, `observe_without_branch`'s default-branch check | Short-circuited `default_branch == *branch` to always `true` | "the first-file write landing on a branch other than the default" | `read_and_ensure_conflict_when_the_empty_repositorys_default_branch_differs`, `scaffold_fake_agrees_with_live::agrees_on_an_empty_repository_with_a_mismatched_default_branch` |
| 5 | `scaffold_ensure.rs`, `initialize_empty_repository` | On a successful `PUT`, returned a synthesized `Absent { head: String::new(), .. }` immediately, skipping the re-observe entirely | "a decision taken from the `PUT`'s own response body instead of a re-read (violates decision (a) step 3's 'never decides from the `PUT`'s own body')" | The two ref-visibility-poll tests caught it, but **the existing "fresh empty repository" happy-path acceptance test did not** -- see Finding 1 below. New test added. |
| 6 | `scaffold_ensure.rs`, `ensure`'s commit-retry match on `ScaffoldState::Empty` | Re-entered `initialize_empty_repository` a second time from inside the retry loop instead of reporting the original failure | "a second root commit created on resume (should be exactly one `PUT` ever, per decision (a)'s convergence argument)" | The plan's own 2026-10-06 addendum names this exact case as "not covered by an acceptance test... flagged for X1's attack pass". New test added (Finding 2 below). |
| 7 | `client.rs`, `suppress_first_file_response_body` | Added `422` to the `401`/`403` allowlist that is let through with its response body intact | "a body fragment leaking into any error (violates trust boundary 4...)" | `create_first_file_409_422_404_report_the_shared_failure_message_without_echoing_the_body` |

All seven mutations were caught. Five by tests that already existed; two (5 and 6) needed a new
test, both landed in the commit immediately preceding this document's (`5d96227`, in
`crates/willikins-providers-github/tests/it/scaffold_ensure_mock.rs`).

## Findings

### Finding 1 (closed): the fresh-repository happy path doesn't prove the re-observe is consulted

Mutation 5 (skip the re-observe on a successful `PUT`, decide from the `PUT`'s own success
signal) survived `ensure_on_a_fresh_empty_repository_puts_the_root_file_then_commits_the_rest`
undetected. The reason: that test's `createCommitOnBranch` mock matches on
`expectedHeadOid: "root-head"` via `partial_json_body`. Under the mutation, `ensure`'s own first
commit attempt used `expectedHeadOid: ""` instead, which matched no registered mock and so the
mock server answered an unmatched-request error. `ensure`'s own outer retry loop (built for a
genuinely busy branch, 3g decision (b)) caught that as an ordinary `create_commit_on_branch`
failure, re-observed -- this time for real -- read the correct head, and retried successfully.
Every mock the test asserts (`ref_mock`, `repo_mock`, `branches_mock`, `put_mock`, `commit`) still
shows exactly its expected hit count, because mockito does not count an unmatched request against
any mock's `.expect(n)`. The mutation was real and exactly the shape the task named, but the
happy-path test's design could not see it.

**Closed** by
`ensure_decides_the_first_write_landed_from_a_re_observe_never_from_the_puts_own_body`: alongside
the real `createCommitOnBranch` mock keyed on the re-observed head, it plants a second mock keyed
on an **empty** `expectedHeadOid` with `.expect(0)` -- the only value a `PUT`-trusting
implementation has to put there, since it never read a real one.

### Finding 2 (closed): the plan's own flagged addendum case had no test

The plan's 2026-10-06 addendum (task S3) documents a case decisions (a)/(b) are silent on: after a
`createCommitOnBranch` failure, the retry loop's own re-observe can in principle read
`ScaffoldState::Empty` again (the branch's head vanished and the repository reverted to no
branches at all, between the first observe and the re-read). The addendum records the decision
(report the original failure, never re-enter S3's own initialisation) but says plainly: "Not
covered by an acceptance test... flagged for X1's attack pass." The implementation
(`scaffold_ensure.rs:913`) already matches the decision -- this was a documentation gap, not a
code bypass -- but an untested decision is one the next refactor can silently undo.

**Closed** by
`ensure_reports_the_original_commit_failure_when_a_post_failure_reobserve_finds_the_repository_newly_empty`,
which also doubles as the strongest form of "a second root commit created on resume": it pins
zero `PUT`s and zero second `createCommitOnBranch` calls (and zero rules-diagnostic reads, since
this path returns the original error directly rather than through `with_rule_suffix`, unlike the
unmoved-head case right above it in the same file).

## Probes that did not produce a finding

- **Ref-lag poll shape.** If GitHub's `409`-then-`200` sequencing manifests differently in
  practice -- `get_branch_head` answering `409` while `has_any_branch` already reports `true` --
  `observe` would return `repository_unavailable` from `observe_without_branch` instead of
  `Empty`, and `initialize_empty_repository`'s own `?` would propagate it immediately rather than
  consuming a ref-visibility poll. This converges correctly (the next document run re-reads and
  proceeds) but the poll budget (`MAX_REF_VISIBLE_POLLS`) would not be spent the way the plan's
  acceptance 8 describes. Not a bypass -- a shape the live cycle (L1, not yet run) should confirm
  doesn't occur in practice (verify item 1).
- **GitBranchName as message interpolation.** `default_branch_mismatch` and
  `repository_unavailable` interpolate GitHub-reported strings (`default_branch`) into error
  messages. Checked `willikins-types::GitBranchName::parse`: bounded to 100 characters,
  `[A-Za-z0-9._/-]` only, and the value is already parsed into this type (rejecting anything
  wider) before `observe_without_branch` ever uses it in a message. No control characters, no
  unbounded length, no raw GitHub body text reaches a message this way. Not a finding.
- **Happy-path regression in existing mock `.expect(n)` counts.** Diffed every pre-existing test
  function in `scaffold_ensure_mock.rs` between the commit just before milestone 3l started
  touching this file (`34b2bdd`, the 3k test-layout move) and `HEAD`, function body by function
  body. Of 40 pre-existing tests, exactly one changed:
  `read_reports_not_found_when_the_branch_does_not_exist` -- the one the plan's acceptance 3
  explicitly names as gaining the two new mocks decision (b) now routes through. Every other
  pre-existing test, including every happy-path one, is byte-identical. No regression found.
- **Fake/live parity on modelled rows.** `scaffold_fake_agrees_with_live.rs` carries one row per
  case the fake's `BranchExistence` can model (no record, empty+default, empty+mismatch,
  non-empty missing the branch, legacy `None`) plus the shape and content checks (acceptance 12,
  13). All 14 `agrees_on_*`/`agrees_on_a_*` tests pass against the current tree; mutation 2 and 4
  above additionally proved that breaking the live side's own branch-existence or default-branch
  checks is caught by a `scaffold_fake_agrees_with_live` row, not only by a live-side mock test --
  the parity suite does its job. No disagreement found on any row either side models.

## Verdict

Zero bypasses survive. Two real coverage gaps found and closed, both as new tests in commit
`5d96227` (`crates/willikins-providers-github/tests/it/scaffold_ensure_mock.rs`), immediately
preceding this document's own commit. No source file was left mutated; every mutation above was
restored and confirmed byte-identical to the pre-mutation copy before the next one began.

## Gates run (scoped, this task only)

```
cargo fmt --all --check                                                            -- pass
cargo clippy -p willikins-providers-github --all-targets -j 2 -- -D warnings       -- pass
RUST_TEST_THREADS=2 cargo test -p willikins-providers-github -j 2 --no-fail-fast   -- 39 + 137 passed, 0 failed, 3 ignored
```

The full workspace gate is the coordinator's, per CLAUDE.md and this plan's own Gates section.
