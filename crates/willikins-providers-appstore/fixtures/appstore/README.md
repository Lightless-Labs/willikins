# App Store Connect fixtures

Recorded and hand-authored response bodies used by this crate's mock-server
tests. See `docs/research/2026-09-16-app-store-connect.md` for the primary
sources these shapes rest on.

None of these carry a real credential, a real JWT, or a real team's
identifiers. `T6G4XCV345`, `CAP1234567`, and `com.example.MyApp` are
made-up, obviously-fake values chosen to look like Apple's own documented
examples in shape only.

| File | Verified | Notes |
| --- | --- | --- |
| `bundle_id_get_present.json` | Documented shape | A single `bundleIds` resource, from `BundleIdResponse`'s schema (research note, section 2). Not yet cross-checked against a live `GET`. |
| `bundle_id_list_one.json` | Documented shape | `GET /v1/bundleIds?filter[identifier]=...`'s list shape, one matching row. |
| `bundle_id_list_empty.json` | Documented shape | The same list endpoint with zero rows — `appstore.bundle_id.ensure`'s `Absent` arm. |
| `bundle_id_list_mismatch_name.json` | Documented shape | One row whose `identifier` matches but `name` does not — the convergent `Mismatch { name }` arm. |
| `bundle_id_list_mismatch_platform.json` | Documented shape | One row whose `identifier` matches but `platform` does not — the terminal `Mismatch { platform }` arm. |
| `bundle_id_list_prefix_neighbor.json` | Documented shape | A row whose `identifier` is a different string that merely *starts with* the one requested (`com.example.MyApp.extension` vs `com.example.MyApp`) — proves this crate's client-side exact comparison, not the documented-but-unverified filter, decides the match (research note, section 2's "Unresolved": `filter[identifier]`'s matching semantics). |
| `bundle_id_post_created.json` | Documented shape | `POST /v1/bundleIds`'s success response. Not yet cross-checked against a live `POST`. |
| `bundle_id_patch_response.json` | Documented shape | `PATCH /v1/bundleIds/{id}`'s success response, after a `name` convergence. |
| `capabilities_list_empty.json` | Documented shape | `GET /v1/bundleIds/{id}/bundleIdCapabilities` with no capability enabled yet. |
| `capabilities_list_with_push.json` | Documented shape | The same endpoint with one capability (`PUSH_NOTIFICATIONS`) already enabled. |
| `capability_post_created.json` | Documented shape | `POST /v1/bundleIdCapabilities`'s success response. |
| `error_403.json` | Documented shape | The generic `ErrorResponse` shape (research note, section 2); `willikins-providers-http`'s own `provider_error_from_body` maps any `403` to a fixed `MISSING_PERMISSION` message regardless of body content, so this fixture's exact fields are not load-bearing. |
| `error_5xx.json` | Documented shape | Same `ErrorResponse` shape, generic `5xx` case. |
| `certificate_list_one.json` | Documented shape + pre-flight | `GET /v1/certificates?filter[certificateType]=...&filter[serialNumber]=...`'s list shape, one exact match, `DISTRIBUTION`, unexpired, `activated` absent (the pre-flight's own observation: absent on all 5 real certificates) -- `appstore.certificate.get`'s `Present` arm. |
| `certificate_list_empty.json` | Documented shape | The same list endpoint with zero rows -- the `NotFound` arm. |
| `certificate_list_substring_neighbor.json` | Documented shape + pre-flight | One row whose `serialNumber` is a strict *superstring* of the requested serial (`...3D4E` + `FF`) -- proves the byte-exact compare, not the (proven-substring, pre-flight) filter, decides the match; the requested serial reads `NotFound`. |
| `certificate_list_neighbor_only_page_one.json` | Documented shape | The same substring-neighbor row, used as page one of a two-page pagination test whose page two is `certificate_list_one.json` -- mirrors `bundle_id_list_prefix_neighbor.json` + `bundle_id_list_one.json`'s own pairing. |
| `certificate_list_two.json` | Documented shape | Two rows both matching `certificateType` and `serialNumber` exactly -- the `Conflict` (ambiguous) arm; a defensive fixture, since Apple documents serial numbers as unique in practice. |
| `certificate_list_expired.json` | Documented shape | One exact match whose `expirationDate` is in the past -- the `Conflict` (expired) arm. |
| `certificate_list_deactivated.json` | Documented shape + pre-flight | One exact match with `"activated": false` -- the `Conflict` (deactivated) arm. `activated` is never `false` on the operator's own 5 certificates (pre-flight), so this shape is invented from the documented attribute, not observed live. |
| `certificate_list_activated_true.json` | Documented shape | One exact match with `"activated": true` -- the `Present` arm, distinct from the (also-`Present`) absent-`activated` case `certificate_list_one.json` already covers. |
| `certificate_list_wrong_type.json` | Documented shape | One row whose `serialNumber` matches exactly but `certificateType` does not (`DEVELOPER_ID_APPLICATION_G2`) -- proves the byte-exact compare checks *both* fields, not serial alone; the requested type+serial pair reads `NotFound`. |
| `profile_list_one.json` | Documented shape + pre-flight | `GET /v1/bundleIds/{id}/profiles?fields[profiles]=...`'s list shape, one exact `name` match, `IOS_APP_STORE`, `ACTIVE`, unexpired -- `appstore.profile.ensure`'s list-search step finding its one row. |
| `profile_list_empty.json` | Documented shape | The same relationship endpoint with zero rows -- the `Absent` arm. |
| `profile_list_substring_neighbor.json` | Documented shape | One row whose `name` is a strict superstring of the requested name (`...profile` + `-extra`) -- proves the byte-exact compare, not a filter, decides the match (this tool never uses `filter[name]` at all -- decision (e)); the requested name reads `Absent`. |
| `profile_list_neighbor_only_page_one.json` | Documented shape | The same substring-neighbor row, used as page one of a two-page pagination test whose page two is `profile_list_one.json` -- mirrors the certificate and bundle id pagination pairs above. |
| `profile_list_two.json` | Documented shape | Two rows both matching `name` exactly -- the `Conflict` (ambiguous) arm; name uniqueness per identifier is unverified before the live cycle runs (verify item 1). |
| `profile_get_present_healthy.json` | Documented shape + pre-flight | `GET /v1/profiles/{id}?include=certificates&fields[profiles]=...`'s single-instance shape: `IOS_APP_STORE`, `ACTIVE`, unexpired, exactly one `certificates` relationship member, `profileContent` present -- the `Present` arm. |
| `profile_get_wrong_type.json` | Documented shape | Same instance, `profileType` changed to `IOS_APP_ADHOC` -- the `Mismatch { profile_type }` arm (checked before the certificate relationship, per decision (d)/(e)'s stated order). |
| `profile_get_wrong_certificate.json` | Documented shape | Same instance, the one related certificate's `id` changed -- the `Mismatch { certificate }` arm. |
| `profile_get_two_certificates.json` | Documented shape | Same instance, two related certificates instead of one -- also `Mismatch { certificate }`: "not exactly one" covers both zero and more than one. |
| `profile_get_invalid.json` | Documented shape | Same instance, `profileState` changed to `INVALID` (expiry left in the future, matching the pre-flight's own observation that `INVALID` has causes other than expiry) -- the `Conflict` (invalid) arm. |
| `profile_get_expired.json` | Documented shape | Same instance, `expirationDate` moved to the past, `profileState` left `ACTIVE` -- the `Conflict` (expired) arm, independent of `profileState` per decision (d). |
| `profile_post_created.json` | Documented shape | `POST /v1/profiles`'s success response, `profileContent` present in the `201` body. |
| `profile_post_created_no_content.json` | Documented shape | The same create response with `profileContent` omitted -- exercises `ensure`'s follow-up `GET` (verify item 5, unsettled until the live cycle runs). |
| `profile_post_created_links_only.json` | Apple's JSON:API shape (task-3 adversarial pass) | `POST /v1/profiles`'s `201` as Apple sends it without an `include`: every relationship carries `links` (and `meta`) but **no `data`**. The task-2 client required `relationships.certificates.data`, so this real shape failed to parse, and the created profile's id was lost with it -- `tests/profile_create_response_shapes.rs`. |
| `profile_list_one_links_only.json` | Apple's JSON:API shape (task-3 adversarial pass) | The relationship list read with the same links-only `certificates` relationship on its row, in case Apple returns relationships the `fields[profiles]` list did not name. |

None of the certificate fixtures' ids, serials, or names are the operator's own -- `C3RT1F1CATE1`..`C3RT1F1CATE7` and the `7B3F...`/`AA11...` serials are invented, chosen only to match the pre-flight's own *observed shape* (uppercase hex; the pre-flight saw 30 to 32 characters on all 5 real certificates, and these fixtures use up to 36 -- still well inside `AppleCertificateSerial`'s generous `{1,64}` bound, not a claim that 36 was itself observed live; a `DISTRIBUTION` certificate's `name` beginning `Apple Distribution`).

None of the profile fixtures' ids, names, or content are the operator's own either -- `PR0F1LE1D0001` and its siblings are invented ids in the same made-up-but-plausible shape as the certificate and bundle id fixtures, and every `profileContent` value is the base64 encoding of a plain descriptive sentence (`willikins-example-profile-content-example`), never a real CMS-signed property list and never anything decodable back to Apple-shaped data.

**Nothing here is live-verified yet.** This crate makes no live API call
during ordinary `cargo test --workspace` — see `tests/live_probe.rs`
(read-only, `#[ignore]`) and `tests/live_write_cycle.rs` (guarded,
`#[ignore]`, requires the `live-tests` feature and
`WILLIKINS_LIVE_TESTS=1`) for how to record real responses under
`fixtures/appstore/live/` (gitignored — never committed, since a real
response carries the operator's own team's identifiers).
