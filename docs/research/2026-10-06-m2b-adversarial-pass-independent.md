# Milestone 2b: independent adversarial pass on composition

**Date:** 2026-10-06
**Run as:** Opus 5.5, the independent opus pass the plan's task X1 intended. The first pass
(`2026-10-06-m2b-adversarial-pass.md`, Sonnet, commits `0b04b42`, `4f89af0`, `39841a0`) is not
repeated here. Its two fixes (a path-shaped binding target, the resolver cache) and its four
mutations still hold.
**Scope:** `docs/plans/2026-10-05-milestone-2b-composition.md` as landed through X1: P1, P2, L1, L2,
C1, R1, J1, S1, S2, S3, K1 and F1. Every claim below was read off the code, then either fixed test
first or proven by a mutation. No live provider was called. Placeholders only.

## Method

Each mutation: copy the file to the scratchpad, edit it in place, wait for a quiet host
(`pgrep -x cargo` and `pgrep -f cargo-sweep` both empty for three consecutive seconds), run the
narrowest test that should catch it, read the log body, restore from the copy, touch the file,
then confirm byte identity with `cmp` and `git diff --quiet`. A defect found was fixed test first:
the test was run red on the original code, then green on the fix, and the fix committed alone.

## Findings, fixed (four commits)

### 1. A used document's undeclared input reference panicked the linker (acceptance 4)

**Fixed in `fb42e56`.** `compose::embed` substitutes a child's `${{ inputs.<x> }}` from the
parent's substitution map. That map only has entries for the child's *declared* inputs.
`willikins-dsl` does not refuse a reference to an undeclared input. That refusal belongs to
`check`'s `UndeclaredInput`, and in CLI file mode a sibling document is never checked on its own.
So a child whose node binds `${{ inputs.typo }}` reached `embed`'s `unreachable!` and panicked.
That happened from `describe`/`plan <file>`, from the server's startup scan when the composite is
linked before its child, and from `validate`. Acceptance 4 says no refusal panics.

The fix prefixes the reference like any other input path (`child/typo`). A fixed input
`<step>/<x>` exists only for a *declared* `x`, so the prefixed name matches no input of the flat
workflow. `check` then reports `UndeclaredInput` at `child/boom.key`, the same refusal a monolith
gets for the same typo. Pinned by the compose unit test
`a_childs_reference_to_an_undeclared_input_links_to_a_prefixed_path` and by the fixture pair
`workflows/fixtures/composition/undeclared-input-{root,child}.yaml` with the dsl test
`a_childs_undeclared_input_reference_is_check_s_undeclared_input`. Red before the fix: `entered
unreachable code: ensure_local_subst guarantees a subst entry for every non-path input of step
`c`; `typo` has none`.

### 2. The expansion bound counted tool nodes only (trust boundary 6)

**Fixed in `6058036`.** `flatten` added `wf.nodes.len()` to the running count. A document with no
tool node of its own adds nothing, for example one made only of inputs, pass-through outputs and
`uses:` steps. A tree of such documents was never bounded: eight levels of eight `uses:` steps over
a zero-node leaf is `8^8` `flatten` calls. Each call materialises its own inputs and boundaries,
and no refusal fires. X1's resolver cache removed the disk reads but not this work.

Every `uses:` step now counts once per occurrence, like a tool node. The same tree is refused as
`UsesTooLarge` within `MAX_LINKED_NODES` resolves. Pinned by
`a_fan_out_of_zero_node_documents_is_refused_as_uses_too_large`. Red before the fix: `expansion
bomb: resolve called 8193 times with no bound tripping`. The public diamond fixture still tips at
exactly 2049, now made of 9 + 6 × 333 + 9 + 9 + 8 × 3 counted steps, so the composition
characterization is unchanged. The meaning of `MAX_LINKED_NODES` widens from "tool nodes" to "tool
nodes and `uses:` steps" (plan addendum). `CheckError::UsesTooLarge.nodes`'s doc comment is left
alone, because it is published by schema snapshots.

Residual, recorded but not fixed: the work per occurrence is still proportional to the used
document's own size. That is at most `MAX_LINKED_NODES` occurrences of at most `MAX_DOCUMENT_BYTES`
each. It is linear and bounded, no longer exponential.

### 3. The alias-cycle guard refused a legitimate mutual feed (decision (d7))

**Fixed in `df89f86`.** Decision (d7): "Two `uses:` steps may legitimately feed each other's inputs
when the node-level graph has no cycle, and `check`'s cycle detection judges that on the flat
graph." The guard was per *step*. Resolving any output of a step whose substitution map was still
being built was refused as `UsesOutputCycle`, even when that output came from a node reading none
of the step's inputs.

