# Milestone 3e: one document provisions a new iOS app end to end — Sample

**Created:** 2026-09-27
**Gate:** PRE-FLIGHT PARTLY BLOCKED, 2026-09-27 — implementation (tasks 1 to 3) is clear; the sandbox
**dry run** is blocked on one credential: the sandbox Buildkite token answers `401` on
`GET /v2/access-token`, which needs no scope, so it is revoked or expired rather than under-scoped.
See "Credentials" and the pre-flight checklist.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (type system, tool contract, "policy lives in the
workflow, never in the tool", the 2026-09-22 `Action::Update` addendum)
**Research:** `docs/research/2026-09-16-app-store-connect.md` (section 2, capabilities and
`CapabilitySetting`; section 3, app groups; section 4, app records),
`docs/research/2026-09-16-m3a-buildkite.md`,
`docs/research/2026-09-20-project-survey-and-workflow-library.md` (Sample's "one step in twelve"),
and this plan's own pre-flight, fetched 2026-09-27 and quoted below with its URLs.
**Depends on:** milestones 3a (Buildkite), 3c (signing), 3d (conversions), all Completed.
**Reviewed:** 2026-09-28 (independent adversarial pass over T1 and T2,
`docs/research/2026-09-28-m3e-adversarial-pass.md`)
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
decision (b) (no profiles in the Sample document) was already overturned by operator decision 1 (the
T3a addendum above) and is marked superseded too. (j) is the smallest engine change that fits the
existing model: a gate is an ordinary **pure, read-only tool** that declares itself a gate through a
new default-`None` trait method, so no existing tool, catalog entry or document changes; its `Absent`
plans as `Action::Blocked`, every node downstream of it by data edge plans as `Action::Skip` without
being read, every other node plans and applies exactly as today, and the run ends `blocked` with a
structured list of what the operator must do and "re-run this document once done". No saved run
state. The engine work is three new tasks, **G1–G3**, which run **before T3**; the Sample-specific
gates (the app record exists; `APP_GROUPS` on all three identifiers) belong to T3. What `apply` does
today when a node fails, established from the code: it **stops**, and every later instance in plan
order, dependent or not, is `NotRun` (see (j), "Today").

## Goal

One workflow document, `workflows/sample-ios-app.yaml`, provisions everything a provider API can
provision for **Sample**, the operator's iOS health app, and names everything it cannot as an explicit
manual step that `plan` reports before anything runs. The operator's answers, 2026-09-27:

- Targets: the app, a **notification service extension** and **widgets**; no watchOS app for now.
- It lives in the existing **Example-Org monorepo** (`Example-Org/monorepo`), not a new
  repository. `apps/sample/` already exists there, empty.
- Bundle identifiers `com.example-org.sample`, `com.example-org.sample.nse`,
  `com.example-org.sample.widgets`, following the account's suffix habit (`.nse`, `.keyboard`,
  `.stickers`).
- Doppler project `sample` with `dev`/`stg`/`prd` configs inheriting the operator's shared base configs.
- Manual, and reported as such: the App Store Connect app record (no create API), app groups (no API;
  created by Xcode or the portal), and adding Sample's files to the monorepo (willikins cannot write
  files yet).

The milestone is done when tasks 1 to 3 are green, the attacker has run task 1's live capability cycle
once on the live Apple account under every trust boundary below, and the **dry run** (below) has
applied the Sample document against the sandbox GitHub, Doppler and Buildkite accounts and throwaway
Apple identifiers, re-applied it `Unchanged`, and torn everything down with counts equal before and
after. The real apply against the operator's own accounts is **not** part of this milestone: it is the
operator's, with the credentials listed under "Credentials — for a later real apply".

## Out of scope

- ~~**Provisioning profiles in the Sample document.**~~ **Superseded 2026-09-28** (operator decision 1
  and decision (j)): the profiles are in the document, behind the app-group gate, and a re-run replaces
  any profile Apple invalidated.
- **Writing files** (entitlements, `Info.plist`, `BUILD.bazel`, `.buildkite/`): a manual step until
  file-writing lands.
- **App groups, the app record, an APNs key, a Doppler grant to the CI service account**: manual steps.
- **`APPLE_ID_AUTH` on anything live** (trust boundary 4). The grammar admits its setting; only mocks
  exercise it.
- **SigNoz.** The operator's key expired on 2026-09-23. Nothing in this milestone calls SigNoz, and
  Sample's document has no telemetry node. A later document may add one when a key exists.
