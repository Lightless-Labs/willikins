# Milestone 3e: adversarial pass 3 — F1 (plan-visible replacement), F2 (old Blocked lines), and the Apple error harness

**Date:** 2026-09-29
**Task:** the independent attacker's pass over what landed on `main` for
`docs/plans/2026-09-27-milestone-3e-new-ios-app.md` since pass 2
(`docs/research/2026-09-29-m3e-adversarial-pass-2.md`), written by nobody who wrote any of it.
**Subject:** the diagnosis lane's live-harness work (`8c17ee5`, `b672c3d`, `01149da`, `fdcf332`,
`b72e3f5`, `e0a64f9`) and the fix lane's F1 and F2 (`27a1a09`, `1a4f16d`, `78d4e13`).
**Method:** read the plan, the addenda and every landed file; attack through tests. Every mutation was
made by a script (`mutate.py`, in the pass's scratchpad) that copied the file aside, applied one
exact-text replacement (asserting it occurs exactly once), ran the narrowest test target, copied the saved
file back and touched it, then confirmed byte identity with `filecmp`, with `cmp`, and with an empty `git
diff --stat` on the file (never `git checkout`, `reset` or `stash`). **No live test ran and no provider
was called**: every result below is against in-test tools, mocks or the in-memory fake.

## 0. The state the pass started from

`main` at `78d4e13`, clean apart from the untracked host-maintenance todo (left alone). Nothing was
uncommitted or red on arrival in the crates this pass ran (`willikins-core`, `-providers-appstore`,
`-providers-fake`, `-cli`, `-journal`, `-dsl --test acceptance`, `-server`).

## 1. The questions, and what the evidence says

### Can a plan that will delete ever show only `Create`? Yes, before this pass: `NoOp` (fixed, `3bd1ee9`)

F1 makes `plan` ask `Tool::replaces` when a non-pure tool's `read` answers `Absent`, and plans
`Action::Replace` plus a `Plan::replacing` entry when it says `true`. `apply` re-plans before running and
`check_drift` compares actions, so a profile that turns `INVALID` **between approval and `apply`** is
refused as `DriftKind::Action { NoOp -> Replace }`. That case had no test (F1's own "remaining" list);
it has one now (`439207d`,
`a_profile_invalidated_after_approval_refuses_as_action_drift_and_deletes_nothing`, fake catalog, real
signing document, `Approval::Human`), and it holds.

The hole was **inside** the run. `check_drift` runs once, before any node executes, and `apply` calls
`Tool::ensure` on **every** non-pure instance whose inputs are known, whatever its planned action — a
`NoOp` included (`crates/willikins-core/src/apply.rs`, the `None =>` arm of
`first_unknown_required_input`). `AppstoreProfileEnsure::ensure` re-resolves and deletes on
`ProfileResolution::Invalid`. Apple invalidates a profile when a capability is enabled on its App ID
(live-confirmed by the diagnosis lane's probe 2). In `workflows/sample-ios-app.yaml` the capability
nodes (`healthkit`, `push`, `data_protection`) are declared before, and never feed, the profile nodes;
`check`'s topological order breaks ties by declaration order, so they run first. So any run in which a
capability node plans `Create` while a profile is `ACTIVE` (a capability added to the document later, or
a first enable that failed on an earlier run after the profiles existed) plans `profile: NoOp`, shows an
empty `replacing`, and then deletes and recreates the profile mid-run. The approver saw no delete.

Reproduced red with two in-test tools sharing state (`crates/willikins-core/tests/apply_replaces.rs`):
an invalidator whose `ensure` invalidates, and a replaceable tool shaped like the profile tool. Before
the fix: `deletes == 1` with the approved plan saying `NoOp`.

**Fix (rule 4a, `3bd1ee9`):** just before `ensure`, an instance whose planned action is not
`Action::Replace` asks `Tool::replaces(&resolved_inputs)`; `true` fails that instance with
`ToolErrorKind::Conflict` (a willikins-written message: nothing was deleted, re-run to plan the
replacement), through the existing `Failed` + `NotRun`-tail path. `ApplyError::Drift` was not reused:
its contract is "nothing has been executed yet", and the server journals on that. The re-run's plan
shows `Replace` and names the profile, and approving it replaces exactly once (same test). A tool that
never replaces keeps the default `false` and pays nothing; `appstore.profile.ensure` pays one more
read-only resolve (bundle id list, the relationship list, one instance `GET`) per instance per `apply`.
What remains is the window between that resolve and `ensure`'s own, the one every read-then-write has.
The mirror test (`a_no_op_that_stays_valid_still_converges`) pins that a valid `NoOp` still converges
`Unchanged`.

### Is the non-secret profile name the only identifying text shown? Yes, but no test said so (gap closed, `439207d`)

`replacing_entry` renders exactly `ToolSpec::key` — `identifier` and `name` for the profile tool — through
`Value::render()`; the CLI's `replacing:` section prints only `subject`, each through `single_line`. The
`INVALID` profile's own id never reaches the plan: `read` answers `Absent` with an unknown `profile`
output. But F1's document test read `subject` through a `HashMap` and never checked its length, so
**mutation m7a (subject built from every bound input, the certificate id among them) survived**. The test
now asserts exactly two subject entries, that neither `CERT1` (the fixture's certificate id) nor
`PROFILE1` (the `INVALID` profile's id) appears in `Plan::replacing`, and that `PROFILE1` appears nowhere
in the serialized plan; m7b, the same mutation, is killed. (The certificate id still appears in the plan
as the profile node's *input*, as it did before F1; that is not new and not part of `replacing`.)

### Do plans without a replacement stay byte-identical? Yes

`Plan.replacing` is `#[serde(default, skip_serializing_if = "Vec::is_empty")]`; the characterization
snapshot passes unchanged on `3bd1ee9` and `439207d` (`willikins-dsl --test acceptance`), and m1 (the
`skip_serializing_if` removed) is killed by it. `Action::Replace` is a new enum value, reached only when
`replaces()` answers `true`, so every existing fingerprint (`InstanceFingerprint::action`) is unchanged.
The journal stores `Plan` as `Redacted` JSON, never deserializes it, so an old `PlanRecorded` line reads
back as it did; `InstanceFingerprint` deserializes through `Action`, which only gained a variant.

### Does an old journal line read back? Yes

F2's `#[serde(default)]` on `BlockedGate.awaiting_inputs` lets a `RunFinished { Blocked }` line written
between G2 and G3 deserialize; m3 (the attribute removed) is killed by
`a_run_finished_blocked_line_without_awaiting_inputs_still_deserializes`.

### Does the harness's new reporting ever print `errors[].detail` or an identifier? No, by construction and by test

`apple_error_summary` reads only `errors[].code` and `errors[].title`; m4 (append `detail`) is killed by
four tests, including the sentinel test. `stop_message` repeats a `ToolError` message only on an exact,
anchored match with a willikins-written text; m5 (the status regex unanchored) is killed. Every `println!`
and `panic!` in `tests/live_write_cycle.rs` added by `b672c3d`, `01149da` and `fdcf332` prints counts,
statuses, Apple enum values (`capabilityType`, `profileState`) on the run's own throwaway identifier,
settings *shapes* (`settings null`, `settings[n]`), or the summary; none prints a certificate, profile or
key id. **Accepted risks, recorded, not fixed:** `title` is echoed verbatim (escaped, 120 characters) —
Apple documents it as a generic sentence per code; and `is_code_shape` admits a ten-character
alphanumeric string, which is also the shape of an Apple resource id — `code` is a documented enum, so a
code carrying an id would be Apple misbehaving. Pre-existing and out of this pass's commits: an
unexpected `Observation` in the probe's "not `Absent`" arm is printed with `{other:?}`, which for a
`Present` bundle-id read would include that identifier's opaque id — reachable only if a freshly
generated `com.willikins.probe.delete-me.<pid>-<seconds>` already existed.

## 2. Findings

1. **An approved plan could show `NoOp` while `apply` deleted (fixed, `3bd1ee9`).** See above. Severity:
   the class is `Destructive`, so a human approved the run, but approved a plan that named no delete —
   exactly what F1 set out to rule out, reachable in Sample's own shape.
2. **Test gap: the replacement's subject was not pinned to the key (closed, `439207d`).** m7a survived,
   m7b killed.
3. **Coverage F1 left open (added, `439207d`):** `ACTIVE` at approval, `INVALID` at `apply` → action drift,
   nothing deleted.

## 3. Mutations

Eight runs, seven distinct mutations, each restored and confirmed byte-identical by `filecmp`, `cmp` and an
empty `git diff --stat` on the file.

| # | File | Mutation | Test target | Result |
| --- | --- | --- | --- | --- |
| m7a | `willikins-core/src/plan.rs` | `replacing_entry` renders every bound input, not `ToolSpec::key` | appstore `profile_documents` | **survived** (9 green) |
| m7b | same | same, after `439207d` | appstore `profile_documents` | killed, the INVALID-plans-Replace test |
| m1 | `willikins-core/src/plan.rs` | `Plan.replacing` without `skip_serializing_if` | `willikins-dsl --test acceptance` | killed, characterization |
| m2 | `willikins-core/src/plan.rs` | `plan_one` plans `Create` where `replaces()` says `true` | appstore `profile_documents` | killed, 2 tests (Replace; action drift) |
| m3 | `willikins-core/src/plan.rs` | `BlockedGate.awaiting_inputs` without `serde(default)` | journal `event_shapes` | killed, F2's test |
| m4 | `willikins-providers-appstore/tests/support/apple_error_report.rs` | summary appends `errors[].detail` | appstore `redaction apple_error_report` | killed, 4 tests incl. the sentinel |
| m5 | same | `stop_message`'s status regex unanchored | same | killed, the withholding test |
| m6 | `willikins-core/src/apply.rs` | rule 4a removed (`ensure` called directly) | core `apply_replaces` | killed, `deletes == 1` |

Finding 1 also has its own red-then-green: `apply_replaces` failed on `78d4e13`'s `apply` (`left: 1,
right: 0`, "a delete ran that the approved plan never showed") and passes on `3bd1ee9`.

## 4. Verify items

No provider was called, so none moved. Rule 4a's extra resolve is three read-only `GET`s per profile
instance per `apply`; it spends no write and no rate the diagnosis lane cares about, but a live `apply` of
Sample will now read each profile twice.

## 5. Not settled, for the coordinator

- The diagnosis lane's prescribed fix (the first `list_bundle_id_capabilities` request carries no
  `limit`) is still not made; every capability node on the live account still fails its `read` with
  `400`, and replace-when-INVALID stays mock-proven.
- The MCP `outputSchema` conformance case for a non-empty `Plan.replacing` was not added. `Replacing.subject`
  has the same `Vec<(PortName, String)>` shape as `BlockedGate.subject`, whose blocked case already
  conforms, so the risk is low.
- `Applied`/`RunRecord` still carry no `Replaced` status; a replacement applies as `Created`. With rule 4a
  a replacement can only run where the approved plan said `Replace`, so the journal's approved plan names
  it.

## 6. Gate after the pass

Scoped only, per the host rules: `cargo fmt --all --check`; `cargo clippy --all-targets -D warnings` on
`willikins-core` and `willikins-providers-appstore` (clean); `cargo test` over `willikins-core` (every
suite green), `willikins-providers-appstore` and `willikins-providers-fake` together with `willikins-cli`
(600 passed, 3 ignored live tests, 0 failed), `willikins-dsl --test acceptance` (characterization
byte-identical), `willikins-journal` and `willikins-server`; `cargo check -p willikins-types`. The full
workspace gate is the coordinator's.

Commits: `3bd1ee9`, `439207d`, and this record with the plan's addendum.
