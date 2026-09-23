---
title: Turn Checked into a typed graph that plan, apply and describe consume
created: 2026-09-23
status: pending
priority: medium
area: core/check
related:
  - docs/plans/2026-09-23-milestone-3d-conversions.md
  - docs/plans/2026-09-11-willikins-design.md
  - crates/willikins-core/src/check.rs
  - crates/willikins-core/src/plan.rs
  - crates/willikins-core/src/apply.rs
  - crates/willikins-core/src/describe.rs
  - todos/2026-09-22-secrecy-inference.md
---

# `Checked` as a typed graph

The operator's principle, 2026-09-23: *parse, don't validate*. Their words: *"there's a world
between [making workflows Rust code] and making an actual parser that doesn't rely on post-parsing
checks."* `check` is that parser today, but what it returns is mostly the *input* (`Checked::workflow`,
the document unchanged) plus a few side tables. So `plan`, `apply` and `describe` walk the raw
document again, and every place they rely on something `check` established is an `unreachable!`
that re-asserts it. This todo is the milestone that ends that. `Checked` becomes a typed graph, so
the consumers read parsed structure and **cannot represent** the states `check` has already ruled
out.

## Measured facts (planner, 2026-09-23, tree at `8f85739`, recounted rather than copied)

- **All of `Checked`'s six fields are `pub`:** `workflow`, `order`, `class`, `warnings`, `types`,
  `output_types` (`crates/willikins-core/src/check.rs:86-106`). Anyone *can* build one with a struct
  literal. Today nothing outside `check` does (`check.rs:586` is the only struct literal). The
  milestone-3d brief said `describe.rs:409` builds one; it does not, because that test helper calls
  `check(…).expect(…)`. The hazard is the public constructor, not a current abuse.
- **Non-test `unreachable!` sites that re-assert what `check` established** (counted above each
  file's first `#[cfg(test)]`):
  - `plan.rs`: **16** `unreachable!`, 0 `expect`. They cover: `checked.order` naming only workflow
    nodes; a compatible catalog, asserted twice; a literal never on an `AnySecret` port; a literal
    already validated; `Literal` never reaching `resolve_binding`; `item` only inside `for_each`;
    every dependency planned first, asserted twice; an output port existing, asserted three times; a
    referenced node existing; every instance filling every output; `Keyed` only against a `for_each`
    node; and `Foreign`/`Mismatch` handled above.
  - `apply.rs`: **7** `unreachable!`, 0 `expect`. Two of them (lines 950 and 975) are internal
    arithmetic facts of `check_drift`, not facts `check` established. The other five are: node names,
    the catalog, one instance per non-`for_each` node, a key per `for_each` instance, and "a literal,
    an item, or an unbound port cannot be Unknown".
  - `describe.rs`: **1** `unreachable!` (line 301, "an unregistered input type was already
    rejected"). The brief's figure of 5 counted its test module's `.expect(` calls.
  - For comparison, `check.rs` itself has 7 `unreachable!` and 2 `unwrap` outside tests. Those are
    its own internal invariants and are not in scope.
- **Direct field reads outside `willikins-core`:** only `workflow` and `warnings`.
  `willikins-cli/src/main.rs:241` (`workflow.inputs`), `:261`/`:263` (`warnings`),
  `willikins-cli/src/commands.rs:620` (`workflow.name`); `willikins-server/src/butler.rs:307`
  (`warnings`), `:1195` (`workflow.inputs`); `willikins-server/src/startup.rs:143` stores a
  `pub checked: Checked`. Inside core, `plan.rs` reads `order`, `workflow` and `class`, `apply.rs`
  reads `class` and `workflow`, and `describe.rs` reads `workflow`. `types` and `output_types` are
  read only by tests (about 20 sites across `willikins-core`, `willikins-cli`,
  `willikins-providers-doppler` and `willikins-providers-github`).

## The shape to aim for

A `Checked` whose fields are private, whose only constructor is `check`, and whose nodes, in
topological order, each carry: the resolved tool (spec plus handle, so no second catalog lookup), and
for every bound input port an **edge**. An edge holds its source, already classified: a parsed
literal `Value`, a workflow input, the `for_each` item, a `Step` to a node index plus port, or a
`Keyed` to a node index plus key plus port. It also holds its resolved type and the conversion
`check` chose, if any. `for_each` sources and workflow outputs are edges of their own kinds. `plan`
walks nodes and edges; it never matches on a `Binding`, never looks a node up by name, and never
re-parses a literal. Each `unreachable!` listed above should disappear because its case is
**unrepresentable**, not because it moved somewhere else.

Keep the accessor surface the CLI and server need (`workflow()` for the document's name and
inputs, `warnings()`), so their change is mechanical.

## Milestone 3d's edge record is the first brick

Milestone 3d (`docs/plans/2026-09-23-milestone-3d-conversions.md`, decision (e)) turns
`Checked::types` from `IndexMap<PortName, TypeRef>` into `IndexMap<PortName, Edge>`. `Edge` has
private fields (`ty`, `conversion`) and a `check`-only (`pub(crate)`) constructor. It is the first
part of `Checked` that `plan` and `apply` **read** rather than re-derive from the document: they
deliver every resolved port value through the edge's conversion, and they never consult the
conversion table themselves. It is also the first part that cannot be forged outside
`willikins-core`. This todo grows that record. It adds the classified source (which removes the
`Binding` match and the literal re-parse from `plan`) and indexes it by node rather than by name,
until `Checked::workflow` is needed only for its name and inputs.

## Constraints on the eventual plan

- Every existing negative fixture's exact errors, every document's `Plan::fingerprint` and plan
  JSON, and the journal wire format must be unchanged. The characterization snapshot milestone 3d
  adds to `crates/willikins-dsl/tests/acceptance.rs` is the ready-made proof: it must not move.
- `check`'s error accumulation order and cascade suppression stay exactly as documented in
  `check.rs`'s module doc.
- Secrecy inference (`todos/2026-09-22-secrecy-inference.md`) also wants per-edge structure (one
  constraint per use site). Sequence the two deliberately: whichever lands second builds on the
  other's edge type rather than inventing its own.
