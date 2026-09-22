# Milestone 3c: App Store signing — select a distribution certificate, produce an App Store profile

**Created:** 2026-09-22
**Gate:** PRE-FLIGHT CLEAR, 2026-09-22 — the read-only probe found the credential authenticating, at
least one usable distribution certificate, and `GET /v1/profiles` answering `200`. See "Pre-flight
checklist" below.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (type system, tool contract, "policy lives in
the workflow, never in the tool", and the 2026-09-21/22 addenda "Credentials are ports, resolvers are
nodes" and "a provider's filter is a narrowing hint, never a key")
**Research:** `docs/research/2026-09-16-app-store-connect.md` (sections 1 to 3 and "Settled live,
2026-09-22") and `docs/research/2026-09-22-m3c-app-store-signing.md` (this milestone's additions:
`ProfileCreateRequest`, the `profileType`, `profileState` and `CertificateType` enums, TN3125, the
Program Roles profile rows). Every Apple fact below is quoted verbatim in one of the two, with its URL.
**Depends on:** the bundle-identifier provider that landed 2026-09-22 without a plan of its own (see
"Retrospective"), whose `AppstoreClient`, credential ports and paginated exact-compare read this
milestone reuses unchanged.

## Goal

willikins gains two capabilities against App Store Connect:

1. **SELECT** a distribution certificate — read-only, by explicit ports, refusing rather than
   guessing.
2. **PRODUCE** an App Store provisioning profile relating a bundle identifier to that certificate,
   whose content then flows into Doppler through the existing `doppler.secret.set`, so that Buildkite
   (which holds one Doppler service-account token and nothing else) can sign with it.

The milestone is done when every acceptance test below passes and one opt-in live cycle against the
operator's production account has created one throwaway identifier and at most two throwaway
profiles on it, proved the profile is an App Store distribution profile by its own content, settled
the verify list, and deleted everything it created, with certificate, profile and identifier counts
equal before and after.

## Out of scope

- **Certificate creation, modification, activation and revocation.** The operator agreed on
  2026-09-22 that creation is buildable through a CSR chain but deserves its own design pass: whoever
  generates the CSR holds a code-signing private key, which is a class of secret this workspace has
  never held. Until that pass, certificate writes are **structurally absent** from the crate, not
  merely unused (decision (g)).
- **Development, ad hoc, in-house, Developer ID ("DIRECT") and every non-iOS profile type.** Refused
  by type (decision (b)).
