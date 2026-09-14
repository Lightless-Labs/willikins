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

Pinned by tests, not fixed, each small:

- **Replay reads the path, not the locked descriptor.** A rename over the journal path while
  a journal is open orphans every later append. Fix: `seek(0)` on the held `File` instead of
  a fresh path-based read. Five lines, no wire change. Do it in task 10a when the server
  owns the journal path.
- **`JournalObserver::ensure_started` marks `started` before its append.** If that one
  append fails, `run_and_journal` skips its own backfill `RunStarted` and the fold drops that
  run's later events. The error is returned, but the views lose the run.
- **`debug_assert!(!started)` in the refusal path** panics in a debug build for any future
  caller of `run_and_journal` that emits an event and then returns a refusal. Fine for
  `apply`; document or remove when a second caller appears.
- **Views fold the whole entry list on every call** (O(entries)). Fine now; a long-lived
  server journal will want a cached fold.
- **No hash chain, by decision.** A keyless chain computed and verified by the same binary is
  not tamper evidence. Revisit only with an external anchor (a signed checkpoint, or the
  journal shipped to append-only storage).
- **`ApplyRefusedReason::PlanFailed` has no `FileJournal` round-trip pin** (only the memory
  journal). Five lines.
