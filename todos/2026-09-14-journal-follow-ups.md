---
title: "Journal follow-ups left by the task 5 verifier"
created: 2026-09-14
status: open
priority: low
area: journal
related:
  - crates/willikins-journal/src/file.rs
  - crates/willikins-journal/src/observer.rs
  - crates/willikins-journal/tests/replay_integrity.rs
---

# Journal follow-ups

Pinned by tests. Two closed in task 10a; the rest still open, each small:

- **Closed (2026-09-14, task 10a): replay reads the path, not the locked descriptor.**
  `FileJournal::open` now calls its private `replay(file: &mut File, path: &Path)` on the
  exact descriptor it already holds open and locked (seeking back to the start), never a
  fresh `path`-based read, so a rename over the path after the lock is taken has no effect on
  what replays. Pinned by
  `crates/willikins-journal/src/file.rs`'s
  `replay_reads_the_held_descriptor_not_the_path_so_a_rename_over_it_has_no_effect`.
- **Closed (2026-09-14, task 10a): `ApplyRefusedReason::PlanFailed` now has a `FileJournal`
  round-trip pin.**
  `crates/willikins-journal/tests/run_and_journal_refusal.rs`'s
  `an_apply_refused_plan_failed_event_round_trips_through_a_file_journal`, alongside the
  existing `MemoryJournal` one.
- **`JournalObserver::ensure_started` marks `started` before its append.** If that one
  append fails, `run_and_journal` skips its own backfill `RunStarted` and the fold drops that
  run's later events. The error is returned, but the views lose the run.
- **`debug_assert!(!started)` in the refusal path** panics in a debug build for any future
  caller of `run_and_journal` that emits an event and then returns a refusal. Fine for
  `apply`; document or remove when a second caller appears. Note for whoever picks this up:
  task 10a added a second caller, `continue_run_and_journal`, but it has no refusal path of
  its own (see its own doc comment) so the assertion is still only ever reachable through
  `run_and_journal`.
- **Views fold the whole entry list on every call** (O(entries)). Fine now; a long-lived
  server journal will want a cached fold. Still true after task 10a: `Butler` calls straight
  through to the journal's own `plan`/`run`/`runs`/`pending_approvals`, no caching added.
- **No hash chain, by decision.** A keyless chain computed and verified by the same binary is
  not tamper evidence. Revisit only with an external anchor (a signed checkpoint, or the
  journal shipped to append-only storage).
