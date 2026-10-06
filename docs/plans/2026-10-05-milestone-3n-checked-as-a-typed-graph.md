# Milestone 3n: `Checked` is a typed graph that plan, apply and describe consume

**Created:** 2026-10-05 (the typed-graph lane of the operator's "what's next? We can explore multiple lanes in
parallel"; source `todos/2026-09-23-checked-as-a-typed-graph.md`)
**Reviewed:** 2026-10-05 (portfolio review of the five plans of 2026-10-05: 3k, 2b, 3l, 3m, 3n)
**Addendum:** 2026-10-05 — portfolio review. Order confirmed and extended: 3k first, then F1–F3, then 3l, then 2b,
then S1 onward, then 3m last, so 3m's `rollback.rs`, `resolve_pure` and `not_reversible` are written against the
sealed graph. Fixes: F3 has three test-only `known_dyn` callers, not two (`value.rs`'s own tests too); B1's commit 1
adds the `render.rs` arm `willikins-cli` needs (`check_error_detail` matches `CheckError` exhaustively); B2 is one
workspace commit with no shim, because Rust has no overloading and a `#[deprecated]` shim fails every unmigrated
caller under `-D warnings`; the gitignored `operator_*` tests call the three-argument `plan` (16 sites) and `apply`
(8 sites), so B2 migrates them locally, uncommitted (Needs the operator); a trybuild fixture directory sits beside
`tests/it/`, never inside it. If 3m lands first after all, G3, G7 and B2 carry `rollback.rs`.
**Addendum:** 2026-10-06 — F2 landed as one commit, not the table's two. `fill_outputs` is one shared function
between `plan_one` and `apply`'s post-`ensure` site; changing its signature (checked, registry-taking) without
updating both call sites in the same commit does not compile, so "green alone" forces a single commit covering
`plan.rs`, `apply.rs` and their tests — the two call sites are two production sites of one check, not two
behaviours. Also fixed in the same commit, discovered by the full-crate runs verify item 5 calls for: the
Skip-path `fill_outputs` call (plan.rs) would have added a 31st non-test `unreachable!` against the plan's own
ratchet baseline (30), so it now calls a new infallible `unknown_outputs` helper instead. Two pre-existing
`willikins-core` tests documented the exact gap F2 closes and are evidence of red: `apply_adversarial.rs`'s
`boundary_a_secret_returned_on_a_non_secret_port_flows_on_but_never_prints` asserted the old flow-through (its own
doc comment said "pinned rather than fixed here... Handed to pass 2"), and
`undeclared_outputs_are_dropped_and_forgotten_ones_become_unknown`'s `LyingEnsureTool` carried a wrong-typed `org`
that the old unchecked fill silently accepted. Both are rewritten to assert the new refusal (the first renamed to
`a_secret_returned_on_a_non_secret_port_is_refused_before_it_reaches_a_sink`); `LyingEnsureTool`'s `org` is now
correctly typed so it isolates only the undeclared/missing-port behaviour it was otherwise meant to pin, and the
secret-on-non-secret lie moved to a new `SecretLeakEnsureTool`. `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`'s
"Handed to pass 2" entry is now closed; flagged for D1 to update (names the superseded test). Verify item 5: zero
in-tree tools (willikins-providers-fake and all six provider crates) returned a wrong-typed output on any existing
test, so no provider-crate fix commits were needed.
**Addendum:** 2026-10-06 — F3, two corrections found landing it. First, decision (f3) says `known_dyn_as` tests the
object "through the global registry", and SHARED VALUES' signature carries no registry parameter; that cannot be
right once a caller's `Catalog` is not built over the global registry, which is exactly what every 3d conversion
fixture does (`tests/it/apply.rs`'s `registry()` leaks its own `TypeRegistry` holding only its local `ConvA`/`ConvB`
types). `plan`'s `for_each` keying, tested against the global registry, raised a spurious `EdgeTypeMismatch` on the
otherwise untouched, currently-green `apply::conversions::keyed_and_item_edges_deliver_known_b` — acceptance test 6
forbids breaking it, and it is real evidence, not a hypothetical. `known_dyn_as` gained a leading
`registry: &TypeRegistry` parameter, the same shape `is_operator_acknowledgement`'s and `is_template_source`'s own
doc comments already justify for the identical reason; `plan`'s keying passes `catalog.registry()` (the same
registry `check_input_types` already validated the source's declared element type against, immediately before any
node is planned); the three test-only callers (`value.rs`, `disclosure.rs`, `willikins-server/src/mcp.rs`) pass
`willikins_types::registry()`. Second, verify item 3 is now settled empirically rather than left open: a minimal
two-crate probe outside this workspace (not committed) confirmed that both `cargo check` and trybuild's
`compile_fail` (which compiles in a `cargo check`-equivalent, no-codegen mode) miss a `const { assert!(...) }`
failure inside a generic function body — the assertion is only evaluated at monomorphization, which codegen
reaches and type-checking does not — while `cargo build` and a rustdoc `compile_fail` doctest both catch it. F3
therefore places the compile-fail proof as a doctest on `Value::known`, exactly as the plan's own fallback
anticipated, and does not add a trybuild case for this acceptance test.

## Goal

`check` already parses a document. What it returns is mostly the document again (`Checked::workflow`) plus side tables,
so `plan`, `apply` and `describe` walk the raw `Binding`s a second time, look nodes up by name, look tools up in a
catalog, re-parse literals, and re-assert with `unreachable!` what `check` already established. The operator's
principle (2026-09-23): *parse, don't validate*. This milestone makes `check` return a parsed graph whose consumers
**cannot represent** the states `check` ruled out.

1. **Part F (fixes that do not wait).** Two panics are reachable from a YAML document today, both because `apply`
   re-derives from the document instead of from what `check` and `plan` produced (decision (f1)). A tool's own
   outputs are type-checked where they are produced. No `Value` is ever named by what an object says about itself.
   These three tasks can land before milestone 2b.
2. **Part S (sealed).** `Checked`'s fields are private and `check` is its only constructor. A hand-built `Checked`
   no longer compiles, which closes both 3d bypasses (a converted edge moved onto another port, an undeclared input
   bound).
3. **Part G (the graph).** Nodes in topological order, each holding the tool handle `check` resolved, typed node ids
   by kind, and for every bound port a classified, typed source. `plan` and `apply` walk nodes and sources. They
   never match on a `Binding`, never look a node or tool up by name, never re-parse a literal. `describe` reads a
   check-built input table. Of the 24 `unreachable!` in `plan.rs` that re-assert `check`, 7 in `apply.rs` and 2 in
   `describe.rs`, none survives. Each case becomes unrepresentable instead of moving somewhere else.
4. **Part B (boundaries).** Two things that are lints today become refusals. First, an identifier-typed `for_each`
   source is a `check` error, replacing milestone 3i's document `for_each` guard test. Second, `plan` and `apply` lose their
   `&Catalog` parameter, so "no second lookup" is a signature fact and no longer a clippy rule plus a grep.

Nothing observable changes for any document that runs today. Every plan JSON, fingerprint, journal line, negative
fixture's exact errors and the characterization snapshot stay byte-identical. The only exceptions are the new
refusals in parts F and B, each of which is pinned by its own test.

## What earlier documents already decided

- **Milestone 3d, decision (e)** (`docs/plans/2026-09-23-milestone-3d-conversions.md`): `Edge` (private fields,
  `pub(crate)` constructors, `ty`/`conversion`/`delivered`/`elements`/`deliver`) is the first brick. It stays public
  and unchanged here. This milestone attaches a classified source to it and indexes it by node id.
- **Milestone 3d's follow-up (2026-09-24):** "fail loudly at parsing". A wrong-typed value is an error naming types,
  never content, and never a panic. `TypeId`, never `TYPE_NAME`, decides whether an object is of a type.
- **The 3d adversarial pass** (`docs/research/2026-09-23-m3d-adversarial-pass.md`, "Not settled"): the hand-built
  `Checked` routes. A tool's wrong-typed output reaches the next tool. A misnamed object in a list output panics in
  `for_each` keying. `Value::known`/`known_dyn` of a misnamed type panics at construction.
- **Milestone 3i, decision (b8)** and its adversarial residual (`docs/research/2026-10-02-m3i-adversarial-pass.md`): a
  `for_each` instance key is a pre-rendered string. Three guard tests pin, for shipped documents only, that none is
  identifier-typed. "Making it impossible ... takes a `check` refusal ... the coordinator's decision."
- **`check`'s module doc:** the error accumulation order and cascade suppression are normative and stay exact.

## Out of scope

- Secrecy inference (`todos/2026-09-22-secrecy-inference.md`). It builds on this milestone's `Source`/`PortEdge`
  (one constraint per use site) and comes after it.
- `check.rs`'s own internal `unreachable!`s (7) and `unwrap`s (2), which assert its own invariants.
- The invariants `Catalog::insert` establishes for gates (`plan.rs`'s four gate `unreachable!`s). They are a catalog
  property, not a `check` one.
- Validating `DomainType::TYPE_NAME` in the derive (decision (b3) says why it waits).
- Plan identity covering inputs (`todos/2026-09-14-plan-identity-must-cover-inputs.md`).
- Any change to `Binding`, `Workflow` or the DSL. Documents, the linker of milestone 2b and `check`'s input are
  untouched.

## Decisions, part F: fixes that do not wait

### (f1) Two apply panics, predicted from reading, pinned red first

Both are cases of `apply` re-deriving from the document. The red test confirms each message (verify item 1).

1. **An empty `for_each` source.** `plan` records a zero-instance node as `NodeResult::ForEach(vec![])`, so a later
   `Step` reference aggregates to a known empty list. `plan_adversarial.rs`'s
   `an_empty_for_each_source_plans_zero_instances_and_a_known_empty_list` pins this for `plan`. `apply`'s loop walks
   `fresh.nodes`, which has no entry for a zero-instance node, so `apply`'s results never get one. A downstream
   node's `Step` port (re-resolved mid-run) or a workflow output referencing it then reaches
   `unreachable!("`checked.order` plans every node before its dependents")` (`plan.rs`, `resolve_step`).
   **Minimal fix:** `apply` walks `checked.order` and takes each node's group of planned instances, possibly empty,
   from `fresh.nodes`. An empty group of a `for_each` node records `NodeResult::ForEach(vec![])`, exactly as `plan`
   does.
2. **`${{ item }}` inside a `with:` list on a `for_each` node, beside a `${{ steps.… }}` element.** `check` accepts
   it, because a list element resolves `Item` through the node's `ItemContext`. `plan` passes the item. `apply`'s
   `resolve_instance_inputs` re-resolves the whole list, since one element may change mid-run, and passes
   `item: None`. That reaches `unreachable!("`check` rejects `item` used outside a for_each node")`.
   **Minimal fix:** a crate-private `plan` walk returns each planned instance's item `Value` beside the `Plan`
   (decision (g6)'s `Walk`, in its first form), and `apply` passes the instance's own item.

Both fixes are later subsumed by the graph (G1, G6). They land first because they are panics a document can reach,
and because their tests then guard the rewrite.

### (f2) A tool's outputs are type-checked where they are produced

`fill_outputs` clones whatever a tool returned, in `plan_one` and after `ensure` in `apply`. A pure tool declaring
`out: ConvA` and returning a `ConvB` reaches the next tool (3d probe). The fix is a check at production: every
declared output port the tool provided must have `value.ty() == declared`, in name and list flag, and every known
object must pass `registry.type_matches(&declared.name, object) == Some(true)`, which is the `TypeId` test.

**Error surface: no new variant.** At plan the error is `PlanError::Tool { node, error }`, and at apply it is
`ApplyError::Tool { node, instance, error, applied }` with the instance `Failed`. In both, `error` is
`ToolError { kind: Invalid, message }` and the message is in SHARED VALUES. The reasons:

- `plan_error_serde.rs` and the server's `plan_error_kind` enumerate `PlanError`'s kinds, and milestone 2b adds
  `InputNotSettable` to the same enum. A new variant would move the error snapshots and the journaled kind set for an
  error that only a broken catalog can produce.
- Wrapping the error in the existing `Tool` variants gives plan and apply the same shape. A tool that breaks its own
  spec is reported exactly like a tool that fails.

The cost is that this error's `kind` is `Invalid`, the same kind a tool's own input refusal uses. The message tells
the two apart. The message names the node, port and types, never content. A same-named impostor uses the "another
Rust type declared as" wording (as `InputTypeMismatch` does), and a misnamed object falls back to the declared name
(`reported_type_name_or`). Undeclared extra ports a tool returns are still dropped, as today.

At apply, the `ensure` has already run when its outputs are refused. That is the same position as any `ensure` that
fails after a partial write, and the error says so.

### (f3) No `Value` is ever named by what its object says about itself

- **`Value::known::<T>` and `known_list::<T>`** gain `const { assert!(willikins_types::registry::is_type_name(T::TYPE_NAME), "...") }`.
  A misnamed `T` is then a compile error at the call site, where today it panics at run time. This needs a `const fn
  is_type_name(&str) -> bool` in `willikins-types` (byte loop: `[A-Z]` then `[A-Za-z0-9]*`, the same grammar as
  `TYPE_NAME_PATTERN`). `TypeName::from_static`'s `debug_assert!` uses it too. A test pins that the `const fn` agrees
  with the regex on a table of names. Inline `const` blocks are stable since Rust 1.79, and the workspace's
  `rust-version` is 1.88.
- **`Value::known_dyn(object)`** names the value by `object.type_name()` and panics on a misnamed object. It is
  replaced by `Value::known_dyn_as(ty: TypeName, object) -> Result<Value, ObjectTypeMismatch>`, which tests the
  object by `TypeId` against `ty` through the global registry. Its only production caller is `plan`'s `for_each`
  keying, which now names each item by the source's declared element type. A mismatch is
  `PlanError::EdgeTypeMismatch { site: Site::ForEach { node }, expected, found }`, an existing variant at an existing
  site. Once (f2) has landed, an input (already type-checked) and a tool output (checked at production) cannot reach
  this, so it is a backstop. Its three test-only callers (`value.rs`'s own tests, `disclosure.rs`'s test table,
  `willikins-server/src/mcp.rs`'s test helper) move to `known_dyn_as`.
- `known_dyn_list(element, items)` is unchanged. Tests use it on purpose to forge impostor lists, and `plan`'s
  aggregation feeds it only outputs that (f2) has already checked.

## Decisions, part S: sealed

### (s1) Private fields, accessors, one constructor

`Checked`'s six fields become private to the `check` module. The only `Checked { … }` struct literal in non-test code
is in `check()`. The front ends' reads become accessors, mechanically:

| Read today | Becomes |
| --- | --- |
| `checked.workflow.name` (CLI `commands.rs`, journal tests) | `checked.name()` |
| `checked.workflow.inputs` (CLI `main.rs`, server `butler.rs`) | `checked.document().inputs()` |
| `checked.workflow.outputs.keys()` (CLI test) | `checked.document().output_names()` |
| `checked.warnings` | `checked.warnings()` |
| `checked.class` | `checked.class()` |
| `checked.order` | `checked.order()` |
| `checked.types[..][..]`, `checked.types` iteration | `checked.edge(&node, &port)`, `checked.types()` |
| `checked.output_types` | `checked.output_types()` |
| `format!("{:?}", checked.workflow)` (check_adversarial) | the test formats its own `workflow` |

`document()` returns a `DocumentView<'_>` that exposes the name, description, declared inputs and output names, and
**not** the nodes. `plan.rs`, `apply.rs` and `describe.rs` are other modules, so they cannot reach the private
fields. Until G7 they read the workflow through a `pub(crate) fn workflow()` that G7 deletes. After G7, no code
outside `check.rs` can reach a `Node` or a `Binding` through a `Checked`. That is a type boundary, not a grep.

Accessor return types are fixed from S1 on, so later tasks never change their call sites (SHARED VALUES). `types()`,
`output_types()` and `order()` return owned values. They are read only by tests and the characterization, and from
G7 on they are views computed from the graph. Then the characterization prints what `plan` actually consumes, not
a parallel copy of it.

### (s2) The forged-`Checked` test becomes a compile-fail test

`crates/willikins-core/tests/apply.rs`'s `a_converted_edge_moved_onto_another_port_errors_at_plan_and_apply` builds
`Checked { types: forged, ..checked.clone() }`. That no longer compiles, and its replacement proves it: a trybuild
case asserting that a struct literal of `Checked`, and a read of a private field, are refused (pinned `.stderr`).
`trybuild` is already a workspace dependency. The `EdgeTypeMismatch` backstop keeps a test, but only as a
`#[cfg(test)]` unit test inside `check.rs`, the one module that can still forge an edge.

### (s3) `Debug` is hand-written from G1 on

From G1 on, `Checked` holds `Arc<dyn Tool>`, and `dyn Tool` is not `Debug`. `Checked`'s `Debug` is written by hand.
It prints node names, tool names, sources by kind and type, and never a literal's content: a literal prints as its
`Value`'s own `Debug`, which redacts by type. `check_adversarial.rs`'s redaction case must stay green.

## Decisions, part G: the graph

### (g1) Representation

```text
Checked {
  document:  Workflow                // private; only DocumentView reaches it
  registry:  &'static TypeRegistry   // the catalog's, captured by check
  inputs:    Vec<CheckedInput>       // declaration order; InputId = index
  nodes:     Vec<CheckedNode>        // TOPOLOGICAL order (declaration tie-break, unchanged)
  outputs:   Vec<CheckedOutput>      // declaration order
  class, warnings
}
CheckedInput  { name, spec: InputSpec, entry: &'static TypeEntry, awaited: bool }
CheckedNode   { name, declared_at: usize, tool: Arc<dyn Tool>, shape: Shape }
Shape         = Single { id: SingleId, ports: Vec<PortEdge<Infallible>> }
              | Each   { id: EachId, source: Source<Infallible>, element: TypeRef, ports: Vec<PortEdge<InEach>> }
PortEdge<I>   { port: PortName, with_position: usize, bound: Bound<I> }   // tool-spec input order, bound ports only
Bound<I>      = Scalar { source: Source<I>, edge: Edge }
              | List   { elements: Vec<Source<I>>, edge: Edge }           // edge is Edge::list; elements align with edge.elements()
Source<I>     = Literal(Value)                                    // parsed once, by check
              | Input(InputId)                                    // a declared input, always
              | Item(I)                                           // I = Infallible outside an Each node's ports
              | Step      { from: SingleId, slot: OutputSlot, ty: TypeRef }
              | Aggregate { from: EachId,   slot: OutputSlot, ty: TypeRef }   // Step onto a for_each node: list<T>
              | Keyed     { from: EachId,   key: String, slot: OutputSlot, ty: TypeRef }
CheckedOutput { name, source: Source<Infallible>, ty: TypeRef }
```

Why each piece:

- **Typed node ids by kind** (`SingleId`, `EachId`). These are separate index spaces, so `plan`'s and `apply`'s
  result tables are typed: `Vec<SingleResult>` (`Outputs | Skipped`) and `Vec<EachResult>` (`Instances | Skipped`).
  A `Keyed` holds an `EachId`, so "a keyed reference to a node with no `for_each`" cannot be written down.
  `Aggregate` versus `Step` replaces the `for_each.is_some()` lookups. The source carries the declared output type,
  so typing an `Unknown` for a skipped node needs no catalog lookup.
- **`Item(Infallible)`**. A single node's ports, a `for_each` source and a workflow output cannot hold an item: the
  match arm is `Source::Item(never) => match never {}`. Resolution of a `Source<InEach>` takes `&Value` (the
  instance's item) as a non-optional argument. One generic resolver serves both:
  `resolve<I>(…, source: &Source<I>, item: impl Fn(&I) -> Value)`.
- **`Literal(Value)`**. `check` parsed it already (`check_literal` now returns the `Value`, not its `TypeRef`).
  `plan` delivers the stored value, so it never re-parses and its two "already validated" assertions go.
- **`Input(InputId)`**. It can name only a declared input, which closes the 3d "undeclared input bound" route by type.
  `CheckedInput.awaited` is `is_operator_acknowledgement` computed once against the catalog's registry. `entry` is
  that registry's `TypeEntry`, so `describe` stops consulting the global registry and stops asserting the type is
  registered.
- **`OutputSlot`**: the output port's index in the producing tool's `spec.outputs`. `fill_outputs`, and (f2)'s
  checked version of it, fill every declared port in spec order, so a `FilledOutputs` (crate-private, built only
  there) is indexed by slot. It still renders as today's `Outputs` map wherever a `PlannedNode` or an event carries
  it.
- **`declared_at` and `with_position`** carry the two orders the graph does not otherwise hold (SHARED VALUES):
  the characterization's TYPES section is in node declaration order, and a gate's `awaiting_inputs` is in the node's
  `with:` order.
- **`tool: Arc<dyn Tool>`**. This is the handle `check` resolved, so the walk never asks a catalog again. The server
  and CLI already check and plan against one catalog (`butler.rs` holds `self.catalog` for both). Verify item 4 is
  the audit that this holds everywhere, and it is a precondition of G1, not of B2.

**Typed ids are indexes, not proofs, and the plan says so.** `single[from.0]` is a bounds-checked index, not an
`unreachable!`. Ids are minted only by `check`, never leave `willikins-core`, and index tables built in the same
topological pass, where a source's id is always lower than its consumer's. A generative brand (a lifetime per
`Checked`) would make a foreign id a type error too. It costs every signature a lifetime parameter to defend against
code that cannot exist outside this crate, so it is not done.

### (g2) What `plan` does now

`plan` walks `checked.nodes` in order. For each node, it collects gate causes from its sources (no `Binding`), plans
`Skip` or resolves every port edge (scalar or list) for every instance, calls `plan_one`, and pushes to the typed
result table. Workflow outputs resolve from `CheckedOutput` sources. `check_input_types` walks `checked.inputs`.
`ResolveCtx` holds `inputs`, `registry` and the two result tables, and no longer holds `workflow` or `edges`. The
fallback "a missing edge means pass-through" is gone, because every bound port carries its edge (verify item 2).

### (g3) What `apply` does now

`apply` walks `checked.nodes` and the `Walk`'s per-node groups (g6) together, so zero-instance nodes and
instance-to-item pairing are structural (subsuming (f1)). It resolves **every** port of an instance from the graph,
not only `Step`/`Keyed` ports layered over `planned.inputs`. `Literal` (the same stored `Value`), `Input` (the same
`inputs` map) and `Item` (the same `Value` the walk recorded) resolve to exactly the values `plan` used, so the
`NodeStarted.inputs` an observer sees is unchanged. `binding_may_change_mid_run` then has nothing to decide and is
deleted.

**Unknown attribution comes from resolution.** The resolver returns, beside each value, where an `Unknown` came
from: `Upstream(node)` for `Step`/`Aggregate`/`Keyed` and `WorkflowInput(input)` for an awaited input. For a list,
it is the first attributable element, as today. A literal and an item are always known, so they never produce an
`Unknown` and need no attribution. That deletes `classify_unknown_binding`'s two `unreachable!`s and
`first_unknown_required_input`'s "unbound required port" one. `ApplyError::UnknownInput` and
`UnknownRequiredInput` carry exactly the fields they carry today.

### (g4) Gates read sources

`GateTracking::collect_causes` walks `Source`s (`Step`/`Aggregate` by producing node, `Keyed` by node and key) and
skips `Item` at top level exactly as today. `mark_blocked`'s `awaiting_inputs` is every `Source::Input` among the
node's port edges whose resolved value is `Unknown`, deduplicated by name and **sorted by `with_position`**, which is
today's `with:`-map order. `BlockedGate` and `Replacing` keep names, so the plan JSON is unchanged.

### (g5) What `describe` does now

`describe` walks `checked.inputs`. `missing_input` and `awaiting_input` read `CheckedInput.entry`, and its two
`unreachable!`s go. An undeclared name in the caller's partial inputs is still an `InputError`, unchanged. Milestone
2b's `fixed_by` rides in `CheckedInput.spec`, so 2b's R1 hiding keeps working unchanged (sequencing below).

### (g6) The walk `plan` shares with `apply`

`pub(crate) fn walk(checked, inputs, …) -> Result<Walk, PlanError>`. `plan()` returns `walk(..)?.plan`. A `Walk` is
`{ plan: Plan, groups: Vec<Group> }`, aligned with `checked.nodes`:
`Group = Single { planned: usize } | Each { instances: Vec<EachInstance { planned: usize, key: String, item: Value }> } | Skipped { planned: usize }`,
where `planned` indexes `plan.nodes`. F1 introduces the item vector as the walk's first form, and G1 gives it this
shape. Fingerprints and plan JSON are unaffected: the `Plan` is the same value.

## Decisions, part B: boundaries

### (b1) An identifier-typed `for_each` source is a `check` error (coordinator sign-off)

`CheckError::IdentifierForEachSource { node, ty }` is reported when a `for_each` source's element type is marked
`#[domain(identifier)]` in the catalog's registry (`entry.info.identifier`). An instance key is a rendered string and
also the text a `Keyed` reference matches, so it cannot be masked without breaking keyed references. It also cannot
be printed whole without breaking 3i's promise. Refusing is the honest answer, and no shipped, fixture or private
document does this (verify item 6).

**Order** (`check_for_each`): secret source, then scalar source, then **identifier**, then
`colliding_for_each_default`. The identifier check comes before the duplicate-default check because that error's
`key` field is a rendered identifier. The module doc's "Error ordering" section gains this sentence.

**3i's guards:** `no_shipped_document_for_each_source_resolves_to_an_identifier_typed_list` is deleted, because
`check` now refuses what it searched for. A negative fixture plus its acceptance test replace it.
`no_shipped_document_declares_an_identifier_typed_input_default` stays. A default is not a `for_each` key, and
`describe` already masks it. The catalog guard (no identifier-typed key or gate-subject port) stays as a test,
because it is a catalog property, and moving it into `Catalog::insert` is a separate change.

The cost: `check` accepts strictly less, and the characterization gains exactly one entry, for the new fixture.
No existing entry moves.

### (b2) `plan` and `apply` lose `&Catalog` (coordinator sign-off)

From G1 on, the catalog parameter is used only for `catalog.registry()`, and `Checked` holds that registry. B2
removes the parameter: `plan(&checked, &inputs)` and `apply(&checked, &inputs, &approved, &approval, &mut observer)`.
The disallowed-methods rule and the `probe_conversion` tripwire stay as belt and braces. The structural fact is that
`plan` and `apply` have no catalog to probe.

This is about 208 `plan` and 109 `apply` call sites in roughly 30 files, plus milestone 3m's `resolve_pure` if it has
landed. The edit is mechanical: delete one argument. **Precondition:** verify item 4's audit, already done before G1,
showed that no caller checks against one catalog and plans or applies against another. B2 is **one workspace commit**
(portfolio review: a per-crate split needs a shim, a shim cannot share the name `plan` because Rust has no
overloading, and a `#[deprecated]` one turns every unmigrated caller into a `-D warnings` failure). The same step
migrates the 16 `plan(&checked, …, &catalog)` and 8 `apply(` call sites in the gitignored
`crates/willikins-cli/tests/operator_*.rs` on the operator's machine, never committed (they are not tracked). If the coordinator declines, the parameter stays, documented as "used only for its registry", and
nothing else in this plan changes.

### (b3) Not this milestone: `TYPE_NAME` checked in the derive

A `const` assertion in `#[derive(DomainType)]` would refuse a misnamed type at its definition. Two tests derive
misnamed types on purpose to exercise `reported_type_name_or` (`value.rs`'s `misnamed` module and `tests/apply.rs`'s).
`DomainObject` is unsealed, so a hand-written misnamed object stays possible either way, and the fallback is still
needed. (f3) closes the panics without it. The derive check is a candidate follow-up: two tests would become
trybuild cases.

## Lint to type boundary

| Today | After |
| --- | --- |
| `Checked`'s fields are `pub`; a forged `Checked` is caught by `EdgeTypeMismatch` at run time | private fields; a struct literal does not compile (trybuild) |
| `plan`/`apply` match `Binding`, and a tripwire greps for `probe_conversion` | `plan.rs`/`apply.rs` hold no `Binding` (no import); `Source` is all they see; the tripwire also greps `Binding::` |
| `item` outside a `for_each`: `unreachable!` | `Source<Infallible>`: no value to construct |
| keyed reference onto a single node: `unreachable!` | `Keyed { from: EachId }` |
| undeclared input: `unreachable!` (and the 3d bypass) | `Input(InputId)` into the check-built table |
| literal re-parsed: `unreachable!` twice | `Literal(Value)` parsed once |
| tool looked up in a catalog: `unreachable!` five times | `CheckedNode.tool` |
| output port looked up: `unreachable!` five times | `OutputSlot` into `FilledOutputs` |
| `describe` asserts a registered type twice | `CheckedInput.entry` |
| conversion table "never probed by plan" (clippy rule + tripwire) | plus: `plan`/`apply` take no catalog (b2) |
| 3i's guard test over shipped documents' `for_each` sources | `CheckError::IdentifierForEachSource` for every document |
| `Value::known` of a misnamed type: run-time panic | compile error |
| a tool's wrong-typed output reaches the next tool | refused where produced |

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| Public accessors (S1 on, signatures fixed) | `name(&self) -> &WorkflowName`; `document(&self) -> DocumentView<'_>`; `class(&self) -> Class`; `warnings(&self) -> &[CheckWarning]`; `order(&self) -> Vec<NodeName>`; `types(&self) -> IndexMap<NodeName, IndexMap<PortName, Edge>>`; `edge(&self, &NodeName, &PortName) -> Option<&Edge>`; `output_types(&self) -> IndexMap<OutputName, TypeRef>` |
| `DocumentView<'a>` | `name() -> &WorkflowName`, `description() -> Option<&Description>`, `inputs() -> &IndexMap<InputName, InputSpec>`, `output_names() -> impl Iterator<Item = &OutputName>`. No nodes |
| Crate-private, S1 to G7 only | `pub(crate) fn workflow(&self) -> &Workflow`; deleted by G7 |
| `types()` order | nodes in **declaration** order (`declared_at`), ports in the tool spec's input order, bound ports only; a node with no bound port has no entry. Exactly what `Resolver::record_edge` produces today |
| `output_types()` order | workflow output declaration order; every output present on success |
| `order()` | topological, ties by declaration index (`topo_order`, unchanged) |
| Ids (crate-private newtypes, minted only in `check.rs`) | `SingleId(u32)`, `EachId(u32)`, `InputId(u32)`, `OutputSlot(u32)`; assigned in topological order within each kind |
| Item marker | `pub(crate) struct InEach;` (unit). `std::convert::Infallible` everywhere else |
| `with_position` | `node.with.get_index_of(port)` |
| `awaiting_inputs` order | ascending `with_position`, deduplicated by input name |
| `Inputs` map order (plan JSON, `NodeStarted`) | tool spec input order, bound ports only (unchanged) |
| Walk (G1 on) | `pub(crate) struct Walk { pub(crate) plan: Plan, pub(crate) groups: Vec<Group> }`; `Group` as in decision (g6) |
| Output-check message (F2) | `` returned output `{port}` as `{found}`, but its spec declares `{expected}` ``; impostor (`found == expected`): `` returned output `{port}` as a value of another Rust type declared as `{found}`, but its spec declares `{expected}` ``. `ToolErrorKind::Invalid`. Wrapped in `PlanError::Tool` / `ApplyError::Tool` |
| `const fn` (F3) | `willikins_types::registry::is_type_name(name: &str) -> bool` |
| `known_dyn_as` (F3) | `Value::known_dyn_as(registry: &TypeRegistry, ty: TypeName, object: Arc<dyn DomainObject>) -> Result<Value, ObjectTypeMismatch>` (leading `registry`: 2026-10-06 addendum); `pub struct ObjectTypeMismatch { pub expected: TypeRef, pub found: TypeRef }` (`found` via `reported_type_name_or`) |
| Keying backstop (F3) | `PlanError::EdgeTypeMismatch { site: Site::ForEach { node }, expected: TypeRef::scalar(element), found }` |
| New `CheckError` (B1) | `IdentifierForEachSource { node: NodeName, ty: TypeRef }` (`ty`: the source's resolved `list<T>`), kind `"IdentifierForEachSource"`, Display `` node `{node}`: `for_each` over `{ty}`, an identifier type: its instance keys would print identifiers whole `` |
| B1 fixture | `workflows/fixtures/identifier-for-each.yaml`, shaped like `for-each-over-scalar.yaml`: input `ids: { type: list<AppleCertificateId> }`; step `loop: { tool: fake.irreversible.ensure, for_each: ${{ inputs.ids }}, with: { key: third-thoughts } }`. `check` returns exactly `[IdentifierForEachSource { node: loop, ty: list<AppleCertificateId> }]` |
| Unreachable ratchet (S1 on) | non-test `unreachable!`/`.expect(`/`.unwrap()`/`panic!` per file: the lines above the file's first `#[cfg(test)]` module, or the whole file when it has none (`apply.rs` has none). Measured at `28dc3e1`: `plan.rs` 30 `unreachable!` and 0 of the other three, `apply.rs` 9 and 0, `describe.rs` 2 and 0. Final: `plan.rs` 6 (`replacing_entry`'s key port, the four `Catalog::insert` gate ones, `plan_one`'s "handled above"), `apply.rs` 2 (`check_drift`'s two), `describe.rs` 0. A task lowers the constants to what it leaves and never raises one |
| B2 signatures | `plan(checked: &Checked, inputs: &IndexMap<InputName, Value>) -> Result<Plan, PlanError>`; `apply(checked, inputs, approved, approval, observer)` |
| Placeholders | "the operator's iOS app document", `example-org`, `com.example.app` |

## Sequencing with milestone 2b (composition) and the other lanes

**2b first, then this milestone's S and G parts. F1–F3 may land at any time.**

What the two plans touch in common:

| Structure | 2b (`docs/plans/2026-10-05-milestone-2b-composition.md`) | 3n |
| --- | --- | --- |
| `check()` entry | P2: `Unlinked` as the first thing `check` does | G1: builds the node table at the end |
| `Resolver::resolve` | C1: boundary signature rules, exact type, error-only | G3/G4: returns a `Source` instead of `(TypeRef, Option<(NodeName, PortName)>)` |
| `check_input_types`, `describe` | R1: `InputNotSettable`, `NotSettable`, fixed inputs hidden | G2: both walk `CheckedInput` |
| Front-end reads of `checked.workflow` | S1, K1: `butler.rs` (inputs), CLI `main.rs` (inputs), `commands.rs` (name) | S1: the same lines become accessors |
| `InputSpec` | gains `fixed_by` | carried whole in `CheckedInput.spec` |
| `Checked.class`, `output_types` | read as-is (d5, d11) | accessors, values unchanged |
| Characterization | a new composition snapshot (F1) | must leave both byte-identical |

Why 2b goes first:

1. **2b is what the operator is waiting on.** It has a live convergence step on their document. 3n changes nothing
   they can see.
2. **2b's core edits are additive and 3n's are rewrites.** Additions such as new error variants, a boundary rule
   and an input filter are carried through a rewrite once, by the rewriter, who has them in view. In the other
   order, 2b's implementers would write C1 and R1 against an internal API that is still moving under them.
3. **3n's proof gets stronger.** Landing second, its byte-identical gate also covers 2b's composition snapshot, so
   flattened `<step>/<node>` graphs, fixed inputs and boundary-substituted bindings are proven unchanged too.
4. **The flat graph is 3n's input as-is.** 2b links before `check`, and `check` sees one `Workflow` with no `uses`,
   so the typed graph needs no composition concept. Node paths are just longer `NodeName`s.

F1–F3 touch only `apply`'s walk, `fill_outputs`, `Value` constructors and `willikins-types`, none of which 2b edits.
F1 fixes two panics, so it is worth landing at once.

If the coordinator instead wants 3n first, S1 must land before 2b's S1 and K1 (the same lines), and 2b's C1 is
written against G3's resolver.

**Other lanes.** Milestone 3k (`docs/plans/2026-10-05-milestone-3k-faster-gates.md`) moves every crate's integration
tests under `tests/it/`. Tasks here name test files by role, and an implementer puts them wherever 3k has left that
crate. Milestone 3m's R4 adds `resolve_pure(checked, catalog, inputs, wanted)`, which walks `Checked::order` and
shares `plan`'s resolver. If R4 lands before G3, G3 migrates it. If after, it is written against the graph. B2 also
drops its catalog parameter. The portfolio order (2026-10-05) puts 3m after this whole milestone. If it lands first
anyway, `rollback.rs` (`plan_teardown` walks node bindings and tool declarations) joins G3's and G7's scope, since
G7 deletes the `workflow()` it would read, and its `&Catalog` parameters join B2. After 3k, a new test is a
`tests/it/<stem>.rs` module, a `--test <stem>` gate is `--test it <stem>::`, and S1's trybuild fixtures live in
their own directory beside `tests/it/` (as `willikins-types` keeps `tests/derive/fail/`), driven by one `it` module.

## Acceptance tests

1. **Empty `for_each` at apply** (F1). A document `for_each`es over an input whose value is `[]`, with a single node
   binding `${{ steps.configs.config }}` and a workflow output on the same. `apply` succeeds: zero instances, and
   the downstream port and the output are a known empty `list<DopplerConfig>`. Red first with the `unreachable!`
   message.
2. **`item` in a list port at apply** (F1). A `for_each` node over `list<Text>` with a `list<Text>` port bound
   `[${{ item }}, ${{ steps.up.text }}]` applies. Each instance's `ensure` receives `[<its item>, <up's text>]`. Red
   first with the `unreachable!` message.
3. **Outputs checked where produced** (F2). These cases use the 3d `conv.liar` shape. A pure tool returning `ConvB`
   for `out: ConvA` gives `PlanError::Tool`, `Invalid`, exact message, and the downstream tool is never read. An
   `ensure` returning a wrong type gives `ApplyError::Tool` with that instance `Failed`. A same-named impostor gets
   the impostor message. A list output holding a misnamed object is refused with the declared-name fallback. None
   of these panics.
4. **No self-named value** (F3). `Value::known` of a misnamed `DomainType` does not compile (trybuild or
   `compile_fail` doctest, verify item 3). `known_dyn_as` refuses an impostor and a misnamed object by `TypeId`.
   The keying backstop gives `EdgeTypeMismatch` at `Site::ForEach`, as a unit test with a forged list. `is_type_name`
   agrees with `TYPE_NAME_PATTERN` on a table of at least 12 names (empty, lowercase start, digit start, `_`, `-`,
   non-ASCII, one letter, long).
5. **Sealed** (S1). A trybuild case with a pinned `.stderr`: `Checked { .. }` outside the crate and a read of
   `checked.types` (the field) are both refused. The moved `EdgeTypeMismatch` backstop test passes inside
   `check.rs`.
6. **Byte-identical** (every task). The characterization snapshot (`characterization_of_every_document`), every
   negative fixture's exact errors, every `Plan` JSON and fingerprint, every journal fixture, and 2b's composition
   snapshot if it exists: none moves. A `.snap.new` is a defect, not a review item. The one exception is B1's added
   entry.
7. **No second lookup.** The existing `probe_conversion` tripwire in `check.rs`'s tests is extended. Non-test code in
   `plan.rs`, `apply.rs` and `describe.rs` contains no `Binding`, `catalog.get(` or `.workflow(`. Non-test code in
   `plan.rs` and `apply.rs` also contains no `Value::parse(`. `describe.rs`'s `parse_raw` stays, because it is the
   one sanctioned place a caller's text becomes a `Value`. No grep for `.nodes`, since `Plan::nodes` is legitimately
   indexed there. From G7 on, the nodes are unreachable by type: `DocumentView` has none.
8. **The ratchet** (S1 on). A test counts non-test `unreachable!`/`.expect(`/`.unwrap()`/`panic!` per file against
   SHARED VALUES' constants, and after G7 asserts the final values.
9. **Gates over sources** (G5). A document with a `for_each` gate and acknowledgement inputs bound in a `with:`
   order that differs from the tool's spec order. `awaiting_inputs` keeps `with:` order, `holds_back` and
   `Skip` propagation are unchanged, and `apply_gates.rs` and `plan_gates.rs` pass untouched.
10. **Attribution** (G6). Every existing `UnknownInput`/`UnknownRequiredInput` test passes untouched. A new case: a
    list port of a literal, an item and an upstream `Unknown` attributes to the upstream node.
11. **Identifier `for_each` refused** (B1). The fixture produces exactly `[IdentifierForEachSource]`. The same node
    with a colliding default produces the identifier error and not `DuplicateForEachDefault`. A secret source still
    produces only `SecretForEachSource`.
12. **Describe from the table** (G2). `describe`'s outputs are byte-identical over every shipped document, and
    `describe.rs` has no `willikins_types::registry()` call.
13. **`Debug`** (G1). `format!("{checked:?}")` prints no secret literal content, and the existing redaction cases
    pass.
14. **Adversarial pass** (X1), recorded under `docs/research/`. Every bypass becomes a fixture plus a test.

## Verify before relying on them

1. **The two F1 panics exist as described.** They are predicted from reading `apply.rs` (`resolve_instance_inputs`
   passes `item: None`; the loop walks `fresh.nodes`) and `plan.rs` (`resolve_step`, `resolve_binding`). The red
   tests settle them. If either does not reproduce, the task records why and keeps the test as a pin.
2. **On a successful `check`, every bound port has an edge and every output a type.** This is read from `check.rs`:
   each `None` from `resolve` without a pushed error (self-reference, a secret input, a broken `for_each`) always has
   a root-cause error elsewhere. G3 asserts it in construction: `check` builds a `PortEdge` for every bound port or
   fails.
3. **Does trybuild's compile-fail mode catch a post-monomorphization `const` error?** `Value::known`'s assertion
   fires at monomorphization, which `cargo check` may not reach. If trybuild misses it, F3 uses a `compile_fail`
   doctest on `Value::known`, which rustdoc compiles fully. The doctest harness is memory-heavy, so run it with
   `RUST_TEST_THREADS=2`.
4. **Same catalog for check and plan/apply** (precondition of G1, then of B2). Run an ast-grep audit of every
   function calling `check(` and `plan(`/`apply(`: list those where the catalog expression differs, or a
   `let catalog = …` re-binds between the calls. The answer at planning time is unknown. Each hit is resolved
   explicitly in G1's commit message, because a test that swapped catalogs would otherwise silently run against the
   check-time tools.
5. **No in-tree tool returns a wrong-typed output** on any existing test. F2's full-crate runs settle it, and every
   hit is a tool bug fixed in its own commit.
6. **No document `for_each`es over an identifier list.** Read at planning: no tracked document, fixture or private
   document declares a `list<…>` input of one of the eight identifier types, and no tool outputs one. B1 rechecks
   with the private `operator_*` tests green.
7. **`Arc<dyn Tool>` held across plans.** The server keeps one `Checked` per document from startup. Tools are already
   shared through the catalog's `Arc`s, so holding a second `Arc` changes nothing about their concurrency.
8. **2b as landed.** Before S1, re-read 2b's addenda for the final shapes of `InputSpec.fixed_by`,
   `Workflow.boundaries`, `Unlinked` and the composition snapshot's path.
9. **3k as landed.** Before each task, check where that crate's integration tests now live.

## Gates

Per task, scoped, never the full workspace gate: the coordinator runs that. Before **each** cargo command, wait for 3
consecutive seconds in which both `pgrep -x cargo` and `pgrep -f cargo-sweep` print nothing, polled every second,
then start the command in the same shell. Use `-j 2` and `RUST_TEST_THREADS=2`. Run fmt, clippy and tests as separate
commands, in the background with a 600,000 ms timeout, and read the output file's body, never piped through `tail` or
`tee`. A linker "missing .rcgu.o" or `E0463` means the host's cargo-sweep ran: `cargo clean -p <crate>` and rebuild.
Never edit tracked files while cargo builds.

```
cargo fmt --all --check
cargo clippy -p <crate> [-p <crate>...] --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test -p <crate> [-p <crate>...] -j 2 --no-fail-fast
cargo check -p willikins-types -j 2        # whenever willikins-types changed
```

**Every task from F1 on also runs `-p willikins-dsl`'s acceptance target** (the characterization), and from 2b's F1
on, 2b's composition snapshot too. Both stay byte-identical. **S1 and B2** change call sites across the workspace.
Their compile gate is `cargo clippy --workspace --all-targets -j 2 -- -D warnings`, and their test gate is the
crates they edited. The coordinator runs the full gate once after each.

Commit as soon as a commit's scoped gates are green, with `git commit --only <paths>`. Local hooks refuse any commit
or message naming the operator's private setup: never `--no-verify`; rewrite with placeholders.

## Tasks

One lane at a time on `main`. F1–F3 may run before 2b; S1 onward runs after 2b's K1. Each task is test first, one
behaviour per commit, green alone, and commits by path with `git commit --only` (never `git add -A`, `commit -a`,
stash, `checkout --` or `reset`), with the implementer's own `Co-Authored-By` trailer. Nobody pushes.

| # | Task | Delegate to |
| --- | --- | --- |
| F1 | **Two apply panics** (decision (f1); acceptance 1, 2; verify item 1). Commit 1: the empty-`for_each` red test (core apply tests, the fake `doppler.config.ensure` as in `plan_adversarial.rs`), then `apply` walks `checked.order` and records an empty group as `ForEach(vec![])`. Commit 2: the item-in-list red test (a core test tool with a `list<Text>` port), then the crate-private walk returns each instance's item and `apply` passes it. Scoped: `-p willikins-core`, then `-p willikins-dsl` acceptance | sonnet implements, opus attacks |
| F2 | **Outputs checked where produced** (decision (f2); acceptance 3; verify item 5). Commit 1, `plan.rs`: `fill_outputs` becomes a checked fill used by `plan_one`, with the message from SHARED VALUES. Commit 2, `apply.rs`: the same after `ensure`. A tool found returning a wrong type gets its own fix commit in its own crate, test first. Scoped: `-p willikins-core`, then `-p willikins-providers-fake` and every provider crate's tests once | sonnet implements, opus attacks |
| F3 | **No self-named value** (decision (f3); acceptance 4; verify item 3). Commit 1, `willikins-types`: `is_type_name` (const fn), its agreement table, `from_static` using it. Commit 2, core: `Value::known`/`known_list`'s `const` assertion and its compile-fail proof, `known_dyn_as` and `ObjectTypeMismatch`, `plan`'s keying by declared element type with the `EdgeTypeMismatch` backstop, and `known_dyn`'s removal (its two test callers move). Scoped: `-p willikins-types` then `cargo check -p willikins-types`; `-p willikins-core -p willikins-server` | sonnet implements |
| S1 | **Sealed** (decisions (s1), (s2); acceptance 5, 8; verify item 8). Commit 1: the accessors and `DocumentView` added while the fields are still `pub`, and every read in the workspace migrated (table in (s1)). Commit 2: the fields made private, the trybuild case (dev-dependency on core), the forged test moved into `check.rs`, and the ratchet test at today's counts. Scoped: workspace clippy; tests `-p willikins-core -p willikins-dsl -p willikins-cli -p willikins-server -p willikins-journal` and the two providers' parity tests | sonnet implements |
| G1 | **Nodes and tool handles** (decisions (g1), (g6), (s3); acceptance 13; verify items 4, 7). One commit: `CheckedNode` with `name`, `declared_at`, `tool`, and `Shape` (ids, `element`, no ports yet), built at the end of `check`; the hand-written `Debug`; `Walk` with `groups`; `plan` and `apply` walk nodes and groups. This deletes `plan.rs`' 696/699 and `apply.rs`' 609/612/823/831 assertions and subsumes F1's first fix. The commit message lists verify item 4's audit result. Scoped: `-p willikins-core`, then dsl acceptance | sonnet implements, opus attacks |
| G2 | **Input table** (decision (g5); acceptance 12). One commit: `CheckedInput`, `InputId`; `check_input_types`, `describe` and the input-resolution path read it. 2b's `NotSettable`/`InputNotSettable` and fixed-input hiding keep their tests green. Scoped: `-p willikins-core`, then `-p willikins-server` describe tests and dsl acceptance | sonnet implements |
| G3 | **Scalar sources** (decisions (g1), (g2); verify item 2). Commit 1: `Source`, `PortEdge`, `Bound::Scalar`, `OutputSlot`, `FilledOutputs`, typed result tables, `CheckedOutput`, the `for_each` source; `check_literal` returns the `Value`; `plan` and `apply` resolve scalar ports, `for_each` sources and outputs from sources. List ports still go through the old path. Commit 2 (only if 3m's R4 landed): `resolve_pure` over sources. Scoped: `-p willikins-core`, then dsl acceptance and the composition snapshot | sonnet implements, opus attacks |
| G4 | **List sources** (decision (g1)). One commit: `Bound::List`; `resolve_list_binding` over sources; the old `Binding` resolver deleted from `plan.rs` once nothing calls it. Scoped: `-p willikins-core`, then dsl acceptance | sonnet implements |
| G5 | **Gates over sources** (decision (g4); acceptance 9). One commit: `collect_causes` and `mark_blocked` over sources, `awaiting_inputs` by `with_position`; `plan.rs` drops its `Binding` import. Scoped: `-p willikins-core`, then `-p willikins-cli` gate tests and dsl acceptance | sonnet implements |
| G6 | **Apply from the graph** (decision (g3); acceptance 10). One commit: every port resolved from the graph with the group's item; attribution returned by the resolver; `binding_may_change_mid_run`, `classify_unknown_binding` and `first_unknown_required_input`'s assertion deleted; `apply.rs` drops `Binding`. Scoped: `-p willikins-core`, then `-p willikins-cli` and `-p willikins-server` apply tests | sonnet implements, opus attacks |
| G7 | **Views, not copies** (decision (s1); acceptance 6, 7, 8). Commit 1: `types()`, `edge()` and `output_types()` computed from the graph; the stored maps deleted. Commit 2: `pub(crate) fn workflow()` deleted; the tripwire extended (acceptance 7); the ratchet set to its final values. Scoped: `-p willikins-core`, then dsl acceptance (the TYPES and OUTPUTS sections are the proof) | sonnet implements |
| B1 | **GATED (coordinator sign-off): identifier `for_each` refused** (decision (b1); acceptance 11; verify item 6). Commit 1, core: the variant, its place in `check_for_each`, the module doc's order, serialization and `Display` tests, and its arm in `crates/willikins-cli/src/render.rs`'s `check_error_detail` (exhaustive, so `willikins-cli` does not compile without it). Commit 2: the fixture with its header comment, its acceptance test, its one characterization entry, and the 3i document guard deleted. Scoped: `-p willikins-core -p willikins-dsl -p willikins-cli` | sonnet implements |
| B2 | **GATED (coordinator sign-off): no catalog in `plan`/`apply`** (decision (b2); verify item 4). One workspace commit, no shim (decision (b2)). Purely mechanical. Any call site that is not a plain argument deletion stops the task and is reported. The same step deletes the argument at the gitignored `operator_*` tests' call sites, locally, never staged (Needs the operator). Scoped: workspace clippy, then each edited crate's tests, then the `operator_*` targets | sonnet implements |
| X1 | **Adversarial pass**, recorded under `docs/research/2026-10-xx-m3n-adversarial-pass.md`, placeholders only. Every bypass becomes a fixture plus a test. At least four mutations restored from saved copies (`cmp` for byte identity). Priority targets: any way to build or edit a `Checked` outside `check`; a source resolving to a value of a type other than its edge's; an `Item` reaching a single node; a keyed reference resolving into the wrong instance; `awaiting_inputs` or `holds_back` reordering; a plan JSON, fingerprint or `NodeStarted.inputs` that moved; a tool output admitted by name rather than `TypeId`; a literal's content in `Debug`; a panic reachable from any document | opus |
| D1 | **Documents** (coordinator). The `check.rs` module doc's "Known gaps" and ordering sections; a CLAUDE.md and AGENTS.md invariant (edited together, byte-identical): "`Checked` is built only by `check`; `plan`, `apply` and `describe` read its typed graph and never a `Binding`"; the design doc's type-system section addendum; the todo marked done; HANDOFF's RESUME HERE and todo table; this plan's addenda and **Completed** header | coordinator |

Fifteen tasks. F1, G1, G3 and G6 are the ones most likely to need a second, independent attack, because they change
how values reach tools.

## Risks

1. **A refactor that drifts.** G3 and G6 rewrite the resolution path every value takes. Several nets catch drift:
   the characterization, every plan's JSON and fingerprint, `NodeStarted.inputs`, `ApplyError` shapes, and 2b's
   composition snapshot. A snapshot update in any G task is a stop, not a review.
2. **Check-time tool handles** (G1). If a caller checked against one catalog and planned against another, its
   behaviour changes at G1, not at B2. Verify item 4 runs first for this reason.
3. **F2 turns green tests red.** A tool that returns, say, `Text` where it declares a domain type was always wrong,
   but fixing it can widen F2. Each fix is its own commit in its own crate, and a provider-crate fix that touches a
   live request is attacked separately.
4. **Two large mechanical diffs** (S1, B2) collide with any parallel lane that edits the same call sites. They run
   alone, and B2 only after the coordinator signs off.
5. **The identifier refusal** (B1) removes something `check` accepted. No known document uses it (verify item 6). A
   future document that wants to iterate identifiers needs a keyed-by-index design, not a relaxed check.
6. **Post-monomorphization errors** (F3) may surface only at `cargo build`/`test`, not `cargo check`. The gates
   build, so the boundary holds. Verify item 3 settles how it is tested.
7. **Ordering against 2b.** If 2b slips, S1 onward waits, while F1–F3 continue. If the coordinator reverses the
   order, see the sequencing section's last paragraph.
8. **Privacy.** Nothing in this milestone needs a real name. The private `operator_*` tests are run, never edited,
   except in B1's recheck, which is read-only, and in B2, which deletes the catalog argument at their `plan`/`apply`
   call sites locally and never commits them (Needs the operator).
9. **Host contention.** Other sessions run cargo almost continuously. If no 3-second quiet window comes within 40
   minutes, the task stops and reports the contention.

## Needs the coordinator

1. **Sign off B1** (an identifier-typed `for_each` source becomes a `check` error, plus one new characterization
   entry). If declined, B1 is dropped and 3i's guards stay.
2. **Sign off B2** (`plan`/`apply` lose `&Catalog`, about 317 call sites). If declined, the parameter stays,
   documented as registry-only.
3. **Order:** 2b first (recommended), or 3n's S1 before 2b's S1/K1.

## Needs the operator

1. **B2 edits the gitignored `operator_*` tests** (portfolio review): they call `plan(&checked, &inputs, &catalog)`
   16 times and `apply(` 8 times, so B2 leaves them uncompilable unless the catalog argument is deleted there too.
   The plan's answer: the B2 implementer makes that one mechanical deletion locally, uncommitted, in the same step.
   The operator confirms that, or makes the edit themselves. Only needed if B2 is signed off.
