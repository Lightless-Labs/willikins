# Milestone 3e: adversarial pass over gates, operator acknowledgement, replace-when-INVALID and the Sample document

**Date:** 2026-09-29
**Task:** the independent attacker's pass over everything landed on `main` for
`docs/plans/2026-09-27-milestone-3e-new-ios-app.md` since the first pass
(`docs/research/2026-09-28-m3e-adversarial-pass.md`), written by nobody who wrote any of it.
**Subject:** replace-when-INVALID (`f980d78`, `63144a8`); gates in `plan` (G1: `e06d85c`, `650cae7`,
`c9fa63e`); gates in `apply`, the journal, the CLI and MCP (G2: `393b4bf`, `fe01d97`); operator
acknowledgement (G3: `70e5e0a`, `3f41d6d`, `0b13a51`, `daffc94`); the two conversion rows (T3b:
`1c65573`, `b1526aa`); Sample's two observed gates and the Sample document (T3c: `98bad49`, `5af1503`,
`16bef8c`); `doppler.branch_config.ensure` and the `prd_deployment_ios` rewiring (B1: `0117cf9`,
`cdbefd1`); the `resolve_recorded_inputs` fix (B2: `162d7f7`, `ffc418d`).
**Method:** read the plan and every landed file, then attack through tests. Every mutation was made by a
script (`mutate.py`, in the pass's scratchpad) that saved the file, applied one exact-text replacement
(asserting it occurs exactly once), ran the narrowest test target in the same command, copied the saved
file back, and confirmed byte identity with `filecmp` and with `cmp` from the shell, plus an empty `git
diff --stat` on the file (never `git checkout`, `reset` or `stash`). No live test ran and no provider was
called: every result below is against mocks, the in-memory fake, or the built binary over the fake.

## 0. The state the pass started from

`main` at `f06516d`, clean apart from the untracked host-maintenance todo (left alone). **Red on
arrival:** `willikins-server`'s `acceptance_13_trusted_directory` and `image_contents` both failed, as B1
and B2 had filed: their hardcoded workflow lists were one document behind since T3c added
`workflows/sample-ios-app.yaml`. Confirmed red, fixed first (`b9287db`): both lists name
`sample-ios-app`, the image test is now "fourteen positive documents". (`image_contents.rs`'s module doc
still says "three positive documents", stale since long before this milestone; left as it was.)

## 1. What was attacked, and what held

### A blocked gate never lets a dependent run

`plan` decides the skip set from a node's *bindings*, before `bind_ports` and before `read`
(`crates/willikins-core/src/plan.rs`, `GateTracking::collect_causes`): any `Step`, `Keyed` or `for_each`
source naming a blocked or skipped node makes the node `Action::Skip`, unbound and unread, and a
skipped `for_each` source collapses to one `instance: None` entry. `Binding` is a closed enum of five
arms (`Input`, `Step`, `Keyed`, `Item`, `Literal`), so there is no composite binding that could reach
another node without passing through `collect_causes`. `apply` classifies each planned instance by its
`Action` *before* `resolve_instance_inputs`, the pure branch and the unknown-input match, so a skipped
non-pure node (Sample's `appstore.profile.ensure`) never reaches `ensure` and never stops the run as
`UnknownInput` (mutation m2). A gate satisfied between plan and apply is `Action` drift, refused before
the `SinkToken` is minted, so an approved plan that said "skip" can never quietly run the node.

**A test gap, closed.** Mutation m1 (the whole-node `Step` arm of `collect_causes` disabled) left
`plan_gates.rs` **green**: its graph only ever blocks a `for_each` *instance*, and its transitive node
binds by `Keyed`. `apply_gates.rs` killed it (three tests), so the engine was covered, but acceptance 12's
own file did not pin "a `Step` binding on a blocked gate is `Skip`". `a512cb5` adds
`a_step_binding_on_a_blocked_scalar_gate_is_skipped_and_never_read`; m1 re-run against it goes red.

### A blocked gate never stops an independent node

The walk continues past `Blocked` and `Skipped` (they push a status and `continue`); only a tool failure
still stops it with a `NotRun` tail over every later instance, dependent or not — decision (j) kept
failure semantics unchanged on purpose, and a failure dominates a block (`outcome_of`: `Err` is
`Failed` whatever `blocked` held). `apply_gates.rs`'s first test and `sample_document.rs`'s run 1 (bundle
ids, capabilities, Doppler, Buildkite all created while eight gates block) are the evidence.

### A gate-free document is unchanged — except over MCP, where it was not (fixed)

- **Characterization.** `git diff 010e162 HEAD` (the commit before G1) on
  `acceptance__characterization_of_every_document.snap` is additions only: the entries of
  `acknowledgement-default.yaml`, `acknowledgement-gate-resume.yaml`, `acknowledgement-literal.yaml`,
  `appstore-bundle-id-text-into-identifier.yaml` and `sample-ios-app.yaml` (whose own entry B1 later
  edited, which is allowed: the document is new in this milestone), plus one insta metadata line
  (`assertion_line: 284`) in the header, which is not an entry. Every pre-existing entry's `plan_json`
  and `fingerprint` is byte-identical. Mutation m3 (`Plan.blocked` serialized even when empty) turns the
  snapshot red with fifteen `"blocked":[]` insertions, so the byte-identity is enforced, not incidental.
- **Journal.** `pre_pass_2_replay.rs` and `post_pass_2_shapes.rs` gained only match arms; no frozen
  fixture file changed since `010e162`.
- **MCP output schemas — a real regression, fixed (`d986c75`).** See finding 1.

### The blocked report is complete and forwardable

`BlockedGate` carries `node`, `instance`, `tool`, `need`, `how`, `subject` (rendered through
`Value::render` from ports `Catalog::insert` guarantees are non-secret `Exact` ports), `awaiting_inputs`
and `holds_back`, with no `Value` inside, so it is secret-free by construction. MCP `plan` returns it in
`PlanResponse.plan.blocked`; `run_status` returns `state: "blocked"`, `blocked` and the fixed `next_step`;
both tool descriptions tell an agent to forward `need`/`how`/`subject` and re-`plan` with the awaited
inputs. The CLI's `supply: --input <name>=done` line is rendered by the engine from `awaiting_inputs`,
never by a tool. With finding 1 fixed, a new test validates the structured result of `describe`, `plan`,
`apply` and `run_status` against the schema the server itself publishes, for a gate-free and a blocked
document alike (`tests/mcp_output_schema_conformance.rs`).

### An acknowledgement cannot be defaulted, secret, or satisfied by accident

- `check` refuses a `default:` on an input of the type and a literal on a port of it, recognising the
  type by the registry entry's `TypeId`, and by name *without* the list flag, so `list<OperatorAcknowledgement>`
  is refused too. Mutation m4 (the default refusal disabled) is killed by the unit test.
- The type is public (`IS_SECRET` false, registry entry non-secret), so the "no secret input types" rule
  is untouched.
- No conversion row targets it (`conversions!` holds three rows, all out of `AppleBundleIdentifier`), and
  no tool outputs it (no output declaration names the type anywhere under `crates/*/src`; its one other mention there is a render test's own literal), so no edge can
  deliver a known acknowledgement from anywhere but the operator's own `--input`.
- An unsupplied `list<OperatorAcknowledgement>` used as a `for_each` source resolves `Unknown` and fails
  `plan` as `ForEachUnknown` — loud, not a silent pass (read from `plan.rs`; the type is never used that
  way in a shipped document).
- The butler's rebuild from the journal (`resolve_recorded_inputs`) leaves an unrecorded acknowledgement
  out; it can never add one. A hand-edited record that drops a supplied one only blocks, the safe
  direction. Mutation m9 (the B2 exception removed) is killed by
  `acknowledgement_gate_blocked_resume.rs`.
- `apply --plan-id` refuses `--input` outright, so an acknowledgement is only ever supplied to a fresh
  `plan`.
- **By design, not by accident:** one input bound to two gates satisfies both, and an acknowledgement
  does not persist between runs — every re-run must supply every `*_done=done` again or block again
  (exit 3). Sample binds one input per gate. Both are consequences of "no saved run state", recorded
  here so the dry run and the operator expect them.

### Exit status

`exit_for_run_state`: `Succeeded` 0, `Failed`/`Running` 1, `Blocked` 3; `plan` exits 0 with a non-empty
`blocked`; `describe` exits 0 when only `awaiting` is non-empty (`ok` is `errors` and `missing` empty).
Mutation m5 (`Blocked` mapped to 0) is killed by `sample_apply_blocked_redaction.rs`, a real binary run
over a real file journal.

### replace-when-INVALID

- **Never an `ACTIVE` profile.** Only `profileState == "INVALID"` (exact) resolves `Invalid`. Mutation m6
  (every state treated as replaceable) turns eight mock tests red, among them
  `ensure_leaves_an_active_profile_untouched_and_never_deletes_it`.
- **Never another identifier.** `find_bundle_id` compares identifiers byte-exact, and the profile list is
  the bundle id's own relationship (`/v1/bundleIds/{id}/profiles`); its pagination re-attaches only
  `links.next`'s query string to that fixed path (`client.rs`, `list_bundle_id_profiles`), so a response
  cannot steer it at another bundle id's profiles or another host.
- **Never another name.** `find_profile_row` compares names byte-exact before `profileState` is read
  (`ensure_never_deletes_an_invalid_profile_of_a_different_name`).
- **But the id deleted was the instance read's, not the matched row's — fixed (`50de4a2`).** See finding 2.
- **Delete failure stops before any create.** Mutation m7 (`let _ = client.delete_profile(..)`) is
  killed by `ensure_stops_before_any_create_when_the_delete_fails`.
- **Delete without create is possible, and was silent — fixed (`79a4f28`).** See finding 3.
- **Class.** `Class::Destructive` is right under the design doc: `class.rs` defines it as "destroys or
  overwrites something", and `ensure` now deletes a resource it did not create in the same call. The
  class is static (`Checked::class`), so the Sample document requires approval on every run, including a
  first run whose profiles are all skipped — decision (j) point 5 says so and the operator accepted it.
  Not fixed, recorded: the plan renders a replacement as `create`; nothing in the approved plan says a
  delete will run (finding 4).
- The fake twin mirrors the live resolution order and drops only the one `INVALID` record at its exact
  key (`records.len() == 1` is required before `Invalid` is returned).

### The conversions

Three rows, all `AppleBundleIdentifier => {AppleProfileName, AppleBundleIdName, Text}`, public to
public, so secrecy-monotone trivially (and `conversions!`'s `const` assert refuses a secret-to-public row
at compile time). Total by grammar containment, pinned by three proptests each; mutation m11 (the
`AppleBundleIdName` `From` impl lowercasing) is killed by two of them. One hop: `check` records the
conversion on the edge, and `plan`/`apply` never probe the table (milestone 3d's lint and tripwire,
unchanged). The reverse direction still fails `check` (`appstore-bundle-id-text-into-identifier.yaml`).

### The Sample document's three-run sequence

`sample_document.rs` runs check/plan/apply three times over one `FakeState`: gates unmet (eight blocked,
the three profiles and their Doppler writes skipped, every independent node created); the two observed
gates satisfied by mutating the fake (exactly six nodes newly created); the four acknowledgements
supplied (nothing blocked or skipped; only the three write-only `doppler.secret.set` sinks report
`Created` again, by that tool's design). The ordering claim the design rests on — a profile is held by
its own app-group gate, not merely by registration — is what mutation m10 attacks: rebinding
`app_profile`'s `identifier` and `name` from `steps.app_id.identifier` instead of the gate. It is killed
(run 1's "`app_profile` must be `Skip`"), so the document's data edge is pinned, not only described.
Each profile's `name` is bound from its gate too, which is why both ports had to move for the mutation to
bite.

A rule for gate authors, stated because the Sample gates satisfy it only by construction: a gate whose
non-acknowledgement input is an `Unknown` predicted output reaches `read` with an `Unknown` and fails
`plan` through `helpers::get` — a hard error, not `Blocked`. Sample's gates bind `identifier`, which
`appstore.bundle_id.ensure` predicts known from the input, so they are safe.

## 2. Findings

1. **Every gate-free `describe`, `plan` and `run_status` result failed its own published MCP
   `outputSchema` (fixed, `d986c75`).** `Plan.blocked`, `Applied.blocked`, `Description.awaiting` and
   `RunRecord.blocked` are skipped when empty, but rmcp's `schema_for_output` builds with
   `SchemaSettings::draft2020_12()`, the deserialize contract, which lists every field without a serde
   default as `required`. So the published schema required a field the common-case result omits — every
   document that existed before the gates. The MCP specification: servers MUST return structured content
   conforming to the declared `outputSchema`, and clients SHOULD validate it; a validating client refuses
   the result. G1's addendum had called the `required` entry cosmetic. Test-first: the new conformance
   test went red on `describe` ("`awaiting` is a required property"); the fix adds `default` to the four
   fields' serde attribute, so the schemas drop them from `required` and serialization is unchanged. The
   snapshots that moved (`schema_generation__{plan,applied,description,run_record}_schema_generates` and
   the MCP tool-list snapshot) moved by **removing** those four entries from `required` plus the doc text —
   a deliberate contract correction, not additive drift. `BlockedGate.awaiting_inputs` is always
   serialized, so it conforms; it has no serde default either, so a journal `RunFinished { Blocked }` line
   written between G2 and G3 would not deserialize now — local, unpushed commits, so noted, not fixed.
2. **The id replace-when-INVALID deleted was the one the instance read answered with, never checked
   against the matched row (fixed, `50de4a2`).** `resolve` matched the row by exact name, then read
   `GET /v1/profiles/{row id}` and `resolve_instance` took the id to delete from *that response's* `id`.
   The "never another identifier or name" guarantee rested on Apple answering with the resource asked
   for. Test-first with a mock whose instance body carries another id and name, `INVALID`: before, `read`
   answered `Absent` (so `plan` would show `create` and `apply` would delete the other profile); now both
   `read` and `ensure` refuse with `Provider` and no `DELETE` is issued on either id (mutation m8 proves
   the test bites).
3. **A create failing after the delete left no profile, and the journaled failure named only the create
   (fixed, `79a4f28`).** Convergent — the next run creates it — but a destructive tool's failure record
   must name its side effect. The error now reads "the INVALID profile at this key was deleted, but
   creating its replacement failed; re-run to create it: …", same `kind`. Relevant beyond `500`s: see
   verify item 14 on an immediate same-name create after a delete.
4. **Not fixed, a design finding: an approved plan never shows the delete.** A replacement plans as
   `Action::Create`; the tool's static `Destructive` class forces approval, but an approver reading the
   plan sees a create. A plan-visible marker (an `Action::Replace`, or an observation that says "replaces
   `INVALID`") would close it; it touches the engine's action set and every renderer, so it is for the
   coordinator to schedule, not for this pass.
5. **Test gap, closed (`a512cb5`):** see "A blocked gate never lets a dependent run".
6. **Red on arrival, fixed (`b9287db`):** the two `willikins-server` workflow-list tests.
7. **Red on arrival, found by this pass's own scoped gate, fixed (`369706f`):**
   `willikins-cli`'s `acceptance_11_mcp_parity::validate_parity_for_every_shipped_document` makes one MCP
   `validate` call per shipped document against a clock that never advances, under the default read
   rate of 60 a minute. B2's fixture brought the count to 61, so the last document in sort order,
   `sample-ios-app.yaml`, was refused `RateLimited`. No one had run `-p willikins-cli` since. The sweep
   now builds its butler with a read rate of at least the document count; the limiter is not what it
   tests, and the rate-limit tests that do test it (`acceptance_read_ops.rs`, `adversarial_10a.rs`, `adversarial_10b.rs`, `http_server.rs` in `willikins-server`) are untouched.

## 3. Mutations

Thirteen runs, eleven distinct mutations, each restored and confirmed byte-identical by `cmp` and an empty
`git diff --stat`.

| # | File | Mutation | Test target | Result |
| --- | --- | --- | --- | --- |
| m1 | `willikins-core/src/plan.rs` | whole-node `Step` arm of `collect_causes` disabled | `plan_gates` | **survived** (9 green) |
| m1b | same | same | `apply_gates`, `operator_acknowledge_document` | killed, 3 `apply_gates` tests |
| m1c | same | same, after `a512cb5` | `plan_gates` | killed, the new test |
| m2 | `willikins-core/src/apply.rs` | classify `Blocked` only, not `Skip` | `apply_gates` | killed, 3 tests (`downstream` not `Skipped`) |
| m3 | `willikins-core/src/plan.rs` | `Plan.blocked` without `skip_serializing_if` | `willikins-dsl --test acceptance` | killed, characterization (15 `"blocked":[]`) |
| m4 | `willikins-core/src/check.rs` | `AcknowledgementDefault` never pushed | core `--lib acknowledgement` | killed |
| m5 | `willikins-cli/src/commands.rs` | `RunState::Blocked` exits 0 | `sample_apply_blocked_redaction` | killed, "a blocked run must exit 3" |
| m6 | `willikins-providers-appstore/src/tools/profile_ensure.rs` | every `profileState` treated as `INVALID` | `profile_ensure_mock` | killed, 8 tests incl. the `ACTIVE` one |
| m7 | same | delete failure ignored (`let _ =`) | `profile_ensure_mock` | killed, `ensure_stops_before_any_create_when_the_delete_fails` |
| m8 | same | instance-id check disabled (finding 2's fix) | `profile_ensure_mock` | killed, the new test |
| m9 | `willikins-server/src/butler.rs` | B2's acknowledgement exception removed | `acknowledgement_gate_blocked_resume` | killed, `apply` refuses with "the plan recorded no value for input `gate_done`" |
| m10 | `workflows/sample-ios-app.yaml` | `app_profile` bound from `app_id`, not its gate | `sample_document` | killed, "run 1: `app_profile` must be Skip" |
| m11 | `willikins-types/src/appstore.rs` | `AppleBundleIdentifier => AppleBundleIdName` lowercases | types `--lib bundle_identifier_to_bundle_id_name` | killed, 2 proptests |

Finding 1 has its own red-then-green in lieu of a mutation: the conformance test failed on `f06516d`'s
code and passed after `d986c75`.

## 4. Verify items

This pass called no provider, so none of the plan's open verify items (5, 6, 7, 8, 9, 10, 11, 12, 13)
could move; each needs a live read or the operator. One is added: **14**, whether an immediate
`POST /v1/profiles` of the same name on the same identifier succeeds right after `DELETE
/v1/profiles/{id}` of the `INVALID` one, or answers `409 ENTITY_ERROR` until the delete propagates
(milestone 3c found names unique per identifier). If it `409`s, replace-when-INVALID fails every first
attempt — now with an error that says so (finding 3) — and converges on the next run. The written,
unrun `appstore_live_profile_replace_cycle` settles it.

## 5. Not settled

- Finding 4 (the plan never shows the delete).
- `BlockedGate.awaiting_inputs` has no serde default (finding 1's last sentence).
- The gate mechanism is still proven end to end only against the fake catalogue and the built binary over
  it; no MCP client outside the process and no live provider has driven a blocked run. The dry run
  remains the coordinator's, and needs the sandbox Buildkite token the operator has since renewed.
- `PortSpec.derived_only` has the same `skip_serializing_if`-without-`default` shape, but no tool's
  `outputSchema` embeds `PortSpec` (`list_tools` publishes an opaque object), so it does not violate a
  published schema today.

## 6. Gate after the pass

Scoped only, per the host rules: `cargo fmt --all --check`; `cargo clippy --all-targets -D warnings` on
`willikins-core`, `willikins-journal`, `willikins-server`, `willikins-providers-appstore` and
`willikins-cli` (clean); `cargo test` over `willikins-core`, `willikins-journal` and `willikins-server`
(every suite green, after `d986c75`), `willikins-providers-appstore` (199 passed, 3 ignored live tests),
`willikins-cli` (178 passed after `369706f`; the one failure before it is finding 7), `willikins-dsl
--test acceptance` (5 passed, characterization byte-identical); `cargo check -p willikins-types`. Not
re-run by this pass: `willikins-providers-fake`, `willikins-tools`, `willikins-providers-doppler`,
`willikins-providers-github`/`-buildkite`/`-signoz`, none of which this pass touched (the serde attribute
in finding 1 changes no serialized byte). The full workspace gate is the coordinator's.

Commits: `b9287db`, `d986c75`, `50de4a2`, `79a4f28`, `a512cb5`, `369706f`, and this record with the
plan's addendum.
