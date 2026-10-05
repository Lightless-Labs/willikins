---
title: "Milestone 3k adversarial pass: one integration-test binary per crate"
created: 2026-10-05
status: complete
area: tests (all thirteen crates), layout guard, journal
related:
  - docs/plans/2026-10-05-milestone-3k-faster-gates.md
  - docs/solutions/tooling/one-binary-test-move-silent-traps.md
---

# Milestone 3k adversarial pass

This pass attacked everything milestone 3k landed through T15: the thirteen per-crate moves into `tests/it/`
(T1, T2, T4–T14), the layout guard (T3), and the docs (T15). The reviewer wrote none of it. No provider was called.
Every mutation ran against the fake, a mock server, or the guard's own filesystem walk.

At the start, the tree was clean apart from the pre-existing untracked `goal.txt` and the cargo-target todo. Neither
was touched. `main` was at `cb65db3`, nothing was uncommitted, and nothing was red.

The priorities, from the task: a test that silently stops running, an undeclared module, a snapshot that moved
without its contents, the journal's `--exact` filter, the proptest seed file, a gated live target folded into `it`,
and the private `operator_*` targets still building.

## Acceptance 1, re-run independently for all thirteen crates

The plan's list recipe was re-run on `cb65db3` for every crate into a separate directory outside the tree
(`m3k/review/`). Each list was diffed against T0's `before/<crate>.txt`.

