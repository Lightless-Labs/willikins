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

- **Provisioning profiles in the Sample document.** Decision (b): they are minted afterwards, one run
  of `workflows/appstore-signing-profile-from-doppler.yaml` per identifier, once the manual app-group
  step is done.
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

## The named manual steps

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
| T3 | **The Sample document.** Commit 1: the two conversion rows with their `From` impls, proptests and the reverse negative fixture (acceptance 7). Commit 2: `workflows/sample-ios-app.yaml` and its graph tests over the fake catalogue in `crates/willikins-cli/tests/sample_document.rs`, any fake-state fixture it needs under `workflows/fixtures/state/` (acceptance 8, 9) | sonnet implements, opus attacks, writes and runs the dry run once a Buildkite token exists |

## Risks

1. **Apple's settings shape differs from decision (d).** The live cycle fails at its first
   `DATA_PROTECTION` create or read; the guard deletes the throwaway; the attacker adapts the parse test
   first. Mocks alone could not have caught it, which is why the live cycle is required.
2. **The key cannot enable capabilities** (`403`). The tools still land on mocks; the milestone is not
   complete until a key that can exists, and the operator is told which role it needs.
3. **The data protection level is the operator's call.** Decision (e) argues for the default class; a
   stricter one is one input change, but `COMPLETE_PROTECTION` needs Sample's code to write NSE-readable
   files with an explicit weaker class.
4. **A manual step is only a report.** Skipping M2 before M4 yields profiles without the app group,
   which Apple will invalidate on M2 — recoverable by re-running M4, not silent corruption.
5. **The dry run is blocked on a Buildkite token.** Tasks 1–3 do not need it.
6. **Build time and disk.** Two small crates touched per task; the host's 60 GB target directory and
   the 04:00 cargo-sweep remain the constraint; never gate across the sweep.
