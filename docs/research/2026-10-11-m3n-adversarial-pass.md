---
title: "Milestone 3n adversarial pass (X1): Checked as a typed graph, S1 through B2"
created: 2026-10-11
status: complete
area: willikins-core (check, plan, apply, describe, value), willikins-dsl (characterization), willikins-cli (private targets)
related:
  - docs/plans/2026-10-05-milestone-3n-checked-as-a-typed-graph.md
  - docs/research/2026-10-06-m3n-f-slice-adversarial-pass.md
  - docs/research/2026-10-06-m2b-adversarial-pass-independent.md
---

# Milestone 3n adversarial pass (X1)

This is the X1 record for the whole milestone. The first slice (F1 to F3) has its own record,
`docs/research/2026-10-06-m3n-f-slice-adversarial-pass.md`. This pass attacked everything that landed after
that slice: S1 (`53b1c63`, `1c03ed1`, `675ea80`), G1 (`5d8edcb`), G2 (`2b4188a`, `8403254`), G3 (`f4c85db`),
G4 (`f35059d`), G5 (`c4c992c`), G6 (`f312a02`), G7 (`487ce8e`, `f76a605`), B1 (`cc38bf4`, `c3d92cd`), and B2
(`85176e6`), together with each task's addendum. The reviewer wrote none of that code. No provider was called,
nothing ran against a live account, and no Railway command was run.

An earlier session started X1 and left two test-only commits. This session finished it:

- `fb36435` adds a plan-time scalar impostor refusal. It closes the F-slice residual.
- `9ff46d4` checks that `Checked`'s `Debug` masks an identifier-typed bound literal.

At the start, `main` was at `9ff46d4`. Nothing was uncommitted and nothing was red. The untracked `goal.txt` and the
cargo-target todo were already present, and neither was touched. The baseline was re-run before any mutation:
`-p willikins-core --lib --test it` gave lib 245 and `it` 298, all passing. No `.snap.new` existed anywhere in the
tree.

The priorities, from the task:

1. A second lookup that survives where the graph should answer, including a ratchet that can be bypassed.
2. A hand-built `Checked` or graph that bypasses `check`.
3. Drift between plan and apply.
4. A `for_each` over identifiers that prints full ids.
5. The composite (`uses:`) path behaving differently from the flat path.
6. `apply` resolving anything that does not come from the graph.
7. The operator's private tests still passing.

## Mutations

Each mutated file was first saved under the session scratchpad. After each run it was restored with `cp` followed by
`touch`, and `cmp` against the saved copy printed nothing, so every restore was byte-identical. Only one mutation was
in the tree at a time. Every run used `-j 2` and `RUST_TEST_THREADS=2`. One mutation (M-F) wrote an insta
`.snap.new`, which was deleted. The tree is now clean.

