# Milestone 3e: one document provisions a new iOS app end to end — Walter

**Created:** 2026-09-27
**Completed:** 2026-09-29 — full gate by the coordinator on `a6cbef2`: fmt, workspace clippy, 183 suites / 2559 tests / 0 failed / 18 ignored, `cargo check -p willikins-types`; all three guards passed. Live on the operator's Apple account: the capability cycle and the profile replace cycle both passed on throwaway identifiers (counts 21/5/13 unchanged). Read-only dry run of `workflows/walter-ios-app.yaml` recorded below.
**Gate:** PRE-FLIGHT PARTLY BLOCKED, 2026-09-27 — implementation (tasks 1 to 3) is clear; the sandbox
**dry run** is blocked on one credential: the sandbox Buildkite token answers `401` on
`GET /v2/access-token`, which needs no scope, so it is revoked or expired rather than under-scoped.
See "Credentials" and the pre-flight checklist.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (type system, tool contract, "policy lives in the
workflow, never in the tool", the 2026-09-22 `Action::Update` addendum)
**Research:** `docs/research/2026-09-16-app-store-connect.md` (section 2, capabilities and
`CapabilitySetting`; section 3, app groups; section 4, app records),
`docs/research/2026-09-16-m3a-buildkite.md`,
`docs/research/2026-09-20-project-survey-and-workflow-library.md` (Walter's "one step in twelve"),
and this plan's own pre-flight, fetched 2026-09-27 and quoted below with its URLs.
**Depends on:** milestones 3a (Buildkite), 3c (signing), 3d (conversions), all Completed.
**Reviewed:** 2026-09-28 (independent adversarial pass over T1 and T2,
`docs/research/2026-09-28-m3e-adversarial-pass.md`)
**Reviewed:** 2026-09-29 (independent adversarial pass over T3a, G1–G3, T3b, T3c, B1 and B2,
`docs/research/2026-09-29-m3e-adversarial-pass-2.md`)
**Reviewed:** 2026-09-29 (independent adversarial pass 3 over the diagnosis harness, F1 and F2,
`docs/research/2026-09-29-m3e-adversarial-pass-3.md`)
**Reviewed:** 2026-09-29 (independent adversarial pass 4 over R1, R2, R3 and R4,
`docs/research/2026-09-29-m3e-adversarial-pass-4.md`)
**Reviewed:** 2026-09-30 (independent adversarial pass 5 over K1, L1, D1 and D2,
`docs/research/2026-09-30-m3e-adversarial-pass-5.md`)
**Reviewed:** 2026-09-30 (independent adversarial pass 7 over the App Attest gate task,
`docs/research/2026-09-30-m3e-adversarial-pass-7.md`)
**Addendum:** 2026-09-28 (attacker) — **four defects fixed test-first.** `c054a09`: every capability
read parsed every row's `settings` strictly, so another row's missing `enabled` or `null` `options`
failed a `HEALTHKIT` read; the four fields are now optional. `92dec9e`: decision (d)'s "exactly one
enabled option, the requested one" was implemented as "the requested one among the enabled". `d4abcf4`
and `1328d59`: the live cycle now records an absent `enabled`/`settings` distinctly, Apple's field
names, and the read-back shape **before** asserting convergence. Seven mutations killed.
**Addendum:** 2026-09-28 (attacker) — **the live capability cycle ran once and stopped at trust
boundary 6.** Counts 21 identifiers / 5 certificates / 13 profiles before and after, equal again on an
independent recount, throwaway leftovers 0. `HEALTHKIT` and `PUSH_NOTIFICATIONS` created and converged;
the `DATA_PROTECTION` create carrying `settings` was **accepted**, but the re-ensure read the fresh row
back as `Mismatch { setting }` (`Conflict`). The guard deleted the throwaway by its create id. The row's
shape was not recorded (the cycle recorded it after the assertion; fixed in `1328d59`, unrun). Risk 1
has happened: the create shape works, the read-back does not match decision (d)'s assumption, and the
real shape is still unseen. A second run is the coordinator's call. Verify items 1 and 3 settled yes;
item 2 unsettled (see "Verify before relying on them").
**Addendum:** 2026-09-28 (attacker) — **T3-blocking: a capability node downstream of a fresh
identifier fails `plan` with `NotFound`.** `plan` reads every node whose key is known;
`appstore.bundle_id.ensure`'s `Absent` predicts `identifier`, so `healthkit`/`push`/`data_protection`
bound from `steps.app_id.identifier` are read, and `appstore.bundle_id_capability.ensure`'s `read`
refuses a missing parent. Reproduced against the fake catalog. Decision (c)'s "backstop, not the
mechanism" fires at plan time, and acceptance 8 cannot pass as written. `appstore.profile.ensure`
already reads `Absent` for an unregistered identifier (the precedent). Decision needed before T3; the
tool is unchanged.
**Addendum:** 2026-09-28 (implementer) — **two fixes landed test-first, from the coordinator's second
live capability cycle.** After creating `DATA_PROTECTION` with
`DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH`, Apple's list endpoint returned this
row shape: `attributes: {capabilityType, settings}`; `settings`: an array of one entry; setting entry:
`{key, options}`; option entry: `{key}` **only** — no `enabled` field at all — and the setting listed
exactly one option, `PROTECTED_UNTIL_FIRST_USER_AUTH`. `HEALTHKIT` and `PUSH_NOTIFICATIONS` rows
returned `settings: null`. This settles verify item 2, and not as decision (d) assumed.
**Fix 1** (`fcdb9da`): the read rule is now, in one sentence, *when no option under the requested key
carries an `enabled` field, the listed option is the selection — Present iff exactly one option is
listed and it is the requested one; when at least one option does carry `enabled`, the older
exactly-one-`enabled: true` rule still applies as a defensive fallback never observed live.* Mock
fixtures rebuilt on the observed key-only shape; one fixture (`..._two_enabled.json`) kept as the sole
test of the enabled-field branch; doc comments corrected.
**Fix 2** (`a19247e`, `2bb3773`): `appstore.bundle_id_capability.ensure`'s `read` now reports `Absent`
for a capability whose parent identifier is not registered yet — exactly the precedent
`appstore.profile.ensure` already set — so the T3-blocking `NotFound` above no longer fires at plan
time; `ensure`'s own `Absent` arm still refuses with `NotFound` if the parent is genuinely missing at
apply time, since this tool still cannot create one. A new graph test over the fake catalog
(`capability_documents.rs`) plans a document that both registers a fresh bundle id and enables a
capability on it, bound from the registration node's own `identifier` output, and asserts both nodes
plan as `Action::Create`. Verify item 2 and the T3-blocking finding are both settled; see "Verify before
relying on them" and the post-flight checklist below.
**Addendum:** 2026-09-28 (independent review) — **Fix 1's branch rule tightened.** A row mixing an
`enabled: true` option with a bare listed one read `Present`: the rule was chosen by "at least one
option carries `enabled`". Now the `enabled` rule applies only when *every* option carries it, the
key-only rule only when none does, and a mix is `Mismatch { setting }` (`9acecc4`). A surviving mutation
showed the two-listed test only requested the second-listed option (`79994c8`), and the fresh-bundle-id
graph test now also applies and re-plans as converged (`a468df2`). Record:
`docs/research/2026-09-28-m3e-adversarial-pass.md`, "Capability read fixes, independent review".

**Addendum:** 2026-09-28 — **`list_bundle_id_capabilities` now paginates, closing the "Not settled"
finding of the independent review** ("The capability list is read as one page with no `limit`",
`docs/research/2026-09-28-m3e-adversarial-pass.md`). Apple's row order for
`GET /v1/bundleIds/{id}/bundleIdCapabilities` was observed live to change between reads, so a
capability sitting past whatever page Apple's default returns could read `Absent` even though it is
already enabled, and `ensure` would then `POST` a duplicate whose result is undocumented. Fixed exactly
the way `list_bundle_ids` already handles its own substring-filter paging risk: request `limit=200`,
follow `links.next`, and re-attach only that URL's query string to this client's own fixed capabilities
path, so a response can never steer the client at another host or path (`e4ba9af`). Test-first, mock
only: a capability the unstable order put on page two is found and never triggers a duplicate `POST`
(`e4ba9af`); a `links.next` that never stops is refused past the same page cap `list_bundle_ids` already
carries (`8077680`); a `links.next` naming a different bundle id's path, or a different host entirely,
is never followed as-is (`850af30`). The in-memory fake has no wire layer to paginate and is unaffected;
`fake_agrees_with_live` (22 green) and the pre-existing capability mocks needed `match_query(Any)` added
now that every request carries `?limit=200`. The default page size of the endpoint itself remains an
open verify item (unaffected by this fix, since the client no longer depends on it), but the
duplicate-`POST` risk the review named is closed either way.

**Addendum:** 2026-09-28 (operator decision 1, task T3a) — **`appstore.profile.ensure` gains
replace-when-INVALID; the tool is now `Class::Destructive`.** The operator asked "Can't willikins
create a new profile?" and the answer is yes, scoped exactly: a profile row found at this tool's own
key (`identifier`, `name`) whose `profileState` is `INVALID` is no longer a terminal
`ToolErrorKind::Conflict`. `read` reports it `Observation::Absent` (checked before `profile_type` or
`certificate`, so a stale profile of any shape at this key replans as a create); `ensure` deletes it by
the id `read`'s own resolution carried, then creates fresh. It never touches a profile that is `ACTIVE`,
or `INVALID` under a different identifier or a different name -- both are excluded structurally by the
existing exact-key lookups (`find_bundle_id`, `find_profile_row`), never by an extra check. Expiry is
untouched: still an independent, terminal `Conflict`, per the tool's own module doc. Per the design
doc's class rules, a tool whose `ensure` can now delete a resource it did not itself just create in the
same call is `Class::Destructive`, not `Class::Reversible` -- the same reasoning that makes
`doppler.service_token.rotate` destructive -- so a plan reaching this tool now requires approval. This
does **not** change any existing document's characterization: `workflows/appstore-signing-profile-from-doppler.yaml`'s
`plan` already fails earlier, at `issuer_id_text`'s `NotFound` (a missing sandbox secret), before the
`profile` node is ever reached, confirmed unchanged by `characterization_of_every_document` (still green,
byte-identical for every existing entry).

Landed (`crates/willikins-providers-appstore`, `crates/willikins-providers-fake`): the live tool's
`observe_instance`/`observe` replaced by `resolve_instance`/`resolve`, returning a private
`ProfileResolution` (`NotFound` / `Invalid { id }` / `Decided(Observation)`) instead of `Observation`
directly, so `ensure` can see the doomed row's id without a second read; a `create_new` helper factors
the create-or-ambiguous-reread logic shared by the `NotFound` and `Invalid` arms. The fake tool mirrors
the same shape in memory (drops the `INVALID` record from its `Vec` before appending a fresh one).
Mock tests (`profile_ensure_mock.rs`) cover every arm the task asked for: `INVALID` replaced
(delete-then-create, a genuinely fresh id); `ACTIVE` left untouched (no `DELETE` mock registered at
all, so an unexpected call would fail the test); `INVALID` of a different name never deleted (a
substring-neighbor row, excluded before `profileState` is ever inspected -- new fixtures
`profile_list_substring_neighbor_invalid.json`, `profile_post_created_after_replace.json`); a delete
failure (mocked `500`) stops before any create (`.expect(0)` on the `POST` mock, asserted). The fake
twin gained the same `Invalid` arm and `fake_agrees_with_live.rs`'s `profile_invalid_state_agrees` now
asserts agreement on `Observation::Absent` (previously on matching error kinds, since the old behaviour
was a shared terminal `Conflict`). Both catalog snapshots (`catalog_parity__appstore_profile_ensure_spec.snap`,
`willikins-providers-fake`'s own catalog snapshot) updated for `"class": "destructive"`.

**The gated live write cycle, extended, written but not run** (per the task: opus runs it):
`crates/willikins-providers-appstore/tests/live_write_cycle.rs` gains a second `#[ignore]`d test,
`appstore_live_profile_replace_cycle`, reusing that file's own credential, counting, and cleanup-guard
helpers. Apple offers no API to set `profileState` directly, and the account already carries two
unexplained `INVALID` profiles from an unknown cause (2026-09-23 handoff) -- so this harness does not
guess at another one. It uses the one *documented* way to produce a real `INVALID` profile: Apple's own
words, quoted in this plan's pre-flight, "Provisioning profiles that contain a modified App ID become
invalid." The cycle creates one throwaway identifier and one profile on it (as the existing cycle does),
enables `HEALTHKIT` (no setting, already live-proven able to enable by task 1's own cycle) on that same
throwaway identifier only, asserts the profile now reads `Absent`, calls `ensure` again and asserts a
genuinely different profile id with `changed: true`, converges on a third call, then deletes the live
profile and the identifier and confirms counts and independent `404`s exactly as the existing cycle
does. This is the one narrow, explicitly documented exception to that file's long-standing "no
capability is ever enabled here" invariant -- recorded in both the file's own module doc and here, never
silently widened. If a future run finds `HEALTHKIT` cannot be enabled, or Apple does not invalidate the
profile the way its own documentation says, the instructions are to stop and report rather than invent
another way to force `INVALID` -- in that case the replace-when-INVALID path stays proven only by the
mock arms and the fake's own tests above, which is an acceptable, explicitly stated fallback per the
task, not a gap to paper over.

Gates run (scoped, this task's own crates): `cargo fmt --all --check`; `cargo clippy -p
willikins-providers-appstore -p willikins-providers-fake --all-targets -j 2 -- -D warnings`;
`RUST_TEST_THREADS=2 cargo test -p willikins-providers-appstore -p willikins-providers-fake -j 2
--no-fail-fast` (green throughout: `willikins-providers-appstore`'s suites total 179 passed, 3 ignored
(the opt-in live probe), `profile_ensure_mock` alone 23 (five new); `willikins-providers-fake` green
throughout, `fake_agrees_with_live`'s `profile_invalid_state_agrees` now proves agreement on `Absent`
rather than on matching error kinds); `cargo check -p willikins-types -j 2`; plus, defensively (not this
task's own crate, but the one other place a class change could plausibly break something), `cargo test
-p willikins-cli --test appstore_profile_apply_redaction` (green -- that test already passes
`--approve`, so the class change is invisible to it) and `cargo test -p willikins-dsl --test acceptance`
(green, `characterization_of_every_document` byte-identical). `cargo clippy -p
willikins-providers-appstore --features live-tests --tests -j 2 -- -D warnings` and `cargo test
--features live-tests --test live_write_cycle --no-run` both green (compiles; never executed).

**Addendum:** 2026-09-28 (operator decision 6, designer) — **a manual step is a GATE node; an unmet
gate is BLOCKED, not failed.** The operator, verbatim: "if it's idempotent, it's even better than making
it pausable / resumable. It'd basically be: run all that can be ran, hit a blockage requiring user /
operator action / feedback, tell to re-run the document once action is done / feedback / info are
provided. If it's cheaply idempotent, then it's basically capable of pausing and resuming, without
requiring any feature-specific development." And: "because if it just fails or whatever, well...".
Decision (a) (manual steps as `template.render` outputs) is **superseded** by decision (j) below;
decision (b) (no profiles in the Walter document) was already overturned by operator decision 1 (the
T3a addendum above) and is marked superseded too. (j) is the smallest engine change that fits the
existing model: a gate is an ordinary **pure, read-only tool** that declares itself a gate through a
new default-`None` trait method, so no existing tool, catalog entry or document changes; its `Absent`
plans as `Action::Blocked`, every node downstream of it by data edge plans as `Action::Skip` without
being read, every other node plans and applies exactly as today, and the run ends `blocked` with a
structured list of what the operator must do and "re-run this document once done". No saved run
state. The engine work is three new tasks, **G1–G3**, which run **before T3**; the Walter-specific
gates (the app record exists; `APP_GROUPS` on all three identifiers) belong to T3. What `apply` does
today when a node fails, established from the code: it **stops**, and every later instance in plan
order, dependent or not, is `NotRun` (see (j), "Today").

**Addendum:** 2026-09-28 (implementer) — **G1 landed, three commits** (`e06d85c`, `650cae7`,
`c9fa63e`). `willikins-core`: `Gate`/`GateError`, `Tool::gate()` (default `None`), `Catalog::insert`'s
gate validation (not pure / subject not an input / subject `AnySecret` / subject secret, each with its
own test), `Action::Blocked`/`Action::Skip`, `plan`'s skip-set walk (`GateTracking`), `NodeResult::Skipped`,
`aggregate_for_each_port`'s any-instance-blocked rule, and `Plan.blocked: Vec<BlockedGate>`
(skip-if-empty). `crates/willikins-core/tests/plan_gates.rs`: small in-test tools only (a mixed
`for_each` gate, a shared `test.counted` read counter, four `Catalog::insert` refusals), 7 tests,
including a `downstream` node bound only to `consumer` (itself skipped only because of the blocked
`gate` instance) to pin the "holds transitively" claim one hop out, not just the direct case.
`willikins-cli`: `action_text`'s `Blocked`/`Skip` labels and `plan_text`'s `blocked:` section (decision
(j), point 7), two new render tests (acceptance 13). Gates run: `cargo fmt --all --check`; `cargo
clippy -p willikins-core --all-targets` and `-p willikins-cli --all-targets` (both `-D warnings`,
both clean); `cargo test -p willikins-core` (19 suites) and `-p willikins-cli` (15 suites), both green;
`cargo check -p willikins-types`; `cargo test -p willikins-dsl --test acceptance`
(`characterization_of_every_document` byte-identical, acceptance 9). Every `willikins_core::Plan`
literal outside this crate (ten `willikins-journal` test fixtures, verified compiling under the `-p
willikins-cli` clippy pass since it pulls that crate in) gained `blocked: Vec::new()`, purely
mechanical. `mcp_server__the_tool_list_and_every_schema_is_snapshotted`'s insta snapshot moved
additively one commit early (`PlanResponse` embeds `Plan` directly), fixed and reverified green
(6/6) rather than left for G2. **Honest limits, for G2/G3:** no gate tool exists yet in the fake
catalog, so the CLI/MCP `blocked` surface is proven only at the render layer, never through a real
`plan`/`apply` binary run — that needs a fake gate tool or T3's own Walter gates. `Plan.blocked`
appears in the published JSON Schema's `required` array despite `skip_serializing_if` (same
pre-existing pattern as `PortSpec.derived_only`); an MCP client validating strictly against the
schema would reject a gate-free plan. `BlockedGate` derives `Serialize`/`JsonSchema` only, not
`Deserialize`, though `NodeName`/`ToolName`/`PortName` all already do — trivial to add when G2 needs
it for the journal.

**Addendum:** 2026-09-28 (implementer) — **G2 landed, two commits** (`393b4bf`, `fe01d97`).
`willikins-core`: `apply` now classifies each planned instance's `Action` *first*, before
`resolve_instance_inputs` and before the `pure` branch — a `Blocked` gate (always pure) is reported
`NodeStatus::Blocked` rather than `Computed`, and a `Skip` instance is reported `NodeStatus::Skipped`
without ever resolving its (empty, by construction) bindings, which would otherwise panic on a
literal-bound required port or misdeliver `Unknown` through a conversion edge. Neither tool is
called; the walk continues past both, and only a genuine tool failure still stops it with a
`NotRun` tail. `Applied` gains `blocked: Vec<BlockedGate>` (skip-if-empty), carried straight from
rule 2's fresh re-plan; the `for_each` grouping threads each instance's own `blocked` flag into
`ForEachInstance` instead of the placeholder `false` G1 left; a whole-node `Skip` (a `for_each`
source itself blocked, planned as one entry with `instance: None`) now builds `NodeResult::Skipped`
rather than reaching the `unreachable!` a for_each grouping with no keyed instance used to hit.
`willikins-journal`: `Outcome` gains a third variant, `Blocked { outputs, blocked }`, chosen by one
`outcome_of` helper shared by `run_and_journal` and `continue_run_and_journal` so `Butler` and the
CLI path never disagree; `RunState` gains `Blocked`; `RunRecord` gains `blocked` (skip-if-empty) and
`next_step` (skip-if-none, the new `BLOCKED_NEXT_STEP` constant), both filled by the fold.
`BlockedGate` gained `Deserialize` (the trivial addition G1 flagged), since nothing in it is ever a
`Value`. `crates/willikins-core/tests/apply_gates.rs` (acceptance 14, small in-test tools only, the
same choice `plan_gates.rs` made): a blocked run still creates every independent resource; a second
apply once the gate opens runs the previously-skipped node and converges; a third reads everything
`Unchanged`/`Computed`; a gate flipped between plan and apply refuses as `Action` drift before
anything runs; a tool failure elsewhere in the same run still stops with a `NotRun` tail while the
earlier `Blocked`/`Skipped` statuses survive in the partial `Applied`. `willikins-journal` gained a
`views.rs` fold test and an `event_shapes.rs` round-trip test for `Outcome::Blocked`; the two
frozen-fixture suites (`pre_pass_2_replay`, `post_pass_2_shapes`) needed the new match arms their
own exhaustive matches now require — both still replay their frozen fixtures byte for byte.
`willikins-cli`: `node_status_text`/`run_state_text` gain the new labels; `run_record_text` reuses
`plan_text`'s own `blocked:` section and adds a trailing `next_step:` line; `exit_for_run_state`
maps `RunState::Blocked` to exit code **3** (distinct from a failure's 1). `willikins-server`:
`plan` and `run_status`'s MCP descriptions say what to do on a non-empty `blocked` (forward
`need`/`how`/`subject`, then re-`plan`); the tool-list snapshot moved additively (both descriptions,
`BlockedGate`'s schema now under `run_status`'s output too, `NodeStatus`'s two new variants,
`RunState::Blocked`). Gates run, both commits: `cargo fmt --all --check`; `cargo clippy` per touched
crate pair `--all-targets -D warnings`; `cargo test` per touched crate pair (core+journal 36 suites,
cli+server full suite including `mcp_server`, `adversarial_10a`/`10b`/`11`/`13`, `blocking_pool_13`,
`http_server`, all green); `cargo check -p willikins-types`; `cargo test -p willikins-dsl --test
acceptance` (`characterization_of_every_document` byte-identical); the defensive
`appstore_profile_apply_redaction` run (green — that test already passes `--approve`, so the
Destructive class change stays invisible to it). **Honest limit, same as G1's:** exit 3 and the
CLI/MCP blocked surface are proven at the render/unit level only — no gate tool exists in the fake
catalog yet, so driving `Action::Blocked`/`Skip` through a real `plan`/`apply` *binary* run is still
G3/T3's job, once Walter's own gates (or a fake gate tool) exist. Plan not marked Completed.

**Addendum:** 2026-09-28 (implementer) — **G3 landed, four commits** (`70e5e0a`, `3f41d6d`,
`0b13a51`, `daffc94`; `e9b6235` is this addendum's own first version, corrected here rather than
amended).
`willikins-types`: new public `OperatorAcknowledgement` (grammar exactly `done`, registry entry).
`willikins-core`: recognised everywhere by its registry entry's `TypeId`
(`crate::value::is_operator_acknowledgement`, via `TypeRegistry::type_matches`), never by comparing
`DomainType::TYPE_NAME` strings (the milestone 3d rule) — `check` refuses a default on a workflow
input of this type (`CheckError::AcknowledgementDefault`) and a literal bound to a port of it
(`CheckError::AcknowledgementLiteral`), each with its own unit test plus a DSL-level negative
fixture (`workflows/fixtures/acknowledgement-default.yaml`; `acknowledgement-literal.yaml` followed
in a third, small commit once `operator.acknowledge` existed for it to name; acceptance 16);
`describe` lists an unsupplied
input of this type under a new `awaiting` field (skip-if-empty, same convention as `Plan.blocked`)
instead of `missing`, so it never blocks `plan` the way a genuinely missing input does; `plan`'s
`Binding::Input` arm resolves such an unsupplied input to `Value::unknown` instead of
`PlanError::MissingInput`, never adding it to the resolved map, so `PlanRecorded.inputs` and the
butler's rebuild from the journal stay untouched; `BlockedGate` gained `awaiting_inputs` (the
workflow inputs bound to a blocked gate's own ports whose value is still `Unknown`, declared order,
deduplicated), proven end to end against a small in-test gate tool in
`crates/willikins-core/tests/plan_gates.rs` (an unsupplied input blocks the gate and is named in
`awaiting_inputs`; supplying `done` makes it `Compute`). `willikins-tools`: the production
`operator.acknowledge` gate (pure; `step: Text` the one subject; `acknowledged:
OperatorAcknowledgement`; reads `inputs.get` directly rather than through `helpers::get`, which
would fail on an `Unknown` value instead of reporting the gate unmet). `LIVE_TOOL_NAMES` moved
26 → 27 at every pinned site (`willikins-server`'s const, `insert_pure_tools`, the
`from_provider_arrays` partition test; `willikins-providers-fake`'s catalog, now twenty-nine tools;
`willikins-tools`' own `register()`); `crates/willikins-core/tests/operator_acknowledge_document.rs`
is the positive fixture acceptance 16 asks for, proving the *production* tool (not only G3's
in-test double) blocks without the input and computes with it, through the real fake catalog;
`willikins-cli`'s `plan_text` renders one `supply: --input <name>=done` line per
`awaiting_inputs`, engine-rendered, never tool-authored. Every touched crate's own scoped gates
(`fmt`, clippy `-D warnings`, tests) are green; snapshots (three in `willikins-core`, one each in
`willikins-journal`, `willikins-dsl`, `willikins-server`, `willikins-tools`,
`willikins-providers-fake`, plus `willikins-types`' own catalog snapshot) moved additively only;
`characterization_of_every_document` gained exactly two new fixture entries (`acknowledgement-default.yaml`,
`acknowledgement-literal.yaml`), byte-identical elsewhere. Two gaps a self-review caught after the
first two commits, each its own commit: `describe_text` (`willikins-cli`) rendered nothing for an
`awaiting` input, so `willikins describe` in text mode silently dropped it even though JSON mode
carried it; it now renders an `awaiting` block beside `missing`, same convention (`document says:`
included). And `AwaitingInput` was never exported from `willikins-core`'s crate root — an oversight
in the first commit, invisible until something outside the crate needed to name the type. **Honest
limits, for T3:** the Walter document itself (its own app-record and
app-group gates, profiles behind the app-group gate) is unwritten — T3's job; no live run has
exercised `operator.acknowledge` against a real MCP client end to end, only the fake catalog and
the CLI text renderer at the unit level, the same honest limit G1/G2 recorded for the engine
mechanism generally.

**Addendum:** 2026-09-28 (implementer) — **T3b landed, two commits** (`1c65573`, `b1526aa`).
`willikins-types`: two new conversion rows out of `AppleBundleIdentifier`, registered through
`conversions!` beside milestone 3d's `AppleBundleIdentifier => AppleProfileName` row —
`=> AppleBundleIdName` (its own hand-written `From` impl in `appstore.rs`, proved by the identical
grammar-containment argument the profile-name row already carries, since `AppleBundleIdName::parse`'s
four refusals are byte-for-byte the same as `AppleProfileName::parse`'s) and `=> Text` (its `From`
impl in `text.rs`, not `appstore.rs`, since `Text`'s tuple field is private to its own module; the
proof is simpler still, since `Text::parse` bounds only length at 65536, well over
`AppleBundleIdentifier`'s own 255-character bound, and refuses nothing else). Each row carries the
same three proptest strategies as the existing row (the bundle-identifier grammar directly, the
implication over arbitrary strings, and the identifier's own alphabet at lengths 1–300 straddling
the 255 bound) plus boundary unit tests at exactly 255 and 256 characters. `conversion_rows()`'s
stale "to date, only one" doc comment is corrected. Acceptance 7's reverse-direction fixture,
`workflows/fixtures/appstore-bundle-id-text-into-identifier.yaml`, binds a `Text`-typed input to
`appstore.bundle_id.ensure`'s `identifier` port following
`appstore-profile-name-into-identifier.yaml`'s exact pattern; `check` still refuses it with the same
`TypeMismatch` it always did (a new acceptance test in
`crates/willikins-providers-appstore/tests/bundle_id_documents.rs`), proving the new row does not
loosen `check` in the direction it was never registered for. The `AppleBundleIdName` reverse needed
no second document-level fixture, since its grammar is identical to `AppleProfileName`'s and the
"space is not an identifier" fact is pinned at unit level instead
(`a_bundle_id_name_with_a_space_is_not_a_bundle_identifier`). `characterization_of_every_document`
gained exactly the one new fixture's entry, byte-identical elsewhere. Scoped gates green throughout:
`willikins-types` (408 tests, including the 11 new ones), `willikins-providers-appstore` (4 tests),
`willikins-dsl`'s acceptance suite (5 tests, snapshot re-verified green after accepting the one
additive hunk), clippy `-D warnings` on both touched crates, `cargo check -p willikins-types`, `cargo
fmt --all --check`. **These are exactly the two rows decisions (a) and (h) already named as needed,
not new design.** Decision (a) (superseded by (j), kept for history) names
`AppleBundleIdentifier => Text` by name for a manual step's `value`, bound straight from
`inputs.app_identifier` so the dry run's text names its own throwaway identifier rather than a
literal; `template.render`'s `value` port is `exact("Text", true)`
(`crates/willikins-tools/src/template_render.rs`), so this is a real `Exact` edge, not a polymorphic
sink incidentally accepting it. Decision (h) names `AppleBundleIdentifier => AppleBundleIdName` for
every bundle-identifier `name` port, "the operator's 'bundle id everywhere' habit ... exactly as 3d
did for profile names." T3b supplies both rows T3 will bind through; no further conversion work is
owed to T3. Test-first process note: the impl and its proptests were written in the same edit pass
rather than red-then-green in separate steps, since the `From` impl is what the proptest module needs
to compile at all (the missing impl is a compile error, not a runtime red); the tests were run once,
immediately after, never against a stub.

**Addendum:** 2026-09-29 (implementer) — **T3c landed, four commits** (`98bad49`,
`5af1503`, `7337276` this addendum's own first version, `16bef8c` a review-driven
follow-up). Task T3 as amended by decisions 1, 2 and 6.

Commit 1 (`98bad49`): the two Walter gates T3 needed and the plan left undesigned
("Walter, for T3 (not designed here)") — `appstore.app.get` (M1, a leaf: `GET
/v1/apps` filtered by `bundleId`, exact client-side compare, paginated the same
defensive way `list_bundle_ids` treats an unconfirmed filter semantic) and
`appstore.app_group.gate` (M2, one per identifier: resolves the parent bundle id
the same "unregistered is `Absent`" way `appstore.bundle_id_capability.ensure`
does, then checks the `APP_GROUPS` row on `list_bundle_id_capabilities`; passes
`identifier` through as its own output, per decision (j) point 2, so a profile
node orders after *this* gate rather than merely after registration). Both
`pure`, no key, `Class::Reversible`, `ensure` the identity of `read`. Fake twins,
catalog registration, `LIVE_TOOL_NAMES` 27 → 29 at every pinned site (including
a pre-existing one-behind staleness in `willikins-providers-doppler/tests/live_catalog.rs`,
fixed in passing, not introduced here), `catalog_parity`, `fake_agrees_with_live`
(six new behavioural-agreement tests), `pure_tools_agree.rs`'s 13 → 15 case list.
Scoped gates green: fmt; clippy `-p willikins-providers-appstore -p
willikins-providers-fake -p willikins-server -p willikins-providers-doppler
--all-targets -D warnings`; `cargo test` over the same four crates; `cargo check
-p willikins-types`; `cargo test -p willikins-dsl --test acceptance` (byte-identical
-- no document names either tool yet).

Commit 2 (next): `workflows/walter-ios-app.yaml` and
`crates/willikins-cli/tests/walter_document.rs`. The document: the seven-node
ASC credential chain; `app_id`/`nse_id`/`widgets_id` (names bound through the
`AppleBundleIdentifier => AppleBundleIdName` conversion, decision (h)); the host
capabilities `healthkit`/`push`/`data_protection` (the last with `setting:
${{ inputs.data_protection }}`, default `PROTECTED_UNTIL_FIRST_USER_AUTH`); the
leaf `app_record` gate; the three `app_group.gate` nodes (`app_app_groups`/
`nse_app_groups`/`widgets_app_groups`); the three profiles (named through the
`=> AppleProfileName` conversion, each binding `identifier` from its own
app-group gate, never from the registration node — the data edge decision (j)
point 2 asks for); their content into Doppler (`app_profile_to_doppler` etc.,
against a dedicated `prd_config` node, since `doppler.secret.set`'s `config`
port is `derived_only`); `naming.v1`, `doppler.project.ensure`, per-environment
root configs and their inheritance; `monorepo_ref` (`github.repo.get`, never
`ensure`); the Buildkite cluster and pipeline; and the four remaining manual
steps (M3/M5/M6/M7) as `operator.acknowledge` leaves. M0 (base-config names) and
M4 (profiles) are gone, exactly as decisions 2 and 1 say; M1/M2 are the two
observed gates above.

**One real gap, one re-diagnosed as a verify item, both recorded rather than
papered over:**
1. ~~**No tool can create a *named branch* Doppler config.** Operator decision 2
   asks for one config, `prd_deployment_ios`, inheriting the three base
   configs; `doppler.config.ensure` only ever derives a config named after its
   *environment* (`naming::v1::doppler_root_config`), and `EnvironmentSlug`'s
   own grammar (kebab-case, 16 characters) cannot even spell that name (snake_case,
   19 characters). The document instead makes every root config (`dev`/`stg`/`prd`)
   inherit the three base configs — strictly broader than asked (`dev`/`stg`
   gain the deployment credentials too). A new tool (or an optional `name` port
   on `doppler.config.ensure`) is the real fix; out of this task's scope.~~
   **Closed 2026-09-29, task B1** — `doppler.branch_config.ensure` now exists
   and the document creates the operator's real, single `prd_deployment_ios`
   config; see the B1 addendum below.
2. **Not a type gap after all, on reflection — a verify item.** The first cut
   of this addendum read decision 2's hyphenated `github` project config names
   (`lightless-labs`, `bande-a-bonnot`) as things `willikins_types::DopplerConfigName`'s
   grammar (`[a-z0-9_]+`) cannot express, and proposed widening the grammar.
   But `naming::v1::doppler_root_config`'s own doc already states this
   codebase's convention: "Doppler config names use underscores rather than
   hyphens" — and Doppler's own platform docs do not settle whether a config
   slug accepts a hyphen at all (`docs/research/2026-09-12-m2-dependencies.md`
   line 589). So decision 2's spelling is very likely just human-readable
   prose for a slug already named `bande_a_bonnot`, not a name this document
   cannot express. The document's `base_configs` default uses that underscored
   spelling; the honest ask is to list project `github`'s configs read-only
   and confirm the slug before a real apply, not to widen a type that may
   already be correct.

Both are recorded in the document's own header comment and carried to the
operator via `needs_operator`/the verify list, not worked around.

Tests: `crates/willikins-cli/tests/walter_document.rs`, in-process against the
fake catalog (`check`/`plan`/`apply`, `Approval::Human` since
`appstore.profile.ensure`'s `Class::Destructive` requires it on every run,
including a run whose profiles are all skipped). One shared `FakeState` across
three sequential runs: (1) nothing seeded beyond the credential chain, the
monorepo, the Buildkite cluster and the certificate — `app_record` and the
three app-group gates `Blocked`, the four acknowledgement leaves `Blocked`, the
three profiles and their `doppler.secret.set` nodes `Skip` (never read), every
independent node (bundle ids, capabilities, Doppler, Buildkite, the monorepo
reference) plans and applies for real; (2) the fake state is mutated directly
between runs (`apple_apps`/`apple_bundle_id_capabilities`) to stand in for the
operator's own portal work — a fresh plan shows exactly the four acknowledgement
leaves still blocked, the two observed gates now `Compute`, and the three
profiles plus their Doppler writes newly `Create`, with every node from run 1
reading `Unchanged`/`NoOp`; (3) the four acknowledgement inputs are supplied
(`done`) — nothing is `Blocked` or `Skip`, every node applies clean. A second
test proves `check` alone (the type system's own static proof that no secret
reaches a non-secret port). Every plan/applied JSON, across all three runs, is
asserted free of the signing key's PEM marker and the fake profile tool's own
plaintext content prefix, carrying only `[REDACTED` markers for the two secret
ports (`key`, `content`). Characterization: the new document's own entry
(byte-identical elsewhere; fails `plan` at `issuer_id_text`'s `NotFound`, same
precedent as the signing document).

Scoped gates for commit 2: `cargo fmt --all --check`; `cargo clippy -p
willikins-cli --all-targets -D warnings`; `cargo test -p willikins-cli` (full
crate, green); `cargo check -p willikins-types`; `cargo test -p willikins-dsl
--test acceptance` (characterization snapshot additive only).

**Honest limits.** The dry run (task's own "written and run by the attacker
after task 3") is not part of this commit — it needs a working sandbox
Buildkite token (still `401` as of the plan's pre-flight) and is explicitly the
next lane's job. The live capability cycle's own gaps (verify items) are
unaffected by this task. Plan not marked Completed.

**Follow-up commit (`16bef8c`), after a review pass:**

- **A real engine defect found, T3-blocking for the acknowledgement path
  specifically.** `crates/willikins-server/src/butler.rs::resolve_recorded_inputs`
  (the `for (name, spec) in &checked.workflow.inputs` loop) requires **every**
  declared workflow input to have an entry in the journal's recorded
  `PlanRecorded.inputs` JSON, or refuses with `ButlerError::RecordedInputUnreadable`.
  But G3's own design (decision (j) point 6, "the input stays absent from the
  resolved map") deliberately never records an unsupplied `OperatorAcknowledgement`
  input there at all. So **any** document with an acknowledgement gate left unmet
  on the very run that should exercise decision 6's "run, get blocked, re-run"
  fails instead: exit 1, "recorded input `<name>` could not be read back: the
  plan recorded no value for input `<name>`" — never a blocked run. Reproduced
  live through the built binary (`crates/willikins-cli/tests/walter_apply_blocked_redaction.rs`'s
  own module doc has the exact repro: remove any of the four `--input
  *_done=done` lines from that test and it fails this way instead of blocking).
  This is not specific to Walter or to this task — it is `willikins-server`'s
  own surface, and it blocks **every** caller that rebuilds inputs from the
  journal: one-shot `apply --approve`, `apply --plan-id`, and (unverified, same
  code path) MCP's `apply`. The bounded fix: in `resolve_recorded_inputs`, a
  missing entry for an input whose declared type is `OperatorAcknowledgement`
  (`crate::value::is_operator_acknowledgement`, never by name) should leave
  that input out of the resolved map entirely, mirroring `plan.rs`'s own
  `Binding::Input` arm — but not made here: it is G3's engine surface, and per
  this repo's own process it needs its own attack pass, not a same-session
  patch. Filed for the coordinator to dispatch, not worked around.
- Test assertions tightened per review: run 2's `Created` set is asserted
  exactly equal to the six newly-unblocked nodes; `app_app_groups`'s own
  `holds_back` is asserted exactly `[app_profile, app_profile_to_doppler]`
  (the ordering claim the whole design rests on); each acknowledgement gate's
  `awaiting_inputs` is asserted exactly its own `*_done` input; run 3 is
  asserted to create nothing **except** the three `doppler.secret.set` nodes,
  which — by that tool's own documented design, a write-only sink that can
  never compare against what is already stored — report `changed: true` on
  every apply, not only the first. Any future dry-run harness should expect
  the same: step 5's "every node `Unchanged`" does not hold for those three.
- Gap 2 (above) re-diagnosed from "the type is too narrow" to "verify the real
  slug" — see the corrected text above and a new verify item 12.
- A new journal-level test, `crates/willikins-cli/tests/walter_apply_blocked_redaction.rs`:
  a real `willikins apply --approve` binary run, the two observed gates
  deliberately unmet, the four acknowledgements supplied (to avoid the defect
  above, which is not what this test means to prove) — exit **3**, the
  `blocked:` section naming `app_record`/`app_app_groups`/`nse_app_groups`/
  `widgets_app_groups`, "re-run this document once done", and neither stdout,
  stderr nor the journal file ever carrying the signing key's PEM marker or a
  fake profile's plaintext content.

**Addendum:** 2026-09-29 (implementer) — **task B1 landed: `doppler.branch_config.ensure`,
closing gap 1 from T3c's addendum, and the Walter document now creates the
operator's real `prd_deployment_ios` layout (operator decision 2, 2026-09-28).**

Established from `docs.doppler.com`'s own `.md` twins, fetched verbatim
2026-09-29 (`configs-create.md`, `configs-get.md`, `configs-object.md`,
`branch-configs.md`): `POST /v3/configs` (body `project`, `environment`,
`name`) creates a branch config; `GET /v3/configs/config` (already the
endpoint `doppler.config.ensure` uses) reads one back, `root: false` and an
`environment` field distinguishing it from an environment's own root config.
Whether that `name` must already carry the `<environment>_` prefix or
Doppler applies it server-side is *not* settled by either page — but it is
already settled **empirically**, and pinned in this very plan
(`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`, "Notes for
milestone 3"): the milestone 2 live write cycle posted `name: "probe"` under
environment `dev` and got a `400`, then posted `name: "dev_probe"` and had it
stored as `dev_probe`, `root: false` — Doppler does **not** prefix
server-side. A caller must supply the full, already-prefixed name.

**New tool, not a port on `doppler.config.ensure` — reconsidered once, then
settled.** The first draft of this task treated the gap as "extend
`doppler.config.ensure` with an optional `name` port", reasoning by analogy
to `appstore.bundle_id_capability.ensure`'s optional `setting` port and the
mechanical fact (verified against `crates/willikins-core/src/plan.rs`, not
assumed) that `for_each` instance keys are `Value::render()` of the item, not
`ToolSpec.key` — so `spec.key` genuinely does not need to grow to
disambiguate two named configs under one environment, contrary to an
initial mechanical objection. That fact stands, but it was the wrong reason
to decide the question either way: the real discriminator is the
**observation contract**. `doppler.config.ensure`'s `Present` is "`200` and
`root: true`"; a branch config's is "`200`, `root: false`, and this
environment" — the flag the root tool keys its whole identity on is
*inverted*, not merely narrowed under the same predicate the way `setting`
narrows `appstore.bundle_id_capability.ensure`'s (same endpoint, same
`Present` shape, one more conjunct). `ensure`'s create path also calls a
genuinely different endpoint (`POST /v3/environments` versus `POST
/v3/configs`). A tool whose observation contract flips depending on whether
an optional port is bound is two tools sharing one spec, and three things
already on `main` pin the root tool's identity to "root, specifically": its
own description, the `docs/HANDOFF.md` architecture-gotcha ("`doppler.config.ensure`
needs `root: true`"), and this plan's own T3c framing of this exact gap
("the naming rule is settled, **the tool is not**", line ~386 before this
addendum). So: `doppler.branch_config.ensure`, a new tool, following the
dotted-name style `doppler.config.inherits.ensure` already set. Full
rationale, including the reconsidered mechanical argument, is in the tool's
own module doc
(`crates/willikins-providers-doppler/src/tools/branch_config_ensure.rs`).

**Port shape: `branch` is the suffix, not the full name.** Given the
prefix is mandatory and the tool must assemble it, the port takes only the
suffix (`deployment_ios`, typed `DopplerConfigName` — its grammar and length
bound already fit a suffix as well as a full name); `full_name` snake-joins
`<environment's words().snake()>_<branch>` and **re-parses** the result as a
`DopplerConfigName` before it ever reaches the wire, so Doppler's 60-character
"Config Slug" cap (documented as counting the environment prefix) fails
loudly at construction for a branch name that would not fit, rather than
reaching Doppler as an unparseable or ambiguously-refused request. This
mirrors `naming::v1::doppler_root_config`'s own snake join, but is **not**
added to `naming::v1` (frozen) or a new `v2` row: assembling a provider's own
mandatory wire format is mechanism, the same reasoning `doppler_root_config`
itself already rests on, not the policy `naming::v1` exists to hold — and
today only one document needs it.

**Read.** `Present` needs `root == Some(false)` **and** the fetched config's
own `environment` field to equal the one this tool was asked to ensure under
(`ConfigBody` gained an `environment: Option<String>` field for this);
anything else at `200` is `Foreign` (mirroring, inverted, the collision
guard `doppler.config.ensure`'s own `root: true` check already
carries — a same-named branch under a different environment, or, vanishingly
unlikely, an environment's own root config sitting at this literal name).
A missing project or config is `Absent` through the same
`looks_like_a_missing_project` predicate `doppler.config.ensure` already
applies to the identical endpoint.

**Verify item, not assumed true** (new verify item 13, added below): what
Doppler answers `GET /v3/configs/config` for a branch config that is absent
from a project that *does* exist — as opposed to the missing-*project* case
every Doppler `read` in this crate already tolerates — is unestablished by
any primary source read for this task. `read_still_propagates_a_400_with_an_unrelated_message`
(both this tool's own mock suite and `doppler.config.ensure`'s) shows an
unknown `400` still fails the plan today; this task pins only the `404`
shape as `Absent` and records the gap rather than guessing.

**Landed:** `crates/willikins-providers-doppler/src/client.rs` (`create_branch_config`,
`CreateBranchConfigBody`, `ConfigBody.environment`); `crates/willikins-providers-doppler/src/tools/branch_config_ensure.rs`
(new); the fake twin `crates/willikins-providers-fake/src/tools/doppler_branch_config_ensure.rs`
(membership-only, like its sibling — cannot model `Foreign`, recorded in its
own module doc and in `fake_agrees_with_live.rs`'s updated header rather than
worked around); thirteen new mock tests
(`crates/willikins-providers-doppler/tests/branch_config_ensure_mock.rs`);
two new `fake_agrees_with_live.rs` tests (`Present`/`Absent` only, per the
limit above); a new `catalog_parity` spec-equality test plus its insta
snapshot; `LIVE_TOOL_NAMES` 29 → 30 and `DOPPLER_TOOL_NAMES` 9 → 10 in
`crates/willikins-server/src/catalog.rs` (`insert_doppler_tools` inserts it
right after `doppler.config.ensure`); the fake catalog's own count 31 → 32
and its `catalog_json_snapshot` (additive); every touched doc-comment count
corrected in place (`willikins-providers-doppler/src/lib.rs` and
`tools/mod.rs`, `willikins-providers-fake/src/lib.rs`,
`willikins-providers-doppler/tests/live_catalog.rs`).

**`workflows/walter-ios-app.yaml` rewired** to the real layout: `prd_config`
now calls `doppler.branch_config.ensure` (`project`, `environment: prd`,
`branch: deployment_ios`) instead of re-`ensure`ing the `prd` root config a
second time; `inherit` now binds `config` from `prd_config`'s own output as
a single scalar node (no longer `for_each`-ed over all three root configs);
`configs` (the `dev`/`stg`/`prd` root configs) is unchanged and gains **no**
inheritance — closing gap 1 from the T3c addendum exactly as decision 2
asked, and narrowing what `dev`/`stg` can see (they no longer inherit the
deployment credentials the old root-config approximation leaked to them).
The document's own header comment and `base_configs`' description are
corrected in place (dated, not silently rewritten). **Correction to this
addendum's own first version:** it said the SHARED VALUES table's "Sandbox
Doppler base config (dry run)" row (`bande-a-bonnot-shared/ios_base`) was
superseded by this task. On inspection that row is unaffected — it is the
*dry-run harness's own sandbox stand-in* (the sandbox workplace holds none of
decision 2's three real base configs, so the not-yet-run dry run still needs
to create `bande-a-bonnot-shared/ios_base` and pass it as its own
`base_configs` override, exactly as "The dry run" section already states).
Only the *document's own default* for `base_configs` stopped naming
`bande-a-bonnot-shared/ios_base` — done by T3c, not by B1 — and that row was
already marked superseded there. Nothing in the SHARED VALUES table needed a
change.

**Characterization snapshot: one existing entry changed, not only new ones
added — flagged, not swept under.** The monorepo instructions this task ran
under state the snapshot "may change only by the addition of new
documents". `crates/willikins-dsl/tests/acceptance.rs`'s `characterize`
records every node/port's resolved type for the whole document, so rewiring
`prd_config`'s tool and inputs necessarily adds one line to the **existing**
`walter-ios-app.yaml` entry (`prd_config.branch: DopplerConfigName`) and
changes nothing else in it (`inherit.config`/`inherit.inherits` keep their
existing types and lines, since the node's own port names and types are
unchanged — only what feeds `config` changed, which this report does not
capture instance-by-instance). Verified by diff before accepting: every
other document's entry in the snapshot is byte-identical; the walter entry's
only change is that one additive line. This is the direct, necessary
consequence of the task's own instruction to rewire the Walter document
itself — recorded here rather than accepted silently, per the task's own
"stop and report rather than improvise" posture.

**Verify item 13** (new, alongside items 1–12 above): the absent-branch-config-in-an-existing-project
`GET` response shape (400 vs 404) is unverified; see "Read" above.

Gates run (scoped, this task's own crates): `cargo fmt --all --check`;
`cargo clippy -p willikins-providers-doppler -p willikins-providers-fake -p
willikins-server --all-targets -j 2 -- -D warnings` (clean); `RUST_TEST_THREADS=2
cargo test -p willikins-providers-doppler -p willikins-providers-fake -j 2
--no-fail-fast` (green throughout, every new test passing, both new insta
snapshots additive-only, verified by diff); `RUST_TEST_THREADS=2 cargo test
-p willikins-server -j 2 --no-fail-fast` (catalog tests, MCP tool-list
snapshot, additive-only **apart from two pre-existing, unrelated failures**,
below); `cargo test -p willikins-cli -j 2 --no-fail-fast`
(`walter_document.rs`'s three-run graph test unchanged and still green — the
tool swap is transparent to its assertions, which never name `configs`/
`inherit`/`prd_config` individually); `cargo check -p willikins-types -j 2`;
`cargo test -p willikins-dsl --test acceptance -j 2` (the one flagged,
verified additive change above). Plan not marked Completed.

**Found, not fixed — for the coordinator, since the full workspace gate will
hit these regardless of this task.** `cargo test -p willikins-server`
(unrelated to this task's own diff, but a scoped gate this task's own touch
of `catalog.rs` required) surfaces two pre-existing failures, both hardcoded
workflow-directory listings never updated when T3c added
`workflows/walter-ios-app.yaml` (T3c's own gates never ran `cargo test -p
willikins-server` at all):
`crates/willikins-server/tests/acceptance_13_trusted_directory.rs::starting_against_the_real_workflows_directory_succeeds_and_journals_server_started`
(a hardcoded `names` array missing `"walter-ios-app"`) and
`crates/willikins-server/tests/image_contents.rs::the_image_workflows_directory_holds_exactly_the_thirteen_positive_documents`
(a hardcoded set and its "thirteen" claim, now fourteen). Both are one-line
fixes (add the missing entry, correct the count) with no design content;
neither touches Doppler or anything this task changed. Filed for the
coordinator to dispatch, matching this plan's own precedent for the
`resolve_recorded_inputs` defect T3c's follow-up found.

**Addendum:** 2026-09-29 (implementer) — **task B2 landed, two commits
(`162d7f7`, `ffc418d`): the `resolve_recorded_inputs` defect T3c's
follow-up recorded is fixed test-first.** `willikins_core::value::is_operator_acknowledgement`
moves from `pub(crate)` to `pub` (the same registry-`TypeId` test, never
by name) so `willikins-server`'s `resolve_recorded_inputs` -- a different
crate -- can recognise an unsupplied `OperatorAcknowledgement` input the
same way `plan.rs`'s own `Binding::Input` arm already does, and leave it
out of the resolved map instead of refusing with `RecordedInputUnreadable`.
New end-to-end test over the fake catalogue,
`crates/willikins-server/tests/acknowledgement_gate_blocked_resume.rs`,
against a new positive fixture (`workflows/fixtures/acknowledgement-gate-resume.yaml`:
one independent node, one `operator.acknowledge` gate, one node
downstream of the gate) — proves, through a real `Butler::apply` over a
real `FileJournal` (reopened independently mid-test to prove durability,
mirroring `tests/file_journal_round_trip.rs`) and the MCP surface
(`plan`/`apply`/`run_status`): the first run starts and blocks rather
than refusing (the independent node computes, the gate blocks, the
downstream node is skipped, `run_status` reports `blocked` with the
fixed `next_step` text); a second run, over a fresh `Butler` reopening
the same on-disk journal once the operator supplies the acknowledgement,
converges (the downstream node now computes, nothing blocked). The DSL
characterization snapshot gains exactly the new fixture's own entry,
verified by diff before accepting. A second, unrelated pre-existing
failure was found and fixed in passing, in this task's own scoped
`willikins-core` gate: `catalog::tests::list_tools_json_snapshot`'s
hardcoded conversions literal was one commit behind T3b's own two new
rows (`AppleBundleIdentifier => AppleBundleIdName`/`=> Text`), landed
before this task started — a mechanical, no-design-content fix, its own
commit (`162d7f7`), matching this plan's own precedent
(`live_catalog.rs`'s one-behind `LIVE_TOOL_NAMES`, B1's addendum above).
**Not re-attacked** in this task: `RecordedInputUnreadable`'s *other*
failure mode (the whole recorded `inputs` payload being unreadable JSON)
is unchanged and untouched; the two willikins-server pre-existing
failures B1 already filed for the coordinator
(`acceptance_13_trusted_directory.rs`, `image_contents.rs`, both about
`walter-ios-app.yaml`'s own entry) still fail, confirmed unrelated to
this task's diff (neither file nor test this task touched). Gates run
(scoped): `cargo fmt --all --check`; `cargo clippy -p willikins-core -p
willikins-server --all-targets -j 2 -- -D warnings` (clean); `cargo test
-p willikins-core -j 2` (19 suites, all green); `cargo test -p
willikins-server -j 2` (green apart from the two pre-existing,
unrelated failures above); `cargo check -p willikins-types -j 2`; `cargo
test -p willikins-dsl --test acceptance -j 2` (additive-only, verified
by diff). Plan not marked Completed.

**Addendum:** 2026-09-29 (attacker, second pass) — **replace-when-INVALID, the gates, the
acknowledgement, the conversions and the Walter document attacked; three defects fixed test-first, two
red-on-arrival suites fixed, one test gap closed, eleven mutations (one survived, then killed).** Record:
`docs/research/2026-09-29-m3e-adversarial-pass-2.md`.
- **Every gate-free MCP `describe`, `plan` and `run_status` result failed its own published
  `outputSchema`** (`d986c75`). `Plan.blocked`, `Applied.blocked`, `Description.awaiting` and
  `RunRecord.blocked` are skipped when empty, but rmcp builds output schemas for the deserialize
  contract, which listed them as `required` — so decision (j)'s "a document with no gate is unaffected"
  held for the bytes and not for the contract. G1's addendum had called this cosmetic; a validating MCP
  client refuses such a result. The four fields now carry `serde(default)`; a new test
  (`crates/willikins-server/tests/mcp_output_schema_conformance.rs`) validates `describe`, `plan`,
  `apply` and `run_status` against the server's own schemas for a gate-free and a blocked document. The
  schema snapshots moved by **removing** those `required` entries, deliberately.
- **`appstore.profile.ensure` deleted by the id the instance read answered with**, never checked against
  the row its exact name compare matched (`50de4a2`): now refused with `Provider`, nothing deleted.
- **A create failing after replace-when-INVALID's delete was journaled as the create's error alone**
  (`79a4f28`): it now says the `INVALID` profile was deleted and a re-run creates the replacement.
- **Held, by mutation:** a blocked gate's dependents are never read or run (m1c, m2, and m10 on the
  Walter document's own gate edge); a gate-free plan stays byte-identical in the characterization
  snapshot (m3); an acknowledgement cannot be defaulted (m4) and an unrecorded one no longer refuses a
  journal rebuild (m9); a blocked run exits 3 (m5); replace-when-INVALID never touches an `ACTIVE`
  profile (m6), never creates after a failed delete (m7), and never acts on a mismatched instance (m8);
  the `=> AppleBundleIdName` conversion is byte-for-byte (m11). `Class::Destructive` is right under
  `class.rs`'s "destroys or overwrites something".
- **Not fixed, for the coordinator:** an approved plan renders a replacement as `create`; nothing in it
  says a delete will run (a plan-visible replace marker would close it). `BlockedGate.awaiting_inputs`
  has no serde default, so a `RunFinished { Blocked }` journal line written between G2 and G3 no longer
  reads back (local, unpushed commits only). An acknowledgement does not persist between runs, by
  design: every re-run must supply every `*_done=done` again or block (exit 3).
- **Red on arrival, fixed:** B1/B2's two `willikins-server` workflow-list tests (`b9287db`), and
  `willikins-cli`'s MCP `validate` sweep, rate-limited on its 61st document since B2's fixture
  (`369706f`). A test gap: `plan_gates.rs` never pinned a `Step` binding on a blocked scalar gate
  (`a512cb5`).
- Verify items: none of 5–13 could move without a provider call; item 14 is new (below).

**Addendum:** 2026-09-29 (diagnosis) — **the replace cycle's STOP at "capability enable" was
willikins' own capability list, not Apple refusing `HEALTHKIT` on an `IOS` identifier.**
`GET /v1/bundleIds/{id}/bundleIdCapabilities?limit=200` answers **`400`, `errors[].code`
`PARAMETER_ERROR.ILLEGAL`, `errors[].title` "A given parameter is not allowed for this request"**; the
same `GET` with no query answers `200`. Every `appstore.bundle_id_capability.ensure` `read` and `ensure`
starts with that list, and `AppstoreClient::list_bundle_id_capabilities` has sent `?limit=200` since
`e4ba9af` (the pagination addendum above), which landed **after** the last live capability cycle, so
no live run had sent it before. The App Store Connect OpenAPI description 4.5 (fetched 2026-09-29)
lists `limit` (maximum 200) on this path; Apple refuses it. Observed on `IOS` identifiers; the code
names the parameter, not the platform, and `UNIVERSAL` was not re-probed.
- **Two live probes**, `appstore_live_ios_capability_probe` (`fdcf332`), each on one fresh `IOS`
  throwaway `com.willikins.probe.delete-me.<pid>-<unix-seconds>`, deleted by its create id in the same
  guarded run. Probe 1 (no profile): the list `400`, the tool's `read` `Provider: provider returned status
  400`, a raw `POST /v1/bundleIdCapabilities` for `HEALTHKIT` **accepted, `201`**. Probe 2 (one throwaway
  `IOS_APP_STORE` profile first, `ACTIVE`): same `400` with `limit`; with no query the list read
  `IN_APP_PURCHASE (settings null)` before and `HEALTHKIT (settings null), IN_APP_PURCHASE (settings
  null)` after; the `POST` **accepted, `201`**; the profile then read **`profileState INVALID`**.
  Certificates received `GET` only.
- **Counts** 5 certificates / 13 profiles / 21 bundle ids before and after each probe; independent `404`s
  on every deleted id; an independent recount afterwards (`appstore_counts_and_leftovers_probe`): 5
  certificates (4 `DEVELOPER_ID_APPLICATION_G2`, 1 `DISTRIBUTION`), 13 profiles (11 `IOS_APP_STORE`
  `ACTIVE`, 2 `INVALID`), 21 bundle ids, 0 throwaway leftovers.
- **What it means.** Walter's capability nodes are sound on `IOS` identifiers as designed: the enable
  works with or without a profile. An `IOS` identifier arrives with `IN_APP_PURCHASE` already enabled
  (`settings null`), which the read rule already tolerates. Apple's documented invalidation ("Provisioning
  profiles that contain a modified App ID become invalid") is **live-confirmed** on a throwaway, so the
  replace cycle's way of producing an `INVALID` profile is right and needs no alternative. What blocks
  replace-when-INVALID's live proof, the live capability cycle, and every capability node in `plan --live`
  or `apply` on any platform is the client's `limit`. **Replace-when-INVALID stays mock-proven until
  then.**
- **Prescribed fix (not made here; a diagnosis task, and both live probes are spent):** the first request
  of `list_bundle_id_capabilities` carries no query; keep following `links.next` (its query string only,
  re-attached to the fixed path, as now). The capability mocks match `match_query(Any)`, so they would
  not catch this either way: pin "the first request carries no `limit`" in a mock. Then re-run
  `live_capability_cycle` and `appstore_live_profile_replace_cycle` once each. Apple's default page size
  for this path stays unverified (two rows, `links.next` absent, say nothing about it).
- **Harness, test-first** (`8c17ee5`, `b672c3d`): `tests/support/apple_error_report.rs`, whose tests run
  in `tests/redaction.rs` in every gate, summarises Apple's error body as status plus every
  `errors[].code` and `errors[].title`, never `errors[].detail`; `tool_ok` now repeats a `ToolError`
  message only when willikins wrote it (`provider returned status N`, the fixed `401`/`403` and
  parse-position texts), so this STOP now reads `Provider: provider returned status 400`; the raw profile
  `POST` reports codes and titles. `b72e3f5` corrects the replace cycle's comment (the identifier is
  `IOS`, not `UNIVERSAL`). Verify item 15 below is new and settled.

## Goal

One workflow document, `workflows/walter-ios-app.yaml`, provisions everything a provider API can
provision for **Walter**, the operator's iOS health app, and names everything it cannot as an explicit
manual step that `plan` reports before anything runs. The operator's answers, 2026-09-27:

- Targets: the app, a **notification service extension** and **widgets**; no watchOS app for now.
- It lives in the existing **Bande-a-Bonnot monorepo** (`Bande-a-Bonnot/monorepo`), not a new
  repository. `apps/walter/` already exists there, empty.
- Bundle identifiers `com.bande-a-bonnot.walter`, `com.bande-a-bonnot.walter.nse`,
  `com.bande-a-bonnot.walter.widgets`, following the account's suffix habit (`.nse`, `.keyboard`,
  `.stickers`).
- Doppler project `walter` with `dev`/`stg`/`prd` configs inheriting the operator's shared base configs.
- Manual, and reported as such: the App Store Connect app record (no create API), app groups (no API;
  created by Xcode or the portal), and adding Walter's files to the monorepo (willikins cannot write
  files yet).

The milestone is done when tasks 1 to 3 are green, the attacker has run task 1's live capability cycle
once on the live Apple account under every trust boundary below, and the **dry run** (below) has
applied the Walter document against the sandbox GitHub, Doppler and Buildkite accounts and throwaway
Apple identifiers, re-applied it `Unchanged`, and torn everything down with counts equal before and
after. The real apply against the operator's own accounts is **not** part of this milestone: it is the
operator's, with the credentials listed under "Credentials — for a later real apply".

## Out of scope

- ~~**Provisioning profiles in the Walter document.**~~ **Superseded 2026-09-28** (operator decision 1
  and decision (j)): the profiles are in the document, behind the app-group gate, and a re-run replaces
  any profile Apple invalidated.
- **Writing files** (entitlements, `Info.plist`, `BUILD.bazel`, `.buildkite/`): a manual step until
  file-writing lands.
- **App groups, the app record, an APNs key, a Doppler grant to the CI service account**: manual steps.
- **`APPLE_ID_AUTH` on anything live** (trust boundary 4). The grammar admits its setting; only mocks
  exercise it.
- **SigNoz.** The operator's key expired on 2026-09-23. Nothing in this milestone calls SigNoz, and
  Walter's document has no telemetry node. A later document may add one when a key exists.
- **Changing `buildkite.pipeline.ensure`'s frozen bootstrap.** Decision (f).
- **The core `Action::Update` gap**, per-target credential routing, secrecy inference. Unchanged.
- **Railway.** No Railway command.

## Trust boundaries (normative)

Every lane holds these as absolute; they extend milestone 3c's eight, which still hold.

1. **Live Apple writes only on throwaway identifiers** named `com.willikins.probe.delete-me.` followed
   by a per-run unique suffix (and, for the dry run, that identifier plus `.nse` and `.widgets`),
   created and deleted inside the same guarded test. Never create, modify or revoke a certificate.
   Never touch an existing identifier, app, profile or device.
2. **The Walter document carries no bundle-identifier default.** `app_identifier`, `nse_identifier`
   and `widgets_identifier` are required inputs, so a forgotten `--input` fails `plan` instead of
   registering `com.bande-a-bonnot.walter` on the live account. The operator's real values appear in the
   document's header comment and in this plan, never as a default. The dry-run harness additionally
   refuses to start unless all three begin with the throwaway prefix.
3. **Delete only by the id the same run's create returned.** For the dry run that id comes from the
   document's own outputs (`app_id`, `nse_id`, `widgets_id`, each `AppleBundleIdId` straight from
   `appstore.bundle_id.ensure`), never by name or filter.
4. **Never enable `APPLE_ID_AUTH` on a live identifier, throwaway included.** Research note section 2,
   verbatim: "App IDs can't be deleted if they are grouped with other apps for features like Sign in
   with Apple." An identifier the test cannot delete breaks boundary 1.
5. **Counts before and after.** Bundle identifiers, profiles and API-visible certificates are counted
   read-only before any Apple write and must be equal afterwards (5/13/21 at milestone 3c; the live
   run records its own).
6. **Surprise means stop.** An unexpected status, a filter matching more than asked, a count that moved,
   a capability row whose settings shape differs from decision (d)'s assumption: delete only what this
   run made, record it, and report rather than improvise.
7. **No value reaches a file, log, commit, command line or report.** Credentials are resolved from
   `~/.config/willikins/sandbox.env` and sandbox Doppler in the same command that uses them, tokens on
   `curl`'s standard input, never argv. The operator's certificates, profiles and identifiers are
   reported by count and type only. The Apple team id visible in the monorepo's `BUILD.bazel` is not
   reproduced anywhere.
8. **The operator's own `gh` credential is never used.** GitHub is reached as
   `WILLIKINS_GITHUB_TOKEN` (the sandbox PAT) only.
9. **Nothing under `/Users/thomas/Projects/bande-a-bonnot` is written.** It is read for its pattern.

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| Walter document | `workflows/walter-ios-app.yaml`, `name: walter-ios-app` |
| Walter's real identifiers (header comment and later real apply only; never a default) | `com.bande-a-bonnot.walter`, `com.bande-a-bonnot.walter.nse`, `com.bande-a-bonnot.walter.widgets` |
| Walter's app group (manual step M2) | `group.com.bande-a-bonnot.walter` — the monorepo's `group.<app bundle id>` convention |
| Capabilities the document enables, host identifier only | `HEALTHKIT` (no setting), `PUSH_NOTIFICATIONS` (no setting), `DATA_PROTECTION` with `DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH` (decision (e)) |
| Capabilities on the extensions through the API | none (`APP_GROUPS` is manual on all three) |
| New type (task 1) | `AppleCapabilitySetting`, grammar exactly `DATA_PROTECTION_PERMISSION_LEVEL=COMPLETE_PROTECTION\|DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNLESS_OPEN\|DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH\|APPLE_ID_AUTH_APP_CONSENT=PRIMARY_APP_CONSENT`, non-secret, example `DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH` |
| New port (task 1) | `appstore.bundle_id_capability.ensure` gains `setting: AppleCapabilitySetting`, **optional** (`exact("AppleCapabilitySetting", false)`); key stays `(identifier, capability)` |
| Capability ↔ setting pairing (task 1) | `DATA_PROTECTION` ↔ `DATA_PROTECTION_PERMISSION_LEVEL` (required); `APPLE_ID_AUTH` ↔ `APPLE_ID_AUTH_APP_CONSENT` (required); every other capability: `setting` must be absent |
| Create body with a setting (task 1) | `attributes.settings = [{"key": KEY, "options": [{"key": OPTION, "enabled": true}]}]` — the shape to verify live (verify item 2) |
| New tool (task 2) | `github.repo.get`: input `repo: GitHubRepo` (required); output `repo: GitHubRepo`; no key; `Reversible`; **pure**; `GET /repos/{owner}/{repo}` through the existing `GitHubClient::get_repo`; 404 → `NotFound`; archived → `Conflict`; any 200 otherwise → `Present`, whoever owns it (no topic check) |
| `LIVE_TOOL_NAMES` | 25 → 26 (task 2), `crates/willikins-server/src/catalog.rs` and every site that pins the count (`crates/willikins-providers-doppler/tests/live_catalog.rs` among them) |
| New conversions (task 3) | `AppleBundleIdentifier => Text`, `AppleBundleIdentifier => AppleBundleIdName` — both public to public, total by grammar containment (`[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*`, at most 255 characters, fits `Text`'s 65,536 and `AppleBundleIdName`'s 1–255 no-control rule) |
| Throwaway prefix | `com.willikins.probe.delete-me.` + `<pid>-<unix-seconds>` (as milestone 3c) |
| Live capability cycle | new file `crates/willikins-providers-appstore/tests/live_capability_cycle.rs`, its own `[[test]]` entry with `required-features = ["live-tests"]`, `#[ignore]`, `WILLIKINS_LIVE_TESTS=1` |
| ASC credential in Doppler | sandbox workplace, `app-store-connect/prd`, `ASC_API_KEY_ISSUER_ID`, `ASC_API_KEY_ID`, `ASC_API_KEY_BASE64` (unchanged from 3c) |
| Sandbox GitHub org | `Willikins-Test` (0 repositories on 2026-09-27) |
| Sandbox monorepo stand-in | `Willikins-Test/monorepo`, created and deleted by the dry-run harness |
| Sandbox Doppler base config (dry run) | project `bande-a-bonnot-shared`, config `ios_base`, marked inheritable by the dry-run harness (none exists: the sandbox holds only `app-store-connect`) |
| Sandbox Buildkite org | `willikins-test` (token currently `401`) |
| Characterization snapshot | `crates/willikins-dsl/tests/snapshots/acceptance__characterization_of_every_document.snap` — may change **only** by new entries |

## Pre-flight checklist (doors and corners)

Filled by the planner, 2026-09-27, from Apple's own pages fetched verbatim that day (the `.md` twins of
developer.apple.com documentation pages, the account help pages, and the App Store Connect OpenAPI
specification, now **version 4.5**, 973 paths — the research note read 4.4.1). Read-only throughout: no
provider write of any kind.

### What an iOS health app with an NSE and widgets needs outside code

| # | Prerequisite | Where it lives | Automatable by willikins? | In this milestone |
| --- | --- | --- | --- | --- |
| 1 | App ID for the app, the NSE and the widgets | ASC API `POST /v1/bundleIds` | **Yes** (`appstore.bundle_id.ensure`) | document, 3 nodes |
| 2 | HealthKit on the app's App ID | ASC API, `HEALTHKIT`, no setting exists for it | **Yes** | document |
| 3 | HealthKit entitlement `com.apple.developer.healthkit` | entitlements file | No (a file) | manual M3 |
| 4 | `com.apple.developer.healthkit.access` (clinical records) — only if Walter reads health records | entitlements file | No | manual M3, omitted unless needed |
| 5 | `com.apple.developer.healthkit.background-delivery` (iOS 15+) — only if Walter uses `HKObserverQuery` background delivery | entitlements file | No | manual M3 |
| 6 | `NSHealthShareUsageDescription`, `NSHealthUpdateUsageDescription` | `Info.plist` | No | manual M3 |
| 7 | `healthkit` in `UIRequiredDeviceCapabilities` (Xcode adds it; remove if HealthKit is optional) | `Info.plist` | No | manual M3 |
| 8 | Push on the **host** App ID | ASC API, `PUSH_NOTIFICATIONS`, no setting needed for token auth | **Yes** | document |
| 9 | `aps-environment` entitlement on the host | entitlements file | No | manual M3 |
| 10 | An APNs auth key (`.p8`) for whatever sends Walter's pushes | portal only: spec 4.5 has no keys resource | No | manual M5 |
| 11 | NSE target (`com.apple.usernotifications.service` extension point) | `Info.plist`, `BUILD.bazel` | No | manual M3 |
| 12 | Widget extension target (`com.apple.widgetkit-extension`) | `Info.plist`, `BUILD.bazel` | No | manual M3 |
| 13 | App group registered (`group.com.bande-a-bonnot.walter`) | portal or Xcode; no API | No | manual M2 |
| 14 | `APP_GROUPS` enabled **and the group assigned** on all three App IDs | portal "Configure" or Xcode; the API can flip the flag but cannot assign | No — `appstore.bundle_id_capability.ensure` refuses `APP_GROUPS` on `Absent` by design | manual M2 |
| 15 | `com.apple.security.application-groups` on all three targets | entitlements files | No | manual M3 |
| 16 | Data protection level on the host App ID | ASC API, `DATA_PROTECTION` + `DATA_PROTECTION_PERMISSION_LEVEL` setting | **Yes, after task 1** | document |
| 17 | App Store distribution profiles, one per identifier, minted **after** 2, 8, 14 and 16 | ASC API | **Yes** (`appstore.profile.ensure`) | a later run of the signing document, manual M4 |
| 18 | App Store Connect app record | website only | No | manual M1 |
| 19 | Health privacy: disclose the health data collected; never store personal health information in iCloud | App Review 5.1.3; app content | No | recorded in M3's checklist |
| 20 | Doppler project and configs, inheriting shared base configs | Doppler API | **Yes** | document |
| 21 | Buildkite CI service account can read project `walter` | Doppler; no `doppler.project_member.ensure` yet | No | manual M6 |
| 22 | Buildkite pipeline for Walter pointing at the monorepo | Buildkite API | **Yes** (with the frozen bootstrap) | document |
| 23 | Pipeline's stored bootstrap selects `apps/walter/.buildkite/…` on the monorepo queue | Buildkite settings | No — decision (f) | manual M7 |
| 24 | The monorepo exists and the credential can see it | GitHub API | **Yes, after task 2** (`github.repo.get`) | document |

### Sources, verbatim

- **HealthKit, enable** — <https://developer.apple.com/documentation/healthkit/setting-up-healthkit.md>:
  "Before you can use HealthKit, you must enable the HealthKit capabilities for your app. In Xcode,
  select the project and add the HealthKit capability. Only select the Clinical Health Records checkbox
  if your app needs to access the user's clinical records. App Review may reject apps that enable the
  Clinical Health Records capability if the app doesn't actually use the health record data." And:
  "When you enable the HealthKit capabilities on an iOS app, Xcode adds HealthKit to the list of
  required device capabilities … If HealthKit isn't required for the correct operation of your app,
  delete the `healthkit` entry from the "Required device capabilities" array."
- **HealthKit, App ID** — <https://developer.apple.com/documentation/xcode/configuring-healthkit-access.md>:
  "After you add the HealthKit capability, Xcode links the HealthKit framework to your target and
  updates the target's entitlements file to include the com.apple.developer.healthkit. If Xcode
  automatically manages the signing of your app, it also enables HealthKit for your app's App ID."
  The monorepo signs manually (`local_provisioning_profile` by name), so the App ID side must be done
  by the API or the portal — which is what the document does. Purpose strings: "The presence of these
  purpose strings is an App Store requirement for any app that integrates with HealthKit."
- **Usage strings** — `NSHealthShareUsageDescription.md`: "This key is required if your app uses APIs
  that access the someone's health data." `NSHealthUpdateUsageDescription.md`: "This key is required
  if your app uses APIs that update the user's health data."
- **Clinical records entitlement** — `com.apple.developer.healthkit.access.md`: "Only add values for
  data types that your app needs to access. App Review may reject apps that don't use the data
  appropriately."
- **Background delivery** — `HKHealthStore/enableBackgroundDelivery(for:frequency:withCompletion:).md`:
  "For iOS 15 and watchOS 8 and later, you must enable the HealthKit Background Delivery by adding the
  com.apple.developer.healthkit.background-delivery entitlement to your app. If your app doesn't have
  this entitlement, the … method fails with an errorAuthorizationDenied error."
- **HealthKit store while locked** — `healthkit/protecting-user-privacy.md`: "the device encrypts the
  HealthKit store when the user locks the device. As a result, your app may not be able to read data
  from the store when it runs in the background."
- **Push** — `usernotifications/registering-your-app-with-apns.md`: "enable the Push Notifications
  capability in your Xcode project … Enabling this option in iOS adds the aps-environment … In your
  developer account, enable the push notification service for the App ID assigned to your project."
  The NSE — `modifying-content-in-newly-delivered-notifications.md`: "Modifying a remote notification
  requires a notification service app extension, which you include inside your iOS app bundle … A
  notification service app extension ships as a separate bundle inside your iOS app", and "Notification
  service app extensions only operate on remote notifications configured in the system to display an
  alert to the user." Registration and the device token are the app's, so push is enabled on the host
  App ID; nothing Apple says asks for it on the extension's (verify item 5).
- **APNs key** — <https://developer.apple.com/help/account/capabilities/communicate-with-apns-using-authentication-tokens/>:
  "You can use one APNs signing key to authenticate tokens for multiple apps. The signing key works for
  both the development and production environments. The signing key doesn't expire, but can be
  revoked." <https://developer.apple.com/help/account/keys/create-a-private-key/>: "Required role:
  Account Holder or Admin … WARNING: Save this file in a secure place because the key is not saved in
  your developer account and you won't be able to download it again." Spec 4.5: no path matches
  `key|apns|push` other than `alternativeDistributionKeys` and `searchKeywords`.
- **App groups** — `com.apple.security.application-groups.md`: "Format the identifier as follows:
  `group.<group name>`. Apple ensures that the group name you choose is unique when you register the
  app group on the Apple Developer website." `xcode/configuring-app-groups.md`: "You need to register
  app groups for iOS, iPadOS, tvOS, visionOS, and watchOS apps." The help page
  <https://developer.apple.com/help/account/identifiers/register-an-app-group/>: "Required role:
  Account Holder or Admin … Alternatively, you can create app groups when you enable app groups in
  Xcode." Spec 4.5: zero `appGroup` paths or schemas, as the research note found for 4.4.1.
- **Capability changes invalidate profiles** —
  <https://developer.apple.com/help/account/identifiers/enable-app-capabilities/>: "Provisioning
  profiles that contain a modified App ID become invalid. You'll need to regenerate the provisioning
  profiles that use that App ID." And: "The following app capabilities require additional steps: Sign
  in with Apple, App groups, Apple Pay, Data protection, iCloud, and push notifications." Data
  protection's extra step: "enable the Data Protection capability. Under Sharing and Permissions,
  select an option." Push's extra step concerns only the TLS-certificate route: "If you communicate
  with the Apple Push Notification service (APNs) using a TLS certificate, push notifications aren't
  fully enabled … until you create a corresponding client TLS certificate." Token-based APNs needs no
  certificate, so `PUSH_NOTIFICATIONS` with no setting is complete.
- **Data protection classes** — `uikit/encrypting-your-app-s-files.md`: "Complete until first user
  authentication. (Default) The file is inaccessible until the first time the user unlocks the device
  … Complete. The file is accessible only when the device is unlocked."
- **Settings, from the specification 4.5** (`components.schemas`, verbatim JSON, abridged only by
  elision): `CapabilitySetting.key` enum `["ICLOUD_VERSION","DATA_PROTECTION_PERMISSION_LEVEL",
  "APPLE_ID_AUTH_APP_CONSENT"]`, with `options: array of CapabilityOption`; `CapabilityOption.key` enum
  `["XCODE_5","XCODE_6","COMPLETE_PROTECTION","PROTECTED_UNLESS_OPEN","PROTECTED_UNTIL_FIRST_USER_AUTH",
  "PRIMARY_APP_CONSENT"]` with `enabled: boolean`; `BundleIdCapabilityCreateRequest.data.attributes`:
  `{"capabilityType": …, "settings": {"type":"array","items":{"$ref":"#/components/schemas/CapabilitySetting"},"nullable":true}}`,
  `"required":["capabilityType"]`; `BundleIdCapabilityUpdateRequest` carries the same two attributes;
  `BundleIdCapability.attributes` carries `capabilityType` and `settings`. `POST
  /v1/bundleIdCapabilities` answers `201, 400, 401, 403, 409, 422, 429`; `PATCH …/{id}` answers `200`
  among the same; `DELETE …/{id}` answers `204`. `CapabilityType` is unchanged: 28 members.
- **App Review 5.1.3** — <https://developer.apple.com/app-store/review/guidelines/>: "You must disclose
  the specific health data that you are collecting from the device … Apps must not write false or
  inaccurate data into HealthKit … and may not store personal health information in iCloud."
- **Buildkite webhook** — `buildkite/docs` `pages/pipelines/source_control/github.md` (raw): after
  creating a pipeline, "Follow the onscreen instructions to set up a webhook: Add a new webhook in
  GitHub … The repository webhook is required so that the Buildkite GitHub app does not need read access
  to your repository." So an API-created pipeline builds only when a webhook or an explicit build
  exists (verify item 8).

### Sandbox inventory (read-only, names and counts only, 2026-09-27)

- [x] **GitHub sandbox PAT authenticates**; org `Willikins-Test` has **0** repositories. The monorepo
  stand-in does not exist yet.
- [x] **Doppler sandbox token authenticates**; the workplace has **1** project, `app-store-connect`,
  with its three root configs, none inheritable, none inheriting. **No shared base config exists**, so
  the dry run creates one.
- [ ] **Buildkite sandbox token: `401`** on `GET /v2/access-token` (a call that needs no scope) and on
  the clusters and pipelines lists. **Blocks the dry run only.** See "Credentials".
- [x] **App Store Connect specification** re-read at 4.5; nothing this plan relies on changed.
- Not re-probed: the live Apple account (its counts are the live cycle's first step).

## The monorepo, surveyed read-only

`/Users/thomas/Projects/bande-a-bonnot` (remote `Bande-a-Bonnot/monorepo`), 2026-09-27.

- **Build system.** Bazel with `rules_apple` and `rules_swift`; one `apps/<app>/ios/BUILD.bazel` per
  app. Danksworth (the iOS app with extensions) uses `ios_application` plus `ios_extension` and
  `ios_imessage_extension`, `minimum_os_version = "26.0"`, `families = ["iphone", "ipad"]`, versions from
  `apple_bundle_version`, `infoplists` per target, and entitlements chosen by `select()` — `None` for CI
  and simulator builds, the file otherwise.
- **Signing.** Manual, by profile name: five `local_provisioning_profile` rules in Danksworth — one
  Xcode-managed wildcard team profile for local development, and **four distribution profiles each named
  exactly after its bundle identifier** ("Profile names must match EXACTLY in 3 places: 1. Apple
  Developer Portal profile name 2. This BUILD.bazel profile_name 3. fastlane/Fastfile constants"). That
  is precisely what `appstore-signing-profile-from-doppler.yaml` produces (milestone 3d binds the profile
  name to the identifier).
- **Entitlements.** One file per target under `ios/Resources/<Target>.entitlements`. App groups follow
  `group.<app bundle id>` (Danksworth's host, keyboard and stickers share one; its share extension uses a
  second, suffixed group). The host also carries
  `keychain-access-groups = $(AppIdentifierPrefix)<bundle id>`. Pocket Companion (a single-target app)
  carries its capability entitlements the same way.
- **Bundle identifiers.** `com.bande-a-bonnot.<app>` and `com.bande-a-bonnot.<app>.<suffix>` —
  Walter's three follow it.
- **Buildkite.** One pipeline per app in the operator's Buildkite organisation, slug equal to the app
  name. Each app keeps `apps/<app>/.buildkite/` with `pipeline.yml`, an `upload-pipeline.sh` selector,
  `bootstrap.yml` (the **stored** bootstrap, checked in because "Buildkite runs it before the repository
  exists and nothing else records it"), `provider-settings.json` (every provider trigger disabled during
  rollout), and a README. The stored bootstrap sets `GIT_CONFIG_*` job environment to select a host
  credential helper, targets the `ci-macos-apple-silicon` queue, and runs
  `bash apps/<app>/.buildkite/upload-pipeline.sh`, which calls
  `buildkite-agent pipeline upload --no-interpolation <file>`. Guests run under a pinned `tart-ci` plugin.
- **Doppler.** The existing apps use one Doppler project per app (Pocket Companion: project
  `pocket-companion`, CI config `prd_deployment`), handed to guests by the plugin's
  `doppler_token_secret`. No app in the monorepo yet uses config inheritance, so **the real names of the
  operator's shared base configs in that workplace are not observable from here** (verify item 9).
  Team-key names seen there: `APP_STORE_CONNECT_API_KEY_BASE64`, `APP_STORE_CONNECT_API_KEY_ID`,
  `APP_STORE_CONNECT_API_KEY_ISSUER_ID`, `APPLE_DISTRIBUTION_CERTIFICATE_P12_BASE64`,
  `APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD` (names only).
- **`apps/walter/`** exists and is empty.

**Where Walter must follow the pattern** (all of it is manual step M3's content):
`apps/walter/ios/BUILD.bazel` with `ios_application` `Walter` embedding two `ios_extension`s
(`WalterNotificationService`, `WalterWidgets`) — Danksworth's `extensions = []` is a deliberate
exception, Walter's host embeds both; three `local_provisioning_profile`s named exactly after the three
identifiers plus the local wildcard one; `Resources/Walter.entitlements`,
`Resources/WalterNotificationService.entitlements`, `Resources/WalterWidgets.entitlements` selected
`None` for CI and simulator builds; `group.com.bande-a-bonnot.walter` in all three;
`apps/walter/.buildkite/{pipeline.yml, upload-pipeline.sh, bootstrap.yml, provider-settings.json,
README.md}` copied from Danksworth's shape with triggers disabled.

## Decisions

### (a) A named manual step is a workflow output bound to a `template.render` node

**Superseded 2026-09-28 by decision (j).** Kept for its history: its own "honest trade" below is
exactly what (j) fixes — a gate orders what it guards, and a re-run is the acknowledgement.

Existing mechanisms suffice, so no engine feature is proposed. `template.render` is pure, so `plan`
evaluates it and the plan's `outputs` show every manual step **before anything runs**; `apply` shows
them again. Each step is one node, `tool: template.render`, whose `template` is a literal
`TemplateSource` naming the step, every one beginning `MANUAL Mn ({{ value }}): …` so each names the
app it belongs to, and whose `value` is the app identifier,
bound from `${{ inputs.app_identifier }}` through the new `AppleBundleIdentifier => Text` conversion, so
the text names the identifier actually being provisioned (the dry run's throwaway, the real run's
`com.bande-a-bonnot.walter`). The output names are `manual_m1_app_record` … `manual_m7_buildkite_bootstrap`
(`^[a-z][a-z0-9_]*$`).

Why the conversion rather than a literal `value`: a literal would name the real identifier during the
dry run, which is misleading on exactly the run whose job is to show the text. Why bind from the input
and not `steps.app_id.identifier`: the input is known at plan time on every path.

**The honest trade.** An output is a report, not a checkpoint: nothing orders a manual step, nothing
records that the operator did it, and a node exists only to carry text. A first-class `manual:` section
in the document format (rendered, journaled, perhaps acknowledged) would be better and is recorded as a
candidate for a later milestone, not designed here. What a manual step leaves behind is still caught
where it matters: an `APP_GROUPS` or profile document run before M2 either converges or refuses loudly.

### (b) The Walter document mints no provisioning profile

**Superseded 2026-09-28** by operator decision 1 (replace-when-INVALID, the T3a addendum) and decision
(j): the second fact below is answered by a gate that passes the identifier through, which is a data
edge. Kept for its history.

Two facts force it. Apple: "Provisioning profiles that contain a modified App ID become invalid." App
groups can only be assigned by hand (M2), after the document has run, so any profile the document
minted would be invalid by the time Walter builds. And milestone 3c's coordinator addendum, item 4: the
graph orders nodes only by data edges, and a profile node consumes nothing a capability node produces,
so "capabilities, then profile" is not expressible. So profiles are manual step **M4**: after M2, run
`workflows/appstore-signing-profile-from-doppler.yaml` once per identifier, which names each profile
after its identifier — the monorepo's own convention. That document needs no change.

### (c) The host identifier alone carries API-enabled capabilities

`HEALTHKIT`, `PUSH_NOTIFICATIONS` and `DATA_PROTECTION` go on `app_identifier`. The NSE rides on its
host's push registration (Apple's NSE and APNs pages above); the widgets read from the app group
container rather than from HealthKit, because "the device encrypts the HealthKit store when the user
locks the device" and widget timelines refresh while locked. If Walter's widgets later read HealthKit
directly, one `HEALTHKIT` node on `widgets_identifier` is added — policy lives in the document.
Clinical records and background delivery are entitlement-file choices (M3), not App ID state.

Each capability node binds `identifier` from `${{ steps.app_id.identifier }}`, never from the input:
that data edge is what orders it after the registration. The tool's own `NotFound` ("run
appstore.bundle_id.ensure first") is the backstop, not the mechanism.

### (d) Capability settings: one optional `setting` port, a closed pair type, refused at `read` when it does not fit

`appstore.bundle_id_capability.ensure` gains `setting: AppleCapabilitySetting`, optional. No production tool has shipped an optional port before, so
the engine was read for it (2026-09-27): `check` raises `UnboundInput` only when `port_spec.required`
(`crates/willikins-core/src/check.rs`, `check_with_port`), and `plan`'s `bind_ports` skips a port the
node did not bind, so the tool's `Inputs` simply lack the key (`crates/willikins-core/src/plan.rs`,
"`node` did not bind is simply absent from the result"). `helpers::get` fails on a missing port, so the
tool and its fake twin read `setting` with `inputs.get(&port("setting"))` and `helpers::known` when
present (or a small `get_optional` helper beside `get`, test first). The type's
grammar is exactly the four `KEY=OPTION` pairs the specification can express for a capability this tool
does not refuse (SHARED VALUES). `ICLOUD_VERSION` is excluded because `ICLOUD` is already refused.

**The cross-port rule, and why `check` cannot hold it.** `DATA_PROTECTION` requires a
`DATA_PROTECTION_PERMISSION_LEVEL` setting, `APPLE_ID_AUTH` requires `APPLE_ID_AUTH_APP_CONSENT`, and
every other capability takes none. That relates two ports of one tool; `check` types each port
independently and has no per-tool cross-port hook, and adding one is an engine change this milestone
does not make. So `read` enforces it first, before any request (it runs during `plan`, so a mismatched
pair fails the plan before any node is ensured): `ToolErrorKind::Invalid` naming the capability and the
setting key it needs or forbids. A negative fixture pins the plan-time error. Requiring the setting for
`DATA_PROTECTION` and `APPLE_ID_AUTH` changes the tool's documented behaviour (its module doc says both
are complete without one); no shipped document uses the tool, so no characterization entry moves.

**Create.** With a setting, the body's `attributes.settings` is `[{"key": KEY, "options": [{"key":
OPTION, "enabled": true}]}]`; without one, the key is absent exactly as today (a JSON matcher pins both).

**Read.** The capability row is found by `capabilityType` as today. With a setting requested, the row's
`settings[]` entry for that key is inspected: exactly one option with `enabled: true` equal to the
requested one → `Present`; any other state (a different enabled option, none enabled, the key or
`settings` absent) → `Observation::Mismatch { port: setting }`. That is terminal at `plan` (the
`Action::Update` gap is unchanged even though `PATCH /v1/bundleIdCapabilities/{id}` exists), and the
module doc says to change it by hand or pass the current value. The parse keeps only `key`, option
`key` and `enabled`; `name` and `description` are never read. **Both the create shape and "the selected
option is the one marked `enabled`" are unobserved** (verify items 1 and 2): the live cycle settles
them, and if Apple differs the attacker records an addendum and adapts the parse, test first.

### (e) Walter's data protection level is `PROTECTED_UNTIL_FIRST_USER_AUTH`

Chosen from the NSE and background delivery, not from a wish for the strongest class. `COMPLETE`
makes a file "accessible only when the device is unlocked"; the NSE runs when a notification arrives,
typically while locked, and reads what the app left in the group container; HealthKit background
delivery wakes the app while locked. `PROTECTED_UNTIL_FIRST_USER_AUTH` is iOS's default class, so
enabling it changes no behaviour — its value is that the class becomes an explicit, reviewed line in the
document and the profile, and a later tightening is a one-line, visible edit. The operator may prefer
`PROTECTED_UNLESS_OPEN` or `COMPLETE_PROTECTION` with file-level exceptions; that is their call and
costs one input change (risk 3). The same level goes into `Walter.entitlements` as
`com.apple.developer.default-data-protection` (M3).

### (f) Buildkite: the frozen bootstrap stays; reseeding it is manual step M7

The monorepo's stored bootstraps carry three things willikins' frozen `UPLOAD_CONFIGURATION` does not:
a per-app selector script, `GIT_CONFIG_*` job environment for the host checkout, and the Mac queue. A
path port would fix one of the three, would widen milestone 3a's trust boundary 8 ("no other field")
and brush the no-shell-command invariant. The operator already reseeds stored steps by hand per their
own cookbook ("Check the stored step into the repository … Read back stored configuration and compare
it with the checked-in copy before dispatching"), and `buildkite.pipeline.ensure` never compares
`configuration`, so a hand-edited bootstrap survives every re-run as `Present`. So the document creates
the pipeline (org, slug `walter`, cluster, the monorepo) and **M7** replaces its stored bootstrap with
`apps/walter/.buildkite/bootstrap.yml`. Until then the pipeline would upload the root
`.buildkite/pipeline.yml`, which the monorepo does not have — harmless, because no webhook exists until
someone adds one (the Buildkite doc above), and M3 lands Walter's `.buildkite/` files **before** the
real apply.

### (g) The monorepo is referenced through a read-only `github.repo.get`, never created

`buildkite.pipeline.ensure`'s `repo` is an ordinary `exact("GitHubRepo", true)` port, so a bare input
would type-check. A pure, read-only `github.repo.get` (modelled on `buildkite.cluster.get`) makes the
reference *checked*: `plan` fails with `NotFound` if the monorepo does not exist or the credential
cannot see it, before any write, and the dry run exercises GitHub through the document rather than only
through the harness. It ignores ownership (the monorepo carries no willikins topic, and must not need
one): `github.repo.ensure` would read it `Foreign` and refuse, which is correct for `ensure` and wrong
for a reference. An archived repository is a `Conflict`: a pipeline on it could never build.

### (h) Names: `naming.v1` for Doppler and Buildkite, the monorepo as an input

`naming.v1` with `org` = the monorepo's organisation and `slug` = `walter` yields Doppler project
`walter` and pipeline slug `walter` (the monorepo's slug-equals-app-name pattern). Its `github_repo`
output is deliberately unused: in a monorepo the repository is referenced, not derived. `monorepo:
GitHubRepo` is an input defaulting to `Bande-a-Bonnot/monorepo` (a default is safe here: a wrong repo is
caught by `github.repo.get`, and the dry run overrides it to the stand-in). Bundle-identifier *names*
are bound from the identifiers through the new `AppleBundleIdentifier => AppleBundleIdName` conversion,
the operator's "bundle id everywhere" habit stated in the document, not in any tool — exactly as 3d did
for profile names.

### (i) Doppler: `walter`, `dev`/`stg`/`prd`, inheriting the shared base configs

As `workflows/doppler-ios.yaml`, inlined: `doppler.project.ensure`, `doppler.config.ensure` per
environment, `doppler.config.inherits.ensure` per config with `base_configs` a defaulted list input
(the document format cannot bind a list literal to a port; that header explains why). Default
`[bande-a-bonnot-shared/ios_base]` — a **placeholder shaped like the Lightless Labs one**; the real
names are unknown (verify item 9) and M0 asks the operator to confirm or edit it before a real apply.
The ASC credential chain reads `config` (`app-store-connect/prd`) through the same token, so in a real
apply that config must be in the same workplace as `walter` (one `WILLIKINS_DOPPLER_TOKEN` is one
workplace).

### (j) A manual step is a gate: blocked, not failed; re-running the document is the resume

**Decided 2026-09-28** (operator decision 6; replaces (a)). Designed as the smallest change to the
existing model. Every behaviour of a document that uses no gate stays byte-identical: its plan JSON,
its fingerprint, its journal lines, its CLI text and its exit codes.

**Today, from the code** (`main` at `63144a8`):

- `plan` stops at the first problem: `crates/willikins-core/src/plan.rs` module doc lines 36–41, and
  the walk at 501–567 returns the first `PlanError`. A node whose key port is `Unknown` is
  `PlanError::KeyUnknown` before its `read` (`plan_one`, 807–813).
- `apply` **stops at the first failure, and independent branches do not run.** A failed `ensure`
  pushes `NodeStatus::Failed`, appends `not_run_tail` over *every* later instance in plan order,
  dependent or not, and returns `ApplyError::Tool` (`crates/willikins-core/src/apply.rs` 629–657); the
  two unknown-input refusals do the same (682–706). The walk is by plan position, not by dependency.
- The journal records `Outcome::Failed` for any `Err` (`crates/willikins-journal/src/observer.rs`
  366–369), and `finish_runs` synthesizes `NotRun` for every planned instance that has no
  `NodeFinished` (`crates/willikins-journal/src/journal.rs` 469–495). `RunState` is
  `running|succeeded|failed`.
- The CLI's `apply` exits 0 on `Succeeded`, 1 on `Failed` or `Running`, 2 on usage or configuration
  (`crates/willikins-cli/src/commands.rs` 531–535). An input with no default and no value is
  `describe`'s `missing` (`crates/willikins-core/src/describe.rs` 249), and `plan` then exits 1 before
  planning anything (`crates/willikins-cli/src/main.rs` 341–345).
- MCP's `plan` returns `PlanResponse` (`crates/willikins-server/src/types.rs` 21) with the `Plan`
  inside; `run_status` returns the journal's `RunRecord` (`crates/willikins-server/src/mcp.rs` 638–651).
- Nodes are ordered **only by data edges**: `Node` has `tool`, `for_each` and `with`, nothing else
  (`crates/willikins-core/src/workflow.rs` 195–203); there is no `when` and no `after`.

Failure semantics do **not** change: a tool failure still stops the walk with a `NotRun` tail, and a
failure dominates a block. Only a gate's `Absent` gets the new treatment.

**1. What a gate is.** An ordinary tool that is `pure: true` (read-only, no key, `Reversible`, never
`ensure`d — the codebase already calls provider-reading tools such as `github.repo.get` and
`appstore.certificate.get` pure), and that answers one new trait method with a default:

```rust
// crates/willikins-core/src/tool.rs
pub struct Gate {
    pub need: &'static str,              // what must be true, e.g. "APP_GROUPS enabled on this bundle identifier"
    pub how: &'static str,               // how the operator makes it true
    pub subject: &'static [&'static str] // input ports the report names, rendered by the engine
}
pub trait Tool { /* unchanged */ fn gate(&self) -> Option<&Gate> { None } }
```

A default method, not a `ToolSpec` field or an `Observation` variant: a `ToolSpec` field would edit
about fifty struct literals and every catalog entry's JSON, and a new `Observation` variant would add
an arm to about fifteen exhaustive provider matches. Neither is needed. `Catalog::insert`
(`crates/willikins-core/src/catalog.rs` 53) refuses a gate whose spec is not pure, and a `subject`
entry that is not one of the tool's input ports with an `Exact`, non-secret type.

A gate's `read` has the usual three answers. **`Present(outputs)`**: the condition holds; the node
plans `Compute`, exactly as any pure tool. **`Absent { .. }`**: the thing the operator must make does
not exist yet (the app record, the capability row, the acknowledgement); the node plans the new
**`Action::Blocked`**. **`Foreign`, `Mismatch`, or an error**: unchanged, a hard `PlanError` — a gate
that sees something *wrong*, rather than something *missing*, stops the plan as today. A gate
observes reality wherever an API exposes it, reusing the provider's existing client read (Walter's:
`GET /v1/apps` filtered by bundle id, research note section 4; the capability list
`list_bundle_id_capabilities` already paginates), and otherwise takes an operator acknowledgement
(point 6).

**2. Ordering: a gate guards only what consumes its output.** Because edges are data edges, a gate
orders a node only by emitting a typed output that node binds. The rule for gate authors: **a gate
passes through the key it checked.** Walter's app-group gate takes `identifier` and outputs it again,
and each profile node binds `identifier: ${{ steps.<gate>.identifier }}`, never
`${{ steps.app_id.identifier }}` — that edge is what puts the profiles after the gate. A gate nothing
consumes is a **leaf**: it blocks the run and is reported, but holds nothing back. An explicit
`needs:` edge would be a document-format change and is **not** in this milestone.

**3. Plan.** `plan` walks `Checked::order` as today, carrying one set of blocked-or-skipped nodes (the
topological order makes one pass the transitive closure):

- A node any of whose bindings — a `with` port or its `for_each` source, `Step` or `Keyed` — names a
  node in the set plans **`Action::Skip`** and is **never read**, so `KeyUnknown` cannot fire. The
  decision is made from the bindings *before* `bind_ports`, and a skipped node is not bound at all
  (its `inputs` are empty), so no edge delivers an `Unknown` through a conversion. Its outputs are all
  `Unknown`, filled by `fill_outputs` from nothing. A `Keyed` binding to one instance
  of a `for_each` gate is skipped only if *that* instance is blocked; a `Step` binding aggregating a
  `for_each` gate is skipped if any instance is.
- A `for_each` node whose *source* is skipped cannot be expanded: it plans as one `PlannedNode` with
  `instance: None` and `Action::Skip`.
- `NodeResult` gains a `Skipped` arm, so `resolve_step` (plan.rs 711) and `resolve_keyed` (768) yield
  `Value::unknown` of the port's type (a list type for a `for_each` node). Without it,
  `aggregate_for_each_port` (728) over zero instances would return a **known empty list** — wrong.
- Every other node, including every node that does not depend on a gate, is planned exactly as today.
- Workflow outputs bound to a skipped node resolve `Unknown`.
- `Plan` gains `blocked: Vec<BlockedGate>` with `#[serde(skip_serializing_if = "Vec::is_empty")]`, so
  a plan without a blocked gate serializes byte-identically (the characterization snapshot prints
  `plan_json` and `fingerprint_json` verbatim, `crates/willikins-dsl/tests/acceptance.rs` 220–222).

```rust
// Plain data, no `Value`: Serialize + Deserialize + JsonSchema, stored in the journal untouched,
// like NodeStatus and InstanceFingerprint.
pub struct BlockedGate {
    pub node: NodeName,
    pub instance: Option<String>,
    pub tool: ToolName,
    pub need: String,                    // the gate's static `need`
    pub how: String,                     // the gate's static `how`
    pub subject: Vec<(PortName, String)>,// each `subject` port rendered by Value::render, in declared order
    pub awaiting_inputs: Vec<InputName>, // G3: unsupplied acknowledgement inputs bound to this gate
    pub holds_back: Vec<NodeName>,       // every node skipped because of this gate, plan order, deduplicated
}
```

**4. Apply.** Rule 2's re-plan produces the same `Blocked`/`Skip` actions. The walk **classifies by
`planned.action` first**, before `resolve_instance_inputs` (apply.rs 571), before the pure branch
(574) and before the unknown-required-input match (599). The order is load-bearing: a skipped
*non-pure* node (Walter's `appstore.profile.ensure`, `identifier` bound from a blocked gate) would
otherwise reach 599 with an `Unknown` upstream port and stop the whole run as
`ApplyError::UnknownInput`, and a blocked gate, being pure, would otherwise be reported `Computed`.
The walk then treats a
`Blocked` instance as `NodeStatus::Blocked` and a `Skip` instance as `NodeStatus::Skipped`, emits
`NodeStarted` and `NodeFinished` for both (so the journal does not fold them into `NotRun`), calls no
tool, records `NodeResult::Skipped`, and **continues**. Every node not downstream of a blocked gate
runs. apply.rs's grouping (711–729) must build `NodeResult::Skipped` for a skipped group rather than
reach its `unreachable!` on a `for_each` node with no keyed instance. A tool failure still stops the
walk with the `NotRun` tail and returns `ApplyError::Tool`; its partial `Applied` shows any `Blocked`
and `Skipped` statuses reached so far. `Applied` gains `blocked: Vec<BlockedGate>` (skip if empty),
taken from the fresh plan; `Ok(Applied)` with a non-empty `blocked` is a **blocked run**, not an error.

**5. Drift and approval.** The new actions ride in `InstanceFingerprint::action` as `"blocked"` and
`"skip"`; the fingerprint's shape is unchanged. A gate satisfied between plan and apply is
`Action` drift, so `apply` refuses and the operator re-plans — correct, since the approved plan never
said the guarded nodes would run; the CLI's one-shot `plan`+`apply` makes that window small, and the
server's own pre-check (`crates/willikins-server/src/drift.rs`) compares actions by equality and needs
no change. The class stays **static** (`Checked::class`, apply rule 1): a document containing
`appstore.profile.ensure` (Destructive since `f980d78`) requires approval on **every** run, including
a first run whose profiles are all skipped. For Walter that means `--approve` (or an approver) each
run; the operator accepted "whatever approval that implies" in decision 1.

**6. Operator acknowledgement (G3).** Where no API shows the state, a gate reads an acknowledgement:
`operator.acknowledge` in `willikins-tools`, pure, a gate, inputs `step: Text` (required, the
gate's `subject`: the document states the manual step in its own words — policy in the workflow)
and `acknowledged: OperatorAcknowledgement` (required); output `step: Text`; `read` answers
`Present` when `acknowledged` is known and `Absent` when it is `Unknown`. `OperatorAcknowledgement` is
a new public type in `willikins-types`, grammar exactly `done`, example `done`, which no tool outputs.
It is **never defaulted and never a literal**, and an unsupplied one is **awaited, not missing**:

- `check` refuses a `default:` on an input of this type and a literal bound to a port of it (two new
  `CheckError`s, each with a negative fixture). The type is recognised through the registry entry's
  `TypeId`, never by its name (the milestone 3d rule).
- `describe` does not list an unsupplied input of this type in `missing`; it lists it in a new
  `awaiting: Vec<…>` (skip if empty), each with a prompt naming the input and the value `done`.
- `plan`'s `Binding::Input` arm (plan.rs 695) yields `Value::unknown` for such an input instead of
  `PlanError::MissingInput`. The input stays **absent** from the resolved map, so `PlanRecorded.inputs`
  and the butler's rebuild from the journal are untouched.
- A blocked acknowledgement gate reports, in `awaiting_inputs`, the workflow inputs bound to its ports
  whose value is `Unknown`, so the report can say which `--input name=done` satisfies it.

Honest limit: an acknowledgement gate has no typed pass-through, so it is a **leaf** (point 2). It
reports and blocks the run's outcome; it orders nothing. For Walter's acknowledgement steps that is
enough (decision (f) already argues the pipeline is harmless before its files land).

**7. The report.** One shape, `BlockedGate`, everywhere; every string that reaches text output goes
through `render::single_line`.

- **CLI `plan`**, text: node lines read `app_groups (tool): Blocked` and `nse_profile (tool): Skip`;
  after the node lines and before `outputs:`, when `blocked` is non-empty:

  ```
  blocked: 2 gates need the operator; everything that does not depend on them is planned
    nse_app_groups (<T3's gate tool>): APP_GROUPS enabled on this bundle identifier
      identifier: com.example.nse
      how: register group.<app identifier>, enable App Groups on the identifier and assign the group (portal Configure, or Xcode)
      holds back: nse_profile, nse_profile_to_doppler
    m7_bootstrap (operator.acknowledge): an operator acknowledgement that this manual step is done
      step: Replace the walter pipeline's stored bootstrap with apps/walter/.buildkite/bootstrap.yml
      how: do the step, then supply the awaited input
      supply: --input m7_bootstrap_done=done
  re-run this document once done
  ```

  (Illustrative: the gate tool, node and input names are T3's. `supply:` is rendered by the engine
  from `awaiting_inputs`, never by the tool.)

  `plan` still exits **0**: a plan with blocked gates is a valid plan. JSON: the `Plan` with `blocked`.
- **CLI `apply`**: the same node and `blocked:` lines in the run record's text, then
  `state: blocked` and `re-run this document once done`. Exit status **3** when the run ended blocked
  and nothing failed (0, 1 and 2 are taken); a failure still exits 1.
- **MCP**: `plan`'s `PlanResponse.plan.blocked`; `run_status`'s `RunRecord` gains `state: "blocked"`,
  `blocked: [BlockedGate]` (skip if empty) and `next_step: "re-run this document once every blocked
  gate's need is met"` (skip if absent), so an agent can forward it verbatim. The two tools'
  descriptions say: on `blocked`, forward each entry's `need`, `how` and `subject` to the operator,
  then call `plan` again with the same inputs (plus any `awaiting_inputs`) once done.

**8. The journal.** Additive, no existing line changes. `PlanRecorded.plan` (redacted JSON) carries
`blocked` only when non-empty; `fingerprint` carries the two new actions. `NodeFinished` gets statuses
`blocked` and `skipped`. `RunFinished`'s `Outcome` gains `Blocked { outputs:
Redacted<IndexMap<OutputName, Value>>, blocked: Vec<BlockedGate> }`, written when `apply` returns `Ok`
with a non-empty `blocked`; `Succeeded` otherwise, as today. The fold maps it to `RunState::Blocked`
and fills `RunRecord.blocked`. Old journals replay byte for byte (`pre_pass_2_replay`); an older binary
cannot read a newer `blocked` line, which is the usual forward-compatibility cost of a new variant.

**9. Invariants.**

- *Secrecy.* A gate never authors a string from its inputs: `need` and `how` are `&'static str`, and
  the engine renders only the declared `subject` ports, which `Catalog::insert` guarantees are
  non-secret `Exact` ports, through `Value::render` (an `Unknown` renders `<unknown>`). `BlockedGate`
  therefore holds no `Value` and no secret by construction, which is why the journal may store it
  unwrapped. `OperatorAcknowledgement` is public and is never a secret input.
- *`Plan::fingerprint`.* Unchanged in shape; two new `Action` values. The secret-is-not-drift marker is
  untouched.
- *Journal wire format.* Additive variants and skip-if-empty fields only (point 8).
- *Class.* Static, from the graph (point 5). Gates are pure, so they never raise a class.
- *No new input kind for secrets.* The awaited input is public and typed; the "no secret input types"
  refusal stands.
- *Idempotence is the resume.* No run state is saved. A re-run re-plans from provider state; every
  node is idempotent, so what ran reads `NoOp`/`Unchanged`, what was skipped runs once its gate passes.

**Snapshots that move, additively, and are not characterization drift:** the `schema_generation__*`
snapshots for `NodeStatus`, `Applied`, `AppliedNode`, `RunRecord` and `RunNode` (new variants and
fields), `mcp_server__the_tool_list_and_every_schema_is_snapshotted` (schemas and two descriptions),
and, in G3, the type-registry and tool-catalog snapshots (one new type, one new tool) and
`LIVE_TOOL_NAMES` **26 → 27** (`crates/willikins-server/src/catalog.rs` 51 lists `willikins-tools`'
pure tools too), with every site that pins the count. `acceptance__characterization_of_every_document.snap` changes only by the new
fixtures' entries.

**Walter, for T3 (not designed here).** Two observed gates: the app record exists (a new read-only
gate over `GET /v1/apps`, a leaf) and `APP_GROUPS` enabled on each of the three identifiers (a gate
per identifier that passes `identifier` through to that identifier's profile). The API shows
`APP_GROUPS` *enabled*, not the group *assigned* (pre-flight row 14): the gate can pass before the
group is assigned, and the next run's replace-when-INVALID heals the profiles Apple then invalidates.
The remaining manual steps become `operator.acknowledge` leaves or are dropped, T3's call.

## The named manual steps

**2026-09-28:** under decision (j) these are **gate candidates** for T3, not outputs; M4 is gone (the
profiles are in the document behind the app-group gate), and M1 and M2 are observed gates. The table
is kept as the list of what the operator must do.

Each appears in the plan as an output. Order matters where stated.

| Id | Output | When | What |
| --- | --- | --- | --- |
| M0 | `manual_m0_base_configs` | before the real apply | Confirm the shared base config names in the operator's Doppler workplace and edit `base_configs`' default if they differ; they must exist and be inheritable |
| M1 | `manual_m1_app_record` | after the apply (the identifier must exist) | Create the App Store Connect app record for the app identifier (Apps → + → New App). Account Holder must have signed the latest agreement. SKU per the operator's own convention |
| M2 | `manual_m2_app_group` | after the apply, before M4 | Register `group.<app identifier>` (Account Holder or Admin), enable App Groups on all three identifiers and assign the group (portal Configure, or Xcode). This modifies the App IDs, so it must precede M4 |
| M3 | `manual_m3_monorepo_files` | **before** the real apply for `.buildkite/`; any time for the rest | Add Walter's files under `apps/walter/` following the survey above: `BUILD.bazel`, three entitlements files (HealthKit, `aps-environment`, app group, data protection on the host; app group on both extensions; background delivery and clinical records only if used), `Info.plist` with both HealthKit usage strings and the two extension points, privacy manifest, and `.buildkite/` with triggers disabled |
| M4 | `manual_m4_profiles` | after M2 | Run `workflows/appstore-signing-profile-from-doppler.yaml` once per identifier (content into project `walter`, `prd`) |
| M5 | `manual_m5_apns_key` | before Walter sends a push | Confirm the team's APNs auth key (`.p8`) is in a shared base config Walter inherits; if none exists, create one in the portal (Account Holder or Admin; downloadable once) |
| M6 | `manual_m6_ci_doppler_access` | before the pipeline's first signing build | Grant the Buildkite CI Doppler service account read access to project `walter` (no `doppler.project_member.ensure` yet) |
| M7 | `manual_m7_buildkite_bootstrap` | after the apply, after M3's `.buildkite/` lands | Replace the `walter` pipeline's stored bootstrap with `apps/walter/.buildkite/bootstrap.yml`, read it back and compare; keep provider triggers disabled until validated; add the repository webhook only then |

## The Walter document

**Superseded 2026-09-29 by the T3c addendum** (gates, not manual-step outputs;
no `template.render` nodes; the three profiles are in the document; `base_configs`'
default is decision 2's real configs, one substituted per the addendum's verify
item). Kept below for history only.

`workflows/walter-ios-app.yaml`. Inputs: `config: DopplerConfig` (ASC credential; no default),
`app_identifier`, `nse_identifier`, `widgets_identifier: AppleBundleIdentifier` (**no defaults**,
trust boundary 2), `platform: AppleBundleIdPlatform` (default `IOS`), `data_protection:
AppleCapabilitySetting` (default `DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH`),
`org: GitHubOrg` (default `Bande-a-Bonnot`), `slug: ProjectSlug` (default `walter`), `monorepo:
GitHubRepo` (default `Bande-a-Bonnot/monorepo`), `buildkite_org: BuildkiteOrg` (no default),
`cluster: BuildkiteClusterName` (no default), `environments: list<EnvironmentSlug>` (default
`[dev, stg, prd]`), `base_configs: list<DopplerConfig>` (default `[bande-a-bonnot-shared/ios_base]`).

Nodes: the seven-node ASC credential chain exactly as `appstore-bundle-id-from-doppler.yaml`;
`app_id`, `nse_id`, `widgets_id` (`appstore.bundle_id.ensure`, names from the identifiers by
conversion); `healthkit`, `push`, `data_protection` (`appstore.bundle_id_capability.ensure` on
`steps.app_id.identifier`, the last with `setting: ${{ inputs.data_protection }}`); `names`
(`naming.v1`); `doppler`, `configs` (`for_each`), `inherit` (`for_each`); `monorepo_ref`
(`github.repo.get`); `buildkite_cluster`, `pipeline` (`repo: ${{ steps.monorepo_ref.repo }}`); eight
`template.render` manual-step nodes. Outputs: `app_id`, `nse_id`, `widgets_id` (the ids the dry run
deletes by), the three identifiers, `pipeline_url`, and the eight `manual_*` outputs. Every node is
`Reversible`; no `SinkToken` is minted; no approval is needed. The header comment states the operator's
real identifiers, the M-steps, and why profiles are absent.

## The dry run (written and run by the attacker after task 3)

**Step 4 superseded 2026-09-29 by the T3c addendum**: `base_configs`' real default
no longer names `bande-a-bonnot-shared/ios_base` (that project is retired, decision
2), and step 4's "assert all eight `manual_*` outputs" no longer applies -- gates
replaced outputs. The dry-run harness (still the next lane's job, still blocked on
the sandbox Buildkite token) should instead assert `plan --live`'s `blocked` names
the two observed gates it expects unmet, then a second `plan --live` after
satisfying them shows `blocked` empty and the profile nodes `Create`.

A guarded harness, `crates/willikins-cli/tests/live_walter_dry_run.rs` (`live-tests` feature, `#[ignore]`,
`WILLIKINS_LIVE_TESTS=1`), driving the built binary against the live providers:

1. Read-only counts: Apple (bundle ids, profiles, certificates), GitHub sandbox repositories, sandbox
   Doppler projects, sandbox Buildkite pipelines.
2. Set up the sandbox: create `Willikins-Test/monorepo` (through `github.repo.ensure` or the sandbox PAT
   on `curl`'s stdin), create project `bande-a-bonnot-shared` with config `ios_base` and mark it
   inheritable. Record each for teardown.
3. Refuse to continue unless `app_identifier`, `nse_identifier`, `widgets_identifier` are
   `com.willikins.probe.delete-me.<unique>`, `…<unique>.nse`, `…<unique>.widgets`.
4. `plan --live`, then `apply`, with `monorepo=Willikins-Test/monorepo`, `org=Willikins-Test`,
   `buildkite_org=willikins-test`, `base_configs=[bande-a-bonnot-shared/ios_base]`,
   `config=app-store-connect/prd`. Record `app_id`, `nse_id`, `widgets_id` from the apply's outputs
   **into the guard before any assertion**. Assert all eight `manual_*` outputs are present and name the
   throwaway identifier.
5. Re-`apply`: every node `Unchanged`.
6. Teardown in the guard, which runs on every drop: the three Apple identifiers by recorded id
   (deleting an App ID removes its capabilities), the Buildkite pipeline, Doppler projects `walter` and
   `bande-a-bonnot-shared`, the stand-in repository. Then every count equal to step 1, and each
   recorded Apple id answering `404` to an independent read.

The profile tool is never called; certificates only ever see `GET`. Blocked until a working sandbox
Buildkite token exists.

## Acceptance tests

1. **`AppleCapabilitySetting`** accepts its four members and its example; refuses `ICLOUD_VERSION=XCODE_6`,
   a bare key, a bare option, lowercase, `KEY=` and a mismatched pair such as
   `APPLE_ID_AUTH_APP_CONSENT=COMPLETE_PROTECTION`.
2. **The pairing refusal** — `DATA_PROTECTION` without `setting`, `APPLE_ID_AUTH` with a data-protection
   setting, and `HEALTHKIT` with any setting each fail at `read` with `Invalid` and make **no** request
   (mockito expects zero calls). Negative fixture `workflows/fixtures/appstore-capability-setting-mismatch.yaml`
   (a `HEALTHKIT` node given a setting) fails `plan` against the fake catalog with that error.
3. **Create body** — with a setting, a JSON matcher pins exactly `capabilityType`, the one-element
   `settings` of decision (d) and the `bundleId` relationship; without one, no `settings` key. One
   `POST`, never retried.
4. **Read** — requested option enabled → `Present`; another option enabled → `Mismatch { setting }`; no
   option enabled, `settings` absent, or the key absent → `Mismatch { setting }`; a `Mismatch` never
   issues a write; the portal-configuration refusal for `APP_GROUPS`/`APPLE_PAY`/`ICLOUD` is unchanged.
5. **Fake twin and parity** — the fake capability tool gains the port and the pairing refusal;
   `catalog_parity` snapshots equal; `fake_agrees_with_live` covers each new arm.
6. **`github.repo.get`** against a mock — 200 → `Present` with `repo`; 404 → `NotFound`; archived →
   `Conflict`; 401 → `UNAUTHENTICATED`; 403 → `MISSING_PERMISSION`; only `GET` is ever recorded; pure;
   its fake twin agrees; both catalogs validate; `LIVE_TOOL_NAMES` is 26 everywhere it is pinned.
7. **Conversions** — two new rows, each with a proptest proving every `AppleBundleIdentifier` parses as
   the target; the reverse (`Text` into an `AppleBundleIdentifier` port) still fails `check` with a type
   mismatch (negative fixture).
8. ~~**The Walter document** checks clean against both catalogs; plans against the fake catalog with
   throwaway inputs; the plan's outputs carry all eight `manual_*` texts, each naming the supplied app
   identifier; each capability node's `identifier` edge comes from `app_id`; no node is a profile; no
   `SinkToken`; a fake `apply` then a second `apply` reads every node `Unchanged`; omitting any identifier
   input fails `plan` naming it.~~ **Rewritten 2026-09-29 by the T3c addendum**: the document checks
   clean against the fake catalog (`the_document_checks_cleanly_against_the_fake_catalog`); the three
   profiles ARE nodes, each behind its own app-group gate; a first run over the fake catalogue blocks on
   the app-record and app-group gates plus the four acknowledgement leaves, skipping exactly the profile
   and Doppler-write nodes; a second run (fake state mutated to satisfy the two observed gates) creates
   the profiles and their Doppler writes and nothing else; a third run (the four acknowledgements
   supplied) is a clean converge with no `Blocked`/`Skip` action anywhere
   (`crates/willikins-cli/tests/walter_document.rs`).
9. **Characterization** — `acceptance__characterization_of_every_document.snap` changes only by new
   entries, asserted in each task: task 1 adds the setting-mismatch fixture's entry, task 3 adds the
   document's and the conversion fixture's; every existing entry is byte-identical.
10. **The live capability cycle** (task 1, run by the attacker once): counts; one throwaway identifier;
    `HEALTHKIT`, `PUSH_NOTIFICATIONS`, and `DATA_PROTECTION` at `PROTECTED_UNTIL_FIRST_USER_AUTH` each
    `changed: true`, then re-`ensure` `changed: false`; a read of `DATA_PROTECTION` with
    `COMPLETE_PROTECTION` requested reads `Mismatch { setting }` and the capability row list is identical
    before and after it; the raw row's settings shape recorded by key names and booleans only; the
    identifier deleted by its create id; counts equal; the id answers `404`.
11. **The dry run**, as above.
12. **Gates in `plan`** (G1). With a gate reading `Absent`: the gate is `Blocked`; every node reachable
    from it by `Step`, `Keyed` or a `for_each` source is `Skip` and its `read` is never called; a node
    reachable only from other branches plans exactly as without the gate; `Plan.blocked` names the
    gate, its static `need`/`how`, its rendered `subject` and its `holds_back`. With the gate
    `Present`: `Compute`, its dependents plan as they would with no gate, and the plan's JSON has no
    `blocked` key at all. A gate whose `subject` names a secret or `AnySecret` port, or that is not pure, is refused
    by `Catalog::insert`. No `BlockedGate` JSON ever contains a seeded secret.
13. **Plan text** (G1). The `blocked:` section and `re-run this document once done` appear only when
    `blocked` is non-empty; `plan` exits 0 either way; every existing CLI text snapshot is unchanged.
14. **Gates in `apply`** (G2). A blocked run creates every independent resource, reports `Blocked` and
    `Skipped` statuses (not `NotRun`) in `Applied`, the journal and `RunRecord`, and records
    `Outcome::Blocked`; a second apply after the gate passes runs the skipped nodes and converges; a
    third reads every node `Unchanged`/`Computed`. A tool failure in the same run still stops the walk
    with a `NotRun` tail and `Outcome::Failed`. A gate flipped between plan and apply refuses as
    `Action` drift. Journals written before G2 replay byte for byte.
15. **Surfaces** (G2). CLI `apply` exits 3 on a blocked run, 1 on a failure, 0 otherwise; `run_status`
    returns `state: "blocked"`, `blocked` and `next_step`; the schema snapshots move only additively.
16. **Acknowledgement** (G3). `OperatorAcknowledgement` accepts `done` only; a default of it and a
    literal on a port of it each fail `check` (negative fixtures); an unsupplied one is `awaiting`, not
    `missing`, and the plan blocks on `operator.acknowledge` naming it in `awaiting_inputs`; supplying
    `done` makes the gate `Compute`; the recorded plan inputs never contain an unsupplied one.

## Credentials

**Needed now (dry run):**
- A **fresh sandbox Buildkite API token** for organisation `willikins-test`, scopes `read_pipelines`,
  `write_pipelines`, `read_clusters` — the current one answers `401` on a no-scope call. Blocks the dry
  run only.
- Present and working: the sandbox GitHub PAT (`Willikins-Test`, which can create and delete a
  repository there, proven by milestone 2); the sandbox Doppler token (can create projects, proven by
  milestone 2's smoke run); the ASC credential in sandbox Doppler `app-store-connect/prd`.

**For a later real apply (the operator's, not this milestone's):**
- A Doppler service-account token for the operator's real workplace that can create project `walter`,
  its configs and their inheritance, and read the ASC credential from a config in **that same**
  workplace.
- A GitHub token that can read `Bande-a-Bonnot/monorepo` metadata (a fine-grained PAT, metadata read on
  that one repository); `github.repo.get` needs nothing more.
- A Buildkite token for the operator's organisation with `read_clusters` and `write_pipelines`
  (`write_pipelines` is also delete; there is no narrower grant).
- The ASC team key already used by the signing documents (it can create identifiers and profiles;
  whether it can enable capabilities is task 1's live question, verify item 3).
- Account Holder or Admin in the portal for M1, M2, M5.

## Post-flight checklist (the attacker fills this)

- [ ] Live capability cycle ran once; counts equal; the throwaway id answers `404`. — 2026-09-28: ran
  once, stopped at the `DATA_PROTECTION` re-ensure; counts 21/5/13 equal before and after by an
  independent recount, leftovers 0; the guard deleted the throwaway by its create id; the `404` read
  never ran (the cycle stopped first).
- [x] `DATA_PROTECTION` with a setting was accepted by Apple and read back as decision (d) assumes, or an
  addendum records the real shape and the adapted parse. — 2026-09-28: accepted, **not** read back as
  decision (d) assumed; the coordinator's second live capability cycle recorded the real shape (row
  `attributes: {capabilityType, settings}`; one setting entry `{key, options}`; option entry `{key}`
  only, no `enabled`; exactly the requested option listed; `HEALTHKIT`/`PUSH_NOTIFICATIONS` rows
  `settings: null`), and the parse is now adapted to it, test-first (`fcdb9da`; see the header
  addendum). This implementer did not re-run the live cycle to confirm the fix's own assertions pass
  live — only mock fixtures rebuilt on the observed shape were exercised.
- [x] The key can enable `HEALTHKIT` and `PUSH_NOTIFICATIONS`. — 2026-09-28, on a `UNIVERSAL` throwaway.
- [ ] Dry run applied, re-applied `Unchanged`, tore down; every count equal; no leftover in any sandbox.
- [ ] The eight manual steps appear in the live plan's outputs naming the throwaway identifier.
- [x] No operator identifier, certificate, profile, team id or credential in any file, log or commit.
  — 2026-09-28, for the capability cycle's three logs: UUID, `eyJ`, `PRIVATE KEY`, 24+ hex, 200+
  base64 and throwaway-identifier greps all 0.
- [x] `no_certificate_writes_guard`, `secret_literal_guard`, `no_gh_writes_guard` green. — 2026-09-28:
  27, 15, 7 passed.

## Verify before relying on them

1. **Does a create carrying `settings` succeed with `options: [{key, enabled: true}]`?** Unobserved; the
   live cycle settles it. **Settled 2026-09-28: yes** (accepted, `changed: true`).
2. **How does a capability row report its selected option** — by `enabled: true` on one option, or
   another way? Unobserved. **Settled 2026-09-28 (the coordinator's second live capability cycle):** by
   listing the selected option's `key` alone — no option ever carries `enabled` at all. The candidate
   that held (research record, section 4): `enabled` absent on every option, not `settings` absent, not
   Apple's default applied, not two enabled. The read rule is adapted accordingly (`fcdb9da`).
3. **Can the ASC team key enable capabilities at all?** No capability has ever been enabled through it.
   **Settled 2026-09-28: yes** (`HEALTHKIT`, `PUSH_NOTIFICATIONS` created and converged).
4. **Is `DATA_PROTECTION` without a setting accepted?** The tool's module doc claims it; never proven.
   Task 1 refuses it anyway, so the answer only matters for the doc correction.
5. **Does App Store validation need anything on the NSE's App ID beyond registration?** Apple's pages
   ask nothing of it; the first real upload settles it.
6. **Does a re-POST of an enabled capability return `409`, `201` or `200`?** Irrelevant to correctness
   (list-then-branch), carried forward from the research note.
7. **Is `filter[identifier]` still substring for suffixed siblings** (`<unique>` also matching
   `<unique>.nse`)? Expected yes; the exact compare already handles it; the dry run exercises it.
8. **Does the operator's Buildkite organisation auto-register webhooks** for an API-created pipeline
   (for example through a full-access GitHub App)? Buildkite's doc says the webhook is added by hand.
9. **The real shared base config names** in the operator's Doppler workplace (M0).
10. **Does the host credential helper answer for `Bande-a-Bonnot/monorepo.git` without a
    `GIT_CONFIG_*` override?** The cookbook says it did on the Mini; M7 copies the override regardless.
11. **Which `BundleIdPlatform` the operator's existing identifiers use** (counts only); the document
    defaults to `IOS`.
12. **Is the `github` project's base config actually named `bande_a_bonnot` (underscore), or something
    else?** (T3c addendum, 2026-09-29.) Decision 2's prose spells it `bande-a-bonnot`; this codebase's own
    convention snakes hyphens to underscores for Doppler config names, and Doppler's own docs do not
    settle whether a hyphen is even accepted. `workflows/walter-ios-app.yaml`'s `base_configs` default
    uses the underscored spelling pending a read-only list of that project's configs. **Settled 2026-09-29
    (R1, Doppler name grammars):** the real-workplace probe answers this directly -- the `github` project's
    config is named `bande-a-bonnot`, hyphenated, exactly as decision 2 spelled it, not `bande_a_bonnot`. The
    open half of this item was never Doppler's own answer, it was `willikins_types::DopplerConfigName`'s
    grammar, which could not spell the hyphenated name until this addendum; correcting the document's
    `base_configs` default is not R1's to make, though: this plan's own characterization-snapshot boundary
    says the snapshot may change only by *adding* a document, and editing an existing document's default
    changes its `describe`/`plan` output, so it stays the coordinator's call.
13. **What does `GET /v3/configs/config` answer for a branch config absent from a project that does
    exist** (task B1, 2026-09-29)? Every other Doppler `read` in this crate tolerates a missing
    *project* (`404`, or a `400` naming "no access"); a missing *config* under an existing project is a
    different case no primary source read for this task settles. `doppler.branch_config.ensure`'s mock
    suite pins only the `404` shape as `Absent`; a `400` naming anything else still fails the plan
    (`read_still_propagates_a_400_with_an_unrelated_message`). **Not only that tool's own node**: on the
    very first run against a real workplace, `walter-ios-app.yaml`'s `inherit` node
    (`doppler.config.inherits.ensure`, which reads the same `GET /v3/configs/config` for
    `walter/prd_deployment_ios` and uses the identical `looks_like_a_missing_project` predicate) reads
    this exact config *before* `prd_config`'s own `ensure` has created it — so if the real answer turns
    out to be an unverified `400`, the plan fails at `inherit`, not at `prd_config`, and the coordinator
    should look there first. Verify before a real apply creates `prd_config` for the first time against
    the operator's own workplace.
14. **Does an immediate same-name `POST /v1/profiles` succeed right after replace-when-INVALID's
    `DELETE`?** (2026-09-29, adversarial pass 2.) Milestone 3c found profile names unique per identifier
    (`409 ENTITY_ERROR` on a duplicate). If the delete propagates lazily, every first replacement fails
    with `409` — now reported as "the INVALID profile at this key was deleted … re-run to create it"
    (`79a4f28`) — and converges on the next run. The written, unrun `appstore_live_profile_replace_cycle`
    settles it.
15. **Does `GET /v1/bundleIds/{id}/bundleIdCapabilities` accept `limit`?** (2026-09-29.) The OpenAPI
    description 4.5 says yes, up to 200. **Settled 2026-09-29, live: no** — `400 PARAMETER_ERROR.ILLEGAL`
    "A given parameter is not allowed for this request"; with no query, `200`. ~~The client still sends it
    (`e4ba9af`); see the diagnosis addendum.~~ **Fixed 2026-09-29** — the first request now carries no
    query string at all; see the addendum at the end of this file.

**Verify list, 2026-09-29 (adversarial pass 2):** no provider was called, so items 5–13 are unchanged:
5, 7 and 11 need the dry run or a live read; 6 stays irrelevant to correctness; 8, 9 and 10 need the
operator; 12 and 13 need a read-only list and a `GET` against the operator's own Doppler workplace
before a real apply.

## Gates

Scoped, per the host rules: `pgrep -x cargo` and `pgrep -f cargo-sweep` must print nothing before every
cargo command; `-j 2`, `RUST_TEST_THREADS=2`; in the background with a 600,000 ms timeout; read the log
body; never pipe through `tail` or `tee`; never edit tracked files while cargo builds.

```
cargo fmt --all --check
cargo clippy -p <touched crate> --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test -p <touched crate> -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
```

plus `RUST_TEST_THREADS=2 cargo test -p willikins-dsl --test acceptance -j 2` for the characterization
snapshot. The full workspace gate is the coordinator's. A linker "missing .rcgu.o" or `E0463` is the
host sweep: `cargo clean -p <crate>` and rebuild.

## Tasks

One lane at a time on `main`, in order; each commits by path with `git commit --only`.

| # | Task | Delegate to |
| --- | --- | --- |
| T1 | **Capability settings.** Commit 1: `AppleCapabilitySetting` in `willikins-types` (registry entry); the optional `setting` port, the pairing refusal at `read`, the create body, the settings-aware read and `Mismatch { setting }` in `appstore.bundle_id_capability.ensure` and its client; the module doc corrected; the fake twin; mock tests; the negative fixture (acceptance 1–5, 9). Commit 2: `tests/live_capability_cycle.rs` with its own `[[test]]` entry, **written, not run** — the attacker runs it (acceptance 10) | sonnet implements, opus attacks and runs the live cycle |
| T2 | **`github.repo.get`.** One or two commits: the pure read-only tool over `GitHubClient::get_repo`, its fake twin, mock tests, `LIVE_TOOL_NAMES` 25 → 26 with every pinned site, catalog and MCP snapshots (acceptance 6, 9) | sonnet implements, opus attacks |
| G1 | **Gates in `plan`** (decision (j), points 1–3). Commit 1, `willikins-core` only: `Gate` and `Tool::gate()` (default `None`); `Catalog::insert` refuses a gate that is not pure or whose `subject` port is missing, not `Exact`, or secret; `Action::Blocked` and `Action::Skip`; the skip set in `plan` (a node binding a blocked or skipped node, through `with` or `for_each`, `Step` or `Keyed`, is `Skip` and never read); `NodeResult::Skipped` in `resolve_step`/`resolve_keyed`/aggregation; a skipped `for_each` source planning one `instance: None` entry; `BlockedGate` and `Plan.blocked` (skip if empty). Test-first against small in-test tools, one of whose `read` panics if called: a blocked gate's dependents are `Skip` and unread, transitively; an independent branch plans as before; a met gate is `Compute`; a `Keyed` dependent of another instance is not skipped; outputs bound to skipped nodes are `Unknown`; `BlockedGate` renders a secret-free subject (acceptance 12). Commit 2, `willikins-cli`: `plan` text shows `Blocked`/`Skip` and the `blocked:` section with `re-run this document once done`; exit 0; no section when empty (acceptance 13). Characterization byte-identical (acceptance 9) | sonnet implements, opus attacks |
| G2 | **Gates in `apply`, the journal, the CLI and MCP** (points 4, 5, 7, 8). Commit 1, `willikins-core` and `willikins-journal`: `NodeStatus::Blocked`/`Skipped` with both events emitted, classified by `planned.action` before input resolution, the pure branch and the unknown-input match (a skipped non-pure node must not stop the run as `UnknownInput`; test it with a non-pure dependent); the walk continues past them; `Applied.blocked`; the `apply.rs` grouping builds `NodeResult::Skipped`; failure still stops with a `NotRun` tail; `Outcome::Blocked`, `RunState::Blocked`, `RunRecord.blocked`/`next_step`; a gate satisfied between plan and apply is `Action` drift (acceptance 14). Commit 2, `willikins-cli` and `willikins-server`: run text, exit 3 on a blocked run, MCP descriptions, the additive schema snapshots listed in (j) (acceptance 15) | sonnet implements, opus attacks |
| G3 | **Operator acknowledgement** (point 6). Commit 1, `willikins-types` and `willikins-core`: `OperatorAcknowledgement` (grammar `done`, public, registry entry); `check` refuses a default of it and a literal on a port of it (two negative fixtures); `describe`'s `awaiting` instead of `missing`; `plan` resolves an unsupplied one `Unknown` without putting it in the resolved map; `BlockedGate.awaiting_inputs`. Commit 2, `willikins-tools` and `willikins-server`: `operator.acknowledge`, `LIVE_TOOL_NAMES` 26 → 27, catalog snapshots, a positive fixture that plans blocked without the input and `Compute` with it, and CLI `supply:` lines (acceptance 16) | sonnet implements, opus attacks |
| T3 | **The Walter document** (after G1–G3; **2026-09-28**: its acceptance 8, dry-run step 4 and the post-flight "eight manual steps" item were written for decision (a) and are rewritten by T3 for gates: the observed app-record and app-group gates, profiles behind the app-group gate, the remaining steps as acknowledgement leaves; the Doppler layout of operator decision 2). Commit 1: the two conversion rows with their `From` impls, proptests and the reverse negative fixture (acceptance 7). Commit 2: `workflows/walter-ios-app.yaml` and its graph tests over the fake catalogue in `crates/willikins-cli/tests/walter_document.rs`, any fake-state fixture it needs under `workflows/fixtures/state/` (acceptance 8, 9) | sonnet implements, opus attacks, writes and runs the dry run once a Buildkite token exists |

## Risks

1. **Apple's settings shape differs from decision (d).** The live cycle fails at its first
   `DATA_PROTECTION` create or read; the guard deletes the throwaway; the attacker adapts the parse test
   first. Mocks alone could not have caught it, which is why the live cycle is required.
2. **The key cannot enable capabilities** (`403`). The tools still land on mocks; the milestone is not
   complete until a key that can exists, and the operator is told which role it needs.
3. **The data protection level is the operator's call.** Decision (e) argues for the default class; a
   stricter one is one input change, but `COMPLETE_PROTECTION` needs Walter's code to write NSE-readable
   files with an explicit weaker class.
4. **A manual step is only a report.** *(Addressed 2026-09-28 by decision (j): the app-group gate
   keeps the first run from minting profiles, and a re-run replaces any Apple invalidates later. What
   remains: the API shows `APP_GROUPS` enabled, not the group assigned, so the gate can pass early.)* Skipping M2 before M4 yields profiles without the app group,
   which Apple will invalidate on M2 — recoverable by re-running M4, not silent corruption.
5. **The dry run is blocked on a Buildkite token.** Tasks 1–3 do not need it.
6. **Build time and disk.** Two small crates touched per task; the host's 60 GB target directory and
   the 04:00 cargo-sweep remain the constraint; never gate across the sweep.

**Addendum:** 2026-09-29 (implementer) — **F1 and F2 landed, two commits** (`27a1a09`, `1a4f16d`),
closing pass 2's finding 4 and the last sentence of its finding 1.

- **F1: an approved plan now shows a replacement's delete, not only `Create`** (`27a1a09`). A tool-declared
  hook, `Tool::replaces(&self, inputs: &Inputs) -> Result<bool, ToolError>`, default `false`, in the exact
  manner of `Tool::gate()` and for the same stated reason: an `Observation` variant has about fifteen
  exhaustive matches across the workspace and `Observation::Absent`'s own struct literal is built at
  about forty call sites, so widening either would touch code this fix need not touch. `plan` calls it
  only when a non-pure, non-gate tool's `read` has already answered `Absent` — exactly the case that
  would otherwise plan `Action::Create` — so every other tool's plan, `read_calls` count, and the fake's
  own bookkeeping are unaffected. A `true` answer plans `Action::Replace` instead, an additive sibling of
  `Blocked` and `Skip`, riding in `InstanceFingerprint::action` as `"replace"` exactly as they do.
  `Plan` gains `replacing: Vec<Replacing>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`,
  learning finding 1's own lesson so the field is never `required` in the published MCP schema), mirroring
  `Plan.blocked`: each entry names the node, instance, tool, and the resource's own key ports
  (`ToolSpec::key`) rendered from the bound inputs `plan_one` already holds — known already, since a key
  port must be `Known` before `Tool::read` is even called, and rendered through `Value::render()`, so it
  is no secret by construction the same way `BlockedGate::subject` is. `AppstoreProfileEnsure::replaces`
  re-resolves the bundle id and profile row (the same cost `ensure` already pays to re-resolve at apply
  time) and answers `true` only for the one `INVALID` record at this exact key; the fake twin mirrors it
  without incrementing `read_calls`. The CLI's `action_text` renders `Replace` (the compiler forced the
  new arm); a new `replacing:` section, beside `blocked:`, names what goes away, in both the CLI text and
  therefore the approval text an operator or approver reads before granting a `Destructive` plan. The MCP
  `plan` tool's description now tells an agent to forward a non-empty `replacing` entry's `subject` to the
  approver; `PlanResponse` carries the field for free, since it embeds the same `Plan` type.
  Tests, test-first: `profile_ensure_mock.rs`'s four `replaces_*` mock tests (`INVALID` → `true`;
  `ACTIVE`, absent, and a different-named `INVALID` row → `false`; no `DELETE` or `POST` mock registered
  at all); `fake_agrees_with_live.rs`'s three `profile_replaces_is_*_and_agrees` parity tests; and
  `profile_documents.rs`'s `an_invalid_profile_plans_as_a_replacement_naming_the_profile_by_its_non_secret_name`,
  which plans the real signing document (`workflows/appstore-signing-profile-from-doppler.yaml`) against
  a new `workflows/fixtures/state/appstore-signing-profile-invalid.json` and asserts `Action::Replace`
  plus one `Plan::replacing` entry naming the profile by its own `(identifier, name)` — both equal to
  `com.example.willikins-demo`, the document's own convention, never a secret. The existing ACTIVE-profile
  positive test now asserts `Action::NoOp` and an empty `Plan::replacing` explicitly, so the two arms are
  both pinned in the same file. `render.rs` gained `plan_text_shows_a_replacement_and_names_what_it_deletes`
  and its mirror, `plan_text_has_no_replacing_section_when_nothing_replaces`. Every `Plan { .. }` struct
  literal in the tree (about twenty, across `willikins-cli`, `willikins-core` and `willikins-journal`'s
  own tests) gained `replacing: Vec::new()` mechanically; none of their behaviour changes. Not done, left
  for the coordinator: a `Replaced` `NodeStatus`, or a `replacing` field on `Applied`/`RunRecord` — `apply`
  already re-plans before running and its own node line is the same `PlannedNode`, so `Action::Replace`
  reaches it for free without either.
- **F2: `BlockedGate.awaiting_inputs` gets a serde default** (`1a4f16d`). The field arrived at task G3,
  after `BlockedGate` already derived `Deserialize` at G2, with no `#[serde(default)]` — so a
  `RunFinished { Blocked }` journal line written by a binary built between G2 and G3 carries no
  `awaiting_inputs` key at all, and replaying it now failed outright rather than reading it as empty.
  Fixed with the one attribute; a new test,
  `a_run_finished_blocked_line_without_awaiting_inputs_still_deserializes`
  (`crates/willikins-journal/tests/event_shapes.rs`), parses such a line directly and confirms it still
  round-trips forward. The fix also drops the field from its own published JSON Schema `required` list —
  the identical corrective move pass 2's finding 1 made for the other three fields, so the
  `schema_generation` and MCP tool-list snapshots moved the same way, additively.
- **Both verified test-first**: F2's fix was confirmed red without `#[serde(default)]` (a fresh failure
  reproduced by temporarily reverting the attribute) and green with it, before either commit.
- **Gates run, scoped, both commits**: `cargo fmt --all --check`; `cargo clippy --all-targets -D warnings`
  on every touched crate (`willikins-core`, `willikins-cli`, `willikins-journal`, `willikins-server`,
  `willikins-providers-appstore`, `willikins-providers-fake`); `cargo test` over the same crates, every
  suite green (schema-generation and MCP tool-list snapshots accepted additively, reviewed diff by diff
  before accepting); `cargo test -p willikins-dsl --test acceptance` — the characterization snapshot is
  byte-identical, confirmed by a passing run with no `INSTA_UPDATE`, not merely by inspection. The full
  workspace gate was not run (host rule; the coordinator's).
- **Not settled, unaffected by this addendum**: the plan's own open verify items (5–14) and the
  T3-blocking `NotFound`-at-plan-time finding from the 2026-09-28 pass; neither touches gates,
  acknowledgement, or replace-when-INVALID's own resolution logic. Plan not marked Completed.

**Addendum:** 2026-09-29 (attacker, third pass) — **F1's guarantee had a hole inside the run; closed
test-first (`3bd1ee9`), one test gap closed and F1's open coverage added (`439207d`), eight mutation runs
(one survived, then killed).** Record: `docs/research/2026-09-29-m3e-adversarial-pass-3.md`.
- **An approved plan could show `NoOp` while `apply` deleted.** `check_drift` runs once before any node,
  and `apply` calls `ensure` on every non-pure instance, `NoOp` included. A profile `ACTIVE` at approval
  and at the re-plan, invalidated mid-run by an earlier capability node on its identifier (Walter's
  capability nodes are declared first and run first), was deleted and recreated by
  `appstore.profile.ensure` while the plan said `NoOp` and `Plan::replacing` was empty. **Rule 4a** in
  `willikins_core::apply`: just before `ensure`, an instance not planned `Replace` asks
  `Tool::replaces`; `true` fails it with `Conflict`, nothing deleted, and the re-run plans and names the
  `Replace`. Every other tool pays nothing (default `false`); **the profile tool pays one extra read-only
  resolve per instance per `apply`**, so a live `apply` of Walter reads each profile twice.
- **Held, by mutation:** the replacement's subject is exactly the key (`identifier`, `name`), and neither
  the certificate id nor the `INVALID` profile's id reaches `Plan::replacing` (m7a survived the old test,
  m7b killed by the strengthened one); a replacement-free plan stays byte-identical in the
  characterization snapshot (m1); `plan` never shows `Create` where `replaces()` says `true` (m2); an old
  `RunFinished { Blocked }` line reads back (m3); the harness never prints `errors[].detail` (m4) and
  repeats only anchored willikins-written messages (m5); rule 4a is load-bearing (m6).
- **Added:** `ACTIVE` at approval, `INVALID` at `apply` refuses as `DriftKind::Action { NoOp -> Replace }`
  with nothing deleted.
- **Accepted risks, recorded:** Apple's `title` is echoed (escaped, bounded); `is_code_shape` admits an
  id-shaped string. **Still open:** ~~the capability list's `limit` fix (the diagnosis addendum's
  prescribed fix), so replace-when-INVALID stays mock-proven~~ **closed 2026-09-29, see the addendum
  below** — replace-when-INVALID still stays mock-proven, since the fix is client-only and no live
  cycle was re-run; an MCP conformance case for a non-empty `replacing`.
  Plan not marked Completed.

**Addendum:** 2026-09-29 (implementer) — **the diagnosis addendum's prescribed fix landed test-first,
one commit.** `AppstoreClient::list_bundle_id_capabilities`'s first request now carries no query string
at all; only a later page, reached through `links.next`, carries one, and only because Apple's own
`next` URL does — exactly as prescribed. The three existing pagination mocks that pinned
`limit=200` on the first page now pin `Matcher::Missing` (no query) instead. A new mock test,
`read_sends_the_first_capabilities_request_with_no_query_string`
(`crates/willikins-providers-appstore/tests/bundle_id_capability_ensure_mock.rs`), registers its mock
with `match_query(Matcher::Missing)` (so it only matches a request whose full path carries no query
whatsoever) and fails if the first request still attaches `?limit=200` — the gap every other
capability mock's `match_query(Matcher::Any)` left open, per the diagnosis addendum. **Confirmed not
vacuous, observed rather than assumed:** reintroducing `?limit={PAGE_LIMIT}` on the first request (in
the real file, restored from a saved copy after) made exactly four tests fail --
`read_sends_the_first_capabilities_request_with_no_query_string` (the new one) and the three retargeted
`Matcher::Missing` pagination tests, `read_finds_the_requested_capability_on_a_later_page`,
`ensure_never_posts_when_the_requested_capability_is_on_a_later_page`, and
`read_reattaches_only_the_query_string_never_the_host_or_path_links_next_names` -- 23 passed, 4 failed;
`read_refuses_past_the_page_cap_rather_than_spinning` (`Matcher::Any`) stayed green throughout, as
expected. The saved copy was restored (`cp`), `cmp` confirmed byte-identical, and the file was
`touch`ed; a re-run of the same single test target on the restored bytes was green, 27 passed, 0 failed
-- the last observed run is on exactly what this commit carries. One live clippy hit along the way,
also fixed: `clippy::doc_markdown` on a bare "OpenAPI" in two of this fix's own new doc comments
(`client.rs`, `bundle_id_capability_ensure_mock.rs`), backticked. The doc comment above the method is
corrected to state the live `400` and the first-request exception, citing this file's own diagnosis
addendum rather than repeating the old, now-wrong "asks for `PAGE_LIMIT` rows" claim; `live_write_cycle.rs`'s
`CAPABILITY_LIST_QUERIES` doc, which had claimed the client "sends" `?limit=200` in the present tense,
is corrected to the past tense window it actually describes (between `e4ba9af` and this fix).
`list_bundle_ids` (which does accept `limit` live) is unchanged. **Scoped gates green, observed:**
`cargo fmt --all --check` (no output); `cargo clippy -p willikins-providers-appstore --all-targets
--features live-tests -j 2 -- -D warnings` (clean after the one `doc_markdown` fix above); `RUST_TEST_THREADS=2
cargo test -p willikins-providers-appstore -j 2` (16 test binaries, every `test result:` line `0 failed`,
`live_probe.rs`'s 3 ignored the only non-zero-ignored suite, as expected for its opt-in live tests).
Not done here, for the coordinator: re-running `live_capability_cycle` and
`appstore_live_profile_replace_cycle` against the live account now that the client no longer sends the
refused `limit`, per the diagnosis addendum's own instructions — both live probes that diagnosed this
were already spent, and no provider call was made for this fix.

## Dry run, 2026-09-29 (coordinator)

Read-only `plan --live` of `workflows/walter-ios-app.yaml` for Walter: GitHub, Doppler and Buildkite in the sandbox
(org `Willikins-Test`, the new sandbox Doppler workplace, Buildkite `willikins-test` / `Default cluster`), App Store
Connect on the operator's live account, `Lightless-Labs/willikins` (public) standing in for the private monorepo, no
acknowledgement inputs supplied. Exit 0; nothing written. Class `Destructive`, `requires_approval: true`.

- **Compute (13):** the credential chain (7 nodes), the certificate selection, `names`, `monorepo_ref`,
  `buildkite_cluster`.
- **Create (13):** bundle ids `com.bande-a-bonnot.walter`, `.nse`, `.widgets` (named after themselves); capabilities
  HEALTHKIT, PUSH_NOTIFICATIONS, DATA_PROTECTION on the host; Doppler project `walter`, configs `dev`/`stg`/`prd`, branch
  config `prd_deployment_ios`, its inheritance; Buildkite pipeline `walter`.
- **Blocked (8):** `app_record`; `app_app_groups`, `nse_app_groups`, `widgets_app_groups`; acknowledgements
  `m3_repo_files`, `m5_apns_key`, `m6_ci_doppler_access`, `m7_bootstrap`.
- **Skip (6):** the three profiles and their three Doppler writes, each held back by its app-group gate.
- The report ends "re-run this document once done", with a `supply: --input <step>_done=done` line per acknowledgement.

**Found by the dry run, carried forward:** the inheritance node plans `Create` without confirming that its base
configs exist. In the sandbox they do not (`appstore-connect/deploy_ios`, `github/bande_a_bonnot`,
`open-telemetry/prd_signoz`), so an apply there would fail at that node. The real apply needs them present in the
operator's workplace, and the `github` project's real config slug is still verify item 12. The SigNoz key reaches
Walter through `open-telemetry/prd_signoz` inheritance; the document mints none, so no SigNoz call is made.

**Addendum:** 2026-09-29 (R1, Doppler name grammars) — **`DopplerConfigName` now accepts hyphens, settling verify
item 12 the other way than the T3c addendum guessed.** A fresh live probe against the sandbox workplace (this
plan's own "DOPPLER NAME GRAMMAR" header note) found Doppler accepted a branch config literally named
`prd_bande-a-bonnot` and a root config literally named `bande-a-bonnot`, with no underscore substitution — so
`github/bande-a-bonnot` was never a name this codebase's types could not express; it needed one character added
to a pattern, not a read-only list to disambiguate a spelling. `DopplerConfigName`'s grammar widens from
`[a-z0-9_]+` to `[a-z0-9_-]+` (max length, case and digit rules unchanged), and `DopplerConfig`'s hand-written
combined pattern is updated to match. Every other Doppler name type in `crates/willikins-types/src/doppler.rs`
(`DopplerProject`, `DopplerTokenName`, `SecretName`) was checked against Doppler's own API reference, fetched
verbatim 2026-09-29 (`.md` twins of `projects-create`, `projects-get`, `configs-create`, `configs-get`,
`service_tokens-create`, `secrets-update`, plus `docs/platform-limits`): every one of those schemas declares its
name field a bare `"type": "string"` with no `pattern` at all, so the reference does not mandate any of this
codebase's tighter grammars — they are recorded as willikins' own convention rather than a Doppler requirement,
each now with a "Grammar source" note in its own doc comment citing where that reading came from (the reference
when it speaks, this plan's live probe when the reference is silent). `EnvironmentSlug`
(`crates/willikins-types/src/slug.rs`) was checked the same way even though it lives outside `doppler.rs` and is
not itself a Doppler-owned type: Doppler calls the identical notion an "environment" and its `POST
/v3/environments`'s `slug` is equally a bare `"type": "string"` (`environments-create.md`, fetched 2026-09-29),
with a looser 2-50 character platform limit than this type's 16-character cap, so it too gained a "Grammar
source" doc note and a test pinning the probed root config's own name (`bande-a-bonnot`, an ordinary multi-word
slug already inside its grammar). No type's *behaviour* changed except `DopplerConfigName`'s and
`DopplerConfig`'s. Test-first: three
new `DopplerConfigName`/`DopplerConfig` tests pin the two probed strings exactly and one flips
`doppler_config_name_rejects_hyphens` to `doppler_config_name_accepts_hyphens`; the only snapshot this touched,
`crates/willikins-types/tests/snapshots/catalog__catalog_json_snapshot.snap`, moved by exactly the two lines
carrying `DopplerConfigName`'s and `DopplerConfig`'s published JSON Schema `pattern`, read diff-by-diff before
accepting. `naming::v1` is untouched (frozen) and its own invariant still holds: every snake-joined name it
produces was already inside the old, narrower grammar, so it is still inside the new, wider one. Scoped gates
green, on both commits: `cargo fmt --all --check`; `cargo clippy -p willikins-types --all-targets -j 2 -D
warnings`; `cargo test -p willikins-types -j 2` (412 lib tests plus every integration suite, catalog snapshot
accepted additively); `cargo check -p willikins-types -j 2`; and, since the type is used well beyond its own
crate, `cargo test -p willikins-dsl --test acceptance -j 2` (characterization snapshot byte-identical), `cargo
test -p willikins-providers-doppler -j 2` and `cargo test -p willikins-providers-fake -j 2` (both green, no
snapshot moved in either). The full workspace gate was not run (host rule; the coordinator's). Verify item 12
above is superseded by this addendum: the honest answer was never "which spelling does Doppler already use", it
was "our own type couldn't spell the hyphenated one yet."

**Not done here, recorded rather than fixed:** Doppler's own platform limits give `DopplerProject`,
`EnvironmentSlug` and `DopplerConfigName` a minimum length of 2, but all three still admit a single character
(`min_len` isn't set; the derive already supports it) — a one-character name passes `check` and would fail
live. And `naming.rs`'s `doppler_root_config` doc (the "Panics" note) already misdescribes `DopplerConfigName`
as "lowercase, digits, underscore; 64-character limit" before this addendum — it is 60, not 64, and now also
accepts hyphens; `naming::v1` is frozen, so this is a documentation-only staleness to fix in a future pass, not
a behaviour change, and it does not affect the "never panics" claim it supports (both the old and the new
grammar are supersets of a snake join's characters).

**Addendum:** 2026-09-29 (R2, GitHub credentials as ports) — **`willikins-providers-github`'s three
tools gain an optional, secret-typed `token` port ([`GitHubToken`](crates/willikins-types/src/github.rs)),
a `github.token.parse` tool mirroring `apple.signing_key.parse`, and the "credentials are ports,
resolvers are nodes" addendum now covers GitHub the same way it already covers App Store Connect.
Every existing document and the server keep working unchanged: the port is optional, so an unbound
`token` does exactly what it always did (falls back to the client built once from
`WILLIKINS_GITHUB_TOKEN` at catalog-construction time).**

- **`GitHubToken`** (`crates/willikins-types/src/github.rs`): secret, derived (`#[domain(secret, ...)]`),
  pattern mirrors `willikins_providers_github::CREDENTIAL_PATTERN`'s shape (`github_pat_`/`ghp_` prefix,
  no length bound GitHub does not itself document) but duplicated rather than imported, since a type
  in `willikins-types` may not depend on a provider crate. Unlike `DopplerServiceToken`'s fixed-length
  real shape, this type's pattern has no minimum length, so no test value here needs `concat!`-assembly
  to dodge `secret_literal_guard.rs`'s twenty-character floor — a short example such as `ghp_example` is
  a fully valid value and never reaches it. Registered in `domain_types!`; the catalog snapshot moved
  by exactly one new entry, reviewed diff-by-diff.
- **`GitHubToken::reveal_for_authorization`**, the fourth and narrowest token-less exception (after
  `OpaqueSecret`'s and `DopplerSecretValue`'s `reveal_for_transform` and `AppleSigningKey`'s
  `reveal_for_signing`): scoped to building one outbound `Authorization` header inside
  `github.repo.ensure`/`github.actions_secret.ensure`/`github.repo.get`'s own `Tool::read`, which never
  receives a `SinkToken`, when a document binds their optional `token` port. Named in `clippy.toml`'s
  `disallowed-methods` reason and in `crates/willikins-core/tests/expose_secret_guard.rs`'s
  `willikins-types` exemption list (`github.rs`, `reveal_for_authorization`).
- **`github.token.parse`** (`crates/willikins-tools`): pure, `AnySecret` input, mirrors
  `apple.signing_key.parse` line for line — a document chains `doppler.secret.get` (or `env.get`,
  optionally through `base64.decode`) straight into it. Registered in `willikins_tools::register`, the
  fake catalog, and the live catalog's `insert_pure_tools` (credential-independent, always present,
  exactly like the three `apple.*.parse` tools); `LIVE_TOOL_NAMES` 30 → 31.
- **A real defect caught before commit, not after:** the first `client_for_token` unconditionally built
  its `Http` against `GITHUB_API_BASE_URL`, so a document binding `token` would have talked to the real
  GitHub API even in every mock test and in the fake-catalog document test — exactly the "no provider
  call of any kind" boundary this task was given. Fixed with a new, general seam,
  `willikins_providers_http::Http::with_credential` (clones the route, headers, and sleeper; swaps only
  the credential) and `GitHubClient::with_credential` on top of it, so `client_for_token` now takes the
  tool's own default client and swaps its credential rather than building a fresh one against a fixed
  base URL. `client_for_token_preserves_the_default_clients_base_url` (in `client.rs`'s own tests) pins
  this by routing a bound-token call through a mock server and asserting on both the route and the
  `Authorization` header; `repo_get_mock.rs`'s
  `a_bound_token_port_is_used_even_when_the_default_credential_would_be_refused` is the sibling proof at
  the tool level (the default credential is refused outright by the mock, so the test can only pass if
  the bound token's credential authorized the request). The minted credential's own label is
  `GitHubToken port`, distinct from `CREDENTIAL_VAR`'s `WILLIKINS_GITHUB_TOKEN`, so a 401 against a
  Doppler-sourced token never points an operator at the wrong variable.
- **`ScopedClient`** (`crates/willikins-providers-github/src/client.rs`): `Default(&GitHubClient)` or
  `Bound(GitHubClient)`, `Deref`s to `GitHubClient` so every existing `self.client.method(...)` call
  site becomes `client.method(...)` unchanged regardless of which case applies. `ScopedClient::default_for`
  is the one place a tool decides between them, from `get_optional(inputs, "token")`.
- **Fake twins, catalogue parity, fake/live agreement:** all three fake GitHub tools
  (`willikins-providers-fake`) gained the identical `token` port for spec parity
  (`tests/catalog_parity.rs` continues to pin fake and live specs byte-for-byte equal; the three
  ToolSpec snapshots moved additively). `fake_agrees_with_live.rs` gained
  `agrees_on_present_with_the_token_port_bound`. The fake tools never inspect the port's value (they
  have no real credential to check), which is the correct behaviour for a fake.
- **Redaction, proven both ways:** `redaction.rs` gained a second marker, `PORT_TOKEN_MARKER`
  (`concat!`-assembled, matching `CREDENTIAL_MARKER`'s own convention even though this type's grammar
  does not strictly require it), and two tests —
  `a_bound_token_port_marker_reaches_no_header_but_authorization` and
  `a_bound_token_port_marker_reaches_no_observation_or_error` — proving a bound token's marker reaches
  no recorded request field but `Authorization`, and no `Observation` or `ToolError` (`Debug` or
  `message`) either, across both a `404` and a genuinely failing `500`.
- **A new document, an addition, not a change:** `workflows/github-repo-token-from-doppler.yaml`
  resolves `GH_CLONE_TOKEN` from Doppler through `github.token.parse` and binds it to
  `github.repo.get`'s `token` port — the operator's own words, quoted in this task's brief: the GitHub
  token "is in... doppler! The whole point of the damn thing!" Its own document test
  (`crates/willikins-providers-github/tests/github_token_documents.rs`) plans it end to end against
  seeded fake state (`workflows/fixtures/state/github-token.json`) and asserts the whole serialized
  `Plan` never carries the seeded token's raw value. `crates/willikins-dsl/tests/acceptance.rs`'s
  characterization sweep picks the new document up automatically; its snapshot gained exactly one
  `=== workflows/github-repo-token-from-doppler.yaml ===` block and nothing else — confirmed by diffing
  the pre-change snapshot against the accepted one and filtering out the insta-metadata line:
  `diff old.snap new.snap | grep '^[<>]' | grep -v assertion_line` produced twelve `>` lines, all inside
  the new block (types, outputs, and `PLAN ERROR: node \`token_secret\`: NotFound: ...` — the empty fake
  state has no seeded secret, the same shape every other Doppler-chain document's characterization entry
  already shows), zero `<` lines, and `grep -c ghp_` on the new snapshot is `0`.
- **Buildkite: not free, left as is.** Unlike GitHub, Buildkite has no domain type for its own token at
  all in this workspace (`willikins-providers-buildkite`'s own `CREDENTIAL_PATTERN` lives only as a
  provider-crate constant), so giving it the same shape needs a new secret type, a parse tool, and a
  `BuildkiteClient::with_credential` seam — none of which falls out of this change for free. Left for a
  follow-up task.
- **The gap this task leaves, stated plainly:** `willikins_server::catalog::live_catalog_for_document`
  (and `live_catalog_from_env`) still read `WILLIKINS_GITHUB_TOKEN` from the process environment
  whenever a document names *any* `github.*` tool, even when every such node already binds `token` from
  Doppler. This is required by this task's own "server unchanged" boundary — making the port required,
  or making the default client optional, would have changed `describe`/`plan` output for every existing
  document or the server's own startup behaviour, both out of scope here. So the milestone's stated end
  state ("the only credential outside Doppler is the Doppler token itself") is **not yet reachable**
  through `plan --live`/`apply --live` for a document like Walter's monorepo reference: it would still
  need `WILLIKINS_GITHUB_TOKEN` set, alongside whatever it resolves from Doppler. The one-function
  follow-up: make `insert_github_tools`'s credential lazy (`Option<Http>`/a closure resolved only if a
  node's `read`/`ensure` actually reaches the unbound-port path), erroring at call time rather than at
  catalog-construction time when the environment variable is absent. Recorded as `remaining`, not
  buried here.
- **Host observation, not this task's to fix:** a `cargo test --target-dir target/pi-check` process
  (unrelated to this task; a separate tool's own watcher, judging by its target dir) restarted repeatedly
  over the course of this session, one process per file save in this same working tree, compiling the
  same live source concurrently with every gate run here. One run transiently hit `E0027: pattern does
  not mention field 'archived'` at a line in `repo_ensure.rs` whose destructuring predated this task's
  own `observe` refactor -- a shape that no longer existed in the file on disk at the time. An immediate
  retry with byte-identical source was clean. Recorded rather than chased further: this workspace's own
  host rules already single out concurrent cargo activity against a shared `target/` as a source of
  exactly this kind of ghost failure (a linker "missing .rcgu.o" is the documented sibling case); this
  one used a different `--target-dir` and still produced a transient, non-reproducing compiler error, so
  the hazard is broader than the documented one. Not this task's watcher to stop.
- **Other exhaustive-list tests this change touched, found by running them rather than guessed:**
  `willikins-providers-fake/tests/pure_tools_agree.rs` enumerates every pure tool's own inputs by hand
  (a new case for `github.token.parse`, and its own "N pure tools" self-check moved from 15 to 16);
  `willikins-server/tests/acceptance_13_trusted_directory.rs` and
  `willikins-server/tests/image_contents.rs` each assert the exact sorted list of top-level
  `workflows/*.yaml` filenames the trusted directory (respectively the shipped container image) holds,
  both needing `github-repo-token-from-doppler` inserted in its sorted position (between
  `doppler-project` and `new-rust-service-buildkite`) and the fixed count in each test's own name and
  message corrected (fourteen to fifteen). `willikins-cli`'s own multi-document sweeps
  (`acceptance_11_mcp_parity.rs`) compute their document count and read-rate dynamically from the
  directory rather than hard-coding either, so they needed no change — confirmed by running them, not
  assumed from reading the code.
- **Scoped gates green, both commits:** `cargo fmt --all --check`; `cargo clippy` on every touched
  crate (`willikins-types`, `willikins-tools`, `willikins-providers-http`, `willikins-providers-github`,
  `willikins-providers-fake`, `willikins-server`, `willikins-core`, `willikins-cli`) `--all-targets -D
  warnings`; `cargo test` over `willikins-types`, `willikins-tools`, `willikins-providers-http`,
  `willikins-providers-github` (run four times before it stayed green — see the host-observation bullet
  above for the first, transient failure; the rest were real, fixed test-first: an empty mock body that
  should have been `{}`, and a stale doc comment claiming a mock returns 404 when it is fixed at 200),
  `willikins-providers-fake`, `willikins-core`, `willikins-server`, `willikins-providers-doppler`'s
  `live_catalog` binary, and `willikins-cli` — every suite green. Insta snapshots accepted additively and
  reviewed diff-by-diff: github `catalog_parity`'s three tool specs (`+token` port each), `willikins-tools`'
  `catalog_specs_snapshot` (+1 tool), `willikins-providers-fake`'s `catalog_json_snapshot` (three `+token`
  ports, +1 tool), `willikins-types`' own `catalog_json_snapshot` (+1 registered type). The server's MCP
  tool-list/schema snapshot (`mcp_server.rs`) did **not** move — it snapshots the eight MCP meta-tools
  (`plan`, `apply`, `describe`, ...), not the provider catalog, so it was never going to be touched by
  this change; checked, not assumed. `cargo test -p willikins-dsl --test acceptance` — the
  characterization snapshot gained exactly the one new document's own block, verified by diff (see
  above). The full workspace gate was not run (host rule; the coordinator's).

**Addendum:** 2026-09-29 (R3, base configs checked at plan time) — **a new gate,
`doppler.config.inheritable.gate`, closes the dry run's carried-forward finding**: a base config
`doppler.config.inherits.ensure` is about to name that does not exist, or exists but is not marked
inheritable, now blocks the plan instead of failing an apply mid-run after earlier nodes have
already written.

- **Gate, not a list-shaped check on the `.ensure` node itself, and why.** `doppler.config.inherits.ensure`'s
  own `inherits` port is `pure: false` (it writes), so decision (j)'s `Gate` cannot live on that tool
  at all (`Catalog::insert`'s `validate_gate` refuses a gate whose spec is not pure). A separate,
  pure tool is the only shape decision (j) allows; that tool's own subject is a **single**
  `config: DopplerConfig`, not the whole `list<DopplerConfig>` a document's `base_configs` input
  carries, because `need`/`how` are `&'static str` (a gate never authors a string from its inputs)
  and a list subject can only ever render the *whole* list back at a blocked report, never say which
  entry is the actual problem — one gate per base config, `for_each`-expanded over a document's own
  list, is the only shape that can point at the offender. `doppler.config.inheritable.gate` mirrors
  `appstore.app_group.gate`'s own shape exactly: `config` in, `config` passed through as its own
  output (decision (j) point 2, "a gate passes through the key it checked"), `subject: &["config"]`.
- **Missing and non-inheritable both read `Absent`, never `Mismatch`.** The gate's own `observe` is
  `crate::tools::DopplerConfigInheritableEnsure::observe` field for field (same `get_config` call,
  same `inheritable == Some(true)` test, same `looks_like_a_missing_project` tolerance for a missing
  parent project or config) with one difference in what the two readings *mean*: that tool's
  `Absent` means "`ensure` will `POST` it inheritable"; this gate's `Absent` means "the operator must
  make it true", because a shared base config living in another project (App Store Connect's,
  GitHub's, `open-telemetry`'s) is not this document's to flip inheritable out from under whoever
  else depends on its current state — the same posture `doppler.config.inherits.ensure`'s own module
  doc already takes toward an unexpected *extra* inherited entry it will not silently drop. The
  first draft of this task read "exists but not inheritable" as `Mismatch` (a hard `PlanError`,
  never reachable through a re-run), on the theory that a boolean already sitting at the wrong value
  is "something wrong" rather than "something missing" — but `appstore.app_group.gate`'s own
  precedent already settles this the other way: "registered but `APP_GROUPS` not enabled" is
  `Absent`, a thing the operator has not yet made true, not an ownership conflict. A gate's only
  route to a hard `PlanError` is a genuine provider failure (a non-2xx this crate does not otherwise
  tolerate, or a transport error): `read_propagates_a_genuine_provider_failure_rather_than_blocking`
  proves a mocked `500` returns `Err`, distinct from every `Absent` shape this gate otherwise
  tolerates (missing entirely, `404`; visible to no project this token can see, `400`; exists with
  `inheritable: false`; exists with the field never mentioned at all — `config_get_present.json`'s
  own omission, the same fixture `doppler.config.inheritable.ensure`'s own tests already read this
  way) and the one `Present` shape (`inheritable: true`).
- **Registered end to end, not designed in isolation.** `willikins-providers-doppler` (the live tool,
  `crates/willikins-providers-doppler/src/tools/config_inheritable_gate.rs`) and
  `willikins-providers-fake` (the fake twin,
  `crates/willikins-providers-fake/src/tools/doppler_config_inheritable_gate.rs`) both gained it,
  `catalog_parity.rs` pins their `ToolSpec`s equal (a new insta snapshot,
  `catalog_parity__doppler_config_inheritable_gate_spec.snap`), and `willikins-server`'s live catalog
  gained it too: `LIVE_TOOL_NAMES` 31 → 32, `DOPPLER_TOOL_NAMES` 10 → 11 (it needs the Doppler
  credential, the same reason `doppler.secret.get` — also pure — sits there and not with the nine
  provider-independent pure tools), `insert_doppler_tools` inserts it alongside
  `doppler.config.inheritable.ensure`. `willikins-providers-fake`'s own catalog grew from 33 tools to
  34 (`catalog_registers_every_fake_tool`'s exhaustive list and count, `catalog_json_snapshot`), and
  `pure_tools_agree.rs` gained a case (seeding both `with_doppler_config` and
  `with_doppler_config_inheritable` so its `read` answers `Present`, the shape that test requires of
  every pure tool it lists).
- **Mock and fake tests, present/missing/not-inheritable, as asked.** The live tool's own
  `#[cfg(test)]` module (mock-server, `willikins_providers_http::testing::MockProvider`, exact query
  string pinned on every case per the HANDOFF's own lesson) proves: present (`inheritable: true`);
  absent when the field is `false`; absent when the field is never mentioned; absent on a `404`;
  absent on the `400` "does not have access" shape; a genuine `500` propagates as an `Err` rather
  than blocking; `Present` passes `config` through unchanged; `ensure` never issues a second (`POST`)
  request and never reports `changed`. Every case reuses this crate's own already-verified fixtures
  (`config_get_inheritable_true.json`, `config_get_inheritable_false.json`, `config_get_present.json`,
  `error_404.json`, `error_400_no_access.json`, `error_5xx.json`) — no new fixture was needed. The
  fake twin's own tests prove the same three shapes (does not exist; exists but not inheritable;
  exists and inheritable) purely against `FakeState`'s existing `doppler_configs` and
  `doppler_config_inheritable` sets, with no change to `FakeState` itself.
- **Not wired into `workflows/walter-ios-app.yaml` — deliberately, not an oversight.** This task's
  own boundary says the characterization snapshot of every document's `check` and `plan` may change
  only by the addition of new documents; inserting a `doppler.config.inheritable.gate` node ahead of
  `inherit` and rebinding `inherit.inherits` from it would edit an *existing* document's plan output
  (new node, a rebound edge), which that boundary forbids. So Walter's own `inherit` node still binds
  `inherits: ${{ inputs.base_configs }}` directly and still plans `Create` without this check — the
  fix exists and is proven at the tool level, but the document that motivated it is not yet using it.
  Recorded as `remaining`, for whoever wires it: one gate node per entry of `base_configs`
  (`for_each: ${{ inputs.base_configs }}`, `config: ${{ item }}`), and `inherit.inherits` rebound to
  the aggregated `${{ steps.<gate>.config }}` rather than `${{ inputs.base_configs }}` directly — the
  `for_each`-gate aggregate-to-list path decision (j) point 3 describes (`aggregate_for_each_port`
  over a `for_each` node's own `Skipped` instances) is designed but, as far as this task found, not
  yet exercised by any shipped document; proving it belongs to that follow-up, not to this one.
- **Scoped gates green:** `cargo fmt --all --check`; `cargo clippy -p willikins-providers-doppler -p
  willikins-providers-fake -p willikins-server --all-targets -j 2 -- -D warnings`; `cargo test -p
  willikins-providers-doppler -p willikins-providers-fake -j 2` (RUST_TEST_THREADS=2; two insta
  snapshots accepted, diffed by hand before accepting — the new tool's own spec snapshot and exactly
  one `+1 tool` entry in the fake catalog's snapshot, both reviewed above); `cargo test -p
  willikins-server -j 2`, all green. The full workspace gate was not run (host rule; the
  coordinator's).

**Addendum:** 2026-09-29 (R4, the Walter document edited onto the real layout) —
**`workflows/walter-ios-app.yaml` now reads its credentials the way R2 and R3 made possible, from
the real workplace's own names, in place — no new tool, no new type.**

- **The App Store Connect credential is no longer a caller-supplied input.** The `config` workflow
  input is gone; `issuer_id_text`, `key_id_text` and `key_base64` now read a literal, fixed base
  config, `appstore-connect/deploy_ios` (one of the real layout's own shared base configs, decision
  2), under the real workplace's own secret names — `APP_STORE_CONNECT_API_KEY_ISSUER_ID`,
  `APP_STORE_CONNECT_API_KEY_ID`, `APP_STORE_CONNECT_API_KEY_BASE64` — replacing this document's
  own former placeholder names (`ASC_API_KEY_ISSUER_ID`/`_ID`/`_BASE64`), which the sandbox
  workplace still uses. The document's own header now says so explicitly: a sandbox `plan`/`apply`
  of this document will fail `NotFound` at `issuer_id_text` until the sandbox workplace is reseeded
  under the real names, or the sandbox stays behind for good and only the real layout is exercised
  going forward — a decision this task leaves to the operator, not one it makes here.
- **The GitHub token is resolved from Doppler, mirroring R2's own proof document exactly.** Two new
  nodes, `gh_token_secret` (`doppler.secret.get` against `github/bande-a-bonnot`, `GH_CLONE_TOKEN`)
  into `gh_token` (`github.token.parse`), bound to `monorepo_ref`'s optional `token` port — the same
  `doppler.secret.get` → `github.token.parse` → token-port chain
  `workflows/github-repo-token-from-doppler.yaml` already proves end to end. `monorepo_ref` (still
  `github.repo.get`, never `.ensure` — the monorepo is referenced, not created) authenticates as the
  project's own Doppler-held credential now, not a bare `WILLIKINS_GITHUB_TOKEN` from the process
  environment; R2's own stated gap still applies verbatim, so that variable must still be set in the
  process for a live run of any `github.*`-naming document, this one included, until the recorded
  follow-up (`insert_github_tools`'s lazy credential) lands.
- **`base_configs`'s own default is now the real, hyphenated spelling.** `github/bande_a_bonnot` →
  `github/bande-a-bonnot`, settled by R1's grammar widening — this document's own header used to
  carry a "VERIFY ITEM 12" note guessing at the underscored spelling; that note is now replaced with
  one recording the settled answer, not still asking the question.
- **Everything else is unchanged, deliberately: the monorepo, the three bundle identifiers, both
  capabilities and the app record, both app-group gates, the three profiles, and R3's own
  `doppler.config.inheritable.gate` (still not wired in here — R3's own addendum already recorded
  that as its follow-up, not this task's).**
- **Test-first, both fixtures and tests updated to match.** `crates/willikins-cli/tests/walter_document.rs`'s
  `base_inputs()` no longer supplies `config`; its `seeded_state()` now seeds the three App Store
  Connect secrets under `appstore-connect/deploy_ios`'s real names and a `GH_CLONE_TOKEN` secret
  under `github/bande-a-bonnot`; `assert_no_secret_leaked` gained a check that the seeded GitHub
  token's raw value (`ghp_example`) never reaches JSON output, proven across all three of that test's
  own runs. `crates/willikins-cli/tests/walter_apply_blocked_redaction.rs` dropped its now-removed
  `config=` CLI input and updated `base_configs=` to the hyphenated spelling, and gained the same
  `ghp_example` non-leak assertion across stdout, stderr and the journal file. Both were run red
  first (removing the `--input config=...` line against the *old* document failed with "1 missing
  input(s)", confirming the harness actually exercises the document rather than trivially passing),
  then green after the document was edited. `workflows/fixtures/state/walter-ios-app.json` — the CLI
  test's own fixture, read directly through `--fake-state` — moved its two `doppler_values` keys and
  one `doppler_secrets` key onto `appstore-connect/deploy_ios`'s real names, and gained a
  `github/bande-a-bonnot#GH_CLONE_TOKEN` entry.
- **The characterization snapshot moved by exactly this document's own entry, reviewed diff-by-diff**
  (`diff` against the pre-change snapshot, insta's own stripped `assertion_line` header aside): three
  new `TYPES` lines for `gh_token_secret`/`gh_token`, one new line for `monorepo_ref.token:
  GitHubToken`, and the synthesized-input `PLAN ERROR` line's secret path moving from
  `third-thoughts/prd#ASC_API_KEY_ISSUER_ID` to `appstore-connect/deploy_ios#APP_STORE_CONNECT_API_KEY_ISSUER_ID`
  — nothing else in the file changed. This is the one document this task's own boundary allows that
  snapshot to move for; every other document's entry is untouched.
- **No provider call of any kind was made for this task** — every check above ran against the fake
  catalog or the empty catalog, never a live account, never the sandbox.
- **Scoped gates green:** `cargo fmt --all --check`; `cargo clippy -p willikins-cli -p willikins-dsl
  --all-targets -j 2 -- -D warnings`; `cargo test -p willikins-cli -p willikins-dsl -j 2`
  (`RUST_TEST_THREADS=2`; every suite `0 failed`, the one new snapshot accepted and reviewed above);
  `cargo check -p willikins-types -j 2`. The full workspace gate was not run (host rule; the
  coordinator's).

**Addendum:** 2026-09-29 (attacker, pass 4 over R1–R4) — **three coverage gaps closed test-first; the
headline finding is that Walter still does not stop on a missing base config.** Record:
`docs/research/2026-09-29-m3e-adversarial-pass-4.md`. Ten mutations, each restored and `cmp`'d
byte-identical, no provider called.

- **Closed, `e00d8f3`.** A Doppler config name is interpolated unescaped into every Doppler query string,
  so `DopplerConfigName`'s grammar is the only guard against a smuggled parameter. Widening it (and
  `DopplerConfig`'s combined pattern) to also admit `& = ? # . / % + ;`, space and newline left all 419
  lib tests green; two new tests refuse each of them.
- **Closed, `72659fe`.** R2's bound-token redaction proofs drive `github.repo.ensure` only. Writing the bound
  token into `github.repo.get`'s error — the one GitHub node Walter binds `token` on — left the whole crate
  green. The new test reads `github.repo.get` with a marker token bound against a 404, a 401 and a 500.
- **Closed, `4a3d0dd`.** Every Walter test supplies `base_configs` itself and seeds under the names the
  document reads, so reverting the default to `github/bande_a_bonnot` was caught by nothing, and dropping
  `monorepo_ref`'s `token` binding only by the characterization snapshot.
  `the_document_reads_the_real_layout_by_name` pins the real configs, secret names, the
  `github/bande-a-bonnot#GH_CLONE_TOKEN` → `github.token.parse` → `token` chain on every GitHub node, and the
  default.
- **Proven sound:** the unbound port falls back to the default credential (both directions killed, the
  unbound one only by a unit test and one redaction test, not by any tool mock); the gate's
  omitted-field and genuine-failure readings; every existing document's characterization entry; and
  fingerprints, which hash outputs only, never a `ToolSpec`, so the new optional port cannot move one.
- **Open, for the coordinator, in order:** (1) wire R3's gate into Walter — today `walter_document.rs`'s
  fake state seeds no base config at all, `inherit` is never blocked, and run 3 applies clean, so a real apply
  would create project `walter` and its configs before `inherit` fails; (2) R2's server gap
  (`WILLIKINS_GITHUB_TOKEN` still read for any `github.*` document); (3) `base_configs`, `org`, `slug` and
  `monorepo` are still caller-overridable inputs, against "the names are the policy"; (4) every other App
  Store Connect document and the sandbox still use `ASC_API_KEY_*`. Verify items: `GH_CLONE_TOKEN` must begin
  `ghp_` or `github_pat_` (the only prefixes `GitHubToken` accepts); whether Doppler refuses a config name
  that is `-` or starts, ends or doubles a hyphen, which the grammar now admits.

**Addendum:** 2026-09-29 (K1, Buildkite credentials as ports) — **`willikins-providers-buildkite`'s
two tools gain an optional, secret-typed `token` port ([`BuildkiteToken`](crates/willikins-types/src/buildkite.rs)),
a `buildkite.token.parse` tool mirroring `github.token.parse` (R2), closing the gap R2's own addendum
recorded ("Buildkite: not free, left as is... Left for a follow-up task"). Every existing document and
the server keep working unchanged: the port is optional, so an unbound `token` does exactly what it
always did (falls back to the client built once from `WILLIKINS_BUILDKITE_TOKEN` at
catalog-construction time).**

- **`BuildkiteToken`** (`crates/willikins-types/src/buildkite.rs`): secret, derived
  (`#[domain(secret, ...)]`), pattern mirrors `willikins_providers_buildkite::CREDENTIAL_PATTERN`
  exactly, including its `{20,}` floor (unlike `GitHubToken`, whose own provider-crate pattern has no
  floor to mirror). That floor makes a real value of this type exactly what
  `secret_literal_guard.rs`'s `BUILDKITE_TOKEN` pattern looks for, so -- like
  `DopplerServiceToken` -- this type's own `example` is `concat!`-assembled rather than a plain string
  literal (the derive's `example` key accepts any constant expression for exactly this reason).
  Registered in `domain_types!`; the catalog snapshot moved by exactly one new entry, reviewed
  diff-by-diff.
- **`BuildkiteToken::reveal_for_authorization`**, the fifth token-less exception (after `OpaqueSecret`'s
  and `DopplerSecretValue`'s `reveal_for_transform`, `AppleSigningKey`'s `reveal_for_signing`, and
  `GitHubToken`'s own `reveal_for_authorization`): scoped exactly as narrowly as `GitHubToken`'s --
  building one outbound `Authorization` header inside `buildkite.cluster.get`/`buildkite.pipeline.ensure`'s
  own `Tool::read`, which never receives a `SinkToken`, when a document binds their optional `token`
  port. Named in `clippy.toml`'s `disallowed-methods` reason and in
  `crates/willikins-core/tests/expose_secret_guard.rs`'s `willikins-types` exemption list
  (`buildkite.rs`, `reveal_for_authorization`).
- **`buildkite.token.parse`** (`crates/willikins-tools`): pure, `AnySecret` input, mirrors
  `github.token.parse` line for line. Registered in `willikins_tools::register`, the fake catalog, and
  the live catalog's `insert_pure_tools` (credential-independent, always present); `LIVE_TOOL_NAMES`
  32 → 33.
- **`ScopedClient`** (`crates/willikins-providers-buildkite/src/client.rs`): `Default(&BuildkiteClient)`
  or `Bound(BuildkiteClient)`, `Deref`s to `BuildkiteClient`, built the same way
  `willikins-providers-github`'s own `ScopedClient` is, over `BuildkiteClient::with_credential` (in
  turn `willikins_providers_http::Http::with_credential`, the general seam R2 introduced): a bound
  token still reaches whatever `base_url` the tool's own default client was built against (a mock
  server in a test, the real API in production), never `BUILDKITE_API_BASE_URL` unconditionally --
  proven directly by `client_for_token_preserves_the_default_clients_base_url`, so this task did not
  need to rediscover R2's own "real defect caught before commit" the hard way. `buildkite.pipeline.ensure`'s
  `observe` moved from a `&self` method to a static function taking `&BuildkiteClient`, so both `read`
  and `ensure` can pass whichever `ScopedClient` the request's own `token` binding implies.
- **Fake twins, catalogue parity, fake/live agreement:** both fake Buildkite tools
  (`willikins-providers-fake`) gained the identical `token` port for spec parity (`catalog_parity.rs`
  continues to pin fake and live specs byte-for-byte equal; both ToolSpec snapshots moved additively).
  `fake_agrees_with_live.rs` gained `pipeline_ensure_agrees_on_present_with_the_token_port_bound` and
  `cluster_get_agrees_on_a_single_match_with_the_token_port_bound`. The fake tools never inspect the
  port's value, the correct behaviour for a fake.
- **Redaction, proven both ways:** `redaction.rs` gained a second marker, `PORT_TOKEN_MARKER`
  (`concat!`-assembled), and two tests -- `a_bound_token_port_marker_reaches_no_header_but_authorization`
  and `a_bound_token_port_marker_reaches_no_observation_or_error` -- proving a bound token's marker
  reaches no recorded request field but `Authorization`, and no `Observation` or `ToolError` either,
  across both a `404` and a genuinely failing `500`.
- **The gap R2's own adversarial pass left open, closed here proactively.** Pass 4 (finding 3) found
  that no tool-level *mock* test pinned which credential the *unbound* path actually used -- a mutation
  that made the unbound path authorize with a fixed, bogus credential instead of the tool's own default
  survived all three of GitHub's tool-level mock suites, killed only by a unit test and one `redaction.rs`
  test. Both Buildkite mock suites (`cluster_get_mock.rs`, `pipeline_ensure_mock.rs`) gain
  `unbound_read_authorizes_with_the_tools_own_default_credential`: builds the tool's default client
  from a distinctive credential, calls `read` with no `token` port bound, and asserts the captured
  `Authorization` header equals *exactly* that credential -- not a generic, interchangeable test token.
  Both mock suites also gain `read_authorizes_with_the_bound_token_port_not_the_default_credential` and
  `a_bound_token_port_is_used_even_when_the_default_credential_would_be_refused` (the latter's default
  credential is refused outright by the mock, so the test can only pass if the bound token authorized
  the request), mirroring `github.repo.get`'s own `repo_get_mock.rs` pair.
- **A new document, an addition, not a change:** `workflows/buildkite-cluster-token-from-doppler.yaml`
  resolves a Buildkite API access token from Doppler through `buildkite.token.parse` and binds it to
  `buildkite.cluster.get`'s `token` port, mirroring `github-repo-token-from-doppler.yaml` (R2) exactly
  (`buildkite.cluster.get`, not `.pipeline.ensure`, for the same reason: it is pure, so `plan` resolves
  the whole document without approval). Its own document test
  (`crates/willikins-providers-buildkite/tests/buildkite_token_documents.rs`) plans it end to end
  against seeded fake state (`workflows/fixtures/state/buildkite-token.json`) and asserts the whole
  serialized `Plan` never carries the seeded token's raw value -- the task's own "never reaches a
  plan" claim, proven directly rather than only inherited from `render()`. The "error" claim is
  `redaction.rs`'s two bound-token tests (above); the "journal"/"log" claims are inherited from the
  same rendering path Walter's own CLI-level test (`walter_apply_blocked_redaction.rs`) already proves
  for a bound secret port in general -- Walter binds no Buildkite token, so nothing here re-proves it
  at that level, and this is stated rather than left to look like it was. The fixture's own seeded
  secret is the placeholder `BUILDKITE_TOKEN_PLACEHOLDER`, substituted for a `concat!`-assembled real
  token at test run time, the same technique
  `willikins-providers-doppler/fixtures/doppler/README.md` documents for
  `service_token_post_created.json`'s `key`. `crates/willikins-dsl/tests/acceptance.rs`'s
  characterization sweep picks the new document up automatically; its snapshot gained exactly one
  `=== workflows/buildkite-cluster-token-from-doppler.yaml ===` block and nothing else -- confirmed by
  diffing the pre-change snapshot against the accepted one (filtering the insta-metadata line): one
  new block, zero lines changed anywhere else. `acceptance_13_trusted_directory.rs`'s two document-list
  assertions and `image_contents.rs`'s exhaustive `COPY workflows/*.yaml` set both gained the new
  filename in sorted position (fifteen to sixteen positive documents); both green.
- **A real, committed guard violation, found by the advisor, not by re-running the guard.** Four of
  the bound-token-port mock/agreement tests split their `concat!("Bearer ", ...)` at the wrong seam,
  leaving the whole test token in the *second* argument alone: `bkua_` immediately followed by a
  contiguous 27-character alphanumeric run, exactly what `secret_literal_guard.rs`'s
  `BUILDKITE_TOKEN` pattern (`{20,}` after any of nine prefixes) exists to catch, and it does --
  `no_provider_token_shaped_literal_anywhere_in_the_tree` fails on the committed tree. `willikins-core`
  was re-run once after commit 1 (before any bound-token test existed) and not again before commit 2,
  so this shipped. Fixed by moving the split to `concat!("Bearer bkua_", "theboundtokenexampleexample")`
  -- the same seam the type's own `example` and every other marker in this task already used -- in all
  four call sites (`cluster_get_mock.rs` x2, `pipeline_ensure_mock.rs` x2); re-verified against the
  guard's own regex by hand (nine-prefix alternation, `{20,}` floor) before re-running, not only by the
  test passing. **Lesson for the next task in this pattern: re-run `willikins-core --test
  secret_literal_guard` after every new test file that mints a credential-shaped test value, not only
  after the type and its own `example` are added.**
- **`willikins-providers-doppler/tests/live_catalog.rs` hardcodes `LIVE_TOOL_NAMES`'s own array length**
  (`const LIVE_TOOL_NAMES: [&str; N] = willikins_server::LIVE_TOOL_NAMES;`) and was not touched by the
  server-side count bump (32 → 33): this is a compile error, not a runtime failure, so
  `cargo test -p willikins-providers-doppler --test live_catalog` would not even build until fixed.
  Found by the advisor (R2's own `ad8c190` touched this exact file for the same reason when
  `github.token.parse` joined the live catalog, and this task had not grepped for it). Fixed: `32` →
  `33`, plus the module doc's stale tool count and provider list corrected in the same edit. Green
  after the fix.
- **No provider call of any kind was made for this task** -- every check ran against the fake catalog,
  the empty catalog, or a mock server that never leaves the process.
- **Scoped gates green, all three commits:** `cargo fmt --all --check`; `cargo clippy` on every touched
  crate (`willikins-types`, `willikins-tools`, `willikins-core`, `willikins-providers-buildkite`,
  `willikins-providers-fake`, `willikins-server`, `willikins-providers-doppler`) `--all-targets -D
  warnings`; `cargo test` over `willikins-types`, `willikins-core` (including
  `secret_literal_guard`/`expose_secret_guard`, re-run after the fix above), `willikins-tools`,
  `willikins-providers-buildkite` (including the new `buildkite_token_documents.rs`),
  `willikins-providers-fake`, `willikins-server` (including the two updated exhaustive document-list
  tests), `willikins-providers-doppler --test live_catalog` (re-run after the fix above),
  `willikins-providers-github` (regression check: untouched, still green), `willikins-cli --test
  acceptance_m3a_buildkite`, and `willikins-dsl --test acceptance` -- every suite green
  (`RUST_TEST_THREADS=2`). Insta snapshots accepted additively and reviewed diff-by-diff: buildkite
  `catalog_parity`'s two tool specs (`+token` port each), `willikins-tools`' `catalog_specs_snapshot`
  (+1 tool), `willikins-providers-fake`'s `catalog_json_snapshot` (two `+token` ports, +1 tool),
  `willikins-types`' own `catalog_json_snapshot` (+1 registered type), `willikins-dsl`'s
  characterization snapshot (+1 document block, diffed line by line). `cargo check -p willikins-types
  -j 2`. The full workspace gate was not run (host rule; the coordinator's).
- **Remaining, carried forward, unchanged by this task:** the same server-side gap R2 recorded still
  applies to Buildkite too -- `live_catalog_for_document`/`live_catalog_from_env` still read
  `WILLIKINS_BUILDKITE_TOKEN` from the process environment whenever a document names any
  `buildkite.*` tool, even when every such node already binds `token` from Doppler (the same
  `insert_*_tools`-credential-lazy follow-up R2 named would need to cover both providers at once).

**Addendum:** 2026-09-30 (L1, provider credentials required only where a port is unbound) --
**closes the gap R2 and K1 both recorded: `live_catalog_for_document` now demands
`WILLIKINS_GITHUB_TOKEN`/`WILLIKINS_BUILDKITE_TOKEN` only when at least one `github.*`/`buildkite.*`
node in the document being planned actually leaves its own `token` port unbound. A document whose
every such node binds it (`workflows/github-repo-token-from-doppler.yaml`,
`workflows/buildkite-cluster-token-from-doppler.yaml`) needs neither -- the operator's own rule,
"every provider credential a document needs is resolved from Doppler through the document", now holds
for GitHub and Buildkite exactly as it already did for App Store Connect.**

- **The rule, precisely.** `willikins_server::catalog` gains `credential_port(Provider) ->
  Option<PortName>`: `Some(port("token"))` for `Provider::GitHub` and `Provider::Buildkite` (the port
  R2 and K1 gave their tools), `None` for `Provider::Doppler` (its token is the root of the whole
  credential chain -- no document can ever bind it, by design) and `Provider::SigNoz` (never grew
  one). `first_unbound_node_for(&Workflow, Provider)` walks a document's nodes in declaration order
  and returns the first one belonging to `provider` that still needs the *environment* credential: for
  `None` ports, that is simply the first node using that provider at all (`first_tool_for`'s own old
  meaning, so Doppler and `SigNoz` are completely unchanged); for `Some(port)`, it is the first node
  whose `with` does not contain that key -- exactly the same "the node did not bind it, so the key is
  simply absent" fact `plan`'s own `bind_ports` already keys off, not a new analysis invented here.
  `first_tool_for` itself is untouched and keeps its original job (gating *insertion*: "does this
  document call this provider's tools at all"); `first_unbound_node_for` is the new, narrower gate on
  the *credential*. `DocumentCredentialError` gained a `node: NodeName` field (Display: "... (needed
  because `<document>`'s node `<node>` uses `<tool>`)"), so a refusal now names the exact node that
  still needs the credential, not merely the first node of that provider in the document.
- **Extends, not replaces, the existing "live needs only what it uses" mechanism** (`willikins-cli`'s
  operator-reported wart, closed earlier this milestone): `live_catalog_for_document` is still the one
  and only per-document catalog builder, still reached from the same two CLI call sites
  (`build_catalog_for_document`, used by `plan <file>`/`apply <file> --live`), and the fake/live
  catalog split is untouched.
- **When a provider's tools are used but its credential is not required**, they must still be
  inserted into the catalog (a document's own nodes must resolve at `check`/`plan`), built against a
  client that holds *no* environment credential at all. That is new production surface in
  `willikins-providers-http`, not a placeholder value: `Http` gained `credential: Option<Credential>`
  and a new constructor, `Http::without_credential(base_url, headers, missing_var)`. The one choke
  point every public request method (`get`/`put`/`patch`/`put_empty`/`post`/`delete`/
  `delete_with_body`) routes through, `run_retrying`, refuses locally -- a `ProviderError` naming
  `missing_var`, `status: None` -- **before building a URL, opening a connection, or sending
  anything**, whenever `credential` is `None`; `apply_credential` then only ever runs once that has
  already been ruled out, so its `.expect()` is genuinely unreachable, not defensive-by-hope. This was
  the deliberate design choice over a placeholder/empty credential (considered and rejected): if the
  "every node binds its own port" analysis above were ever wrong, a placeholder would send an
  unauthenticated request to the real provider API -- a live network call as the side effect of an
  analysis bug, which this workspace's own boundaries never allow. Refusing locally is the only safe
  default, and it is proven, not assumed:
  `without_credential_refuses_get_before_touching_the_network`/`..._refuses_post_before_touching_the_network`
  build a mock server with `.expect(0)` on the only registered mock and assert it *stays* at zero
  hits. `Http::with_credential` on a credential-less client still works exactly as before (`Some`
  replaces `None`), proven by `with_credential_on_a_credentialless_client_reaches_the_network_normally`
  -- the same swap a bound `token` port's `ScopedClient::Bound` (`client_for_token` in both provider
  crates) already performs, so the R2/K1 mechanism composes with this change for free; no tool-level
  code in either provider crate changed at all.
  `willikins_providers_github::http_client_without_credential()` and
  `willikins_providers_buildkite::http_client_without_credential()` are the two new, narrow entry
  points `live_catalog_for_document` calls instead of `credential_from_env()` + `http_client()` when
  `first_unbound_node_for` says the credential is not required.
- **What the server now requires at startup, and why it is unchanged.** `willikins-server serve`
  (`run_serve` -> `live_catalog_from_env`) and `willikins apply --plan-id --live`
  (`commands::build_catalog` -> the same function) still unconditionally require
  `WILLIKINS_GITHUB_TOKEN`, `WILLIKINS_DOPPLER_TOKEN`, `WILLIKINS_BUILDKITE_TOKEN`, and the `SigNoz`
  credential plus host, exactly as before this task. Neither reads one document up front to narrow
  against -- a long-lived server resolves whichever document a caller names next, and `apply
  --plan-id` resolves against a whole trusted `--workflows-dir` -- so "refuse before any of them" is
  still the only sound posture there, unrelated to whether any one document in that directory happens
  to bind every port. This task's rule lives entirely in `live_catalog_for_document`, reached only
  from `plan <file> --live` / `apply <file> --live`'s single-document path
  (`build_catalog_for_document`); the trust boundary at startup is untouched by construction, not
  merely by omission -- `live_catalog_from_env` and `run_serve` were not edited.
- **Tests, test-first.** `willikins-providers-http/src/http.rs`: the two `without_credential`
  refusal tests and the one `with_credential`-recovers test above (mockito). `willikins-server/src/
  catalog.rs`: six new unit tests on the pure decision logic --
  `credential_port_is_the_token_port_for_github_and_buildkite_only`;
  `first_unbound_node_for_is_none_when_every_{github,buildkite}_node_binds_token`;
  `first_unbound_node_for_names_the_first_node_that_actually_leaves_it_unbound` (a bound node before an
  unbound one -- the refusal must name the second, not the first);
  `first_unbound_node_for_is_none_for_a_provider_the_document_never_uses`;
  `first_unbound_node_for_still_requires_doppler_regardless_of_any_token_like_binding` (Doppler has no
  credential port at all, so even a node that happens to bind a same-named `token` port is still
  "required" -- proving the rule cannot accidentally exempt a provider that never opted in);
  `first_unbound_node_for_matches_first_tool_for_when_no_node_binds_anything` (the pre-L1 shape is
  unchanged). `willikins-cli/tests/serve_and_live.rs`, subprocess, `env_clear`, no network (the file's
  own standing rule): `plan`/`apply <file> --live` on `workflows/github-repo-token-from-doppler.yaml`
  and `plan <file> --live` on `workflows/buildkite-cluster-token-from-doppler.yaml`, each with only
  `WILLIKINS_DOPPLER_TOKEN` set (never `WILLIKINS_GITHUB_TOKEN`/`WILLIKINS_BUILDKITE_TOKEN`) and no
  `--input`, reach the ordinary missing-input refusal (exit 1, on stdout, before
  `willikins_core::plan` ever calls a tool's `read()`) rather than a config refusal naming GitHub or
  Buildkite (exit 2) -- proving the catalog was built without demanding the now-unnecessary
  credential, the same no-network technique
  `plan_live_on_a_doppler_only_document_needs_only_the_doppler_token` already established. The
  existing `plan_live_on_a_document_using_github_still_refuses_with_only_a_doppler_token_set`
  (`new-rust-service.yaml`, whose `github.repo.ensure` node does *not* bind `token`) is the control,
  unchanged and still green, now also asserting `json["node"] == "repo"` -- proving the rule still
  refuses, naming the right node, exactly when it is needed.
- **A pre-existing, unrelated gate failure found and fixed: a second guard-tripping quote of the same
  literal, in this very file.** `cargo test -p willikins-core --test secret_literal_guard` failed
  `no_provider_token_shaped_literal_anywhere_in_the_tree` on this plan file itself before this
  addendum: the K1 addendum's own illustrative quote of the *wrong*-seam `concat!` split spelled the
  same `bkua_`-prefixed, 27-character contiguous run as one literal, prose describing the mistake it
  fixed -- itself exactly the shape the guard scans the whole tree for, markdown included, and this
  file was clean by `git status` before this addendum's own edit (so this was pre-existing, not
  introduced by this task's code diff). Rewritten above to describe the shape (`bkua_` immediately
  followed by a contiguous 27-character run) rather than spell it, the same fix this bullet almost
  repeated: a first draft of *this very bullet* quoted that literal a second time to describe the
  first violation, which the advisor caught before this addendum was committed -- proof, inside this
  same task, of K1's own recorded lesson ("re-run the guard after every new text that spells a
  credential shape, not only after code"). `cargo test -p willikins-core --test secret_literal_guard`
  is green on this file after both rewrites, verified before this addendum was committed, not assumed.
- **No provider call of any kind was made for this task** -- every check ran against a mock server
  that never leaves the process, an in-memory `Workflow`, or a subprocess with `env_clear` and no
  registered credential for the provider under test.
- **Scoped gates green, two commits:** `cargo fmt --all --check`; `cargo clippy` on every touched
  crate (`willikins-providers-http`, `willikins-providers-github`, `willikins-providers-buildkite`,
  `willikins-server`, `willikins-cli`) `--all-targets -D warnings`; `cargo test` over
  `willikins-providers-http` (including the trybuild `compile_fail` suite -- `Http` still has no
  `Debug`), `willikins-providers-github`, `willikins-providers-buildkite`, `willikins-server` (full
  crate, all suites), `willikins-cli` (full crate, all suites), `willikins-providers-doppler --test
  live_catalog` (regression check: `LIVE_TOOL_NAMES`'s count is unchanged by this task, still green);
  `cargo check -p willikins-types -j 2`. The full workspace gate was not run (host rule; the
  coordinator's).

**Addendum:** 2026-09-30 (D1, the base-config gate wired in; `org`/`slug`/`monorepo` become
literals) -- **closes pass 4's own carried-forward finding 1 ("Walter still does not stop on a
missing base config") and three of its finding 3's four caller-overridable names ("`base_configs`,
`org`, `slug` and `monorepo` are still caller-overridable inputs, against 'the names are the
policy'").**

- **The gate, wired exactly as R3's own "remaining" note prescribed.** `workflows/walter-ios-app.yaml`
  gains `base_config_gate` (`doppler.config.inheritable.gate`, `for_each: ${{ inputs.base_configs }}`,
  `config: ${{ item }}`) between `prd_config` and `inherit`; `inherit.inherits` is rebound from
  `${{ inputs.base_configs }}` to the aggregate `${{ steps.base_config_gate.config }}` -- a `Step`
  binding over a `for_each` gate node (decision (j), point 3), the one shape R3's own addendum
  recorded as designed but not yet exercised by any shipped document. A base config that is missing
  or not yet marked inheritable now blocks its own gate instance at plan time; `inherit` itself, since
  it binds the *aggregate* of all three instances, plans `Action::Skip` rather than ever reaching
  Doppler with an incomplete `inherits` list mid-apply. Nothing else in the graph is held back: the
  three `doppler.secret.set` nodes bind `config` from `prd_config`, never from `inherit`, so a
  blocked base config's own `BlockedGate.holds_back` names exactly `["inherit"]`, and every
  independent node -- the three bundle identifiers, `doppler`, the Buildkite pipeline -- still plans
  and applies for real.
- **`org`, `slug` and `monorepo` are bare literals now, not inputs.** The operator's own words, "Just
  update the doc": each was a scalar `with:` port used at exactly one site (`names.org`, `names.slug`,
  `monorepo_ref.repo`), so each is now the literal value directly (`Bande-a-Bonnot`, `walter`,
  `Bande-a-Bonnot/monorepo`) and the three input declarations are gone. `check`'s own `Checked::types`
  records a literal-bound port's type exactly the same way it records a referenced one (proven by the
  characterization snapshot: `names.org: GitHubOrg` and `monorepo_ref.repo: GitHubRepo` did not move),
  so removing the input changes nothing downstream of those two ports.
- **`base_configs` stays a defaulted input, not a literal -- a document-format limit, not an
  oversight.** Verified directly against the DSL rather than assumed: a step's `with:` value is
  `IndexMap<String, String>` (`crates/willikins-dsl/src/document.rs`), so a YAML sequence is refused
  outright ("with values must be strings or references"), and `for_each` must always be a reference,
  never a literal (`parse_for_each_value`, `crates/willikins-dsl/src/reference.rs`) -- there is no way
  to bind a list literal to a `with:` port at all, and `base_config_gate`'s own `for_each` source must
  be something. `doppler-ios.yaml`'s own header already carries this exact reasoning for its own
  `base_configs`; this document's own input description now says the same thing and cites it. So of
  pass 4's four caller-overridable names, three (`org`, `slug`, `monorepo`) are closed; `base_configs`
  is not closable in this document format and is reported that way, not silently left to look fixed.
- **Test-first, both fixtures and tests updated to match.**
  `crates/willikins-cli/tests/walter_document.rs`: `base_inputs()` no longer supplies `org`/`slug`/
  `monorepo`; `seeded_state()` is now `seeded_state_with_base_configs(present)` seeding
  `doppler_configs`/`doppler_config_inheritable` for whichever base configs are asked present (all
  three for the existing three-run scenario, so the new gate does not change any of its existing
  blocked-set assertions), and a new test,
  `a_missing_base_config_blocks_its_gate_and_skips_inherit`, seeds two of three, plans and applies:
  the missing config's own `base_config_gate` instance is `Blocked`, the other two are `Compute`,
  `inherit` is `Skip` (never `NodeStatus::Blocked` -- it is not itself a gate, only downstream of
  one), its `BlockedGate.holds_back` is exactly `["inherit"]`, and every independent node (`app_id`,
  `nse_id`, `widgets_id`, `doppler`, `pipeline`) still plans `Create` and applies `Created`; a
  `Blocked` run is still `Ok`, not an error, and no secret leaks into either the plan or applied JSON.
  `the_document_reads_the_real_layout_by_name` (pass 4's own literal-pinning test) gained assertions
  that `org`/`slug`/`monorepo` are no longer declared inputs, that `names.org`/`names.slug`/
  `monorepo_ref.repo` are the exact literals above, that `base_config_gate` calls
  `doppler.config.inheritable.gate` and expands over `${{ inputs.base_configs }}` with `config: ${{
  item }}`, and that `inherit.inherits` binds `base_config_gate`'s own aggregate, never
  `inputs.base_configs` directly. `crates/willikins-cli/tests/walter_apply_blocked_redaction.rs`
  dropped its now-stale `--input org=…`/`slug=…`/`monorepo=…` lines -- **verified, not assumed, that
  this is cleanup and not a correctness fix**: `build_partial_inputs`
  (`crates/willikins-cli/src/main.rs`) passes an argument naming an input `checked`'s workflow does
  not declare through as a scalar rather than refusing it (its own doc comment: "since
  `willikins_core::describe` only needs its name to report it as unrecognised"), so the three stale
  lines would have sat in `PartialInputs` unused and harmless, never causing a refusal. Removing them
  keeps the invocation honest about what the document now reads, nothing more. `workflows/fixtures/state/walter-ios-app.json` gained `doppler_configs`/
  `doppler_config_inheritable` entries for all three base configs, so the CLI-level test's own run
  clears the new gate exactly as it did before this task.
- **The characterization snapshot moved by exactly one line, in this document's own entry, diffed
  byte for byte against the pre-change snapshot**: `base_config_gate.config: DopplerConfig`, inserted
  between `prd_config.branch: DopplerConfigName` and `inherit.config: DopplerConfig` (declaration
  order). `inherit.inherits: list<DopplerConfig>` is unchanged -- its recorded type is the port's
  resolved type, not which binding kind produced it. The PLAN section is unaffected: the
  characterization's own `synthesized_inputs` plans against the empty fake catalog, which still fails
  at the very first node, `issuer_id_text` (`NotFound`), before the graph ever reaches
  `base_config_gate`.
- **No provider call of any kind was made for this task** -- every check ran against the fake catalog
  or the empty catalog, never a live account, never the sandbox.
- **Scoped gates green:** `cargo fmt --all --check`; `cargo clippy -p willikins-dsl -p willikins-cli
  -p willikins-core --all-targets -j 2 -- -D warnings`; `cargo test -p willikins-dsl -j 2` (one insta
  snapshot accepted and diffed line-by-line, reviewed above); `cargo test -p willikins-cli -j 2` (17
  suites, 0 failed, including both `walter_document.rs`'s four tests and
  `walter_apply_blocked_redaction.rs`); `cargo test -p willikins-server -j 2` (full crate, all suites,
  regression check: this task touches no `willikins-server` source, run because the crate's own
  document-list tests name `walter-ios-app.yaml`); `cargo test -p willikins-core --test
  secret_literal_guard` and `--test expose_secret_guard` (this addendum's own text scanned clean, per
  K1/L1's own recorded lesson); `cargo check -p willikins-types -j 2`. All green
  (`RUST_TEST_THREADS=2`). The full workspace gate was not run (host rule; the coordinator's).
- **Decision (h) is now stale in one clause, not edited in place** (its own header block is not this
  task's to touch): "Names: `naming.v1` for Doppler and Buildkite, the monorepo as an input" -- the
  monorepo is a literal now, not an input; this addendum supersedes that clause.
- **Honest process note: the new test was not run red against the pre-change document.** The gate
  node, the rebound edge and `a_missing_base_config_blocks_its_gate_and_skips_inherit` were written
  together, then run once, green. The one genuine red this task produced was the characterization
  snapshot's own assertion failing on the predicted new `TYPES` line (confirmed byte-for-byte against
  the pre-change snapshot before accepting it) -- real evidence the change reached the document, but
  not the same thing as watching the new test fail against the old graph first.
- **Remaining, carried forward:** `base_configs` itself is still a caller-overridable input by
  mechanical necessity (above) -- not a gap this task can close, only document honestly; R2's own
  server-side gap (`WILLIKINS_GITHUB_TOKEN` read unconditionally at `serve`/`apply --plan-id`
  startup) is untouched, unrelated to this task; the sandbox-versus-real Doppler naming split R4
  recorded is untouched.

**Addendum:** 2026-09-30 (D2, the Buildkite credential resolved from Doppler; `buildkite_org`/
`cluster` become literals) -- **closes the one credential K1/L1 left the real Walter document
still reading from the process environment: `workflows/walter-ios-app.yaml`'s own
`buildkite.cluster.get`/`buildkite.pipeline.ensure` nodes now bind K1's optional `token` port,
resolved from Doppler exactly the way R4's own GitHub chain already is, and `buildkite_org`/
`cluster` are edited in place to the real workplace's own names, "Just update the doc" (D1's own
words) applied to the one pair of caller-overridable names D1 itself did not touch.**

- **The chain, mirroring `gh_token_secret`/`gh_token` exactly.** `bk_token_secret`
  (`doppler.secret.get`, `config: buildkite/prd`, `name: PIPELINE_CREATION_TOKEN` -- the real
  workplace's own secret, per the task brief probed read-only 2026-09-29) into `bk_token`
  (`buildkite.token.parse`), bound to both `buildkite_cluster.token` and `pipeline.token`. Neither
  Buildkite node in this document leaves `token` unbound any more, so -- by L1's own rule,
  unmodified by this task -- `plan`/`apply workflows/walter-ios-app.yaml --live` no longer read
  `WILLIKINS_BUILDKITE_TOKEN` at all; combined with R4's GitHub chain (already bound) and App
  Store Connect's own three-port credential (never an environment variable in the first place),
  this document now needs exactly one environment credential to plan or apply live:
  `WILLIKINS_DOPPLER_TOKEN`.
- **`buildkite_org` and `cluster` are bare literals now, not inputs**, the same treatment D1 gave
  `org`/`slug`/`monorepo`: each was a scalar `with:` port used at exactly one site each on two
  nodes (`buildkite_cluster.org`/`.name`, `pipeline.org`), so each is now the literal value
  directly -- `la-bande-a-bonnot` (the real Buildkite org slug) and `Default cluster` (the real
  org's only cluster), both probed read-only 2026-09-29 and named in this task's own brief. No
  document-format limit applies here the way it does to `base_configs` (D1): both are plain
  scalars, not a list, so nothing stops the literal binding.
- **The fixture is the tightest constraint in this task, not the document.** Any string that
  parses as a `BuildkiteToken` is exactly the shape `secret_literal_guard.rs`'s `BUILDKITE_TOKEN`
  pattern exists to catch (K1's own addendum already recorded this for
  `buildkite-cluster-token-from-doppler.yaml`'s own fixture), and the guard scans every file in
  the tree, JSON included -- so `workflows/fixtures/state/walter-ios-app.json` carries only the
  placeholder `BUILDKITE_TOKEN_PLACEHOLDER` under `buildkite/prd#PIPELINE_CREATION_TOKEN`, never a
  real-shaped literal on disk. `crates/willikins-cli/tests/walter_document.rs` builds its own fake
  state inline (`serde_json::json!`, never reading this file), so it seeds a `concat!`-assembled
  token directly (`SEEDED_BUILDKITE_TOKEN`, same seam K1's own `SEEDED_TOKEN` uses:
  `concat!("bkua_", "...")`), no placeholder needed. `crates/willikins-cli/tests/walter_apply_blocked_redaction.rs`
  is the one consumer that cannot do that -- it hands the fixture's own path straight to the built
  binary's `--fake-state` -- so it now reads the checked-in file, substitutes the placeholder for
  the same `concat!`-assembled token, and writes the result into its own `TempDir` before passing
  *that* path instead; its existing "no secret leaks into stdout/stderr/journal" assertions gained
  a fourth check for this token, alongside the PEM marker, the fake profile's plaintext prefix, and
  the seeded GitHub token. The fixture's `buildkite_clusters` key also moved from
  `ci-macos-apple-silicon` to `Default cluster` -- the document now requests the real literal name,
  and the fake tool ignores `org` for its own lookup (confirmed directly against
  `FakeBuildkiteClusterGet::lookup`, not assumed), so only the name needed to change.
- **Test-first, both graph tests and the CLI-level test extended to match.** `walter_document.rs`:
  `base_inputs()` no longer supplies `buildkite_org`/`cluster`; `CLUSTER` is now `"Default
  cluster"`; `seeded_state_with_base_configs` seeds `buildkite/prd#PIPELINE_CREATION_TOKEN`;
  `assert_no_secret_leaked` gained the new token constant. `the_document_reads_the_real_layout_by_name`
  gained the Buildkite mirror of its own existing GitHub block: `bk_token_secret`'s literals,
  `bk_token`'s binding, every `buildkite.*` node (`buildkite_cluster`, `pipeline`) binding `token`
  from `bk_token.value`, both nodes' `org` literal `la-bande-a-bonnot`, `buildkite_cluster.name`
  literal `Default cluster`, and `buildkite_org`/`cluster` no longer declared inputs.
  `walter_apply_blocked_redaction.rs` dropped its now-stale `--input buildkite_org=…`/`cluster=…`
  lines (the same harmless cleanup D1 made for `org`/`slug`/`monorepo` -- `build_partial_inputs`
  passes an unrecognised `--input` through unused rather than refusing it, so these two lines sat
  unused, never causing a refusal) and gained the placeholder-substitution step and leak assertion
  above.
- **The L1 proof, extended from the two single-tool documents to the real one.** K1/L1's own
  `crates/willikins-cli/tests/serve_and_live.rs` tests proved the rule for
  `github-repo-token-from-doppler.yaml` and `buildkite-cluster-token-from-doppler.yaml` in
  isolation; this task's own brief asks for the same proof over `workflows/walter-ios-app.yaml`
  itself. Two new tests, `plan_live_on_walter_needs_neither_github_nor_buildkite_credential` and
  `apply_live_on_walter_needs_neither_github_nor_buildkite_credential`: subprocess, `env_clear`,
  only `WILLIKINS_DOPPLER_TOKEN` set (never `WILLIKINS_GITHUB_TOKEN`/`WILLIKINS_BUILDKITE_TOKEN`),
  no `--input` -- reach the ordinary missing-input refusal (exit 1, on stdout, `missing
  \`app_identifier\`` for `plan`) rather than a config refusal naming GitHub or Buildkite (exit 2),
  proving the live catalog was built without demanding either credential. Checked by hand against
  the built binary before writing the assertions (`willikins plan workflows/walter-ios-app.yaml
  --live` with a shaped `dp.sa.` token and no other input), not guessed: exit 1, empty stderr,
  first missing input named is `app_identifier` (declaration order); `apply` behaves identically.
  App Store Connect's own tools are inserted unconditionally by `live_catalog_for_document`
  (`insert_appstore_tools`, no credential gate at all -- its credential is three ports, never an
  environment variable), confirmed by reading `willikins-server/src/catalog.rs` before writing the
  test rather than assumed, so nothing about this task touches that path.
- **No provider call of any kind was made for this task** -- every check ran against the fake
  catalog, the empty catalog, or a subprocess with `env_clear` and no registered credential for the
  provider under test.
- **The characterization snapshot moved by exactly five lines, all additive, all inside this
  document's own `TYPES:` block, diffed byte for byte against the pre-change snapshot** (`diff`'s
  own count, not estimated): `bk_token_secret.config`, `bk_token_secret.name`, `bk_token.value`,
  `buildkite_cluster.token`, `pipeline.token`. The snapshot format prints only each port's resolved
  type, never whether it is literal- or input-bound, so `buildkite_cluster.org`/`.name` and
  `pipeline.org` moving from an input reference to a literal changed no line (D1's own
  `names.org`/`monorepo_ref.repo` made the identical point for `org`/`slug`/`monorepo`). Nothing
  outside `=== workflows/walter-ios-app.yaml ===` moved.
- **Scoped gates green:** `cargo fmt --all --check` (one real formatting fix applied by `cargo fmt
  --all` before the check passed -- a wrapped `.get()` chain in the new test assertions);
  `cargo clippy -p willikins-cli -p willikins-dsl -p willikins-core --all-targets -j 2 -- -D
  warnings`; `cargo test -p willikins-cli -j 2` (17 suites, 0 failed, including
  `walter_document.rs`'s four tests, `walter_apply_blocked_redaction.rs`, and `serve_and_live.rs`'s
  sixteen, two of them new); `cargo test -p willikins-dsl -j 2` (one insta snapshot accepted and
  diffed line-by-line, reviewed above); `cargo test -p willikins-server -j 2` (regression check,
  full crate, all suites, 0 failed -- this task touches no `willikins-server` source, run because
  the crate's own document-list tests name `walter-ios-app.yaml`); `cargo test -p willikins-core
  --test secret_literal_guard --test expose_secret_guard` (both green after the fixture and the new
  test files, per K1/L1's own recorded lesson: re-run the guard after any new text that mints a
  credential-shaped value); `cargo check -p willikins-types -j 2`. All green
  (`RUST_TEST_THREADS=2`). The full workspace gate was not run (host rule; the coordinator's).
- **`walter_document.rs`'s own graph-test assertions were not run red against the pre-change
  document first** -- they were written together with the document edit and run once, green,
  except the characterization snapshot, whose own assertion failed on the predicted new `TYPES:`
  lines before being accepted (real evidence the change reached the document). **The two new
  `serve_and_live.rs` tests were, honestly this time (a correction to this addendum's own first
  draft, which wrongly claimed the reverse): the pre-change document was recovered from the parent
  commit (`git show 794bae1:workflows/walter-ios-app.yaml`, since the tracked file was already
  edited by the time the test was written) and run by hand against the built binary --
  `WILLIKINS_DOPPLER_TOKEN` alone, no `--input`, `--live` -- and it refused exit 2, `kind:
  "Buildkite"`, `node: "buildkite_cluster"`, `tool: "buildkite.cluster.get"` (`buildkite_cluster`
  left `token` unbound before this task, so `first_unbound_node_for(Buildkite)` named it and the
  catalog demanded `WILLIKINS_BUILDKITE_TOKEN`). Only after seeing that genuine red was the
  post-change binary run and its exit 1 / `missing \`app_identifier\`` observed, then written into
  the two new tests. The lesson D1's own correction (`794bae1`) already recorded applies here too:
  say what was actually observed, not what the change was intended to produce.
- **Buildkite scope sufficiency, asserted from the endpoint list, not live-tested.** The real
  token's three scopes are `read_pipelines`, `write_pipelines` and `read_clusters` --
  *deliberately* no `read_organizations` (task brief). Read directly against
  `crates/willikins-providers-buildkite/src/client.rs`: `buildkite.cluster.get`'s own `read`/`ensure`
  only ever calls the client's `list_clusters_page` (`GET .../clusters`); `buildkite.pipeline.ensure`'s
  own `observe`/`ensure` only ever call `get_pipeline` (`GET .../pipelines/{slug}`) and
  `create_pipeline` (`POST .../pipelines`) -- confirmed by reading both tool files' own bodies, not
  merely the client's method list. The client also carries a fourth method, `delete_pipeline`
  (`DELETE .../pipelines/{slug}`), but its own module doc says it plainly: "used only by the opt-in
  live write cycle to clean up after itself" -- neither tool calls it, confirmed by grepping for
  the one call site in the whole crate. So every request either Buildkite tool this document uses
  can build is `GET`/`POST` under `/v2/organizations/{org}/pipelines...` or
  `/v2/organizations/{org}/clusters...` (nested resource paths, covered by the three granted
  scopes); neither tool ever calls the bare `GET /v2/organizations`/`GET /v2/organizations/{org}`
  that `read_organizations` alone would gate, and neither ever reaches `DELETE` at all. Not
  live-tested (this task made no provider call of any kind), so this is a static read of the
  client's own request-building code, not a settled fact about Buildkite's own scope enforcement.
- **The fixture's placeholder has exactly one other consumer, checked rather than assumed.**
  `grep -rn "walter-ios-app.json" docs/ workflows/ crates/ todos/` finds only this addendum's own
  prose and `walter_apply_blocked_redaction.rs` (which already substitutes the placeholder before
  use) -- no `HANDOFF.md` line or other document hands the checked-in fixture straight to
  `--fake-state` without that step, so nothing else silently breaks on `bk_token`'s parse failure.
  A future hand run of `--fake-state workflows/fixtures/state/walter-ios-app.json` outside a test
  harness would still hit exactly that failure by design -- worth remembering if one is ever run
  ad hoc.
- **Remaining, carried forward, unchanged by this task:** `base_configs` is still a
  caller-overridable input by mechanical necessity (D1); R2's own server-side gap
  (`WILLIKINS_GITHUB_TOKEN`/`WILLIKINS_BUILDKITE_TOKEN` read unconditionally at `serve`/`apply
  --plan-id` startup, L1's own addendum) is untouched -- a long-lived server or a
  `--workflows-dir` apply still cannot narrow to one document's own bound ports; the
  sandbox-versus-real Doppler naming split R4 recorded is untouched; Buildkite scope sufficiency
  above is a static claim, not live-proven; this document's four
  `operator.acknowledge` leaves and its two observed gates are unaffected by this task.

**Addendum:** 2026-09-30 (attacker, pass 5 over K1, L1, D1 and D2) -- **one coverage gap closed
test-first (`753d1e8`); two findings recorded for the coordinator; seven mutations, six killed by
existing tests and one (M5) killed only by the new test.** Full record:
`docs/research/2026-09-30-m3e-adversarial-pass-5.md`. No live test, no provider call.

- **Confirmed by mutation:** Walter reads exactly `PIPELINE_CREATION_TOKEN` from `buildkite/prd`
  through `doppler.secret.get` (M1). `plan`/`apply <file> --live` on Walter need only
  `WILLIKINS_DOPPLER_TOKEN`, and unbinding one Buildkite node's `token` brings back an exit-2 refusal
  that names `WILLIKINS_BUILDKITE_TOKEN` and that node (M2). `inherit` is held back by the
  base-config gate (M3). The Buildkite org, cluster, GitHub org, slug and monorepo are literals, not
  inputs (M4). A bound token authorizes the request (M6), and an unbound one falls back to the
  tool's own environment-built client (M7). Every mutation was restored with `cmp` 0.
- **F1, fixed (`753d1e8`):** the Buildkite half of L1's "an unbound port still refuses" had no test.
  A mutant that never required `WILLIKINS_BUILDKITE_TOKEN` in `live_catalog_for_document` survived
  `serve_and_live`, `walter_document`, `acceptance_m3a_buildkite` and `willikins-server --lib`.
  `plan_live_on_a_document_leaving_buildkite_unbound_still_refuses_naming_the_variable`
  (`new-rust-service-buildkite.yaml`) was red against that mutant and is green on the tree.
- **D1 qualified:** a missing or non-inheritable base config does **not** stop Walter before any
  write. By decision (j) it holds back `inherit` alone, and every independent writer (bundle
  identifiers, `doppler`, `configs`, `prd_config`, `pipeline`) still plans `create`.
- **F2, recorded:** `base_configs` can still be overridden, and that defeats the gate.
  `--input base_configs=` yields an empty list, so the plan has zero gate instances and `inherit`
  plans `noop` with `inherits: []`, with nothing blocked or reported. A substituted list gates and
  inherits whatever the caller names, provided it exists and is inheritable. Approval is the only
  thing that limits this today. Closing it needs a document-format decision: a list literal for
  `for_each`/`with:`, or a fixed-input marker.
- **F4, recorded:** `apply --plan-id --live` and `serve` still require GitHub, Doppler, Buildkite
  and `SigNoz` (credential and host) credentials. The `SigNoz` key expired on 2026-09-23, and that
  check is shape-only. For the real Walter run, only the one-shot `apply
  workflows/walter-ios-app.yaml --live --approve --journal <path>` is Doppler-only.
  `ApplyArgs::live`'s doc comment says "three" where four are required.
- **Not a leak:** the token was refused at every non-secret port tried, as a literal, as an input,
  and unparsed. It is accepted as a document output (by design) and rendered `[REDACTED
  BuildkiteToken]` in the plan, the apply output and the journal, with the raw run appearing 0 times.
- **Snapshot:** from `37136a2` to `HEAD` the snapshot has 21 insertions and 0 deletions: the new
  document's block, six `TYPES` lines in Walter's own entry, and insta's `assertion_line` metadata.
  No other document's lines moved.
- **Scoped gates green:** `cargo fmt --all --check`; `cargo clippy -p willikins-cli --all-targets
  -j 2 -- -D warnings`; `cargo test -p willikins-cli --test serve_and_live` (17 passed),
  `--test walter_document` (4 passed), `--test walter_apply_blocked_redaction`, `--test acceptance_m3a_buildkite`;
  `cargo test -p willikins-providers-buildkite --test cluster_get_mock --test pipeline_ensure_mock`;
  `cargo test -p willikins-dsl --test acceptance`; `cargo test -p willikins-core --test
  secret_literal_guard`. The full workspace gate was not run (host rule; the coordinator's).

**Addendum:** 2026-09-30 (T3f, Apple's real bundle-id name rule) -- **`AppleBundleIdName` tightened
to what a live probe actually witnessed; the `AppleBundleIdentifier => AppleBundleIdName` conversion
T3b registered is removed as never total; Walter's three bundle ids now name themselves with plain
literals.** Four commits (`f71e405`, `7a87b4a`, `0f15efd`, `4de0b09`).

- **Trigger.** The first real apply of Walter (2026-09-30) found `POST /v1/bundleIds` for the app
  identifier answering `409 ENTITY_ERROR.ATTRIBUTE.INVALID` when `name` was bound to the dotted
  identifier through T3b's conversion -- nothing was created (bundle id count unchanged before and
  after), and every later node in the graph read `NotRun`. Reproduced by the coordinator on
  throwaway identifiers: a `name` identical to its own dotted identifier is refused; a plain
  space-separated name is accepted. `AppleBundleIdName`'s grammar was this crate's own guess (any
  non-control, non-invisible character), so `check` passed a value Apple's own API refuses, and the
  conversion registered against `AppleBundleIdentifier`'s grammar (which admits a dot) was never
  total against Apple's real rule -- exactly the shape CLAUDE.md's conversions invariant forbids.
- **Apple's documentation, fetched verbatim 2026-09-30.** The ASC OpenAPI description still declares
  `name` as a bare `{"type": "string"}` with no `pattern` and no `maxLength`
  (`https://developer.apple.com/documentation/appstoreconnectapi/bundleidcreaterequest/data-data.dictionary/attributes-data.dictionary.md`),
  matching `docs/research/2026-09-16-app-store-connect.md` section 2's earlier finding. The portal
  help page for registering an App ID says only "Enter a name or description for the App ID in the
  Description field" (`https://developer.apple.com/help/account/identifiers/register-an-app-id/`) --
  no character rule at all. The documentation leaves this entirely open; only a live probe could
  settle it.
- **Six live probes, one run, on the operator's real account (never Apple's -- there is no
  sandbox).** Each a raw `POST /v1/bundleIds` on its own throwaway
  `com.willikins.probe.delete-me.<pid>-<time>-<n>` identifier, deleted by its own returned id before
  the next probe ran. Bundle id count: 21 before, 21 after.

  | case | name shape | result |
  |------|------------|--------|
  | dot (plain name, not equal to its identifier) | `Probe.Dot.Name` | 409 `ENTITY_ERROR.ATTRIBUTE.INVALID` "An attribute in the provided entity has invalid value" |
  | hyphen surrounded by spaces | `Probe - Hyphen` | 201 |
  | apostrophe | `Probe's Apostrophe` | 409 `ENTITY_ERROR.ATTRIBUTE.INVALID` (same title) |
  | ampersand | `Probe & Ampersand` | 409 `ENTITY_ERROR.ATTRIBUTE.INVALID` (same title) |
  | digit-leading | `1Probe Digit` | 201 |
  | name equal to its own dotted identifier | `com.willikins.probe.delete-me.<n>` | 409 `ENTITY_ERROR.ATTRIBUTE.INVALID` (same title) |

  The dot case was deliberately split from the "name equals identifier" case: both refuse
  identically, which confirms the dot character itself is refused rather than only that specific
  shape. No other punctuation, no non-ASCII letter, and no name over ~30 characters was probed;
  `AppleBundleIdName`'s grammar admits only what was witnessed (ASCII letters, digits, space,
  hyphen) and refuses everything else, per the witness asymmetry (admitting a character Apple
  refuses is the live-breaking direction; refusing one Apple would have accepted is merely
  conservative and can be loosened later on its own probe).
- **Landed, test-first, in commit order** (each commit's tree checked or tested green before the
  next): the probe test (`crates/willikins-providers-appstore/tests/live_write_cycle.rs`,
  `appstore_live_bundle_id_name_probe`, gated exactly like its siblings: `live-tests` feature,
  `#[ignore]`, `WILLIKINS_LIVE_TESTS=1`) and its live run above; Walter's three `name` ports
  (`app_id`, `nse_id`, `widgets_id`) rebound from `${{ inputs.*_identifier }}` to the literals
  `"Walter"` / `"Walter - NSE"` / `"Walter - Widgets"` (the coordinator's proposal, following the
  account's existing "Foo - Bar" naming habit -- the operator may still change these; profile names
  are untouched, since Apple accepts dots there); the conversion row, its `From` impl, its
  containment-proof doc comment, and its proptest module removed from
  `crates/willikins-types/src/appstore.rs` and `conversion_rows()`, with a new negative fixture
  (`workflows/fixtures/appstore-bundle-id-identifier-into-name.yaml`) and acceptance test
  (`appstore_bundle_id_identifier_into_name_is_rejected` in
  `crates/willikins-providers-appstore/tests/bundle_id_documents.rs`) pinning that `check` refuses an
  `AppleBundleIdentifier` bound to the `name` port exactly as before the row ever existed; then
  `AppleBundleIdName::parse` tightened to the allow-list above (subsuming the old control- and
  invisible/bidi-character checks, since every admitted character is already free of both), with one
  unit test per probed case citing the probe date, a new unit test pinning that a dotted identifier
  is no longer a valid name, and the published JSON schema gaining the matching `pattern`.
- **Snapshots moved exactly as predicted.** `characterization_of_every_document`: the new fixture's
  entry, plus Walter's three `app_id`/`nse_id`/`widgets_id` `.name` edges losing their `->
  AppleBundleIdName` conversion arrow (no `PLAN` line moved -- the characterization's synthesized
  inputs make Walter's `plan` fail at its very first Doppler read before reaching any bundle id node,
  unaffected by this task). `willikins-types`' own catalog snapshot: `AppleBundleIdName`'s schema
  entry gaining `"pattern"`. Nothing else moved in either snapshot.
- **Scoped gates green throughout:** `cargo fmt --all --check`; `cargo clippy -p
  willikins-providers-appstore --features live-tests --all-targets -j 2 -- -D warnings` and `-p
  willikins-types --all-targets -j 2 -- -D warnings`; `cargo test -p willikins-types -p
  willikins-providers-appstore -p willikins-providers-fake -p willikins-cli -p willikins-dsl -p
  willikins-server -j 2` (every suite green, including `walter_document` and
  `characterization_of_every_document` after accepting their snapshots); `cargo check -p
  willikins-types`. The full workspace gate was not run (host rule; the coordinator's).
- **Not done here, left for the coordinator/operator:** the three literal names
  (`"Walter"`/`"Walter - NSE"`/`"Walter - Widgets"`) are a proposal, not a decision -- confirm with
  the operator before a real apply. `AppleBundleIdName`'s allow-list is conservative by
  construction: a future name the operator wants (e.g. a non-ASCII letter) needs its own probe
  before the grammar admits it, not an inference from this addendum. No live test touched an
  existing identifier, app, certificate, profile, or device; the six throwaway identifiers this
  task's probe created are all deleted, and the account's bundle id count is unchanged (21).

**Addendum:** 2026-09-30 (attacker, pass 6 over T3f) -- **T3f holds: the grammar matches every probe
in both directions, the removed row stays removed, and Walter checks and plans with its literal names.
One doc defect fixed (`9fa6512`). One evidence gap recorded that changes what happens before the next
real apply.** Full record: `docs/research/2026-09-30-m3e-adversarial-pass-6.md`. No live test, no
provider call.

- **Confirmed by mutation (all restored, `cmp` 0):** re-admitting `.` in `AppleBundleIdName` (M1)
  and dropping `' '` (M1b) are each killed by T3f's probe-cited unit tests. Re-adding the
  `AppleBundleIdentifier => AppleBundleIdName` row and its `From` (M3) is killed by
  `appstore_bundle_id_identifier_into_name_is_rejected` alone, and `willikins-types`' own suites stay
  green. Walter's `"Walter - NSE"` → `"Walter.NSE"` (M4a) fails `check` with `InvalidLiteral`, and →
  `${{ inputs.nse_identifier }}` (M4b) fails it with `TypeMismatch`. `"Walter"` → `"Barnum"` (M5)
  survives: the literals are an unconfirmed proposal. Pin them in
  `the_document_reads_the_real_layout_by_name` once the operator confirms them.
- **F1, fixed (`9fa6512`):** the `.md` twin that `AppleBundleIdName`'s doc and the T3f addendum cite
  states no type for `name`. The source is the OpenAPI specification zip (version 4.5, re-fetched:
  `name` is a bare string on bundle-id create and update, and on profile create).
- **F2, recorded (doc corrected in `9fa6512`):** `AppleBundleIdentifier => AppleProfileName` is total
  against willikins' `AppleProfileName` grammar (M2, a parse refusing `.`, is killed by the
  grammar-generated proptests). Against Apple, the only dotted profile names seen were created outside
  willikins and outside the monorepo's fastlane, which runs `readonly: true`. Every
  `POST /v1/profiles` willikins made used a name without a dot, and the real Walter apply never
  reached a profile node. **Before the next real Walter apply** (coordinator, live): one throwaway
  `POST /v1/profiles` whose `name` equals its own dotted `com.willikins.probe.delete-me.*` identifier,
  deleted by returned id (`raw_post_profile` already has the shape).
- **F3-F5, recorded:** the `.*` implication proptest survived M2 (it rarely produces an identifier).
  Nothing in `willikins-types` ties a row's `From` to its target's `parse`, so an
  example-round-trip guard over every row is suggested. `AppleBundleIdName` admits unprobed shapes:
  all-space, hyphen-only, leading/trailing/doubled spaces, and lengths over about 32. These are verify
  items for the next probe run.
- **Checked non-issues:** `read` compares Apple's `name` raw and never parses it, so the tightening
  cannot break reading an existing identifier. `=> Text` feeds only willikins' pure tools, so no
  provider rule applies. No document, fixture or fake state binds a dotted value to a bundle-id name.
- **Scoped gates green after `9fa6512`:** `cargo fmt --all --check`; `cargo clippy -p willikins-types
  --all-targets -j 2 -- -D warnings`; `cargo test -p willikins-types`; `cargo check -p
  willikins-types`; `cargo test -p willikins-cli --test walter_document` (4 passed). Both snapshots are
  unmoved. The full workspace gate was not run (host rule; it is the coordinator's).

**Addendum:** 2026-09-30 (the App Attest gate task) — **the operator asked "Could we update the
workflow to enable App Attest on apps by default?" Since App Store Connect's API can only read this
capability, never write it, the honest answer is a gate, not a write: `appstore.bundle_id_capability.gate`
(a read-only gate over any observed capability, generalizing `appstore.app_group.gate`) and Walter's own
`app_app_attest` node, which holds back the host app's profile until App Attest is confirmed enabled.
Platform is also corrected to the literal `UNIVERSAL` (fact 1, below). Six commits, test-first, no live
call. No provider call of any kind in this task.**

- **The coordinator's live facts this task rests on (2026-09-30, read-only plus one guarded live probe):**
  1. **Platform is a permanent trap on this account.** The first real Walter apply created all three
     identifiers `IOS`; after the operator restored the app record and configured App Groups in the
     portal, all three — and all 21 of the operator's other bundle ids — read `platform: UNIVERSAL`. A
     re-run then refused at plan time (`app_id.platform`: the resource is ours, but its current value
     does not match what was requested, and this tool will not change it). Platform is immutable through
     the API, so `IOS` was never a safe default here.
  2. **App Attest is readable, not writable.** The host identifier's capability list now returns
     `APP_ATTEST` and `APP_ATTEST_OPT_IN` (the operator enabled them by hand in the portal), although
     neither is among the 28 members of Apple's own `CapabilityType` enum (specification 4.5). On a
     throwaway `UNIVERSAL` identifier, `POST /v1/bundleIdCapabilities` with either as `capabilityType`
     answered `409 ENTITY_ERROR.ATTRIBUTE.TYPE` ("An attribute in the provided entity has the wrong
     type"). Apple's own docs: App Attest works in the host app and in watchOS/action/SSO extensions
     only — `generateKey` fails when called from an app extension regardless of `isSupported` — so no
     gate applies to the NSE or widgets extensions.
  3. **The gate design follows M2's own precedent exactly** (decision (j) point 2, "a gate passes
     through the key it checked"): enabling a capability changes the App ID and invalidates profiles
     minted before it, so a gate — never a write the tool cannot make anyway — holds the dependent
     profile back until the capability reads enabled, and `appstore.profile.ensure`'s own
     replace-when-INVALID heals a profile invalidated by the operator's later portal work on the
     document's next run, the same healing M2 already relies on.
- **Task 1, the gate tool.** `AppleObservableCapabilityType` (`willikins-types`) admits
  `AppleCapabilityType`'s 28 writable members plus the two observed-only ones, each sourced in its own
  doc comment (the specification for the 28, the 2026-09-30 live read for the two) — `AppleCapabilityType`
  itself is untouched, staying exactly the 28 writable members the CLAUDE.md invariant requires. A new
  unit test pins the superset relation directly (every writable member parses as observable; the two
  extras parse as observable but are refused by `AppleCapabilityType`). `appstore.bundle_id_capability.gate`
  (`crates/willikins-providers-appstore/src/tools/capability_gate.rs`) generalizes
  `appstore.app_group.gate` to any `AppleObservableCapabilityType`: `identifier` and `capability` are
  both ordinary input ports, both named in `Gate::subject` (`Gate::need`/`how` are `&'static str` and
  cannot interpolate either), with `how` stating plainly that willikins cannot enable the capability and
  the operator does it in the portal or Xcode. Mock tests cover parent-absent, registered-but-not-listed,
  and present (using `APP_ATTEST`); a fake twin, `fake_agrees_with_live.rs` parity (three cases), a
  catalog-parity spec-equality and snapshot test, and a `pure_tools_agree.rs` case (using `APP_GROUPS`,
  already seeded there, since the fake's `with_apple_bundle_id_capability` seed helper cannot express a
  read-only capability) all pass. Registered in the live catalog (`LIVE_TOOL_NAMES` 33 → 34,
  `insert_appstore_tools` — whose own doc comment's stale "four" live tools is corrected to seven, the
  count it had already drifted to before this task) and the fake catalog (35 → 36 tools); the pinned
  array length in `willikins-providers-doppler/tests/live_catalog.rs` moves with it.
- **Task 2, the Walter document, edited in place.** `platform` is the bare literal `UNIVERSAL` on all
  three bundle identifiers (fact 1); `platform` is no longer a declared input. A new node,
  `app_app_attest` (host identifier only, reading `identifier` from `app_id` directly rather than
  chained through `app_app_groups`, so both gates report together on the very first run rather than one
  surfacing only after the other is already met), holds back `app_profile`'s `identifier` port — while
  `app_profile.name` still binds through `app_app_groups`, the same "gate passes through the key it
  checked" shape applied through two independent ports of the same node — and so `app_profile_to_doppler`
  transitively. The NSE and widgets profiles are unaffected; they keep binding through their own
  `app_group.gate` alone. `crates/willikins-cli/tests/walter_document.rs`: `base_inputs` drops
  `platform`; run 1's blocked set and `Applied.blocked` count (8 → 9) gain `app_app_attest`; a new
  plan-only interleaved case (App Groups on, App Attest still off) proves the host profile and its
  Doppler write alone are held back while the NSE and widgets profiles and their Doppler writes proceed;
  run 2 and run 3's node lists gain `app_app_attest`; `the_document_reads_the_real_layout_by_name` pins
  the `UNIVERSAL` literals, the gate's own binding shape (`identifier` from `app_id` directly, the
  profile's `identifier`/`name` split across the two gates), and that the NSE/widgets profiles are
  unaffected. `walter_apply_blocked_redaction.rs` drops the stale `--input platform=IOS` (the fixture
  seeds no bundle id at all, so this was never a platform-mismatch trap) and names `app_app_attest` among
  the blocked nodes it expects in stdout.
- **The characterization snapshot moved by exactly five additive lines**, all inside Walter's own
  `TYPES:` block, for `app_app_attest`'s five ports — diffed byte for byte against the pre-change
  snapshot. Walter's own `PLAN ERROR` line is unaffected (the characterization's synthesized inputs
  still fail at the very first Doppler read, before any Apple node). No other document's lines moved.
  `willikins-types`' and `willikins-providers-fake`'s own catalog snapshots moved additively (new
  registered type, new registered tool) — neither is the restricted characterization snapshot.
- **Scoped gates green throughout, one commit at a time:** `cargo fmt --all --check`; `cargo clippy`
  (`-D warnings`) on every touched crate (`willikins-types`, `willikins-providers-appstore`,
  `willikins-providers-fake`, `willikins-server`, `willikins-providers-doppler`, `willikins-cli`);
  `cargo test` on the same crates plus `willikins-dsl --test acceptance` (the characterization) and
  `willikins-core --test secret_literal_guard` (no provider-token-shaped literal introduced), every
  suite green; `cargo check -p willikins-types`. The full workspace gate was not run (host rule; the
  coordinator's).
- **No provider call of any kind was made for this task** — every check ran against the fake catalog,
  the mock server, or the type registry directly.
- **Not done here, left for the coordinator/operator:** a live probe confirming `APP_ATTEST`/
  `APP_ATTEST_OPT_IN` behave identically on an `IOS`-platform identifier, not only `UNIVERSAL` (every
  identifier on this account is already `UNIVERSAL`, so this is a theoretical gap, not a known one);
  whether the operator wants `APP_ATTEST_OPT_IN` gated too (Walter's own document only asks for
  `APP_ATTEST`, the base capability); this plan is not re-marked Completed (it already is, from task 3)
  and its own gate design decisions above are additive to, not a revision of, decision (j).

**Addendum:** 2026-09-30 (attacker, pass 7 over the App Attest gate task) — **no defect found; three
coverage commits, eight mutations killed.** Record: `docs/research/2026-09-30-m3e-adversarial-pass-7.md`.
- **Confirmed by mutation:** `appstore.bundle_id_capability.ensure` cannot be asked to write `APP_ATTEST`
  (widening `AppleCapabilityType` is killed by the superset unit test and a new fixture test; the literal
  on Walter's own `healthkit` fails `check`); the gate blocks exactly when the capability is absent
  (inverting `enabled` is killed); it holds back the host profile and its Doppler write and nothing else
  (re-binding the host profile through `app_app_groups`, or the NSE profile through `app_app_attest`, is
  killed); platform is the literal `UNIVERSAL` on all three identifiers (an `IOS` literal is killed, by the
  real-layout pin alone: the fake does not care); the characterization moved only additively.
- **Committed, tests only:** `191f91c` (the gate's `ensure` sends no `POST` while unmet, parent absent or
  capability unlisted); `6d5f28d` (negative fixtures `appstore-capability-read-only-literal.yaml`,
  `InvalidLiteral`, and `appstore-capability-observable-into-ensure.yaml`, `TypeMismatch`, with their
  acceptance tests; the characterization gained only those two documents); `69c04f8` (Walter's interleaved
  plan pins the gate's rendered subject, host identifier and `APP_ATTEST`, and the exact blocked set).
- **Apple's docs, re-read:** App Attest works in the app and in action, extensible SSO and watchOS
  extensions only, so gating the host alone is right; Xcode has an App Attest capability that writes the
  environment entitlement, and a distributed build ignores that entitlement and uses production.
- **Verify, no live call made:** whether a freshly created `UNIVERSAL` identifier already lists `APP_ATTEST`
  (if so, the gate never blocks on a new app; safe either way). **Open to the operator:** gating
  `APP_ATTEST_OPT_IN` too; naming the App Attest entitlement in `m3_repo_files`'s step text.
