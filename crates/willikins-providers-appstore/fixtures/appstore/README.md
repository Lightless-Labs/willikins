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

**Nothing here is live-verified yet.** This crate makes no live API call
during ordinary `cargo test --workspace` — see `tests/live_probe.rs`
(read-only, `#[ignore]`) and `tests/live_write_cycle.rs` (guarded,
`#[ignore]`, requires the `live-tests` feature and
`WILLIKINS_LIVE_TESTS=1`) for how to record real responses under
`fixtures/appstore/live/` (gitignored — never committed, since a real
response carries the operator's own team's identifiers).
