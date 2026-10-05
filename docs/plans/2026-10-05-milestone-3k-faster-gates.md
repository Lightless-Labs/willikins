# Milestone 3k: faster gates, one integration-test binary per crate

**Created:** 2026-10-05 (from `todos/2026-09-29-faster-gates.md`)
**Reviewed:** 2026-10-05 (portfolio review of the five plans of 2026-10-05: 3k, 2b, 3l, 3m, 3n)
**Addendum:** 2026-10-05 — portfolio order: this milestone runs **first**, before 3n, 3l, 2b and 3m, because every other lane adds `tests/*.rs` files and `--test <stem>` gates; landing first means nothing is moved twice and every later gate is cheaper. Recipe step 2 now reads the gated targets from each `Cargo.toml` (`[[test]]` blocks with `required-features`), not from SHARED VALUES, because 3l and 3m each add a gated live target and either may land first if the coordinator reorders. T15 edits `AGENTS.md` together with `CLAUDE.md`: the two are byte-identical copies.

## Goal

Make the full four-command gate faster and its build output smaller, without changing what any test checks.

Today every top-level `crates/*/tests/*.rs` file is its own crate, and each one becomes its own statically linked
test executable. Run with default features, the public tree builds **164** of them. The operator's machine builds
**167**: the three gitignored `operator_*` targets are added. Ten more targets are gated behind `live-tests`. This
milestone moves each crate's default-built integration tests into **one** binary, `tests/it/main.rs`, with one module
per former file. That means **13** binaries in the public tree, 16 on the operator's machine, and the 10 gated
targets unchanged. Each move is `git mv` (history follows with `git log --follow`). Each crate is one task and one
commit, green alone.
**Addendum:** 2026-10-05 (coordinator, T0) — baseline at `c7076c5`, warm, after touching `willikins-core/src/lib.rs`: clippy 67 s; `test --no-run --timings` 382 s; the full test run 251 s (200 suites, 3242 passed, 19 ignored, 0 failed). `target/` 17,783,836 KB (`deps` 6,294,040 KB, `incremental` 11,331,980 KB). Integration-test executables: 167, 3082 MB (cli 20/226 MB, core 21/220, dsl 4/64, journal 13/173, appstore 15/372, buildkite 10/218, doppler 24/581, fake 6/84, github 12/283, http 5/103, signoz 5/105, server 23/582, types 9/70). The 13 `before/<crate>.txt` lists are outside the tree.

Done when:
- every crate's default-built integration tests build as one binary;
- the per-crate list of test names is the same set before and after (acceptance 1);
- a layout guard keeps the result from eroding (acceptance 2);
- the coordinator's after-measurement shows the gain against the baseline (T0 and T16).

## Out of scope

