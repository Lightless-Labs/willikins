---
title: "Plan identity must cover node inputs and the workflow name, not only the fingerprint"
created: 2026-09-14
status: open
priority: medium
area: core
related:
  - docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md
  - crates/willikins-core/src/apply.rs
  - crates/willikins-core/src/plan.rs
  - crates/willikins-server/src/butler.rs
  - crates/willikins-server/src/document.rs
  - crates/willikins-server/tests/acceptance_8_plan_identity.rs
---

# Plan identity must cover node inputs and the workflow name

**Addendum 2026-09-14 (task 10a):** the server-side check landed. `Butler::apply` reloads
the named document from the trusted directory (`crates/willikins-server/src/document.rs`'s
`load_named_document`, shared with `plan`), compares its sha256 against
`PlanRecord::document_sha256` (now a validated `willikins_journal::DocumentSha256`, closing
adversarial pass 1's item 3 as well), and separately compares the reloaded document's own
internal `name:` against the recorded workflow -- both refuse with `DocumentChanged`, and
`load_named_document` also refuses the workflow-name mismatch at `plan` time itself
(`ButlerError::UnknownWorkflow`), so a `PlanRecord::workflow` and a document's own internal
name can never disagree for an untampered file. Pinned by
`crates/willikins-server/tests/acceptance_8_plan_identity.rs`'s
`a_document_swapped_underneath_an_approved_plan_is_refused_as_document_changed`, built on the
`workflows/fixtures/plan-identity-{a,b}.yaml` pair this todo's item 1 named. Only the
milestone 3 question below (item 2's own point, restated) remains open; `area` narrowed from
`core, server` to `core` since the server half is done.

Found by the task 4 verifier. `Plan::fingerprint()` covers node names, instance keys,
actions and rendered non-secret outputs, but not node inputs. Two workflows with the same
node names, actions and outputs but a different `value` binding into
`github.actions_secret.ensure` (whose output list is empty) fingerprint identically, so a
plan approved for one would pass the executor's drift check for the other and write a
different secret into the same sink. Core's `apply` also never compares `approved.workflow`
with `checked.workflow.name`.

This is closed by the server's plan identity (task 10a, see the addendum above):
`apply(plan_id)` reloads the document by name from the trusted directory, compares its
sha256 with the one recorded at `plan`, and refuses with `DocumentChanged` otherwise.
Adversarial pass 1 (task 9) attacked exactly this (approve plan A, swap the document for B
with the same fingerprint, apply A) and the pair it left,
`workflows/fixtures/plan-identity-{a,b}.yaml`, is task 10a's own acceptance-test-8 fixture.

**Open (milestone 3):** whether core should also fold rendered non-secret inputs (and the
redaction marker for secret ones) into `Plan::fingerprint` itself, which would make the core
check self-sufficient at the cost of journaling every input twice. Core's `apply` still never
compares `approved.workflow` with `checked.workflow.name` on its own -- that remains true by
design; the server closes it from outside core, as the addendum describes.
