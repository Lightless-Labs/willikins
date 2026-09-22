# App Store Connect research: certificates and App Store provisioning profiles (milestone 3c)

**Created:** 2026-09-22
**Plan:** `docs/plans/2026-09-22-milestone-3c-app-store-signing.md`
**Previous:** `docs/research/2026-09-16-app-store-connect.md` — this note adds only what that one
does not already settle. Its section 3 already quotes the certificate and profile operation lists,
the CSR requirement, the missing profile `PATCH`, and the four `GET /v1/profiles` filters; its
"Settled live, 2026-09-22" section already records that `filter[identifier]` matches by substring
and that Apple's published one-distribution-certificate-per-team limit is wrong.


## Method

Primary sources only, fetched verbatim on 2026-09-22:

1. **The OpenAPI specification**, `https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip`
   (HTTP 200, 260282 bytes, inner file dated 2026-07-15, `"info": {"title": "App Store Connect API",
   "version": "4.4.1"}` — the same file the previous note read). Schemas and parameters were
   extracted with a JSON query over `components.schemas` and `paths`; quotes below are those
   extractions.
2. **The DocC JSON twins** under `https://developer.apple.com/tutorials/data/documentation/appstoreconnectapi/<path>.json`
   and the Markdown twins (`<doc URL>.md`). For every schema this note needed, the Markdown twin
   carries only the abstract; the DocC JSON carries the property list and `allowedValues`. **Neither
   carries a single word of description for any `profileType`, `profileState` or `CertificateType`
   member** — Apple's own `CertificateType` page lists eighteen names under "Possible Values", each
   with an empty description.
3. **Apple's help pages** under `https://developer.apple.com/help/account/`, fetched as HTML and
   reduced to text by stripping tags (there is no `.md` twin for `/help/`), and **TN3125** as its
   Markdown twin.
4. **The Program Roles grid** at `https://developer.apple.com/support/roles/`, whose cells carry their
   meaning in an `alt` attribute on a `<figure>` icon, read from the HTML. This recovers the
   profile rows the previous note reported as lost to text extraction.


## 1. `ProfileCreateRequest`: required attributes and relationships

