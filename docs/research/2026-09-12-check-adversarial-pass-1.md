# Adversarial pass 1 on `check` (acceptance test 12, first pass)

**Date:** 2026-09-12
**Target:** `crates/willikins-core/src/check.rs` and `workflow.rs` as merged by task 7
(`68f3df7`, `b74bc6d`)
**Plan:** `docs/plans/2026-09-11-milestone-1-core.md`, sections "Type model", "willikins-core",
"willikins-providers-fake", "Acceptance tests"
**Fixtures:** `crates/willikins-core/tests/check_adversarial.rs`

The goal set for this pass: construct a `Workflow` that `check` **accepts** and that either
moves a secret into a non-secret place, or that a later stage (`describe`, `plan`) could not
execute; or one that `check` **rejects wrongly**. Four defects were found and fixed. Three of
them (findings 1, 2 and 3) were observed failing against the pre-fix code; the fourth is a
latent hole no workflow can reach today, pinned by a property-test invariant instead.

## Baseline

All four gates passed on the inherited tree before any change was made
(`rtk proxy cargo fmt --all --check`, `clippy --workspace --all-targets -- -D warnings`,
`test --workspace`, `check -p willikins-types`).

## Findings

### 1. A workflow input's default value was never checked against its declared type (leak)

`check_workflow_inputs` looked only at `InputSpec.ty`. An input could be declared `Text`,
defaulted to a `DopplerServiceToken`, and bound to `template.render`'s non-secret `value`
port. `check` resolved the binding from the *declared* type, accepted the workflow, and
recorded `Text` as the resolved type of the sink port — while the value a later stage would
actually push into the template is the secret default. Observed pre-fix output:

```
expected check to reject this workflow, got Checked { workflow: Workflow { name: "secret-default",
  inputs: {motd: InputSpec { ty: Text, default: Some([REDACTED DopplerServiceToken]) }}, ... },
  types: {readme: {template: TemplateSource, value: Text}} }
```

Redaction still holds at the `Value` boundary, so no byte would print; but the taint rule —
"a secret output may only flow to a secret-accepting input" — was defeated at the one place
where a value enters the graph without passing through a tool.

The same hole without a secret is a plain unexecutable workflow: `visibility: RepoVisibility`
defaulted to an `EnvironmentSlug`, or a scalar default on a `list<T>` input (and the reverse).

**Fix:** `CheckError::DefaultTypeMismatch { input, expected, found }` — a seventeenth variant
the plan does not list, because the plan type-checks defaults when a *document* is loaded,
which leaves a `Workflow` built any other way unchecked. Reported per input in declaration
order; suppressed for an input whose declared type is already `SecretWorkflowInput`, which is
the root cause. Commit `8c484f5`.

Tests: `a_secret_default_on_a_non_secret_input_is_rejected`,
`a_default_of_the_wrong_non_secret_type_is_rejected`,
`a_default_of_the_right_type_but_the_wrong_cardinality_is_rejected`,
`well_typed_defaults_including_unknown_are_accepted` (the positive fixture's own defaults, and
an `Unknown` default of the declared type, must keep passing).

### 2. Workflow output types overwrote the port types of a node named `outputs`

Task 7 recorded a resolved output type into `Checked.types` under a synthetic
`NodeName("outputs")`, in the same map as real nodes' port types, and its own report called
the collision cosmetic. It was not. A workflow with a node literally named `outputs` and a
workflow output whose name matches one of that node's ports had the node's port type silently
replaced (outputs are checked after nodes). Worse, any consumer that iterates `Checked.types`
and looks each key up in `workflow.nodes` — the obvious thing for `describe` and `plan` to do
— **panics** on the synthetic key. The proptest found that independently, shrinking to a
workflow with no nodes and one output.

**Fix:** `Checked.output_types: IndexMap<OutputName, TypeRef>`, a separate map; `Checked.types`
is now exactly the node `with` bindings. The *error* fields keep the `outputs` and `for_each`
sentinels, because changing those means changing the plan's own field lists for `UnknownNode`,
`ItemOutsideForEach` and `UnknownPort`. Commit `898780c`.

Tests: `a_node_named_outputs_keeps_its_own_port_types`, plus the acceptance-4 assertion in
`tests/check.rs` moved to `output_types`.

