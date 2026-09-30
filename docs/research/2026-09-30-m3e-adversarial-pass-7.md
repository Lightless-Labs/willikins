# Milestone 3e: adversarial pass 7 — the App Attest gate task

**Date:** 2026-09-30
**Task:** the independent attacker's pass over the App Attest gate task, which landed on `main` for
`docs/plans/2026-09-27-milestone-3e-new-ios-app.md` in answer to the operator's "Could we update the
workflow to enable App Attest on apps by default?". The attacker wrote none of it.
**Subject:** `fbda1ee` (`AppleObservableCapabilityType`), `a732788` (`appstore.bundle_id_capability.gate`),
`87edd19` (fake twin, parity, pure-tool coverage), `4f95231` (live catalog), `ca5379f` (Walter: platform
`UNIVERSAL`, the `app_app_attest` gate), `f96fd4a` (the task's addendum).
**Method:** read the task's addendum, the six commits and the code they touch; re-read Apple's own
App Attest pages for the two claims the task makes about them; then attack with mutations. Every
mutation followed the same steps: copy the file into the pass's scratchpad, apply one exact-text
replacement (a script asserted it occurred exactly once), run the narrowest test target, copy the saved
file back, `touch` it, and confirm byte identity with `cmp` (exit 0 every time) and a clean
`git status`. Never `git checkout`, `reset` or `stash`. **No live test ran and no provider API was
called.** The only network traffic was two fetches of Apple's public documentation.

## 0. The state the pass started from

`main` at `f96fd4a`, clean apart from `goal.txt` and the host-maintenance todo (both left alone).
Nothing uncommitted, nothing red: the baseline run of `walter_document` (4/4) and
`walter_apply_blocked_redaction` (1/1) passed before any mutation.

## 1. The questions, and what the evidence says

### Can `appstore.bundle_id_capability.ensure` be asked to write `APP_ATTEST` or `APP_ATTEST_OPT_IN`? No

Three independent walls, each now pinned:

1. **The writable grammar.** `AppleCapabilityType` is still exactly Apple's 28 `CapabilityType`
   members; `fbda1ee` added a *separate* type, `AppleObservableCapabilityType`, rather than widening
   it. Mutation H (below) widens the writable pattern with `APP_ATTEST` and is killed by the unit test
   `observable_capability_type_is_a_strict_superset_of_the_writable_one` and by this pass's new
   fixture test.
2. **No conversion.** `conversions!` has no row between the two capability types (none could be total
   in the observable-to-writable direction), so a typed binding cannot cross. This pass adds
   `workflows/fixtures/appstore-capability-observable-into-ensure.yaml`: an input typed
   `AppleObservableCapabilityType` bound into the writer's `capability` port fails `check` with exactly
   one `TypeMismatch` (expected `AppleCapabilityType`, found `AppleObservableCapabilityType`).
3. **The literal.** `workflows/fixtures/appstore-capability-read-only-literal.yaml`: the writer given
   the literal `APP_ATTEST` fails `check` with exactly one `InvalidLiteral` (`AppleCapabilityType`,
   "does not match the required pattern"). Mutation C shows the same refusal on Walter itself.

And structurally, the gate module cannot write a capability at all: the client's only capability write,
`AppstoreClient::create_bundle_id_capability`, takes `&AppleCapabilityType`, which
`capability_gate.rs` never imports.

### Does the gate block exactly when the capability is absent, and never write? Yes

`observe` resolves the parent by exact identifier (an unregistered identifier is `Absent`, not a
refusal), lists its capabilities and looks for a row whose `capabilityType` equals the requested one.
`CapabilityAttributes.capability_type` is a plain `String` and the setting fields are all optional, so a
row type outside the 28 (the very rows App Attest produces) deserializes rather than failing the whole
listing. Mutation F (`if enabled` → `if !enabled`) is killed by two of the four mock tests. `ensure`
re-observes and returns `changed: false` either way; this pass adds two mock tests that call `ensure`
on an unmet gate (parent unregistered; parent registered without the capability) with zero-call `POST`
mocks on `/v1/bundleIds` and `/v1/bundleIdCapabilities`, asserted afterwards. Any other write would
hit mockito's unmatched-request 501 and fail `ensure` outright.

### Does it hold back the host profile and its Doppler write, and nothing else? Yes

`app_profile.identifier` binds `steps.app_app_attest.identifier`; `app_profile.name` still binds
`steps.app_app_groups.identifier`. Both gates pass the same identifier through, so the value is one
value and the data edges order the host profile after both. The interleaved plan (App Groups on, App
Attest off) asserts `app_app_attest`'s `holds_back` is exactly `{app_profile, app_profile_to_doppler}`.
This pass adds two assertions there: the blocked set is exactly `app_app_attest` plus the four
acknowledgements (every app-group gate and the app record are open), and the rendered `subject` is
`[("identifier", <host identifier>), ("capability", "APP_ATTEST")]` — `need`/`how` are `&'static` and
cannot name the capability, so `subject` is the only place the operator learns which one is missing.
Mutation A (host profile re-bound through `app_app_groups`) and mutation G (the gate's subject cut to
`identifier`) are both killed.

### Do the NSE and widgets profiles still proceed? Yes

Their `identifier` and `name` bind through their own `app_group.gate` alone, and the interleaved plan
asserts `nse_profile`, `widgets_profile` and both their Doppler writes plan `Create` while App Attest
is still off. Mutation E (the NSE profile's `name` bound through `app_app_attest`) is killed. Apple's
own page (below) supports gating the host alone.

### Is the platform `UNIVERSAL` on all three identifiers, and a literal, not an input? Yes

All three `*_id.platform` bindings are the bare literal `UNIVERSAL` and `platform` is no longer a
declared input; `the_document_reads_the_real_layout_by_name` pins both. Mutation B (`nse_id.platform:
IOS`) is killed by that test alone: the three-run fake test passes under it, because the fake does not
care which platform a fresh identifier carries — the literal pin is the only guard, and it holds. No
stale `--input platform=` remains in a runbook: the only hit outside `docs/plans` and `docs/research`
is `docs/HANDOFF.md` line 188, which is another document's (`platform=UNIVERSAL`) and unaffected.

### Does every other document plan byte-identically? Yes

Across the six landed commits (`git diff fbda1ee~1 f96fd4a`) the characterization snapshot gained
exactly five lines, all `app_app_attest.*` inside Walter's own `TYPES:` block; no other document's
lines moved. This pass's own run of `willikins-dsl --test acceptance` wrote a `.snap.new` whose `diff`
against the committed snapshot was exactly the two new fixtures' blocks (eight added lines), nothing
else; it was accepted and the suite re-ran green with `INSTA_UPDATE=no`.

## 2. Mutations

| # | file | mutation | killed by |
|---|---|---|---|
| A | `workflows/walter-ios-app.yaml` | `app_profile.identifier` ← `app_app_groups.identifier` | `gates_unmet_then_satisfied_then_acknowledged` (host profile `Create`, expected `Skip`), `the_document_reads_the_real_layout_by_name` |
| B | same | `nse_id.platform: IOS` | `the_document_reads_the_real_layout_by_name` only |
| C | same | `healthkit.capability: APP_ATTEST` | three `walter_document` tests, at `check`: `InvalidLiteral { node: healthkit, port: capability, AppleCapabilityType }` |
| D | same | `app_app_attest.capability: APP_ATTEST_OPT_IN` | run 2's blocked set still holds `app_app_attest` (the fake seeds only `APP_ATTEST`); the real-layout literal pin |
| E | same | `nse_profile.name` ← `app_app_attest.identifier` | the interleaved plan: `nse_profile` `Skip`, expected `Create` |
| F | `crates/willikins-providers-appstore/src/tools/capability_gate.rs` | `if enabled` → `if !enabled` | `absent_when_registered_but_the_capability_is_not_in_the_list`, `present_once_the_read_only_capability_is_listed` |
| G | `crates/willikins-providers-fake/src/tools/appstore_bundle_id_capability_gate.rs` | `subject: &["identifier"]` | this pass's new subject assertion in the interleaved plan |
| H | `crates/willikins-types/src/appstore.rs` | `AppleCapabilityType`'s pattern gains `APP_ATTEST` | `observable_capability_type_is_a_strict_superset_of_the_writable_one`, this pass's `a_read_only_capability_literal_is_refused_by_check_on_capability_ensure` |

Every restore: `cp` from the scratchpad copy, `touch`, `cmp` exit 0, `git status` clean.

## 3. What this pass committed (tests only; no defect found in the landed code)

- `191f91c` — two never-writes mock tests for the gate's `ensure`.
- `6d5f28d` — the two negative fixtures and their acceptance tests in
  `crates/willikins-providers-appstore/tests/capability_documents.rs`; the characterization snapshot
  moves only by the two new documents.
- `69c04f8` — Walter's interleaved plan pins the gate's rendered subject and the exact blocked set.

Scoped gates green: `cargo fmt --all --check`; `cargo clippy -p willikins-providers-appstore -p
willikins-cli --all-targets -- -D warnings`; `willikins-dsl --test acceptance`; `willikins-cli`'s
`walter_document`, `walter_apply_blocked_redaction`, `acceptance_11_mcp_parity` and `adversarial`
(the latter two sweep every fixture); `willikins-core --test secret_literal_guard`; and, after the last
restore, all of `willikins-types`, `willikins-providers-appstore` and `willikins-providers-fake`, plus
`cargo check -p willikins-types`. The full workspace gate is the coordinator's.

## 4. Apple's documentation, re-read (fetched 2026-09-30)

- *Establishing your app's integrity*
  (`developer.apple.com/documentation/devicecheck/establishing-your-app-s-integrity`), verbatim:
  "Action, extensible SSO, and watchOS extensions are supported. All other extension types are not
  supported, even if the isSupported method property is true." Notification service and widget
  extensions are neither, so gating the host alone is right.
- *App Attest Environment* entitlement
  (`developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.devicecheck.appattest-environment`):
  "add the App Attest capability to your app target. This adds the entry to the app's entitlements file
  with `development` as the associated value", and "After distributing your app through TestFlight,
  the App Store, or the Apple Developer Enterprise Program, your app ignores the entitlement you set and
  uses the production environment." So the gate's `how` ("from Xcode's Signing & Capabilities") names a
  real Xcode capability, and a missing entitlement cannot misroute a distributed build.

## 5. Notes and open items (none blocks the task)

- **Fresh identifiers, unverified live.** Whether a freshly created `UNIVERSAL` identifier already lists
  `APP_ATTEST` (a fresh `IOS` one arrives with `IN_APP_PURCHASE`) is unknown. If it does, the gate never
  blocks on a new app and "on by default" is already true; if not, it blocks as designed. Either way it
  is safe; only the first-run report differs. A verify item.
- **`m3_repo_files`'s step text** names "three entitlements files" but not the App Attest environment
  entitlement. Xcode's App Attest capability writes it (`development`), and a distributed build ignores
  it (above), so nothing breaks; the text could name it for completeness. Not edited here: the literal
  is the document's policy text and is pinned by tests.
- **`APP_ATTEST_OPT_IN`.** Walter gates only `APP_ATTEST`; mutation D shows that swapping in the
  opt-in would block on the fake until seeded. Whether the operator wants it gated too is theirs to say.
- **Mocks match `Matcher::Any` queries.** The shared client's exact queries (`filter[identifier]`; no
  query string on the capabilities path, which rejects `?limit=`) are pinned in
  `bundle_id_capability_ensure_mock.rs`, and the gate calls the same client functions, so this is not a
  gap.
- **Out of scope, pre-existing:** `healthkit`/`push`/`data_protection` are not ordered before
  `app_profile`; a profile minted beside a capability create is healed by replace-when-INVALID on the
  next run (the design M2 already relies on).
