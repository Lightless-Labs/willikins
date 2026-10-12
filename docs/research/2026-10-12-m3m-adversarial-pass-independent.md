# Milestone 3m adversarial pass (X1, independent), over the whole milestone

**Date:** 2026-10-12
**Reviewer:** Claude Opus 5.5 (independent: wrote none of the code under review)
**Scope:** every landed 3m task, R1 through L3 (`c970ef3`..`bc8b309`), against
`docs/plans/2026-10-05-milestone-3m-safe-rollback.md` (every addendum, both Reviewed lines). The earlier X1 record
over R1 alone is `docs/research/2026-10-12-m3m-adversarial-pass.md`; its two fixes (`d916efc`, `9d9fcf9`) still hold.
**Method:** read the plan in full, then `willikins-core/src/rollback.rs` (non-test code in full), `check.rs`'s
teardown policy check, the linker's carry-through, `willikins-server`'s `plan_rollback`/`apply_rollback` and
`rollback.rs`, every inverse's `delete` in the provider and fake crates, the three live cycles' gating, and the
milestone's commits for privacy. Attacked the priority list; proved conclusions by mutation; fixed two real defects
test-first. No live call, no Railway command, no App Store call of any kind.

## Verdict

The milestone holds on its top priorities: policy is the document's, a node that declares nothing is never deleted,
exactly one `TeardownToken::new` exists, every inverse re-observes and re-checks accepted losses in `delete`, no
credential is read from the journal, and no new App Store call exists outside mocks. Two real defects were found and
fixed:

1. **A rider matching two containers was decided by the first one only** (core). A kept node inside a deleted
   container could escape decision 3a's refusal, and a `teardown: delete` one made a real environment refuse the
   whole teardown. Fixed in `7429187`.
2. **LIFO was checked only when a teardown was planned** (server). A later run of the same workflow made between plan
   and apply did not stop the apply: the earlier run's resources were deleted while the later run stood. Fixed in
   `b81a8d0`.

Two coverage gaps were closed with tests (`535c8a9`, `59ae1ea`).

## Findings

### F1 (fixed, `7429187`): first-match riding hid a deleted container behind a kept one

`find_container` returned the first instance the rider's `Within` list matched, in declaration order.
`doppler.branch_config.ensure` declares two containers, the project first and the environment
(`doppler.config.ensure`) second, and one branch config sits inside both. With the project kept and the environment
deleted:

- **Branch under `teardown: delete`:** the branch matched the kept project, so it became its own standalone
  `Delete`, and the environment's `Created.riders` never named it. The live `doppler.config.ensure` inverse refuses
  "the environment has a config this run did not create", so the whole teardown refused. Decision 3a says the branch
  rides with the environment. This is over-refusal, the safe direction, but a real defect.