- **Changing `buildkite.pipeline.ensure`'s frozen bootstrap.** Decision (f).
- **The core `Action::Update` gap**, per-target credential routing, secrecy inference. Unchanged.
- **Railway.** No Railway command.

## Trust boundaries (normative)

Every lane holds these as absolute; they extend milestone 3c's eight, which still hold.

1. **Live Apple writes only on throwaway identifiers** named `com.willikins.probe.delete-me.` followed
   by a per-run unique suffix (and, for the dry run, that identifier plus `.nse` and `.widgets`),
   created and deleted inside the same guarded test. Never create, modify or revoke a certificate.
   Never touch an existing identifier, app, profile or device.
2. **The Sample document carries no bundle-identifier default.** `app_identifier`, `nse_identifier`
   and `widgets_identifier` are required inputs, so a forgotten `--input` fails `plan` instead of
   registering `com.example-org.sample` on the live account. The operator's real values appear in the
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
9. **Nothing under `/Users/operator/Projects/example-org` is written.** It is read for its pattern.

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| Sample document | `workflows/sample-ios-app.yaml`, `name: sample-ios-app` |
| Sample's real identifiers (header comment and later real apply only; never a default) | `com.example-org.sample`, `com.example-org.sample.nse`, `com.example-org.sample.widgets` |
| Sample's app group (manual step M2) | `group.com.example-org.sample` — the monorepo's `group.<app bundle id>` convention |
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
| Sandbox Doppler base config (dry run) | project `example-org-shared`, config `ios_base`, marked inheritable by the dry-run harness (none exists: the sandbox holds only `app-store-connect`) |
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
| 4 | `com.apple.developer.healthkit.access` (clinical records) — only if Sample reads health records | entitlements file | No | manual M3, omitted unless needed |
| 5 | `com.apple.developer.healthkit.background-delivery` (iOS 15+) — only if Sample uses `HKObserverQuery` background delivery | entitlements file | No | manual M3 |
| 6 | `NSHealthShareUsageDescription`, `NSHealthUpdateUsageDescription` | `Info.plist` | No | manual M3 |
| 7 | `healthkit` in `UIRequiredDeviceCapabilities` (Xcode adds it; remove if HealthKit is optional) | `Info.plist` | No | manual M3 |
| 8 | Push on the **host** App ID | ASC API, `PUSH_NOTIFICATIONS`, no setting needed for token auth | **Yes** | document |
| 9 | `aps-environment` entitlement on the host | entitlements file | No | manual M3 |
| 10 | An APNs auth key (`.p8`) for whatever sends Sample's pushes | portal only: spec 4.5 has no keys resource | No | manual M5 |
| 11 | NSE target (`com.apple.usernotifications.service` extension point) | `Info.plist`, `BUILD.bazel` | No | manual M3 |
| 12 | Widget extension target (`com.apple.widgetkit-extension`) | `Info.plist`, `BUILD.bazel` | No | manual M3 |
| 13 | App group registered (`group.com.example-org.sample`) | portal or Xcode; no API | No | manual M2 |
| 14 | `APP_GROUPS` enabled **and the group assigned** on all three App IDs | portal "Configure" or Xcode; the API can flip the flag but cannot assign | No — `appstore.bundle_id_capability.ensure` refuses `APP_GROUPS` on `Absent` by design | manual M2 |
| 15 | `com.apple.security.application-groups` on all three targets | entitlements files | No | manual M3 |
| 16 | Data protection level on the host App ID | ASC API, `DATA_PROTECTION` + `DATA_PROTECTION_PERMISSION_LEVEL` setting | **Yes, after task 1** | document |
| 17 | App Store distribution profiles, one per identifier, minted **after** 2, 8, 14 and 16 | ASC API | **Yes** (`appstore.profile.ensure`) | a later run of the signing document, manual M4 |
| 18 | App Store Connect app record | website only | No | manual M1 |
| 19 | Health privacy: disclose the health data collected; never store personal health information in iCloud | App Review 5.1.3; app content | No | recorded in M3's checklist |
| 20 | Doppler project and configs, inheriting shared base configs | Doppler API | **Yes** | document |
| 21 | Buildkite CI service account can read project `sample` | Doppler; no `doppler.project_member.ensure` yet | No | manual M6 |
| 22 | Buildkite pipeline for Sample pointing at the monorepo | Buildkite API | **Yes** (with the frozen bootstrap) | document |
| 23 | Pipeline's stored bootstrap selects `apps/sample/.buildkite/…` on the monorepo queue | Buildkite settings | No — decision (f) | manual M7 |
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

