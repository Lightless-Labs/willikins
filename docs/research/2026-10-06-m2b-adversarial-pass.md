# Milestone 2b adversarial pass (X1)

**Date:** 2026-10-06. **Scope:** composition (`docs/plans/2026-10-05-milestone-2b-composition.md`),
parts P, L, C, R, J, S, K, F as landed through task F1. Placeholders only; nothing here names the
operator's real organisation, accounts or documents.

**Delegate note.** The plan's Tasks table marks X1 "Delegate to: opus." This session has no way to
spawn a separate model for a sub-task, so the pass below was run directly by the implementing
session (Sonnet), reading every landed file before attacking it rather than trusting its own
earlier work. The coordinator may want a second, independent attack before treating this pass as
final; nothing here was skipped or stubbed in anticipation of one.

## Method

Priority targets came from the task row and ultimately from trust boundaries 1-6 and decisions
(d3), (d9), (d10). Each was attacked by reading the actual landed code first (`compose.rs`,
`check.rs`, the DSL's `reference.rs`, `document.rs`, `butler.rs`, `site.rs`), then either (a)
writing a fixture/test that demonstrates a real bypass, landing a fix, and proving the fix with a
mutation that restores the bypass, or (b) tracing the code to the point that settles the question
and recording why it holds, without manufacturing a test for something that was never broken.

Two real bypasses were found and fixed. Both are defects in the *linker itself*
(`willikins-core::compose`) or its *server-side caller* (`willikins-server::document`), not in any
one fixture — so neither needed a new fixture under `workflows/fixtures/composition/`: a
hand-built `Workflow` (finding 1) is inexpressible through the DSL by construction, and a
resolver's own call-count behaviour (finding 2) isn't a document property the DSL could vary. Both
are pinned by unit tests in the module they fixed. A third item closes an explicit coverage gap the
task row names, without being a bypass (the enforcement was already correct; only the end-to-end
test was missing).

## Findings: bypasses, fixed

### 1. `PathInAuthoredName` only scanned keys, not binding targets (trust boundary 4)

**File:** `crates/willikins-core/src/compose.rs`. **Fixed in:** `0b04b42`.

The linker's defence against a hand-built `Workflow` smuggling a path-shaped name in
(`CheckError::PathInAuthoredName`, decision (d3)) scanned `wf.nodes.keys()`, `wf.uses.keys()`, and
`wf.inputs.keys()` for a `/` — but never the *targets* a `Binding::Step`/`Binding::Keyed` names.
`NodeName`'s type itself accepts a `/`-path (that's the whole point of the widening), so nothing
stopped a binding from naming `org/gh_token` directly, where `org` is a real `uses:` step and
`gh_token` is an internal node of whatever document `org` resolves to — never declared as that
document's output.

`rewrite_at_level`'s own dispatch is `if wf.uses.contains_key(node) { ... } else { Ok(binding.clone()) }`.
A multi-segment name like `org/gh_token` is not a key of `wf.uses` (whose keys are always one
segment — the DSL refuses a `/` there), so it fell straight into the `else` arm: passed through
untouched, with no refusal and no rewriting. After the rest of the tree finished linking, if the
child named `org` happened to have an internal node literally called `gh_token`, the final flat
graph *would actually contain a node at that path* (linking renames a child's own nodes to
`<step>/<child node>`), and the reference would resolve to it — a parent reaching directly into a
child's internal node, which is exactly what trust boundary 4 ("the reference grammar cannot name a
node inside a child") exists to prevent.

The DSL itself can never author this: `willikins-dsl/src/reference.rs`'s reference grammar accepts
no `/` in a node segment (already proven by that file's own tests,
`a_slash_in_a_step_reference_node_is_rejected` and siblings — see "held" section below). The attack
surface is a hand-built `Workflow`, built with plain `pub` field construction — which `workflow.rs`'s
own module doc explicitly documents as a supported way to build one, independent of the DSL. So
this is real: any future internal code (or test harness) that constructs a `Workflow` by hand,
believing `PathInAuthoredName` already protects every path a document could carry, would be wrong.

