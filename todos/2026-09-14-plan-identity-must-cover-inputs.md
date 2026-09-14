---
title: "Plan identity must cover node inputs and the workflow name, not only the fingerprint"
created: 2026-09-14
status: open
priority: high
area: core, server
related:
  - docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md
  - crates/willikins-core/src/apply.rs
  - crates/willikins-core/src/plan.rs
---

# Plan identity must cover node inputs and the workflow name

Found by the task 4 verifier. `Plan::fingerprint()` covers node names, instance keys,
actions and rendered non-secret outputs, but not node inputs. Two workflows with the same
node names, actions and outputs but a different `value` binding into
`github.actions_secret.ensure` (whose output list is empty) fingerprint identically, so a
plan approved for one would pass the executor's drift check for the other and write a
different secret into the same sink. Core's `apply` also never compares `approved.workflow`
with `checked.workflow.name`.

Today this is closed only by the server's plan identity (task 10a): `apply(plan_id)` reloads
the document by name from the trusted directory, compares its sha256 with the one recorded
at `plan`, and refuses with `DocumentChanged` otherwise. Two things follow:

1. Task 10a's `Butler::apply` must check the workflow name and the document hash before
   calling core `apply`, and acceptance test 8 must pin that a different document with an
   identical fingerprint is refused as `DocumentChanged`.
2. Adversarial pass 1 (task 9) attacks exactly this: approve plan A, swap the document for
   B with the same fingerprint, apply A.

Whether core should also fold rendered non-secret inputs (and the redaction marker for
secret ones) into the fingerprint is a design question for milestone 3: it would make the
core check self-sufficient at the cost of journaling every input twice.