In the counterexample, `a.x ← steps.b.out_m` and `b.x ← steps.a.out_m`, where `out_m` is a node
`m` that reads only a defaulted `seed`. The flat graph is `a/m → b/n`, `b/m → a/n`, which is
acyclic, and the same four nodes written as a monolith check clean. The linker refused it, so
`check` disagreed with the monolith.

Each `with:` is now validated as a whole first (`validate_uses_step`). Substitutions are then
resolved one `(step, input)` at a time, and an output reference resolves only the inputs its own
binding reads. A loop made only of pass-throughs is still `UsesOutputCycle`. A node-level loop
through a step's own output is `check`'s `Cycle`, as in a monolith. Pinned by
`two_uses_steps_feeding_each_other_without_a_node_cycle_link_and_check`, which links and then
checks the flat graph. Red before the fix: `[UsesOutputCycle { node: "a", output: "out_m" }]`.
Every existing L2 refusal test stayed green unchanged, including the sited `UnknownUsesOutput` and
`KeyedOnUses`.

### 4. A long chain of `uses:` steps overflowed the stack and aborted the process

**Fixed in `da38af7`.** Substitution resolution recursed once per link of a chain of `uses:` steps
feeding each other. A document can declare such a chain in reverse dependency order (`s0` reads
`s1`'s output, `s1` reads `s2`'s, and so on). 2000 steps of a zero-node child stay under the
bound, so nothing refuses them. On a thread with a 2 MiB stack the linker overflowed it, and a
stack overflow aborts the whole process (`SIGABRT`). It is not a panic a handler can catch.

The server runs every MCP call on a `spawn_blocking` thread. Nothing in `willikins-server` sets
`thread_stack_size`, so that thread has tokio's default 2 MiB stack. `validate` and `describe`
accept a caller-supplied body. A long enough chain in one request could therefore take the server
down. The body only needs to chain any trusted document that declares an output reading one of its
inputs. Example: a public organisation document's pass-through `github_org`. A child with one tool
node allows about 1000 links before the bound; a zero-node child allows about 2000.

The probe was a debug build. Release frames are smaller, so the release threshold is higher but
unknown. The fix makes depth
independent of the build profile. `substitution_order` computes the resolution order with an
explicit stack. Each substitution is then resolved after everything it reads, so `ensure_input`
recurses one level deep. The same walk refuses a pass-through ring of any length as
`UsesOutputCycle` without recursing around it. Pinned by
`a_long_reverse_chain_of_uses_steps_links_on_a_two_mib_stack` and
`a_long_ring_of_pass_through_uses_steps_is_refused_on_a_two_mib_stack`, both run on a 2 MiB
thread. Red before the fix: the same chain as a temporary probe, never committed, aborted the test
binary with `thread '<unknown>' has overflowed its stack` / `signal: 6, SIGABRT`.

## Mutations, each restored byte-identical (`cmp` and `git diff --quiet`, confirmed)

These target defences the first pass did not mutate.

| # | File | Mutation | Test run | Result |
| --- | --- | --- | --- | --- |
| M1 | `willikins-server/src/butler.rs` | `reload_and_check`'s `resolver.shas() != &record.used` → `false && …` | `--test it composition_s2::` | **RED**, 3 of 7: a child edited, a child swapped under the same root bytes, a grandchild edited. Each applied with no `DocumentChanged`. `the_root_edited_to_add_a_second_child…` stays green, because the root sha check catches that case on its own |
| M2 | `willikins-core/src/check.rs` | `check_boundaries`'s `check_input_spec(…)` → `false && …` | dsl `composition::a_secret_typed_child_input…` | **RED**: `check` passes, with a `DopplerSecretValue` step output bound into the child's secret-typed input. A secret enters a used document (trust boundary 3) |
| M3 | `willikins-server/src/document.rs` | `workflow_path` returns a symlinked candidate as `Ok` | `--lib document::tests::` | **RED**, 2: `expected Symlink, got Ok`, and the resolver resolves a symlinked child (trust boundary 1) |
| M4 | `willikins-core/src/compose.rs` | the `depth >= MAX_USES_DEPTH` refusal → `false && …` | `compose::tests::a_nine_deep…` | **RED**: the nine-deep chain is no longer `UsesTooDeep`. The walk runs past depth 8 until the resolver runs out (`UnknownWorkflow { workflow: w9 }`) |

Each fix above also has its own red run, recorded with the finding.

## Priority targets, held (verified by reading, plus the mutations above)

- **A secret entering a used document through a binding or a default.** A bound boundary runs the
  five signature rules over the child's declared spec (M2 proves the call is load-bearing). An
  unbound one becomes a fixed input that `check_workflow_inputs` checks. A scalar reference must
  match the declared type exactly. A secret *default* cannot exist, because a secret declared type
  is refused before its default is looked at.