`/Users/operator/Projects/example-org` (remote `Example-Org/monorepo`), 2026-09-27.

- **Build system.** Bazel with `rules_apple` and `rules_swift`; one `apps/<app>/ios/BUILD.bazel` per
  app. AppTwo (the iOS app with extensions) uses `ios_application` plus `ios_extension` and
  `ios_imessage_extension`, `minimum_os_version = "26.0"`, `families = ["iphone", "ipad"]`, versions from
  `apple_bundle_version`, `infoplists` per target, and entitlements chosen by `select()` — `None` for CI
  and simulator builds, the file otherwise.
- **Signing.** Manual, by profile name: five `local_provisioning_profile` rules in AppTwo — one
  Xcode-managed wildcard team profile for local development, and **four distribution profiles each named
  exactly after its bundle identifier** ("Profile names must match EXACTLY in 3 places: 1. Apple
  Developer Portal profile name 2. This BUILD.bazel profile_name 3. fastlane/Fastfile constants"). That
  is precisely what `appstore-signing-profile-from-doppler.yaml` produces (milestone 3d binds the profile
  name to the identifier).
- **Entitlements.** One file per target under `ios/Resources/<Target>.entitlements`. App groups follow
  `group.<app bundle id>` (AppTwo's host, keyboard and stickers share one; its share extension uses a
  second, suffixed group). The host also carries
  `keychain-access-groups = $(AppIdentifierPrefix)<bundle id>`. App Three (a single-target app)
  carries its capability entitlements the same way.
- **Bundle identifiers.** `com.example-org.<app>` and `com.example-org.<app>.<suffix>` —
  Sample's three follow it.
- **Buildkite.** One pipeline per app in the operator's Buildkite organisation, slug equal to the app
  name. Each app keeps `apps/<app>/.buildkite/` with `pipeline.yml`, an `upload-pipeline.sh` selector,
  `bootstrap.yml` (the **stored** bootstrap, checked in because "Buildkite runs it before the repository
  exists and nothing else records it"), `provider-settings.json` (every provider trigger disabled during
  rollout), and a README. The stored bootstrap sets `GIT_CONFIG_*` job environment to select a host
  credential helper, targets the `ci-macos-apple-silicon` queue, and runs
  `bash apps/<app>/.buildkite/upload-pipeline.sh`, which calls
  `buildkite-agent pipeline upload --no-interpolation <file>`. Guests run under a pinned `vm-ci-plugin` plugin.
- **Doppler.** The existing apps use one Doppler project per app (App Three: project
  `app-three`, CI config `prd_deployment`), handed to guests by the plugin's
  `doppler_token_secret`. No app in the monorepo yet uses config inheritance, so **the real names of the
  operator's shared base configs in that workplace are not observable from here** (verify item 9).
  Team-key names seen there: `APP_STORE_CONNECT_API_KEY_BASE64`, `APP_STORE_CONNECT_API_KEY_ID`,
  `APP_STORE_CONNECT_API_KEY_ISSUER_ID`, `APPLE_DISTRIBUTION_CERTIFICATE_P12_BASE64`,
  `APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD` (names only).
- **`apps/sample/`** exists and is empty.

**Where Sample must follow the pattern** (all of it is manual step M3's content):
`apps/sample/ios/BUILD.bazel` with `ios_application` `Sample` embedding two `ios_extension`s
(`SampleNotificationService`, `SampleWidgets`) — AppTwo's `extensions = []` is a deliberate
exception, Sample's host embeds both; three `local_provisioning_profile`s named exactly after the three
identifiers plus the local wildcard one; `Resources/Sample.entitlements`,
`Resources/SampleNotificationService.entitlements`, `Resources/SampleWidgets.entitlements` selected
`None` for CI and simulator builds; `group.com.example-org.sample` in all three;
`apps/sample/.buildkite/{pipeline.yml, upload-pipeline.sh, bootstrap.yml, provider-settings.json,
README.md}` copied from AppTwo's shape with triggers disabled.

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
`com.example-org.sample`). The output names are `manual_m1_app_record` … `manual_m7_buildkite_bootstrap`
(`^[a-z][a-z0-9_]*$`).

Why the conversion rather than a literal `value`: a literal would name the real identifier during the
dry run, which is misleading on exactly the run whose job is to show the text. Why bind from the input
and not `steps.app_id.identifier`: the input is known at plan time on every path.

**The honest trade.** An output is a report, not a checkpoint: nothing orders a manual step, nothing
records that the operator did it, and a node exists only to carry text. A first-class `manual:` section
in the document format (rendered, journaled, perhaps acknowledged) would be better and is recorded as a
candidate for a later milestone, not designed here. What a manual step leaves behind is still caught
where it matters: an `APP_GROUPS` or profile document run before M2 either converges or refuses loudly.

### (b) The Sample document mints no provisioning profile

**Superseded 2026-09-28** by operator decision 1 (replace-when-INVALID, the T3a addendum) and decision
(j): the second fact below is answered by a gate that passes the identifier through, which is a data
edge. Kept for its history.

Two facts force it. Apple: "Provisioning profiles that contain a modified App ID become invalid." App
groups can only be assigned by hand (M2), after the document has run, so any profile the document
minted would be invalid by the time Sample builds. And milestone 3c's coordinator addendum, item 4: the
graph orders nodes only by data edges, and a profile node consumes nothing a capability node produces,
so "capabilities, then profile" is not expressible. So profiles are manual step **M4**: after M2, run
`workflows/appstore-signing-profile-from-doppler.yaml` once per identifier, which names each profile
after its identifier — the monorepo's own convention. That document needs no change.

### (c) The host identifier alone carries API-enabled capabilities

`HEALTHKIT`, `PUSH_NOTIFICATIONS` and `DATA_PROTECTION` go on `app_identifier`. The NSE rides on its
host's push registration (Apple's NSE and APNs pages above); the widgets read from the app group
container rather than from HealthKit, because "the device encrypts the HealthKit store when the user
locks the device" and widget timelines refresh while locked. If Sample's widgets later read HealthKit
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