| Crate | Before | After | Diff |
| --- | --- | --- | --- |
| willikins-cli | 215 | 222 | +7, all `unique_temp_prefixes::*` (acceptance 5's own test, recorded in T14's addendum) |
| willikins-core | 263 | 275 | +12, all `test_layout_guard::*` (T3's guard; **not** recorded in any addendum until now) |
| willikins-dsl | 34 | 34 | empty |
| willikins-journal | 95 | 95 | empty |
| willikins-providers-appstore | 203 | 203 | empty |
| willikins-providers-buildkite | 89 | 89 | empty |
| willikins-providers-doppler | 267 | 267 | empty |
| willikins-providers-fake | 13 | 13 | empty |
| willikins-providers-github | 113 | 113 | empty |
| willikins-providers-http | 27 | 27 | empty |
| willikins-providers-signoz | 24 | 24 | empty |
| willikins-server | 216 | 216 | empty |
| willikins-types | 130 | 130 | empty |

No crate lost a name. Every `Running` line printed under `--tests` is either `unittests` or `tests/it/main.rs`. The
one exception is cli, which also prints the three gitignored private targets: they still build and list, and they
stay unmoved. No gated `live-tests` target appears without the feature. `git diff -M c7076c5 HEAD -- crates` adds or
removes no `#[ignore]`, `#[cfg]` or `#[should_panic]` line outside the two new test modules. This pass's guard fix
adds six more core names (finding 1), so core is now 281.

## Findings and fixes

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | Medium (silent test loss, guard green) | **Layout guard rule 2 counted declarations that rustc never compiles.** `declared_modules` was a line scan for `mod <ident>;`. It skipped `// mod x;` but accepted a `mod x;` inside a `/* */` block comment, one under a preceding `#[cfg(..)]` (which may compile it to nothing) or `#[path = ".."]` (which compiles some other file), and every module under a crate-level `#![cfg(..)]`. In each case `x.rs` silently stops compiling while the guard stays green: the exact loss rule 2 exists to catch. Proven on the real tree (mutation M1): with `mod state_bookkeeping;` wrapped in `/* */` in fake's `main.rs`, the guard passed 12/12 and fake's `it` listed 8 tests instead of 13. | `c93134a`, test first. Five fixture tests (block comment, nested block comment, `cfg` on one line and across lines, `path`, crate-level `#![cfg]`) were red on the old code (5 failed, 13 passed). One non-regression fixture keeps the real `main.rs` shape green: the `#[path]`-declared `common` plus a lint attribute. The new scan strips `//` and nesting `/* */` comments, follows attributes across lines with bracket depth, and errs loud: an attribute it cannot close stops the scan. After the fix: 18/18 green on the real tree, and the same mutation went red (M2). |
| 2 | Medium (silent pass, would reopen on any module rename) | **The journal's cross-process lock tests passed when their child ran no test.** `spawn_probe` re-runs the test binary with `--ignored --exact locking::lock_probe_child` and trusts the exit status. A filter that matches nothing exits 0. Acceptance 4 proved the filter right once, but nothing kept it right: a module rename, or a later split into `tests/it_<area>/`, makes the filter stale again, and the guard cannot see that. Proven (M3): with the filter set back to the pre-move `lock_probe_child`, every child printed `running 0 tests … 95 filtered out` and all three parent tests passed. | `e75cc89`, test first. Before probing, the child writes `<journal>.child-ran`. `spawn_probe` removes the marker before spawning and panics if it is absent afterwards, with a message naming the `--exact` trap and the solutions note. Red with the stale filter (M5: both probe tests failed), green with the right one. The journal list is unchanged (no `#[test]` added): 92 passed and 3 ignored, the same 95 as before, and each child now reports `1 passed`. |

## Mutations

Each mutated file was saved to the scratchpad first. Afterwards it was restored from that copy, `touch`ed, and
`cmp`-confirmed byte-identical before the next mutation. No `git checkout`, `reset` or `stash` was used. Every run
waited for a quiet host and used `-j 2` and `RUST_TEST_THREADS=2`.

| # | Mutation | Result | Proves |
| --- | --- | --- | --- |
| M1 | fake `tests/it/main.rs`: `mod state_bookkeeping;` wrapped in `/* */` (old guard) | guard **green** 12/12, fake list 13 → 8 | finding 1's bypass is real, and silent |
| M2 | M1 again, plus types `tests/it/main.rs`: `#[cfg(any())]` before `mod catalog;` (fixed guard) | guard red: `UndeclaredModule` for both, plus rule 4's `OrphanSnapshot` for types' catalog snapshot | the fix closes both shapes on the real tree |
| M3 | journal `locking.rs`: filter `"locking::lock_probe_child"` → `"lock_probe_child"` (old code) | **green**, every child `running 0 tests` | finding 2's trap is still open after the move, and silent |
| M4 | journal `locking.rs`: first `spawn_probe(&path, "refused")` → `"granted"`, filter correct (plan acceptance 4) | red: the child panics with "nothing holds the lock, so this open must succeed", and the parent fails | the child really runs with the module-qualified filter |
| M5 | M3's stale filter on the fixed code | red: both probe tests fail. Alone, the message reads "the child process ran no test: `--exact lock_probe_child` matched nothing" | finding 2's fix |
| M6 | fake `tests/proptest-regressions/ensure_properties.txt`: an unparsable line appended. Decoys with unparsable lines at the old `tests/ensure_properties.proptest-regressions` and beside the source at `tests/it/ensure_properties.proptest-regressions` | proptest prints `…/tests/proptest-regressions/ensure_properties.txt:8: unparsable line, ignoring` once per proptest (2), and nothing for either decoy | the moved seed file is the one proptest reads (acceptance 6), and the old location is dead |
| M7 | dsl `tests/it/snapshots/it__acceptance__characterization_of_every_document.snap`: one content line changed, run with `INSTA_UPDATE=no` | red: insta names `crates/willikins-dsl/tests/it/snapshots/…` and fails. No `.snap.new` or `.pending-snap` was left anywhere | the moved snapshot is the one compared, and its contents carry weight (acceptance 3) |
| M8 | github `tests/live_scaffold_cycle.rs` (gated) moved into `tests/it/` and declared in `main.rs` | every `cargo test` in the workspace fails at manifest parsing: "can't find `live_scaffold_cycle` test at `tests/live_scaffold_cycle.rs`". (`cargo metadata --no-deps`, by contrast, exits 0 and silently omits the target) | a gated target folded into `it` is loud in every gate; the guard needs no rule for it |
| M9 | cli `prerendered_identifier_guards.rs`: prefix `willikins-cli-prerendered-guard-` → `willikins-cli-test-` (T14's rename undone) | red: `willikins-cli-test- is used by more than one file: …/cli.rs, …/prerendered_identifier_guards.rs` | acceptance 5 is live |

## Checked and accepted (no change)

- **Declared modules equal files on disk** in all thirteen `tests/it/main.rs`. Each declares `common` at most once
  (seven crates), with `#[path = "../common/mod.rs"]`. No moved file still says `mod common;`. `tests/it/` has no
  subdirectory apart from `snapshots/`.
- **All 52 snapshots** are git renames (similarity 74–99%). Each one's diff is exactly two lines, `-source:` and
  `+source:`, and each new `source:` names an existing `crates/<c>/tests/it/<m>.rs` matching its `it__<m>__` prefix.
  `tests/snapshots/` holds nothing tracked in any crate. In cli it holds only the 35 gitignored private snapshots.
- **The ten gated targets** sit top-level, unmoved, with `[[test]]` blocks byte-identical to `c7076c5` (no
  `Cargo.toml` changed in the milestone). The six ungated live-shaped files in `it` are exactly (d3)'s list.
- **Tree-walking guards still reach the moved files.** `secret_literal_guard`, `no_gh_writes_guard` and
  `no_certificate_writes_guard` recurse. Their self-exemptions name `tests/it/…`; a stale one flags its own file,
  which is loud. `sink_token_guard`'s cross-crate read of `willikins-cli/tests/it/acceptance.rs` panics on a
  missing file, so it is loud too. No test globs `tests/*.rs` in a way the move could empty: every other
  `read_dir` reads fixtures, workflows or `src/`.
- **Shared-process hazards.** No test uses `CARGO_CRATE_NAME`, `module_path!`, `set_current_dir`, or a binary-name
  assumption. The only `current_exe` is the journal child (finding 2). The `CARGO_TARGET_TMPDIR` scratch names are
  distinct (`expose_secret_guard_second_pass`, `sink_token_guard_second_pass`, `test_layout_guard_<case>`). Outside
  cli, every temporary directory comes from `tempfile`.
- **No stale `--test <stem>` command** for a moved stem outside `docs/plans` and `docs/research` (verify item 5
  re-run).
- **Privacy.** The local hooks' pattern list matches nothing in the `HEAD` tree, in any message from `c7076c5..HEAD`,
  in any added line of the milestone's diff, or in any changed path. None of the three private target stems, and none
  of the private snapshots' test names, appears in any tracked file. The test-name lists stayed outside the tree.

## Recorded, not fixed

- **Core's acceptance-1 exception was unrecorded.** T3 added twelve `test_layout_guard::*` names to core's binary,
  and only cli's +7 had an addendum. Both are now in the plan's addendum, with this pass's six.
- **The silent panic hook swallowed sibling failures' messages (observed).** In M5, both failing tests printed no
  panic message when run as a module; run alone, the message appears. `a_panic_while_the_journal_is_held_releases_the_lock`
  replaces the global hook with a silent one around its panicking thread. This is the (d9) risk the plan said to
  record, not fix, and it has now happened. A likely cure, to verify first: drop the hook swap in that test and in
  `core/apply_adversarial.rs`. libtest's per-test output capture is inherited by spawned threads, so the panic text
  would go to that test's own captured output, which is shown only if that test fails.
- **`unique_temp_prefixes` limits.** It scans `tests/it/*.rs` only, not `tests/common/mod.rs`, which compiles into the
  same binary. It ignores brace-less fixed names (`temp_dir().join("willikins-x")`), which carry no pid and would
  collide across processes too. It reads a 4-line window after each `temp_dir(` call, so a prefix held in a `const`
  is invisible. None of these occurs today.
- **The guard does not see a second multi-file target.** A new `tests/<dir>/main.rs` is auto-discovered as another
  test binary, and rule 1 looks only at top-level files. That costs a binary but loses no test (the plan's own
  `tests/it_<area>/` split contingency would take this shape). An inline `mod b { mod c; }` in `main.rs` would also
  count `c` as declaring `tests/it/c.rs`. A renamed proptest module orphans its `tests/proptest-regressions/<m>.txt`,
  which is the seed-file twin of rule 4's orphan snapshot. Only fake has a seed file today.
- **Folding the private targets into `it` cannot be done privately.** `.gitignore` anchors them to `tests/`, and rule 2
  would demand a `mod operator_x;` in the tracked `main.rs`. "Needs the operator" item 2 can only be a separate
  private directory (for example `tests/operator/main.rs`, which no rule inspects), never `it`.
- **Prose still names the old `tests/<stem>.rs` paths** in `src/` doc comments, `Cargo.toml` comments, fixture
  READMEs, `workflows/fixtures/*.yaml` headers, todos and `docs/solutions/`. Some of these mentions are correct,
  because a stem like `live_write_cycle` is gated in some crates and moved in others. Five tracked snapshots
  (core `schema_generation` ×3, journal `schema_generation`, server `mcp_server`) bake one such `src/` doc path into
  their schemars descriptions, so fixing those comments churns the snapshots. None is executable: the
  `.railway/railway.ts`, `deploy/teardown_test.sh` and `README.md` hits are all comments or prose. This belongs in a
  docs pass, not here.
