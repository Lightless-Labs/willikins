# Milestone 3m: deterministic, safe rollback of what one run created

**Created:** 2026-10-05 (from `todos/2026-09-29-deterministic-safe-rollback.md`)
**Reviewed:** 2026-10-05 (portfolio review of the five plans of 2026-10-05: 3k, 2b, 3l, 3m, 3n)
**Reviewed:** 2026-10-12 (four-lens review of the policy revision: coherence, feasibility against the code, security, scope)
**Addendum:** 2026-10-05 — portfolio review. Order: this milestone runs **last**, after 3k, 3n F1–F3, 3l, 2b and 3n's
S, G and B parts, so `rollback.rs`, `resolve_pure` and `not_reversible` are written once against the sealed typed
graph (no `Binding`, no catalog lookup), its plan-JSON change does not land inside 3n's byte-identical window, and the
teardown identity can include 2b's used documents. Fixes: R2's "trybuild compile-fail without `executor`" cannot run in
this workspace (`willikins-types` turns `executor` on for its own tests through a self dev-dependency;
`crates/willikins-types/tests/derive_compile_fail.rs` documents why) and becomes the structural guarantee SinkToken
already uses; retiring `deploy/teardown.sh` is a code change (a cli test runs its suite and `no_gh_writes_guard`
expects it on disk), so it is task D0, gated, not part of D1; F1 commit 1 edits the `Plan { .. }` literals in
`willikins-cli`'s `render.rs` tests and regenerates the published plan schemas; the only tracked characterization
is the dsl one (it prints `plan_json`); R8/R9 check and copy 2b's used-document closure; P1 extends the `RepoBody` 3l
already extended. Two interactions are recorded under Risks. "Sequencing with other milestones" below holds the rest.
**Addendum:** 2026-10-05 (operator's decisions) — (a) `appstore.profile.ensure` stays Retained: a teardown never deletes a provisioning profile. (b) A Buildkite pipeline with any build is drift: the teardown refuses and says why; willikins never deletes build history. (c) The sandbox Buildkite token already deletes throwaway pipelines in the sandbox org (milestone 3i's live bootstrap cycle, step 8, 2026-10-02).
**Addendum:** 2026-10-05 (operator's correction, supersedes (a) and (b) above) — teardown behaviour is the document's policy, never the tool's ("Shouldn't this be a document decision?"). A tool declares only what it can do: `appstore.profile.ensure` gains an inverse that deletes the profile by the id the journal recorded; the Buildkite pipeline's inverse reports whether the pipeline has builds. Each node in a document declares its teardown policy (for example `teardown: retain | delete`, and for a pipeline what to do when it has builds: refuse or delete), validated by `check` against what the tool admits. A node that declares nothing gets the conservative default: retain, and refuse a pipeline with builds. The operator's iOS app document declares retain for its profiles and refuse for its pipeline. The Tasks table is revised to this before 3m starts. (c) stands.
**Addendum:** 2026-10-12 — revised for document-level teardown policy, in place: the Goal, trust boundary 8, SHARED
VALUES, decision 3a (new: the `teardown:` field, its default, how `check` validates it), decision 4 (the table states
capability only; the profile gains a `Delete` inverse; a pipeline's builds become a declared loss, not drift),
decisions 6, 7, 8 and 10 (policy evaluated per step; a document-kept step is `Kept`, told apart from a tool's
`Retained`), acceptance tests 5, 6, 13, 15, 16 and new 18-20, Needs the operator (items 1 and 2 answered), and the
Tasks table (new T1 and C1; R1, R3, R4, R5, R7, P4, P5, F1, R9 and L4 amended; 3k's test paths and 3n's catalog-free
signatures written into the rows). Nothing a tool declares says whether to delete; a node that declares nothing is
kept. The 2026-10-05 sequencing notes on 3k, 3n, 2b and 3l still hold and are now reflected in the rows.
**Addendum:** 2026-10-12 — the review's findings, folded in. The policy type is `TeardownPolicy` (the plan type keeps
`Teardown`). A node the document keeps that would ride inside a container it deletes refuses the teardown (decision
3a). A loss is re-checked by `delete` itself and recorded in the fingerprint, so a build that starts after approval
is never deleted. A container tolerates only riders that actually wrote or found something. The repository's drift
rule reads Actions secret names. Trust boundary 6 and decision 1 observe only `teardown: delete` nodes. T1 proves its
`CheckError`s on stub tools; the document fixtures move to T2, after the fakes declare (P6). R5 splits into R5a and
R5b, whose riding matcher F1 reuses. `rollback.rs` joins 3n's tripwires. `delete_profile` already exists and is
reused. D0 covers `live_smoke.rs`. Scoped commands use `--test it -- <filters>`.
**Addendum:** 2026-10-12 — X1 pass over R1 (`docs/research/2026-10-12-m3m-adversarial-pass.md`). R1 states
capability only and defaults to `Retained`. Two declaration shapes it admitted would let a rider vouch for contents it
never wrote, and `Catalog::insert` now refuses both on the tool's own side: a `Within` that maps no port
(`ReversalError::WithinMapsNoPort`: "every pair is equal" holds vacuously) and a `Within` own port that is secret
(`ReversalError::WithinPortSecret`: the journal holds only a marker, re-parsed `Unknown`, and two `Unknown`s compare
equal). `needs` may still name a secret credential port (decision 5). Forward requirements recorded there: R5a counts
the `LazyLock` regex's `expect` in `rollback.rs` when it adds the file to 3n's ratchet; R5b computes riding without
unbounded recursion (a `Within` may name its own tool); P7 should land before L1-L4 run.
**Addendum:** 2026-10-12 — R2 landed. The row reads "exactly one `TeardownToken::new` call site, inside
`apply_teardown`", but `apply_teardown` is task R6 and did not exist when R2 ran, so that sentence could not be made
literally true without inventing a throwaway stub for a capability token (exactly the shape X1's priority list warns
against: "a delete reachable without a `TeardownToken`", minted and dropped by nothing real). Instead,
`teardown_token_guard.rs`'s enclosing-function rule (the same per-function exemption `expose_secret_guard.rs` already
uses for `Credential::authorize`) is live today over a pinned count, `EXPECTED_SITES: usize = 0`, proven non-vacuous
by unit tests over hand-written sources rather than the real tree. R6 must bump `EXPECTED_SITES` to `1` in the same
commit that adds `apply_teardown`'s own call and its `#[allow(clippy::disallowed_methods)]`; leaving either behind
fails the guard.

**Addendum:** 2026-10-12 — R4 landed (`resolve_pure` in `plan.rs`, reusing `bind_ports`/`resolve_source`/`plan_one`
over `checked.nodes()`'s own order, with no new ratchet token and no `&Catalog`). `wanted: &[NodeName]` names the
pure producer nodes a `Delete` step's secret `needs` ports are directly bound to; the function computes their own
pure ancestors by a two-pass walk (readability forward, the backward-reachable "needed" set in reverse, never
crossing a non-pure node) and reads, via `bind_ports`/`plan_one`, exactly the nodes that are both. Its return,
`Resolved`, is `pub` (opaque) with private tables and a `pub(crate) fn ctx` that builds a `ResolveCtx` for
`resolve_source` — the same seam R5a's `plan_teardown` reads a `Delete` step's secret value through. Since neither
`Resolved`'s fields nor `ctx` have a non-test caller yet (R5a is next), the plain `lib` build (no `--cfg test`) would
flag both dead; both carry `#[cfg_attr(not(test), expect(dead_code))]`, self-removing the moment R5a's
`plan_teardown` calls `ctx()` from outside `#[cfg(test)]` — the same discipline R2's `EXPECTED_SITES = 0` already
uses. R5a's own commit must delete both attributes (leaving either behind fails the lib build once it is the real
caller). `wanted` naming a node outside `checked`'s own graph is a no-op, not an error: deriving it from the checked
graph is the caller's contract. Acceptance 3's unit tests (a resolver chain, a pure node bound to an impure output,
an unwanted pure gate, and `wanted` naming an impure node directly) live in `plan.rs`'s own `#[cfg(test)]` module,
not `tests/it/`, since they build a `Checked`/`Source` and call `ctx()`, all crate-private.

**Addendum:** 2026-10-12 — R3 landed (`Tool::inverse()`, `Standing`, `Deleted`, `Created`, the `Inverse` trait, and
`Catalog::insert`'s "`Delete` exactly when `inverse()` is `Some`" rule, with two new `ReversalError` variants,
`DeleteWithoutInverse` and `InverseWithoutDelete`). Decision 3's `Created<'a>` code sample names three fields whose
types no landed task yet defines: `window: CreationWindow` and `riders: &'a [Rider<'a>]` are R5a/R5b's, `accept: &'a
[Loss]` is T1's (`Loss` itself lives in `workflow.rs`, added by T1). Minting throwaway `CreationWindow`/`Rider`/`Loss`
shells here would be exactly the pattern R2's own addendum above already refused for `TeardownToken`'s capability
token. So R3's `Created` carries only `inputs`, `outputs` and `settled_by` (`crate::apply::Timestamp`, which already
exists); `Inverse::delete`'s documented contract already states the rule those three fields will carry (observe
first; anything but `AsCreated` is `Conflict`, so is `AsCreated` with an unaccepted declared loss; `Gone` is `Ok(Deleted
{ already_gone: true })`), in prose rather than in the type, so their later arrival changes no contract, only the type
that can express it. R5a adds `window`, R5b adds `riders`, T1 adds `accept` — each in the commit that adds the type it
needs, the same discipline R6 owes `EXPECTED_SITES` above. R3 also added two stub `Tool` implementations to
`testing.rs` behind `test-support`, `StubDeletableTool` (`Reversal::Delete` with the loss `"builds"`, a working
`inverse()`) and `StubContainedTool` (`Reversal::Contained`, riding out with the first), for T1's own stubs line to
bind in a document.

**Addendum:** 2026-10-12 — R5a landed (`plan_teardown`, three commits: the 3n guard registrations, `RunFacts`'/
`Teardown`'s own types plus `Created::window`, then `plan_teardown` itself). Deviations from the row, each recorded
here rather than left silent:
- **`RunFacts`'s own shape** (node, instance key, action, `RunProgress` — `NotFinished { started: Option<Timestamp> }`
  or `Finished { started, finished, status }` — recorded inputs/outputs) is this task's own invention: the row names
  the type but not its fields. `RunProgress::NotFinished { started: None }` folds the journal replay's own synthesized
  `NodeStatus::NotRun` (a planned instance the run never reached) together with "never started at all" — both mean
  nothing was created, and R8's own journal-replay caller is better placed to tell them apart in its own terms than
  `RunFacts` is forced to here.
- **Two helper types not in the row**, both private to `rollback.rs`: `Secrets<'a>` (bundles `checked`/`resolved`/
  `facts`/`single_names`/`each_names` so `build_needs_inputs`/`resolve_secret_source` stay under clippy's
  `too_many_arguments`) and `Decision` (`Step(TeardownAction)` / `Refused(&'static str)`, what `decide_candidate`
  returns per candidate) and `decide_candidate` itself, extracted out of `plan_teardown`'s own body so neither
  function tripped clippy's `too_many_lines`. None of these change the row's own SHARED VALUES surface
  (`RunFacts`/`Teardown`/`TeardownStep`/`TeardownAction`/`TeardownError` are exactly as specified).
- **A `${{ item }}`-bound secret `needs` port is not yet resolvable.** Decision 5 only asks for a pure resolver
  chain's `Source::Step`/`Aggregate`/`Keyed`; a credential bound directly to a `for_each` node's own item would need
  that instance's own recorded item value, which `RunFacts` does not carry (no landed or planned tool binds a
  credential this way). `resolve_secret_source` reports `TeardownError::UnresolvableCredential` for it rather than
  guessing or widening `RunFacts` on spec. Flagged for whichever later task first needs it.
- **`Reversal::Contained`'s own placeholder.** R5b (riding) has not landed; until it does, a `Contained` instance's
  own `TeardownAction` is `Retained { why: otherwise }` — the tool's own "left in place" reason, never a real rider
  decision. R5b's own row already says it "replaces this placeholder with a real rider decision", so this is not a
  new deviation, only flagged here for a reader of `rollback.rs` alone.
- **`rollback.rs` joins 3n's three guards** (the row's own last sentence) at a baseline of 2, not the file's
  pre-R5a 1: one pre-existing `.expect(` (`LOSS_NAME_REGEX`'s `LazyLock::new`) plus one new `unreachable!` in
  `decide_candidate` for `Catalog::insert`'s own "`Delete` exactly when `inverse()` is `Some`" invariant (R3), which
  is not visible to the type system at that call site — the same "honestly counted rather than reached for" choice
  `plan.rs`'s own G7 addendum already made for its two remaining fallbacks.
- **`StubDeletableTool` (testing.rs, test-support) gains a `standing_calls()` counter** (an `Arc<AtomicUsize>` shared
  by its `StubInverse`), via a new `with_standing_and_counter` constructor `with_standing` now delegates to: acceptance
  4, 6 and 19 all assert "zero `standing` calls" for a kept or already-refused candidate, which needs a count, not an
  inference. `testing.rs` is not covered by any ratchet or tripwire, so this adds no guard obligation.
- **Acceptance tests 4, 6 and 19 live in `rollback.rs`'s own `#[cfg(test)]` module**, not `tests/it/`: each builds a
  one-node `Checked` from `StubDeletableTool`/`StubContainedTool` and a trivial `resolve_pure`-built `Resolved`, the
  same "needs a crate-private type, so it cannot live in `tests/it/`" reasoning R4's own addendum already gives for
  `resolve_pure`'s tests.
- **Not done here, left to the row that owns it:** riding (`TeardownAction::RidesWith`, transit riders, a kept
  candidate inside a deleted container) is R5b's; the opus adversarial pass (X1) has not run over this task's own
  code yet.

**Addendum:** 2026-10-12 — a same-day review of R5a above (before X1) found one correctness bug and two gaps; fixed in
a fourth commit, same task:
- **Bug, fixed:** `resolve_secret_source` checked only the *direct* producer's purity. A `needs` port bound to a pure
  node that is itself bound to an *impure* node's own output (decision 5's own example: a credential resolver chain
  two nodes deep) passed that check, then `resolve_source` returned `Value::unknown` (`resolve_pure`'s own `Skipped`
  rule for an unreadable pure node) and `standing()` was called with it as though it were real — silently. Fixed by
  checking `value.is_known()` after `resolve_source` and reporting `UnresolvableCredential` otherwise; safe on every
  arm (`Literal` and a pure `Step`/`Aggregate`/`Keyed` reach it only once already known to resolve to something, and
  a secret `Source::Input` cannot exist, `check` already refuses a secret workflow input). No new ratchet token.
- **Added `pub fn wanted_resolvers(checked) -> Vec<NodeName>`** (plus a small private `source_producer_name`
  helper): the row's own `resolved: &Resolved` parameter presumes a caller can compute `resolve_pure`'s own `wanted`,
  but nothing before this addendum let one — `Checked::edge()` returns an `Edge`, never a `Source`, and
  `CheckedNode`/`Shape`/`Source` are crate-private. Without this, task R8 (`Butler::plan_rollback`) could not call
  `resolve_pure` correctly at all. Walks every node whose tool is `Reversal::Delete` and whose policy is
  `teardown: delete`, and names each secret `needs` port's own `Step`/`Aggregate`/`Keyed` producer.
- **Added test coverage for the decision 5 path itself** (a `FixedTool`/`wrap_tool`/`SecretNeedTool` fixture, four
  tests): a secret `needs` port resolving through a pure chain and the resolved value actually reaching the
  inverse's own `Created.inputs`; bound directly to an impure node; bound to a pure node downstream of an impure one
  (the bug fix's own regression test — fails without the `is_known` check); and `wanted_resolvers` skipping a kept
  node's own producer. Before this addendum, nothing exercised `Secrets::resolve_secret_source`'s secret branch at
  all — every acceptance-4/6/19 test uses `StubDeletableTool`'s non-secret `repo` port.
- **Left open, recorded rather than silently dropped:** SHARED VALUES credits R5a (with R7) for "Step fingerprint"
  (`StepFingerprint { node, instance, action, standing: Option<StandingKind> }`). This task's own row does not name
  `StepFingerprint` or `StandingKind` among what it must produce, and `TeardownStep` does not carry the observed
  `Standing` (an `AsCreated`'s `losing` names are consumed into `TeardownAction::Delete`'s bare `uncertain` bool and
  otherwise dropped). R7 needs them back — "a loss that appears between approval and apply therefore differs" is
  exactly `losing`'s own job. Minting `StandingKind`/`StepFingerprint` here, before R7 has designed the rest of the
  typed fingerprint and its journal shape, risked the same "throwaway shell" R2's and R3's own addenda already
  refused elsewhere. R7 must either add `standing: Option<StandingKind>` to `TeardownStep` (or derive it from
  `TeardownAction` plus whatever `AsCreated`'s `losing` turns out to need) in the commit that adds `StepFingerprint`
  itself, or explain in its own addendum why it does not need to.
- **`wanted_resolvers` over-approximates decision 5 on purpose, documented on the function itself.** Decision 5 names
  the pure nodes a `Delete` *step's* own secret `needs` ports are bound to; a step only exists for an instance
  `plan_teardown`'s own candidate predicate admits, which needs `RunFacts`. `wanted_resolvers` has only `checked`, so
  it reads every `teardown: delete` node's own secret `needs` ports, whatever a given run's instances actually did.
  Safe (never an impure `read`; the only cost is evaluating a resolver chain `plan_teardown` then does not need) but
  wider than necessary. Tightening it to the run's own candidates is task R8's call, not this one's.
- **Also fixed, same pass:** `UnresolvableCredential`'s own message named one cause ("not bound to a pure resolver
  chain") when the variant already covers three (an impure producer, a resolved-but-`Unknown` value, and the
  not-yet-supported `${{ item }}` case); reworded to "not honestly bound to a pure resolver chain" so a reader is not
  misled about which one fired.

**Addendum:** 2026-10-12 — R5b landed (riding, decisions 3 and 3a, acceptance 5), in `crates/willikins-core/src/rollback.rs`
alone (`-p willikins-core`: `cargo fmt --check`, `clippy --all-targets -D warnings`, `cargo test`, all green; the ratchet
stays at 2, below). Deviations from the row, recorded:
- **Riding's own structural match is computed once, independent of the fixed point that decides each instance's own
  action.** The coordinator's own dispatch for this task (never the plan's row or the X1 addendum, which says only
  "without unbounded recursion") described computing riding "as a fixed point seeded from `Delete` steps", which reads
  as if the *step* (a resolved action) seeds riding. That cannot work: a container's own `standing()` call needs its
  own tolerated riders (`Created::riders`) *before* it is known whether that call will even return `AsCreated` (let
  alone `Delete`), so nothing yet resolved can seed anything. The actual implementation splits the two: `container_of`
  (one match per instance, by [`rides_with`] alone, over every instance `build_ride_views` admits, regardless of
  status) is pure structure, computed up front from recorded values alone, exactly as decision 3 already says ("riding
  is computed over all of the run's instances... the mapping is deterministic and needs no graph"); `riders_of`/
  `transitive_riders` (both wrote-filtered) read off that structure to build a `Created.riders` for any instance that
  ends up calling `standing`, whatever its own eventual label. Only the **label** each instance itself receives
  (`RidesWith`/`Kept`/standalone) is a fixed point (`resolve_actions`), consulting what `container_of[idx]` already
  settled to. The dispatch's "seeded from `Delete` steps" is better read as "seeded from whichever instances need no
  other instance's own resolution first" (a `Retained` tool, an unmatched instance, or one whose single `Within`
  entry matches nothing) — those resolve immediately and everything else's resolution bottoms out in one of them,
  transitively.