- Two attributes, both required: `name` and `profileType`. Two relationships required,
  `bundleId` and `certificates`; `devices` is the only optional one. `data.type` is the fixed enum
  `[profiles]`. (source: specification, `components.schemas.ProfileCreateRequest`)
  - "\"attributes\": {\"type\": \"object\", \"properties\": {\"name\": {\"type\": \"string\"}, \"profileType\": {\"type\": \"string\", \"enum\": [...]}}, \"required\": [\"profileType\", \"name\"]}, \"relationships\": {\"type\": \"object\", \"properties\": {\"bundleId\": {...}, \"devices\": {...}, \"certificates\": {...}}, \"required\": [\"certificates\", \"bundleId\"]}"
  - The DocC twin agrees: "PROP bundleId: ProfileCreateRequest.Data.Relationships.BundleId (required) / PROP certificates: ProfileCreateRequest.Data.Relationships.Certificates (required) / PROP devices: ProfileCreateRequest.Data.Relationships.Devices" (<https://developer.apple.com/tutorials/data/documentation/appstoreconnectapi/profilecreaterequest/data-data.dictionary/relationships-data.dictionary.json>)
- `certificates` is an **array** in the schema, but Apple's help page says an App Store profile holds
  exactly one. (source: <https://developer.apple.com/help/account/provisioning-profiles/create-an-app-store-provisioning-profile/>)
  - "Select your distribution certificate, then click Continue. An App Store provisioning profile contains a single distribution certificate. Enter a profile name, then click Generate."
- `POST /v1/profiles` declares `201, 400, 401, 403, 409, 422, 429` — the same boilerplate set as
  every create in the previous note, so `409`'s presence says nothing profile-specific. The DocC
  twin's 409 row reads "Conflict — The provided resource data is not valid." (source: specification;
  <https://developer.apple.com/tutorials/data/documentation/appstoreconnectapi/post-v1-profiles.json>)
- `DELETE /v1/profiles/{id}` declares `204, 400, 401, 403, 404, 429`. Apple's own discussion:
  "You can delete provisioning profiles, and may wish to do so if they are expiring or obsolete."
  (source: <https://developer.apple.com/documentation/appstoreconnectapi/delete-v1-profiles-_id_.md>)


## 2. The `profileType` enum, and which members are App Store distribution

- The full enum, fourteen members, identical in `ProfileCreateRequest`, `Profile.attributes` and
  the `filter[profileType]` parameter. (source: specification)
  - "\"profileType\": {\"type\": \"string\", \"enum\": [\"IOS_APP_DEVELOPMENT\", \"IOS_APP_STORE\", \"IOS_APP_ADHOC\", \"IOS_APP_INHOUSE\", \"MAC_APP_DEVELOPMENT\", \"MAC_APP_STORE\", \"MAC_APP_DIRECT\", \"TVOS_APP_DEVELOPMENT\", \"TVOS_APP_STORE\", \"TVOS_APP_ADHOC\", \"TVOS_APP_INHOUSE\", \"MAC_CATALYST_APP_DEVELOPMENT\", \"MAC_CATALYST_APP_STORE\", \"MAC_CATALYST_APP_DIRECT\"]}"
- **Apple documents no member of this enum.** The DocC property carries `allowedValues` and nothing
  else; there is no `ProfileType` page, and the specification carries no description on the
  property. Which members are "App Store distribution" is therefore **not stated by any API
  source**, and this note does not infer it from the `_APP_STORE` suffix.
- What Apple *does* define is what an App Store distribution profile **contains**, which is a test
  that can be run against a created profile's own content rather than against its name.
  (source: <https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles.md>)
  - "Most profiles apply to a specific list of devices. This is encoded in the `ProvisionedDevices` property: [...] App Store distribution profiles have no `ProvisionedDevices` property because you can't run an App Store distribution signed app locally."
  - "Developer ID and In-House (Enterprise) distribution profiles have the `ProvisionsAllDevices` property, indicating that they apply to all devices."
- And the portal names the kind: "Create an App Store Connect provisioning profile … Uploading an
  app to App Store Connect requires an app record registered with an explicit App ID. You can create
  your own App Store Connect provisioning profile with an explicit App ID to use when you upload your
  app to App Store Connect. … Required role: Account Holder or Admin." (source: the help page above)
- **Consequence, recorded as a decision in the plan rather than a fact here:** the milestone admits
  exactly one member, `IOS_APP_STORE`, and its live cycle proves membership by content — the
  created profile has no `ProvisionedDevices` and no `ProvisionsAllDevices`, per TN3125 — rather
  than by name. The other three `*_APP_STORE` members are candidates, admitted one at a time, each
  only after the same content test.


## 3. What a profile read returns: `profileState` and `expirationDate`

- `Profile.attributes`: `name`, `platform`, `profileType`, `profileState`, `profileContent`, `uuid`,
  `createdDate`, `expirationDate`. (source: specification, `components.schemas.Profile`)
  - "\"profileState\": {\"type\": \"string\", \"enum\": [\"ACTIVE\", \"INVALID\"]}, \"profileContent\": {\"type\": \"string\"}, \"uuid\": {\"type\": \"string\"}, \"createdDate\": {\"type\": \"string\", \"format\": \"date-time\"}, \"expirationDate\": {\"type\": \"string\", \"format\": \"date-time\"}"
- **`profileState` has two members, `ACTIVE` and `INVALID`, and no `EXPIRED`.** Same in
  `filter[profileState]`: "filter[profileState] query array enum= ['ACTIVE', 'INVALID']". So expiry is
  not a state this API reports; it is a date the reader must compare against a clock. Whether
  Apple flips an expired profile to `INVALID` or leaves it `ACTIVE` with a past `expirationDate` is
  stated nowhere; the pre-flight probe tallies existing profiles by state against their dates, and
  the plan's read checks both.
- One Apple-stated cause of `INVALID`: "Provisioning profiles that contain a deleted App ID become
  invalid." (source: <https://developer.apple.com/help/account/identifiers/delete-an-app-id/>, already
  quoted in the previous note, section 2). This is why cleanup deletes profiles first.
- The expiry itself: "Every profile has an `ExpirationDate` property which limits how long the
  profile remains valid. […] This validity period varies by profile type, but it's typically not
  more than a year." (source: TN3125)
- `profileContent` is a bare `{"type": "string"}` — no `format`, no `maxLength`. The operation
  names say what it is: "List and download profiles — Find and list provisioning profiles and
  download their data." TN3125 describes the downloaded file as a CMS-signed property list, whose
  one non-plaintext property is `DeveloperCertificates`. No Apple source bounds its size; the probe
  measures the operator's own.
- The read-back surfaces available, from the specification's paths and parameters:
  - `GET /v1/profiles` — filters `filter[name]`, `filter[profileType]`, `filter[profileState]`,
    `filter[id]`; `limit` up to 200; `include` of `bundleId`, `devices`, `certificates`;
    `fields[profiles]` enumerating every attribute and relationship. **No `filter[bundleId]`.**
  - `GET /v1/bundleIds/{id}/profiles` — "List all profiles for a bundle id — Get a list of all
    profiles for a specific bundle ID." Takes `fields[profiles]` and `limit` (max 200); **no
    `include`, no filter**. A relationship read, so scoped by construction rather than by a filter
    of unknown matching semantics.
  - `GET /v1/profiles/{id}` — `include` of `bundleId`, `devices`, `certificates`; `404` declared.
  - `GET /v1/profiles/{id}/certificates` and `.../relationships/certificates` — "List all
    certificates in a profile" / "List certificate IDs for a profile".
  (source: specification; <https://developer.apple.com/documentation/appstoreconnectapi/profiles.md>)


## 4. `CertificateType`, and which types can sign an App Store profile

- The full enum, eighteen members. (source: specification, `components.schemas.CertificateType`)
  - "\"enum\": [\"APPLE_PAY\", \"APPLE_PAY_MERCHANT_IDENTITY\", \"APPLE_PAY_PSP_IDENTITY\", \"APPLE_PAY_RSA\", \"DEVELOPER_ID_KEXT\", \"DEVELOPER_ID_KEXT_G2\", \"DEVELOPER_ID_APPLICATION\", \"DEVELOPER_ID_APPLICATION_G2\", \"DEVELOPMENT\", \"DISTRIBUTION\", \"IDENTITY_ACCESS\", \"IOS_DEVELOPMENT\", \"IOS_DISTRIBUTION\", \"MAC_APP_DISTRIBUTION\", \"MAC_INSTALLER_DISTRIBUTION\", \"MAC_APP_DEVELOPMENT\", \"PASS_TYPE_ID\", \"PASS_TYPE_ID_WITH_NFC\"]"
  - Apple's page for the type: "CertificateType — Literal values that represent types of signing
    certificates." and eighteen "Possible Values" with **empty descriptions**
    (<https://developer.apple.com/tutorials/data/documentation/appstoreconnectapi/certificatetype.json>).
- `Certificate.attributes`: `name`, `certificateType`, `displayName`, `serialNumber`, `platform`,
  `expirationDate`, `certificateContent`, `activated`. `GET /v1/certificates` filters:
  `filter[displayName]`, `filter[certificateType]`, `filter[serialNumber]`, `filter[id]`. No bundle
  identifier filter and no relationship to one: certificates are team-scoped. (source: specification)
- **Which types can sign an App Store profile: stated only in portal terms, never in API terms.**
  Apple's certificate table names the portal types whose purpose includes App Store submission.
  (source: <https://developer.apple.com/help/account/certificates/certificates-overview/>)
  - "Apple Distribution — Distribute your iOS, iPadOS, macOS, tvOS, visionOS, watchOS app on devices on designated devices for testing or submit it to App Store Connect."
  - "iOS Distribution — Distribute your iOS, iPadOS, tvOS, or watchOS app on designated devices for testing or to submit it to App Store Connect. For use with Xcode 11 and earlier."
  - "Mac App Distribution — Sign a Mac app before submitting it to the Mac App Store."
  - And on revocation: "iOS Distribution Certificate (App Store) — If your Apple Developer Program membership is valid, your existing apps on the App Store won't be affected. However, you'll no longer be able to upload new apps or updates signed with the expired or revoked certificate to App Store Connect. Builds already uploaded to App Store Connect but not yet submitted for App Review may be marked as Invalid Binary if they were signed with a revoked certificate."
- The mapping from those portal labels to the enum (`Apple Distribution` → `DISTRIBUTION`,
  `iOS Distribution` → `IOS_DISTRIBUTION`, `Mac App Distribution` → `MAC_APP_DISTRIBUTION`) is
  **stated by no Apple source fetched**. The pre-flight probe checks it on the live account without
  printing a name: per `certificateType`, it counts how many certificate `name`s begin with Apple's
  own label `Apple Distribution` or `iOS Distribution`, and it records which certificate types
  actually sign the account's existing profiles of each `profileType`.
- Why `displayName` cannot pick one certificate out of several of the same type, in Apple's words
  from the same page: "Note: In your keychain, a signing certificate name contains a hint to the
  type, and includes the team name and Team ID." Type plus team is identical for every certificate
  of one type on one team.
- `activated` is a boolean the certificate can carry, and the only thing `PATCH
  /v1/certificates/{id}` changes: "Modify a Certificate — Update the activation status for a specific
  certificate." (previous note, section 3). A deactivated certificate is not a usable one.


## 5. Who may create a distribution profile

The Program Roles grid's profile rows, read from the `alt` text of each cell's icon, in column order
Account Holder, Admin, App Manager, Developer, Finance, Marketing, Sales, Customer Support.
(source: <https://developer.apple.com/support/roles/>)

- "Create and delete distribution provisioning profiles || Full access. || Full access. || Requires access to Certificates, Identifiers and Profiles, which can be provided in App Store Connect. || (blank) || (blank) || (blank) || (blank) || (blank)"
- "Create development provisioning profiles || Full access. || Full access. || Requires access to Certificates, Identifiers and Profiles, which can be provided in App Store Connect. || Requires Xcode Automatic Signing. || (blank) …"
- "Download provisioning profiles || Full access. || Full access. || Requires access to Certificates, Identifiers and Profiles, which can be provided in App Store Connect. || Requires access to Certificates, Identifiers and Profiles, which can be provided in App Store Connect. || (blank) …"

These describe users. The key's own role is not visible through the API, and the previous note's
open question — whether an API key can be granted the App Manager toggle — stands. The practical
reading: a key that can list profiles (a Developer-with-access key could) is not thereby a key that
can create one (Developer's cell is blank). **A `200` on `GET /v1/profiles` proves read access
only**, which is why the plan's live write cycle, not the probe, is the first thing that will know.


## Still unresolved after this pass

Carried into the plan's verify list, which the live cycle settles or reports:

- Whether profile `name` is unique per team, per identifier, or not at all.
- Whether an expired profile reads `INVALID`, or `ACTIVE` with a past `expirationDate`.
- The enum-to-label mapping for the three distribution certificate types.
- Whether `IOS_APP_STORE` is an App Store distribution type by TN3125's content test.
- Whether `POST /v1/profiles`' `201` response carries `profileContent`, or a read is needed after.
- `profileContent`'s real size, against `DopplerSecretValue`'s 65536-character bound and Doppler's
  own undocumented per-value limit.

## Settled by the read-only pre-flight, 2026-09-22

`crates/willikins-providers-appstore/tests/live_probe.rs`, `appstore_signing_probe`, `GET` only,
counts and statuses only (the plan's pre-flight checklist has the full tallies):

- **`filter[serialNumber]` matches by substring.** Whole string, strict prefix and strict suffix of
  one real serial each returned that certificate. Same method, same answer as `filter[identifier]`.
- `serialNumber` is uppercase hexadecimal, 30 to 32 characters, on all 5 certificates.
- `activated` is **absent** from every certificate even when named in `fields[certificates]`.
- `DISTRIBUTION` certificates carry `name`s beginning `Apple Distribution` (1 of 1).
- All 13 profiles on the account are `IOS_APP_STORE`, each with 0 devices and exactly 1 certificate
  of type `DISTRIBUTION`; 2 are `INVALID` with unexpired dates, so `INVALID` has causes other than
  expiry. `profileContent` is 16240 to 18960 characters.
