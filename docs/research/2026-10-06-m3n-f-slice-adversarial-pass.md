---
title: "Milestone 3n adversarial pass, first slice: F1, F2 and F3"
created: 2026-10-06
status: complete
area: willikins-core (apply, plan, value), willikins-types (registry)
related:
  - docs/plans/2026-10-05-milestone-3n-checked-as-a-typed-graph.md
  - docs/research/2026-09-23-m3d-adversarial-pass.md
  - docs/research/2026-09-14-executor-journal-adversarial-pass-1.md
---

# Milestone 3n adversarial pass, first slice (F1–F3)

This pass attacked only milestone 3n's first slice: F1 (`c05c713`, `79f3e77`), F2 (`56b43d1`) and F3 (`0d6d8f6`,
`552b92c`, and the docs commits `4bc1be5` and `91eb78a`). The rest of 3n, S1 onward, has not been written, so it was
not attacked. The plan's X1 row covers the whole milestone and will have its own record. This file is named apart so
that the two records do not collide. The reviewer wrote none of the code under attack. No provider was called, and
nothing ran against a live account.

At the start, `main` was at `91eb78a`. Nothing was uncommitted and nothing was red. The untracked `goal.txt` and the
cargo-target todo were already present, and neither was touched. Baselines were re-run independently before any
mutation:

- `-p willikins-core`: lib 194 tests, `it` 286, doctests 1, all passing.
- `-p willikins-dsl --test it acceptance::`: 13 passing, including `characterization_of_every_document`.
- No `.snap.new` anywhere in the tree.

The priorities, from the task, were:

1. The two apply panics are really gone.
2. A tool output is checked where it is produced.
3. No `Value` can be constructed that names itself.
4. Nothing else in plan or apply changes behaviour.

## Mutations

Each mutation was applied to a file saved beforehand under the scratchpad. The file was then restored with `cp`
followed by `touch`, and `cmp` against the saved copy printed nothing (byte-identical). Only one mutation was in the
tree at a time. Every run used `-j 2` and `RUST_TEST_THREADS=2`.

| # | File | Mutation | Run | Result |
| --- | --- | --- | --- | --- |
| M1 | `value.rs` | delete `Value::known`'s `const { assert!(is_type_name(..)) }` | `--doc` | **killed**: the `compile_fail` doctest reports "Test compiled successfully, but it's marked `compile_fail`". The doctest is not vacuous: its snippet fails to compile only because of the assertion. |
| M2 | `value.rs` | delete `Value::known_list`'s identical assertion | full core (lib, `it`, doc) | **survived**: 194 + 286 + 1 passing. Fixed in `5ed0c45` (below). After the fix, the same mutation is killed by the new doctest at `known_list`. |
| M3 | `apply.rs` | `let item: Option<&Value> = None;` instead of the walk's item (F1, point 2) | core lib + `it` | **killed**: `item_in_a_list_port_applies_with_each_instances_own_item` panics at `plan.rs` with exactly the plan's predicted message, "`check` rejects `item` used outside a for_each node". |
| M4 | `apply.rs` | at the top of the `checked.order` loop, `continue` when `fresh.plan.nodes[index]` does not name this node (the pre-F1 walk) | core lib + `it` | **killed**: `an_empty_for_each_source_applies_zero_instances_and_a_known_empty_list` panics with exactly the predicted "`checked.order` plans every node before its dependents". |
| M5 | `plan.rs` | delete `check_output_type`'s `value.ty() != expected` refusal | core lib + `it` | **survived** the 286 existing tests. Fixed in `e02d28d` (below). After the fix, the three new tests fail under it and nothing else does. |
| M6 | `plan.rs` | disable `check_output_type`'s `TypeId` loop (`if false && …`) | core lib + `it` | **killed** by 2: `a_list_outputs_misnamed_element_is_refused_with_the_declared_name_fallback` and `apply_refuses_a_same_named_impostor_from_ensure_where_it_is_produced`. |
| M7 | `apply.rs` | after `ensure`, `Ok((outputs, changed))` instead of `fill_outputs(..)` | core lib + `it` | **killed** by 4: `ensures_wrong_typed_output_fails_the_instance_at_apply`, `apply_refuses_a_same_named_impostor_from_ensure_where_it_is_produced`, `a_secret_returned_on_a_non_secret_port_is_refused_before_it_reaches_a_sink`, and `undeclared_outputs_are_dropped_and_forgotten_ones_become_unknown`. |
| M8 | `plan.rs` | `key_for_each_items` tests against `willikins_types::registry()` instead of the catalog's registry | core lib + `it` | **killed**: `apply::conversions::keyed_and_item_edges_deliver_known_b` fails with `EdgeTypeMismatch { site: ForEach { node: "each" }, expected: ConvA, found: ConvA }`. This reproduces the evidence behind the 2026-10-06 addendum's registry parameter. The parameter is load-bearing, not cosmetic. |
| M9 | `registry.rs` (types) | `is_type_name` also accepts `_` | `-p willikins-types --lib` | **killed**: `is_type_name_agrees_with_the_regex_on_a_table_of_names` fails on `"Git_HubRepo"`. |

Seven of nine were killed outright. The two survivors were real test gaps, and both are now closed.

## Findings and fixes

### 1. `known_list`'s compile-time refusal was unpinned (fixed, `5ed0c45`)

Decision (f3) puts the `const` assertion on both `Value::known` and `Value::known_list`. Acceptance 4 asked for a
compile-fail proof, and the proof existed only on `known`. Deleting `known_list`'s assertion left the whole crate
green (M2). A second `compile_fail` doctest now sits on `known_list`, using the same misnamed-derive snippet but calling
`known_list(vec![value])`. Under M2 it reports "Test compiled successfully". Restored, it passes. This change is to a
doc comment only: no production code changed.

