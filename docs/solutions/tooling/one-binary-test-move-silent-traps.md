---
title: "Three ways folding tests/*.rs into tests/it/main.rs can silently lose coverage"
category: tooling
tags: [cargo, integration-tests, libtest, proptest, insta, test-layout, gates]
module: general
symptom: "A test still passes green after the move, but it either runs nothing, or runs against a seed file nobody reads any more"
root_cause: "Three cargo/libtest/proptest behaviours fail open rather than loud when a single-binary-per-file layout becomes one binary per crate"
date: 2026-10-05
---

# Three silent traps in moving to one test binary per crate

## Context

Milestone 3k (`docs/plans/2026-10-05-milestone-3k-faster-gates.md`) moved each crate's default-built
`crates/<c>/tests/*.rs` files into one binary, `tests/it/main.rs`, one `mod <stem>;` per former file. The move
itself is loud when it goes wrong — a compile error, a self-flagging guard, a missing insta snapshot. Three
things are not: each one still exits `0` with no failing assertion, so a gate that only reads the final exit code
never sees the loss.

## Trap 1: an `--exact` filter without its module prefix

**Symptom.** A test spawns a child process to run one specific test in isolation, e.g.
`cargo test --ignored --exact lock_probe_child`. Before the move, `lock_probe_child` was the full, unique test
path inside its own binary. After the move, every test's path is `<module>::<name>` (libtest's `--exact` matches
the *full* path). The old, unqualified name now matches nothing: the child process runs zero tests, exits `0`,
and the parent that waits on its exit code reads it as success.

**Mechanism** (plan's decision (d2)): `--exact` is a full-path match, not a substring. Once a binary holds
`mod locking;`, every test in that file is addressed as `locking::lock_probe_child`.

**What closes it.** Update every such filter string to the module-qualified form
(`"locking::lock_probe_child"`, in willikins-journal's `locking.rs`). The plan's acceptance 4 pins this with a
mutation check: temporarily flip the child's own expectation, confirm the *parent* test now fails, then restore
and confirm that with the filter left unqualified the same mutation would have passed — proving the silent trap
really is silent, not merely theoretical.

## Trap 2: a proptest regression seed left where proptest no longer looks

**Symptom.** A `proptest!` macro with default failure persistence (`SourceParallel`, the default) writes and
reads a regression seed file next to the test's source. After the file moves into `tests/it/`, the test still
runs, still passes, and silently stops reading the seed that pinned a previously-found failing case — a
regression could reappear and nothing would notice until the failing input is independently rediscovered.

**Mechanism** (plan's "Sources, verbatim", proptest's `failure_persistence/file.rs`): `SourceParallel` walks
*up* from the test's own source file looking for a directory holding `lib.rs` or `main.rs`; if it finds one, it
writes the seed at `<that directory's parent>/proptest-regressions/<relative path>.txt`, and otherwise falls
back to `WithSource`, which writes `<source file>.proptest-regressions` right beside the test. Before the move,
the walk up from `tests/<stem>.rs` found neither file and used that fallback — which is exactly why the old seed
sat at `tests/ensure_properties.proptest-regressions`, next to the source, not under a `proptest-regressions/`
directory. Once `tests/it/main.rs` exists, the walk up from `tests/it/<stem>.rs` finds it on the very first step,
so `SourceParallel` now applies and the seed path becomes `tests/proptest-regressions/<stem>.txt` — a different
path from the one the old file sits at, which proptest therefore never reads again.

**What closes it.** `git mv` the seed file to where proptest will now look,
`tests/proptest-regressions/<stem>.txt` (done once, in willikins-providers-fake's `ensure_properties.rs`; see
decision (d8)). The layout guard's rule 5 makes the old path a standing check: no `tests/*.proptest-regressions`
file may exist once a crate is out of `PENDING`, so a reintroduced one (e.g. by a careless `git mv` in a future
move) fails the guard instead of silently losing history. Acceptance 6 pins that the moved file's content —
specifically its `cc` line, the seed itself — is byte-identical to the pre-move file.

## Trap 3: an undeclared module

**Symptom.** A file sits at `tests/it/<stem>.rs` but `tests/it/main.rs` has no `mod <stem>;` line for it (a typo,
a merge, or a task simply forgetting one file in a batch move). Rust's module system treats an undeclared file
as nothing: it is never parsed, never compiled, and every `#[test]` function inside it never exists as far as
`cargo test` is concerned. The crate's test run still exits `0` — fewer tests ran, and nothing says so.

**Mechanism:** a `.rs` file with no `mod` declaration anywhere that names it is just an unreferenced file on
disk; cargo's auto-discovery only applies to *top-level* `tests/*.rs` files becoming their own binaries, never to
files inside a directory that is itself one target (`tests/it/`, per the Cargo book's project-layout page quoted
in the plan).

**What closes it.** The layout guard's rule 2 (`crates/willikins-core/tests/it/test_layout_guard.rs`): for every
crate out of `PENDING`, every `tests/it/*.rs` other than `main.rs` must be named by a `mod <stem>;` line in
`main.rs`, or the guard fails naming the orphan file. Combined with acceptance 1 (the per-crate list of test
names, generated by actually running `cargo test -- --list`, diffed against the pre-move list), an undeclared
module shows up twice: once as a guard failure, once as missing names in the list diff.

## The common shape

All three are the same failure shape: a check that used to be *implicit* in "this is its own binary, so its exit
code means something" becomes an explicit thing someone has to get right once binaries are shared. None of the
three is specific to this migration — any crate that later adds a new `tests/it/<stem>.rs`, a new `proptest!`
macro, or a new cross-process `--exact` filter can reintroduce one of them. The layout guard (traps 2 and 3) and
acceptance 4's mutation-check pattern (trap 1) are the load-bearing defenses; a green gate alone is not evidence
against any of the three.

## References

- `docs/plans/2026-10-05-milestone-3k-faster-gates.md`: "Risks" ("A silent loss rather than a red test"),
  decisions (d7) and (d8), acceptance tests 1, 4 and 6.
- `crates/willikins-core/tests/it/test_layout_guard.rs`.
- `crates/willikins-journal/tests/it/locking.rs` (`lock_probe_child`).
- `crates/willikins-providers-fake/tests/it/ensure_properties.rs` and
  `crates/willikins-providers-fake/tests/proptest-regressions/ensure_properties.txt`.
