---
title: "The RTK hook can summarize a failing cargo build into 'No issues found'"
category: tooling
tags: [cargo, clippy, rtk, gates, zsh, exit-codes]
module: general
symptom: "A gate run prints 'cargo clippy: No issues found' and exit 0 while the build actually failed"
root_cause: "The RTK Claude Code hook rewrites bare cargo commands and summarizes their output; piping through tail/tee in zsh also hides the real exit status"
date: 2026-09-12
---

# The RTK hook can summarize a failing cargo build away

## Symptom

During task 5b an agent's `cargo clippy --workspace --all-targets -- -D warnings` printed
only `cargo clippy: No issues found` and `EXIT:0`, while the crate did not compile (E0599 and
E0061 in a test module). Two gate runs were reported green on a broken tree.

## Root cause

Two things stacked. The RTK hook transparently rewrites bare `cargo ...` into `rtk cargo ...`
and can summarize compiler output. Separately, the agent read `${PIPESTATUS[0]}` in a zsh
shell, where that variable does not exist, after piping through `tee` and `tail`.

## Fix

- Run gates as `rtk proxy cargo ...`, which executes the raw command without filtering.
  **Superseded 2026-09-12:** the RTK hook was removed from this machine (`rtk` is no longer
  on the path), so gates run as bare `cargo ...` again. The rest of this note still applies.
- Never pipe gate output. Redirect it to a file, echo `$?` on its own line, then read the
  file body. Grep the body for `error` and `test result` rather than trusting the exit line.
- Put this in the repo's CLAUDE.md so every agent prompt inherits it.

## References

- `CLAUDE.md` "Commands" section.
- `docs/plans/2026-09-11-milestone-1-core.md` "Gates".