| # | File | Mutation | Run | Result |
| --- | --- | --- | --- | --- |
| M-A | `describe.rs` | add `#[cfg(test)] mod early {}` right after the imports, then add `willikins_types::registry()` and `Some(1u8).unwrap()` inside `describe()` | `--lib --test it` filtered to `ratchet::`, `never_name`, `describe_consults` | **survived**: the ratchet (pinned at 0), `describe_consults_no_second_registry` and both tripwires all stayed green. This is a bypass. Fixed in `c05c955` (below). With that fix, the same mutation is killed by `each_ratchet_file_is_split_at_its_test_module_only` (left 2, right 1). |
| M-G | `check.rs` | `if false && is_identifier(..)` in `check_for_each` (B1 disabled) | full core | **killed** by 2: `check::acceptance_11_identifier_for_each_source_is_refused_even_with_a_colliding_default` and the new composite pin `check::acceptance_11_a_used_documents_identifier_for_each_is_refused_after_linking` (`39ffc11`). |
| M-B1 | `plan.rs` | `walk`'s single-node `plan_one` gets `willikins_types::registry()` in place of `checked.registry()` | full core | **killed** by 1 behaviour test: `apply::conversions::a_converted_secret_is_redacted_as_its_target_everywhere`. After `7973fb8` it is also killed structurally, by `plan_and_apply_consult_no_second_registry`. |
| M-B2 | `apply.rs` | `apply_instance`'s post-`ensure` `fill_outputs` gets the global registry | full core | **killed** by 2: `apply::conversions::an_unknown_step_edge_plans_unknown_b_and_ensures_known_b` and `an_unknown_list_element_plans_unknown_list_and_ensures_known_list`. |
| M-D | `check.rs` | `build_source`'s `Source::Step` arm stores `TypeRef::list_of(ty.name)` instead of `ty` | full core, then `-p willikins-dsl --test it` | **survived**: core passed 245 + 300 and dsl passed 60, with both characterizations byte-identical. This is a test gap. Fixed in `61ff3f2` (below). With that fix, the new test fails on `downstream_value` (left `list: true`, right `list: false`). |
| M-C | `value.rs` | `impl Debug for Value` prints `Disclosure::Revealed` instead of `Masked` | full core | **killed** by 2 lib tests: `check::tests::debug_of_a_checked_workflow_masks_an_identifier_typed_literal` (`9ff46d4`) and `value::tests::debug_of_a_known_identifier_shows_only_the_masked_prefix`. |
| M-E | `apply.rs` | `tool.ensure(&planned.inputs, ..)` instead of the instance's freshly resolved inputs (plan-to-apply drift) | full core | **killed** by 2: `apply::conversions::an_unknown_step_edge_plans_unknown_b_and_ensures_known_b` and `an_unknown_list_element_plans_unknown_list_and_ensures_known_list`. |
| M-F | `plan.rs` | `resolve_keyed_source` matches any instance with a non-empty key, which is in practice the first one (a keyed reference into the wrong instance) | full core | **killed** by 3: `apply::conversions::keyed_and_item_edges_deliver_known_b`, `plan::acceptance_6_insta_snapshot_of_the_empty_state_plan` (its `.snap.new` was deleted), and `plan::acceptance_7_environments_dev_qa_with_configs_prd_referenced_is_key_not_in_for_each`. |

Of the eight mutations, six were killed outright and two survived. Both survivors were real gaps, and both are now
closed by test-only commits. M-B1 was also re-run after `7973fb8` to confirm the new structural pin kills it.

The earlier session planned two more runs, M7 and M9. They are covered here: M7 is M-C, and M9 is retargeted as M-D.
M9 as first written mistyped `CheckedOutput.ty`. Since G7, that field is what `output_types()` reads, so the
characterization's OUTPUTS section kills that mutation directly. The field that actually types a skipped output's
`Unknown` is the `ty` on its `Source`, and M-D attacks that field.

## Findings and fixes

### 1. The ratchet and the tripwire could be bypassed with an early `#[cfg(test)]` (fixed, `c05c955`)

Three guards decide what counts as non-test code the same way, by cutting each file at its first line that is
exactly `#[cfg(test)]`:

