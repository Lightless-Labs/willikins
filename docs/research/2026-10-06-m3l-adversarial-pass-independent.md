# Milestone 3l: independent adversarial pass on the new-repository scaffold

**Date:** 2026-10-06
**Run as:** Opus 5.5, the independent opus pass the plan's task X1 intended. The first pass
(`2026-10-06-m3l-adversarial-pass.md`, Sonnet 5, commits `5d96227`, `e0d9114`, `19932d4`) is not
repeated here: its seven mutations (a `sha` in the `PUT` body, a `PUT` on a non-empty repository, a
`301` read as `Absent`, the default-branch check short-circuited, a decision taken from the `PUT`'s
own body, re-entering initialisation from the commit-retry loop, a `422` body let through) all
stay caught.
**Scope:** `crates/willikins-providers-github/src/client.rs`, `src/tools/scaffold_ensure.rs`, their
mock tests under `tests/it/`, the fake scaffold in `crates/willikins-providers-fake`, the D1
document and its test, and the live cycle `tests/live_new_repository_cycle.rs` (read only, never
run), against `docs/plans/2026-10-05-milestone-3l-new-repositories.md` after S1-S4, F1, D1, L1
and X1.

## Method

Each mutation: copy the source to a scratch file, edit it in place, wait for a quiet host
(`pgrep -x cargo` and `pgrep -f cargo-sweep` both empty for three consecutive seconds), run the
scoped tests (`cargo test -p willikins-providers-github --lib --test it`; for the fake,
`-p willikins-providers-fake`, then `--test it scaffold_fake_agrees_with_live::` and
`-p willikins-cli --test it new_repository_scaffold_document::`), read the log body, then restore
from the copy and confirm byte identity with `cmp` (and `git diff --quiet`). A mutation that
survived got a new test, written while the source was back at its original; the test was run green
on the original, and the same mutation was applied again to watch the new test fail.

## Mutations, each restored byte-identical (`cmp`, confirmed)

| # | Where | Mutation | Before this pass | After |
| --- | --- | --- | --- | --- |
| B | `observe_without_branch`, `GET /repos` match | A `403` read as `RepositoryAbsent`, as a `404` is | **Survived** (176 tests green) | Caught by `a_403_from_get_repos_fails_loudly_never_absent` |
| D | `observe_without_branch`, branches listing | A failing branches listing read as "no branches" (`unwrap_or(false)`) | **Survived** | Caught by `a_failing_branches_listing_is_never_read_as_empty` |
| C | `root_file` | Root file is the first declared, not the byte-order smallest | **Survived** | Caught by `ensure_writes_the_byte_order_smallest_file_first_whatever_the_declaration_order` |
| G | `initialize_empty_repository` | The rule suffix applied to exhausted `409`s too | **Survived** | Caught by `exhausted_409s_on_the_first_file_put_never_read_the_rules` |
| A | Fake scaffold | Reworded the fake's repository-absent and default-branch-mismatch messages | **Survived** (fake 283, parity 15, D1 4 tests green) | Caught by `agrees_on_repository_absent` and `agrees_on_an_empty_repository_with_a_mismatched_default_branch` |
| H | `observe_without_branch` | Any `409` on the ref read as "not available yet", even on an empty repository | Caught by two existing tests (`read_reports_absent_when_the_repository_is_empty...`, `read_and_ensure_conflict_when_...differs`) | Unchanged |
| F | Fake scaffold `ensure` | Never records the branch after initialising an empty repository | Caught by the fake's own unit test and D1's run-2 `NoOp` test | Unchanged |

Five of seven survived the whole suite. All five are closed by tests, in two commits:
`e9304f8` (`tests/it/scaffold_ensure_mock.rs`, five tests) and `2dd6573`
(`tests/it/scaffold_fake_agrees_with_live.rs`). No source file changed. The fixes are tests
because in every case the implementation was correct and only the pin was missing.

## Findings