- **`CARGO_INCREMENTAL=0`**, option 2 of the other session's note. It is a separate lever with its own trade-off:
  interactive lanes rebuild after small edits, and incremental compilation helps there. It needs its own measurement,
  after this milestone. The 4.5 GB of incremental state that integration-test crates hold today (see "Expected
  effect") shrinks with the crate count anyway.
- Profile changes (`debug`, `split-debuginfo`, per-package `opt-level`). `[profile.dev] debug = 1` is already set.
- The 10 gated live targets. They are not built by the gate, so they cost nothing there (decision (d3)).
- The three gitignored `operator_*` targets in `willikins-cli` and their 35 gitignored snapshots. They stay exactly
  where they are (decision (d4)).
- Unit-test binaries (`src/**` `#[cfg(test)]`) and doctests. Their snapshots under `src/snapshots/` do not move.
- The untracked `todos/2026-09-23-cargo-target-60gb-integration-test-binaries.md`. It belongs to another session.
  This plan reads it and never edits, adopts, stages or deletes it.
- The orphan helper modules `crates/willikins-providers-appstore/tests/common/mod.rs` and
  `crates/willikins-providers-signoz/tests/common/mod.rs`. No test declares them, so they are not compiled today.
  They are recorded here as a finding and left untouched; a later todo can delete or wire them.

## Sources, verbatim

The Cargo book, `doc/book/src/reference/cargo-targets.md` (rust-lang/cargo `master`, fetched 2026-10-05 with
`curl` from raw.githubusercontent.com):

> Each integration test results in a separate executable binary, and `cargo test` will run them serially. In some
> cases this can be inefficient, as it can take longer to compile, and may not make full use of multiple CPUs when
> running the tests. If you have a lot of integration tests, you may want to consider creating a single integration
> test, and split the tests into multiple modules. The libtest harness will automatically find all of the `#[test]`
> annotated functions and run them in parallel. You can pass module names to `cargo test` to only run the tests
> within that module.

> Setting the keys `autolib`, `autobins`, `autoexamples`, `autotests`, or `autobenches` to `false` in the
> `[package]` section will disable auto-discovery of the corresponding target type. [...] Disabling automatic
> discovery should only be needed for specialized situations.

> The `required-features` field specifies which features the target needs in order to be built. If any of the
> required features are not enabled, the target will be skipped.

`doc/book/src/guide/project-layout.md`, same fetch:

> If a binary, example, bench, or integration test consists of multiple source files, place a `main.rs` file along
> with the extra modules within a subdirectory of the `src/bin`, `examples`, `benches`, or `tests` directory. The
> name of the executable will be the directory name.

`doc/book/src/reference/features.md`: "Cargo sets features in the package using the `rustc` `--cfg` flag".

The Rust Reference, `src/items/modules.md` (rust-lang/reference `master`, fetched 2026-10-05):

> For `path` attributes on modules not inside inline module blocks, the file path is relative to the directory the
> source file is located.

For the three test libraries, the source read was the locked version in the local cargo registry (`Cargo.lock`
pins insta 1.48.0, trybuild 1.0.121, proptest 1.11.0):

- **insta** `src/runtime.rs` `get_snapshot_filename`: the file is
  `<dir of file!()>/snapshots/<module_path!() with "::" replaced by "__">__<name>.snap`.
  `src/comparator.rs` `DefaultComparator::matches` compares `reference.contents() == test.contents()` only. Metadata
  (the `source:` and `expression:` header lines) is compared only under `INSTA_REQUIRE_FULL_MATCH`. A snapshot with
  no reference file fails the assertion (`update_result != InPlace && !force_pass`).
- **trybuild** `src/flock.rs`: "High-quality lock to coordinate different #[test] functions within the *same*
  integration test crate" (an intra-process `Mutex`), plus a file lock across processes. The project directory is
  `target/tests/trybuild/<package name>` (`run.rs`). It does not depend on the test binary's name. Features are read
  from the running binary's own fingerprint (`features.rs`).
- **proptest** `src/test_runner/failure_persistence/file.rs`: the default is `SourceParallel("proptest-regressions")`.
  It walks up from the test's source file "until a directory containing a file named `lib.rs` or `main.rs` is found".
  The file then sits at `<that dir's parent>/proptest-regressions/<relative path>.txt`. "If no `lib.rs` or `main.rs`
  can be found, [...] this behaves like `WithSource`", which writes `<source>.proptest-regressions` beside the source.

## Expected effect, measured from the tree as it is (2026-10-05)

Read from `target/debug/deps/*.d`, which maps each executable to its source file (script in T0), and from the m3j
coordinator gate log of 2026-10-04.

| Measure | Today | After (expected) |
| --- | --- | --- |
| Default-built integration-test targets, public tree | 164 | 13 |
| Same, operator's machine | 167 | 16 |
| Gated `live-tests` targets | 10 | 10 (unchanged) |
| Test executables linked by `cargo test --workspace` (integration + 15 unit/bin) | 179 | 28 |
| Test crates checked by `cargo clippy --workspace --all-targets` (integration only) | 164 | 13 |
| Integration-test executable bytes, one generation | 3.08 GB (median 17 MB, max 60 MB) | 0.5–0.8 GB (each crate's binary is the union of its files; the sum of per-crate maxima is 0.36 GB) |
| Integration-test incremental directories (`target/debug/incremental`) | 6,615 directories, 4.5 GB | about a tenth as many directories; the size is to be measured (T16) |
| `target/` total | 17 GB (6.0 GB `deps`, 10 GB `incremental`, 0.5 GB `tests/trybuild`) | measured in T16 |
| m3j warm full gate (core touched) | 13 min: clippy 1m02, test build 7m06, 200 suites running 209 s summed | measured in T16 |

Per crate (default-built integration targets today → after; lines of tracked test code):

| Crate | Today | After | Gated, unchanged | Lines |
| --- | --- | --- | --- | --- |
| willikins-cli | 17 (+3 private) | 1 (+3 private) | `live_smoke` | 11,510 |
| willikins-core | 21 | 1 | — | 13,592 |
| willikins-dsl | 4 | 1 | — | 1,490 |
| willikins-journal | 13 | 1 | — | 4,518 |
| willikins-providers-appstore | 15 | 1 | `live_write_cycle`, `live_capability_cycle` | 10,456 |
| willikins-providers-buildkite | 10 | 1 | `live_write_cycle`, `live_bootstrap_cycle` | 5,172 |
| willikins-providers-doppler | 24 | 1 | `live_write_cycle`, `live_project_member_cycle`, `live_secret_name_gate_cycle` | 12,085 |
| willikins-providers-fake | 6 | 1 | — | 1,875 |
| willikins-providers-github | 12 | 1 | `live_scaffold_cycle` | 6,088 |
| willikins-providers-http | 5 | 1 | — | 873 |
| willikins-providers-signoz | 5 | 1 | `live_write_cycle` | 1,025 |
| willikins-server | 23 | 1 | — | 12,318 |
| willikins-types | 9 | 1 | — | 2,172 |

Where the time goes: the 164 links and 164 per-crate dependency-metadata loads become 13. The duplicated
monomorphisations of shared generic dependency code (serde, ureq, mockito, rmcp) also drop from one copy per file to
one per crate. The front-end work on the test code itself stays about the same. In the run phase, 167 process
launches become 16, and libtest's two threads stay busy across what used to be separate files, instead of draining
one small binary at a time.

## Decisions

### (d1) `tests/it/main.rs`, with auto-discovery left on

Each crate gets `tests/it/main.rs` (the target is named `it`, after its directory). It declares one `mod <stem>;`
per moved file, and each `tests/<stem>.rs` becomes `tests/it/<stem>.rs` unchanged apart from the edits in (d2).

`autotests = false` with explicit `[[test]]` entries is rejected because it fails open twice:
1. a new `tests/foo.rs`, or one a task forgets to move, would silently stop compiling, so its tests would stop running;
2. the gitignored `operator_*.rs` targets would stop being discovered on the operator's machine, and a tracked
   manifest cannot name them without leaking them.

With auto-discovery on, a stray top-level file still runs, only as an extra binary: a cost, never a lost test. The
layout guard (d7) turns that cost into a red test too. Nothing is added to any `Cargo.toml`, and every existing
`[[test]] ... required-features` entry stays byte-identical.

### (d2) What each moved file needs, and nothing else

| Construct today | In `tests/it/<stem>.rs` | Why |
| --- | --- | --- |
| `mod common;` (core, journal, server, cli, github, doppler, buildkite) | removed; the file says `use crate::common;` if it names `common::` outside a `use`, and every `use common::` becomes `use crate::common::` | a `mod common;` in a non-mod-rs file would look for `tests/it/<stem>/common.rs`. The module is declared once in `main.rs` (below) |
| `#[path = "support/apple_error_report.rs"] mod apple_error_report;` (appstore `redaction.rs`) | `#[path = "../support/apple_error_report.rs"]` | the Reference: relative to the directory of the source file, now `tests/it/` |
| `include_str!("../src/tools/secret_name_gate.rs")` (doppler `secret_name_gate_mock.rs`) | `include_str!("../../src/tools/secret_name_gate.rs")` | `include_str!` resolves relative to the including file |
| `.args(["--ignored", "--exact", "lock_probe_child"])` (journal `locking.rs`) | `"locking::lock_probe_child"` | `--exact` matches the full test path, which now has the module prefix. Left unchanged, the child runs no test and exits 0: a **silent** pass. Acceptance 4 pins it |
| self-exemption paths in guards (see (d6)) | `tests/it/...` | each guard skips itself by path |
| `env!("CARGO_MANIFEST_DIR").join("tests").join("fixtures")`, `tests/ui`, `tests/derive/fail`, `tests/conversions/fail` | unchanged | rooted at the package, not at the source file. Fixture and trybuild directories do not move |

`main.rs`, per crate, in this order: a `//!` doc paragraph (the workspace's `missing_docs` lint, plus a pointer to
this plan); then, only for crates whose helpers a moved file uses,
`#[path = "../common/mod.rs"] mod common;` once, followed by a blank line; then the `mod <stem>;` lines,
alphabetical, as rustfmt's `reorder_modules` keeps them. `common` is declared **once per binary, never per module**.
Declaring it in several modules would trip `clippy::duplicate_mod` and would compile several partial copies, each with
its own dead code. A single shared copy only adds use sites, so it cannot create a dead-code warning that one file
alone did not already have.

Checked and found absent, so nothing to do: inner `#![...]` attributes in a top-level test file (only
`common/mod.rs` carries `#![allow(dead_code)]`, which is module-scoped either way); `crate::` paths in test code (only
in comments); `#[macro_export]` (none; the two `variant_kinds!` definitions in core are textually scoped per module);
`harness = false` targets; link-time registries (no `inventory`, `linkme` or `ctor` in `Cargo.lock`);
`std::env::set_var` (forbidden by `unsafe_code = "forbid"` under edition 2024); and conflicting trait impls (every impl
in a test file involves a type local to that file, and those types are distinct per module).

### (d3) The gated live targets stay top-level

The 10 `required-features = ["live-tests"]` targets keep their files, `[[test]]` entries and documented
`--test live_*` commands. They are not built by the gate, so folding them in would buy nothing. It would also force
every one of them to compile whenever `it` builds with `--features live-tests`, and would change commands that past
plans and `docs/solutions/` quote. Their helpers stay where they are, `tests/common/mod.rs` and
`tests/support/apple_error_report.rs`, shared by path with `it`.

The **ungated** live-shaped tests (`live_probe` in github, doppler, buildkite and appstore; `live_write_cycle` in
github; `live_catalog` in doppler) compile in every gate today, so they move into `it` like any other file. Their doc
comments' run commands become `cargo test -p <crate> --test it <stem>:: -- --ignored --nocapture`.

### (d4) The operator's private targets stay where they are

`crates/willikins-cli/tests/operator_*.rs` and `crates/willikins-cli/tests/snapshots/operator_*` keep their paths
and their `.gitignore` lines. They remain three separate binaries, each with module path `operator_<x>`, so insta
keeps naming their snapshots `tests/snapshots/operator_<x>__<name>.snap`. None of them declares `mod common;` or a
`#[path]`, and none references a tracked test file, so moving the tracked files does not touch them.

Every move in every task is driven by `git ls-files`, never by a shell glob. An implementer on the operator's machine
must not be able to sweep a gitignored file into `it`, or into a commit. Folding the three private targets into one
private binary is the operator's own business, not this milestone's.

`crates/willikins-core/tests/operator_acknowledge_document.rs` is tracked and generic. Despite its prefix, it moves
like every other core file.

### (d5) Snapshots move by rename, and only their `source:` line is edited

For module `<m>` in crate `<c>`, insta now computes `crates/<c>/tests/it/snapshots/it__<m>__<rest>.snap`, where it
used to compute `crates/<c>/tests/snapshots/<m>__<rest>.snap`. The task, in the same commit:

1. `git mv` each tracked `tests/snapshots/<m>__<rest>.snap` to `tests/it/snapshots/it__<m>__<rest>.snap`;
2. rewrites its `source: crates/<c>/tests/<m>.rs` header line to `source: crates/<c>/tests/it/<m>.rs`. Nothing else
   in the file changes, so git records a rename at about 99% similarity.

Step 2 is for truthful metadata and for a run under `INSTA_REQUIRE_FULL_MATCH`. The default comparison reads
contents only, so it cannot hide a regression. A missed **rename** is loud: insta finds no reference, writes a
`.snap.new` and fails. This milestone never runs `cargo insta accept` or sets `INSTA_UPDATE`. A snapshot whose
contents would change is a defect in the move, not something to accept.

The 52 tracked snapshots that move:

| Crate | Count | Former prefix(es) |
| --- | --- | --- |
| core | 15 | `plan__` (1), `schema_generation__` (14) |
| dsl | 1 | `acceptance__` |
| journal | 5 | `schema_generation__` |
| types | 1 | `catalog__` |
| appstore | 7 | `catalog_parity__` |
| buildkite | 4 | `catalog_parity__` |
| doppler | 13 | `catalog_parity__` |
| github | 4 | `catalog_parity__` |
| signoz | 1 | `catalog_parity__` |
| server | 1 | `mcp_server__` |
| cli | 0 tracked | the 35 gitignored `operator_*` snapshots stay (d4) |

Comments that quote an old snapshot path are updated in T15:
`crates/willikins-cli/tests/prerendered_identifier_guards.rs` and
`crates/willikins-providers-appstore/tests/capability_documents.rs` both name the dsl characterization snapshot.

### (d6) Guards that know their own path, or another file's

| Guard (crate) | Edit | Task |
| --- | --- | --- |
| `secret_literal_guard` (core) | `is_exempt` becomes `crates/willikins-core/tests/it/secret_literal_guard.rs` | core |
| `no_certificate_writes_guard` (appstore) | `is_exempt` and its own unit test become `tests/it/no_certificate_writes_guard.rs`. The negative example `tests/bundle_id_ensure_mock.rs` becomes `tests/it/bundle_id_ensure_mock.rs` | appstore |
| `no_gh_writes_guard` (cli) | `is_exempt` and `the_exempt_files_are_the_two_guards_that_must_say_gh_by_name` become `crates/willikins-cli/tests/it/no_gh_writes_guard.rs` | cli |
| `sink_token_guard` (core) | `a_tests_directory_file_calling_sink_token_new_is_not_itself_a_failure` reads `willikins-cli/tests/acceptance.rs` and becomes `willikins-cli/tests/it/acceptance.rs` | **cli task**, same commit as the move (cross-crate) |

A stale self-exemption fails loudly: the guard flags its own file. Every guard's walk already recurses, and the
`sink_token_guard` and `expose_secret_guard` walks skip any directory named `tests` at any depth, so `tests/it/` is
covered with no further change. Their `CARGO_TARGET_TMPDIR` scratch directories (`sink_token_guard_second_pass`,
`expose_secret_guard_second_pass`) are already distinct, which matters now that they share a process.

### (d7) A ratcheting layout guard, so the result cannot erode

New in core: `tests/it/test_layout_guard.rs` (T3). It walks every `crates/*/` package with a `tests/` directory. It
holds `PENDING: &[&str]`, the crates not yet moved. For every crate **not** in `PENDING`, it asserts:

1. every top-level `tests/*.rs` is either named by a `[[test]]` block in that crate's `Cargo.toml` that has a
   `required-features` line, or, in `willikins-cli` only, matches `operator_*.rs`;
2. `tests/it/main.rs` exists, and every `tests/it/*.rs` other than `main.rs` is declared by a `mod <stem>;` line in
   it. An undeclared file is otherwise never compiled: a **silent** loss;
3. `tests/snapshots/` is absent, or (in `willikins-cli` only) holds only `operator_*` entries;
4. every `tests/it/snapshots/*.snap` starts with `it__<m>__` for a module `<m>` that `main.rs` declares. This catches
   an orphan left by a later module rename;
5. no `tests/*.proptest-regressions` file exists. Since `tests/it/main.rs` exists, proptest's `SourceParallel` walk
   now stops at `tests/it/`, so seeds beside the source are **silently** never read (decision (d8)).

For every crate **in** `PENDING`, it asserts the opposite: no `tests/it/` yet, so a half-done move cannot hide behind
the list. `PENDING` ends empty (T14). The constant and its check then stay, so a new crate starts in the target
layout.

`Cargo.toml` is read as text, a line scan of `[[test]]` blocks for `name = "..."` and `required-features`. No TOML
crate enters the tree. The guard's own tests build throwaway layouts under
`CARGO_TARGET_TMPDIR/test_layout_guard_<case>` (one directory per case, removed first), one per rule, each proving
the rule fires.

Ratchet discipline in each crate task: first remove the crate from `PENDING` and see the guard go red; then move;
then see it green.

### (d8) proptest regression seeds move to where proptest will look

One file exists: `crates/willikins-providers-fake/tests/ensure_properties.proptest-regressions`. After the move,
proptest resolves `tests/it/ensure_properties.rs` → `tests/proptest-regressions/ensure_properties.txt`, so the fake
task runs
`git mv crates/willikins-providers-fake/tests/ensure_properties.proptest-regressions crates/willikins-providers-fake/tests/proptest-regressions/ensure_properties.txt`.
The same rule covers `convergence_property.rs`, whose `ProptestConfig { source_file: Some(file!()), .. }` uses the
default persistence. Proptests elsewhere (cli `adversarial`, types `naming_properties`, `naming_v1_properties`,
`message_bounds`, core `check_adversarial`, dsl `property`) have no seed file today. Any they write later lands under
`tests/proptest-regressions/`, which is tracked (not gitignored).

### (d9) Tests that used to be alone in their process now share it

`cargo test` runs binaries one at a time, so two files of one crate never ran concurrently. Now they do, on
`RUST_TEST_THREADS=2`. Checked:

- **Temp directories (cli).** Most file-local helpers build `temp_dir()/<prefix>-<pid>-<label>` and some
  `remove_dir_all` it first. The pid is now shared, so prefixes must be unique per file. One collision exists:
  `willikins-cli-test-` is used by both `cli.rs` and `prerendered_identifier_guards.rs`. The cli task renames the
  latter to `willikins-cli-prerendered-guard-`, and acceptance 5 pins prefix uniqueness. Every other crate uses the
  `tempfile` crate.
- **Panic hooks.** `core/apply_adversarial.rs` and `journal/locking.rs` swap the global panic hook around a
  `catch_unwind`. This was already process-wide among the tests of their own file. Now the window can also hide
  another module's panic message, never its failure. Leave it as is, and note it in the task report if a failure ever
  prints no message.
- **Ports and globals.** Every server test binds `127.0.0.1:0`; `http://127.0.0.1:1` is only an unreachable base URL.
  The `static` items (`GATE`s, `CAPTURED_BODY`, `UNKNOWN_INPUT_HITS`, `LazyLock`s) are module-private. One
  `tracing` subscriber install exists (`server/adversarial_13.rs`, `try_init`, which tolerates an existing one).
- **Live recordings.** The writes to `fixtures/*/live/` are all in `#[ignore]` tests.
- **Timing-sensitive tests** (`server/blocking_pool_13.rs`, the journal locks) may meet more concurrent load. See
  Risks.

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| Binary directory and target name | `tests/it/`, target `it`, entry `tests/it/main.rs` |
| Shared helpers declaration in `main.rs` | `#[path = "../common/mod.rs"] mod common;` |
| appstore support path in `tests/it/redaction.rs` | `#[path = "../support/apple_error_report.rs"]` |
| doppler `include_str!` in `tests/it/secret_name_gate_mock.rs` | `"../../src/tools/secret_name_gate.rs"` |
| journal child test filter | `"locking::lock_probe_child"` |
| Snapshot rename | `tests/snapshots/<m>__<rest>.snap` → `tests/it/snapshots/it__<m>__<rest>.snap`; `source:` line → `crates/<c>/tests/it/<m>.rs` |
| proptest seed file (fake) | `tests/proptest-regressions/ensure_properties.txt` |
| cli temp prefix rename | `prerendered_identifier_guards.rs`: `willikins-cli-test-` → `willikins-cli-prerendered-guard-` |
| Layout guard | `crates/willikins-core/tests/it/test_layout_guard.rs`, `const PENDING: &[&str]`, scratch `CARGO_TARGET_TMPDIR/test_layout_guard_<case>` |
| `PENDING` at T3 (11) | `willikins-cli`, `willikins-journal`, `willikins-providers-appstore`, `willikins-providers-buildkite`, `willikins-providers-doppler`, `willikins-providers-fake`, `willikins-providers-github`, `willikins-providers-http`, `willikins-providers-signoz`, `willikins-server`, `willikins-types` |
| Gated targets that stay top-level | cli `live_smoke`; appstore `live_write_cycle`, `live_capability_cycle`; buildkite `live_write_cycle`, `live_bootstrap_cycle`; doppler `live_write_cycle`, `live_project_member_cycle`, `live_secret_name_gate_cycle`; github `live_scaffold_cycle`; signoz `live_write_cycle` |
| Private targets that stay top-level | `crates/willikins-cli/tests/operator_*.rs` (gitignored) |
| Ungated live-shaped run command | `cargo test -p <crate> --test it <stem>:: -j 2 -- --ignored --nocapture` |
| Test-name lists (T0, each task) | outside the tree: `"${XDG_CACHE_HOME:-$HOME/.cache}/willikins/m3k/{before,after}/<crate>.txt"`. Never committed (the operator's machine lists private test names) |

The list recipe, per crate (acceptance 1). Names from a former `tests/<stem>.rs` binary are prefixed `<stem>::`;
names from `it` are taken as printed; unit-test and doctest binaries are dropped:

```sh
C=willikins-dsl   # the crate
RUST_TEST_THREADS=2 cargo test -p "$C" --tests -j 2 -- --list > "$OUT.raw" 2>&1
awk '
  /Running tests\/it\/main\.rs/ { m = "@it"; next }
  /Running tests\//            { f = $2; sub(/^tests\//, "", f); sub(/\.rs$/, "", f); m = f; next }
  /Running /                   { m = ""; next }
  m != "" && /: test$/         { n = $0; sub(/: test$/, "", n); print (m == "@it" ? n : m "::" n) }
' "$OUT.raw" | sort > "$OUT"
```

`--list` includes `#[ignore]` tests, so the ignored live-shaped tests are compared too.

## Gates

Per task, scoped, never the full workspace gate (the coordinator runs that). Before **each** cargo command, wait for 3
consecutive seconds in which both `pgrep -x cargo` and `pgrep -f cargo-sweep` print nothing, polled every second,
then start the command in the same shell. Use `-j 2` and `RUST_TEST_THREADS=2`. Run each command separately, in the
background with a 600,000 ms timeout, and read the output file's body, never piped through `tail` or `tee`. A linker
"missing .rcgu.o" or `E0463` means the host's cargo-sweep ran: `cargo clean -p <crate>` and rebuild. Never edit
tracked files while cargo builds.

```
cargo fmt --all --check
cargo clippy -p <crate> --all-targets -j 2 -- -D warnings
cargo clippy -p <crate> --all-targets --features live-tests -j 2 -- -D warnings   # crates with a live-tests feature
RUST_TEST_THREADS=2 cargo test -p <crate> -j 2 --no-fail-fast
RUST_TEST_THREADS=2 cargo test -p willikins-core --test it test_layout_guard -j 2  # from T3 on
cargo check -p willikins-types -j 2                                                # types task only
```

Plus the list recipe after the move, diffed against T0's `before/<crate>.txt`. The diff must be empty.

Commit as soon as a commit's scoped gates are green, with `git commit --only <paths>`. The paths are every renamed
source and destination, plus each edited file: `git mv` stages both sides of a rename, and `--only` must name both.
Never `git add -A`, `commit -a`, stash, `checkout --` or `reset`. Each implementer uses its own `Co-Authored-By`
trailer. Nobody pushes. Local hooks refuse any commit or message naming the operator's private setup: never
`--no-verify`; rewrite with placeholders.

## Tasks

One lane at a time on `main`, in this order: each task rebuilds a crate that later tasks depend on, and the host
cannot build two at once. One crate per task, one commit per task unless stated, test first (the `PENDING` ratchet
or the list diff is the red step).

The per-crate recipe (R), referenced below:
1. Remove the crate from `PENDING` in core's layout guard (from T4 on). Run the guard: red.
2. List the files with
   `git ls-files 'crates/<c>/tests/*.rs' | grep -E '^crates/<c>/tests/[^/]+\.rs$'`. Drop every file named by a
   `[[test]]` block of `crates/<c>/Cargo.toml` that carries `required-features` (the same line scan the layout guard
   uses; SHARED VALUES' list is the tree as of 2026-10-05, and a lane that landed since may have added one). Compare
   the dropped set with SHARED VALUES and report any difference. `git mv` each remaining file to `tests/it/`.
3. Write `tests/it/main.rs` (decision (d2)).
4. Apply the crate's (d2), (d6), (d8) and (d9) edits, and fix doc comments inside the moved files that name their
   own old path or an old `--test <stem>` command.
5. `git mv` the crate's tracked snapshots and rewrite their `source:` lines (d5).
6. Run the gates and the list diff. Commit: "<crate>'s integration tests build as one binary".

| # | Task | Delegate to |
| --- | --- | --- |
| T0 | **Baseline (coordinator, no commit).** At `main` before T1: `touch crates/willikins-core/src/lib.rs`, then time `cargo clippy --workspace --all-targets -j 2 -- -D warnings`, then `RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-run --timings` (keep `target/cargo-timings/cargo-timing.html` outside the tree), then the full test run. Record `du -sk` of `target/`, `target/debug/deps`, `target/debug/incremental`, and the executable count and bytes per crate with the `.d`-mapping script below. Capture the 13 `before/<crate>.txt` lists. No `cargo clean`: a cold baseline costs a full rebuild and is the operator's call | coordinator |
| T1 | **willikins-dsl (pilot):** 4 files, 1 snapshot; no `common`, no guard yet. Proves the recipe on the smallest crate. Steps 2–6 of R. Gates: `-p willikins-dsl` | sonnet implements |
| T2 | **willikins-core:** 21 files (`operator_acknowledge_document.rs` included), `common`, 15 snapshots, `secret_literal_guard` self-path (d6). `sink_token_guard` keeps its cli path until T14. Gates: `-p willikins-core` | sonnet implements |
| T3 | **The layout guard** (d7), test first: one fixture test per rule and per `PENDING` direction, then the real walk over the workspace with `PENDING` = the 11 crates in SHARED VALUES. Green with dsl and core moved. One commit. Gates: `-p willikins-core` | sonnet implements, opus attacks |
| T4 | **willikins-types:** 9 files, 1 snapshot. `derive_compile_fail.rs` and `derive_pass.rs` join `it`; trybuild's intra-process lock serialises its two suites, and `tests/derive/fail`, `tests/conversions/fail` stay put. Gates: `-p willikins-types`, `cargo check -p willikins-types` | sonnet implements |
| T5 | **willikins-providers-fake:** 6 files, the seed-file move (d8). Gates: `-p willikins-providers-fake` | sonnet implements |
| T6 | **willikins-providers-http:** 5 files; `credential_compile_fail.rs` joins `it`; `tests/ui` stays. Gates: `-p willikins-providers-http` | sonnet implements |
| T7 | **willikins-journal:** 13 files, `common`, 5 snapshots, the `locking::lock_probe_child` filter (d2), fixtures stay. Gates: `-p willikins-journal` | sonnet implements |
| T8 | **willikins-providers-github:** 12 files (ungated `live_probe`, `live_write_cycle` in), `live_scaffold_cycle` stays, `common`, 4 snapshots, doc-comment run commands (d3). Gates: `-p willikins-providers-github`, plus `--features live-tests` clippy | sonnet implements |
| T9 | **willikins-providers-signoz:** 5 files, `live_write_cycle` stays, 1 snapshot. The orphan `tests/common/` is untouched. Gates: `-p willikins-providers-signoz`, plus `--features live-tests` clippy | sonnet implements |
| T10 | **willikins-providers-buildkite:** 10 files, two gated stay, `common` (shared by `it`'s `live_probe` and the gated targets), 4 snapshots, and `fixtures/buildkite/README.md`'s `--test live_probe` command. Gates: `-p willikins-providers-buildkite`, plus `--features live-tests` clippy | sonnet implements |
| T11 | **willikins-providers-doppler:** 24 files (`live_probe`, `live_catalog` in), three gated stay, `common`, 13 snapshots, the `include_str!` path. Gates: `-p willikins-providers-doppler`, plus `--features live-tests` clippy | sonnet implements |
| T12 | **willikins-providers-appstore:** 15 files, two gated stay, the `support/` path, `no_certificate_writes_guard` self-path (d6), 7 snapshots. The orphan `tests/common/` is untouched. Gates: `-p willikins-providers-appstore`, plus `--features live-tests` clippy | sonnet implements |
| T13 | **willikins-server:** 23 files, `common`, 1 snapshot. Gates: `-p willikins-server` | sonnet implements |
| T14 | **willikins-cli:** 17 files; `live_smoke` and the private `operator_*` stay; `common` (shared with `live_smoke`); `no_gh_writes_guard` self-path; core's `sink_token_guard` path (d6), in the same commit; the temp-prefix rename (d9); `PENDING` becomes empty. Gates: `-p willikins-cli` (on the operator's machine this includes the private targets, which must stay green and unmoved), `--features live-tests` clippy, `-p willikins-core --test it sink_token_guard`, the layout guard | sonnet implements, opus attacks |
| T15 | **Docs, one commit, no code:** `docs/HANDOFF.md` (guard paths, the run-one-test form `cargo test -p <crate> --test it <module>::`, the binary count), the CLAUDE.md commands note (the same run-one-test line), with `AGENTS.md` edited identically (`cmp CLAUDE.md AGENTS.md` silent), the snapshot-path comments named in (d5), `docs/solutions/tooling/` (a new entry on the three silent traps: the `--exact` filter, the proptest seed location, an undeclared module), and the faster-gates todo (`status: done`), with the Open TODOs table regenerated. Historic plans and research records are not rewritten | sonnet implements |
| T16 | **After-measurement and close (coordinator).** Repeat T0's protocol on the final tree, then the full four-command gate. Record both tables in a **Completed** header and an addendum here. If `target/` and the gate time did not drop, say so and why before closing. Tell the other session (or the operator) the numbers, since the untracked note's "done when" depends on them | coordinator |

The `.d`-mapping script for T0 and T16 (read-only over `target/`; run from the repository root):

```python
import os, re, collections
D = "target/debug/deps/"
per = collections.defaultdict(dict)
for n in os.listdir(D):
    p = D + n
    if not (os.path.isfile(p) and os.access(p, os.X_OK)) or n.endswith((".d", ".dylib", ".so", ".rlib", ".rmeta")):
        continue
    d = p + ".d"
    if not os.path.exists(d):
        continue
    m = re.search(r"crates/(willikins-[a-z-]+)/tests/([A-Za-z0-9_]+)(?:/main)?\.rs", open(d).readline())
    if m:
        st = os.stat(p)
        key = m.group(2)
        if key not in per[m.group(1)] or per[m.group(1)][key][1] < st.st_mtime:
            per[m.group(1)][key] = (st.st_size, st.st_mtime)
for c, t in sorted(per.items()):
    print(c, len(t), round(sum(s for s, _ in t.values()) / 1e6), "MB")
```

Before publishing numbers from it, drop the `operator_*` rows, whose names are private.

## Acceptance tests

1. **No test lost, none added** (every crate task). The crate's `after/<crate>.txt` equals T0's
   `before/<crate>.txt`, line for line, `#[ignore]` tests included. The count of passed plus ignored tests in the
   crate's test output equals the count before.
2. **Layout guard** (T3, then every task). Each of (d7)'s five rules has a fixture case proving it fires: an
   undeclared `tests/it/x.rs`; a stray top-level `tests/y.rs` with no gated `[[test]]`; a leftover
   `tests/snapshots/m__n.snap`; an `it__gone__n.snap` for an undeclared module; a leftover `.proptest-regressions`.
   It also fires for a crate in `PENDING` that already has `tests/it/`. On the real tree it is green after T3, with
   `PENDING` shrinking one crate per task, and empty after T14.
3. **Snapshots unchanged** (every task with snapshots). `git diff -M --stat` for the commit shows each snapshot as a
   rename. `git diff -M` shows exactly one changed line per snapshot (the `source:` header). No `.snap.new` or
   `.pending-snap` file exists after the gates. On the operator's machine, the 35 private snapshots are untouched and
   their tests pass.
4. **The locking child really runs** (T7). A mutation check: temporarily change the child's expectation `"refused"`
   to `"granted"` in `spawn_probe`'s call. The parent test must fail. Restore with `cmp` against a saved copy. With
   the filter left at `lock_probe_child`, the same mutation would pass, which is how the silent trap is shown.
5. **Unique temp prefixes** (T14). A test in cli's `it` reads every `tests/it/*.rs` that calls `temp_dir()`,
   collects each string literal matching `"willikins-[a-z0-9-]+-\{` (the prefix before the first `{`), and asserts
   that each prefix occurs in exactly one file. It fails today on `willikins-cli-test-`.
6. **Seeds still read** (T5). `tests/proptest-regressions/ensure_properties.txt` exists, its `cc` line is
   byte-identical to the old file's, and `tests/ensure_properties.proptest-regressions` does not exist (guard rule 5).
7. **Gated targets still build** (T8–T12, T14). `cargo clippy -p <crate> --all-targets --features live-tests` is
   green. `cargo test -p <crate> --features live-tests --no-run` builds the gated binaries next to `it`. The
   coordinator runs that second check once, in T16, for all six crates.
8. **Fewer binaries** (T16). The `.d`-mapping script reports one `it` row per crate, the 10 gated targets only when
   built with `live-tests`, and no other integration-test row except the private ones.

## Verify before relying on them

1. **The binary-count and size estimates** in "Expected effect" are derived from today's `target/`. T16 replaces
   them with measurements.
2. **rustc's peak memory for the largest merged crate** (core, about 13.6k lines; server, about 12.3k plus `rmcp`
   and `axum`). It should be well within the host's 11 GB at `-j 2`, but this is not measured. T2 and T13 each record
   `/usr/bin/time -l` maximum resident size for their test build. If one exceeds about 3 GB, split that crate into two
   binaries (`tests/it/` and `tests/it_<area>/`) instead of loosening `-j`.
3. **That trybuild's two suites in `types` do not interfere** when they share a process. The lock is documented in
   its source, but the arrangement has not been run here. T4's gates show it.
4. **That insta writes no new snapshot in CI-like runs.** The renames carry the contents. If a `.snap.new` appears,
   the rename was wrong.
5. **That no other tool or script names a moved test target.** A `git grep` for `--test <stem>` outside `docs/plans`
   and `docs/research` found two hits: `README.md` (`live_smoke`, which stays) and
   `crates/willikins-providers-buildkite/fixtures/buildkite/README.md` (`live_probe`, updated in T10). Re-run the
   grep in T15.

## Risks

- **A silent loss rather than a red test.** Three traps fail open: an undeclared module, a seed file in the old
  place, and an `--exact` filter without its module. Guard rules 2 and 5 and acceptance 4 close them. Everything
  else in the move fails loudly (a compile error, a self-flagging guard, a missing snapshot).
- **Concurrency between former files** (d9). A flake that appears only after a move is a shared-resource bug the
  separate binaries used to hide. Fix the resource (a unique name or a `tempfile` directory), never `--test-threads=1`.
- **Timing-sensitive tests under more load** (`blocking_pool_13`, journal locks). If one flakes, record it, and
  widen its own bound with a stated reason rather than serialising the binary.
- **Per-task rebuilds of core's test binary** from T4 on, since every `PENDING` edit touches it. It rebuilds only
  core's `it` test crate, not the library.
- **Merge friction with lanes running in parallel.** A test file added under `tests/` while this milestone is under
  way still compiles (auto-discovery) but goes red under the guard once its crate is out of `PENDING`. The fix is to
  move it. The coordinator should schedule each crate's task when no other lane is editing that crate's `tests/`.
- **Privacy.** The cli task runs on a tree holding private targets. `git ls-files`-driven moves and
  `git commit --only` keep them out. The test-name lists stay outside the tree.
- **Host contention.** Other sessions run cargo almost continuously. If no 3-second quiet window comes within 40
  minutes, the task stops and reports the contention.

## Needs the operator

Nothing blocks this milestone. Two choices are the operator's, neither needed to finish:

1. The untracked `todos/2026-09-23-cargo-target-60gb-integration-test-binaries.md` belongs to another session.
   This plan does not touch it. After T16's numbers, the operator or that session decides whether it
   is done.
2. Whether to fold the three private `operator_*` targets into one private binary. It would save two more links on
   the operator's machine. It is outside the tracked tree, so it is never an agent's change to make unasked.