**Fix:** a new `authored_binding_targets` walks every `Step`/`Keyed` target in `wf`'s own nodes'
`with`/`for_each`, `wf`'s own outputs, and every `uses:` step's own `with:`, and feeds those into
the same scan `PathInAuthoredName` already ran over keys. Pinned by two new tests:
`a_slash_in_an_authored_bindings_target_is_refused` (the output-binding case sketched above) and
`a_slash_in_a_uses_steps_with_binding_target_is_refused` (the same shape inside a sibling `uses:`
step's own `with:`). Both assert `resolve` is never called — the refusal fires before any child is
even looked up, exactly like the existing key-based `PathInAuthoredName` tests.

### 2. `TrustedResolver` re-read and re-parsed on every occurrence (trust boundary 6, decision (d9))

**File:** `crates/willikins-server/src/document.rs`. **Fixed in:** `4f89af0`.

`willikins_core::compose::link` calls its `resolve` callback once per *occurrence* of a `uses:`
step, by design — not once per distinct name. This is correct and necessary for the node-count
bound to mean what it says (a diamond's second occurrence of the same child must count again,
acceptance 4's `an_exponential_diamond_is_refused_before_full_expansion` test already pins this).
But `TrustedResolver::resolve` (the server's own `resolve` implementation, used by every `plan`,
`apply`, `validate`, `describe`, and `scan_directory` call) did a full `std::fs::read` plus a full
YAML parse on *every* call, with no memoization.

`MAX_LINKED_NODES` (2048) bounds the final flat graph's *tool node* count, counted the moment each
document's own node count is added to the running total — but it says nothing about how many times
`resolve` itself is called, when the documents being referenced are node-sparse. A caller-controlled
root (`validate`'s request body, reachable over the network to any authenticated principal) with
many `uses:` steps, all naming the same real, trusted document that happens to declare few tool
nodes, forces one full re-read and re-parse of that document per occurrence — work bounded only by
how many `uses:` steps a 256 KiB body (`willikins_dsl::MAX_DOCUMENT_BYTES`) can hold, which for a
small `with: {}` step is in the thousands. If the referenced trusted document is itself close to the
256 KiB cap (plausible for a document with many declared inputs/outputs but one or two tool nodes),
the result is tens to hundreds of megabytes of repeated disk I/O and YAML parsing inside one
`validate` call, well before any bound ever has reason to refuse — an expansion bomb that does real
work before refusal, the exact shape the task names as a priority target.

**Fix:** `TrustedResolver` now caches a successful `(DocumentSha256, Workflow)` by name (a
`BTreeMap`, since `WorkflowName` has no `Hash` — the domain-type macro never derives `Hash` for any
type, secret or not, so a `HashMap` key was never an option here regardless). The cache is sound
specifically *because* `link` calls `resolve` once per occurrence for counting purposes: one
resolver, one `link` call, is one synchronous snapshot of the trusted directory, so every occurrence
of the same name within it must see the same bytes regardless of whether they're re-read — caching
is strictly more consistent, not merely faster, than re-reading mid-walk (it closes a tiny,
pre-existing TOCTOU window of its own: without it, a file edited *during* a single `link` call could
be read in two different states by two different occurrences). A failed resolve is never cached,
because `link` is first-error-wins and aborts the entire call through `?` the instant one occurs, so
a failed name is never asked for a second time inside one call. `used`/`shas` (decision (d10)'s own
plan-identity bookkeeping) are unaffected: `link` still calls `resolve`, and therefore `flatten`,
once per occurrence; only the disk read and the parse underneath it are shared.

Pinned by two new tests in `document.rs`'s own module: `a_repeated_name_is_read_from_disk_once` (50
occurrences of one name read the file exactly once, via a new `reads()` counter) and
`two_different_names_are_each_read_once` (the cache is per name, not global).

## Findings: an explicit coverage gap closed (not a bypass)

### 3. No end-to-end CLI test for "a caller setting a fixed input"

**File:** `crates/willikins-cli/tests/it/composition.rs`. **Landed in:** `39841a0`.

The task row names, as a priority target, "a caller setting a fixed input through the CLI, MCP
`plan` or `describe`." Reading the code: `willikins_core::describe` already refuses unconditionally
(`InputError::not_settable`, task R1), and `Butler::describe_inner`/`Butler::plan_inner`
(`crates/willikins-server/src/butler.rs`) both call that exact function — the same one the CLI's
`cmd_describe`/`cmd_plan` call — so the MCP surface is covered by construction, not by a separate
code path that could drift from it. What was missing was an end-to-end proof through the real CLI
binary: `describe_links_the_sibling_and_hides_its_fixed_inputs` proved a fixed input is *hidden*,
but nothing proved a caller-supplied value for one is *refused*. Added
`describe_refuses_a_callers_value_for_a_fixed_input` and
`plan_refuses_a_callers_value_for_a_fixed_input`, both against the already-public
`example-org.yaml`/`new-rust-service-in-org.yaml` pair, asserting exit code 1 and the exact
`NotSettable` discriminant on the named input. No fix was needed; the gap was in coverage, not
enforcement.

## Findings: held, verified by reading the code (no new test needed)

- **Reference grammar reaching inside a child (trust boundary 4), via the DSL.** The three
  reference regexes in `crates/willikins-dsl/src/reference.rs` (`INPUT_RE`, `STEP_RE`, `KEYED_RE`)
  each capture a node/input segment as `[a-z][a-z0-9_]*` — no `/` — so `${{ steps.a/b.p }}`,
  `${{ steps.a/b[k].p }}`, and `${{ inputs.a/b }}` all fail to parse as any of the three forms and
  are refused as "not a valid reference," already pinned by that file's own tests
  (`a_slash_in_a_step_reference_node_is_rejected` and two siblings, written when the `NodeName`
  pattern itself was widened). This closes the DSL-authored half of finding 1 above; only a
  hand-built `Workflow` could still reach it, which the fix above now also refuses.
- **A body supplying or shadowing a child (trust boundary 2).** Traced `Butler::validate_inner`,
  `Butler::describe`, and `Butler::plan_inner`: each parses a caller's `DocumentSource::Body`
  purely in memory (`willikins_dsl::parse_document`, in `Butler::load_source`) and never writes it
  to `self.workflows_dir`. Every `uses:` child, for every one of these calls, resolves through a
  freshly-built `TrustedResolver::new(&self.workflows_dir)` — the real trusted directory, never the
  body. There is no code path by which a request body could shadow, or be mistaken for, a trusted
  document.
- **A child resolved outside the trusted directory.** Symlink: refused, not followed
  (`document.rs`'s `workflow_path` uses `symlink_metadata`, never `metadata`, specifically so a
  symlink is told apart from a followed path; pinned by
  `a_symlinked_document_is_refused_not_followed` and the resolver-level
  `trusted_resolver_refuses_a_symlinked_child_without_recording_it`). `..`: inexpressible — `uses:`
  takes a `WorkflowName` (kebab-case, no `/`, no `.`), parsed as a typed value by the DSL before any
  filesystem lookup happens, never a raw path string, so there is no string for a `..` segment to
  live in. A name that is not a `WorkflowName`: refused at parse time by the same typed-value rule,
  before `link` or `resolve` is ever reached. `.yml` beside `.yaml`: `workflow_path` tries `.yaml`
  first, deterministically, already exercised by `falls_back_to_a_yml_file` (the `.yml`-only case)
  and `composition_s2.rs`'s own `.yaml`→`.yml` swap test (the `DocumentChanged` case); a directory
  holding *both* names at once is not attacker-reachable to begin with, since writing into the
  trusted directory requires the access the trust boundary already assumes.
- **`${{ item }}`/`Keyed` rebinding inside a child.** `ItemInUses` and `KeyedOnUses` are both
  pinned by existing tests (`item_bound_to_a_uses_input_is_refused_as_item_in_uses`,
  `a_keyed_reference_onto_a_uses_step_is_refused`, plus the "sited from inside another step's
  `with:`" pair). Mutation 3 below independently demonstrates what disabling the `ItemInUses` half
  actually does: the item silently reaches the boundary as `Binding::Item`, exactly the silent
  rebind decision (d4) warns about — proof the existing test is load-bearing, not merely present.
- **A child changed between plan and apply without `DocumentChanged`.** Read
  `Butler::reload_and_check` directly: it treats *every* failure mode as one undifferentiated
  `Err(())` — a reload that fails to load, a relink that fails (including a child that became
  unresolvable, e.g. retargeted to a symlink, between plan and apply), a `used`-closure mismatch, or
  a `check` failure on the freshly linked graph — and every one of those becomes
  `ButlerError::DocumentChanged` in `Butler::apply`'s caller. There is no path through this function
  that reaches the approval/fingerprint logic below it without first proving the closure is
  unchanged.
- **A name collision between a linked path and an authored name** (e.g. a hand-built `wf.nodes`
  and `wf.uses` sharing the literal key `"org"`). Traced `flatten`'s Phase 3c (the node-insertion
  walk): a `uses:` step's children are *always* inserted under their prefixed name
  (`<step>/<child node>`); a plain authored node is *always* inserted under its own unprefixed name.
  The two insertion paths can never produce the same final key for this scenario — an authored node
  literally named `"org"` and a `uses:` step named `"org"` end up as `nodes["org"]` (the authored
  one) and `nodes["org/<child's own nodes>"]` (the step's children), which never collide. Harmless;
  no fixture needed, since the DSL can't express it anyway (one `steps:` map, one slot per key) and
  the hand-built case does not corrupt the result.
- **A `Site` display collision.** `site.rs`'s own module doc states the proof directly: `/` is
  neither `.` nor `[`, so a path-valued `node` renders as part of `Site::Port`/`Site::ForEach`
  exactly as a one-segment name would, and still cannot coincide with `Site::Output`'s
  `workflow.outputs.<name>` form (`no_two_site_forms_share_a_display_string`). Checked separately:
  the `"{node}[{key}]"` rendering used for a `for_each` *instance* (in `apply.rs`, `render.rs`, and
  the server's `error.rs`) takes an unrestricted instance-key string, which could already contain
  `[`/`]` characters before milestone 2b — that ambiguity, if it is one, is pre-existing and
  orthogonal to the node-path widening, because `NodeName` itself still excludes `[` and `]`
  entirely; composition does not make it any easier to construct than it already was.
- **Class or approval lowered by composition.** `check.rs`'s class computation
  (`Class::max_of(workflow.nodes.keys()...)`) reads straight off `workflow.nodes` — the *flat*,
  post-linking node map `check` always operates on — with no special-casing by name shape. A node
  contributed by a used document is indistinguishable, to this computation, from one the root
  authored directly, so a composite's class is the true maximum over every non-pure node in the
  whole flattened tree, matching decision (d11) and acceptance 6's own pinned test
  (`uses-class-root.yaml`/`uses-class-child.yaml`, one `Irreversible` node inside the child).
- **List-boundary secrecy smuggling** (the plan's own flagged, accepted gap:
  `check_boundaries` skips `Binding::List` entirely at the exact-type check). Reasoned rather than
  fixture-tested: secrecy in this codebase is a property of the Rust *type* itself, never a
  per-value flag independent of type (`IS_SECRET` is a `DomainType` constant), and no conversion row
  exists, or is ever registered, from a secret type to a non-secret one or back (`conversions!`'s
  own compile-time assertion). A list element bound into a declared `list<T>` (non-secret `T`,
  since a secret-typed declared input is already refused by `check_input_spec` regardless of binding
  shape) can therefore never *be* a secret value of exactly type `T` — if the child actually consumes
  the list anywhere, the existing per-element type check at that consumption site still catches a
  real mismatch; if it never consumes it, the value is inert. The skip changes *where* a mismatch is
  reported, not *whether* a secret can cross. No new fixture, consistent with the plan's own
  narrowing addendum, which already names this exact case as out of scope for an acceptance test.

## Mutations (restored from saved copies; `cmp` confirms byte identity)

Each mutation: `cp` the clean, already-committed file to the scratchpad, edit in place, run the
narrowest test that should catch it, confirm red, restore from the saved copy, `cmp` the restored
file against the saved copy, and `git diff --quiet` against the committed tree.

| # | File | Mutation | Test run | Result |
| --- | --- | --- | --- | --- |
| 1 | `crates/willikins-core/src/compose.rs` | `authored_binding_targets(wf)` removed from the `PathInAuthoredName` scan's `.chain(...)` | `compose::tests::a_slash_in_a*` (2 tests) | **RED**: both panic — `resolve` is called (for `child`/`leafchild`) when it must not be, because the binding-target scan no longer catches the path before `flatten` reaches resolution |
| 2 | `crates/willikins-server/src/document.rs` | Cache lookup in `TrustedResolver::resolve` wrapped in `if false { ... }` | `document::tests::a_repeated_name_is_read_from_disk_once` | **RED**: `assertion left == right failed: left: 50, right: 1` — every occurrence re-reads the file |
| 3 | `crates/willikins-core/src/compose.rs` | Pre-existing `ItemInUses` check wrapped `if false && contains_item(binding)` | `compose::tests::item_bound_to_a_uses_input_is_refused_as_item_in_uses` | **RED**: `unwrap_err()` called on `Ok(..)` — `link` succeeds, with the boundary's `binding` field literally `Some(Item)`, the exact silent-rebind shape decision (d4) warns about |
| 4 | `crates/willikins-core/src/compose.rs` | Pre-existing `UsesCycle` stack check wrapped in `if false { ... }` (depth bound left intact, so this cannot loop forever) | `compose::tests::a_self_use_is_refused_as_a_one_entry_cycle` | **RED**: panics — `resolve` is called for the self-use (`got 'loop'`), which the test's own resolver forbids; with the depth bound still active this terminates (no hang), it simply misclassifies what should be a cycle |

Every mutation was reverted by overwriting with the saved pre-mutation copy; `cmp` reported no
difference in all four cases, and `git diff --quiet` against the already-committed tree passed
immediately after each restoration (and once more after all four, at the end of the pass). The full
`willikins-core` suite (232 unit + 292 `tests/it` + 2 doctests) was re-run clean after the last
restoration to confirm nothing was left mutated.

## Gates run

- `cargo fmt --all --check` — clean.
- `cargo clippy -p willikins-core --all-targets -j 2 -- -D warnings` — clean.
- `cargo clippy -p willikins-server --all-targets -j 2 -- -D warnings` — clean.
- `cargo clippy -p willikins-cli --all-targets -j 2 -- -D warnings` — clean.
- `cargo test -p willikins-core -j 2` (full) — 232 + 292 + 2 doctests, 0 failed (run twice: once
  before the mutation phase, once after, identical counts).
- `cargo test -p willikins-server -j 2` (full) — 96 + 229 (2 ignored), 0 failed.
- `cargo test -p willikins-cli -j 2` (full) — 25 + 199 + 1 + 34 (1 ignored) + 2, 0 failed.
- `cargo test -p willikins-dsl -j 2 --test it acceptance::` — 13 passed, including
  `characterization_of_every_document` byte-identical (no `uses:`-bearing fixture was added, so no
  snapshot regeneration was needed for this task).

All run with `RUST_TEST_THREADS=2`/`-j 2`, each preceded by a 3-second quiet wait on
`pgrep -x cargo`/`pgrep -f cargo-sweep`, never piped through `tail`/`tee`.

## Fixtures

No new fixture under `workflows/fixtures/composition/` was added. Both real bypasses (findings 1
and 2) live in code paths the DSL cannot vary into existence — a hand-built `Workflow`'s binding
target, and a resolver's own call-count behaviour — so each is pinned by a unit test in the module
it fixed instead, per the task's own allowance for "hand-built probes: `compose.rs` unit tests" and
a server-side resolver test. Because no fixture changed, the dsl characterization snapshot
(`characterization_of_every_composition_document`) did not need regenerating for this task; it was
re-run to confirm.

## Commits

- `0b04b42` — `willikins-core: PathInAuthoredName also refuses a path-shaped binding target (X1)`
- `4f89af0` — `willikins-server: TrustedResolver caches a resolved document by name (X1)`
- `39841a0` — `willikins-cli: closes a priority target -- a fixed input via --input (X1)`

## Open items for the coordinator

- A second, independent attack pass (the plan's own "opus attacks" convention) has not run against
  this one; this session could not spawn one. Nothing here should be read as a substitute for it.
- The `for_each`-instance-key display ambiguity noted under "held" (an instance key containing `[`
  or `]`) pre-dates milestone 2b and is not new; it is recorded here only because it was explicitly
  checked, not because it is this milestone's to fix.