### Finding 1 (closed): a 403 or a failing branches listing could have become a first-file `PUT`

Trust boundary 3 says the first-file `PUT` is made "only on a repository the scaffold has just
observed to be empty". The only test of the `GET /repos` error row was the `301`, and no test made
the branches listing fail. Mutation B, a `403` widened into `RepositoryAbsent`, would plan
`Create` for a repository the token cannot read and then fail at apply with a false "does not
exist". Mutation D, a failing listing read as empty, would `PUT` on a repository whose emptiness
nobody had observed. Neither changed a single test result. Both now fail a test that also pins
`PUT` and `POST /graphql` at `.expect(0)` and the fixed `MISSING_PERMISSION` text.

### Finding 2 (closed): the root file's byte-order rule was untested

Every empty-repository fixture (the mock tests' `three_files`, the D1 document, the live seed
files) declares its byte-order-smallest path first, so a root chosen by declaration order
(mutation C) passed everything. The new test declares `.editorconfig` last and mounts only its
`PUT`.

### Finding 3 (closed): the rule-suffix exclusion for `409` was invisible

SHARED VALUES limits the S4 suffix to "a refused first-file `PUT` (non-409)". The five-`409`s test
asserts the exact message, but mounts no rules mock. A suffix attempt then fails and is silently
omitted, so the message stays the same and mutation G passed. The new test mounts a rules mock that
would answer, at `.expect(0)`.

### Finding 4 (closed): fake/live parity compared kinds, not messages

`willikins-providers-fake` re-types the repository-absent, mismatch and missing-branch messages by
hand. Its own unit test checks `contains("trunk")`, and the parity rows compared
`ToolErrorKind` only. A reworded fake message (mutation A) passed every fake, parity and D1 test.
The three rows now compare messages on `read` and `ensure`.

### Finding 5 (open, for the coordinator): after its own `2xx` `PUT`, ref lag surfaces as `NotFound`, not as a poll

Decision (a) says that after a `2xx` `PUT` whose re-observe "still reads empty", `ensure` polls.
`initialize_empty_repository` polls only on `ScaffoldState::Empty`. Ref lag can take another
form: `git/ref/heads/{branch}` still answers `404` (or `409`) while `branches?per_page=1` already
lists the new branch. In that case `observe_without_branch` returns the missing-branch `NotFound`
("branch `main` does not exist on ...; this tool never creates one"), or the "not available yet"
`Provider` error. `observe(...)?` then propagates it at once. Nothing is wrong and the next run
converges (`Absent`, root file in `already_equal`), but the first run fails, with a message that
says the tool never creates a branch it just created, and the poll budget goes unspent. X1 recorded
only the `409` form, as a probe. The `404` form is the more likely one, because the two endpoints
are separate reads. The same lag would also fail the live cycle's step 8: its scaffold `read`
comes straight after a raw `PUT`. Fixing it means treating those two errors as "not visible yet"
inside the post-`PUT` poll loop only. That is a change to how decision (a) reads decision (b)'s
table, so it is the coordinator's call. L1's live run (steps 6 and 8) shows whether it happens.

### Finding 6 (open, low): the `409` retry `PUT`s two seconds after the last emptiness check

The loop runs `PUT` (409), then a re-observe (still `Empty`), then `pause(UNAVAILABLE_WAIT)`, then
`PUT` again. The write therefore lands about 2 s after the observation that allowed it. If someone
pushes a first branch during that pause, the retry creates the root path on their branch. It cannot
overwrite anything (no `sha`), and the re-observe after it governs the rest, so this is the same
outcome as the observe-then-`PUT` window that decision (a) already accepts. It is only wider.
Pausing *before* the re-observe keeps every recorded wait and request count in acceptance 7 the same
(4 waits and 6 observes for five `409`s, 2 waits and 3 observes for the 409-409-201 case) and
narrows the window to one request. That reorder is recommended, not made here. It would change
source code, and code written by an attacker needs its own attack.