### 3. `Step` on a `for_each` node with a list-typed output produced a type the model cannot represent

`Step` on a `for_each` node is promoted to `list<T>`. When `T` is itself a list there is no
`list<list<T>>` — `TypeRef` carries a single cardinality flag — and task 7 kept `list: true`,
silently flattening. `check` therefore accepted a workflow whose `sink.lines` binding claimed
`list<Text>` while the graph would produce one `list<Text>` per `for_each` instance: nothing a
later stage could execute or represent.

**Fix:** `CheckError::NestedList { node, port, referenced }` — an eighteenth variant, likewise
absent from the plan. Only the promotion is refused; a list-typed output on a plain node still
binds normally. Commit `f8b1c9a`.

Tests: `step_on_a_for_each_node_with_a_list_output_is_rejected`,
`a_list_output_on_a_plain_node_still_binds_to_a_list_port`. No milestone-1 tool has a
list-typed output, so the attack uses two catalog tools the plan's port table has no row for
(`fake.text_list`, `fake.text_sink`), added to the adversarial catalog only.

### 4. A secret binding with no attributable source was dropped silently (latent)

`check_with_port` reported `SecretToNonSecretSink` only when the resolved secret had a
`(node, port)` source to attribute it to; a secret from a source-less binding (`Item`,
`Input`) hit a bare `return` — no error, no recorded type, binding accepted-and-forgotten.
Unreachable today (a secret `for_each` source is already `SecretForEachSource`, a secret input
is already `SecretWorkflowInput`), so this is defence in depth rather than a live leak: the
branch now falls through to the ordinary port-type check, which cannot accept a secret on a
non-secret port either, so no binding is ever dropped without an error. Pinned by a proptest
invariant: after any `Ok`, every bound `with` port has an entry in `Checked.types`.
Commit `f8b1c9a`.

## Attacks that found nothing (pinned)

| # | Attack | Outcome |
| --- | --- | --- |
| 1 | `Keyed` into a `for_each` node whose output port is a secret list, feeding `template.render.value` | `SecretToNonSecretSink { from: (secrets, tokens), to: (readme, value) }` |
| 2 | `Step` into a `for_each` node whose element output is secret (promoted to `list<DopplerServiceToken>`), feeding `template.render.value` | `SecretToNonSecretSink` |
| 3 | `for_each` over a workflow input list whose element type is secret, `Item` into `github.actions_secret.ensure.value` | exactly `SecretWorkflowInput`. The task brief expected `SecretForEachSource`; the implementation reports the root cause and cascade-suppresses the consumer. Kept: one error naming the real problem beats two naming a symptom. Discrepancy recorded here, not fixed. |
| 4 | The literal `[REDACTED DopplerServiceToken]` bound to a `Text` port | accepted, resolves to `Text`. Nothing special-cases the marker, which is correct: the marker is what redaction *prints*, never what it recognises. |
| 5 | A secret bound to a non-secret port that is *also* a type mismatch | only `SecretToNonSecretSink`; the security error is never hidden behind a type error. |
| 6 | The same `for_each` node referenced by both `Step` and `Keyed` | both accepted, `list<DopplerConfig>` and `DopplerConfig` respectively; one edge, declaration order preserved. |
| 7 | `Keyed` naming an output port the referenced `for_each` node's tool does not have | `UnknownPort` naming the *referenced* node, whose spec lacks the port. |
| 8 | A `with` key bound twice | structurally impossible: `IndexMap` keeps the first position and the last binding. The DSL (task 10) must detect a duplicate YAML key itself — the same gap `DuplicateNode` exists for. |
| 9 | A node whose `for_each` reads its own output | exactly one `Cycle { nodes: [configs] }`; the `Item` bindings it breaks cascade silently. |
| 10 | A *required* port bound to `Item` outside a `for_each` node | exactly `ItemOutsideForEach`; not also `UnboundInput`, since the port is bound. |
| 11 | An unknown tool plus a broken `for_each`, an `Item` and an undeclared input on the same node | exactly `UnknownTool` for that node, plus other nodes' errors in the documented order; identical across two runs. |
| 12 | An empty workflow (no inputs, no nodes, no outputs) | accepted: empty order, `Class::Reversible`, no warnings, both type maps empty. |
| 13 | A workflow **output** bound to a secret | **accepted, deliberately** — see below. |