- **Devices.** A device is create-only with no delete ("You can only remove registered devices
  through the Apple Developer website"), so nothing in this milestone registers or references one.
- **Replacing a profile.** There is no `PATCH`; a drifted, invalid or expired profile is reported,
  and replacing it is the document's call (decision (d)).
- **The app record, capabilities, and the core `Action::Update` gap** recorded in the design doc's
  2026-09-22 addendum. Unchanged.
- **Buildkite-side installation of the profile** (writing it into a keychain or
  `~/Library/MobileDevice/Provisioning Profiles` on an agent). That is pipeline content in the
  repository, not a willikins tool.
- **Railway.** No Railway command of any kind.

## Trust boundaries (normative, and independent of anything else in this plan)

The Apple account is the operator's **live production developer account**, with real apps and bundle
identifiers. The operator's words: *"don't blow up my existing ASC apps / bundle ids"*. Every lane
working this milestone holds these as absolute:

1. **The only permitted certificate operation is `GET`.** Never create, modify, (de)activate,
   revoke or delete a certificate — not in code, a test, a probe or a script. Revoking a distribution
   certificate is team-wide and "Builds already uploaded to App Store Connect but not yet submitted
   for App Review may be marked as Invalid Binary". Enforced by the guard of decision (g).
2. **Never modify, rename, delete or change a capability** on any identifier, app, profile,
   certificate or device the same guarded test did not create. Never create an app record.
3. **Every created thing carries an unmistakable throwaway name** (SHARED VALUES) followed by
   something unique to the run.
4. **Delete only by the id the same test's own create returned.** Never by name, never by filter.
5. **Read-only first.** Count certificates, profiles and bundle identifiers before any write.
6. **Surprise means stop.** An unexpected status, a filter matching more than asked, a count that
   moved: delete only what this run made, and report rather than improvise.
7. **The live cycle's profile references a real production certificate.** That reference is
   read-only — nothing about the certificate changes when a profile names it, and deleting the
   profile deletes the reference — but it is stated here so no reviewer discovers it.
8. **No value reaches a file, a log, a commit, a command line or a verdict.** The Apple credential is
   resolved from sandbox Doppler in the same command that uses it (the Doppler header on `curl`'s
   stdin, never argv). A certificate's display name, name, serial number or id, and a profile's name,
   uuid, id or content, belong to the operator and appear nowhere but in memory — except the
   throwaway profile's own name, which is by design never a real one.

## SHARED VALUES

Implementers read this table instead of their prompts. Nothing here may be retyped from memory.

| What | Value |
| --- | --- |
| Certificate tool | `appstore.certificate.get` (pure, read-only) |
| Profile tool | `appstore.profile.ensure` |
| Certificate tool inputs | `issuer_id: AppleIssuerId` (no), `key_id: AppleKeyId` (no), `key: AppleSigningKey` (**secret**), `certificate_type: AppleCertificateType` (no), `serial_number: AppleCertificateSerial` (no) — all required |
| Certificate tool outputs | `certificate: AppleCertificateId` (no) |
| Profile tool inputs | `issuer_id: AppleIssuerId` (no), `key_id: AppleKeyId` (no), `key: AppleSigningKey` (**secret**), `identifier: AppleBundleIdentifier` (no), `name: AppleProfileName` (no), `profile_type: AppleProfileType` (no), `certificate: AppleCertificateId` (no, **derived only**) — all required |
| Profile tool outputs | `profile: AppleProfileId` (no), `content: AppleProfileContent` (**secret**) |
| Profile tool key | `identifier`, `name` |
| In-scope profile types | `IOS_APP_STORE` only. `AppleProfileType`'s grammar admits nothing else |
| In-scope certificate types | `DISTRIBUTION`, `IOS_DISTRIBUTION`. `AppleCertificateType`'s grammar admits nothing else |
| Certificate selection | `GET /v1/certificates?filter[certificateType]=<type>&filter[serialNumber]=<serial>&limit=200`, every page, then **byte-exact** compare of `serialNumber` and `certificateType`; exactly one match, unexpired, `activated` not `false` → its id. Zero → `NotFound`. More than one → `Conflict`. Expired or `activated: false` → `Conflict` |
| Profile read | resolve `identifier` to its id with the existing `AppstoreClient::list_bundle_ids` (paginated, byte-exact); then `GET /v1/bundleIds/{id}/profiles?limit=200&fields[profiles]=name,profileType,profileState,expirationDate`, every page; byte-exact compare of `name`; the single match is re-read by `GET /v1/profiles/{id}?include=certificates&fields[profiles]=name,profileType,profileState,expirationDate,profileContent,certificates` |
| Profile create | `POST /v1/profiles`, `data.type` `profiles`, attributes `name` + `profileType`, relationships `bundleId` (one) + `certificates` (exactly one), **no `devices` key** |
| Credential in Doppler | sandbox workplace, config path **`app-store-connect/prd`**, secrets **`ASC_API_KEY_ISSUER_ID`**, **`ASC_API_KEY_ID`**, **`ASC_API_KEY_BASE64`** (the P-256 key, base64-wrapped). **Not** in `~/.config/willikins/sandbox.env`, which holds only the Doppler token that unlocks them |
| Read-only probe gating | `#[ignore]` + **`WILLIKINS_LIVE_PROBE=1`**, no feature (`tests/live_probe.rs`, `appstore_signing_probe`) |
| Live write cycle gating | the crate's existing **`live-tests`** feature (`[[test]] required-features`) + `#[ignore]` + **`WILLIKINS_LIVE_TESTS=1`** |
| Throwaway bundle identifier prefix | `com.willikins.probe.delete-me.` followed by a per-run unique suffix (`<pid>-<unix-seconds>`) |
| Throwaway profile name prefix | `willikins-probe-delete-me-` followed by the same per-run unique suffix |
| Cleanup order | **profiles first**, each by the id its own create returned; **then** the identifier, by the id its own create returned; then counts (certificates, profiles, bundle identifiers) asserted equal to before. Profile-first because "Provisioning profiles that contain a deleted App ID become invalid" |
| Certificate-write guard | `crates/willikins-providers-appstore/tests/no_certificate_writes_guard.rs` |
| Error text for 401 | new `willikins_providers_http::UNAUTHENTICATED` (decision (a)); 403 keeps `MISSING_PERMISSION` byte for byte |
| Positive document | `workflows/appstore-signing-profile-from-doppler.yaml` |

## Pre-flight checklist (doors and corners)

Filled by the planner on 2026-09-22 from `crates/willikins-providers-appstore/tests/live_probe.rs`'s
`appstore_signing_probe`, run once with the credential resolved from sandbox Doppler in the same
command. Every call was a `GET`. Counts and statuses only.

**Portal and account**
- [x] **(c) The credential authenticates.** `GET /v1/bundleIds` answered `200` with a freshly minted
  ES256 JWT.
- [x] **(d) At least one certificate usable for App Store distribution exists.** 5 certificates
  (`meta.paging.total` agrees): 4 `DEVELOPER_ID_APPLICATION_G2`, 1 `DISTRIBUTION`. The `DISTRIBUTION`
  one is unexpired; no `IOS_DISTRIBUTION` or `MAC_APP_DISTRIBUTION` exists. So exactly **1** usable
  certificate, consistent with the operator having deleted distribution certificates earlier
  today (they reported holding three at once).
- [x] **(e) `GET /v1/profiles` answers `200`** for this key (not `403`). 13 profiles
  (`meta.paging.total` agrees), **all `IOS_APP_STORE`**: 11 `ACTIVE`, 2 `INVALID`. None is expired by
  date — the 2 `INVALID` ones are invalid for some reason other than expiry. Every one of the 13 has
  exactly 1 certificate, of type `DISTRIBUTION`, and 0 devices. `profileContent` length: 16240 to
  18960 characters.
- [ ] **Permission to CREATE a profile: unknown.** A `200` on a list proves read access only. The
  Program Roles grid gives "Create and delete distribution provisioning profiles" to Account Holder
  and Admin, and to App Manager only with Certificates, Identifiers & Profiles access; Developer's
  cell is blank — while "Download provisioning profiles" reaches Developer-with-access. A key that can
  list can therefore still be one that cannot create. **The live write cycle (task 3) is the first
  thing that will know.** The key did create and delete a bundle identifier on 2026-09-22, which
  needs a comparable grant, so a refusal would be a surprise — and per trust boundary 6, a surprise
  stops the cycle.

**Facts the probe settled on the way** (no name, serial or id was printed)
- [x] `activated` is **absent** on all 5 certificates even when requested through
  `fields[certificates]`. The selection read must treat absent as "not deactivated" and only an
  explicit `false` as deactivated.
- [x] `serialNumber` is **uppercase hexadecimal**, 30 to 32 characters, on all 5.
- [x] **`filter[serialNumber]` matches by substring.** Whole string, strict prefix and strict suffix
  each returned the original certificate (1 row each). Same answer, by the same method, as
  `filter[identifier]`. The byte-exact compare in the selection read is load-bearing.
- [x] The one `DISTRIBUTION` certificate's `name` begins with Apple's label `Apple Distribution`, and
  none of the 4 Developer ID certificates' does. `DISTRIBUTION` ↔ "Apple Distribution" is observed;
  `IOS_DISTRIBUTION` ↔ "iOS Distribution" is not (no such certificate on the team).
- [x] All 13 existing `IOS_APP_STORE` profiles have no device relationship and are signed by a
  `DISTRIBUTION` certificate — consistent with TN3125's definition, observed on real profiles rather
  than inferred from the enum's name.

**Project**
- [x] The credential's three secret names exist at `app-store-connect/prd` and resolve (the probe
  could not have authenticated otherwise).
