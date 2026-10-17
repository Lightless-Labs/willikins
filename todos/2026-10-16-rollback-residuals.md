---
title: Milestone 3m's open rollback residuals
created: 2026-10-16
status: pending
priority: low
area: core/rollback
related:
  - docs/plans/2026-10-05-milestone-3m-safe-rollback.md
  - docs/research/2026-10-12-m3m-adversarial-pass-independent.md
---

# Milestone 3m's open rollback residuals

The independent adversarial pass recorded these and fixed none of them. All of them fail safe: a teardown refuses
or keeps something rather than deleting the wrong thing. See the research record for each one's evidence.

1. `AlreadyRolledBack` is not re-checked at apply. A second approved teardown of a run that is already rolled back
   refuses as drift (the wrong reason, a safe outcome). This is the cheapest one: a two-line check beside the
   `LaterRunFirst` re-check.
2. Two nodes on one natural key with conflicting policies (a parent's node and a used child's): whichever creates
   first decides, and the forward plan's `not_reversible` then misleads about the other.
3. The forward `not_reversible` over-warns for a would-be transit rider (planned Create, finishes Unchanged).
4. `apply_teardown` can in principle return `Err` (`Replan`) after an earlier delete ran, losing that step's
   `TeardownStepFinished`. Unreachable today; it contradicts `TeardownApplyError`'s own doc.
5. `run_created_something` reads `Create` only, where decision 8 says Create/Replace/Update.
6. The fake Doppler config inverse cannot see an environment's branch configs, and its delete does not cascade
   them. Live Doppler does cascade (L2, verify 3).
7. A self-naming `Within` cycle plus a third, already-deleted container can force a rider standalone. Theoretical:
   no live tool names itself, and P7 pins that.