- **Branch kept (the default):** the branch was listed `Kept` and the environment deleted, and the core rule ("a node
  the document keeps is inside a container it deletes") never fired. Only the provider's drift rule stood between the
  kept branch and the environment delete that takes it along. The fake config inverse has no notion of an
  environment's branch configs, so on the fake catalog the teardown planned and applied.
- **Forward plan:** `not_reversible` listed the kept branch as plain `KEPT`, so it did not preview the refusal.

Proof (red before the fix, recorded in the run log): `a_rider_matching_a_kept_and_a_deleted_container_rides_with_the_deleted_one`
returned `Refused { config: "an untolerated secret name" }`. `a_kept_rider_matching_a_kept_and_a_deleted_container_refuses`
returned `Ok` with the branch `Kept` and the config `Delete`. The forward test listed the branch with `KEPT`.

Fix: `containers_of` keeps every match. `try_resolve` waits for all of them. A container that goes away decides
(`RidesWith` it, or refuse a kept candidate), then a kept one, otherwise the instance stands alone. `riders_of` lists
a rider under every container it sits in, so whichever one is observed tolerates it. `not_reversible` combines
matches the same way (`combined_forward_settled`), so the forward plan and the teardown still cannot disagree. The
dsl characterizations do not move: no tracked document has a multi-container rider whose containers' policies
differ.

The same commit makes `transitive_riders` mark `start` visited. In a mutual ride (a `Within` naming its own tool),
the grounded instance had been handed back to its own inverse as its own rider. `535c8a9` pins this.

### F2 (fixed, `b81a8d0`): a later run between teardown plan and apply bypassed LIFO

`apply_rollback_inner` re-checked the document, the approval and its window, and the fingerprint, but not
`later_run_first`. Sequence: run A; plan a teardown of A; run B of the same workflow (B creates its own project);
approve; apply. The apply succeeded and deleted A's project with B standing. The two runs touch nothing in common,
so no container drift rule can see B. Only decision 8's per-workflow LIFO rule covers it.

Proof: `a_later_run_between_teardown_plan_and_apply_refuses_later_run_first` failed before the fix with
`TeardownRunResult { steps: [(proj, Deleted)], succeeded: true }`.

Fix: `apply_rollback` refuses `LaterRunFirst` after the approval checks and before any re-observation or
`TeardownStarted`. The refusal is journaled against the teardown's own `plan_id`. The test also asserts that nothing
was deleted and that no `TeardownStarted` was written.

### Coverage closed (`59ae1ea`): a used document's policy flipped after the run

Trust boundary 8 through `uses:` was pinned only by forward apply's composition tests (`composition_s2`). The new
`a_used_documents_policy_flipped_after_the_run_refuses_document_changed` runs a composite whose child keeps its node
by default and checks that the teardown lists that node `Kept`. It then flips the child to `teardown: delete`, with
the root's bytes unchanged, and `plan_rollback` refuses `DocumentChanged`. Mutation M7 shows the test is not vacuous.

## Mutations

Each mutation was restored from a copy saved in the scratchpad, `touch`ed, and confirmed byte-identical with `cmp`
(never with git). Every run was scoped.

| # | Mutation | Priority it proves | Result |
| --- | --- | --- | --- |
| M1 | `check.rs`: a node with no `teardown:` becomes `Delete { accept: [] }` instead of `Retain` | delete on a node that declares nothing | killed: 7 red, incl. `the_default_policy_deletes_nothing`, `kept_create_plus_failed_is_kept_with_no_standing_calls`, `riders_of_a_kept_project_are_kept_and_nothing_is_observed` |
| M2 | `try_resolve`: a kept `Delete`-capable candidate inside a deleted container rides instead of refusing | kept node inside a deleted container | killed: `a_kept_config_inside_a_deleted_project_refuses`, `a_kept_rider_matching_a_kept_and_a_deleted_container_refuses` |
| M3 | `apply_teardown`: the fingerprint comparison disabled (`if false && …`) | plan-to-apply drift, a loss appearing after approval | killed: `a_loss_appearing_before_the_replan_refuses_as_drift_though_the_action_is_unchanged`, `a_resource_recreated_after_a_gone_approval_refuses_as_drift` |
| M4 | a second `TeardownToken::new()` in the fake `doppler.project.ensure` inverse's non-test `delete` | a delete from more than one token call site | killed: `teardown_token_guard` names the file and the enclosing `delete` |
| M5 | `rides_with`: the `is_known()` guards removed (`Unknown == Unknown` rides) | contents tolerated under a rider that wrote nothing | killed, by `rides_with_tests::unknown_values_never_ride` only (no plan-level test kills it) |
| M6 | `check_teardown_policies`: `teardown:` admitted on a `Contained` tool | policy reintroduced past a tool's capability | killed: `teardown_on_a_contained_tool_is_refused`, `teardown_delete_on_a_contained_tool_is_refused` |
| M7 | `reload_and_check`: the used-closure (`PlanRecorded.used`) comparison removed | a policy changed after the run without `DocumentChanged` | killed: 3 `composition_s2` forward tests; after `59ae1ea`, also the new rollback test |
| M8 | `transitive_riders`: `visited[start] = true` removed | riding computed as a self ride | killed: `a_mutual_rides_grounded_instance_is_never_its_own_rider` |
| (F1) | the pre-fix first-match riding | kept node in a deleted container; forward/teardown agreement | 3 new tests red, as above |
| (F2) | the pre-fix apply without the LIFO re-check | LIFO bypass | the new test red, as above |

**One equivalent mutant, recorded rather than tested.** `decide_standalone` refuses a loss the tool reports but never
declared, and only then refuses an unaccepted one. Removing the first branch changes nothing observable: `check`
refuses any `accept` entry the tool does not declare (`TeardownLossNotDeclared`), and `Checked` is sealed, so an
undeclared loss is never accepted. It is still refused, under the same reason string. The branch is defence in depth
and no test can tell it apart.

## Priorities, one by one

- **Policy reintroduced in a tool declaration:** none. `Reversal`'s variants carry capability only (the R1 pass).
  `check` admits `teardown:` only on `Delete` (M6) and only declared losses. Every one of the 14 inverses' `delete`
  checks `losing ⊆ created.accept` and nothing else of policy.
- **A delete on a node that declares nothing:** no. `CheckedNode.teardown` defaults to `Retain` (M1), and
  `wanted_resolvers` and `decide_standalone` observe only `teardown: delete`.
- **A delete on a rider of a kept container:** no. A `Contained` or transit rider of a kept container is
  `Kept { KEPT_RIDER }`. A `Delete`-capable rider under `delete` stands alone. After F1, this holds whichever
  declared container is listed first.
- **A kept node inside a deleted container:** it refuses in core (M2), including the multi-container case (F1).
- **A policy changed after the run without `DocumentChanged`:** no. The root sha and the used closure are both
  re-checked at plan and at apply (M7, `59ae1ea`).
- **An unaccepted loss deleted, including one that appears between approval and apply:** no. The fingerprint carries
  `losing` (M3), and each inverse's `delete` re-observes and re-checks `accept` (read in all 14, live and fake).
- **A parent widening a used document's policy through `uses:`:** the DSL refuses `teardown:` on a `uses:` step, and
  the linker copies `node.teardown` unchanged (`compose.rs`). See residual 2 for the one adjacent shape.
- **Contents tolerated under a rider that wrote nothing:** no. `riders_of` filters on `wrote`, R5b proved this by
  mutation, and the filter is kept after F1. `Unknown` never rides (M5).
- **A delete without a `TeardownToken`, from a document, or from a second call site:** no. There is one call site, in
  `apply_teardown`, minted after the fingerprint comparison, and the guard is live over the real tree (M4). No tool
  name ends `.delete`, and P7's table pins every live declaration.
- **A value or full identifier in a reason or output:** every `Standing` and refusal reason is a `&'static str`.
  `TeardownError::Refused` prints `InstanceRef`s (a node name and a `for_each` key), the same surface a forward plan
  already prints. R9 routes recorded outputs through `mask_json`.
- **A credential read from the journal:** no. `decode_value_json` turns `redacted: true` and `state: unknown` into
  `Value::unknown`, and `build_needs_inputs` takes a secret `needs` port only from `resolve_pure`, never from
  `instance.inputs`.
- **LIFO bypass:** found and fixed (F2).
- **Plan-to-apply fingerprint drift:** refused (M3). The server also attributes the refusal to an instance
  (`first_step_drift`) before `TeardownStarted`.
- **A new App Store call outside mocks:** none. P5 and P6 touched `client.rs`, `profile_ensure.rs` and mock or parity
  tests only. No `live-tests` target was added, and L1 through L3 name no App Store client.
- **The forward plan's `not_reversible` disagreeing with the teardown:** fixed for multi-container riders (F1). One
  safe-direction divergence remains (residual 3).
- **Riding computed as a mutual or a self ride:** `rides_with` refuses its own instance, the R5b tie-break grounds
  every cycle, and a grounded instance is no longer its own rider (M8).
- **Privacy:** no milestone commit message names a home path, host or account. The three live cycles are
  `#[ignore]`, behind `live-tests`, and inert without `WILLIKINS_LIVE_TESTS=1`. This pass's fixtures use
  `example-org/…`.

## Gates run (scoped)

`cargo test -p willikins-core -j 2 --no-fail-fast` (340 unit, 317 integration, 2 doctests) and `cargo clippy -p
willikins-core --all-targets -j 2 -- -D warnings` after F1; `cargo test -p willikins-server --test it -j 2 --
rollback_r8::` (12) and `cargo clippy -p willikins-server --all-targets -j 2 -- -D warnings` after F2 and `59ae1ea`;
`cargo test -p willikins-dsl -j 2` (the characterizations, which print `plan_json` and so `not_reversible`: no
snapshot moved); `cargo test -p willikins-cli --test it -j 2 -- delete_call_sites_guard:: rollback_round_trip::
acceptance::milestone_3m` (14); `cargo test -p willikins-core --test it -j 2 -- teardown_token_guard:: ratchet::
secret_literal_guard::` at the start. `cargo fmt --check` on each touched crate. The full workspace gate is the
coordinator's.

## Residual risks and notes (not fixed; for the coordinator)

1. **`AlreadyRolledBack` is checked only at plan time.** Two teardown plans of one run, both approved: when the
   second is applied after the first succeeded, its re-plan reads the deleted steps `Gone` and refuses as
   `TeardownDrift`. The outcome is safe but the reason is wrong. A teardown with no `Delete` step re-applies
   harmlessly. Re-checking `rolled_back_by` beside the new LIFO check would give the right reason.
2. **Two nodes addressing one natural key with conflicting policies** (found by reading, not run). No check refuses two nodes, for instance a
   parent's own node and a used child's node, that create the same resource. Whichever applies first is
   `Create`+`Created`, and its policy decides. The other is `Create`+`Unchanged`, a transit rider that rides with
   nothing, so it is no step. A parent can therefore delete a resource a used document keeps by declaring its own
   `teardown: delete` node on the same key, ordered first. The forward plan lists the child's node "kept by the
   document" while the parent's node is not listed, which is visible but misleading. This does not widen the child's
   policy. It sidesteps it.
3. **Forward over-warns for a would-be transit rider.** `not_reversible` treats every planned `Create` as a candidate.
   A `Delete`-capable node that will finish `Unchanged` (a new project's auto-created `prd` config, planned `Create`)
   is kept by default inside a deleted project, and the forward plan previews "the teardown will refuse". The teardown
   then rides it as transit, by design (decision 3a's transit rule). The divergence is in the safe direction.
4. **`apply_teardown` can return `Err` after a delete ran.** `delete_one` re-resolves a later step's secret `needs`
   through `build_needs_inputs(...)?`, so a `Replan` error after an earlier `Deleted` would drop that step's
   `TeardownStepFinished` (the Butler journals only `TeardownFinished { Failed }`). This is unreachable in practice,
   because the same deterministic resolution succeeded in the re-plan just before. It contradicts
   `TeardownApplyError`'s doc ("never returned once at least one delete call has actually run").
5. **The LIFO rule reads `Create` only** (`run_created_something`), where decision 8 says `Create`/`Replace`/`Update`.
   R8's addendum records this deliberately. A later run's `Update` of something inside an earlier run's container
   does not block that container's teardown. No delete hazard was constructed.
6. **The fake Doppler config inverse cannot see an environment's branch configs**, and its delete does not cascade
   them, while the live inverse refuses on them and Doppler's own delete may cascade them (verify 3, L2). After F1 the
   core rule no longer depends on that provider rule for a kept branch, but the fake still under-reports environment
   contents. Recorded for P6's parity list.
