---
title: "A journal-repair subcommand for a truncated last line"
created: 2026-09-15
status: open
priority: low
area: journal
related:
  - crates/willikins-journal/src/file.rs
  - README.md
---

# A journal-repair subcommand for a truncated last line

Found while writing the README's "Recovery after a crash" section (task 12, step F).

`FileJournal::open` refuses to open a journal whose last line is truncated
(`JournalError::Corrupt`, `crates/willikins-journal/src/file.rs`), naming the line number
and the reason. This is fail-closed by design: willikins never guesses at a partial record.

There is no `willikins` subcommand that repairs this. The documented recovery procedure
(README, "Recovery after a crash") is a manual detour: attach the same volume to a
temporary debug service with a shell, remove the truncated last line by hand, then move
the volume back. This works, but it needs a second Railway service and a human who knows
the procedure.

A `willikins journal repair <path>` subcommand (or a `--repair` flag on `serve`) that:

- opens the file directly, without going through `FileJournal::open`'s all-or-nothing
  validation,
- finds the exact byte offset `JournalError::Corrupt`'s `line` field already names,
- truncates the file to the end of the last complete line, and
- prints what it removed, so the operator can decide whether that is acceptable

would close this gap without adding a debug service to the runbook. Low priority: no
production journal has actually needed this yet.
