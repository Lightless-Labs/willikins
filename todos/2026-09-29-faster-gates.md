---
title: Make the full gate faster by consolidating integration-test binaries
created: 2026-09-29
status: done
priority: high
area: build
related:
  - todos/2026-09-23-cargo-target-60gb-integration-test-binaries.md
  - CLAUDE.md
  - docs/plans/2026-10-05-milestone-3k-faster-gates.md
---

# Faster gates

Every full four-gate run on this host takes one to two hours, which sets the pace of every
milestone: each lane ends with at least one, and a single snapshot change costs another. Another
session's todo (`todos/2026-09-23-cargo-target-60gb-integration-test-binaries.md`, left untracked by
that session and deliberately not adopted here) has the analysis: about 143 files under
`crates/*/tests/*.rs` each compile to their own statically linked binary, `target/` reached 60 GB,
and moving them into one `tests/it/` binary per crate would cut that to about 13. That also shrinks
the disk footprint the host's nightly `cargo-sweep` fights, and the sweep has broken three gates
mid-build.

Coordinate with that session before starting: the file is theirs. Done when every crate's
integration tests build as one binary and a full gate is measurably faster.

**Done 2026-10-05:** milestone 3k (`docs/plans/2026-10-05-milestone-3k-faster-gates.md`) built it. Every crate's
default-built integration tests now compile as one binary, `tests/it/main.rs`: 13 in the public tree, 16 on the
operator's machine, the 10 `live-tests`-gated targets and the three private `operator_*` targets unchanged and
unmoved. The "measurably faster" half of "done when" is the plan's T16 (after-measurement and close, the
coordinator's); see the plan's Completed header and its T0/T16 addenda for the numbers once that lands.