- **A `teardown: delete` rider is never itself observed; only the chain's own grounding instance calls `standing`.**
  Trust boundary 6 is marked normative and says "the teardown plan observes every candidate whose node says
  `teardown: delete`"; decision 6 reads differently, splitting the two ("Riders follow decision 3a" is its own,
  separate sentence from "one whose node says to delete it is observed"), and decision 3's own "riding is not
  tolerance" paragraph only ever asks the *container's* inverse to tolerate a rider, never asks the rider's own
  inverse anything. This task reads decision 6 over trust boundary 6 where they disagree: a `RidesWith` step's own
  `standing` is never called (pinned by `config_rides_with_the_project_created_in_the_same_run`'s `config_calls == 0`
  and the X1 mutual-ride test's single `standing` call total). Flagged for R6, whose own re-observe-before-apply must
  make the same choice consistently: re-observe only what plan time observed (every chain's own grounding instance),
  never a step whose action is `RidesWith`.
- **`plan_teardown`'s main loop is now two phases, not one reverse pass.** Riding "needs no graph" (decision 3), but a
  rider can still structurally precede its container in reverse `Plan::nodes` order (dependents come first) while that
  container's own resolution is exactly what the rider's own label depends on. `resolve_actions` resolves every
  `Create`-actioned, `Created`/`Failed`/`Unchanged` instance first, in whatever order its own dependencies allow (never
  `facts.instances`' own order); `plan_teardown` then walks `facts.instances` in reverse exactly as before, now reading
  each instance's own precomputed `Resolution` from a `HashMap<usize, Resolution>` keyed by its original `facts`
  index, to keep the output's own step order unchanged.
- **`RidesWith`'s own `container` names the *direct* structural match, never the root it may chain up to**, so a token
  inside a config inside a project rides with the config, not the project — the project's own `Created.riders` still
  sees it, transitively (`a_transit_rider_two_hops_down_still_reaches_the_root_container`, pinning exactly this for the
  `new-rust-service` shape decision 3 names). Not stated either way by the row; picked once, pinned, recorded here per
  the X1 addendum's own instruction.
- **Decision 3a states two distinct rules for a `Delete`-capable rider that structurally matches a *kept* container**,
  and this task trusts the longer, normative decision text over acceptance 5's one-line paraphrase ("lists the config
  and the secret `Kept`") where the two disagree: (1) a node with **no policy of its own reason to be kept** (`Contained`,
  or a non-candidate transit rider) that matches a kept container is `Kept { why: KEPT_RIDER }`; (2) a genuine
  `Delete`-capable **candidate** whose own `teardown: delete` matches a kept container is its own standalone `Delete`
  step (decision 3a's own last sentence: "whose container is kept is its own `Delete` step") — kept-ness here is never
  inherited, only mirrored when the rider has no say of its own. Both are pinned by their own test
  (`riders_of_a_kept_project_are_kept_and_nothing_is_observed` for the first; the new
  `a_delete_capable_rider_whose_container_is_kept_is_its_own_delete_step` for the second), since acceptance 5's own
  sentence names only the first and a reader of the row alone would reasonably expect the second to behave the same
  way.
- **The mutual-ride cycle (X1's own forward requirement) is broken by a deterministic tie-break, not a depth bound.**
  `resolve_actions` runs its own relaxation to a fixed point; if any instance is left unresolved once a full pass makes
  no progress, the earliest such instance (by its own position in the ride resolver's filtered sequence, itself
  `facts.instances`' own order restricted to this task's candidate/transit universe) is force-resolved exactly as if
  it matched nothing at all. A forced resolution is never itself pending, so it always breaks the cycle, and it can
  only ever *ground* a chain, never fabricate a second, independent `Delete` out of a match that was never real.
  Pinned by `a_within_naming_its_own_tool_never_produces_a_mutual_ride`: two instances of one tool whose own `Within`
  names that same tool, each matching the other, resolve to exactly one `Delete` and one `RidesWith`, and `standing`
  is called exactly once.
- **New `SHARED VALUES` constants:** `KEPT_RIDER` ("its container is kept by the document", `TeardownAction::Kept`'s
  `why` for the first rule above) and `RIDER_KEPT_INSIDE_DELETED_CONTAINER` ("a node the document keeps is inside a
  container it deletes", decision 3a's own refusal reason, exact string as written there).
- **`Reversal::Contained`'s own R5a-era placeholder is gone.** Every `Contained` instance now gets a real rider
  decision (`RidesWith`, `Kept { why: KEPT_RIDER }`, or its own `otherwise`), never the blanket `Retained { why:
  otherwise }` R5a's addendum flagged as standing in for this task.
- **No new ratchet token.** The one `unreachable!` the row's own shape might have invited (unwrapping
  `resolve_actions`' internal `Vec<Option<Resolution>>` once its own loop has proven every entry `Some`) is instead a
  bare `.flatten()`: the loop's own exit condition already *is* the proof, so nothing needs asserting at the point
  nothing can go wrong. `rollback.rs` holds at 2 (R5a's own two: `LOSS_NAME_REGEX`'s `expect` and `decide_standalone`'s
  — renamed from `decide_candidate`, same call — `Catalog::insert` invariant).
- **Riding's own test fixtures (`StubProjectTool`/`StubConfigTool`/`StubSecretTool`/`StubSelfRidingTool`,
  `RidingInverse`/`ToleranceInverse`) live in `rollback.rs`'s own `#[cfg(test)]` module, not `testing.rs`'s
  `test-support`.** R5b's own acceptance 5 is scoped `-p willikins-core` alone, unlike R3's `StubDeletableTool`/
  `StubContainedTool` (which T1's own `-p willikins-dsl`/`-p willikins-cli` stub-line tests also bind); nothing outside
  this crate needs a tool that rides today. A later task that does should promote what it needs rather than duplicate
  it.
- **`decide_candidate` is renamed `decide_standalone`** (same behaviour, plus a new `riders: &[Rider<'_>]` parameter
  threaded into `Created`): "candidate" now means something broader in this file (the ride resolver's own `Create`+
  `{Created,Failed,Unchanged}` universe), and this function specifically is the "matched nothing, or its match does
  not change its own fate" path — decision 6's own capability × policy × standing path, unchanged.

**Addendum:** 2026-10-12 — a same-day review of R5b above found two gaps, fixed in two more commits, same task:
- **`Created.riders` was asserted only indirectly.** `StubProjectTool` could only hold a fixed-`Standing`
  `RidingInverse`, so `config_rides_with_the_project_created_in_the_same_run` and
  `a_transit_rider_two_hops_down_still_reaches_the_root_container` proved the `RidesWith`/`Delete` labels and the call
  counts, never that the rider actually reached the container's own `Created.riders`: a `transitive_riders` bug
  returning nothing would have left every assertion in both tests green. Fixed by giving `StubProjectTool` any
  `Inverse` (mirroring `StubConfigTool`) and switching both tests' project to a `ToleranceInverse` that checks the
  real count (1, then 2 across the transitive hop). A third test, `a_failed_rider_rides_but_is_never_tolerated`,
  exercises `riders_of`'s own `wrote` filter directly: a `NotRun` rider never reaches a `RideView` at all
  (`build_ride_views` requires a finished instance), so the existing `NotRun` acceptance test never actually ran that
  filter's own line; `Failed` does reach one and is excluded by it. Both mutations (`transitive_riders` returning
  `Vec::new()`; `riders_of` dropping its own `wrote` check) were run by hand against the fixed tests and caught by
  exactly the test built to catch each one, and nothing else.
- **A note for F1, not yet acted on.** `Recorded` and `rides_with` are `rollback.rs`-private today; the Recorded/
  RideView split above was built so `Plan::not_reversible` (task F1) can call `rides_with` directly over a forward
  plan's own instances, with no `RunInstance`. F1's own commit should promote both to `pub(crate)` (`Recorded`'s
  fields included) rather than duplicate either.

**Addendum:** 2026-10-12 — R6 landed (`apply_teardown`, in `crates/willikins-core/src/rollback.rs` alone: `-p
willikins-core`, `cargo fmt --check`, `clippy --all-targets -D warnings`, `cargo test`, all green; the ratchet stays
at 2, `teardown_token_guard.rs`'s `EXPECTED_SITES` moves from 0 to 1 in the same commit as the call, exactly as its
own 2026-10-12 addendum required). Deviations, recorded:
- **`StandingKind`/`StepFingerprint` are taken over from R7, here, by R6** — not left open as R5a's own addendum
  (immediately above) flagged. That addendum's own framing ("R7 must either add `standing` to `TeardownStep`... or
  explain why it does not need to") assumed R7 would be the first caller; R6's own row text says otherwise ("compare
  fingerprints... standing kinds and losses included"), and the row is literal: decision 8's "a loss that appears
  between approval and apply therefore differs" is a case `TeardownAction` alone cannot show — an *accepted* loss
  never changes a step's own action (`Delete { uncertain }` either way) — so comparing two freshly re-planned
  `Teardown`s by `==` would silently let exactly that bypass through (a build appearing under `accept: [builds]`
  between approval and the apply-time re-plan). `StandingKind` carries only `Gone`/`AsCreated { losing }` (the two
  `Standing` variants that ever reach a `Teardown` step at all; `NotOurs`/`Drifted` only ever refuse, so they need no
  fingerprint). `StepFingerprint` bundles `node`+`instance` into one `InstanceRef` rather than SHARED VALUES' own two
  separate fields, matching `TeardownStep::instance`'s own shape — a naming deviation, not a meaning change. `R7`
  owns journaling `StepFingerprint` (as `Redacted<Teardown>` plus the typed fingerprint, decision 8) and may still
  reshape it there; nothing here commits R7 to this exact Rust representation surviving serialization unchanged.
- **No new public function changes `plan_teardown`'s own frozen signature or behaviour.** The shared computation
  (riding fixed point, per-instance classification) moved into a private `plan_teardown_steps`, which now also
  returns each step's own `StepFingerprint` (read straight off values `decide_standalone`/`Resolution` already
  compute — no second round of `standing()` calls); `plan_teardown` is now a two-line wrapper over it, and a new
  `pub fn plan_teardown_fingerprint` is the one a future `Butler::plan_rollback` (task R8) calls to get both the
  `Teardown` to show the operator and the fingerprint to journal and later pass to `apply_teardown`. Every one of
  R5a/R5b's own existing tests (315, now 321 with R6's own six) passed unmodified against this refactor — confirmed
  by running them both before and after.
- **`apply_teardown`'s own signature takes `approved: &[StepFingerprint]`, never a `Teardown`.** A `Teardown`'s own
  `TeardownStep`s do not carry `standing`, so comparing against one could not catch the gap above; `StepFingerprint`
  already carries `instance`+`action` too, so nothing is lost. `facts`/`checked`/`resolved` are the same three the
  original plan was computed from (SHARED VALUES' own "Core signatures" row: `&Checked`, no `&Catalog`).
- **Re-observation is literally a second `plan_teardown_steps` call**, compared to `approved` by `Vec<StepFingerprint>`
  equality (`!=`) before anything else runs. This satisfies the row's "re-observe exactly the set `plan_teardown`
  observed" by construction — including trust boundary 6 as R5b's own addendum reworded it (never a `RidesWith` step
  on its own; only a chain's grounding instance ever calls `standing`) — without a second, parallel implementation of
  the ride resolver that could drift from the first's own rules.
- **Rebuilding `Created` to call `delete`** (the needs re-resolution, window, `settled_by`, accept list and riders
  `decide_standalone` already computes once, at plan time, and discards) is a second, independent computation in a
  new `delete_one`, reusing the same private helpers (`Secrets::build_needs_inputs`, `build_ride_views`,
  `container_of`, `riders_of`, `transitive_riders`, `find_checked_node`, `instance_window`, `shift`) rather than
  threading the first computation's own borrowed materials through the fixed point's `Resolution`/`Decision` types
  (which would have meant giving those types a lifetime tied to `facts`/`checked`, a wider change than this task's
  own row asked for). Every fallback in `delete_one` is a typed `TeardownApplyError::Inconsistent` (`ok_or_else`),
  never `unwrap`/`expect`/`unreachable!`/`panic!`: by construction none of these cases can actually happen once
  `approved == fresh` has just been checked, but `apply_teardown` trusts no caller's data blindly, so the ratchet
  gains nothing from this task (it stays at 2).
- **"Listed for every non-delete step" is read as unconditional.** Decision 8's own two clauses ("a delete failure
  stops the run, and later steps are `NotRun`" vs. "`Listed` for every non-delete step") read ambiguously taken
  alone; this task reads `NotRun` as reserved for a `Delete` step that never got its own `delete` call because an
  earlier one failed, and every non-`Delete` step is `Listed`, whatever its own position relative to a failure.
  `apply_teardown`'s own check order (`!Delete` → `Listed`, checked before `stopped` → `NotRun`) makes this true by
  construction; `a_delete_failure_stops_the_run_and_later_delete_steps_are_not_run` pins the *before* case
  (`checked_lossy_trio`'s own `repo_c`, an `Update`-actioned `Retained` step declared first so it is the *last*
  teardown step, `Listed` even after `repo_a`/`repo_b`'s own failure) -- a same-day advisor-pass finding, closed
  without a second test: one case already proves the unconditional reading, since the check order is what decides
  both, not the position.
- **Acceptance 21's core half is two separate tests, not one**, because the row's own two clauses are two different
  moments: `delete_itself_refuses_an_unaccepted_loss_appearing_only_at_delete_time` (the re-plan still agrees with
  `approved`; only `delete`'s own, later re-observation finds the loss, so `delete` itself returns `Conflict` and the
  step is `Failed`) and `a_loss_appearing_before_the_replan_refuses_as_drift_though_the_action_is_unchanged` (the
  loss is already there by the re-plan, under `accept: [builds]`, so the *action* does not change but the
  fingerprint's own `standing` does, and `apply_teardown` refuses before minting a token at all). A plain `teardown:
  delete` (no `accept`) is required for the first case to even reach `delete`: under `accept: [builds]` the same
  loss at re-plan time would not refuse there at all.
- **A new stub, `ObservableInverse`/`StubLossyTool`, local to `rollback.rs`'s own `#[cfg(test)]` module** (R5b's own
  precedent: nothing outside this crate needs a tool whose `standing`/`delete` disagree after construction).
  `StubDeletableTool` (`testing.rs`) cannot express this — its `StubInverse` is fixed for life and its own `delete`
  never even looks at `created.accept`. `ObservableInverse` separates "what `standing()` reports" from "what
  `delete()` itself re-observes", each independently settable after construction, and `delete()` honours the
  documented `Inverse::delete` contract for real (`Gone` → already-gone; `AsCreated` with every loss accepted →
  deletes; anything else → `Conflict`) — the first stub in this file to do so.

**Addendum:** 2026-10-12 — a same-day review of R6 above (advisor pass, before X1) found one gap: no test exercised
`apply_teardown` with a rider in play, leaving untested exactly the half of trust boundary 6 that is not true by
construction at *delete* time (`delete_one`'s own rebuild of `Created.riders` is a second, independent computation
from `decide_standalone`'s), and the "never a `RidesWith` step on its own" rule at apply time specifically. Fixed,
same task, same file:
- **`apply_teardown_lists_a_rider_and_hands_its_container_the_same_riders_at_delete`**, reusing
  `config_rides_with_the_project_created_in_the_same_run`'s own fixture (`checked_riding`/`facts_multi`): asserts the
  outcomes (`Listed` then `Deleted`, in `approved`'s own reverse-plan order), that the rider's own inverse sees zero
  calls at apply time (not just zero at plan time), and that the container's own `delete` is handed
  `created.riders.len() == 1` -- the one rider its own `standing` already tolerated.
- **A new local fixture, `RidingDeleteInverse`** (combines `ToleranceInverse`'s own riders check with recording what
  `delete` itself was handed): neither existing riding inverse lets a test see `Created.riders` at delete time.
- **`RidingInverse::delete` now also increments its own shared `calls` counter**, not only `standing`: every
  pre-existing test using it only ever calls `plan_teardown` (`delete` was unreachable), so no existing assertion
  changes, but a new test asserting "this inverse's own `delete` was never called" needs the counter to actually
  reflect that.
- **`a_resource_recreated_after_a_gone_approval_refuses_as_drift`**, pinning that `apply_teardown`'s own re-plan
  re-observes a `Gone` step too, not only the `Delete` ones: a resource someone recreated after a `Gone` approval
  must still refuse. A design that only re-ran `approved`'s own `Delete` steps through a fresh `standing` call,
  rather than calling `plan_teardown_steps` in full, would have missed this.
- **`planning_again_after_a_delete_reads_the_step_gone` did not prove its own name**, a second advisor-pass finding
  on the same review: it set `plan` to `Gone` by hand, so only the "reads `Gone`" half of acceptance 8 ran, never
  "the second teardown succeeds". Fixed by having `ObservableInverse::delete` tell itself `Gone` (both `plan` and
  `delete`) on its own successful, non-`already_gone` delete, exactly as a real resource's own `standing` would
  report without being told; the test now runs a full second `plan_teardown_fingerprint` → `apply_teardown` cycle and
  asserts its one step is `Listed` (`delete_calls()` staying `1`) -- the one `TeardownStepOutcome::Listed`-from-`Gone`
  path no other test produced. (The match arm that flips the stub's own state reads its `Mutex` into an owned value
  in its own `{ }` block first, never the match scrutinee directly, so the arm's own re-lock cannot deadlock on a
  guard the match statement might otherwise still be holding.)

**Addendum:** 2026-10-12 — R7 landed (the journal, decision 8, acceptance 9) in four commits:
`crates/willikins-core/src/rollback.rs`+`apply.rs`+`lib.rs` (core's own journal-safety fixes), then three in
`willikins-journal` (`Teardown`/`TeardownApplyError` join `Redactable`; the four events, the new `ApplyRefusedReason`
variants, `DriftReasonKind::Standing`, `TeardownRecord`, `RunRecord::rolled_back_by`, and every downstream compile
fix; the fold's own coverage; the redaction test). `-p willikins-journal`, `cargo clippy -p willikins-server -p
willikins-cli --all-targets -D warnings`, `-p willikins-server --test it -- mcp_server::`, `cargo fmt --all --check`
and `cargo check -p willikins-types` all green. Deviations, recorded:
- **R5a's own open item (`TeardownStep` does not carry `standing`; add it back, or explain why not, in the commit
  that adds `StepFingerprint`) was already closed by R6**, before this task started: R6's own addendum (below R5a's
  in this file, since it landed after) took `StepFingerprint` over from R7 and explained, in `rollback.rs` itself,
  why `TeardownStep` stays as it is ("`TeardownStep`s do not carry `standing`... `StepFingerprint` already carries
  `instance`+`action` too, so nothing is lost"). Nothing left for this task to do on that specific point; flagged
  here only because this task's own dispatch named it.
- **`StepFingerprint.action` is reshaped from `TeardownAction` to a new `TeardownActionKind`, and
  `StandingKind::AsCreated.losing` moves from `Vec<&'static str>` to `Vec<String>`.** `StepFingerprint` is stored
  directly (unwrapped) on `TeardownPlanned.fingerprint`, exactly the way `InstanceFingerprint` already sits on
  `PlanRecorded.fingerprint` -- so it must round-trip through `Deserialize` for a restarted `willikins-server` (task
  R8) to read back a typed, comparable fingerprint from the journal at apply time, the same way `apply` already does
  for a forward run's drift check. `serde` cannot produce a `&'static str` back out of a deserializer without
  leaking memory on every replay, so every `&'static str` payload (`TeardownAction::Kept`/`Retained`/`MadeByHand`'s
  own `why`/`need`, `StandingKind::AsCreated`'s own `losing` names) had to go. `TeardownActionKind` keeps exactly the
  discriminant and the two payloads that carry identity (`Delete::uncertain`, `RidesWith::container`), dropping each
  variant's own display text -- already shown to the operator via `Redacted<Teardown>` -- and this is no loss of the
  safety property decision 8 names: `StepFingerprint.action` already keeps `Kept` and `Retained` apart as different
  enum variants, whatever payload either carries. `TeardownAction`/`Teardown`/`TeardownStep` themselves are
  unchanged, still the rich, display-oriented types `rollback.rs`'s own tests build and compare directly; only the
  parallel, round-tripping `StepFingerprint`/`StandingKind` moved. `InstanceRef` (`apply.rs`) gained `Deserialize`/
  `JsonSchema` for the same reason -- both fields were already owned, so this is a pure addition.
- **`TeardownError::Resolve` and `TeardownApplyError::Replan` move from tuple variants to struct variants with one
  named field** (`Resolve { error: PlanError }`, `Replan { error: TeardownError }`, `#[from]` on the field, same as
  before): both enums are now internally tagged (`#[serde(tag = "kind")]`, for `willikins_journal::Redacted`), and
  each one's single payload is *itself* internally tagged the same way (`PlanError`/`TeardownError`), so a tuple
  variant's own tag insertion would collide with the nested value's own `"kind"` key in one JSON object. A struct
  variant with a named field avoids this exactly the way `willikins_core::ApplyError::Plan { error: PlanError }`
  already does. `#[from]` works the same on a named field, so every `?` call site (`resolve_source(...)?` in
  `rollback.rs`) and the one test matching `TeardownApplyError::Replan { .. }` needed no behavioural change, only the
  pattern's own shape.
- **`TeardownFinished`'s `Failed` outcome carries `Redacted<willikins_core::TeardownApplyError>`, not the type
  directly** (`TeardownOutcome`, a new type mirroring `Outcome` with no `Blocked` equivalent -- a teardown has no
  gate to block on, trust boundary 1). `TeardownApplyError` (and the `TeardownError`/`PlanError` it can nest through
  `Replan`) has no `Deserialize`, for the same reason `Teardown` itself does not (its own `&'static str` payloads),
  so `Event` could not derive `Deserialize` either were this field stored unwrapped -- exactly `Outcome::Failed`'s own
  reason for wrapping `ApplyError`. `Teardown` and `TeardownApplyError` both join `Redactable`'s sealed set in
  `willikins-journal/src/redacted.rs` (the only place that can: its `sealed` module is private to that file).
  `redacted.rs` gains a test proving a secret planted in `PlanError::NameTaken`'s own `key: Inputs` (the one
  `Value`-bearing field reachable through `TeardownApplyError::Replan`'s own nesting) does not leak when wrapped --
  not a second redaction path, the same guarantee `willikins_core::Value`'s own `Serialize` already gives every other
  `Redacted<T>`.
- **No `TeardownRunRecord`/per-step replay view.** `TeardownRecord` mirrors `PlanRecord` (approval state, `applied`
  set on `TeardownStarted`), but a teardown run's own per-instance progress (`TeardownStepFinished`) has no replay
  view yet: nothing in this task's own row or acceptance 9 asks for one, and resuming a half-finished teardown
  (decision 8, "what it deleted reads `Gone`") works through live re-observation (`standing()`), never through
  reading the journal's own per-step history back. `RunRecord::rolled_back_by` is the one cross-run fact a later task
  (R8's `AlreadyRolledBack` refusal) needs, and it is set directly: `fold_teardown_started` records each teardown
  run's own `of_run` in a transient map (mirroring `FinishedNodes`'s own transience), and a `TeardownFinished{Succeeded}`
  reads it back to set the undone run's own `RunRecord.rolled_back_by` to the teardown run's id. A later task that
  does need a teardown's own per-step replay should add it as its own view, not assume one exists here.
- **`fold_approval` resolves a `plan_id` against `plans` first, then `teardowns`.** The two live in one `PlanId`
  namespace and an id is minted for exactly one kind of record (never both), so this is never ambiguous; mirrors
  `Journal::teardown` sitting beside `Journal::plan` and `ReplayedJournal::teardown` beside `ReplayedJournal::plan`.
- **No duplicate-event guard added for the four new events in `file.rs`'s own corrupt-file check**, unlike
  `PlanRecorded`/`RunStarted`/`RunFinished`/`NodeFinished`. Acceptance 9 asks only that every new event round-trips
  and that the pre-milestone fixture still replays; a hand-edited file duplicating a `TeardownPlanned`/`TeardownStarted`/
  `TeardownFinished` line is not a case any acceptance test here exercises. Flagged as a gap a later task (or X1's
  own adversarial pass) may want to close, not silently dropped.
- **Acceptance 9's redaction test is split, per this task's own module doc in `redaction_by_construction.rs`.** The
  shared `events_carrying_the_secret()` loop also asserts every sample shows a `"REDACTED"` marker (so it is never
  vacuously passing); `TeardownFinished`'s own chain (through `PlanError::NameTaken`) joins that loop, since it is
  the one real risk in these new event shapes. `TeardownPlanned` cannot be seeded at all -- neither `Teardown` nor
  `StepFingerprint` has a `Value`-bearing field -- so it gets its own, separate test that only asserts no leak, with
  no marker expected either way. The acceptance text's "into a rider `secret.set`'s value port" half is a
  core-level guarantee already proven in `rollback.rs`'s own test suite (`Created.riders`' secret ports are
  `Value::Unknown` by construction, decision 3): `Created`/`Rider` are `willikins-core`-internal, seen only
  in-process by an inverse's own `standing`/`delete`, and never serialized by any journal event type at all -- not
  something this crate's own types could be asked to redact.
- **`StandingKind`/`TeardownActionKind`/`StepFingerprint`/`Teardown`/`TeardownStep`/`TeardownAction`/`TeardownError`/
  `TeardownApplyError`/`TeardownStepOutcome` join `willikins_core`'s flat `pub use rollback::{...}` re-export list**
  in `lib.rs`, matching `Reversal`/`Within`'s own precedent (every other substantial public type from this module is
  named there, not only reachable through the `rollback` submodule path) -- `RunFacts`/`RunInstance`/`RunProgress`
  stay un-re-exported, since nothing in this task needs them from outside `willikins-core` (task R8's own call).
- **No new ratchet token, in either crate.** `rollback.rs` stays at 2 (no new `unreachable!`/`expect`/`unwrap`/
  `panic!` in non-test code; the `From` impl between `TeardownAction` and `TeardownActionKind` is exhaustive by
  construction).
- **For R8/R9:** `Journal::pending_approvals` is unchanged, `PlanRecord`-only (the approvals page's own
  teardown-shaped rendering is still the follow-up todo this milestone's "Out of scope" names) -- a pending
  teardown is reachable only through `Journal::teardown(plan_id)` once its own `plan_id` is known.
  `willikins-server`'s `Butler::decide` (`butler.rs:747`, `journal.plan(&plan_id)`) still looks up only `plans`; task
  R8 must change it to try `journal.teardown(&plan_id)` too (`fold_approval` already resolves either kind on the
  journal side, `decide` is the one caller left that does not).

**Addendum:** 2026-10-12 — P1 landed (`github.repo.ensure`'s inverse; `Contained` for
`github.actions_secret.ensure` and `github.scaffold.ensure`; decisions 4, 5, 7; acceptance 12) in
`crates/willikins-providers-github` alone, two commits (396110d the client, 7c2dfc6 the tool),
scoped gates (`-p willikins-providers-github`: `cargo fmt --check`, `cargo clippy --all-targets -D
warnings`, `cargo test`) green for both. Deviations and choices, recorded:
- **`needs: &["repo", "token"]`, not just `"repo"`.** The row names only `delete_repo`, but decision
  5 explicitly names `github.repo.ensure`'s `token` port as the credential example, so decision 5
  controls: `standing`/`delete` resolve a document-bound `token` through `ScopedClient::default_for`
  exactly as `read`/`ensure` already do, read from `created.inputs` instead of a tool call's own
  `Inputs`. Pinned by `standing_uses_a_bound_token_client` (a `404` is enough to prove the credential
  swap; the drift table itself is every other test's job).
- **The Actions-secrets read paginates** (`?per_page=100&page=N`, stopping when the running total
  reaches the response's own `total_count` or a page comes back short), where decision 7 writes the
  bare path `GET /repos/{o}/{r}/actions/secrets`. A single, unpaginated page would let a foreign
  secret name on page two pass the drift check unnoticed -- trust boundary 6's "all reads before any
  delete" is read as covering this. Acceptance 12's "pins the exact query" is now the first page's
  query string (`per_page=100&page=1`); a second test (`list_actions_secret_names_follows_total_count_across_pages`)
  pins the second page's.
- **Every `None` on `RepoBody`'s five new decision-7 fields fails closed, never "no opinion".**
  GitHub's own `OpenAPI` description (fetched 2026-10-12, `github/rest-api-description` main,
  `descriptions/api.github.com/api.github.com.json`) marks `full_name`, `created_at`, `pushed_at`,
  `open_issues_count` and `forks_count` all `required` on `full-repository`, so a real response never
  omits one; the only `None` this crate ever produces is a pre-milestone mock fixture that predates
  the field (the row's own reason for `#[serde(default)]`). `full_name: None` and `created_at: None`
  both read `Standing::NotOurs` (the first because `None != Some(repo.to_string())` holds trivially;
  the second by an explicit `let Some(..) = .. else` on the window check); `pushed_at`,
  `open_issues_count` and `forks_count` each read `Standing::Drifted` via `Option::is_none_or`
  (`body.pushed_at.is_none_or(|p| p > settled_by)` and so on) -- "cannot rule the thing out" is read
  the same as "the thing is true", not as "assume the safe case". Each constant's own doc names this.
  `full_name`'s `NotOurs` reason is shared with the 3xx and rename/transfer cases (one row of decision
  7); the collapse is documented on the constant rather than split into a third reason.
- **The creation window is inclusive at both ends** (`>= start && <= end`), pinned by three tests at
  `start − 1s` (`NotOurs`), `end` exactly (`AsCreated`), and `end + 1s` (`NotOurs`) -- acceptance 12
  only asked for the upper edge; the lower edge is symmetric and otherwise unproven.
- **`delete_repo` is retried under `retry_secondary_limit`**, unlike `create_repo`'s own `POST`: a
  retry only ever fires after GitHub's secondary rate limit refused the request outright, before
  anything was deleted, so a repeated `DELETE` carries none of `POST`'s ambiguous-failure hazard.
- **`Side::Input` on both new `Within`s** (`github.actions_secret.ensure`'s and
  `github.scaffold.ensure`'s own `repo` port maps to `github.repo.ensure`'s `repo` **input**): both
  are inputs on the container tool, so this is the only reading the type allows, but it is picked
  once and recorded since the table's "Within" column does not spell out the side.
- **Tests live in a new `tests/it/repo_inverse_mock.rs`**, not `repo_ensure_mock.rs` (19 tests: one
  per decision-7 `NotOurs`/`Drifted` row, the three window-edge tests, a tolerated-vs-foreign Actions
  secret pair, the bound-token test, `delete`'s three outcomes, and the `reversal()` shape). Each
  `Contained` declaration gets one shape test in its own existing mock file
  (`actions_secret_ensure_mock.rs`, `scaffold_ensure_mock.rs`) rather than a new file, since neither
  has an inverse or a provider call to mock. `GitHubScaffoldEnsure` is not inserted into any
  `Catalog` by this crate's own tests (only `catalog_check_parity.rs` does, and only for the repo and
  secret tools); its declaration is structurally identical to the secret's own (which is validated),
  and P7's table test over `LIVE_TOOL_NAMES` validates it next.
- **Commit 1 carried R4's own `#[cfg_attr(not(test), expect(dead_code))]`** on `delete_repo`,
  `list_actions_secret_names`, the two new private list-response structs, and the five new
  `RepoBody` fields, since none has a non-test caller until commit 2's `GitHubRepoInverse` exists.
  Commit 2 removes all nine attributes in the same commit that makes every one of those items live --
  self-enforcing, since an `expect(dead_code)` on a now-live item is itself an `unfulfilled_lint_expectations`
  failure under `-D warnings`.
- **Verify 1 and 2** (this milestone's own "Verify before relying on them" list): fetched 2026-10-12,
  verbatim, from `github/rest-api-description`'s published `OpenAPI` description (`main` branch,
  `descriptions/api.github.com/api.github.com.json`). Confirmed: `GET
  /repos/{owner}/{repo}/actions/secrets` returns `{total_count, secrets: [{name, created_at,
  updated_at}, ...]}` (both envelope fields required) and accepts `per-page`/`page` query
  parameters; `GET /repos/{owner}/{repo}` carries `full_name`, `created_at`, `pushed_at`,
  `open_issues_count` and `forks_count`, all marked `required`; `DELETE /repos/{owner}/{repo}`
  documents `204`, `403`, `307`, `404` and `409` as response statuses. **Left open for L1** (this
  task made no live call): verify 1's other half, "`createCommitOnBranch` bumps `pushed_at`" -- not a
  fact the `OpenAPI` description states, only observable live.
- **No ratchet, tripwire or guard touched.** `rollback.rs`'s own count is unaffected (nothing here
  touches `willikins-core`). The three new `TeardownToken::new()` calls (one per `delete` test in
  `repo_inverse_mock.rs`) are each under `tests/it/`, exempt outright from `teardown_token_guard.rs`'s
  walk (it skips any `tests/` directory), and each carries its own `#[allow(clippy::disallowed_methods)]`
  per the clippy row's own instruction -- the same convention `SinkToken::new()` already uses
  throughout this crate's tests.

**Addendum:** 2026-10-12 — P2 landed (`doppler.project.ensure`'s inverse; decision 7's project row;
acceptance 12) in `crates/willikins-providers-doppler` alone, three commits (89e2083 the client,
c659109 a same-day fix, 5b6d8f3 the tool), scoped gates (`-p willikins-providers-doppler`: `cargo fmt
--check`, `cargo clippy --all-targets -D warnings`, `cargo test`) green for all three. Deviations and
choices, recorded:
- **`delete_environment` takes a `&DopplerConfigName`, not the `&EnvironmentSlug` trust boundary 3's
  own table names.** `create_environment` already sends a `DopplerConfigName` (the derived root
  config's own snake-joined name, `naming::v1::doppler_root_config`) as both `name` and `slug` when it
  creates an environment, so that is the string Doppler actually stores as the environment's own id --
  a multi-word environment's `EnvironmentSlug::Display` would print its kebab form (`pre-prod`)
  instead of the stored `pre_prod`, naming a different, nonexistent environment. `list_environments`'s
  own `EnvironmentEntry::id` reports the same string back, so a future caller (task P3) addresses an
  environment by exactly what it was listed as.
- **`inheritedBy` lives on the single-config `GET` (`DopplerClient::get_config`), never on the list
  endpoint** (`DopplerClient::list_configs`) -- confirmed by fetching `configs-list.md` verbatim
  2026-10-12: its own example and schema carry neither `inheritedBy` nor `inherits`/`inheritable`,
  only `name`/`root`/`locked`/timestamps/`environment`/`project`. So `standing_from_body` makes one
  `get_config` call per tolerated config, reading `inherits` (for `config_has_foreign_secret_name`'s
  own base walk) and `inherited_by` (decision 7's own row) off the same response, never two separate
  reads for the two fields.
- **A new verify item, not yet in the milestone's own numbered list**: `GET /v3/environments`'s
  reference (`environments-list.md`, fetched verbatim 2026-10-12) documents only a `project` query
  parameter, with no `per_page`/`page` in its example request, though its example response carries a
  bare `"page": 1`. `list_environments` sends `per_page`/`page` anyway (harmless either way: the loop
  still terminates on a short page whether or not Doppler honours them, just after more round trips if
  it does not), and Doppler's own documented environments-per-project cap (15, Team/Enterprise,
  "\*case-by-case" above that) means one page covers every project this crate provisions regardless.
  Left for L2 to confirm live, alongside the milestone's existing verify items.
- **`config_has_foreign_secret_name`'s one-level base walk tolerates a base this token can no longer
  see** (a `404`, or the `400` `looks_like_a_missing_project` already recognises), mirroring
  `DopplerSecretNameGate`'s own step 2 exactly rather than failing the whole project's standing with a
  transport error over one base that is no longer there. This was not how commit 1 first shipped it (a
  bare `?` on every base read); found and fixed same-day, test-first, before commit 2 — c659109.
- **`ConfigBody::inherited_by`'s entries are a raw `ConfigRefRaw { project: String, config: String }`,
  not the grammar-validated `ConfigRefBody` `inherits` already uses.** `inherits` is the document's own
  stated intent (`doppler.config.inherits.ensure` wrote it), so a malformed entry there is this crate's
  own bug; `inheritedBy` is whoever happens to depend on this config, never something this run asked
  for, so a foreign inheritor with a project name outside this crate's own grammar must still read as
  `Drifted` (decision 7's own reason), not an opaque "the response body did not parse" that leaves an
  operator guessing. `config_has_foreign_secret_name` and `inherited_by`'s own membership check both
  compare raw strings for exactly this reason.
- **`Gone` is read through `looks_like_a_missing_project`, not a bare `404`** — the only tool in this
  crate whose inverse needed to decide this at all (every other Doppler tool's own future inverse,
  task P3, inherits the same question). A `400` "no access" for a project that genuinely exists but
  sits outside this token's grant is read the same as `Gone`: safe (under-deletion: `delete` is never
  called on a resource this reading cannot prove is actually gone), the same reading every other read
  in this crate already gives the identical ambiguity.
- **`needs: &["project"]` alone.** Unlike `github.repo.ensure`'s `token`, this tool holds no optional,
  document-bound credential port — every call goes through the one client the tool was built with — so
  decision 5's resolver-chain machinery has nothing to resolve here.
- **Two fields stay deliberately dead**, documented on themselves for a later task rather than wired in
  on spec: `ConfigListEntry::created_at` (task P2's own drift rule, decision 7's project row, checks
  config membership only, never a config's own creation window — task P3's `doppler.config.ensure`/
  `doppler.branch_config.ensure` rows need it) and `ConfigRefRaw::config` (the project's own check
  compares only `inheritedBy`'s `project` field; naming *which* config outside the project inherits is
  not this row's own concern).
- **Verify item 6's mock-level answer**: a direct `get_project`/`get_config` unit test in `client.rs`
  parses Doppler's own documented millisecond shape (`2019-11-19T07:19:00.476Z`-style fractional
  seconds) through `Timestamp::parse`, confirming `chrono::DateTime::parse_from_rfc3339` already
  accepts it (RFC 3339 permits fractional seconds); `project_inverse_mock.rs`'s own happy-path test
  uses the same shape end to end, through `standing()`.
- **`DopplerClient::secret_names` now returns `SecretNamesBody` itself, not a bare `Vec<String>`**,
  shared by `secret_name_listed` and the new `config_has_foreign_secret_name` — milestone 3j's trust
  boundary 2 ("the list never leaves the client") holds for the shared path the same way it already
  held for the struct behind it: a type with no `Debug` is harder to leak by accident than a plain
  `Vec<String>`, which always has one.
- **Riders are read fail-closed throughout**: a rider whose own `config`/`name` port this crate cannot
  parse, or whose `config` *output* this crate cannot downcast (`doppler.config.ensure`/
  `doppler.branch_config.ensure`, which report their identity only through an output port), tolerates
  nothing rather than being guessed at. No Doppler tool declares a real `Reversal`/`Within` of its own
  yet (task P3), so every rider-tolerance test in `project_inverse_mock.rs` builds its own `Rider` by
  hand, the same way milestone 3m's own `repo_inverse_mock.rs` did before P1 landed `github.repo.ensure`'s
  `Within` declarations.

**Addendum:** 2026-10-12 — P3 landed (`doppler.config.ensure`'s, `doppler.branch_config.ensure`'s and
`doppler.service_token.ensure`'s inverses; decision 7's three remaining per-tool rows; the
`Contained`/`Retained` declarations of the other five non-pure Doppler tools; acceptance 12) in
`crates/willikins-providers-doppler` alone, four commits (0a2c9ec the hoist, ba54da9 the client, c48d897
the three inverses, 3bf53f1 the five declarations), scoped gates (`-p willikins-providers-doppler`:
`cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test`) green after each and at the
end. Deviations and choices, recorded:
- **Hoisted P2's own per-config drift walk before writing a second and third copy.** `config_output`,
  `tolerated_secret_names`, `tolerated_token_names` and the `get_config` → `config_has_foreign_secret_name`
  → `inherited_by` → `list_service_tokens` body (new `inverse_support::config_contents_drift`, returning a
  `ConfigContentsDrift` enum so each caller keeps its own `&'static str` reason text) moved out of
  `project_ensure.rs` into a new crate-private `inverse_support` module, reused by `config_ensure.rs`
  (scoped to one environment's configs) and `branch_config_ensure.rs` (scoped to the one config). Proved
  behaviour-preserving by running `project_inverse_mock::` before and after: all 50 tests passed
  unmodified, since they match on `Standing`'s variant alone (`Standing::Drifted { .. }`), never its `what`
  text.
- **`doppler.config.ensure`'s own window check reads the matching `ConfigListEntry::created_at`, not a
  new field on `EnvironmentEntry`.** Decision 7's row says "the environment's own creation date", but
  Doppler's environments-list response (fetched verbatim 2026-10-12, `environments-list.md`) does carry a
  `created_at` per entry — this task chose *not* to add it to `EnvironmentEntry` anyway: P2's own addendum
  had already carried `ConfigListEntry::created_at` specifically "so a later task... needs no second
  client change", and an environment's root config is created in the same `POST` as the environment
  itself (`create_environment`'s own doc), so the two timestamps are the same instant in practice. Using
  the entry the membership walk already fetches avoids a second client method and a second round trip.
  Recorded here since the row's literal wording and the implementation's own data source are not the
  same field.
- **`doppler.config.ensure`'s membership and contents walk is scoped to the environment, not the whole
  project** (decision 7's own "scoped to the environment"): `list_configs(project)` is still one call over
  every config in the project (no narrower endpoint exists), but only entries whose own `environment`
  field equals the target's derived root name are read for drift at all — a `dev` config is never
  consulted while rolling back `prd`'s own root
  (`standing_never_reads_a_config_in_another_environment`). The comparison uses the raw, already-prefixed
  id `list_configs`/`create_environment` actually store (the same string `naming::v1::doppler_root_config`
  derives), never `EnvironmentSlug::Display`'s kebab form — unlike `branch_config_ensure.rs`'s own
  pre-existing `observe` (`body.environment.as_deref() == Some(environment.to_string())`), a latent
  divergence for a multi-word environment this task did not touch or fix, since it predates P3 and no
  acceptance test here exercises a multi-word environment's own teardown.
- **`doppler.branch_config.ensure`'s own `within` names both `doppler.project.ensure` and
  `doppler.config.ensure`**, exactly as decision 4's table row lists them: a branch config rides with
  whichever container this run actually deletes, the project or just its own environment.
- **Gone is read by absence from `list_configs`/`list_service_tokens`, for all three new inverses, never
  by a dedicated single-resource `GET`** (Doppler has none for an environment or a token). Each inverse's
  own `standing` treats "the project/config is ours but this resource is no longer listed" the same as a
  missing project: `Standing::Gone`, never `NotOurs` — the same universal "already gone, skip it" reading
  decision 6 gives every other absent resource, and the one reading under which decision 8's resume case
  (a second teardown of the same run, after this step's own delete already succeeded) still converges.
- **`doppler.service_token.ensure`'s own zero-match case reads `Gone`, not `NotOurs`, resolving an
  ambiguity in decision 7's own row text.** The row says `NotOurs` when "no token at `(config, name)`
  created in the window", which literally covers both "a token exists here but outside the window" and "no
  token exists here at all". Doppler's service token API has no single-token `GET` to disambiguate the
  way `github.repo.ensure`'s and `doppler.project.ensure`'s own 404s already do, only the list. This task
  reads the zero-match case as `Gone` (consistent with every other inverse's own "absence is gone" rule
  above, and required for resume to work for this tool too: after this step's own `delete` succeeds, the
  token is no longer listed, so a second teardown plan of the same run must not refuse over its own prior
  success) and reserves `NotOurs` for "a token by this name exists but its own `created_at` falls outside
  the window" (a name this run's own token happens to share with something created at a different time).
  `locate`, a private helper shared by `standing` and `delete`, resolves both the `Standing` and the
  slug `delete` needs from one list read, since no `Standing` variant can carry the slug itself.
- **No inverse declares a loss** (all three `losses: &[]`): decision 4's table names a loss only for
  `buildkite.pipeline.ensure` (task P4). Every `delete` still writes the `losing.iter().all(...)` check
  against `created.accept` the documented `Inverse::delete` contract names, vacuously true today, the
  same choice P1/P2 already made for their own loss-free tools.
- **The five `Contained`/`Retained` declarations add no inverse**: `Catalog::insert`'s "`Delete` exactly
  when `inverse()` is `Some`" rule (task R3) is untouched. `doppler.config.inheritable.ensure`,
  `doppler.config.inherits.ensure` and `doppler.secret.set` each ride with either config tool (own `config`
  input → either tool's `config` output); `doppler.project_member.ensure` rides with
  `doppler.project.ensure`'s own `project` input; `doppler.service_token.rotate` is `Retained` with decision
  4's own exact wording ("the revoked token cannot be restored").
- **No ratchet, tripwire or guard touched.** Nothing here changes `willikins-core`. Every `TeardownToken::new()`
  call (three per inverse's own `delete` tests) is under `tests/it/`, exempt from `teardown_token_guard.rs`'s
  walk, each carrying its own `#[allow(clippy::disallowed_methods)]`.
- **`delete` re-observes via a full `standing` call, the same contract P1/P2's own inverses honour** ("observes
  first, as `ensure` does", `Inverse::delete`'s own doc): by the time any of these three inverses' `delete`
  runs, `apply_teardown` (R6) has already re-planned and compared fingerprints once; `delete` then repeats
  the entire walk a third time (one project `GET`, one `list_configs`, and one `get_config`/`secret_names`/
  `list_service_tokens` triple per tolerated config, for `config.ensure`'s own environment-wide case).
  Correct, not a bug, and the same cost P2's own project-wide inverse already pays — recorded here only so
  a reader of `rollback.rs`'s own apply-time cost is not surprised that `config.ensure`'s `delete` on an
  environment with several branch configs issues that many requests for one `DELETE`.
- **Left for P6/P7**: no Doppler tool is yet checked against `LIVE_TOOL_NAMES`'s own table test or the
  fake catalog's own parity test (both later tasks); `catalog_check_parity.rs`/`catalog_parity.rs` in this
  crate exercise only `ToolSpec` and `check`'s own type resolution, neither of which reads `Reversal` at
  all, so this task's own declarations needed no change there.

**Addendum:** 2026-10-12 — P4 landed (`buildkite.pipeline.ensure`'s inverse and the builds read; `bootstrap.ensure`'s
`Contained`; decision 4/7's buildkite rows; acceptance 12, 21's provider half) in `crates/willikins-providers-buildkite`
alone, three commits (the client's builds read and standing read, the tool's `Delete`/`Inverse`, then the bootstrap
tool's `Contained`), scoped gates (`-p willikins-providers-buildkite`: `cargo fmt --check`, `cargo clippy --all-targets
-D warnings`, `cargo test`) green after each and at the end. Deviations and choices, recorded:
- **Verify item 8, half-settled by a primary source fetched verbatim, half left to L3.** Buildkite's own documented
  example (`buildkite.com/docs/apis/rest-api/pipelines.md`, fetched 2026-10-12) confirms a pipeline's own `created_at`
  exists, but renders it as `"2013-09-03 13:24:38 UTC"` — a space, not a `T`, and a literal `UTC` suffix, **not** RFC
  3339 — unlike the *nested* `created_by.created_at` in the same example, which is RFC 3339
  (`"2013-08-29T10:10:03.000Z"`). `willikins_core::Timestamp`'s hand-written `Deserialize` is strict RFC 3339 only, by
  design, and would hard-fail deserializing this field on every real response. `parse_pipeline_created_at` tries RFC
  3339 first (in case a live pipeline ever actually returns one) and Buildkite's own documented form second, failing
  closed (`None`, read as "outside the window") on anything else. Which form a live pipeline actually sends is left to
  L3; whichever it is, both branches already exist and are tested. The builds endpoint's own list response is
  confirmed bare (`builds.md`'s own "List builds for a pipeline" example: a top-level JSON array, not an envelope), so
  `has_any_build`'s `Vec<serde::de::IgnoredAny>` deserialize is correct against the documented shape, not merely
  convenient.
- **`PipelineStandingBody` is a fourth, narrower response type**, not a seventh field on `PipelineBody`: this crate's
  own precedent (`PipelineConfigurationBody`, `PipelineBootstrapBody`) is one response type per caller, and
  `PipelineBody`'s own "exactly six fields" compile-time pin (`pipeline_body_still_has_exactly_six_fields`) would
  otherwise need touching for a field no other caller reads. Carries `slug`, `description`, `created_at` (the last as
  a raw `Option<String>`, parsed by `parse_pipeline_created_at` at the call site, never inside `Deserialize` itself,
  so a parse failure is an ordinary `None`, never a whole-response deserialize error).
- **`chrono` added to this crate's own `[dependencies]`** (previously absent; `willikins-providers-doppler` already
  carries it), needed only for `parse_pipeline_created_at`'s own second, non-RFC-3339 parse.
- **`needs` carries `token` alongside the two key ports** (`&["org", "slug", "token"]`), mirroring
  `github.repo.ensure`'s own `reversal()`: decision 5 names this tool's `token` port as its own credential example, so
  a document-bound `token` is re-resolved for the teardown exactly as `read`/`ensure` resolve it for a forward run.
- **`standing_from_body`'s own check order is decision 7's row order literally** (description, then the window, then
  the slug), not the order an earlier draft of this task used (description, slug, window) — reordered before landing
  so a reader of the row and a reader of the code never have to reconcile two orders.
- **`DELETE_REFUSED_NOT_AS_CREATED` is the first copy of this message whose loss clause is non-vacuous.** Every other
  `Delete` tool in this workspace declares no loss at all, so their own copy's "or has changed" half never actually
  distinguishes drift from an unaccepted loss at the message level; this tool's own copy names both ("is no longer
  ours, or has a build this node's policy does not accept") since `delete_refuses_as_a_conflict_when_a_build_exists_and_is_not_accepted`
  is the first test in this workspace to exercise that branch for real.
- **`created.riders` is deliberately never read** by `standing_from_body`, called out in its own doc comment: decision
  7 gives `buildkite.pipeline.ensure` no contents rule of its own (unlike `github.repo.ensure`'s Actions secrets or
  `doppler.project.ensure`'s configs) — a pipeline's only reported loss is its builds, which this run's own riders
  could never have written in the first place.
- **The commit-1/commit-2 `#[cfg_attr(not(test), expect(dead_code))]` dance** on `get_pipeline_standing`,
  `has_any_build`, `PipelineStandingBody`, and `parse_pipeline_created_at` follows P1's own precedent exactly: commit 1
  adds all four under the attribute (no non-test caller yet), commit 2 removes all four in the same commit that makes
  `BuildkitePipelineInverse::standing` their first real caller.
- **No ratchet, tripwire or guard touched.** Nothing here changes `willikins-core`. The three `TeardownToken::new()`
  calls in `pipeline_inverse_mock.rs` are under `tests/it/`, exempt from `teardown_token_guard.rs`'s walk, each
  carrying its own `#[allow(clippy::disallowed_methods)]`.
- **Left for P6/P7**, same as every other provider task: no Buildkite tool is yet checked against `LIVE_TOOL_NAMES`'s
  own table test or the fake catalog's own parity test; `catalog_parity.rs` in this crate exercises only `ToolSpec`
  (unaffected, since `reversal()` is not part of it) and stays green unchanged.

**Addendum:** 2026-10-12 — P5 landed (`appstore.bundle_id.ensure`'s, `appstore.bundle_id_capability.ensure`'s and
`signoz.ingestion_key.ensure`'s `Retained` declarations; `appstore.profile.ensure`'s `Delete` and inverse; decision 4's
appstore/SigNoz rows; decision 7's appstore row; acceptance 20; verify 10) in `crates/willikins-providers-appstore` and
`crates/willikins-providers-signoz`, two commits (the three `Retained` declarations, then the profile inverse), scoped
gates (`-p willikins-providers-appstore -p willikins-providers-signoz`: `cargo fmt --check`, `cargo clippy
--all-targets -D warnings`, `cargo test --test it`) green after each. Deviations and choices, recorded:
- **Verify item 10, settled, fetched verbatim.** Apple's published `OpenAPI` description
  (`developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip`, fetched
  2026-10-12) confirms `Profile.attributes.createdDate` (`format: date-time`) and
  `Profile.relationships.bundleId.data.id` both exist; neither is marked `required`, so the inverse's own `standing`
  treats either one's absence the same as an unparseable value -- fail closed, `NotOurs`, not "no opinion" (the same
  reading decision 7's other rows already give a missing field). The row's own fallback ("if the profile carries no
  creation date, P5 stops and reports") was not needed.
- **The sparse standing read's own query is `fields[profiles]=name,createdDate,bundleId`, no `include=`.** The
  `bundleId` relationship's own linkage (`data.id`) is already present on an ordinary sparse-fields read -- `include=`
  only side-loads the *related resource's own attributes*, which this read never needs, unlike
  `AppstoreClient::get_profile`'s own `include=certificates`.
- **The first inverse in this workspace to read `Created::outputs` rather than only `Created::inputs`.** Every other
  `Delete` tool addresses its resource by a key port that is also an *input* (a repository's owner/name, a pipeline's
  org/slug, a Doppler project's name); a profile is addressed by App Store Connect's own opaque id, which is this
  tool's `profile` *output*, never a `needs` port (decision 3's own struct already makes `Created::outputs` available
  with no declaration required -- this is the first task to actually read it). `needs` therefore names only the three
  credential ports and the two ports `standing` compares (`identifier`, `name`), never `profile`.
- **The bundle id lookup inside `standing_from_body` reuses `AppstoreProfileEnsure::find_bundle_id` directly**
  (module-private, same file), rather than a second copy of its byte-exact filter logic: the inverse's own
  `AppstoreClient` is already in hand from `standing`'s own call, so no second client needs building.
- **No loss is ever declared or reported** (`losses: &[]`, `Standing::AsCreated { losing: Vec::new() }` on every
  success): decision 7's own row says a profile has no contents, so unlike `GitHubRepoInverse`'s or
  `BuildkitePipelineInverse`'s own copy, `DELETE_REFUSED_NOT_AS_CREATED`'s loss clause here is always vacuously true --
  kept anyway, for the same shape every other inverse's `delete` already uses, rather than special-cased away.
- **`ProfileStandingResource` carries no `id` field.** The caller already knows it (the id this read was addressed
  to); carrying one unread field was flagged dead code by the plain `lib` build and removed rather than silenced.
- **Nineteen mock tests**, in a new `tests/it/profile_inverse_mock.rs`: the exact standing query, `Gone` on `404`,
  `NotOurs` on a name mismatch, a bundle id mismatch (wrong id and a missing relationship outright), outside the
  window (and missing `createdDate` outright), the `reversal()` shape, and `delete`'s three outcomes (already gone,
  deletes, refuses as `Conflict` with no `DELETE` issued at all -- trust boundary 6).
- **`appstore.bundle_id.ensure`'s and `appstore.bundle_id_capability.ensure`'s own "why" strings are decision 4's table
  text verbatim**, copied character for character rather than paraphrased, per the row's own instruction.
- **No new live App Store call, as the row requires.** No live-tests target, no manual try; the existing live write
  cycle (which already deletes its own throwaway profiles by hand, outside rollback) is untouched.
- **Left for P6/P7**, same as every other provider task: `appstore.profile.ensure` is not yet checked against
  `LIVE_TOOL_NAMES`'s own table test or the fake catalog's own parity test; `catalog_parity.rs` in both crates
  exercises only `ToolSpec` (unaffected, since `reversal()` is not part of it) and stays green unchanged.

## Goal

Given one recorded run, willikins computes a **teardown plan** that deletes exactly what that run created and nothing
else, shows it the way it shows any plan, waits for a human to approve it, and runs it only if a fresh observation
still agrees with what was approved. Concretely:

1. **Each tool declares what it can undo** (decision 3): a typed inverse that deletes one resource, "rides out with
   its container", "cannot be undone, because …", or nothing at all. The declaration is static, like a tool's class,
   and states capability only. A tool that declares nothing is never undone.
2. **Each node declares whether to undo it** (decision 3a): `teardown: retain | delete` in the document, validated by
   `check` against the tool's capability. A node that declares nothing is kept. Policy is the document's, never the
   tool's.
3. **A teardown plan is computed from the journal** (decisions 1, 2, 5, 6): the run's planned actions and node
   outcomes say what it created, the journal's own timestamps bracket when, and the document at its recorded hash
   re-resolves the credentials the deletes need. Steps run in reverse of the applied plan's order.
4. **Drift refuses the whole teardown** (decision 7): before anything is deleted, every candidate the document says
   to delete is observed. A resource that is gone is skipped; one that is not willikins', that was created outside
   this run's window, or that has changed since (a push, a secret nobody in this run wrote, a config someone else
   inherits) refuses the teardown before any delete. So does a declared loss (a pipeline's builds) the node's policy
   does not accept.
5. **Approved like any plan** (decision 8): a teardown is always `Destructive`, always needs a human, is journaled,
   and is re-observed and compared step by step before its first delete.
6. **The forward plan says what a rollback would not undo** (decision 10), before anything runs: what the tool
   cannot undo, and what the document keeps.
7. **The CLI drives it** (decision 9): `willikins rollback`. MCP is out of scope (below).

This milestone covers the todo's three parts: undoing pure creates (Delete), per-tool inverses or their declared
absence (Contained, Retained), and the reverse of a gate (Made by hand).

## The operator's words (2026-09-29)

"If all this works, all we need is deterministic safe rollback, and we'll have basically reinvented CloudFormation."

The todo's own framing stands: willikins keeps no stack state and spans several vendors, so rollback cannot mean
"restore a snapshot". It means converging back, from the journal and from reads of reality, with the plan showing
exactly what it will delete and what it cannot undo.

## Out of scope

- **Undoing an attribute change or an overwrite.** Rollback deletes things this run created. It never restores a
  prior value: a rotated token, an overwritten secret, a raised member role, a replaced profile, a rewritten pipeline
  configuration, or an inheritance set extended in place. Those are `Retained` with a reason (the per-tool table).
- **The MCP surface.** An agent-callable `rollback_plan` is safe (the plan is always approval-gated), but the
  approvals page and `run_status` need a teardown-shaped rendering first. A follow-up todo, written by task D1.
- **Rolling back across journals.** The CLI and a server keep separate journals. A teardown reads one.
- **SigNoz.** Its sandbox key expired on 2026-09-23, so no delete can be proven live. `signoz.ingestion_key.ensure`
  declares `Retained` until a live cycle can run (decision 4).
- **Any rollback of the operator's real runs.** That is the operator's act, from their own journal, after reading the
  teardown plan. No agent plans or applies one.
- **A live App Store delete.** The App Store Connect account willikins reaches is the operator's live account.
  `appstore.profile.ensure`'s inverse is written and tested against mocks only; no new live call, no L-row, and no
  agent ever runs it. The existing live write cycle, which already deletes its own throwaway profiles, is unchanged. It runs only when the operator rolls back their own run whose document says `teardown: delete`
  on a profile node.
- **Overriding a used document's policy.** A `uses:` step cannot set `teardown:` on the nodes it brings in; the used
  document's own nodes declare theirs.
- **Restoring something the run deleted.** `appstore.profile.ensure`'s Replace deletes an INVALID profile first; it
  cannot come back.

## Trust boundaries (normative)

1. **No inverse is a catalog tool.** Nothing named `*.delete` is registered, documented in the schema, or nameable in a
   workflow document. An inverse is reachable only from the teardown executor, with facts read from the journal. A
   delete node a document could name is the open step the design forbids: an agent could delete any repository its
   credential can reach.
2. **A delete takes a `TeardownToken`.** `TeardownToken::new` exists only behind `willikins-types`' `executor` feature,
   is disallowed by `clippy.toml` everywhere except `willikins-core/src/rollback.rs`'s executor, and is never passed to
   `Tool::read`, `Tool::ensure` or `Inverse::standing`. It is the `SinkToken` mechanism, unchanged.
3. **Typed deletes, never paths.** Each new client method takes domain types (`&GitHubRepo`, `&DopplerProject`,
   `&DopplerProject` + `&EnvironmentSlug`, `&DopplerConfig`, `&BuildkiteOrg` + `&BuildkitePipelineSlug`) and builds its
   own path. No inverse takes a URL, a path or a `Text`.
4. **Only what this run created** (decision 1). An instance the run found `NoOp`, `Unchanged` or `Converged` is never
   a candidate, nor is anything another run created.
5. **No value in a reason.** Every drift and refusal reason is a `&'static str`, like `Gate::need`. A teardown step
   names its subject only through non-secret `Value`s, which every output surface masks or redacts as usual.
6. **All reads before any delete.** As `deploy/teardown.sh` did: the teardown plan observes every candidate whose node
   says `teardown: delete` and that is not itself a rider (decision 6), and the executor re-observes all of them before
   the first delete; each `delete` observes once more on its own. A rider is covered by its container's observation,
   whose drift rule must account for it (decisions 3, 7). A kept or `Retained` step is never observed.
7. **A secret is never journaled for a teardown.** Credentials are re-resolved from the document's pure resolver
   nodes at teardown time (decision 5). They are never read back from the journal, which holds only redaction markers
   for them.
8. **Policy is the document's, capability is the tool's.** A tool's declaration never decides whether to delete;
   only a node's `teardown:` does, and a node that declares nothing is kept. The teardown reads policy from the
   document at the run's recorded hash (and its recorded used closure), so a policy cannot be changed after the run:
   flipping a node to `delete` later changes the hash and the teardown refuses with `DocumentChanged`. This is
   intended. The forward plan's `not_reversible` showed the policy when the run was approved.

## SHARED VALUES

| Name | Value | Where |
| --- | --- | --- |
| Core module | `crates/willikins-core/src/rollback.rs` (re-exported as `willikins_core::rollback`) | R1, R3, R5a, R5b, R6 |
| Declaration method | `fn reversal(&self) -> Reversal` on `Tool`, default `Reversal::Retained { why: UNDECLARED }` | R1 |
| `UNDECLARED` | `"this tool declares no reversal"` | R1 |
| Inverse method | `fn inverse(&self) -> Option<&dyn Inverse>` on `Tool`, default `None` | R3 |
| `Reversal` variants | `Delete { needs, within, losses }`, `Contained { within, otherwise }`, `Retained { why }` | R1 |
| `losses` | `&'static [&'static str]`: what a delete would destroy that a run never makes. Only `buildkite.pipeline.ensure` declares one: `"builds"` | R1, P4 |
| Policy type | `willikins_core::workflow::TeardownPolicy` = `Retain` \| `Delete { accept: Vec<Loss> }`; `Loss` a validated name (`^[a-z][a-z_]*$`); `Node.teardown: Option<TeardownPolicy>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`; `CheckedNode.teardown: TeardownPolicy` (`None` becomes `Retain`). The teardown **plan** type is `rollback::Teardown` | T1 |
| YAML | `teardown: retain`, `teardown: delete`, or `teardown: { delete: { accept: [builds] } }`; refused on a `uses:` step | T1 |
| New `CheckError`s | `TeardownNotAdmitted { node, tool }` (any `teardown:` on a tool whose reversal is not `Delete`); `TeardownLossNotDeclared { node, loss }` | T1 |
| `KEPT` | `"the document keeps it (no teardown: delete)"`; `"its container is kept by the document"` for a rider of a kept container | R5a, R5b |
| `Within` | `{ tool: &'static str, ports: &'static [(&'static str, Side, &'static str)] }`, `Side` = `Input` \| `Output` | R1 |
| `Standing` variants | `AsCreated { losing: Vec<&'static str> }` (each a declared loss, empty when none), `Gone`, `NotOurs { why }`, `Drifted { what }` (both `&'static str`) | R3 |
| Step fingerprint | `StepFingerprint { node, instance, action, standing: Option<StandingKind> }`, typed, no values; `StandingKind::AsCreated` carries its `losing` names; `None` for an unobserved step | R5a, R7 |
| `TeardownAction` variants | `Delete { uncertain: bool }`, `RidesWith { container: InstanceRef }`, `Gone`, `Kept { why }` (the document's policy), `Retained { why }` (the tool's capability), `MadeByHand { need }`, `Unfinished` | R5a, R5b |
| Token | `willikins_types::TeardownToken` in `crates/willikins-types/src/sink.rs`, `executor` feature | R2 |
| `CREATION_SKEW` | 120 seconds, `pub const` in `rollback.rs` | R5a, P1–P4 |
| Teardown class | always `Class::Destructive`; `requires_approval` always `true` | R5a, R8 |
| Journal events | `TeardownPlanned`, `TeardownStarted`, `TeardownStepFinished`, `TeardownFinished` | R7 |
| New `ApplyRefusedReason`s | `AlreadyRolledBack { run_id }`, `LaterRunFirst { run_id }`, `NotAForwardRun`, `RunUnfinished` | R7 |
| New `DriftReasonKind` | `Standing` | R7 |
| CLI | `willikins rollback <file> --run <run-id> --journal <path> [--live \| --fake-state F] [--fake-state-out F]`; `willikins rollback --plan-id <id> --journal <path> [...]` | R9 |
| Live probe prefix | `willikins-probe-delete-me-` (`willikins_probe_delete_me_` where `-` is refused) | L1–L3 |
| Forward plan field | `Plan::not_reversible: Vec<NotReversible>`, `#[serde(default, skip_serializing_if = "Vec::is_empty")]`; `NotReversible { node, instance, tool, by: Keeper, why }`, `Keeper` = `Tool` \| `Document` | F1 |
| Core signatures (after 3n) | `resolve_pure(checked, inputs, wanted)`, `plan_teardown(facts, checked, resolved)`, `apply_teardown(...)` take `&Checked` and no `&Catalog`; tool handles come from `CheckedNode`, the registry from `Checked` | R4-R6 |

## Decisions

### 1. What "this run created" means: two journal facts, joined

`NodeStatus::Created` means only that `ensure` reported `changed: true` (`apply.rs`). An instance planned `Update` or
`Replace` finishes `Created` as well, and `doppler.secret.set` and `github.actions_secret.ensure` report it on every
apply. So `Created` alone does not mean "created".

**An instance is a candidate when its `PlanRecorded` action is `Create`** (from the run's own plan, joined by node and
`for_each` instance) **and its outcome is one of these:**

| Planned | `NodeFinished` | Teardown treats it as |
| --- | --- | --- |
| `Create` | `Created` | created by this run: the node's policy decides, then (under `teardown: delete`) the inverse's standing |
| `Create` | `Failed` | **uncertain**: the create may have landed before the failure (milestone 3c's 201-with-no-data orphan). Under `teardown: delete`, a `Delete` tool's standing decides: `AsCreated` gives `Delete { uncertain: true }`, shown apart; `Gone` gives `Gone`. Otherwise `Kept`, never observed |
| `Create` | `NodeStarted` with no `NodeFinished`, in a run that has its `RunFinished` (a journal write that failed mid-run; a run with no `RunFinished` is refused whole, decision 8) | `Unfinished`: no window end exists, so the teardown names it as a manual check and never deletes it |
| `Create` | `Unchanged` | the resource existed by the time `ensure` ran (a new Doppler project's auto-created `dev`/`stg`/`prd` root config, HANDOFF's 2026-09 live record). Not this node's creation, but it **may ride** with a container (decision 3) |
| `Create` | `NotRun`, `Skipped`, `Blocked` | nothing was created |
| `Replace`, `Update` | any | `Retained` ("changed in place, or replaced; the prior state cannot be restored") |
| `NoOp`, `Compute` | any | not this run's doing; never listed, except gates (decision 4) |

**Why not "status `Created` plus a marker":** a marker proves willikins made a resource, not which run made it. A
second document can converge on the same key.

### 2. The creation window: the journal already timestamps it

Every journal entry carries `at`. For a candidate, the window is `[NodeStarted.at − CREATION_SKEW, NodeFinished.at +
CREATION_SKEW]`. A resource whose provider-reported `created_at` falls outside it is `NotOurs` ("created outside this
run's window"): someone deleted and recreated it, or it predates the run. `settled_by` is the run's `RunFinished.at +
CREATION_SKEW`. A container whose change timestamp (GitHub's `pushed_at`) is later than that has changed since the run.

**Why 120 seconds:** provider clocks and this host's clock are independent. Two minutes is wider than any skew a
synced clock shows, and narrower than the gap between two human-paced runs. A provider that reports no `created_at` for
a resource cannot get a `Delete` inverse (decision 4 lists which do).

### 3. How a tool declares its reversal

A default trait method in the manner of `gate()`, `replaces()` and `updates()`: no `Observation` variant (tool.rs
explains the cost) and no change to any existing tool until it opts in.

```rust
pub enum Reversal {
    /// `inverse()` deletes this resource. `needs` names the input ports the delete needs (key ports and credential
    /// ports). `within` names the containers it rides out with when this run deletes one of them too. `losses` names
    /// what a delete would destroy that a run never makes (decision 3a); a node's policy may accept them.
    Delete { needs: &'static [&'static str], within: &'static [Within], losses: &'static [&'static str] },
    /// No delete of its own: it goes when a container this run created goes, and is otherwise left in place for
    /// `otherwise`.
    Contained { within: &'static [Within], otherwise: &'static str },
    /// Never undone by rollback, for `why`.
    Retained { why: &'static str },
}

pub trait Inverse: Send + Sync {
    /// Observe the resource `created` describes. No token: this runs at teardown plan time.
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError>;
    /// Delete it. Observes first, as `ensure` does: anything but `AsCreated` is `Conflict`, so is `AsCreated` with a
    /// loss not in `created.accept`, and `Gone` is `Ok(Deleted { already_gone: true })`.
    fn delete(&self, created: &Created<'_>, token: &TeardownToken) -> Result<Deleted, ToolError>;
}

pub struct Created<'a> {
    pub inputs: &'a Inputs,          // exactly `needs`: non-secret from the journal, secret re-resolved (decision 5)
    pub outputs: &'a Outputs,        // the recorded non-secret outputs; secret ones Unknown
    pub window: CreationWindow,      // decision 2
    pub settled_by: Timestamp,       // decision 2
    pub riders: &'a [Rider<'a>],     // tolerated contents: riders that finished Created, or Unchanged in transit
    pub accept: &'a [Loss],          // the node's accepted losses; empty under a plain `teardown: delete`
}
```

A rider carries only its journal-recorded non-secret values, re-parsed by declared type. Its secret ports are
`Value::Unknown`, and no rider port is ever added to `resolve_pure`'s `wanted`.

`Catalog::insert` validates a declaration as it validates a `Gate`, on the tool's own side only (it sees one tool at a
time): every `needs` port and the left side of every `within` pair names one of the tool's own input ports, `Delete`
holds exactly when `inverse()` is `Some`, every loss name matches `^[a-z][a-z_]*$`, and a pure tool's `reversal()` is
the default (`Retained` with `why == UNDECLARED`). The container side (the container tool exists, the mapped port
exists on the stated `Side`, the types are equal) is checked by P7's table test over `LIVE_TOOL_NAMES` and by P6 over
the fake catalog. A `Within` names its container by tool name. A test over
`LIVE_TOOL_NAMES` checks that each one names a registered tool and that the ports it maps have the same type.

**Containment is typed equality on recorded values, over every instance of the run.** An instance rides with a
container instance when the container's tool matches the `Within`, the container is itself being deleted or rides
with something being deleted, and every mapped port pair holds equal `Value`s in the journal (the `TypeRef` and the
full recorded value). Riding is computed over **all** of the run's instances, whatever their action or status, so a
non-candidate can be a **transit rider**. A new project's auto-created `prd` config finishes `Unchanged`, yet a
`doppler.secret.set` into it must still ride with the project. Without transit, that secret would be `Retained`, and
the project's own drift rule would then refuse the whole teardown over a name this run wrote. A transit rider is listed
as `RidesWith`, since it does go with its container. The mapping is deterministic and needs no graph. When it misfires
it can only cause an under-deletion: a rider is never deleted on its own, and the container's own drift check
(decision 7) must account for every rider.

**Riding is not tolerance.** Every matching instance is listed `RidesWith`, but a container's inverse tolerates (in
`Created.riders`) only riders that actually wrote or found something: finished `Created`, or `Unchanged` in transit. A
rider that `Failed`, was `NotRun`, `Skipped`, `Blocked` or is `Unfinished` wrote nothing this run can vouch for, so
contents at its names are foreign and refuse (an operator who set a secret by hand after a half-failed run sees the
refusal, not a silent delete).

**Why `riders` reach the container's inverse:** a container must tolerate exactly what its own run put inside it.
A new Doppler project holds the configs and secret names this run's riders created, plus Doppler's three default
environments, and nothing more.

### 3a. Teardown policy is the document's

The operator, 2026-10-05: "Shouldn't this be a document decision?" A tool's `Reversal` says what it **can** undo. A
node's `teardown:` says whether this document **wants** it undone.

```yaml
steps:
  repo:
    tool: github.repo.ensure
    teardown: delete                         # delete on rollback; refuse on drift
  pipeline:
    tool: buildkite.pipeline.ensure
    teardown: { delete: { accept: [builds] } }   # delete even when it has builds
  profile:
    tool: appstore.profile.ensure
    teardown: retain                         # the default, written out
```

- **The default is `retain`.** A node with no `teardown:` is kept. Nothing is deleted unless a document asked for it
  in writing, before the run.
- **`check` validates policy against capability.** `teardown:` (either value) is admitted only on a node whose tool
  declares `Reversal::Delete`; anywhere else it is `CheckError::TeardownNotAdmitted`. A `Contained` node goes with its
  container whatever it says, and a `Retained` one cannot be deleted, so a `teardown:` there would be a promise the
  engine cannot keep. Each `accept` entry must name one of the tool's declared `losses`, or it is
  `CheckError::TeardownLossNotDeclared`. Both are static, so `check` refuses them, never `plan`.
- **Losses are not drift.** Drift (`NotOurs`, `Drifted`) means the resource is not what this run made, and always
  refuses. A loss is something a delete would destroy that the run itself never makes but the resource accumulates
  by design: a pipeline's builds. The inverse reports a loss in `Standing::AsCreated { losing }`; the step deletes
  only if every reported loss is in the node's `accept`, and otherwise refuses the teardown naming the loss. A loss
  the tool reports but does not declare is treated as `Drifted` (fail safe).
- **Riders follow their container's policy.** A `Contained` rider, or a non-candidate transit rider, rides only
  with a container whose step is `Delete`; a rider of a kept container is `Kept { why: "its container is kept by the
  document" }`. A `Delete`-capable candidate (a config inside a project, a token inside a config) that matches a
  container being deleted must itself say `teardown: delete`, and is then `RidesWith`. If it is kept (explicitly or
  by default), the teardown refuses with `"a node the document keeps is inside a container it deletes"`: the
  document cannot keep something inside a container it deletes, and silently deleting it would break the default.
  A `Delete`-capable candidate under `teardown: delete` whose container is kept is its own `Delete` step.
- **Composition.** A used document's nodes carry their own `teardown:` through the linker unchanged. A `uses:` step
  has no `teardown:` (the DSL refuses the field there), so a parent cannot widen a used document's deletes.
- **Policy is fixed at the run.** See trust boundary 8.

### 4. The per-tool table (capability only)


Eighteen non-pure live tools and seven gates. "Within" lists the containers each one rides with. Every column is
**capability**: what the tool can undo and why not. Whether a node's resource is undone is its document's
`teardown:` (decision 3a), never this table.

| Tool | Class | Reversal | Inverse (typed delete) | Within | Why |
| --- | --- | --- | --- | --- | --- |
| `github.repo.ensure` | Reversible | **Delete** | `GitHubClient::delete_repo(&GitHubRepo)` | none | marker topic, `created_at`, no later push |
| `github.actions_secret.ensure` | Reversible | Contained | none | repo (`repo`→`repo`) | always writes, prior value unknowable |
| `github.scaffold.ensure` | Irreversible | Contained | none | repo (`repo`→`repo`) | a commit on a branch (on a new, empty repository the first file goes through the Contents API, unsigned; milestone 3l). A revert is a new forward change the document must make |
| `doppler.project.ensure` | Reversible | **Delete** | `delete_project(&DopplerProject)` | none | marker description, `created_at`, contents (decision 7) |
| `doppler.config.ensure` | Reversible | **Delete** | `delete_environment(&DopplerProject, &EnvironmentSlug)` | project (`project`→`project`) | a root config goes only with its environment |
| `doppler.branch_config.ensure` | Reversible | **Delete** | `delete_config(&DopplerConfig)` | project (`project`→`project`); `doppler.config.ensure` (`project`, `environment`) | |
| `doppler.config.inheritable.ensure` | Reversible | Contained | none | `doppler.config.ensure`/`branch_config.ensure` (`config`→output `config`) | a flag on an existing config. Whether it was already set is not recorded |
| `doppler.config.inherits.ensure` | Reversible | Contained | none | same | add-only, and which bases were already there is not recorded |
| `doppler.project_member.ensure` | Reversible | Contained | none | project | add or raise. The prior role is not recorded |
| `doppler.service_token.ensure` | Reversible | **Delete** | existing `delete_service_token`, by the one slug created in the window | config (`config`→output `config`) | |
| `doppler.service_token.rotate` | Destructive | Retained | none | | the revoked token cannot be restored |
| `doppler.secret.set` | Reversible | Contained | none | config (`config`→output `config`) | always writes. The prior value is never read, by design |
| `signoz.ingestion_key.ensure` | Reversible | Retained | none (a delete exists, unproven) | | no live sandbox (out of scope) |
| `buildkite.pipeline.ensure` | Reversible | **Delete**, losses `["builds"]` | existing `delete_pipeline(&BuildkiteOrg, &BuildkitePipelineSlug)` | none | marker description, `created_at`; builds are a declared loss (decision 3a) |
| `buildkite.pipeline.bootstrap.ensure` | Destructive | Contained | none | pipeline (`org`, `slug`) | the prior configuration is not recorded |
| `appstore.bundle_id.ensure` | Reversible | Retained | none | | App Store identity the operator keeps: no ownership marker exists, and deleting an identifier takes its capabilities and profiles with it |
| `appstore.bundle_id_capability.ensure` | Reversible | Retained | none | | disabling a capability invalidates every profile on the identifier |
| `appstore.profile.ensure` | Destructive | **Delete** | the existing `delete_profile(&AppleProfileId)` (already used by the INVALID-replace path and the live write cycle), by the id the run recorded; `standing` reads through a new sparse read with no `profileContent` | none | no marker exists, so ownership is the recorded id, its name and its bundle id, and the creation window (verify 10). The inverse is mock-tested only (Out of scope) |

**Gates** (`appstore.app.get`, `appstore.app_group.gate`, `appstore.bundle_id_capability.gate`,
`buildkite.pipeline.bootstrap.gate`, `doppler.config.inheritable.gate`, `doppler.secret_name.gate`,
`operator.acknowledge`) declare nothing. The engine reports each gate instance the run found `Present` as `MadeByHand
{ need }`, using the gate's own static `need`: this is the todo's "gate in reverse". A Blocked gate was never satisfied
and is not listed.

**Why the identifier and its capabilities stay `Retained`:** none of App Store's resources carries an ownership
marker, and Apple's deletes cascade: deleting an identifier takes its capabilities and profiles, and disabling a
capability invalidates every profile on the identifier. Those are capability reasons, so no document can ask for
them. A profile cascades nothing and is addressed by the exact id the run recorded, so it is deletable when its
document says `teardown: delete` (the operator's correction of 2026-10-05). Whether a given app keeps its profiles
is that app's document's choice. The live cycles still delete their throwaway identifiers by id, as before, outside
rollback.

### 5. Credentials: re-resolved from the document, never from the journal

The journal keeps full non-secret values (masking is an output-surface post-pass, `disclosure.rs`) but only markers
for secrets. A `Delete` tool's `needs` may name a secret credential port (`github.repo.ensure`'s and
`buildkite.pipeline.ensure`'s `token`). Under the design's "credentials are ports, resolvers are nodes", such a port is
bound to a **pure** resolver node, so:

- The teardown requires the workflow document, by name from the trusted directory, **at the run's recorded
  `document_sha256`**. A changed document refuses with the existing `DocumentChanged`.
- New `willikins_core::plan::resolve_pure(checked, inputs, wanted)` evaluates, in the checked graph's order and from
  the recorded workflow inputs (rebuilt with `Butler`'s existing `parse_recorded_value`), **only the backward closure
  of `wanted`**: the pure nodes the `Delete` steps' secret `needs` ports are bound to, and their own pure ancestors.
  Pure includes gates that read providers (`appstore.app.get`, `doppler.secret_name.gate`). Evaluating every pure node
  would make deleting a GitHub repository need App Store credentials. The resolver chains are all a teardown reads. A
  pure node bound to an impure node's output resolves `Unknown` and is not read.
- Each `needs` port takes its non-secret value from the journal (`NodeStarted.inputs`, re-parsed by its declared
  type) and its secret value from `resolve_pure`. An unbound optional credential port stays unbound, and the tool
  falls back to its execution-context credential exactly as forward apply does. A secret `needs` port bound to an
  impure node is `TeardownError::UnresolvableCredential { node, port }`.

**Why not re-plan the whole document:** a forward re-plan reads every node. After a half-finished teardown it reads
`Absent` for what is gone, and `plan` turns other states into hard errors (`AttributeMismatch`). The pure subgraph is
the only part a teardown needs, and it is exactly the part that must be evaluable before anything exists.

### 6. Order, and what a teardown plan is

`plan_teardown(facts, checked, resolved) -> Result<Teardown, TeardownError>` is a pure function of the run's
journal facts, the checked document (its graph and each node's `teardown:`), and what each inverse observes. Its steps are the run's applied plan instances
**in reverse `Plan::nodes` order**, which is a topological order of the graph reversed: dependents come before what
they depend on. A rider is listed in its own position as `RidesWith { container }`. Only `Delete` steps call a
provider at apply time.

**Per step, capability × policy × standing decides the action.** A candidate whose tool is `Retained` is
`Retained { why }`. One whose tool is `Delete` and whose node's policy is `retain` is `Kept { why: KEPT }`, and its
inverse is **not** observed: a kept resource's drift is not this teardown's concern. One whose policy is `delete` is
observed: `AsCreated` with every reported loss accepted is `Delete`, `Gone` is `Gone`. Riders follow decision 3a.

If any observed candidate's standing is `NotOurs` or `Drifted`, or `AsCreated` with a loss its policy does not
accept, or a kept candidate rides inside a container being deleted (decision 3a), `plan_teardown` returns
`TeardownError::Refused { steps: Vec<(InstanceRef, &'static str)> }` and **nothing is recorded as approvable**. The
operator fixes the drift by hand (or deletes the thing by hand, which then reads `Gone`) and plans again. A partial
teardown that deletes some things and refuses others is never offered. It would be worse than one that never started
(`deploy/teardown.sh`'s rule).

### 7. Drift: what "changed since this run created it" means, per Delete tool

Every rule below runs inside the tool's own `standing`, from one or a few `GET`s, and returns a static reason. Only
a candidate whose node says `teardown: delete` is observed (decision 6).

| Tool | `NotOurs` when | `Drifted` when |
| --- | --- | --- |
| `github.repo.ensure` | `404` → `Gone`; any `3xx` (renamed or transferred: `Http` follows no redirect); `full_name` ≠ the recorded key byte for byte; topic `managed-by-willikins` missing; `created_at` outside the window | `pushed_at` > `settled_by`; `open_issues_count` > 0 (issues and pull requests); `forks_count` > 0; an Actions secret name (`GET /repos/{o}/{r}/actions/secrets`, names only) not written by a tolerated rider `github.actions_secret.ensure` |
| `doppler.project.ensure` | `description` ≠ `managed-by: willikins`; `created_at` outside the window | an environment other than `dev`/`stg`/`prd` or a rider's; a config other than those environments' roots or a rider's; any config's secret names (`include_managed_secrets=false`, names only) not written by a rider `doppler.secret.set` into that config or inherited (decision note below); a config with `inheritedBy` naming a config outside this project; a service token whose name no rider created |
| `doppler.config.ensure` (environment) | the project lacks the marker; the environment's `created_at` outside the window | a config in the environment that no rider created; secret names, `inheritedBy`, tokens as for the project, scoped to the environment |
| `doppler.branch_config.ensure` | the project lacks the marker; `root` is true; `environment` differs; `created_at` outside the window | secret names, `inheritedBy`, tokens as above, scoped to the config |
| `doppler.service_token.ensure` | the project lacks the marker; no token at `(config, name)` created in the window | more than one token at `(config, name)` |
| `buildkite.pipeline.ensure` | `description` ≠ `managed-by: willikins`; `created_at` outside the window; the slug differs | none of its own. Any build is the declared loss `builds`, reported in `AsCreated { losing }`: refused unless the node accepts it (decision 3a) |
| `appstore.profile.ensure` | `404` → `Gone`; the id's name or bundle id differs from the run's recorded ones; its creation date outside the window (verify 10) | none: a profile has no contents |

**Inherited names.** The names endpoint lists inherited names (`docs/solutions/providers/doppler-names-endpoint-lists-inherited-names.md`).
A name visible only through a base config the rider `inherits.ensure` added is not drift. Implementation: a name
counts as drift only if it is not written by a rider and not present in any base the config inherits (one names read
per base, the 3j walk's shape).

**Marker-less resources inside a project willikins does not own** (a config this run created in an operator-made
project) are `NotOurs`, so the whole teardown refuses. The Doppler client's own rule applies: configs and tokens are
willikins' only under an owned project. The operator deletes such a config by hand and re-plans; it then reads `Gone`.

### 8. Journal, approval, refusals

- **Teardown plans share the `PlanId` namespace**, so the approval and apply windows (`ButlerConfig`) carry over.
  `approve`/`reject` and their replay must resolve a `plan_id` in either `PlanRecord` or the new `TeardownRecord` (R7).
  `TeardownPlanned { plan_id, of_run, workflow, document_sha256, teardown: Redacted<Teardown>, fingerprint:
  Vec<StepFingerprint>, class, requires_approval: true, principal }` populates the `TeardownRecord`. `Teardown` joins
  `Redactable`'s sealed set. `Redacted` JSON is one-way, so the apply-time comparison reads the **typed**
  `StepFingerprint { node, instance, action, standing }` (action as a discriminant; `standing: Option<StandingKind>`,
  `None` for every step that is never observed and equal only to `None`, and for `AsCreated` the reported `losing`
  names, which are static strings, never values). A loss that appears between approval and apply therefore differs. This is
  the role `PlanRecorded.fingerprint` plays for forward apply.
- **A teardown run** is `TeardownStarted { run_id, plan_id, of_run, principal }`, one `TeardownStepFinished { run_id,
  node, instance, status }` per step (`Deleted`, `AlreadyGone`, `Failed { error }`, `NotRun`, `Listed` for every
  non-delete step), then `TeardownFinished { run_id, outcome }` (`Succeeded` or `Failed { error }`). Separate events
  keep forward `Outcome`, `NodeStatus` and `RunRecord` byte-identical. An older binary cannot read a journal holding
  them, the forward-compatibility cost `Outcome::Blocked` already documents.
- **On replay**, a forward `RunRecord` gains `rolled_back_by: Option<RunId>` (`skip_serializing_if = "Option::is_none"`),
  set when a teardown of it finishes `Succeeded`.
- **Apply** mirrors forward apply's rules: refuse an unknown plan, an expired window, a missing approval, a changed
  document, or a second apply (`AlreadyApplied`). Then re-plan the teardown afresh and compare its fingerprint with
  the approved one step by step (same instances, same actions, same standings). A difference is `ApplyRefused { reason: Drift { …, detail: Standing }
  }`, and nothing is deleted. Then mint the `TeardownToken`, run every `Delete` step in order, and stop at the first
  failure. The remaining steps are `NotRun`.
- **Refusals at teardown plan time** (journaled as `ApplyRefused` with the new reasons): the run is a teardown
  (`NotAForwardRun`); it has no `RunFinished`, because it is still running or the process died (`RunUnfinished`:
  `settled_by` is undefined for it; the existing `RunInProgress` means another run holds the apply lock and stays
  for that); a teardown of it already succeeded
  (`AlreadyRolledBack { run_id }`); or a **later** forward run of the same workflow in this journal mutated something
  (any planned `Create`/`Replace`/`Update` that finished `Created`) and has not itself been rolled back
  (`LaterRunFirst { run_id }`).
- **Kept and Retained stay apart in the journal.** `StepFingerprint.action` carries the discriminant, so a step the
  document keeps and one the tool cannot undo never compare equal, and `TeardownStepFinished` lists both as `Listed`.
- **A half-finished teardown resumes by planning again.** What it deleted reads `Gone`. The run is marked rolled back
  only when a teardown finishes `Succeeded`. A teardown with no `Delete` step (everything `Kept`, `Retained`, `Gone` or
  `MadeByHand`) is still recorded, approved and applied, so the run is marked and an earlier run is no longer blocked
  by `LaterRunFirst`.

**Why LIFO per workflow:** a later run of the same document may have built on what this one created (a config in its
project). Its own teardown is the only one that knows what it added. A different document's later additions are caught
by the container drift checks instead.

### 9. The CLI

`willikins rollback <file> --run <run-id> --journal <path> [--live | --fake-state F] [--fake-state-out F]` plans a
teardown, records it, and prints it: steps in order, uncertain deletes apart, then "rides with", "already gone", "kept by the
document", "cannot be undone" (each with its reason), and "made by hand". It prints the `plan_id` and the `approve` command, and always
requires approval. `willikins rollback --plan-id <id> --journal <path> [--live | ...]` applies an approved teardown.
`--json` and `--reveal` behave as everywhere else. Exit codes are `apply`'s, plus `1` for a refused (drifted)
teardown. `<file>` is copied into the CLI's private trusted directory exactly as `apply <file>` does, so its hash is
checked against the run's.

`deploy/teardown.sh` is **retired** once L4 has done its job live: task D0 deletes it and D1 points the HANDOFF and
README at `willikins rollback`.

### 10. The forward plan names what rollback could not undo

`Plan::not_reversible` lists every instance planned `Create`, `Replace` or `Update` whose rollback would not undo it,
in the plan's order. Each entry is `{ node, instance, tool, by: Keeper, why: &'static str }`:

- the tool is `Retained` (`by: Tool`);
- the tool is `Delete` and the node's policy is `retain` (`by: Document`, `why: KEPT`);
- the tool is `Contained` and no container instance in the same plan is planned `Create` with matching values and
  `teardown: delete` (`by: Tool` when no container matches, `by: Document` when the matching one is kept);
- or the instance is `Replace`/`Update`, whatever the tool declares (`by: Tool`).

A `Delete`-capable node kept inside a container planned `Create` under `teardown: delete` is listed `by: Document` with
`why: "a node the document keeps is inside a container it deletes: the teardown will refuse"`, so the forward plan
shows the refusal before the run. The matching is R5b's function, shared with `plan_teardown`.

Rendered after `replacing` in text, and present in `--json` and MCP plan results. Computed at plan time from the same
`Within` matching as decision 3, so the forward plan and the teardown cannot disagree. **The cost:** with `retain` the
default, nearly every tracked document's characterization gains this array, listing every created node. That is the
point: the forward plan says, before approval, that nothing will be deleted on rollback unless the document asks. The
diff review accepts only added `not_reversible` arrays.

## Acceptance tests

1. **Declarations validate** (R1, R3): `Catalog::insert` refuses a `needs` or `within` port that is not an input, a
   `Delete` without an inverse, an inverse without `Delete`, a malformed loss name, and a pure tool declaring
   anything.
2. **`TeardownToken` is gated** (R2): structurally, as `SinkToken` is (`TeardownToken::new` declared only under
   `#[cfg(feature = "executor")]`, and `cargo check -p willikins-types` with no `--tests` succeeding with `executor`
   off; a trybuild case cannot express it here, see `derive_compile_fail.rs`'s module doc), the `clippy.toml` row,
   and a tripwire test (beside
   `sink_token_guard.rs`) that no source outside `rollback.rs` names `TeardownToken::new`.
3. **`resolve_pure`** (R4) evaluates a resolver chain (`env.get` → parse), leaves a pure node bound to an impure output
   `Unknown`, and calls no impure tool's `read` and no pure node outside the wanted closure (a gate stub counting
   zero reads).
4. **The candidate predicate** (R5a), under `teardown: delete` unless stated: `Create`+`Created` is a candidate; `Update`+`Created`, `Replace`+`Created` and
   `NoOp`+`Unchanged` are not; `Create`+`Failed`+`AsCreated` gives `Delete { uncertain: true }`; a started, unfinished
   instance in a finished run is `Unfinished` and never deleted. A kept `Create`+`Failed` is `Kept`, with zero
   `standing` calls.
5. **Order and riding** (R5b): steps come out in reverse plan order. Every container below says `teardown: delete`,
   and so does every `Delete`-capable rider unless stated. A config rides with the project created in the same run.
   A config with no `teardown:` inside a deleted project refuses the teardown with decision 3a's reason, and the
   forward plan lists it (decision 10). A `NotRun` `secret.set` plus a hand-set secret at its name refuses. A config inside a pre-existing project is its own `Delete`. Mismatched values do not ride. A `Create` +
   `Unchanged` root config is a transit rider, and a secret set into it rides with the project (the
   `new-rust-service` shape). The same run with the project kept lists the config and the secret `Kept` (its
   container is kept) and observes nothing.
6. **Refusal is total** (R5a): one `Drifted` step refuses the whole teardown, with no `TeardownPlanned` recorded. So
   does a pipeline whose inverse reports `builds` under a plain `teardown: delete`; with `accept: [builds]` it is a
   `Delete`. A drifted resource on a **kept** node refuses nothing (its inverse is never called: a counting stub sees
   zero `standing` calls).
7. **Apply re-observes** (R6): a standing that changes between approval and apply refuses with `Drift`/`Standing`
   before any delete (a counting stub sees zero deletes). A delete failure stops the run, and later steps are
   `NotRun`.
8. **Resume** (R6, R8): after a half-finished teardown, planning again reads the deleted steps `Gone`, and the second
   teardown succeeds and marks the run.
9. **Journal** (R7): every new event round-trips. A journal written before this milestone replays byte for byte.
   `redaction_by_construction.rs` seeds a secret into a credential port of a teardown's `Created`, and into a rider
   `secret.set`'s value port, and finds no byte of either in any line.
10. **Refusals** (R8): `AlreadyRolledBack`, `LaterRunFirst`, `RunUnfinished`, `NotAForwardRun`, `DocumentChanged`,
    `ApprovalRequired`, `PlanExpired`, each journaled.
11. **CLI round trip on fake state** (R9): `apply --fake-state S --fake-state-out A`, then `rollback --run R
    --fake-state A`, then approve, then `rollback --plan-id P --fake-state A --fake-state-out B`. `B` holds none of
    the run's created resources and everything seeded in `S`.
12. **Per-tool drift** (P1–P4): a mock test per `NotOurs`/`Drifted` row of decision 7, each pinning the exact query,
    including a `3xx` repo and a `created_at` one second outside the window.
13. **The declaration table is pinned** (P7): one snapshot of every live tool's reversal, losses included. Adding a
    tool without a declaration changes the snapshot.
14. **Delete call sites are inventoried** (P8): a guard lists every `.delete(` and `delete_with_body(` call site in the
    provider crates by file and enclosing function. A new one fails until reviewed.
15. **Forward visibility** (F1): a plan with a scaffold node and no repo `Create` lists the scaffold in
    `not_reversible` (`by: Tool`). With a repo `Create` under `teardown: delete`, it does not. With the repo kept, the
    repo and the scaffold are both listed `by: Document`.
16. **Live, sandbox only** (L1–L4), from documents that say `teardown: delete`: create, roll back, `GET` is `404`. Create, drift (a push, a hand-set secret), and
    the teardown refuses with the drift named. The test then deletes by hand.
17. **Adversarial pass** (X1), recorded under `docs/research/`. Every bypass becomes a fixture or a test.
18. **Policy is checked** (T1 on stub tools; T2 on documents): fixtures `workflows/fixtures/teardown-not-admitted.yaml` (`teardown: delete` on a
    `Contained` tool) and `teardown-loss-not-declared.yaml` (`accept: [builds]` on `github.repo.ensure`) produce exactly
    their `CheckError`. `teardown:` on a `uses:` step is refused by the DSL. A linked document's node keeps its own
    policy (the composite path agrees with the flat one).
19. **The default deletes nothing** (R5a): a run of a document with no `teardown:` anywhere plans a teardown with no
    `Delete` step and calls no inverse.
20. **The profile inverse, on mocks** (P5): the standing read pins its exact sparse query; `standing` reads `Gone` on
    `404` and `NotOurs` on a name or bundle-id mismatch; the inverse's `delete` pins `delete_profile`'s request. P5
    adds no live App Store call.
21. **A loss is re-checked at delete** (R6, P4): a pipeline with no builds at the re-plan and one build when `delete`
    observes is not deleted (`Conflict`), and a build appearing between approval and apply refuses as
    `Drift`/`Standing`.

## Verify before relying on them

Each item is settled by fetching the primary source verbatim, or by the first live cycle that touches it (the result
is recorded as an addendum here and in `docs/solutions/providers/`).

1. GitHub `GET /repos/{owner}/{repo}/actions/secrets` lists names with no values, and `GET /repos/{owner}/{repo}` carries `created_at`, `pushed_at`, `open_issues_count`, `forks_count`,
   `full_name` and `topics`, and **`createCommitOnBranch` bumps `pushed_at`** (L1: if it does not, the scaffold is
   invisible to the drift rule, and a push after the run is still caught).
2. GitHub `DELETE /repos/{owner}/{repo}` answers `204`, and `403` when the org forbids member deletion (the live write
   cycle already met that body).
3. Doppler's environment delete removes the environment's branch configs, or refuses while they exist. Either way, the
   environment's drift rule must hold every branch config as a rider or refuse.
4. Doppler refuses to delete a config another config inherits, or does not. The `inheritedBy` drift rule is correct
   either way, and L2 records which.
5. Doppler's service token list carries `slug` and `created_at` (fetched 2026-10-05 from
   `service_tokens-list.md`: both appear in the example). Confirm live in L2.
6. Doppler's project, environment and config objects carry `created_at` (fetched 2026-10-05: `projects-get.md`,
   `environments-get.md`, `configs-get.md` examples). Confirm the format parses as RFC 3339 in L2.
7. A new Doppler project arrives with exactly `dev`, `stg` and `prd` (m1 research note, and HANDOFF's "three
   auto-created root configs"). L2 records the list a fresh sandbox project shows.
8. Buildkite's pipeline carries `created_at`, and `GET /v2/organizations/{org}/pipelines/{slug}/builds?per_page=1`
   lists builds. `DELETE` answers `204`.
9. Doppler's three delete endpoints (`DELETE /v3/projects/project` with a JSON body, `DELETE
   /v3/environments/environment?project&environment`, `DELETE /v3/configs/config?project&config`) are as fetched on
   2026-10-05 (`projects-delete.md`, `environments-delete.md`, `configs-delete.md`). The project delete takes a
   **body** (`Http::delete_with_body`); the other two take a query.

10. App Store Connect's `Profile` resource carries a creation date (`createdDate`) and a bundle id relationship
    readable in one sparse request without `profileContent`. Fetched verbatim from Apple's published OpenAPI
    description before P5; never from a live call. (`DELETE /v1/profiles/{id}` is already exercised by
    `delete_profile`'s existing callers.) If
    the profile carries no creation date, P5 stops and reports: a `Delete` inverse without a window is not admitted
    (decision 2).

## Sequencing with other milestones (portfolio review, 2026-10-05)

- **3k** (one integration-test binary per crate) lands first. Every new non-gated test this plan names lives at
  `crates/<c>/tests/it/<stem>.rs` with a `mod <stem>;` line in `tests/it/main.rs` (`reversal_table`,
  `delete_call_sites_guard`, the `TeardownToken` tripwire, the rollback and mock tests); a `--test <stem>` gate becomes
  `--test it <stem>::` (R2's `--test sink_token_guard`, P7's `--test reversal_table`, P8's
  `--test delete_call_sites_guard`); a guard's self-exemption path names `tests/it/`. The live cycles L1–L3 are gated
  targets and stay top-level with their own `[[test]]` entries.
- **3n** (`Checked` as a typed graph) lands before this plan. `resolve_pure`, `plan_teardown` and `not_reversible`
  read `CheckedNode`s, their `Source`s and their tool handles, never a `Binding` or `catalog.get(`; if 3n's B2 landed,
  none of the new functions takes a `&Catalog`. 3n's tripwire (no `Binding` in `plan.rs`) then covers `resolve_pure`.
  If the coordinator reverses the order, 3n's G3 and G7 carry `rollback.rs` too (3n's addendum says so).
- **2b** (composition) lands before this plan: node names may be `/`-paths (`org/gh_token`), which changes nothing
  here; the teardown's document identity is the root sha plus `PlanRecorded.used` (R8), and the CLI copies the
  closure (R9).
- **3l** (new repositories) lands before this plan: `RepoBody` and the fake's `GitHubRepoRecord` already carry its
  fields; P1 and P6 build on them (a fake repository delete removes the whole record, branches included).

## Gates

**Published shapes.** Any commit that changes a type published by a `schema_generation` snapshot (core, journal) or by
the server's `mcp_server__the_tool_list_and_every_schema_is_snapshotted` snapshot regenerates that snapshot in the same
commit, diff-reviewed as additions only, and runs the owning crate's test.

Scoped per task, as each row says. `-j 2`, `RUST_TEST_THREADS=2`, in the background with a 600,000 ms timeout,
reading the log body. Never two cargo commands at once. The coordinator runs the full four-command gate once after
F1 and once at the end.

## Tasks

One lane at a time on `main`, in this order. Each task is test first, one behaviour per commit, green alone, and
commits by path with `git commit --only` (never `git add -A`, `commit -a`, stash, `checkout --` or `reset`), with the
implementer's own `Co-Authored-By` trailer. Nobody pushes.

| # | Task | Delegate to |
| --- | --- | --- |
| R1 | **`Reversal` and `Within`** (decision 3; acceptance 1). `rollback.rs` with the declaration types, `Reversal::Delete`'s `losses`, `Tool::reversal()` defaulting to `Retained { why: UNDECLARED }`, and `Catalog::insert`'s port validation (and a loss name's pattern). No tool changes. A `Reversal` states capability only: no field of it says whether to delete. Scoped: `-p willikins-core` | sonnet implements |
| R2 | **`TeardownToken`** (trust boundary 2; acceptance 2). The tripwire requires exactly one `TeardownToken::new` call site, inside `apply_teardown` by enclosing function (as P8's guard does), and exactly one `#[allow(clippy::disallowed_methods)]` for it, not merely "only in `rollback.rs`". `sink.rs` (`new` under `#[cfg(feature = "executor")]`, documented beside `SinkToken`'s note in `crates/willikins-types/tests/it/derive_compile_fail.rs`'s module doc), the `clippy.toml` row, the tripwire test at `crates/willikins-core/tests/it/teardown_token_guard.rs` with its `mod` line. No trybuild case (the self dev-dependency unifies `executor` on). Scoped: `-p willikins-types`, `cargo test -p willikins-core --test it -j 2 -- sink_token_guard:: teardown_token_guard::`, then `cargo check -p willikins-types` | sonnet implements |
| R3 | **`Inverse`, `Created`, `Standing`, `Deleted`** (decision 3). `Tool::inverse()`, `Standing::AsCreated { losing }`, and `Catalog::insert`'s "`Delete` exactly when `inverse()` is `Some`". Test stubs in `testing.rs`. Scoped: `-p willikins-core` | sonnet implements |
| T1 | **`teardown:` in documents** (decision 3a; acceptance 18's core half). Commit 1: `TeardownPolicy`/`Loss` in `workflow.rs`, `Node.teardown`, the DSL field on `StepDecl` with located errors for the three forms (as `WithValue` does), refused on a `uses:` step in `add_uses_step` beside the `for_each` refusal and in the schema's `uses` branch, the linker carrying it unchanged, `CheckedNode.teardown`, and the dsl `document_schema` snapshot (additions only, plus a test that the schema admits the three forms). Commit 2: `check` refuses `TeardownNotAdmitted` and `TeardownLossNotDeclared` (variants, `Display`, `kind()`, the samples list, `willikins-cli`'s exhaustive `render.rs` arm), proved on R3's `testing.rs` stubs (one declaring `Delete` with a loss, one `Contained`), including a linked document's node keeping its own policy; and, in the same commit, the server's `mcp_server` tool-list snapshot (it embeds every `CheckError` variant; additions only). No fixture and no tracked document changes here: until P6 no fake tool declares `Delete`. Scoped: `-p willikins-core -p willikins-dsl -p willikins-cli`, `-p willikins-server --test it -- mcp_server::` | sonnet implements, opus attacks |
| R4 | **`resolve_pure`** (decision 5; acceptance 3). In `plan.rs`, `resolve_pure(checked, inputs, wanted)` reusing `plan`'s own `bind_ports`/`resolve_source` over the checked graph's `Source`s. It names no `Binding`, takes no `&Catalog`, adds no counted fallback (the ratchet pins `plan.rs` at 8), and is not a second probe of the conversion table: 3n's tripwires over `plan.rs` must still pass. Scoped: `-p willikins-core` | sonnet implements, opus attacks |
| R5a | **`plan_teardown`** (decisions 1, 2, 3a, 4's gate rows, 6; acceptance 4, 6, 19). `RunFacts` (built by callers, so core does not depend on the journal crate), `Teardown`, `TeardownStep`, `TeardownAction` with `Kept` apart from `Retained`, the candidate predicate, windows, `Unfinished`, `MadeByHand`, policy × capability × standing per step, losses against `accept`, ordering, total refusal. Takes `&Checked`, no catalog. Adds `rollback.rs` to 3n's guards: `check.rs`'s no-`Binding`/no-second-lookup tripwire, `ratchet.rs`'s no-second-registry pin and its one-`#[cfg(test)]`-split check. Scoped: `-p willikins-core` | sonnet implements, opus attacks |
| R5b | **Riding** (decisions 3, 3a; acceptance 5). The `Within` matcher as one function over a neutral view (tool, node, instance, recorded port values, status), used by `plan_teardown` and later by F1: transit riders, riders of kept containers, a kept `Delete`-capable candidate inside a deleted container refusing, and tolerance limited to riders that wrote or found something. Scoped: `-p willikins-core` | sonnet implements, opus attacks |
| R6 | **`apply_teardown`** (decision 8's apply; acceptance 7, 8's core half, 21's core half). Re-plan, compare fingerprints (standing kinds and losses included), mint the one `TeardownToken`, delete in order, stop on failure. Scoped: `-p willikins-core` | sonnet implements, opus attacks |
| R7 | **Journal** (decision 8; acceptance 9). Also the five `RunRecord` literals in `willikins-cli`'s `render.rs` tests and the `DriftReasonKind` match in the journal's `pre_pass_2_replay.rs`. The four events, the new reasons, `Teardown: Redactable`, the typed `StepFingerprint` (whose action discriminant keeps `Kept` and `Retained` apart), `TeardownRecord`, approval and rejection replay resolving either record kind, `RunRecord::rolled_back_by`, the pre-milestone replay test, and the redaction case. Journal `schema_generation` snapshots and the server's `mcp_server` snapshot that publish a changed shape are regenerated in the same commit (additions only). Scoped: `-p willikins-journal`, plus `cargo clippy -p willikins-server -p willikins-cli --all-targets` and `-p willikins-server --test it -- mcp_server::` | sonnet implements |
| P1 | **GitHub inverse** (decisions 4, 7; acceptance 12). `delete_repo`, a repo read with the decision 7 fields, the Actions secret names read, `GitHubRepoEnsure`'s `Delete` and inverse; `Contained` for `actions_secret` and `scaffold`. Mock tests pin each query. Commit 1: the client. Commit 2: the tool. `RepoBody` (which 3l's S1 extended with `default_branch`) gains the decision 7 fields as `#[serde(default)]` options, since existing mock fixtures predate them; `repo_ensure.rs`'s exhaustive `observe` destructure gains them as `_`. Scoped: `-p willikins-providers-github` | sonnet implements, opus attacks |
| P2 | **Doppler project inverse** (decision 7's project row). `delete_project` (body), `delete_environment`, `delete_config`, and the list reads (environments, configs with `created_at`/`inheritedBy`, names, tokens). Commit 1: the client. Commit 2: `doppler.project.ensure`. Scoped: `-p willikins-providers-doppler` | sonnet implements, opus attacks |
| P3 | **Doppler config, branch config and token inverses**, and the `Contained`/`Retained` declarations of the other five non-pure Doppler tools (`config.inheritable.ensure`, `config.inherits.ensure`, `project_member.ensure`, `service_token.rotate`, `secret.set`). Scoped: `-p willikins-providers-doppler` | sonnet implements, opus attacks |
| P4 | **Buildkite inverse.** The builds read, `buildkite.pipeline.ensure`'s inverse declaring the loss `builds` and reporting it in `AsCreated { losing }` (never `Drifted` for builds), `bootstrap.ensure` `Contained`; `delete_pipeline`'s doc now names its one non-test caller. Scoped: `-p willikins-providers-buildkite` | sonnet implements |
| P5 | **App Store and SigNoz declarations** (acceptance 20; verify 10). `appstore.bundle_id.ensure`, `bundle_id_capability.ensure` and SigNoz `Retained` with decision 4's reasons verbatim. `appstore.profile.ensure` gains `Delete` and its inverse: it reuses the existing `delete_profile(&AppleProfileId)` (whose doc gains the inverse as a third caller), adds a typed sparse standing read (name, creation date, bundle id; no `profileContent`), and implements decision 7's `standing`, pinned by mock tests only. Verify 10 is fetched from Apple's published OpenAPI description first, and if it fails the task stops and reports. **No new live App Store call** (the operator's live account): no new live test or `live-tests` target, no manual try; the existing live write cycle is untouched. Scoped: `-p willikins-providers-appstore -p willikins-providers-signoz` | sonnet implements, opus attacks |
| P6 | **Fake twins**: the same declarations on every fake tool, fake inverses for the seven `Delete` tools over `FakeState` (the profile's included; the fake pipeline reports its fake builds as the `builds` loss; no timestamps: the fake skips the window rule, which the mock tests pin instead), the container-side `Within` check over the fake catalog, and, in the five provider crates' `tests/it/catalog_parity.rs` and `tests/it/fake_agrees_with_live.rs`, parity of declarations and rows for standing. Scoped: `-p willikins-providers-fake`, then `cargo test -p willikins-providers-github -p willikins-providers-doppler -p willikins-providers-buildkite -p willikins-providers-appstore -p willikins-providers-signoz --test it -j 2 -- catalog_parity:: fake_agrees_with_live::` | sonnet implements |
| T2 | **Policy fixtures** (acceptance 18's document half), now that fake tools declare. `workflows/fixtures/teardown-not-admitted.yaml` and `teardown-loss-not-declared.yaml` with header comments, their acceptance assertions (flat and through `uses:`), and the dsl characterization's two new entries (additions only). Scoped: `-p willikins-dsl`, `-p willikins-cli --test it -- acceptance::` | sonnet implements |
| P7 | **Declaration table** (acceptance 13). `crates/willikins-server/tests/it/reversal_table.rs` (with its `mod` line) over `LIVE_TOOL_NAMES`, plus the `Within` type check of decision 3. Scoped: `-p willikins-server --test it reversal_table::` | sonnet implements |
| P8 | **Delete call-site guard** (acceptance 14). `crates/willikins-cli/tests/it/delete_call_sites_guard.rs` (with its `mod` line), written like `no_gh_writes_guard.rs`; its self-exemption names `tests/it/`. Scoped: `-p willikins-cli --test it delete_call_sites_guard::` | sonnet implements |
| R8 | **`Butler::plan_rollback` / `apply_rollback`** (decisions 5, 8; acceptance 8, 10). `RunFacts` from the journal through `parse_recorded_value`, the document hash check, `resolve_pure`, LIFO, double rollback, windows. `Butler::decide` (approve and reject) finds a `plan_id` in either record kind with the same pending, expiry and already-decided rules, pinned by a test approving one teardown plan and rejecting another. On the fake catalog. After 2b: the document is linked through the trusted-directory resolver before `check`, and the teardown refuses `DocumentChanged` unless the root's sha **and** the run's `PlanRecorded.used` closure both match, exactly as 2b's S2 does for apply. Scoped: `-p willikins-server` | sonnet implements, opus attacks |
| R9 | **CLI `rollback`** (decision 9; acceptance 11). The subcommand, text through `Value::display(Disclosure)` and JSON through `mask_json` ("kept by the document" apart from "cannot be undone"; a test that a profile step's id prints masked in both and whole only under `--reveal`), and the fake-state round trip test, over a test-local document that says `teardown: delete` on its deletable nodes. `<file>` is copied with its linked closure, as 2b's K1 made `apply <file>` do. Scoped: `-p willikins-cli --test it` (the new modules) and `render`'s unit tests | sonnet implements |
| F1 | **`Plan::not_reversible`** (decision 10; acceptance 15). Commit 1: the field (with `by: Keeper`) and its computation in core, reading each node's `teardown:` and reusing R5b's matcher, with tests; in the same commit, because each goes red without it: every `Plan { .. }` struct literal (rustc lists them: `render.rs`'s tests, `plan.rs`'s unit tests, the journal's `journal.rs` tests, `tests/common/mod.rs` and about ten `tests/it/` files, `willikins-cli`'s `tests/it/acceptance.rs`), core's `schema_generation__plan_schema` snapshot, the journal's plan-record snapshot, the server's `mcp_server` snapshot (all publish `Plan`), core's `acceptance_6` plan snapshot, and the dsl characterization and composition snapshots, which print `plan_json` (the tracked characterizations; the gitignored `operator_*` snapshots may hold plan JSON too: run them, update locally if they move, never commit). With `retain` the default, nearly every tracked document gains an array: all diff-reviewed as added `not_reversible` arrays or schema properties only. Commit 2: text rendering after `replacing`. Scoped: `-p willikins-core -p willikins-dsl -p willikins-journal`, `cargo clippy -p willikins-cli -p willikins-server --all-targets`, `-p willikins-server --test it mcp_server::`, then `-p willikins-cli`; run the gitignored `operator_*` targets and report | sonnet implements |
| C1 | **The operator's app document declares its policy** (coordinator, after F1, so check admits it and the fake plan shows `not_reversible`). In the gitignored private app document: `teardown: retain` on its profile nodes and `teardown: delete` (no `accept`) on its pipeline node; nothing else changes. Its private tests check cleanly; a fake plan lists the kept nodes. Uncommitted, survives only in this working tree | coordinator |
| L1 | **GitHub live rollback cycle** (acceptance 16, verify 1–2), behind `live-tests`, in the sandbox org only. Written and compiling, **never run by the implementer**. Scoped: `cargo clippy -p willikins-providers-github --features live-tests --all-targets -j 2 -- -D warnings` | sonnet writes, coordinator runs once |
| L2 | **Doppler live rollback cycle** (verify 3–7, 9), as L1, in the sandbox workplace | sonnet writes, coordinator runs once |
| L3 | **Buildkite live rollback cycle** (verify 8), as L1, in the sandbox org | sonnet writes, coordinator runs once |
| L4 | **End to end through the CLI**: `apply` then `rollback` of a scratch copy of `workflows/new-rust-service-buildkite.yaml` that adds `teardown: delete` to its deletable nodes (the tracked document keeps the default), in the sandbox, with a scratch journal. A coordinator runbook in this plan's addendum, not a test | coordinator |
| X1 | **Adversarial pass** (acceptance 17), recorded under `docs/research/2026-10-0x-m3m-adversarial-pass.md`. Priority targets: a delete reachable without a `TeardownToken` or from a document; a candidate that the run did not create (Update, Replace, NoOp, another run's); a rider deleted on its own; a container deleted despite foreign contents (a hand-set secret, an Actions secret, an inheriting config elsewhere, a push); a window bypass by clock or format; a `3xx` repo followed; a credential read from the journal; a value in a reason; LIFO bypass; a teardown applied after its standing changed; policy reintroduced in a tool declaration; a delete reachable on a node that declares nothing, on a rider of a kept container, or on a kept node inside a deleted container; contents tolerated under a rider that wrote nothing; a loss appearing after approval; a policy changed after the run without `DocumentChanged`; an unaccepted loss deleted; a parent widening a used document's policy; any App Store call outside mocks | opus |
| D0 | **Retire `deploy/teardown.sh`** (decision 9), after L4 has run live. One commit: delete `deploy/teardown.sh`, `deploy/teardown_test.sh` and `crates/willikins-cli/tests/it/teardown_script.rs` (which runs that suite) with its `mod teardown_script;` line in `tests/it/main.rs`; in `crates/willikins-cli/tests/it/no_gh_writes_guard.rs`, `the_walk_reads_every_kind_of_file_this_repository_executes` drops `deploy/teardown.sh` from its on-disk list (no tracked `.sh` file remains, so `is_runnable("teardown.sh")` stays as the name check only) and the comments that cite the script as the guard's origin stay as history; the gated `crates/willikins-cli/tests/live_smoke.rs` replaces its teardown step (and its `jq` pre-flight) with `willikins rollback` (plan, approve, apply); the `Cargo.toml` comments citing the script, `smoke_parity.rs`'s comments and README's two teardown sections point at `willikins rollback`. The guard must still pass with no `.sh` in the tree. Scoped: `cargo test -p willikins-cli --test it -j 2 -- no_gh_writes_guard:: smoke_parity::`, `cargo clippy -p willikins-cli --all-targets` and `cargo clippy -p willikins-cli --features live-tests --all-targets` | sonnet implements |
| D1 | **Docs**: after D0, add a design-doc addendum and a CLAUDE.md invariant (edit `AGENTS.md` identically), update the HANDOFF, close the todo, and write the MCP follow-up todo | coordinator |

## Risks

- **A resumed scaffold makes its repository unrollbackable** (with 3l). 3l's recovery for a scaffold that stopped
  between its two writes is "re-run the document". That second run's scaffold commit moves `pushed_at` past the first
  run's `settled_by`, so after the second run is rolled back (LIFO; its scaffold is `Contained` and deletes nothing),
  the first run's repository reads `Drifted` and the teardown refuses. Safe, by hand to resolve; X1 should confirm the
  refusal names the push. A rule that forgives pushes made by later, rolled-back runs of the same workflow is a later
  decision.
- **Over-refusal.** The drift rules are strict on purpose: a repo with one issue always refuses; a pipeline with one
  build refuses unless its node says `accept: [builds]`.
  The remedy is by hand and stated in the reason. Loosening a rule is a later decision with its own plan.
- **Clock skew larger than two minutes** turns a resource willikins created into `NotOurs`, which refuses (safe) and
  never deletes the wrong thing.
- **What GitHub drift does not see.** Deploy keys, webhooks and collaborators added after the run are not read; a
  repository delete takes them. Reading them is a later decision.
- **Cross-document overlap.** Two documents creating inside one container are separated only by the container's drift
  check, not by LIFO. A rider from the other document's run is foreign contents, so it refuses.
- **The journal is the source of truth.** A truncated journal (`todos/2026-09-15-journal-repair-subcommand.md`) can
  lose the `NodeFinished` that bounds a window. That instance is then `Unfinished`, never deleted.
- **Characterization churn** from F1 touches many snapshots. It is diff-reviewed as additions only, and lands last
  among the core tasks so no other task rebases over it.
- **Deletes are permanent.** No live test runs outside the sandbox. The probe prefix and the sandbox variables are
  required, and a live cycle rolls back only the run it just made, from a journal it wrote.
- **Privacy.** No real name in this plan, the tests, or the fixtures. The hooks refuse one, and it must be rewritten,
  never bypassed.
- **Host contention.** If no quiet three-second window comes within 40 minutes, the task stops and reports the
  contention.

## Needs the operator

1. **Answered 2026-10-05:** a profile's teardown is its document's decision. The tool gains an inverse (P5); the
   operator's app document keeps its profiles (C1).
2. **Answered 2026-10-05:** a pipeline with builds is the document's decision too (`accept: [builds]`). The
   operator's app document refuses (C1: `teardown: delete` without `accept`).
3. **Sandbox credentials for L1–L3** in `~/.config/willikins/sandbox.env`: the GitHub PAT with repository
   administration in the sandbox org (already there), the sandbox Doppler token (already there), and a Buildkite token
   able to delete pipelines in the sandbox org (the renewed one is broad enough; confirm).
4. **C1 is a local, uncommitted edit** to the operator's private app document, like 3n B2's edit to the private
   tests. It survives only in this working tree.