### 2. `fill_outputs`' `TypeRef` equality was unpinned (fixed, `e02d28d`)

`check_output_type` refuses an output in two steps. First, the value's declared `TypeRef` must equal the port's. Then
every known object must pass `registry.type_matches`. All four of F2's own tests returned a wrong-typed *known*
value, and the `TypeId` loop alone refuses that with a byte-identical message, since `found` comes from the object's
own reported name. Deleting the equality left every test green (M5). Under that deletion, three wrong shapes flow on:

- A known scalar `ConvA` on a port declared `list<ConvA>`: `as_scalar` yields a matching object and `as_list` is
  empty, so the loop passes.
- A known `list<ConvA>` on a port declared `ConvA`: the reverse.
- A non-pure tool's `read` predicting `out: ConvA` as `Unknown(ConvB)`: there is no object to test.

Three plan-time tests in `tests/it/apply.rs`'s `conversions` module now pin these. They use one double, `Emits`, which
reports exactly the outputs it is given: `Present` if pure, otherwise as the `Absent` prediction. Each test asserts
`PlanError::Tool`, `Invalid`, and the SHARED VALUES message. This change is to tests only: no production code changed.

## Attacks that found nothing

- **Panics reachable from a document, at apply.** `apply` now walks `checked.order`. A node with no planned entry gets
  `whole_skip = false` and an empty group. For a `for_each` node, that empty group becomes `ForEach(vec![])`. A
  non-`for_each` node always plans exactly one entry: either `Skip`, or `plan_one`, or an error ends the walk. Its
  `unreachable!` therefore cannot be reached. The walk's `items` vector is pushed at every `planned.push` site (three
  pairs in `walk`), so `fresh.items[group_end]` is always in bounds. `classify_unknown_binding` skips `Literal` and
  `Item` elements of a list. Its remaining `unreachable!`s need a list made only of literals and items to be
  `Unknown`, and such a list is always known. A `Keyed` reference into a zero-instance node is
  `PlanError::KeyNotInForEach`, raised by rule 2's re-plan before any node runs.
- **A tool output reaching a downstream node unchecked.** Tool outputs are produced at exactly three places. The first
  is `plan_one`'s `Absent`/`Present` arms, which are checked. `Mismatch` and `Foreign` return before any output is
  produced. The second is `apply`'s post-`ensure` chain, which is checked. The third is the pure and `NoOp` paths,
  which reuse the checked planned outputs. The whole-node `Skip` path builds `Unknown`s from the spec itself
  (`unknown_outputs`), so there is nothing for it to check. `plan`'s aggregation (`known_dyn_list(element, …)`) is fed
  only checked instance outputs, and the list-flag check above guarantees each one is a scalar.
- **A self-named `Value`.** Every public constructor was checked. `known` and `known_list` are the const-asserted
  `T::TYPE_NAME`. `known_dyn_as` names by the caller's `ty` and checks the object by `TypeId`. `known_dyn_list` names
  by the caller's element. `parse` and `parse_list` name by the caller's `TypeRef`. `converted` names by
  `conversion.to()`. `unknown` names by the caller's type. `known_dyn` and `type_name_of_object` are gone, and nothing
  in the tree mentions `known_dyn(`. `obj.type_name()` survives only in `reported_type_name_or`, which names an object
  already refused, and in rendering (`static_type_name`).
- **Fake versus live tools.** Every `outputs.insert(` in the six live provider crates was read against the same
  tool's declared outputs: `appstore`, `buildkite`, `doppler`, `github` and `signoz`, plus `http`, which has none. Every
  known value is built from a Rust value of the declared type. Every `Unknown` is built from the declared name. No
  mismatch was found. What this cannot verify is a branch that runs only against a live account. F2 changes how such a
  mismatch fails: it now refuses *after* `ensure` has written, as `ApplyError::Tool` with the instance `Failed`,
  where before it flowed on. Decision (f2) accepts that position. The SHARED VALUES message does not itself say that
  `ensure` ran. The `ApplyError::Tool` variant's doc does.
- **Behaviour drift.** `characterization_of_every_document` passes, and no snapshot moved: the slice's diff
  (`6e7c3c4..91eb78a`) touches no `.snap`, and no `.snap.new` exists. A `for_each` key is still `value.render()` of
  the same object, so plan JSON and fingerprints cannot move. The private, fake-backed `operator_*` targets were run
  read-only and pass: the operator's document target (34 passed, 1 ignored) and its blocked-apply redaction target (1
  passed). The live-catalog target was not run.
- **Privacy.** The slice's diff, its commit messages, and this pass's commits name no operator org, domain, app,
  bundle identifier, layout, host or home path.

## Residual, for X1 and G1

- The F1 fixes are positional. `Walk.items` is aligned with `Plan::nodes` by construction only, and `apply` finds a
  group by matching names at `index`. G1's `groups` (decision (g6)) replaces both with structure. Until then, a new
  `planned.push` without its paired `plan_items.push` would index out of bounds rather than fail to compile.
- No plan-time test covers a scalar same-named impostor. `apply_refuses_a_same_named_impostor_from_ensure_where_it_is_produced`
  covers it at apply, and the misnamed-list test covers the impostor wording at plan. M6 shows that both depend on
  the `TypeId` loop.
- The coordinator still runs the full four-gate run. This pass ran scoped gates only: `-p willikins-core` (lib,
  `it`, doc), `-p willikins-types --lib`, `-p willikins-dsl` acceptance, and the two private targets.