### The decision on secret workflow outputs

The design doc constrains where a secret may *flow*: "a secret output may only flow to a
secret-accepting input" (`docs/plans/2026-09-11-willikins-design.md`, line 62). A workflow
output is not an input, and in milestone 1 it is not part of `Plan` at all (`Plan { nodes,
class, requires_approval }` has no outputs field). Every rendering path goes through `Value`,
which redacts by construction. So a secret workflow output is accepted, and
`Checked.output_types` records it as the secret type it is.

This is a choice, not an oversight, and it is pinned by
`a_secret_workflow_output_is_accepted_and_keeps_its_secret_type`. Milestone 2's
workflow-as-tool must type a composite's output ports; at that point a secret workflow output
becomes a secret *output port* and the ordinary sink rule covers it with no special case. If
that milestone instead decides a workflow may not export a secret at all, the place to enforce
it is `check_outputs`, and this test is the one to invert.

## Property test

`check_is_total_deterministic_and_sound` (proptest, 200 cases, workflows of up to 3 inputs,
4 nodes and 2 outputs drawn from the test catalog plus one unregistered tool name, one
unregistered type name, and literals including the empty string, a `dp.st.` token and the
redaction marker) asserts:

- `check` never panics, for any generated workflow;
- it is deterministic: two runs agree;
- a rejection is never empty;
- on acceptance: `order` is a permutation of the nodes in which every `Step`, `Keyed` and
  `for_each` dependency precedes its dependent; every bound `with` port has a resolved type
  (finding 4's invariant); and no resolved secret type sits on a port that does not accept
  secrets (findings 1 and 4's invariant, from the outside).

The soundness assertions found finding 2's panic on their own, before the hand-written test
for it was reached.

## Plan defects found (reported, not fixed — `docs/plans` is off limits to this pass)

1. **The plan's sixteen `CheckError` variants are not enough.** Two were added:
   `DefaultTypeMismatch` and `NestedList` (findings 1 and 3). The plan's list should grow by
   both, and the `willikins-core` section's `Workflow` bullet should state that an input's
   default is type-checked by `check`, not only at document load — `check` is the only gate a
   programmatically built `Workflow` passes through.
2. **`DuplicateNode` is unreachable from `check`** (inherited from task 7's report, still
   true): `Workflow::nodes` is an `IndexMap`. The plan should say the variant belongs to the
   DSL's document layer.
3. **The plan does not say whether a workflow output may be secret.** Decided here as
   "accepted"; the plan should state it, together with the milestone-2 consequence for
   workflow-as-tool.
4. **The plan gives no home for an output binding's or a `for_each` binding's own errors.**
   Task 7 used synthetic `NodeName("outputs")` and `PortName("for_each")` sentinels, which
   collide with a real node or port of that name. The types half of that collision is fixed
   here; the error half needs the plan to give `UnknownNode`, `ItemOutsideForEach` and
   `UnknownPort` a site that is not a `(NodeName, PortName)` pair.
5. *(Not a plan defect — struck.)* The plan's acceptance test 4 asks for `SecretForEachSource`
   on a `for_each` over **`fake.secret_list`'s `tokens`**, a tool output, which the
   implementation does emit (`tests/check.rs::acceptance_4_for_each_over_a_secret_source_is_rejected`).
   Only *this pass's brief* asked for it over a secret workflow *input*, where the
   implementation reports `SecretWorkflowInput` instead. The plan is consistent; see the
   attack table, row 3.
6. **`check` never validates that a workflow input's declared type is registered at all**
   (inherited from task 7's report), only its secrecy when known. The plan assumes the DSL
   rejects an unregistered type name at load; nothing states it for a `Workflow` built any
   other way. Same shape as finding 1, and worth one rule: everything the document format
   validates, `check` should validate too.

## Commits

- `8c484f5` — finding 1: reject a workflow input whose default value is not its declared type.
- `898780c` — finding 2: give workflow outputs their own type map.
- `f8b1c9a` — findings 3 and 4: reject a `Step` promotion that would need a list of
  lists; never drop a secret binding silently; the property test.
- This note itself, committed last.
