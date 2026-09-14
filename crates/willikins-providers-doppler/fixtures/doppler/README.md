# Doppler fixtures

Recorded/authored response bodies for `willikins-providers-doppler`'s mock-server tests,
each shaped from `docs/research/2026-09-12-m2-dependencies.md` section 3, which quotes
Doppler's published OpenAPI schemas verbatim.

**Status: entirely unverified (2026-09-14).** Unlike `willikins-providers-github`'s
fixtures, none of these have been checked against a live response. Task 8's read-only live
probe (`tests/live_probe.rs`, `#[ignore]`, opt-in via `WILLIKINS_LIVE_PROBE=1`) is written
but has not run: the credential in the operator's sandbox environment
(`WILLIKINS_DOPPLER_TOKEN`) is a `dp.st.` service token, scoped to secrets-only access
within one config, which this crate's own credential regex (`^dp\.(sa|pt)\.[a-zA-Z0-9]{40,44}$`)
refuses by design — a service token cannot provision, and provisioning is exactly what
`credential_from_env` exists to gate. The probe stays unrun, and every fixture below stays
unverified, until the operator supplies a `dp.sa.` (service account) or `dp.pt.` (personal)
token.

Doppler documents no error-body schema for any non-2xx response at all (research note
section 3, "Facts that still need a browser"): `error_404.json` and `error_5xx.json` are
therefore not shaped from any documented schema, only from a WebSearch-sourced guess
(`{"success": false, "messages": [...]}`) the milestone 1 research doc already flagged as
unconfirmed. The live probe's own job, besides refreshing every other fixture here,
includes recording what a real `404` body actually looks like (the milestone plan's
"Verify before relying on them" item 4).

| File | Endpoint | Shape source |
| --- | --- | --- |
| `project_get_present.json` | `GET /v3/projects/project` (200, ours: `description` carries the `managed-by: willikins` marker) | OpenAPI `projects-get`/`projects-create` example shape |
| `project_get_foreign.json` | `GET /v3/projects/project` (200, `description` does not carry the marker) | same schema, Doppler's own example `description` text |
| `project_post_created.json` | `POST /v3/projects` (201) | same schema |
| `config_get_present.json` | `GET /v3/configs/config` (200, `root: true`) | OpenAPI `configs-get`/`configs-create` example shape |
| `environment_post_created.json` | `POST /v3/environments` (201) | OpenAPI `environments-create`/`environments-get` example shape |
| `service_tokens_list_present.json` | `GET /v3/configs/config/tokens` (200, one token named `ci`) | OpenAPI `service_tokens-list` example shape — note it omits `key` and `access`, unlike the create response |
| `service_tokens_list_absent.json` | `GET /v3/configs/config/tokens` (200, empty) | same schema, empty `tokens` array |
| `service_token_post_created.json` | `POST /v3/configs/config/tokens` (200) | OpenAPI `service_tokens-create` example shape; `key` is Doppler's own documented example token value |
| `service_token_delete.json` | `DELETE /v3/configs/config/tokens/token` (200) | OpenAPI `service_tokens-delete` example shape: `{"success": true}` |
| `secret_get.json` | `GET /v3/configs/config/secret` (200) | OpenAPI `secrets-get` example shape; `raw` and `computed` deliberately differ, exercising that this crate reads only `computed` |
| `error_404.json` | any `GET` (404) | **unconfirmed** — no fetched OpenAPI page defines a non-2xx schema; a WebSearch-sourced guess only |
| `error_5xx.json` | any request (5xx) | same caveat as `error_404.json` |

`live/` (created by the probe on its first real run, not present in this repository yet)
will hold the probe's own recordings, redacted, and is gitignored — they will carry the
sandbox project's and the operator's own identity, which is exactly what
`WILLIKINS_SANDBOX_DOPPLER_PROJECT` names.