- [x] `live-tests` feature and `WILLIKINS_LIVE_TESTS=1` already gate `tests/live_write_cycle.rs`.
- [x] `profileContent` (≤ 18960 characters observed) fits `DopplerSecretValue`'s and the new
  `AppleProfileContent`'s 65536-character bound with 3.4× headroom.
- [ ] Doppler's own per-value size limit — undocumented; settled in task 3 (verify item 7).

## Decisions

### (a) The 401/403 split

**Today** `crates/willikins-providers-http/src/http.rs`'s `provider_error_from_body` returns
`MISSING_PERMISSION` ("the credential is missing a permission this request needs") for both statuses,
and `error.rs`'s `From<ProviderError> for ToolError` repeats it in a `Some(401 | 403)` arm. The status
itself survives in `ProviderError::status` — which is how the probe can print `401` versus `403` — but
the *message*, the only thing a `ToolError` and an agent ever see, is identical. That is how a lane
once believed it had verified live: a malformed or expired JWT and a role without permission read the
same.

**Decision.** Split the message, keep dropping the provider's body for both.

- `401` → a new public constant `UNAUTHENTICATED`: *"the provider did not accept the credential
  itself (401): it is malformed, expired, or revoked"*. No "permission" in it, deliberately — the
  point is that it reads differently.
- `403` → `MISSING_PERMISSION`, byte for byte unchanged.
- Both built where the error is built (`provider_error_from_body`) and repeated in the `ToolError`
  conversion, exactly as today. The body is still never parsed for either.

**Honest limit, specific to Apple.** Apple's own status table says a `403` covers "your API key is
revoked, your token is incorrectly formatted, or … the requested operation is not allowed", while its
specification also declares `401` per operation. So for App Store Connect a `403` is not proof of a
role problem; the split is honest about the **status**, not the cause. The appstore crate says so in
its client doc; the shared message stays status-shaped.

**Tests that assert the old wording and must change deliberately** (each in its own commit with the
new assertion first, failing, then the change):
- `crates/willikins-providers-http/src/error.rs` — `permission_message_never_carries_the_body_on_401`
  asserts `contains("permission")` on a `401`. Becomes: the `401` message is `UNAUTHENTICATED`, carries
  no body, and differs from the `403` message.
- `crates/willikins-providers-http/tests/adversarial_messages.rs` —
  `a_401_or_403_body_never_reaches_the_provider_error_message` loops `401` and `403` asserting
  `contains("permission")` for both. Split: `401` asserts `UNAUTHENTICATED`, `403` keeps `permission`.
- `crates/willikins-providers-doppler/tests/provider_messages.rs` —
  `a_401_or_403_body_never_reaches_the_message_even_as_messages` loops both asserting
  `contains("permission")`. Same split.

**Tests that stay green unchanged, and why** (all are `403`-only, and `403` keeps its words):
`crates/willikins-providers-http/src/http.rs` `a_403_is_never_retried_regardless_of_headers`
(`assert_eq!(err.message, MISSING_PERMISSION)`); `crates/willikins-providers-buildkite/tests/cluster_get_mock.rs`
`a_403_is_provider_naming_the_missing_permission_and_never_the_credential`;
`crates/willikins-providers-github/tests/repo_ensure_mock.rs` `a_bare_403_is_a_missing_permission_message_not_a_rate_limit`
and the two secondary-rate-limit tests asserting a rate-limited `403` does **not** say "permission";
`crates/willikins-providers-signoz/tests/ingestion_key_ensure_mock.rs`
`a_403_is_the_fixed_missing_permission_message_and_echoes_no_scope_text` (asserts only that no scope
text is echoed); `crates/willikins-providers-appstore/tests/bundle_id_ensure_mock.rs`
`read_maps_a_403_to_a_provider_error_naming_no_credential` (asserts only the kind). The comment in
`http.rs` about a redirect coming back `401` "and be reported as a missing permission" is updated to
the new wording. New: a `401` test per provider crate is **not** added — the shared crate's tests
cover the mapping once, which is where it lives.