- **The `Binding::List` boundary gap** (the C1 addendum's accepted narrowing), re-attacked.
  Secrecy holds. A secret list element is refused at every place a list can be consumed:
  `check_list_element` refuses any secret element whatever the port's element type, and a list
  that reaches a `for_each` source or a root output is `SequenceNotAllowedHere`. Two smaller
  divergences remain. First, a list element that only *converts* to the declared element type is
  accepted when the child consumes the list at a port, where (d4) says the boundary is exact.
  Second, binding a list literal to a child input the child iterates over (`for_each: ${{
  inputs.envs }}`) is refused as `SequenceNotAllowedHere` at the child's node. A monolith declares
  that list as a defaulted input instead, and the plan's own documents leave such inputs defaulted
  (fixed), which works. Neither is a secrecy bypass. Both are recorded for the plan's owner.
- **A used document found by anything but its name in the trusted directory.** The server's
  `TrustedResolver` and the CLI's `sibling_file` both look up `<name>.yaml`, then `<name>.yml`, in
  the parent's directory only. They refuse a symlink (M3) and require the internal `name:` to
  match. A `WorkflowName` holds no `/` or `.`, so it cannot spell a path. Every server path, body
  or name, links through `TrustedResolver::new(&self.workflows_dir)`. A body never supplies a
  child.
- **A child changed between plan and apply.** The closure comparison is load-bearing (M1). Every
  `reload_and_check` failure is `DocumentChanged`. Residual, recorded:
  `document::load_named_document` parses the file from one read and hashes a *second* read. It
  predates 2b for the root and now also applies to every child. A file replaced between the two
  reads would record a sha that is not the sha of the content the plan was built from, and an
  apply against the replacement would then match that sha. The window is two consecutive reads,
  inside the trusted directory, so it needs a writer the trust model already trusts. The remedy is
  to hash the bytes that were parsed, which needs a `willikins-dsl` entry point that returns them.
- **A node name collision between parent and used document.** An authored name is one segment.
  The DSL refuses a `/`, and `PathInAuthoredName` refuses one in a hand-built workflow. Every
  linked node and every fixed input is at least two segments, under a parent step name that is
  unique. Unchanged by this pass.
- **Plan, journal and approval identity.** A node path is carried as-is. `Plan::fingerprint` is a
  structured list (name, instance, action, outputs), never a joined string, so `a/b` cannot alias
  another split. No provider crate references `NodeName`: a tool sees only its `Inputs`, so the
  fake and the live tools see identical inputs whatever a node is renamed to. The approvals page
  reloads only the root's own description and authored inputs, which is the composite's own
  surface. Each used document is listed by escaped name and short sha (S3).
- **The existing characterization.** `it__acceptance__characterization_of_every_document.snap`
  last changed in `7f3c66e`, an ancestor of P1 (`b8ae770`). No tracked `workflows/*.yaml` changed
  during 2b. The composition characterization changed only by the two fixtures this pass added
  (additions only).

## Not verifiable by this pass

- **Acceptance 13 (O1, the private split's equivalence).** The gitignored `private/workflows/`
  holds only the monolith. No organisation document, no baseline copy and no split test exists
  yet, so the claim "the split plan equals the monolith's apart from node names" cannot be checked.
  O2 (live) follows O1 and is the coordinator's.

## Hand-built only, recorded (no document can express these)

- A `Workflow` whose `uses:` positions are duplicated or out of range panics in Phase 3c's walk
  (`own_nodes.next()`'s `unreachable!`). The DSL always assigns `0..steps.len()`.
- A `uses:` step's `with:` key that names a *fixed* input path (`deeper/x`) passes the
  `UnknownUsesInput` check and is then silently ignored. The fixed input keeps its default, so
  nothing is overridden. The DSL refuses a `/` in a `with:` key.
- `PathInAuthoredName` scans node, `uses:` and input keys and `Step`/`Keyed` targets, but not
  `Binding::Input` names. A path-shaped input reference can only reach a fixed input (whose value
  is fixed anyway) or end as `UndeclaredInput`.

## Gates run (scoped; the full workspace gate is the coordinator's)

- `cargo fmt --all --check`: clean.
- `cargo clippy -p willikins-core [-p willikins-dsl] --all-targets -j 2 -- -D warnings`: clean
  after each commit.
- `cargo test -p willikins-core -j 2 --no-fail-fast`: 237 unit, 292 `tests/it`, 2 doctests, 0
  failed.
- `cargo test -p willikins-dsl -j 2 --test it`: 60 passed, the main characterization byte-identical.
- `cargo test -p willikins-server -j 2 --no-fail-fast`, after the last linker commit: 96 unit, 229
  `tests/it` (2 ignored), 0 failed.
- `cargo test -p willikins-cli -j 2 --test it composition`: 11 passed (`composition::` and
  `composition_equivalence::`).