### (e) Sample's data protection level is `PROTECTED_UNTIL_FIRST_USER_AUTH`

Chosen from the NSE and background delivery, not from a wish for the strongest class. `COMPLETE`
makes a file "accessible only when the device is unlocked"; the NSE runs when a notification arrives,
typically while locked, and reads what the app left in the group container; HealthKit background
delivery wakes the app while locked. `PROTECTED_UNTIL_FIRST_USER_AUTH` is iOS's default class, so
enabling it changes no behaviour — its value is that the class becomes an explicit, reviewed line in the
document and the profile, and a later tightening is a one-line, visible edit. The operator may prefer
`PROTECTED_UNLESS_OPEN` or `COMPLETE_PROTECTION` with file-level exceptions; that is their call and
costs one input change (risk 3). The same level goes into `Sample.entitlements` as
`com.apple.developer.default-data-protection` (M3).

### (f) Buildkite: the frozen bootstrap stays; reseeding it is manual step M7

The monorepo's stored bootstraps carry three things willikins' frozen `UPLOAD_CONFIGURATION` does not:
a per-app selector script, `GIT_CONFIG_*` job environment for the host checkout, and the Mac queue. A
path port would fix one of the three, would widen milestone 3a's trust boundary 8 ("no other field")
and brush the no-shell-command invariant. The operator already reseeds stored steps by hand per their
own cookbook ("Check the stored step into the repository … Read back stored configuration and compare
it with the checked-in copy before dispatching"), and `buildkite.pipeline.ensure` never compares
`configuration`, so a hand-edited bootstrap survives every re-run as `Present`. So the document creates
the pipeline (org, slug `sample`, cluster, the monorepo) and **M7** replaces its stored bootstrap with
`apps/sample/.buildkite/bootstrap.yml`. Until then the pipeline would upload the root
`.buildkite/pipeline.yml`, which the monorepo does not have — harmless, because no webhook exists until
someone adds one (the Buildkite doc above), and M3 lands Sample's `.buildkite/` files **before** the
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

`naming.v1` with `org` = the monorepo's organisation and `slug` = `sample` yields Doppler project
`sample` and pipeline slug `sample` (the monorepo's slug-equals-app-name pattern). Its `github_repo`
output is deliberately unused: in a monorepo the repository is referenced, not derived. `monorepo:
GitHubRepo` is an input defaulting to `Example-Org/monorepo` (a default is safe here: a wrong repo is
caught by `github.repo.get`, and the dry run overrides it to the stand-in). Bundle-identifier *names*
are bound from the identifiers through the new `AppleBundleIdentifier => AppleBundleIdName` conversion,
the operator's "bundle id everywhere" habit stated in the document, not in any tool — exactly as 3d did
for profile names.

### (i) Doppler: `sample`, `dev`/`stg`/`prd`, inheriting the shared base configs

As `workflows/doppler-ios.yaml`, inlined: `doppler.project.ensure`, `doppler.config.ensure` per
environment, `doppler.config.inherits.ensure` per config with `base_configs` a defaulted list input
(the document format cannot bind a list literal to a port; that header explains why). Default
`[example-org-shared/ios_base]` — a **placeholder shaped like the Lightless Labs one**; the real
names are unknown (verify item 9) and M0 asks the operator to confirm or edit it before a real apply.
The ASC credential chain reads `config` (`app-store-connect/prd`) through the same token, so in a real
apply that config must be in the same workplace as `sample` (one `WILLIKINS_DOPPLER_TOKEN` is one
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
observes reality wherever an API exposes it, reusing the provider's existing client read (Sample's:
`GET /v1/apps` filtered by bundle id, research note section 4; the capability list
`list_bundle_id_capabilities` already paginates), and otherwise takes an operator acknowledgement
(point 6).

**2. Ordering: a gate guards only what consumes its output.** Because edges are data edges, a gate
orders a node only by emitting a typed output that node binds. The rule for gate authors: **a gate
passes through the key it checked.** Sample's app-group gate takes `identifier` and outputs it again,
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
*non-pure* node (Sample's `appstore.profile.ensure`, `identifier` bound from a blocked gate) would
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
a first run whose profiles are all skipped. For Sample that means `--approve` (or an approver) each
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
reports and blocks the run's outcome; it orders nothing. For Sample's acknowledgement steps that is
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
      step: Replace the sample pipeline's stored bootstrap with apps/sample/.buildkite/bootstrap.yml
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

**Sample, for T3 (not designed here).** Two observed gates: the app record exists (a new read-only
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
| M3 | `manual_m3_monorepo_files` | **before** the real apply for `.buildkite/`; any time for the rest | Add Sample's files under `apps/sample/` following the survey above: `BUILD.bazel`, three entitlements files (HealthKit, `aps-environment`, app group, data protection on the host; app group on both extensions; background delivery and clinical records only if used), `Info.plist` with both HealthKit usage strings and the two extension points, privacy manifest, and `.buildkite/` with triggers disabled |
| M4 | `manual_m4_profiles` | after M2 | Run `workflows/appstore-signing-profile-from-doppler.yaml` once per identifier (content into project `sample`, `prd`) |
| M5 | `manual_m5_apns_key` | before Sample sends a push | Confirm the team's APNs auth key (`.p8`) is in a shared base config Sample inherits; if none exists, create one in the portal (Account Holder or Admin; downloadable once) |
| M6 | `manual_m6_ci_doppler_access` | before the pipeline's first signing build | Grant the Buildkite CI Doppler service account read access to project `sample` (no `doppler.project_member.ensure` yet) |
| M7 | `manual_m7_buildkite_bootstrap` | after the apply, after M3's `.buildkite/` lands | Replace the `sample` pipeline's stored bootstrap with `apps/sample/.buildkite/bootstrap.yml`, read it back and compare; keep provider triggers disabled until validated; add the repository webhook only then |

## The Sample document

`workflows/sample-ios-app.yaml`. Inputs: `config: DopplerConfig` (ASC credential; no default),
`app_identifier`, `nse_identifier`, `widgets_identifier: AppleBundleIdentifier` (**no defaults**,
trust boundary 2), `platform: AppleBundleIdPlatform` (default `IOS`), `data_protection:
AppleCapabilitySetting` (default `DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH`),
`org: GitHubOrg` (default `Example-Org`), `slug: ProjectSlug` (default `sample`), `monorepo:
GitHubRepo` (default `Example-Org/monorepo`), `buildkite_org: BuildkiteOrg` (no default),
`cluster: BuildkiteClusterName` (no default), `environments: list<EnvironmentSlug>` (default
`[dev, stg, prd]`), `base_configs: list<DopplerConfig>` (default `[example-org-shared/ios_base]`).

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

A guarded harness, `crates/willikins-cli/tests/live_sample_dry_run.rs` (`live-tests` feature, `#[ignore]`,
`WILLIKINS_LIVE_TESTS=1`), driving the built binary against the live providers:

1. Read-only counts: Apple (bundle ids, profiles, certificates), GitHub sandbox repositories, sandbox
   Doppler projects, sandbox Buildkite pipelines.
2. Set up the sandbox: create `Willikins-Test/monorepo` (through `github.repo.ensure` or the sandbox PAT
   on `curl`'s stdin), create project `example-org-shared` with config `ios_base` and mark it
   inheritable. Record each for teardown.
3. Refuse to continue unless `app_identifier`, `nse_identifier`, `widgets_identifier` are
   `com.willikins.probe.delete-me.<unique>`, `…<unique>.nse`, `…<unique>.widgets`.
4. `plan --live`, then `apply`, with `monorepo=Willikins-Test/monorepo`, `org=Willikins-Test`,
   `buildkite_org=willikins-test`, `base_configs=[example-org-shared/ios_base]`,
   `config=app-store-connect/prd`. Record `app_id`, `nse_id`, `widgets_id` from the apply's outputs
   **into the guard before any assertion**. Assert all eight `manual_*` outputs are present and name the
   throwaway identifier.
5. Re-`apply`: every node `Unchanged`.
6. Teardown in the guard, which runs on every drop: the three Apple identifiers by recorded id
   (deleting an App ID removes its capabilities), the Buildkite pipeline, Doppler projects `sample` and
   `example-org-shared`, the stand-in repository. Then every count equal to step 1, and each
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
8. **The Sample document** checks clean against both catalogs; plans against the fake catalog with
   throwaway inputs; the plan's outputs carry all eight `manual_*` texts, each naming the supplied app
   identifier; each capability node's `identifier` edge comes from `app_id`; no node is a profile; no
   `SinkToken`; a fake `apply` then a second `apply` reads every node `Unchanged`; omitting any identifier
   input fails `plan` naming it.
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
- A Doppler service-account token for the operator's real workplace that can create project `sample`,
  its configs and their inheritance, and read the ASC credential from a config in **that same**
  workplace.
- A GitHub token that can read `Example-Org/monorepo` metadata (a fine-grained PAT, metadata read on
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
10. **Does the host credential helper answer for `Example-Org/monorepo.git` without a
    `GIT_CONFIG_*` override?** The cookbook says it did on the Mini; M7 copies the override regardless.
11. **Which `BundleIdPlatform` the operator's existing identifiers use** (counts only); the document
    defaults to `IOS`.

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
| T3 | **The Sample document** (after G1–G3; **2026-09-28**: its acceptance 8, dry-run step 4 and the post-flight "eight manual steps" item were written for decision (a) and are rewritten by T3 for gates: the observed app-record and app-group gates, profiles behind the app-group gate, the remaining steps as acknowledgement leaves; the Doppler layout of operator decision 2). Commit 1: the two conversion rows with their `From` impls, proptests and the reverse negative fixture (acceptance 7). Commit 2: `workflows/sample-ios-app.yaml` and its graph tests over the fake catalogue in `crates/willikins-cli/tests/sample_document.rs`, any fake-state fixture it needs under `workflows/fixtures/state/` (acceptance 8, 9) | sonnet implements, opus attacks, writes and runs the dry run once a Buildkite token exists |

## Risks

1. **Apple's settings shape differs from decision (d).** The live cycle fails at its first
   `DATA_PROTECTION` create or read; the guard deletes the throwaway; the attacker adapts the parse test
   first. Mocks alone could not have caught it, which is why the live cycle is required.
2. **The key cannot enable capabilities** (`403`). The tools still land on mocks; the milestone is not
   complete until a key that can exists, and the operator is told which role it needs.
3. **The data protection level is the operator's call.** Decision (e) argues for the default class; a
   stricter one is one input change, but `COMPLETE_PROTECTION` needs Sample's code to write NSE-readable
   files with an explicit weaker class.
4. **A manual step is only a report.** *(Addressed 2026-09-28 by decision (j): the app-group gate
   keeps the first run from minting profiles, and a re-run replaces any Apple invalidates later. What
   remains: the API shows `APP_GROUPS` enabled, not the group assigned, so the gate can pass early.)* Skipping M2 before M4 yields profiles without the app group,
   which Apple will invalidate on M2 — recoverable by re-running M4, not silent corruption.
5. **The dry run is blocked on a Buildkite token.** Tasks 1–3 do not need it.
6. **Build time and disk.** Two small crates touched per task; the host's 60 GB target directory and
   the 04:00 cargo-sweep remain the constraint; never gate across the sweep.
