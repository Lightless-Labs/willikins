# App Store Connect API research: identifiers, app groups, app records

**Created:** 2026-09-16
**Plan:** none yet — App Store Connect is a *future* provider. The table shape below is borrowed
from `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md` and is a sketch, not a design.
**Previous:** `docs/research/2026-09-12-m2-dependencies.md`


## The question, and the answer in three lines

The operator, 2026-09-16, verbatim:

> "you'll have the AppStore Connect API to explore. See if it allows creating identifiers, app
> groups, and apps."

**Identifiers — yes.** Apple's own operation list for the resource opens with a create:
"[`Register a new bundle id`](/documentation/AppStoreConnectAPI/POST-v1-bundleIds)"
(<https://developer.apple.com/documentation/appstoreconnectapi/bundle-ids>), and the specification
declares `/v1/bundleIds -> ['GET', 'POST']` and `/v1/bundleIds/{id} -> ['DELETE', 'GET', 'PATCH']`
— the full create/read/update/delete quartet.

**App groups — no.** Apple's own Provisioning topic list enumerates exactly seven resources and
App Groups is not among them: "**Provisioning.** Manage bundle IDs, capabilities, signing
certificates, devices, and provisioning profiles. … Bundle IDs / Bundle ID Capabilities /
Certificates / Devices / Profiles / Merchant ID / Pass type Ids"
(<https://developer.apple.com/documentation/appstoreconnectapi.md>). Corroborating: the string
`appGroup` occurs zero times in the 966-path specification. A group is registered in the portal or
in Xcode by an Account Holder or Admin, and no endpoint creates one, reads one back, or attaches
one to an identifier. (The `APP_GROUPS` *capability flag* can be switched on — see section 2 —
but that enables the entitlement class without naming a group, so it does not make the answer
"partly".)

**Apps — no.** The apps resource publishes five operations and none of them creates a record:
"Getting and modifying app information: \"List apps\" … (GET /v1/apps); \"Read app information\" …
(GET /v1/apps/{id}); \"Modify an app\" … (PATCH /v1/apps/{id}); \"Read an app's encryption
declarations\"; \"Read an app's encryption declaration ids\""
(<https://developer.apple.com/documentation/appstoreconnectapi/apps>). The specification agrees:
the only operationIds under those paths are `apps_getCollection` and `apps_updateInstance`, and
`components.schemas` holds an `AppUpdateRequest` with no `AppCreateRequest`. An app record is
created on the App Store Connect website.

So: **two of the three.** A willikins App Store Connect provider could own the bundle identifier
lifecycle outright and could converge everything hanging off an app record that already exists —
locales, categories, versions and platforms, pricing, availability, TestFlight — but it cannot
create the app record and it cannot touch app groups at all.


## Method

Four research passes run in parallel on 2026-09-16 (auth and the credential; bundle identifiers
and capabilities; app groups and the sibling provisioning resources; app records), in the same
format as the milestone 2 note: facts each carrying a source URL and a verbatim quote, then an
unresolved list. A fact taken from a rendered page's paraphrase, a summarising fetch, or a
third-party report rather than bytes read directly is marked **(unverified)**. Anything that could
not be settled verbatim is repeated in "Verify with a browser before relying on them" and appears
in no recommendation.


Apple's documentation site is a JavaScript shell: a plain fetch of a docs URL returns an empty
page. Three working primary-source channels, worth recording for the next pass:

1. **The published OpenAPI specification.** `https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip`
   downloads with no login (HTTP 200, `application/zip`, 260282 bytes, inner file dated
   2026-07-15). It declares `"title": "App Store Connect API", "version": "4.4.1"` and
   `servers: [{"url": "https://api.appstoreconnect.apple.com/"}]`, and carries 966 paths. 4.4.1 is
   also the newest entry in Apple's release-notes index, so the specification is not lagging the
   prose. This is the strongest source available for this API: it settles operation lists, enums,
   required fields and immutability mechanically, and every "no such endpoint" claim below is a
   query over it rather than a failure to find a page.
2. **The Markdown twin.** Appending `.md` to a documentation path —
   `https://developer.apple.com/documentation/appstoreconnectapi/profiles.md` — returns clean text
   with Apple's own topic lists, Notes and cross-references resolved.
3. **The DocC JSON twin** at `https://developer.apple.com/tutorials/data/documentation/<path>.json`,
   flattened locally. The path segment is case-sensitive (`AppStoreConnectAPI`, `GET-v1-apps`).

Help pages under `/help/` and `/support/` have no `.md` twin; those were fetched as HTML and the
sentences recovered by stripping tags. `developer.apple.com/forums` serves a bot-verification
interstitial to `curl`, so any forum quote here is a rendering, not bytes, and is marked
**(unverified)**.

Two corrections made against the summarising fetch tool during this pass, both worth remembering:
a rendered summary of the Program Roles page misreported which roles may create app records (the
real cell contents live in the `alt` attribute of a `<figure>` in the table and were read from the
HTML), and a rendered summary of a forum thread produced a quote about the `.p8` file's encoding
that does not appear in that page's bytes at all. Both summaries were discarded.

No authenticated call was made to any Apple API. There is no App Store Connect credential in this
session and none was sought. Everything below is documentation.


## 1. The credential and authentication

**Recommendation:** an App Store Connect **Team** key, generated in the web UI by an **Admin**,
is the only credential that reaches the endpoints this question is about — Apple states flatly
that Individual keys cannot call Provisioning endpoints, and bundle identifiers are Provisioning
endpoints. The credential is a triple (issuer-ID UUID, key ID, P-256 private key) of which only
the last is secret; there is no handshake, no token endpoint and no refresh — the client mints and
self-signs an ES256 JWT locally, with a **20-minute hard ceiling** for anything willikins would
do. The key itself can never be provisioned, rotated or revoked through the API: the specification
has no path and no schema for it. A Team key reaches every app on the team regardless of its role,
and the rate limit is per key, so one shared Apple credential across projects means one shared
authority and one shared rolling-hour budget.

- The developer generates an App Store Connect API key in the web UI, under Users and Access >
  Integrations; there are two kinds, and generating a Team key requires an Admin account. (source: <https://developer.apple.com/documentation/appstoreconnectapi/creating-api-keys-for-app-store-connect-api>)
  - "An API key has two parts: a public portion that Apple keeps, and a private key that you download. You can use the private key to sign tokens that authorize access to your data in App Store Connect and the Apple Developer website.\n\nThere are two types of API keys:\n\n-Team: Access to all apps, with varying levels of access based on selected roles.\n-Individual: Access and roles of the associated user. Individual keys aren't able to use Provisioning endpoints, access Sales and Finance, or `notaryTool`.\n[...]\nApp Store Connect API keys are unique to the App Store Connect API and you can't use them for other Apple services.\n[...]\nTo generate team keys, you must have an Admin account in App Store Connect."
- The key cannot be created, read or revoked through the API. Across the specification's 966 paths, nothing matches `apiKey` / `api-key` / `keys`, and no schema name matches `ApiKey`. The credential is an out-of-band human prerequisite: willikins can consume it and can never provision it. (source: <https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip>)
  - "\"info\": {\"title\": \"App Store Connect API\", \"version\": \"4.4.1\"}, \"servers\": [{\"url\": \"https://api.appstoreconnect.apple.com/\"}], \"security\": [{\"itc-bearer-token\": []}] — and, checked programmatically over the spec's 966 paths: API-KEY PATHS: []   API-KEY SCHEMAS: []"
- The credential has three parts: an issuer-ID UUID (team keys only), a key ID that goes in the JWT header's `kid`, and the private key file. (source: <https://developer.apple.com/documentation/appstoreconnectapi/generating-tokens-for-api-requests>)
  - "|`kid` - Key Identifier |Your private key ID from App Store Connect, for example, `2X9R4HXF34` |\n[...]\n|`iss` - Issuer ID|Your issuer ID from the API Keys page in App Store Connect, for example, `57246542-96fe-1a63-e053-0824d011072a`|\n[...]\n> Note:\n> Individual keys don't use the Issuer ID key `iss`, but do require the Subject key `sub.`"
- The private key downloads exactly once and Apple keeps no copy. There is no recovery path — only revoke and regenerate, which produces a new key ID. (source: <https://developer.apple.com/documentation/appstoreconnectapi/creating-api-keys-for-app-store-connect-api>)
  - "Once you generate your API key, you can download the private half of the key. The private key is available for download a single time [...] The download link only appears if you haven't downloaded the private key. Apple doesn't keep a copy of the private key."
- ES256 is mandatory, with no alternative offered — which fixes the key as an EC P-256 private key. (source: same token page)
  - "|`alg` - Encryption Algorithm|`ES256` ![](spacer) All JWTs for App Store Connect API must be signed with ES256 encryption.|"
- The audience claim is the literal string `appstoreconnect-v1`, the same for both key kinds; the signed JWT goes in an HTTP bearer header, and Apple explicitly recommends reusing one token until it expires rather than minting one per request. (source: same token page)
  - "|`aud` - Audience|`appstoreconnect-v1`|" / "> Tip:\n> You don't need to generate a new token for every API request. To get better performance from the App Store Connect API, reuse the same signed token for multiple requests until it expires.\n[...]\ncurl -v -H 'Authorization: Bearer [signed token]' \"https://api.appstoreconnect.apple.com/v1/apps\""
- Token lifetime is `exp` minus `iat`, and 20 minutes is the ceiling for anything willikins does. Longer tokens are accepted only for GET-only scopes over a fixed list of Xcode Cloud and analytics resources — Build Actions, Build Runs, Git References, Issues, macOS Versions, Products, Providers, Power and Performance Metrics and Logs, Pull Requests, Repositories, Test Results, Workflows, Xcode Versions. No provisioning or app resource is on that list. (source: same token page)
  - "For every request, App Store Connect calculates the valid time for a token, referred as the token's `lifetime`, by subtracting the `iat` claim from the `exp` claim.\n[...]\nFor most requests, App Store Connect rejects a token with a lifetime greater than 20 minutes. However, it accepts long-lived tokens for some inherently safe requests if:\n\n- The token defines a scope.\n- The scope only includes GET requests.\n- The resources in the scope allow long-lived tokens."
- A token may carry an optional `scope` claim narrowing it to particular requests. Note what Apple's description of an entry names, and does not: only the GET method. It is therefore **not** established that a write can be scope-narrowed, which matters because every willikins `ensure` is a POST, PATCH or DELETE. (source: same token page)
  - "The scope claim is an array of strings, each representing a request. Each scope entry includes:\n\n- The HTTP `GET` method\n- The URL path, for example, `/v1/apps` or `/v1/ciWorkflows/1234`\n- The optional URL query string, for example, `?filter[platform]=IOS`\n\nApp Store Connect rejects a token with a scope claim if none of the scope entries match the attempted request.\n[...]\nYou can use a JWT without a `scope` for any request as long as the role of the API key allows it."
- A key's role is drawn from the same set as team user roles, chosen once at generation time. (source: <https://developer.apple.com/documentation/appstoreconnectapi/creating-api-keys-for-app-store-connect-api>)
  - "When you create an API key, assign it a role that determines the key's access to areas of the App Store Connect API and permissions for performing tasks. For example, keys with the Admin role have broad permissions and can do things like create new users and delete users. Team API keys can access all apps, regardless of their role. The roles that apply to keys are the same roles that apply to users on your team."
- The Program Roles grid, read from the page's HTML (the checkmark cells carry their meaning in a `<figure>`'s `alt` attribute, so this is the literal cell content, not an interpretation): Account Holder and Admin have full access to register, configure and delete App IDs; App Manager can, but only with Certificates, Identifiers & Profiles access granted; Developer may register and configure only through Xcode Automatic Signing and may not delete at all. (source: <https://developer.apple.com/support/roles/>)
  - "Register and configure App IDs || {Full access.} || {Full access.} || {Requires access to Certificates, Identifiers and Profiles, which can be provided in App Store Connect.} || {Requires Xcode Automatic Signing.} || (blank) …\n\nDelete App IDs || {Full access.} || {Full access.} || {Requires access to Certificates, Identifiers and Profiles, which can be provided in App Store Connect.} || (blank) …"
- Same grid, app records: Account Holder, Admin and App Manager have full access; Developer and Marketing need a separately granted permission. (source: same roles page)
  - "Create app records || {Full access.} || {Full access.} || {Full access.} || {Requires access to create app records, granted in Users and Access} || (blank) || {Requires access to create app records, granted in Users and Access.} || (blank) || (blank)"
  - Note the help pages quoted in section 3 say instead "Required role: Account Holder or Admin" for the *website* flows. Those describe the portal, not an API key. Admin is the only role unconditionally sufficient for both App IDs and app records; whether an API **key** (as opposed to a user) can receive the App Manager Certificates, Identifiers & Profiles toggle is unverified — key creation offers only "Under Access, select the role for the key" with no second toggle.
- Revocation is irreversible and Admin-only for team keys; revoked keys stay listed for 30 days. (source: <https://developer.apple.com/documentation/appstoreconnectapi/revoking-api-keys>)
  - "Revoke an API key immediately if it becomes inactive, lost, or compromised. A revoked API key denies access to the App Store Connect API on your organization's behalf.\n\n> Important:\n> Once you revoke an API key, you can't reinstate it. Revoked keys are displayed for 30 days on the API Keys page under the Revoked heading.\n[...]\nTo revoke a team API key, log in to App Store Connect with an Admin account."
- Apple documents **no** rotation mechanism, no key expiry and no overlap window. The only primitive is that a team may hold several keys at once, so an overlap rotation is a manual three-step web-UI procedure: generate a second key, swap the consumer, revoke the first. (source: same pages; the absence is the finding)
  - "To generate team keys, you must have an Admin account in App Store Connect. You can generate multiple API keys with any roles you choose." — and no page in the App Store Connect API documentation set uses the word "rotate" or describes a key expiry.
- The specification declares a single global security scheme: HTTP bearer, `bearerFormat: JWT`, one server. No OAuth flow, no refresh token, no token endpoint, no introspection. (source: <https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip>)
  - "\"security\": [{\"itc-bearer-token\": []}] , \"components\": {\"securitySchemes\": {\"itc-bearer-token\": {\"type\": \"http\", \"scheme\": \"bearer\", \"bearerFormat\": \"JWT\"}}}"
- Auth failures: Apple's prose status-code table has **no 401 row at all**, and 403 covers a revoked key, a malformed token and a disallowed operation alike. But the machine-readable specification *does* declare a 401 per operation ("401 | Unauthorized error(s)" appears in every create's response list, section 2). The two sources disagree in emphasis, not in fact: the prose table omits 401, the spec declares it. A client should therefore handle both and must not branch on the status alone — the `ErrorResponse.code` property is the only programmatic discriminator. (source: <https://developer.apple.com/documentation/appstoreconnectapi/about-the-http-status-code>)
  - "| 403 | Forbidden | The request is not allowed. This can happen if your API key is revoked, your token is incorrectly formatted, or if the requested operation is not allowed. |\n[...]\n| 429 | Too Many Requests | The request cannot be accepted because you have exceeded the rate limit for your API key. You will need to wait awhile and try the request again. |"
- The rate limit is **per key**, not per project, and is reported on every response. One shared Apple credential across projects is one shared rolling-hour budget. (source: <https://developer.apple.com/documentation/appstoreconnectapi/identifying-rate-limits>)
  - "The App Store Connect API limits the volume of requests that you can submit within a specified timeframe. The limits apply to requests you send using the same API key.\n[...]\nEvery response from the API includes an `X-Rate-Limit` HTTP header. Its value has the form:\n\nuser-hour-lim:3500;user-hour-rem:500;\n[...]\nIf you exceed a per-hour limit, the API rejects requests with an HTTP 429 response, with the `RATE_LIMIT_EXCEEDED` error code."
- A team key cannot be narrowed to one app. Only an individual key is app-limited, and only by inheriting its owner's visibility — and individual keys cannot touch Provisioning endpoints. So the operator cannot have both per-app narrowing and identifier management from one key. (source: <https://developer.apple.com/documentation/appstoreconnectapi/creating-api-keys-for-app-store-connect-api>)
  - "Team API keys can access all apps, regardless of their role.\n[...]\n> Note:\n> Team keys give access that's not isolated to a single app, but individual key access is tied to the apps and permissions of the user."
- What the downloaded file *is*, in Apple's own words, is thinner than the folklore. The App Store Connect API pages never state the encoding — only "the private key you downloaded". The nearest Apple sentence is on the general account Keys help page and says only that it is a text file with a `.p8` extension. Apple does not say, on any page fetched here, that it is PEM-encoded PKCS#8. (source: <https://developer.apple.com/help/account/keys/create-a-private-key/>)
  - "Optionally, click Download to generate and download the key now. If you download the key, it's saved as a text file with a .p8 file extension in the Downloads folder. Click Done. WARNING: Save this file in a secure place because the key is not saved in your developer account and you won't be able to download it again."
  - A rendered summary of Apple Developer Forums thread 128616 returned a quote attributed to an Apple engineer reading "a `.p8` file, which a PEM-encoded PKCS#8 private key", but a raw fetch of that thread contains zero occurrences of "p8" or "PKCS" — the forum body is JavaScript-rendered and the summary may have come from elsewhere. **(unverified — discarded, and repeated in the verify list.)** It matters: `jsonwebtoken`'s `from_ec_pem` refuses anything that is not PKCS#8.
- **Correction to a premise this pass was handed.** The workspace does *not* already pin `jsonwebtoken` 11 with an asymmetric allowlist. There is no `jsonwebtoken` in any `Cargo.toml` and none in `Cargo.lock`; the pin and the allowlist discipline are a recorded decision in `docs/research/2026-09-16-m2c-authorization.md`, not yet code. The useful form of the claim is that the same crate the 2c lane has chosen can also sign App Store Connect tokens, and it holds: with `rust_crypto` + `use_pem`, ES256 runs over the pure-Rust `p256` crate with no OpenSSL and no cmake. One hard constraint: the EC key must be PKCS#8 (PEM tag `PRIVATE KEY`); a SEC1 `EC PRIVATE KEY` PEM is explicitly refused. (source: <https://raw.githubusercontent.com/Keats/jsonwebtoken/v11.0.0/src/pem/decoder.rs>)
  - From `src/encoding.rs`: "/// If you are loading a ECDSA key from a .pem file\n/// This errors if the key is not a valid private EC key\n/// Only exists if the feature `use_pem` is enabled.\n///\n/// # NOTE\n///\n/// The key should be in PKCS#8 form." — from `src/pem/decoder.rs`: "// No \"EC PRIVATE KEY\" … tag @ (\"PRIVATE KEY\" | \"PUBLIC KEY\" | \"CERTIFICATE\") => {" and "/// Can only be PKCS8\n    pub fn as_ec_private_key(&self) -> Result<&[u8]> {\n        match self.standard {\n            Standard::Pkcs1 => Err(ErrorKind::InvalidKeyFormat.into())," — from `src/crypto/rust_crypto/ecdsa.rs`: "use p256::pkcs8::DecodePrivateKey;" … "define_ecdsa_signer!(Es256Signer, Algorithm::ES256, SigningKey256);"
  - The 2c note argued RUSTSEC-2023-0071 (`rsa` 0.9, no fix will ever exist) is inapplicable because that lane only *verifies*, with public keys. That reasoning does not transfer verbatim — an Apple lane signs with a private key — but the conclusion still holds, because Apple mandates ES256, so the private-key operation runs through `p256` and never through `rsa`. The `rsa` crate is still dragged in by the `rust_crypto` feature and would still need the same documented `cargo-deny` ignore.

**Unresolved**
- Whether the `.p8` is PEM-encoded PKCS#8. Verify by inspecting the first line of a real key at first use; if it is SEC1, a conversion step is required before `jsonwebtoken` will load it.
- Whether the JWT `scope` claim can narrow a non-GET request. Apple's entry description lists only GET, but the lifetime rule says long-lived tokens require that "the scope only includes GET requests", which implies non-GET scopes are expressible.
- What status and error code an expired or over-long token actually receives.
- Whether an API key (not a user) can be granted the App Manager Certificates, Identifiers & Profiles access.
- Whether Apple imposes any expiry on the key itself, as distinct from the JWT. No page states one, either way.
- Apple publishes no per-endpoint role matrix for API keys: the specification declares one global security scheme with no per-operation scopes or role annotations, so the role-to-endpoint mapping exists only in the prose Program Roles table, written in terms of web-UI tasks.


## 2. Bundle identifiers and capabilities

**Recommendation:** the bundle identifier is the one resource in this whole survey that fits
willikins' contract cleanly: create, list, read by key, rename and delete all exist. Key it on the
`identifier` string and keep the opaque `id` as the handle once known. Two things must be built in
from the start. First, `filter[identifier]`'s matching semantics are undocumented, so the read
must narrow server-side and then compare the returned `identifier` byte-for-byte itself. Second,
the ownership marker willikins uses everywhere else (a GitHub topic, a Doppler description) has no
slot here: `name` is the only free-text attribute, so either the marker lives in `name` or
`Foreign` is undetectable — see the sketch. Capabilities are a weaker resource: they can be flipped
on, they cannot be finished. Apple names six capabilities needing extra configuration and the API
can supply the configuration for none of the identifier-association ones.

- The full operation list, from the specification's paths. Note there is no POST, PATCH or DELETE on any `relationships/*` path: the app linkage is read-only from the identifier's side, so willikins cannot attach an identifier to an app record through this resource. (source: <https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip>)
  - "/v1/bundleIds -> ['GET', 'POST'] ; /v1/bundleIds/{id} -> ['DELETE', 'GET', 'PATCH'] ; /v1/bundleIds/{id}/app -> ['GET'] ; /v1/bundleIds/{id}/bundleIdCapabilities -> ['GET'] ; /v1/bundleIds/{id}/profiles -> ['GET'] ; /v1/bundleIds/{id}/relationships/app -> ['GET'] ; /v1/bundleIds/{id}/relationships/bundleIdCapabilities -> ['GET'] ; /v1/bundleIds/{id}/relationships/profiles -> ['GET']"
- Apple's own documentation page groups the same operations under four headings and states the create's abstract as "Register a new bundle ID for app development." (source: <https://developer.apple.com/documentation/appstoreconnectapi/bundle-ids.md>)
  - "[`Register a new bundle id`](/documentation/AppStoreConnectAPI/POST-v1-bundleIds)\n[`Modify a bundle id`](/documentation/AppStoreConnectAPI/PATCH-v1-bundleIds-_id_)  Update a specific bundle ID's name.\n[`Delete a bundle id`](/documentation/AppStoreConnectAPI/DELETE-v1-bundleIds-_id_)\n[`List bundle ids`](/documentation/AppStoreConnectAPI/GET-v1-bundleIds)  Find and list bundle IDs that are registered to your team.\n[`Read bundle id information`](/documentation/AppStoreConnectAPI/GET-v1-bundleIds-_id_)"
- The create is `POST https://api.appstoreconnect.apple.com/v1/bundleIds`, body `BundleIdCreateRequest`, success `201 -> BundleIdResponse`. Exactly three attributes are required; `seedId` is the only optional one, and `data.type` is the fixed enum `[bundleIds]`. (source: specification, as above)
  - "\"attributes\": {\"type\": \"object\", \"properties\": {\"name\": {\"type\": \"string\"}, \"platform\": {\"$ref\": \"#/components/schemas/BundleIdPlatform\"}, \"identifier\": {\"type\": \"string\"}, \"seedId\": {\"type\": \"string\", \"nullable\": true}}, \"required\": [\"identifier\", \"name\", \"platform\"]}"
- **Apple documents no format for the identifier string at all.** In the specification it is a bare `{"type": "string"}` with no `pattern`, no `maxLength`, no `format`, verified programmatically across the whole create schema; and the documentation renders an *empty* description for every attribute on two independent pages. No client-side validation rule can be derived from Apple's documentation — the server is the only authority on what it accepts. (source: <https://developer.apple.com/tutorials/data/documentation/appstoreconnectapi/bundleid/attributes-data.dictionary.json>)
  - "#### BundleId.Attributes\n  - identifier => ''\n  - name => ''\n  - platform => ''\n  - seedId => ''   [and from the spec: has pattern: False | has maxLength: False]"
- Apple defines explicit and wildcard App IDs only in terms of the portal UI, and the API exposes no way to ask for a wildcard. "Wildcard" occurs **exactly once** in the entire specification, as the boolean `supportsWildcard` on `CapabilityOption` — so the model acknowledges wildcards exist without offering any field, flag, enum or pattern by which a create could request one. Whether a trailing `*` in `identifier` works is not stated by any Apple source and is deliberately not inferred here. (source: <https://developer.apple.com/help/account/identifiers/register-an-app-id/>)
  - "There are two types of App IDs: an explicit App ID, used for a single app, and a wildcard App ID, used for a set of apps. ... To create an explicit App ID, select Explicit App ID and enter the app's bundle ID in the Bundle ID field. ... To create a wildcard App ID, select Wildcard App ID and enter a bundle ID suffix in the Bundle ID field."
- `BundleIdPlatform` is a closed three-member enum. For an iOS shop, `UNIVERSAL` is the value matching Apple's single-App-ID-across-platforms guidance; there is no `TV_OS` or `WATCH_OS` member. (source: <https://developer.apple.com/tutorials/data/documentation/appstoreconnectapi/bundleidplatform.json>)
  - "ABSTRACT: Strings that represent the operating system intended for the bundle. — POSSIBLE VALUES: IOS, MAC_OS, UNIVERSAL — CONTENT: -`IOS`: A string that represents iOS. -`MAC_OS`: A string that represents macOS. -`UNIVERSAL`: A string that represents all possible platforms."
- Reading back: `GET /v1/bundleIds` takes five filters and sorts on the same five fields; `GET /v1/bundleIds/{id}` answers 200 or 404. Filters are typed as arrays in `form` style, so they are comma-separated multi-value. (source: specification)
  - "filter[name] | in= query | type= array | desc: filter by attribute 'name' ; filter[platform] | enum: ['IOS','MAC_OS','UNIVERSAL'] ; filter[identifier] | desc: filter by attribute 'identifier' ; filter[seedId] ; filter[id] | desc: filter by id(s) ; sort | enum: ['name','-name','platform','-platform','identifier','-identifier','seedId','-seedId','id','-id']"
- **Immutability is provable from the shape of the update schema, not inferred from prose.** `BundleIdUpdateRequest.data.attributes` declares exactly one property where create declared four. So `name` is the only changeable attribute; `identifier`, `platform` and `seedId` are immutable after creation. A drifted `name` is convergeable in place; a drifted `identifier` or `platform` is not repairable by PATCH, and delete-and-recreate may be forbidden (below). (source: specification)
  - "\"BundleIdUpdateRequest\": {... \"data\": {\"type\": \"object\", \"properties\": {\"type\": {\"enum\": [\"bundleIds\"]}, \"id\": {\"type\": \"string\"}, \"attributes\": {\"type\": \"object\", \"properties\": {\"name\": {\"type\": \"string\", \"nullable\": true}}}}, \"required\": [\"id\", \"type\"]}}"
- A duplicate create answers in the 409 family — but the leaf code is not documented, and Apple's own instruction is to match by prefix rather than by exact string. `POST /v1/bundleIds` declares `201, 400, 401, 403, 409, 422, 429`. (source: <https://developer.apple.com/tutorials/data/documentation/appstoreconnectapi/parsing-the-error-response-code.json>)
  - "`409 ENTITY_ERROR`: The request entity is valid and in the right format, but the data in it is unacceptable; for example, it contains an invalid email address, or a duplicate locale. ... The `code` property is a stable, machine-readable value indicating the exact type of error. ... [Tip] Examine the error code using prefix matching rather than exact string comparison."
  - Two caveats that pull against each other and must both be carried. Apple's prose defines `ENTITY_ERROR`, but the string `ENTITY_ERROR` occurs **zero** times in the specification, `ErrorResponse.errors[].code` is an unconstrained `{"type": "string"}` with no enum, and the identical response set `['201','400','401','403','409','422','429']` appears on *every* create in this survey — so the presence of 409 is boilerplate and proves nothing resource-specific. The honest position: 409-on-create means "already exists or otherwise unacceptable", which is not by itself proof of a duplicate, so an `ensure` must read first rather than create-and-catch.
- Deletion exists (`DELETE /v1/bundleIds/{id}` -> 204, with 400/401/403/404/429 as documented failures) but Apple documents two refusals — on the help page only, not in the API. (source: <https://developer.apple.com/help/account/identifiers/delete-an-app-id/>)
  - "You can remove App IDs when you no longer need them. However, you cannot delete an explicit App ID for an app you uploaded to App Store Connect. ... Provisioning profiles that contain a deleted App ID become invalid. ... App IDs can't be deleted if they are grouped with other apps for features like Sign in with Apple. You must ungroup related apps in your Sign in with Apple configuration in order to take further action."
  - The specification's DELETE response list contains no 409, so what status the API actually returns when it refuses one of these is stated by no Apple source fetched here.
- Capabilities live on a separate resource whose operation list is conspicuously asymmetric: there is **no GET** on either capability path. The only way to read capabilities is to list the parent's. (source: specification)
  - "/v1/bundleIdCapabilities -> ['POST'] ; /v1/bundleIdCapabilities/{id} -> ['DELETE', 'PATCH'] ; /v1/bundleIds/{id}/bundleIdCapabilities -> ['GET'] — and GET /v1/bundleIds/{id}/bundleIdCapabilities responses: 200 | List of BundleIdCapabilities with get | BundleIdCapabilitiesWithoutIncludesResponse"
- `CapabilityType` is a closed 28-member enum — a good candidate for a willikins domain enum, since it is closed and versioned with the specification. (source: specification)
  - "\"CapabilityType\": {\"type\": \"string\", \"enum\": [\"ICLOUD\",\"IN_APP_PURCHASE\",\"GAME_CENTER\",\"PUSH_NOTIFICATIONS\",\"WALLET\",\"INTER_APP_AUDIO\",\"MAPS\",\"ASSOCIATED_DOMAINS\",\"PERSONAL_VPN\",\"APP_GROUPS\",\"HEALTHKIT\",\"HOMEKIT\",\"WIRELESS_ACCESSORY_CONFIGURATION\",\"APPLE_PAY\",\"DATA_PROTECTION\",\"SIRIKIT\",\"NETWORK_EXTENSIONS\",\"MULTIPATH\",\"HOT_SPOT\",\"NFC_TAG_READING\",\"CLASSKIT\",\"AUTOFILL_CREDENTIAL_PROVIDER\",\"ACCESS_WIFI_INFORMATION\",\"NETWORK_CUSTOM_PROTOCOL\",\"COREMEDIA_HLS_LOW_LATENCY\",\"SYSTEM_EXTENSION_INSTALL\",\"USER_MANAGEMENT\",\"APPLE_ID_AUTH\"]}"
- **A capability can be switched on but not configured.** `BundleIdCapabilityCreateRequest.relationships` has exactly one member, `bundleId`, and it is required — there is no `appGroups`, `cloudContainers` or `merchantIds` relationship to attach. The only configuration surface is `attributes.settings[]`, and `CapabilitySetting.key` is a closed enum of three keys. So settings can express the iCloud Xcode-compatibility version, the data-protection level and the Sign in with Apple consent, and nothing else. (source: specification)
  - "BundleIdCapabilityCreateRequest: \"relationships\": {\"type\": \"object\", \"properties\": {\"bundleId\": {...}}, \"required\": [\"bundleId\"]}   ///   \"CapabilitySetting\": {... \"key\": {\"type\": \"string\", \"enum\": [\"ICLOUD_VERSION\", \"DATA_PROTECTION_PERMISSION_LEVEL\", \"APPLE_ID_AUTH_APP_CONSENT\"]} ...}   ///   CapabilityOption.key values: XCODE_5, XCODE_6, COMPLETE_PROTECTION, PROTECTED_UNLESS_OPEN, PROTECTED_UNTIL_FIRST_USER_AUTH, PRIMARY_APP_CONSENT"
- Apple names the six capabilities that need extra steps, and the extra step is a portal Configure/Edit flow with no API counterpart. (source: <https://developer.apple.com/help/account/identifiers/enable-app-capabilities/>)
  - "The following app capabilities require additional steps: Sign in with Apple, App groups, Apple Pay, Data protection, iCloud, and push notifications. ... Enable app groups: In Certificates, Identifiers & Profiles, enable the App Groups capability, then click Configure. In the App Groups table, select one or more groups you want to assign to the App ID, then click Continue. ... Enable Apple Pay: ... In the Merchant ID table, select the merchant identifiers you want to assign to the App ID, then click Continue."
  - Net: willikins can enable `APP_GROUPS`, `APPLE_PAY` or `ICLOUD` as a flag, and cannot perform the association those capabilities actually require.
- Two converge hazards worth encoding. `IN_APP_PURCHASE` is on by default for an explicit App ID, so it reads Present without willikins having acted; and enabling a capability has effects beyond the identifier. (source: <https://developer.apple.com/help/account/identifiers/enable-app-capabilities/>)
  - "In-App Purchase is enabled by default for an explicit App ID." / "Enabling a capability will affect provisioning profiles for all eligible platforms."
- Whether enabling an already-enabled capability is idempotent is **not documented**. `POST /v1/bundleIdCapabilities` lists 409 among its responses, but that is the same boilerplate set as every other create, so it is a possible response rather than a documented duplicate contract; it could equally return 201. Since there is no GET on the capability resource, the safe pattern is list-the-parent, match on `capabilityType`, then POST or PATCH — never create-and-catch. (source: specification)
  - "===== POST /v1/bundleIdCapabilities responses: 201 | Single BundleIdCapability | BundleIdCapabilityResponse ; 400 | Parameter error(s) ; 401 | Unauthorized error(s) ; 403 | Forbidden error ; 409 | Request entity error(s) ; 422 | Unprocessable request entity error(s) ; 429 | Rate limit exceeded error"
- Bundle identifiers are Provisioning endpoints, so a Team key is mandatory: an Individual key cannot register one whatever its owner's role. In the specification the twelve `bundleId` paths carry the tags `BundleIds`, `BundleIdCapabilities` and `Profiles`. (source: <https://developer.apple.com/documentation/appstoreconnectapi/creating-api-keys-for-app-store-connect-api>)
  - "-Individual: Access and roles of the associated user. Individual keys aren't able to use Provisioning endpoints, access Sales and Finance, or `notaryTool`." — corroborated against the specification, where the bundleId paths carry tags {'BundleIds', 'Profiles', 'BundleIdCapabilities'}.

**Unresolved**
- Whether `filter[identifier]` matches exactly, by prefix, or by substring. This is the single most load-bearing unknown for a read-by-key: if it matched by prefix, a read for `com.acme.app` would also return `com.acme.app.extension` and the tool would report Present for the wrong record.
- Whether `POST /v1/bundleIds` can create a wildcard identifier.
- What status and code a refused DELETE returns (uploaded app, or Sign in with Apple grouping).
- The exact `ErrorResponse.code` leaf value for a duplicate identifier.
- Whether enabling an already-enabled capability returns 409, 201 or 200.
- The permitted characters and maximum length of the identifier string.


## 3. App groups, iCloud containers, and the sibling provisioning resources

**Recommendation:** stop planning against `/v1/appGroups`. It does not exist, and neither does an
iCloud-container resource. Registration is a human act in the portal or in Xcode by an Account
Holder or Admin, and the association between a group and an identifier is unreachable from both
ends. The siblings are a mixed bag worth knowing: merchant IDs and pass type IDs behave exactly
like bundle identifiers (full quartet, key on `identifier`, name-only PATCH); devices can be
created and read but never deleted; certificates and profiles fit willikins badly and for
structural reasons, not accidental ones.

- There is no app-groups resource. Across the specification's 966 paths and every component schema, `appGroup` and `AppGroup` occur **zero** times: no path, no schema, no create request. No create, no read, no list, no delete, and no stable key to read back by — which fails willikins' contract on its own terms before any question of endpoints arises. (source: <https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip>)
  - "grep -o 'appGroup' openapi.json | wc -l  =>  0   ;  grep -o 'AppGroup' openapi.json | wc -l  =>  0   (spec info: {\"title\": \"App Store Connect API\", \"version\": \"4.4.1\"}, 966 paths; inner file dated 07-15-2026)"
- Apple's own documentation index confirms the absence rather than the specification merely lagging: the Provisioning area enumerates exactly seven resources, and App Groups and iCloud Containers are not among them. (source: <https://developer.apple.com/documentation/appstoreconnectapi.md>)
  - "- **Provisioning.** Manage bundle IDs, capabilities, signing certificates, devices, and provisioning profiles.\n...\n[Bundle IDs](/documentation/AppStoreConnectAPI/bundle-ids)\n[Bundle ID Capabilities](/documentation/AppStoreConnectAPI/bundle-id-capabilities)\n[Certificates](/documentation/AppStoreConnectAPI/certificates)\n[Devices](/documentation/AppStoreConnectAPI/devices)\n[Profiles](/documentation/AppStoreConnectAPI/profiles)\n[Merchant ID](/documentation/AppStoreConnectAPI/merchantids)\n[Pass type Ids](/documentation/AppStoreConnectAPI/pass-type-id)"
- The association is unreachable from both ends, and this is provable from the schemas rather than from absence alone. A capability can be bound to a bundle ID and to nothing else, and a bundle ID's own relationships are profiles, capabilities and app — there is no app-groups relationship to list or modify. (source: specification)
  - "BundleIdCapabilityCreateRequest: \"relationships\": {\"type\": \"object\", \"properties\": {\"bundleId\": {...}}, \"required\": [\"bundleId\"]}   ///   BundleId: \"relationships\": {\"type\": \"object\", \"properties\": {\"profiles\": {...}, \"bundleIdCapabilities\": {...}, \"app\": {...}}}"
- Where a group is actually created, and by whom. Both paths are interactive human sessions, so willikins cannot perform this step at all — it can only instruct. (source: <https://developer.apple.com/help/account/identifiers/register-an-app-group/>)
  - "You'll need to register one or more groups to enable app groups.\nRequired role: Account Holder or Admin.\n...\nSelect App Groups, then click continue.\nEnter a description and identifier, click Continue, then click Register.\nAlternatively, you can create app groups when you enable app groups in Xcode."
- The identifier grammar and the cap are stated only in Apple's Xcode documentation, not on the portal help page. Note that the Xcode flow does three things — creates the container, adds it to the App ID, adds it to the entitlements — which is precisely the three-part association no endpoint exposes. (source: <https://developer.apple.com/documentation/xcode/configuring-app-groups.md>)
  - "Each developer account can register a maximum of 1,000 app groups.\n...\nYou need to register app groups for iOS, iPadOS, tvOS, visionOS, and watchOS apps.\n...\n2. Enter a container ID in the dialog that appears. A container ID must begin with `group.` and then a custom string.\n...\n> Note:\n> You can also create macOS app groups using the naming convention `<Developer team ID>.<group name>`."
- iCloud containers are absent in exactly the same shape, by the same evidence. `ICLOUD` is a `CapabilityType` and `ICLOUD_VERSION` a setting key, so the capability can be enabled and its version chosen, but the container itself cannot be created, named, listed or bound. (source: specification)
  - "grep -o 'cloudContainer' openapi.json | wc -l  =>  0   ;  grep -o 'CloudContainer' openapi.json | wc -l  =>  0   ;  CapabilityType enum includes \"ICLOUD\"; CapabilitySetting key enum includes \"ICLOUD_VERSION\""
- Merchant identifiers: full lifecycle, same shape as bundle identifiers. `identifier` is immutable (update declares `name` only); the one caveat is that `merchantIds` is the only resource in this set whose list endpoint has **no** `filter[id]`, so lookup is by identifier or name only. (source: <https://developer.apple.com/documentation/appstoreconnectapi/merchantids.md>)
  - "[`Create a merchant id`](/documentation/AppStoreConnectAPI/POST-v1-merchantIds)  Add a new merchant ID to your team.\n[`Delete a merchant id`](/documentation/AppStoreConnectAPI/DELETE-v1-merchantIds-_id_)\n[`Modify merchant ids`](/documentation/AppStoreConnectAPI/PATCH-v1-merchantIds-_id_)\n[`List merchant ids`](/documentation/AppStoreConnectAPI/GET-v1-merchantIds)\n> Note: Apple Pay is not available for Enterprise teams.   ///   spec: MerchantIdCreateRequest required [\"identifier\", \"name\"]; MerchantIdUpdateRequest attributes {\"name\"}; GET filters: filter[name], filter[identifier] only"
- Pass type identifiers: full lifecycle, and an ordering constraint that matters for a workflow — the pass type ID must exist before a pass type certificate can be created against it. (source: <https://developer.apple.com/documentation/appstoreconnectapi/pass-type-id.md>)
  - "The `passTypeId` resource represents a pass type certificates unique identifier that you can register, modify, and delete. You need a pass type ID before you can create a pass type certificate with the [Certificates](/documentation/AppStoreConnectAPI/certificates) resource.\n...\n[`Modify a passtypeid`](/documentation/AppStoreConnectAPI/PATCH-v1-passTypeIds-_id_)  Update a specific pass type ID's name.\n[`Create a passtypeid`](/documentation/AppStoreConnectAPI/POST-v1-passTypeIds)\n[`Delete a passtypeid`](/documentation/AppStoreConnectAPI/DELETE-v1-passTypeIds-_id_)"
- Devices: create and read yes, delete **no**, and Apple says so outright. This is the "creatable but not deletable" case the brief anticipated: still usable, with `PATCH status=DISABLED` standing in for absence. (source: <https://developer.apple.com/documentation/appstoreconnectapi/devices.md>)
  - "A `devices` resource represents the iOS, Apple TV, Apple Watch, and Mac devices that you register to use for development and testing.\n> Note:\n> You can only remove registered devices through the Apple Developer website.\n...\n[`Register a new device`](/documentation/AppStoreConnectAPI/POST-v1-devices)\n[`List devices`](/documentation/AppStoreConnectAPI/GET-v1-devices)\n[`Modify a registered device`](/documentation/AppStoreConnectAPI/PATCH-v1-devices-_id_)  Update the name or status of a specific device."
- Certificates: create, download, revoke, and a narrow modify — but **no project-derived key**. The filters are `displayName`, `certificateType`, `serialNumber` and `id`; there is no filter by bundle ID, because certificates are team-scoped, not project-scoped, and a serial number is only known after creation. There is therefore no key a willikins read could use to find "this project's certificate". (source: <https://developer.apple.com/documentation/appstoreconnectapi/certificates.md>)
  - "The `certificates` resource represents the digital certificates you use to sign your iOS or Mac apps for development and distribution. You can create new certificates, revoke existing certificates, and download certificates.\n> Note:\n> You can only create Developer ID certificates for macOS through the Apple Developer website or Xcode.\n...\n[`Create a certificate`](/documentation/AppStoreConnectAPI/POST-v1-certificates)  Create a new certificate using a certificate signing request.\n[`Modify a Certificate`](/documentation/AppStoreConnectAPI/PATCH-v1-certificates-_id_)  Update the activation status for a specific certificate.\n[`Revoke a certificate`](/documentation/AppStoreConnectAPI/DELETE-v1-certificates-_id_)   ///   spec GET /v1/certificates filters: filter[displayName], filter[certificateType], filter[serialNumber], filter[id]"
- A certificate needs a CSR, and a CSR is generated locally. `CertificateCreateRequest` requires `csrContent`, and Apple documents its generation as an act on your own Mac. (source: <https://developer.apple.com/help/account/certificates/create-a-certificate-signing-request/>)
  - "Keychain Access on your Mac allows you to create a certificate signing request (CSR).\nLaunch Keychain Access located in /Applications/Utilities.\nChoose Keychain Access > Certificate Assistant > Request a Certificate from a Certificate Authority.\n...\nChoose \"Saved to disk,\" then click Continue.\n...\nWhen creating ALD encryption and signing certificates, you must specify the Key Pair information. Use the command line, such as the Terminal app, to generate your keys and CSRs on your Mac.   ///   spec: CertificateCreateRequest attributes required [\"csrContent\", \"certificateType\"]"
  - **Inference, flagged as such rather than quoted:** a CSR is the public half of a key pair, so whoever generates it holds the private half. If willikins generated the CSR server-side it would hold a code-signing private key — exactly the class of secret the design says the server never touches. Certificate creation therefore stays operator-side; willikins should at most accept a CSR as opaque input and read back `certificateContent`, which is public. Apple does not state this consequence anywhere, so it is reasoning, not a fact.
- The operator's habit of sharing Apple distribution certificates across projects is not a preference to design away — it is what Apple enforces. Only one of each distribution certificate type is allowed per team, and revocation is destructive across every project at once. (source: <https://developer.apple.com/support/certificates/>)
  - "Distribution certificates belong to the team and only one type of each distribution certificate (with the exception of Developer ID certificates) is allowed per team. Only the Account Holder or Admin role can create distribution certificates (if you're enrolled as an individual, you are the Account Holder).\n...\nIf your Apple Developer Program membership is valid, your existing apps on the App Store won't be affected. However, you'll no longer be able to upload new apps or updates signed with the expired or revoked certificate to App Store Connect. Builds already uploaded to App Store Connect but not yet submitted for App Review may be marked as Invalid Binary if they were signed with a revoked certificate."
  - A widely repeated third-party figure of "3 iOS Distribution certificates" appears in forum threads; it contradicts this page and is **(unverified)**, so it is not relied on.
- Profiles: create, read, download and delete — but **no PATCH at all**, in either the specification or the topic list. A profile is immutable in full: changing its certificates, devices or bundle ID means delete-and-recreate, and the `uuid` changes when you do. The read-back key is weak — `filter[name]` is the only natural-key filter. (source: <https://developer.apple.com/documentation/appstoreconnectapi/profiles.md>)
  - "The `profiles` resource represents the provisioning profiles that allow you to install apps on your iOS devices or Mac. You can create and delete provisioning profiles, and download them to sign your code. Provisioning profiles include signing certificates, device identifiers, and a bundle ID.\n### Creating and Deleting Provisioning Profiles\n[`Create a profile`](/documentation/AppStoreConnectAPI/POST-v1-profiles)\n[`Delete a profile`](/documentation/AppStoreConnectAPI/DELETE-v1-profiles-_id_)   ///   spec: /v1/profiles/{id} has only ['DELETE','GET'] — no PATCH; ProfileCreateRequest relationships required [\"certificates\", \"bundleId\"]; GET filters: filter[name], filter[profileType], filter[profileState], filter[id]"
- Every filter in this survey carries only a generic auto-generated description and is typed as an array of strings. Apple states nowhere fetched here whether matching is exact, prefix or substring. Every willikins read must therefore compare the returned attribute itself before deciding Present, Absent, Foreign or Mismatch. (source: specification)
  - "/v1/bundleIds filter[identifier] | desc: \"filter by attribute 'identifier'\" | schema: {\"type\": \"array\", \"items\": {\"type\": \"string\"}}   ///   /v1/devices filter[udid] | desc: \"filter by attribute 'udid'\"   ///   /v1/passTypeIds filter[identifier] | desc: \"filter by attribute 'identifier'\""

**Unresolved**
- Whether profile `name` is unique per team. `filter[name]` is the only natural-key filter for profiles, and no Apple source states uniqueness; if two POSTs with the same name both succeed, a name-keyed profile tool would silently accumulate duplicates.
- Whether enabling `APP_GROUPS` on a bundle ID that belongs to no group succeeds, and what it means. Apple documents neither the precondition nor the effect.
- What Xcode actually calls when it creates an app group. The Xcode documentation says it creates the container and adds it to the App ID, so some endpoint exists — but it is not in the public API and no Apple statement about it was found. A private endpoint is not a supportable dependency.
- Whether a distribution certificate's private key can ever be recovered from Apple. Apple states `certificateContent` is downloadable and says nothing about the private half, which by construction Apple never had.
- The per-team caps on non-distribution certificate types (development, pass type, APNs). Forum threads cite numbers; all are third-party and **(unverified)**.
- The per-role, per-Provisioning-endpoint permission matrix. The Program Roles page has rows for exactly these permissions ("Create and revoke distribution certificates", "Create and delete distribution provisioning profiles", "Can be granted access to Certificates, Identifiers & Profiles") but several grid cells render as images/CSS and did not survive text extraction in this pass.


## 4. App records

**Recommendation:** an app record cannot be created and cannot be deleted through the API. Settle
this from Apple's own enumeration of the operations, not from a failure to find an endpoint: the
specification declares two operations, the documentation page lists five, and none of the seven is
a create. Everything *downstream* of an existing record is API-reachable and fits willikins well —
locales, categories, versions and platforms, pricing, availability, TestFlight, screenshots,
review submission, and since 4.1 even build uploads. So the boundary is exactly one resource wide:
the record itself. The realistic tool shape is read-and-converge, with `Absent` reported as a
blocking human precondition rather than something an `ensure` can fix.

- The specification defines only `get` under `/v1/apps` and only `get` and `patch` under `/v1/apps/{id}`. There is no POST and no DELETE on any `/v*/apps*` path — the only DELETE containing "apps" is `/v1/betaTesters/{id}/relationships/apps`, which unlinks a beta tester. (source: <https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip>)
  - "\"operationId\": \"apps_getCollection\" ... \"operationId\": \"apps_updateInstance\""
- There is no create *schema* either, and the absence is specific rather than spec-wide: the same file contains `BundleIdCreateRequest` and `AppPriceScheduleCreateRequest`. (source: specification)
  - "['AppUpdateRequest', 'BundleIdCreateRequest', 'BundleIdUpdateRequest']  — result of filtering components.schemas for AppCreateRequest, AppUpdateRequest, AppDeleteRequest, BundleIdCreateRequest, BundleIdUpdateRequest; AppCreateRequest is absent."
- Apple's own operation list for the resource, verbatim — five operations, no create and no delete. The 27 further topic sections on that page are reads of sub-resources of an existing app, plus two writes to sub-resources. (source: <https://developer.apple.com/documentation/appstoreconnectapi/apps>)
  - "Getting and modifying app information: \"List apps\" — Find and list apps in App Store Connect. (GET /v1/apps); \"Read app information\" — Get information about a specific app. (GET /v1/apps/{id}); \"Modify an app\" — Update app information, including bundle ID, primary locale, price schedule, and global availability. (PATCH /v1/apps/{id}); \"Read an app's encryption declarations\" — Find and list all available app encryption declarations.; \"Read an app's encryption declaration ids\" — Find and list all available app encryption declaration IDs for a specific app."
- No release note in the API's history announces app-record creation. All 30 versions listed on the release-notes index (1.0 through 4.4.1) were fetched as documentation JSON, flattened and searched case-insensitively for `POST /v1/apps`, "create an app record", "creating an app record", "create a new app", "add a new app" and `AppCreateRequest`. Zero hits in any version. (source: <https://developer.apple.com/documentation/appstoreconnectapi/app-store-connect-api-release-notes>)
  - "App Store Connect API version 2.0 provides resources that enable you to automate actions you take in App Store Connect. Added support for creating, managing, and submitting for review your in-app purchases and auto-renewable subscriptions ..." (representative of every version's prose; no version announces creating an app record)
- The live API's own answer, for what it is worth: a 403 naming the operations it allows, and CREATE is not among them. The three it names map one-to-one onto the specification's operationIds. **(unverified — `developer.apple.com/forums` serves a bot-verification interstitial to a plain fetch, so this came through a rendering of the thread rather than bytes; it is corroboration, not the settling evidence.)** (source: <https://developer.apple.com/forums/thread/759126>)
  - "\"status\": \"403\", \"code\": \"FORBIDDEN_ERROR\", \"title\": \"The given operation is not allowed\", \"detail\": \"The resource 'apps' does not allow 'CREATE'. Allowed operations are: GET_COLLECTION, GET_INSTANCE, UPDATE\""
- Where Apple says a record is created: the website. The page is a pure click-through, and its own FAQ enumerates what the REST API can deliver without including the record. It also names a precondition that is not an endpoint at all — the Account Holder must have signed the latest agreement. (source: <https://developer.apple.com/help/app-store-connect/create-an-app-record/add-a-new-app/>)
  - "Before uploading a build of your app to App Store Connect, first create an app record in your App Store Connect account. ... In Apps, click the add button (+) on the top left. ... In the pop-up menu, select New App. ... Note: You can't add an app to your account until the Account Holder signs the latest agreement in the Business section. ... FAQs — Can I deliver my app information using the App Store Connect REST API? Yes, you can manage In-App Purchases, subscriptions, metadata, and app pricing via the App Store Connect REST API."
- A record *can* be read back by a stable key: `GET /v1/apps` filters on `bundleId` and `sku` as well as `id` and `name`. Note the API is team-scoped, so a Foreign app — another team's record holding the bundle ID you want — is invisible rather than distinguishable; a globally taken bundle ID surfaces as a failure at registration time, not as a Foreign read. (source: specification)
  - "\"name\": \"filter[bundleId]\", \"schema\": {\"type\": \"array\", \"items\": {\"type\": \"string\"}} ... \"name\": \"filter[sku]\" ... \"name\": \"filter[id]\"  (full list: filter[name], filter[bundleId], filter[sku], filter[appStoreVersions.appStoreState] (deprecated), filter[appStoreVersions.platform], filter[appStoreVersions.appVersionState], filter[reviewSubmissions.state], filter[reviewSubmissions.platform], filter[appStoreVersions], filter[id])"
- What is immutable. The resource `id` is the Apple ID, generated at creation; the SKU is fixed from creation; the bundle ID is changeable only until the first build upload — and that last precondition appears **only** in the Help reference, never in the API documentation. (source: <https://developer.apple.com/help/app-store-connect/reference/app-information/>)
  - "Bundle ID — A unique identifier for your app that is used throughout the system. ... You can't change this property after you upload a build. ... SKU — A unique ID you give to your app for internal tracking that's not visible to customers. ... You can't change the SKU after you add the app to your account. ... Apple ID — A unique identifier automatically generated for your app when you add the app to your account. ... You can't edit this property."
- What PATCH can actually change — notably **not** `name` and **not** `sku`. A correction to the docs page while we are here: `AppUpdateRequest.data` has keys `['type','id','attributes']` and its `relationships` is null, so the "Modify an app" abstract's claim that it updates "price schedule, and global availability" is stale; those are separate POSTs. (source: specification)
  - "AppUpdateRequest.data.attributes.properties = accessibilityUrl, bundleId, primaryLocale, subscriptionStatusUrl, subscriptionStatusUrlVersion, subscriptionStatusUrlForSandbox, subscriptionStatusUrlVersionForSandbox, contentRightsDeclaration, streamlinedPurchasingEnabled — and AppUpdateRequest.data has no \"relationships\" property at all."
- Once a record exists, almost everything around it is reachable (the endpoint list below is read off the specification's paths, not quoted per item; only the build-upload addition is quoted): `POST /v1/appInfoLocalizations` (name, subtitle, privacy URLs per locale), `PATCH /v1/appInfos/{id}` (categories and subcategories by relationship), `POST /v1/appStoreVersions` (versions and platforms), `POST /v1/appStoreVersionLocalizations`, `POST /v1/appPriceSchedules`, `POST /v2/appAvailabilities`, `POST /v1/betaGroups` / `betaAppLocalizations` / `betaTesters` / `betaAppReviewSubmissions`, the screenshot and preview set family, `POST /v1/reviewSubmissions` and `POST /v1/appStoreVersionReleaseRequests` — and, since version 4.1, build uploads. (source: <https://developer.apple.com/documentation/appstoreconnectapi/app-store-connect-api-4-1-release-notes>)
  - "You can now use   to upload and manage build uploads for your apps."
- Adding a platform to an existing app is explicitly an API operation, not only a website flow: `platform` and `versionString` are both required on `AppStoreVersionCreateRequest`, with platform drawn from IOS, MAC_OS, TV_OS, VISION_OS. (source: <https://developer.apple.com/documentation/appstoreconnectapi/post-v1-appstoreversions>)
  - "Add a new App Store version or platform to an app. ... Use this endpoint to add a new version of an app. The new version can be an incremental update of an existing app for a particular platform, or it can be the first version on a new platform for the app."
- Removal is website-only, and it is not a clean teardown: the SKU is burned for the organization and the bundle ID is burned too once a build has been uploaded. (source: <https://developer.apple.com/help/app-store-connect/create-an-app-record/remove-an-app/>)
  - "WARNING: If you remove an app, you'll lose ownership of the app name. Removed apps can only be restored if the name isn't currently in use by another developer account. In addition, the SKU can't be reused in the same organization and if you've uploaded a build, your bundle ID can't be reused. ... In Apps, select the app you want to remove. ... Scroll to the Additional Information section and click Remove App."
- The record and the identifier are separate resources in separate systems, and the documented order is identifier first. The clearest Apple sentence stating that order is in a **retired** archived document, so it is quoted here and repeated in the verify list rather than frozen into the sketch. The non-archived corroboration is the provisioning-profile help page: "Uploading an app to App Store Connect requires an app record registered with an explicit App ID." (source: <https://developer.apple.com/library/archive/documentation/ToolsLanguages/Conceptual/YourFirstAppStoreSubmission/CreateYourAppRecordiniTunesConnect/CreateYourAppRecordiniTunesConnect.html>)
  - "Before you begin, make sure that you have these assets ready to enter into the forms: ... A bundle ID that you've set to match your App ID. ... Before you can create the actual iTunes Connect app record, you need to accomplish three main tasks using Xcode: Capture screenshots. Set the launch image. Set your bundle ID."
- Adding platforms is reported to need no new identifiers — platforms are said to share the same Apple ID, SKU and bundle ID as the iOS app. **(unverified — no Apple sentence carrying this was fetched in this pass; it is in the verify list.)**

**Unresolved**
- Whether the bundle ID must already be registered at the moment the record is created. The current Help page does not say so; the only Apple sentence stating that order is in a retired document. Do not freeze the ordering into code on this evidence.
- Whether `PATCH /v1/apps/{id}` actually accepts a `bundleId` change, and under what precondition. The API documentation states none; the "can't change after you upload a build" constraint comes only from the Help reference.
- Whether `bundleId` is unique per team in App Store Connect, such that `filter[bundleId]` returns at most one app. The filter is array-valued and returns a collection; no Apple sentence states one-app-per-bundleId. A read keyed on it must handle a multi-row answer.
- What the API answers for a PATCH on an app you cannot see, or a read of another team's record holding the bundle ID you want. The API appears team-scoped, so Foreign may be indistinguishable from Absent.
- Whether any API path at all amounts to removal (a sub-resource, or an availability change). Only the absence of `DELETE /v1/apps/{id}` was confirmed.
- Whether fastlane's `produce`, widely cited as creating app records, uses the public API or a session-cookie web client. The fastlane source was not fetched, so any claim that a third-party tool creates app records through a supported Apple API is **(unverified)** and is not evidence that the public API supports it.


## Verify with a browser before relying on them

Deduplicated from all four passes. Nothing here may enter a plan's frozen code; each item is
either unfetched, third-party, or an absence that only an empirical probe against a real team can
settle. A probe needs a credential this session did not have and did not seek.

**The credential and the token**

- Is the `.p8` PEM-encoded PKCS#8 with the tag `-----BEGIN PRIVATE KEY-----`? Apple's API pages say only "the private key you downloaded"; the account Keys help page says only "a text file with a .p8 file extension". The widely repeated PKCS#8 claim traced to a rendered forum summary whose source bytes contain no occurrence of "p8" or "PKCS". Inspect the first line of a real key at first use; a SEC1 `EC PRIVATE KEY` PEM would need a conversion step before `jsonwebtoken` will load it.
- Can the JWT `scope` claim narrow a **non-GET** request? Apple's entry description names only the GET method, while the lifetime rule's "the scope only includes GET requests" implies non-GET scopes are expressible. This decides whether scope narrowing gives any protection at all on a willikins write path.
- What status and error code does a token that has expired, or whose lifetime exceeds 20 minutes, receive? The prose status table has no 401 row and its 403 row names only revocation, malformed tokens and disallowed operations, while the specification declares a 401 per operation. Key the client's re-sign branch off the real code, not off the status.
- Does Apple impose any expiry on the API key itself, as distinct from the JWT? No page states one either way.
- Can an API **key** (as opposed to a user) be granted the App Manager "Certificates, Identifiers & Profiles" access? Key creation offers only one role selector with no second toggle.
- Which roles may call each Provisioning endpoint. Apple publishes no per-endpoint role matrix for keys; the "Account Holder or Admin" sentences quoted above are the website's rule.
- Several Program Roles grid cells render as images or CSS and did not survive text extraction in this pass (the certificate and profile rows in particular). Read the grid in a browser before recommending a minimum-privilege role.

**Reading back by key**

- Does `filter[identifier]` (and `filter[udid]`, `filter[name]`, `filter[bundleId]`) match exactly, by prefix, or by substring? Apple's description is the auto-generated "filter by attribute 'x'" and nothing more. Until settled, every read compares the returned attribute byte-for-byte itself. Third-party reports of substring behaviour exist in fastlane issue threads; they were not fetched and are **(unverified)**, flagged only so nobody treats exactness as safe by default.
- Is `bundleId` unique per app record in App Store Connect? The filter is array-valued and no Apple sentence states one-app-per-bundleId.
- Is a provisioning profile's `name` unique per team? It is the only natural-key filter profiles have.

**Duplicate creates and error codes**

- Does a duplicate create actually return 409, and with what `ErrorResponse.code`? The identical response set `['201','400','401','403','409','422','429']` appears on every create in this survey, so 409's presence is boilerplate. Apple's prose defines `409 ENTITY_ERROR` and instructs prefix matching, but the string `ENTITY_ERROR` occurs zero times in the specification and `code` carries no enum, so the leaf value for "duplicate bundle identifier" is unknown. Match the prefix; never hard-code a leaf.
- Does `POST /v1/bundleIdCapabilities` return 409, 201 or 200 when the capability is already enabled? This decides whether a capability `ensure` can be create-and-catch or must be list-then-branch.
- What status does a refused `DELETE /v1/bundleIds/{id}` return — for an identifier belonging to an uploaded app, or one grouped for Sign in with Apple? The documented response set has no 409 to accommodate the refusal the help page describes.

**Capabilities, wildcards, and shapes not in the specification**

- Can `POST /v1/bundleIds` create a **wildcard** identifier? "Wildcard" occurs once in the whole specification, as `CapabilityOption.supportsWildcard`; no create field expresses wildcard-ness.
- Are the identifier string's permitted characters and maximum length constrained? No pattern, no `maxLength`, no prose.
- Does enabling `APP_GROUPS` on an identifier that belongs to no group succeed, and does it mean anything?
- What does Xcode call when it creates an app group? Some endpoint exists; it is not in the public API. A private endpoint is not a supportable dependency and must not be planned against.

**App records and ordering**

- Must the bundle identifier already be registered when the app record is created? The only Apple sentence stating that order is in a retired archived document.
- Does `PATCH /v1/apps/{id}` accept a `bundleId` change, and does it error or silently no-op once a build exists?
- What does the API answer for a read or PATCH of another team's app? Foreign may be indistinguishable from Absent.
- Is there any API path that amounts to removing an app record?
- Do added platforms really share the iOS app's Apple ID, SKU and bundle ID, needing no new identifiers? Stated by a reader, carried by no fetched Apple sentence.
- Does fastlane's `produce` use the public API or a session-cookie web client? Not fetched; no third-party tool's behaviour is evidence about Apple's public API either way.

**Certificates**

- Can a distribution certificate's private key be recovered or re-downloaded? Apple says `certificateContent` is downloadable and nothing about the private half, which by construction Apple never held.
- The per-team caps on non-distribution certificate types (development, pass type, APNs). Forum numbers are third-party and **(unverified)**; the "3 iOS Distribution certificates" figure in particular contradicts Apple's support page.


## Sketch: what a willikins App Store Connect provider could offer

**This is a sketch, not a design.** It is written in the shape of the milestone 2 plan's provider
tables so the two can be compared at a glance, and for no other reason. No port types are named,
no naming derivation row is proposed, no ownership marker is chosen. Several rows rest on facts in
the verify list above and cannot be implemented until those are settled against a real team.

Every row carries the three facts willikins' idempotence contract needs: a **stable key** to read
back by, whether **Present / Absent / Foreign** are knowable, and what a **duplicate create**
answers.

| Tool | `read` | `ensure` |
| --- | --- | --- |
| `appstore.bundle_id.ensure` | **Key:** the `identifier` string; the opaque `id` is the handle once known. `GET /v1/bundleIds?filter[identifier]=<id>` to narrow, then **compare the returned `identifier` byte-for-byte** (match semantics undocumented — verify list), then `GET /v1/bundleIds/{id}`: 200 -> `Present`; 404 -> `Absent`. A differing `name` -> `Mismatch { name }`; a differing `platform` -> `Mismatch` that is **terminal**, because `platform` is immutable and delete may be refused. **Foreign has no marker slot:** `name` is the only free-text attribute, so either the ownership marker lives in `name` (and a human renaming it makes the identifier read `Foreign`, as the GitHub topic already does) or `Foreign` is undetectable and a pre-existing identifier reads `Present`. *if* a list ever returned another team's identifier, `seedId` is filterable and sortable and would distinguish it — but under a team-scoped key `seedId` may simply be constant, and no Apple sentence says otherwise (unverified) | `POST /v1/bundleIds` with `identifier`, `name`, `platform` (all three required; `seedId` optional) -> 201. **Duplicate:** 409 in the `ENTITY_ERROR` family, leaf code undocumented and 409 present on every create as boilerplate — so `ensure` reads first and never creates-and-catches; a 409 after a create that may have landed -> re-`read`. `PATCH /v1/bundleIds/{id}` converges `name` and nothing else. `DELETE` -> 204, refused for an uploaded app or a Sign in with Apple grouping with an undocumented status |
| `appstore.bundle_id_capability.ensure` | **Key:** `(bundle id, capabilityType)`. There is **no GET** on `/v1/bundleIdCapabilities/{id}`, so the read must list the parent: `GET /v1/bundleIds/{id}/bundleIdCapabilities` and match `capabilityType` -> `Present` / `Absent`. This makes a capability a sub-resource of the identifier rather than a tool with its own key. `Foreign` is not expressible — a capability carries no ownership evidence. **Converge hazard:** `IN_APP_PURCHASE` is on by default for an explicit App ID and reads `Present` without willikins acting | `POST /v1/bundleIdCapabilities` with `capabilityType` and the required `bundleId` relationship -> 201; `PATCH /v1/bundleIdCapabilities/{id}` converges `settings`; `DELETE` -> 204 disables. **Duplicate:** undocumented — could be 409, 201 or 200, so list-then-branch is mandatory. **Half a tool by construction:** `APP_GROUPS`, `APPLE_PAY` and `ICLOUD` can be flagged on and cannot be *configured*, because the create request relates a capability to a bundle ID and to nothing else, and `CapabilitySetting.key` has three members, none of which names a group, container or merchant ID |
| `appstore.merchant_id.ensure` | **Key:** `identifier`, via `filter[identifier]` plus a byte-exact compare. `GET /v1/merchantIds/{id}` 200/404 -> `Present` / `Absent`. Same missing marker slot as the bundle identifier, so the same `Foreign` problem. One extra caveat: this is the only resource here whose list has **no** `filter[id]` | `POST /v1/merchantIds` with `name` + `identifier`; `PATCH` converges `name` only (`identifier` immutable); `DELETE` exists. **Duplicate:** same undocumented 409 family. Not available to Enterprise teams |
| `appstore.pass_type_id.ensure` | **Key:** `identifier`, via `filter[identifier]` (and `filter[id]` exists here) plus a byte-exact compare. `Present` / `Absent` clean; `Foreign` undetectable for the same reason | `POST /v1/passTypeIds` with `name` + `identifier`; `PATCH` converges `name` only; `DELETE` exists. **Duplicate:** same undocumented 409 family. Ordering: must exist before a pass type certificate references it |
| `appstore.device.ensure` | **Key:** `udid`, via `filter[udid]` plus a byte-exact compare. `Present` / `Absent` clean; `Foreign` undetectable | `POST /v1/devices` with `name`, `udid`, `platform`. **No DELETE exists** — "You can only remove registered devices through the Apple Developer website." Model absence as `PATCH status=DISABLED`; a tool that can create but not delete is still usable, and this is that case. **Duplicate:** undocumented |
| `appstore.app_record` (**read-only; not an `ensure`**) | **Key:** `filter[bundleId]`, byte-exact compared, and handle a multi-row answer (per-team uniqueness unverified). 200 with a match -> `Present`; no match -> `Absent`. **`Foreign` is indistinguishable from `Absent`:** the API is team-scoped, so another team's record holding the same bundle ID is invisible, and a globally taken bundle ID surfaces only as a registration failure. A read can also report `Mismatch` on the PATCHable attributes | **There is no create and no delete.** `ensure` cannot converge `Absent` to `Present` — which is worse than the create-but-never-delete case; the resource must be created on the website. The realistic shape is either a tool whose `ensure` halts with a human instruction naming the website step, or a manual prerequisite outside the tool graph entirely. Where the record exists, `PATCH /v1/apps/{id}` converges `bundleId`, `primaryLocale`, `contentRightsDeclaration` and the subscription-status URLs — and **not** `name`, **not** `sku` |
| `appstore.app_store_version.ensure` (and the rest of the downstream family) | **Key:** `(app id, platform, versionString)`. Every downstream resource — locales, categories, versions, pricing, availability, TestFlight groups, screenshots, review submissions, build uploads — hangs off an app record's `id`, so each read is a list-the-parent-and-match, like capabilities | `POST /v1/appStoreVersions` with `versionString` + `platform` and the required `app` relationship; this is also how a **platform** is added to an existing app. Not researched row by row in this pass; recorded because it is where the API's real leverage is, once a record exists |
| `appstore.certificate` / `appstore.profile` | **Read-and-reference only; not candidate tools.** A certificate has **no project-derived key**: the filters are `displayName`, `certificateType`, `serialNumber`, `id`, and a serial is known only after creation — so a read cannot find "this project's certificate". A profile's only natural-key filter is `name`, whose uniqueness is unverified | A certificate needs a `csrContent`, and whoever generates the CSR holds a code-signing private key — a class of secret the server never touches, so creation stays operator-side. Apple allows **one distribution certificate of each type per team**, so per-project creation is wrong by construction, and revocation is destructive across every project at once. A profile has **no PATCH at all**: any change is delete-and-recreate with a new `uuid`. Read the team certificate by `filter[certificateType]` and reference it; never `ensure` it |
| `appstore.app_group` / `appstore.icloud_container` | — | **Not a tool.** No path, no schema, no key, no association. Registration is a portal or Xcode act by an Account Holder or Admin |

Ranked by fit: bundle identifiers, merchant IDs and pass type IDs are clean; devices are usable
with disable standing in for delete; capabilities are a sub-resource rather than a keyed tool and
are half-implementable; the app record is read-and-converge with a human precondition;
certificates and profiles are reference data; app groups and iCloud containers are not tools at
all.

Two cross-cutting notes for whoever turns this into a design. There is no per-project Apple
credential and none can be constructed, so the provider's `Credential` is one team-wide secret
whose blast radius is bounded only by the role chosen at key generation. And every `read` in this
table narrows server-side and then re-compares client-side, because Apple documents no filter's
matching semantics — that is a provider-wide rule, not a per-tool workaround.


## What blocks a provider today

Not all of these are endpoints. Several are a human at a keyboard, and they are the ones most
likely to be missed by a plan written from the specification alone.

1. **The credential is generated on a person's machine and can never be provisioned.** A Team key
   is created in the App Store Connect web UI by an **Admin**; the private half downloads exactly
   once and Apple keeps no copy. The specification has no path and no schema for API keys, so
   willikins can never create, read, rotate or revoke the credential it runs on — only consume it.
   Apple documents no rotation mechanism at all: rotation is generate-a-second-key, swap the
   consumer, revoke the first, done by hand. Losing the `.p8` has no recovery path.
2. **The credential's *shape* does not fit the existing `Credential` type.** It is a triple —
   issuer-ID UUID, key ID, P-256 private key — of which only the last is secret. willikins'
   `Credential` today is a single string with a format regex (`github_pat_…`, `dp.sa.…`). An
   App Store Connect provider needs a multi-part credential, or two non-secret inputs alongside one
   secret, before it can exist. That is a change to `willikins-providers-http`, not a new provider
   crate.
3. **There is no token to fetch — the client must mint one.** ES256 over P-256, signed locally,
   `aud: appstoreconnect-v1`, **20-minute hard ceiling** for every endpoint in this survey, and
   Apple's own advice is to reuse one token until it expires rather than sign per request. So a
   long apply must re-sign mid-run. None of that machinery exists: `jsonwebtoken` is not a
   dependency (it is a recorded milestone-2c decision, not code), and it must run inside the
   synchronous `ureq` client on a `spawn_blocking` thread. Adding it pulls `rsa` 0.9 in through the
   `rust_crypto` feature and so re-raises RUSTSEC-2023-0071, which is inapplicable here — Apple
   mandates ES256, so signing runs through `p256` — but still needs the documented `cargo-deny`
   ignore. And whether the `.p8` loads at all depends on it being PKCS#8, which Apple never states.
4. **An app record is created only by the website, and only after a human signs an agreement.**
   "You can't add an app to your account until the Account Holder signs the latest agreement in the
   Business section." No endpoint creates the record, no endpoint deletes it, and no endpoint
   reports whether the agreement is signed. Any workflow that ends in an App Store app has a step
   willikins can name and cannot perform.
5. **App groups, iCloud containers and the merchant-ID association are portal or Xcode only.** Not
   merely uncreatable — unreadable and unassociatable. A capability can be flagged on and then
   cannot be finished, which means a workflow that "enables app groups" produces a half-configured
   identifier and must say so rather than report success.
6. **Certificates require a private key on a developer's own machine.** `csrContent` is required,
   and a CSR is the public half of a locally generated key pair. Apple allows one distribution
   certificate of each type per team, so there is at most one to share and churning it is
   destructive across every project at once — "Builds already uploaded to App Store Connect but not
   yet submitted for App Review may be marked as Invalid Binary if they were signed with a revoked
   certificate." A certificate must be reference data, never a converging resource.
7. **The blast radius cannot be narrowed.** A Team key reaches every app on the team regardless of
   its role; only an Individual key is app-limited, and Individual keys cannot call Provisioning
   endpoints at all — so the operator cannot have both per-app narrowing and identifier
   management. The rate limit is per key, so one shared Apple credential across projects is one
   shared rolling-hour budget. The JWT `scope` claim is the only per-request narrowing available,
   and whether it can narrow a write is unverified.
8. **Read-by-key rests on undocumented filter semantics.** Every filter in the API is described as
   "filter by attribute 'x'" and nothing more. A provider can work around this by re-comparing
   client-side, and does — but until someone probes a real team, no willikins `read` here can claim
   to be exact by construction, only exact by defensive comparison.
9. **`Foreign` has no marker slot on any identifier resource.** GitHub has a topic and Doppler has a
   description; a bundle identifier, a merchant ID and a pass type ID have `identifier` (immutable)
   and `name` (mutable) and nothing else. Either the ownership marker lives in `name`, with the
   collision and readability costs that implies, or willikins adopts a pre-existing identifier
   silently. That is a design decision this note deliberately does not make.

**The deliverable, in one sentence.** Bundle identifiers, fully; the app record, read-only, with
creation left to a person and everything downstream of it reachable; app groups, not at all — and a
provider that does two of those three is still worth building.