### Finding 7 (open, low): `put_was_ok` means "2xx and parsed", not "2xx"

`create_first_file` goes through `Http::put::<IgnoredAny>`, and `Http::finish` parses even a `2xx`
body. A `201` whose body is not JSON becomes `Err(Some(201))`, relabelled as
`FIRST_FILE_FAILURE_MESSAGE`. If the re-observe that follows still reads empty because of ref lag,
`ensure` stops polling and reports a refusal, with a rule suffix. GitHub documents a JSON body on
`201`, so this is unlikely. `Http::put_empty` (status only) would match decision (a)'s intent to
discard the body, but SHARED VALUES says "parsed as `IgnoredAny`", so the change needs an addendum.

### Finding 8 (open, low): an exhausted `409` reports as `Conflict` with no re-run hint

Five `409`s end in `to_tool_error`, which maps status `409` to `ToolErrorKind::Conflict` with
`FIRST_FILE_FAILURE_MESSAGE`. The plan treats this as availability ("GitHub may still be
creating"), and the S2 "unavailable" row uses `Provider` with "re-run this document". The
acceptance test pins the current kind, so changing it is a plan decision.

## Probes that did not produce a finding

- **Live cycle guard.** `DeleteGuard` holds exactly the three identities built in the test body,
  and step 11 deletes the same three. It is armed after step 1's leftover refusal and before the
  first write. Every write and delete targets a `GitHubRepo` built from `sandbox_org()`, which
  refuses any org but `Willikins-Test`. The listing refuses a full page instead of miscounting.
  Residual risk: the guard deletes by name, not by "this run created it". Two runs started in the
  same second would share names, and one could delete the other's throwaway repositories. Sandbox
  only, and step 1's refusal makes it a race of a few seconds. Not worth a mechanism.
- **The operator's `gh` credential.** No file in scope spawns a process. The live cycle
  authenticates through `Http` with the sandbox token from the environment, and the tools take
  `WILLIKINS_GITHUB_TOKEN` or a bound `token` port. `no_gh_writes_guard` covers the tree.
- **Redirects.** `Http` is built with `max_redirects(0)`, so the `301` row fails loudly in
  production too, not only in a mock that omits `Location`.
- **Body leaks.** `with_rule_suffix` reports only rule types that match `^[a-z_]{1,40}$`. The
  `default_branch` is parsed into `GitBranchName` before any message interpolates it. A `2xx`
  parse failure carries a line and column, never body text.
- **Fake `github.repo.ensure`.** Only its create path writes a record (`branches: Some([])`,
  `default_branch: None`). No other path rebuilds a record and drops the new fields.
- **Seed semantics.** Once the marker has landed, the marker alone decides `Present`; the
  empty-repository code is reachable only from `ScaffoldState::Empty`, which needs a repository
  with no branch at all. A repository that has been scaffolded can never re-enter it.
- **Privacy.** Searched the milestone's diff (`9f5fd35..HEAD`) and its commit messages for the
  operator's org, apps, domains, layout and home paths: nothing. The D1 fixture uses
  `example-org/example-repo`.

## Verify-list addition

The live cycle's step 3 asserts that `branches?per_page=1` on a freshly created repository answers
`200 []`, not `409`. The plan's verify list does not name it, yet decision (b) depends on it: a
`409` there turns every new-repository plan into `to_tool_error`. The coordinator's L1 run settles
it alongside verify item 1.

## Gates run (scoped)

```
cargo fmt --all --check                                                          -- pass
cargo clippy -p willikins-providers-github --all-targets -j 2 -- -D warnings     -- pass
RUST_TEST_THREADS=2 cargo test -p willikins-providers-github -j 2 --no-fail-fast -- 39 + 142 passed, 3 ignored
RUST_TEST_THREADS=2 cargo test -p willikins-providers-github --test it scaffold_fake_agrees_with_live:: -- 15 passed
```

The full workspace gate and the L1 live run are the coordinator's.
