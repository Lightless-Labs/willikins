---
title: "Adversarial pass 1's items for the server: approval binding, decision finality, document hash type"
created: 2026-09-14
status: open
priority: high
area: server, journal
related:
  - docs/research/2026-09-14-executor-journal-adversarial-pass-1.md
  - docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md
  - todos/2026-09-14-plan-identity-must-cover-inputs.md
  - crates/willikins-journal/src/journal.rs
---

# Pass 1's items for task 10a

Adversarial pass 1 (task 9) found one defect, fixed it (`de9f41c`), and left four things that
only `willikins-server` can close. Each is pinned by a `boundary_` test that will need
updating when it is closed, named below. The full reasoning is in the research note.

1. **`Approval::Human` is not bound to a plan, a window, or a clock.** Core checks only that a
   human is claimed: it never reads the `at` field and has no plan id to compare against. The
   server must construct `Human` only from a journaled `ApprovalGranted` for *that* `plan_id`
   and check both windows itself. Pins:
   `boundary_an_approval_is_not_bound_to_the_plan_it_approves`,
   `boundary_an_approval_timestamped_in_the_future_still_runs`
   (`crates/willikins-core/tests/apply_adversarial.rs`).

2. **An approval decision is not final in the fold.** `ApprovalGranted` appended after
   `ApprovalRejected` wins, so a rejected plan can be revived by a later append. An
   append-only log's fold has to fold what it is given, so the refusal belongs to whoever
   accepts the second decision: `Butler` must refuse to record an approval or a rejection for
   a plan that already has one, exactly as it refuses a second `apply`. Pin:
   `boundary_a_grant_after_a_rejection_is_the_decision_the_views_report`
   (`crates/willikins-journal/tests/adversarial_pass_1.rs`). The alternative — making the fold
   first-decision-wins — is a semantic change pass 1 did not take unilaterally.

3. **`Event::PlanRecorded::document_sha256` is a bare `String`** with no grammar and no length
   bound, unlike every other agent-facing text field in the workspace, and it is half of the
   plan identity 10a is built on. Give it a validated hex-digest type (or validate at the
   boundary) when the server starts producing it.

4. **No single-apply lock exists anywhere below the server.** Two `apply` calls against one
   provider state interleave freely; core's only protections are the pre-write refusals and
   each tool's own per-resource atomicity. `RunInProgress`/`AlreadyApplied` in `Butler` are
   what make a run exclusive. Pin:
   `boundary_two_threads_applying_one_plan_share_state_with_no_mutual_exclusion`.

Plan identity itself (the `(workflow name, document sha256)` check, and the
`workflows/fixtures/plan-identity-{a,b}.yaml` pair that is its acceptance test) stays in
`todos/2026-09-14-plan-identity-must-cover-inputs.md`, which this does not duplicate.