### (b) Scope: App Store distribution profiles only, refused by type

Development and ad hoc profiles embed devices, and a device is create-only with no delete. A
half-supported development profile would register devices the operator can remove only by hand, and a
live test of one would leave a device behind on a production team. So those types are **refused by
type**, not accepted and then failed: `AppleProfileType`'s grammar is the single member
`IOS_APP_STORE`, a document asking for `IOS_APP_DEVELOPMENT` fails `check` with a `ParseError` naming
`AppleProfileType`, and the create body carries no `devices` key at all (pinned by a JSON matcher).

**Why `IOS_APP_STORE` and not every `*_APP_STORE` member.** Apple documents no member of the
`profileType` enum. The classification rests on what Apple *does* define — TN3125: "App Store
distribution profiles have no `ProvisionedDevices` property" — observed on the operator's 13 existing
`IOS_APP_STORE` profiles (0 devices each) and proved again on the live cycle's own profile by its
content. `TVOS_APP_STORE`, `MAC_APP_STORE` and `MAC_CATALYST_APP_STORE` are candidates, each admitted
later by adding one grammar alternative plus the same live content test, which is additive and needs
no redesign. `IOS_APP_INHOUSE` is excluded outright: TN3125 gives In-House profiles
`ProvisionsAllDevices`.

### (c) The certificate tool: read-only, explicit ports, refuse on zero and on many

`appstore.certificate.get` is pure and read-only, modelled on `buildkite.cluster.get`: `ensure` is
the identity of `read`, it creates nothing, and its output is not an idempotence key.

**Selection ports: `certificate_type` and `serial_number`, both required.** Reasoning:

- Certificates are **team-scoped** with no bundle-identifier filter and no relationship to one, so
  nothing project-derived can find "this project's certificate". The document must name it.
- `displayName` cannot discriminate. Apple: "a signing certificate name contains a hint to the type,
  and includes the team name and Team ID" — so every certificate of one type on one team has the same
  name by construction, and the operator reported holding three "Apple Distribution" certificates at once earlier today.
- The opaque `id` is invisible in the portal and in the keychain; an operator cannot tie it to the
  private key they actually hold.
- **The serial number is the one key visible from the private-key side**: it is in the `.p12` Buildkite
  signs with (`openssl x509 -serial`) and in Keychain Access. A profile must embed the certificate whose
  private key the signer holds, or signing fails at build time; naming the serial is how the document
  says so. It is public (it is in the certificate), so it is a non-secret port.
- `certificate_type` is required alongside so a serial pasted from the wrong certificate kind is
  refused, and so the read can narrow by `filter[certificateType]` too.

**The read.** `filter[serialNumber]` is proven substring (pre-flight), so it is a narrowing hint: every
page is read and `serialNumber` and `certificateType` are compared byte for byte. Exactly one match that
is unexpired and not `activated: false` → `Present` with its id. **Zero** → `ToolError::NotFound`
naming the type and saying no certificate of that type has that serial (never echoing other
certificates). **More than one** → `ToolError::Conflict` naming the count, never the ids. **Expired or
deactivated** → `ToolError::Conflict` saying so. No certificate is ever picked because it is the only
one: that is exactly the guess the design rejects ("correct until someone adds a second one"), and this
team had three earlier today.

The profile tool's `certificate` port is `exact_derived_only("AppleCertificateId")`, the pattern
`doppler.secret.set`'s `config` uses: a document cannot paste a certificate id literal and route around
the selection tool's refusals.

### (d) No `PATCH`: ensure creates or reports; drift is terminal

The specification declares only `DELETE` and `GET` on `/v1/profiles/{id}`. So `ensure` can only create
or report. A profile whose `profileType` differs → `Observation::Mismatch { port: profile_type }`; whose
certificate relationship is not exactly `[certificate]` → `Mismatch { port: certificate }`. Both are
terminal at `plan()` (`PlanError::AttributeMismatch`), which is the design's refuse-do-not-reconcile
rule and also the only thing `plan()` can do with a `Mismatch` today. **Replacing a profile is the
document's call** — delete it by hand or give the document a new `name` (a rotation date, the new
certificate's serial suffix) — per the project's rule that policy lives in the workflow. The tool never
deletes a profile; only the live test's cleanup does, by its own create's id.