- the ratchet (`tests/it/ratchet.rs`'s `non_test_body`);
- `describe.rs`'s registry count;
- `check.rs`'s comment-stripped tripwire, which looks for `Binding`, `catalog.get(`, `.workflow(` and
  `Value::parse(` (acceptance 7).

A `#[cfg(test)]` attribute on any other item above the test module exempts everything below it. That item could be
an import, a helper, or an empty `mod`. `describe.rs` is pinned at 0, so cutting it short still reads 0, and the
mutation M-A added a fallback and a second registry lookup with every guard still green.

`plan.rs` (8) and `apply.rs` (2) are not exposed to the same attack in practice. Their pins are equalities, so a cut
changes the count and fails. A cut that lands at exactly the right count would still pass, though.

The fix pins the split itself. `each_ratchet_file_is_split_at_its_test_module_only` expects exactly one
`#[cfg(test)]` line in `plan.rs` and in `describe.rs`, and none in `apply.rs`. Because this pin is on the file,
not on the splitter, it covers both split implementations at once. The commit changes a test only.

### 2. Nothing pinned the type of an `Unknown` workflow output whose producer was skipped (fixed, `61ff3f2`)

When a node is skipped, or a `for_each` aggregate includes a blocked instance, nothing runs to produce that node's
outputs. A workflow output that reads them resolves to `Value::unknown(ty)`, and `ty` is the one `check` stored on
the output's `Source::Step`, `Aggregate` or `Keyed` (decision (g1): "typing an `Unknown` for a skipped node needs no
catalog lookup").

A consumer *node* of a skipped producer is itself skipped and never resolves a port. So these `ty` fields have
exactly one runtime reader: the workflow outputs in `Plan::outputs` and `Applied::outputs`. The existing gate test
asserted only that `gate_keys` is not known. M-D flipped the list flag, and nothing failed in either crate.

`plan_gates.rs`'s `a_skipped_producers_workflow_outputs_are_unknown_of_their_checked_types` covers one output per
source kind:

- `Step` onto a skipped single node;
- `Aggregate` onto a wholly skipped `for_each` node;
- `Aggregate` onto one with a blocked instance;
- `Keyed` onto a wholly skipped one.

For each output, it asserts the `Unknown`'s exact `TypeRef` and that it equals `output_types()`'s entry. The commit
changes a test only.

### 3. A second registry in plan and apply was caught only by behaviour (pinned, `7973fb8`)

Since B2, `Checked::registry` is the only registry that `plan` and `apply` should read. M-B1 and M-B2 each swapped
one call site to the global registry. They died only because the conversion fixtures in `tests/it/apply.rs` build
their catalog over a local registry. A path that no such fixture reaches would not have been caught.
`plan_and_apply_consult_no_second_registry` now extends describe's existing pin to `plan.rs` and `apply.rs`. It is
a substring count, so an import alias would evade it. The pin covers the plain spelling, and the absent catalog
parameter remains the real guarantee.

### 4. B1 on the composite path (pinned, `39ffc11`)

`link` flattens `uses:` before `check`, so the composite path should refuse exactly what the flat path refuses. Two
cases confirm it, each refused with exactly `[IdentifierForEachSource { node: org/loop, ty: list<AppleCertificateId> }]`:

- a child document whose node iterates a `list<AppleCertificateId>` input bound by the parent's `with:`;
- the same child left unbound, so that its own default becomes a fixed input of the flat workflow.

This is a pin rather than a fix, because the composite path was already correct. M-G shows that it goes red without
B1. It was built in-test so that the composition characterization stays byte-identical.

## Attacks that found nothing

- **Hand-building a `Checked`.** `Checked` derives only `Clone`. It has no `Default`, no `Deserialize` and no public
  constructor. Its eight fields are private. `CheckedNode`, `Shape`, `Source`, `Bound`, `PortEdge`, `CheckedInput`,
  `CheckedOutput` and every id type are `pub(crate)`, so `tests/it/` cannot even name them. The trybuild pair
  (`struct_literal`, `private_field_read`) ran green in every full core run. `DocumentView` exposes no nodes, so
  neither a `Node` nor a `Binding` is reachable through a `Checked` from outside `check.rs`. That boundary holds by
  type, not by grep.
- **Apply resolving from somewhere other than the graph.** `apply` walks `checked.nodes()` zipped with
  `fresh.groups`. It resolves every instance through `crate::plan::bind_ports`, the same function `plan` calls, with
  the item `walk` stored on `EachInstance`. The only values it does not resolve fresh are on two paths. The
  `Blocked`/`Skip` paths emit `planned.inputs` and `planned.outputs`, which `walk` built from the graph. The
  pure-tool path reuses `planned.outputs`, and that path predates this milestone: `git log -S` traces it to
  `c02e0d8` (2026-09-13) and `850a389` (2026-09-28). M-E shows that ensure's inputs come from the fresh resolution
  and not from the plan.
- **A second lookup by name.** `bind_ports` finds each port's `PortEdge` by name, but only within the node's own
  `ports`, which `check` built in spec order. There is no catalog, workflow or registry lookup in `plan.rs`,
  `apply.rs` or `describe.rs`. The tripwire and the new registry pins hold, and B2 removed the catalog parameter.
- **A keyed reference into the wrong instance, and gate ordering.** M-F was killed three ways. G5's addendum
  already demonstrated that removing `awaiting_inputs`' sort makes acceptance 9 fail. The `Aggregate` arm of
  `collect_causes` keeps the old "one blocked instance taints the aggregate" rule, and
  `a_blocked_gate_skips_its_dependents_and_leaves_every_other_branch_alone` pins it.
- **A `for_each` over identifiers.** `check_for_each` refuses an identifier element type by the source's
  *resolved* type. That covers an input, a step output and an aggregate of another `for_each`, flat or linked
  (finding 4). Identifier-to-plain conversion is a compile error (`conversions!`), so no list can launder an
  identifier into `Text`. `Source::Literal` prints through `Value`'s own masked `Debug` (M-C). `Source::Keyed.key`
  is document text, and B1 means no keyed reference can target an identifier-keyed node.
- **Byte-identical runtime artifacts.** On the final tree, `-p willikins-dsl --test it acceptance:: composition::`
  passed 39 of 39. Both characterizations passed, and no `.snap.new` exists. Full `-p willikins-core --lib --test it`
  passed 245 + 302 after this pass's commits, up from 298 because of the four tests added here.
- **The operator's private tests.** Three of the operator's gitignored fake-backed targets were run read-only on the
  final tree. The document target passed 38 with 1 ignored. The blocked-apply redaction target and the
  split-equivalence target each passed 1. The live-catalog target was not run, because it makes live calls.
- **Privacy.** No commit message from this pass, and nothing in this file, names the operator's org, domain, apps,
  bundle identifiers, layout, hosts or home paths. The private targets are named here only by role.

## Residual

- **The F-slice residuals are closed.** F1's positional `Walk.items` is gone. G1's `EachInstance { planned, key,
  item }` carries each instance's item beside its plan index, so an item can no longer drift out of line with its
  instance. The per-node alignment of `groups` with `nodes` is still by construction (see the `zip` item below).
  The plan-time scalar impostor gap is closed by `fb36435`.
- **The ratchet is a substring count of four spellings.** The equality pin and the split pin (finding 1) stop the
  count from drifting. They do not catch a fallback written another way: `Option::unwrap(x)` in UFCS form, a
  slice index, `assert!`, `todo!` or `unimplemented!`. The record names these so nobody reads the pin as proof of
  totality.
- **Two `zip`s rely on lengths matching by construction.** In `apply`, `checked.nodes()` is zipped with
  `fresh.groups`. In `resolve_list_source`, `elements` is zipped with `edge.elements()`. Both are aligned by
  construction, since `walk` pushes one `Group` per node and `check` builds both lists together. A future edit that
  broke that alignment would truncate silently instead of failing. No mutation targeted either one.
- **`Edge::deliver` does not check the value's type on an unconverted edge.** It passes the value through. Its type
  is guaranteed upstream: by `check_input_types` for inputs, by F2's check at production for tool outputs, by
  `check` for literals, and, after finding 2, by the pinned `Source` `ty` for skipped outputs.
- **Stale docs, for D1.** `plan()`'s own doc still says "Walks `checked.order`" and has a `# Panics` section about
  names "not actually present in its own workflow". Neither is true since G1 and G7. This is in addition to the
  dangling `Checked::workflow` link in `willikins-server/src/startup.rs`, which G7's addendum already handed to D1.
- **Gates not run here.** The coordinator still runs the full four-gate run. This pass ran only scoped gates:
  `-p willikins-core` fmt, clippy and tests, `-p willikins-dsl` acceptance and composition, and the three private
  targets.
