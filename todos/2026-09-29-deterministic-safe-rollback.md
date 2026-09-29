---
title: Deterministic, safe rollback of what a run created
created: 2026-09-29
status: pending
priority: medium
area: core/apply
related:
  - deploy/teardown.sh
  - crates/willikins-journal
  - crates/willikins-core/src/class.rs
---

# Deterministic, safe rollback

The operator, 2026-09-29: "If all this works, all we need is deterministic safe rollback, and we'll have basically
reinvented CloudFormation." It splits three ways, and the plan must keep them apart rather than promise one feature:

1. **Undo what a run created, and nothing else: mostly plumbing.** The journal records each node's outcome, the
   `managed-by: willikins` markers prove ownership, and `deploy/teardown.sh` is a hand-written version of this for the
   sandbox. A reverse-topological teardown driven by the journal of one run, deleting only resources that run created
   and that still carry willikins' ownership marker, is the first slice.
2. **Anything that is not a pure create: needs per-tool inverses.** A rotated token cannot be un-rotated; a deleted
   INVALID profile can be replaced but not restored; a converged name or setting has a prior value only if something
   recorded it. Each tool declares its inverse or declares it has none (in the manner of the approval classes), and
   the engine refuses to promise a rollback a tool cannot deliver: plan-visible, before anything runs.
3. **Outside the API: a gate in reverse.** Anything done by hand (an app record, an app-group assignment) is undone by
   hand; rollback reports it as a named manual step, the way gates report the forward direction.

Unlike CloudFormation, willikins keeps no stack state and spans several vendors, so "rollback" cannot mean "restore a
snapshot": it means "converge back", from the journal and from reads of reality, and the plan must show exactly what
it will delete and what it cannot undo. Destructive by construction, so always approval-gated.