**How an INVALID or EXPIRED profile is observed, and why.** `profileState`'s enum is `ACTIVE | INVALID`
— there is no `EXPIRED` — so expiry is not a state Apple reports. The read therefore checks both,
independently: `profileState == INVALID` → `ToolError::Conflict` ("exists but Apple reports it
INVALID"), and `expirationDate` at or before the wall clock → `ToolError::Conflict` ("expired on
<date>"), each saying willikins cannot repair a profile and the document must replace it. Why both:
the pre-flight saw 2 `INVALID` profiles whose dates are in the future, so `INVALID` has causes other
than expiry (a deleted App ID is one Apple names; a revoked certificate is the likely other), and it saw
no expired profile at all, so whether Apple flips an expired one to `INVALID` is unobserved. A `Conflict`
rather than a `Mismatch`, because no input port is wrong: the profile is. The date is parsed with
`chrono` (already a workspace dependency, already in `Cargo.lock`), added to the crate's dependencies.

### (e) The profile's key and its read

**Key: `(identifier, name)`.** The identifier scopes it; the name is what the operator sees in the
portal and what an `exportOptions.plist`'s `provisioningProfiles` map references.

**The read uses the relationship, not the filter.** `GET /v1/bundleIds/{id}/profiles` is a
relationship read, scoped by construction to the identifier the tool already resolved exactly;
`filter[name]` on `/v1/profiles` is team-wide and, by every filter this provider has probed so far
(`filter[identifier]`, `filter[serialNumber]`), almost certainly a substring hint. So `filter[name]` is
**unused**. The relationship read still pages (`limit=200`, `links.next`, query string re-attached to
its own path exactly as `list_bundle_ids` does) and still compares `name` byte for byte.

- identifier not registered (yet) → `Absent`, outputs predicted `Unknown` — the identifier is
  typically created by an earlier node of the same plan, and `identifier` (a string known at plan
  time) rather than the bundle id's opaque id is what keeps the key known, since `plan()` refuses a
  node whose key port is `Unknown`.
- zero exact name matches → `Absent`.
- **two or more exact matches → `ToolError::Conflict`**, naming the count. Name uniqueness is unknown
  (verify item 1); if Apple permits duplicates, the tool refuses rather than choosing.
- one → the instance read of the SHARED VALUES row; then (d)'s checks; then `Present` with `profile`
  and `content`.

**`ensure` on `Absent`.** Resolve the identifier's id (now present), `POST /v1/profiles`, never retried
(the shared client's rule). **Any create failure is resolved by re-reading, never by parsing the error
body** (the Buildkite rule): `Present` → `changed: false`; `Mismatch`/`Conflict` → that; still `Absent`
→ the original error. On `201`, `profile` and `content` come from the response; if the `201` carries no
`profileContent` (verify item 5), one `GET /v1/profiles/{id}` follows.

### (f) `profileContent`'s type: `AppleProfileContent`, **secret**

A profile holds no private key: its content is a CMS-signed property list carrying the public
certificate, entitlements, team and App ID prefix, name, uuid and expiry. Possessing it does not let
anyone sign; the `.p12` does. So why secret:

1. **It must reach `doppler.secret.set`, whose `value` port is `any_secret(true)`**, and `any_secret`
   rejects a non-secret scalar (`willikins-core/src/value.rs`, `any_secret_rejects_a_non_secret_scalar`).
   Non-secret would need that port widened or a "declare secret" tool — both weaken the one invariant
   (a secret output only ever binds to a secret-accepting input) that the whole type system protects.
2. **Over-classification is recoverable, under-classification is not.** A secret value is redacted in
   plans, the journal and rendered output by construction. If the planned secrecy-inference milestone
   (`todos/2026-09-22-secrecy-inference.md`) later decides this value is public, it can relax it; a
   19,000-character blob already written into a journal as plaintext cannot be un-written.
3. **It is still the operator's data** — entitlements and team identifiers — and a 16–19 KB base64 blob
   in a rendered plan is noise an agent should not read.

**Grammar:** standard base64 (`[A-Za-z0-9+/]+={0,2}`), 1 to **65536** characters — the same bound as
`DopplerSecretValue` and `OpaqueSecret`, so anything that parses fits the Doppler sink's own type.
Observed on the live account: **16240 to 18960 characters** across 13 `IOS_APP_STORE` profiles, 3.4×
headroom. App Store profiles carry no device list, which is what makes the size stable. Doppler's own
per-value limit is undocumented (verify item 7). Like every secret type: no plain `Display`, no
`Serialize`, `expose(&SinkToken)` only — `doppler.secret.set`'s existing `object.expose(token)` reaches
it unchanged.

### (g) Certificate writes are structurally absent: a source-scanning guard

`crates/willikins-providers-appstore/tests/no_certificate_writes_guard.rs`, in the manner of
`crates/willikins-cli/tests/no_gh_writes_guard.rs`: walk every `.rs` file under
`crates/willikins-providers-appstore/` (`src/` **and** `tests/`, excluding the guard itself), strip
comments, split into statements on `;` and `}`, collapse whitespace, and fail on any statement that
contains both a write-method token — `.post(`, `.patch(`, `.delete(`, `"POST"`, `"PATCH"`, `"DELETE"`
— and a certificates **path** (`/certificates`, as a path segment, not the JSON relationship key
`"certificates"` a profile create legitimately carries). Statement-level, not line-level, so a call
split across lines (`self.http.post(\n &format!("/v1/certificates/{id}"))`) is caught. Its own unit
tests prove it is not vacuous: a multi-line POST, a `mock("DELETE", "/v1/certificates/X")`, and a
PATCH through `format!` are each flagged; a profile create body naming the `certificates`
relationship, and `GET /v1/certificates` reads, are not. Like its two siblings it catches the honest
mistake, not a determined one (a path assembled from fragments at runtime), and says so in its doc.
Written **first**, so every later line of this milestone is written under it.

### (h) The live write cycle's shape and cleanup order

One test, `tests/live_write_cycle.rs` extended (feature `live-tests`, `#[ignore]`,
`WILLIKINS_LIVE_TESTS=1`), credential resolved from sandbox Doppler in the same command:

1. **Read-only counts first:** certificates, profiles, bundle identifiers, each by paginating and
   counting rows, cross-checked against `meta.paging.total`, reported as counts only.
2. **Certificate selection through `appstore.certificate.get`.** The harness needs a serial and must
   not guess one: if exactly one usable certificate of type `DISTRIBUTION` exists (the pre-flight
   found exactly one), the harness reads its serial into memory and passes it; if more than one
   exists, it requires `WILLIKINS_LIVE_ASC_CERTIFICATE_SERIAL` from the operator and stops without it.
   The serial is never printed.
3. **One throwaway identifier** `com.willikins.probe.delete-me.<pid>-<unix>` through
   `appstore.bundle_id.ensure`; its id recorded in the guard immediately.
4. **Profile one** `willikins-probe-delete-me-<pid>-<unix>`, `IOS_APP_STORE`, through
   `appstore.profile.ensure` → `changed: true`; its id recorded in the guard **before any
   assertion**. Assert on its content, in memory: base64-decodes; the decoded bytes contain
   `ExpirationDate` and **neither `ProvisionedDevices` nor `ProvisionsAllDevices`** (TN3125's App Store
   test, a substring test on the plaintext plist inside the CMS envelope); length ≤ 65536; report the
   length and the `profileState` only.
5. Re-`read` → `Present`; re-`ensure` → `changed: false`.
6. **Profile two, only to settle name uniqueness:** a second `POST /v1/profiles` through the client
   (not the tool, which would read `Present`) with the **same name** on the same identifier. `201` →
   names are not unique per identifier: record its id in the guard, and the tool's `Conflict` arm
   becomes load-bearing; any `4xx` → report the status and the `errors[].code` leaf only. Never more
   than these two profiles.
7. **Cleanup, guarded, in this order:** every recorded profile id, by `DELETE /v1/profiles/{id}`;
   then the recorded identifier id. A `Drop` guard runs the same deletions if any assertion fails,
   so a failed step still cleans up. Deletions by recorded id only — never by name or filter.
8. **Counts after** equal counts before, for all three. An independent read afterwards (its own JWT)
   confirms the throwaway identifier and both profile ids answer `404`.

Optional, task 3's call and sandbox-only: write the throwaway profile's content into a throwaway
config in the **sandbox** Doppler workplace through `doppler.secret.set`, read it back, compare
SHA-256 digests (never the value), delete the throwaway config — the method the SigNoz lane used
(`docs/research/2026-09-21-signoz-doppler-sink-adversarial-pass.md`). This settles verify item 7.

## The type table

| Type | Grammar | Secret | Why a type, not a `String` |
| --- | --- | --- | --- |
| `AppleCertificateType` | `DISTRIBUTION\|IOS_DISTRIBUTION` | no | Closed: the two members that can sign an `IOS_APP_STORE` profile (Apple's certificate table: "Apple Distribution" and "iOS Distribution" submit to App Store Connect; `DISTRIBUTION` ↔ "Apple Distribution" observed live). A Developer ID or development type is refused at parse time |
| `AppleCertificateSerial` | `[0-9A-F]{1,64}` | no | Observed uppercase hex, 30–32 characters on all 5 certificates. Uppercase only because the compare is byte-exact and a lowercase paste would read `NotFound` for a certificate that exists; the bound is generous above the observed length. It reaches a query string, so the grammar also keeps `&` and `=` out of it |
| `AppleCertificateId` | `[A-Za-z0-9]{2,64}` | no | Opaque; same undocumented-grammar reasoning as `AppleBundleIdId`. Reaches a request body only |
| `AppleProfileType` | `IOS_APP_STORE` | no | Decision (b): the refusal of every other type is the grammar |
| `AppleProfileName` | 1–255 characters, no control, invisible or bidirectional character — `AppleBundleIdName`'s rule, hand-written the same way | no | A human-written name compared byte-exact on `read` and quoted in errors; Apple states no bound |
| `AppleProfileId` | `[A-Za-z0-9]{2,64}` | no | Opaque; the handle cleanup deletes by |
| `AppleProfileContent` | base64, 1–65536 characters | **yes** | Decision (f) |

## The tool table

| Tool | Inputs | Outputs | Key | Class | Pure |
| --- | --- | --- | --- | --- | --- |
| `appstore.certificate.get` | `issuer_id`, `key_id`, `key`, `certificate_type`, `serial_number` | `certificate: AppleCertificateId` | — | `Reversible` | yes |
| `appstore.profile.ensure` | `issuer_id`, `key_id`, `key`, `identifier`, `name`, `profile_type`, `certificate` (derived only) | `profile: AppleProfileId`, `content: AppleProfileContent` (secret) | `identifier`, `name` | `Reversible` | no |

`Reversible` for the profile: a profile can be deleted and recreated with no effect beyond its
`uuid`, and nothing a profile create does touches the certificate or the identifier.

## The positive document

`workflows/appstore-signing-profile-from-doppler.yaml`: the credential resolved exactly as
`workflows/appstore-bundle-id-from-doppler.yaml` resolves it; `appstore.bundle_id.ensure`;
`appstore.certificate.get` from inputs `certificate_type` and `serial_number`;
`appstore.profile.ensure` binding `identifier` from the bundle id node, `certificate` from the
certificate node; a `doppler.config.ensure` for the destination (because `doppler.secret.set`'s
`config` port is derived-only); and `doppler.secret.set` binding `value` ←
`steps.profile.content`, under a `SecretName` input the document chooses (policy in the workflow).
Negative fixtures under `workflows/fixtures/`, each with a header naming its acceptance test and exact
error: a development profile type; a literal certificate id bound to `certificate`; the profile's
`content` bound to a non-secret port.

## Acceptance tests

1. **401/403 split** — decision (a)'s three changed tests plus: `401` and `403` messages differ;
   neither carries the body; `403` is still `MISSING_PERMISSION` exactly.
2. **Certificate-write guard** — the tree is clean; its synthetic cases flag and pass as decision (g)
   lists.
3. **Types** — each type accepts its example and refuses: `AppleCertificateType` refuses
   `DEVELOPER_ID_APPLICATION_G2`, `DEVELOPMENT`, lowercase; `AppleCertificateSerial` refuses lowercase,
   `&`, empty, 65 characters; `AppleProfileType` refuses every other `profileType` member by name;
   `AppleProfileContent` refuses a non-base64 character and 65537 characters, and does not implement
   `Display` or `Serialize` (trybuild).
4. **`appstore.certificate.get` against a mock** — one exact match → `Present` with the id; the
   filter returning a substring neighbour only → `NotFound`; two exact matches → `Conflict` naming `2`
   and no id; an expired match → `Conflict`; `activated: false` → `Conflict`; `activated` absent →
   `Present`; an exact match on page 2 → `Present`, with requests showing the query re-attached to its
   own path; `401` → `UNAUTHENTICATED`; `403` → `MISSING_PERMISSION`. Every request recorded is a `GET`.
5. **`appstore.profile.ensure` reads against a mock** — identifier absent → `Absent`; no exact name →
   `Absent`; a substring-neighbour name only → `Absent`; two exact names → `Conflict`; wrong type →
   `Mismatch { profile_type }`; wrong or two certificates → `Mismatch { certificate }`; `INVALID` →
   `Conflict`; expired by date while `ACTIVE` → `Conflict`; healthy → `Present` with `profile` and a
   secret `content`; a match on page 2 of the relationship read → found.
6. **The create body** — a JSON matcher pins exactly `data.type`, `attributes.{name, profileType}`,
   `relationships.bundleId.data`, `relationships.certificates.data` with one element, and **no
   `devices` key**; exactly one `POST` (never retried).
7. **Ambiguous create** — `POST` `500` then re-read `Present` → `changed: false`; re-read `Absent` → the
   original error; re-read `Conflict` → `Conflict`.
8. **Redaction** — a profile content carrying a distinctive marker never appears in an output's
   rendering, a `ToolError`, a `Debug`, the journal or captured `tracing`.
9. **Fake parity** — `willikins-providers-fake` gains both tools with equal `ToolSpec`s (insta), and a
   `fake_agrees_with_live` comparison across every read arm, seeded from the live crate's constants.
10. **Catalogs** — `LIVE_TOOL_NAMES` grows from 23 to 25 in `crates/willikins-server/src/catalog.rs`;
    both catalogs validate against the registry; every site that pins the count or the list
    (`crates/willikins-providers-doppler/tests/live_catalog.rs` among them) is updated in the same commit.
11. **Documents** — the positive document checks clean against both catalogs; each negative fixture
    fails with its named error; a fake apply creates the profile and writes its content through
    `doppler.secret.set`, and the rendered run shows the redaction marker, never content.
12. **The read-only probe** stays green and read-only (`appstore_signing_probe`).
13. **The live write cycle**, decision (h), run once by task 3.

## Post-flight checklist (the verifier fills this)

**Does it actually work?**
- [ ] The live cycle created a profile (`201`) — i.e. the key **can** create profiles.
- [ ] The created `IOS_APP_STORE` profile's decoded content has no `ProvisionedDevices` and no
  `ProvisionsAllDevices` (TN3125's App Store test).
- [ ] `read` after create → `Present`; re-`ensure` → `changed: false`.
- [ ] Profile content length recorded, and ≤ 65536.
- [ ] (Optional) the content round-tripped through sandbox Doppler by SHA-256, and the throwaway
  config is gone.

**Did review miss the basics?**
- [ ] Certificate count, profile count and bundle-identifier count equal before and after.
- [ ] Independent read: the throwaway identifier and every throwaway profile id answer `404`.
- [ ] No certificate call other than `GET` in the crate (the guard is green and non-vacuous).
- [ ] No name, serial, id, uuid or content of the operator's appears in any commit, doc or log kept.
- [ ] `401` and `403` read differently in a `ToolError`, and neither carries the body.
- [ ] Every verify item below is answered or explicitly carried forward.

**Operational readiness**
- [ ] The research note's "Settled live" section and this plan's verify list updated in place.
- [ ] The README names the two tools, the in-scope types, and the certificate-selection ports.
- [ ] The operator knows the one usable certificate's serial is what their documents must name, and
  that it must be the certificate whose `.p12` Buildkite holds.

## Verify before relying on them

Only the live cycle can settle these; none may be frozen into code before it runs.

1. **Is a profile `name` unique?** Per identifier, settled by profile two (decision (h) step 6). Per
   team is **not** settled by this cycle — it would need a second throwaway identifier — and stays
   open; the tool's `Conflict` arm covers either answer.
2. **Can this key create a profile?** Unknown until the first `POST` (pre-flight).
3. **Is `IOS_APP_STORE` an App Store distribution type by TN3125's content test?** The 13 existing
   profiles are consistent; the cycle's own profile is the proof.
4. **Does an expired profile read `INVALID`, or `ACTIVE` with a past date?** No expired profile exists
   on the account and the cycle cannot make one (a profile's expiry is Apple's choice). Stays open;
   the read checks both.
5. **Does the `201` carry `profileContent`?** Settled by the create.
6. **What does a duplicate-name create return, status and `errors[].code` leaf?** Settled by profile
   two if names are unique.
7. **Doppler's own per-value size limit** against ~19,000 characters. Settled only if task 3 runs the
   optional sandbox Doppler round trip.
8. **`IOS_DISTRIBUTION` ↔ "iOS Distribution"**: not observable — no such certificate on the team.
   Admitted by the grammar on Apple's certificate table's word, carried forward.
9. **What makes an unexpired profile `INVALID`?** Two exist on the operator's account. Not probed
   further: reading their relationships would need their ids in a harness, and nothing in this
   milestone depends on the cause.
10. **Is `filter[name]` on `/v1/profiles` substring too?** Not needed (decision (e) does not use it);
    recorded so no later lane mistakes it for a key.

## Gates

All four before every commit, exactly as CLAUDE.md spells them, bare `cargo`, each in the background
with a 600,000 ms timeout, reading the log body and never piping it through `tail` or `tee`:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
```

`pgrep -x cargo` before every cargo command, and wait while it prints anything (an unrelated daemon
runs its own cargo). Never two cargo commands at once, including a scoped clippy beside a test run.
Run `cargo fmt` and a crate-scoped clippy over what was touched before the full gate.

## Tasks

One lane at a time on `main`; no worktrees. Three agents, in order.

| # | Task | Delegate to |
| --- | --- | --- |
| 0 | This plan, the research note, and the pre-flight probe (done 2026-09-22) | planner |
| 1 | **Credentials and certificates.** (i) The certificate-write guard, test-first (acceptance 2) — first, so everything after is written under it. (ii) The 401/403 split (acceptance 1), changing decision (a)'s three tests deliberately, one behaviour per commit. (iii) `AppleCertificateType`, `AppleCertificateSerial`, `AppleCertificateId` in `willikins-types`, registry entries, tests (acceptance 3, those three). (iv) `AppstoreClient::list_certificates` — `GET` only, paginated, byte-exact — and `appstore.certificate.get` with its mock tests (acceptance 4). (v) Its fake twin and parity (acceptance 9, certificate half); `LIVE_TOOL_NAMES` 23 → 24 | sonnet implements, opus verifies |
| 2 | **Profiles.** (i) `AppleProfileType`, `AppleProfileName`, `AppleProfileId`, `AppleProfileContent` (acceptance 3, those four, including the trybuild case). (ii) Client calls `list_bundle_id_profiles`, `get_profile`, `create_profile`, and a `pub` `delete_profile` used only by the live test. (iii) `appstore.profile.ensure` and its mock tests (acceptance 5 to 8). (iv) Fake twin and parity (acceptance 9, profile half); `LIVE_TOOL_NAMES` 24 → 25 (acceptance 10). (v) The positive document and the three negative fixtures (acceptance 11) | sonnet implements, opus verifies |
| 3 | **Verify and fly.** (i) Adversarial pass over tasks 1 and 2, recorded under `docs/research/`; every bypass becomes a fixture plus a test. (ii) Extend `tests/live_write_cycle.rs` per decision (h) and run it **once**, under every trust boundary above; stop on any surprise. (iii) Optionally the sandbox Doppler round trip. (iv) Fill the post-flight checklist and the verify list in place; update the research note's "Settled live". The plan's `Completed` header is the coordinator's, after re-gating | opus |

## Risks

- **The key may not be allowed to create a profile.** The cycle would take a `403` on the first
  `POST`; it deletes the throwaway identifier and reports. The tools still land on mock tests; the
  milestone is not complete until a key that can create one exists.
- **Exactly one usable certificate exists.** If the operator deletes it, `appstore.certificate.get`
  reads `NotFound` and the cycle cannot run — which is the tool working. If they add a second, the
  harness requires `WILLIKINS_LIVE_ASC_CERTIFICATE_SERIAL` rather than choosing.
- **A profile names a certificate; revoking that certificate is what most plausibly made two existing
  profiles `INVALID`.** Not willikins' doing and not something it can do (trust boundary 1), but it is
  why the read treats `INVALID` as terminal and loud.
- **`chrono` enters the appstore crate.** Already a workspace dependency and in `Cargo.lock`; no new
  third-party crate.
- **Build time on this host.** One more tool pair in an already-built crate; `-j 2` stays load-bearing.

## Retrospective: the bundle-identifier provider had no plan

`crates/willikins-providers-appstore` (`appstore.bundle_id.ensure`,
`appstore.bundle_id_capability.ensure`, the three credential ports, `doppler.value.get`,
`apple.issuer_id.parse`/`apple.key_id.parse`) landed on 2026-09-22 without a dedicated plan in
`docs/plans/`, a departure from the monorepo rule of a plan per milestone. Its record is the design
doc's 2026-09-22 addendum — `docs/plans/2026-09-11-willikins-design.md`, the bullets beginning
"`willikins-providers-appstore` lands the first two real Apple tools" and "`filter[identifier]`
matches by substring, so the read paginates" — and the research note's "Settled live, 2026-09-22".
That record stands as written and is not duplicated here. What this plan inherits from it: the
per-call client with no credential at construction, the paginated byte-exact read, the recorded
`Action::Update` gap, and the lesson that live harnesses must be written against where the credential
actually lives (Doppler), not where a prompt assumed it did.
