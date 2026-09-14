# GitHub fixtures

Recorded/authored response bodies for `willikins-providers-github`'s mock-server tests,
each shaped from `docs/research/2026-09-12-m2-dependencies.md` section 2, which quotes
GitHub's published OpenAPI description and troubleshooting docs verbatim.

**Status: verified for every endpoint the tools call (2026-09-14).** These were authored
from the documented schemas (property names, requiredness, the one real captured `422`
example the research note quotes), not recorded from a live call. Two opt-in live tests
have since been run against the sandbox org and closed that gap:

- `tests/live_probe.rs` (`#[ignore]`, `WILLIKINS_LIVE_PROBE=1`) — read-only: `/user`,
  `/orgs/{org}`, a deliberate `404`, and, if the org holds a repository, that repository
  and its Actions public key.
- `tests/live_write_cycle.rs` (`#[ignore]`, `WILLIKINS_LIVE_TESTS=1`) — the write cycle:
  it creates a real private repository, marks it ours, refuses a visibility change, writes
  and rewrites an Actions secret with a synthetic token, records the three fixtures the
  read-only probe could not reach, and deletes the repository again.

**No authored fixture needed changing after either run.** The write cycle's three
recordings carry every key their authored fixture names, with the same JSON type:
`repo_get_present.json`'s 11 authored keys against the live response's 97,
`actions_secret_public_key.json`'s `key_id`/`key` exactly, and
`actions_secret_get_present.json`'s `name`/`created_at`/`updated_at` exactly — GitHub's
`GET .../actions/secrets/{name}` really does answer with those three fields and no value
field of any kind. `user.json`'s and `org.json`'s every field was likewise present in the
probe's live response with the same JSON type (`description` `null` on the org included).
The comparison is deliberately a subset check, so the dozens of extra fields a real
response carries are not drift.

The write cycle's repository is always named **`willikins-live-write-cycle`**, in the org
`WILLIKINS_SANDBOX_GITHUB_ORG` names. The test refuses to run if one already exists, and a
`Drop` guard deletes the repository on every exit path — but **a leftover is deleted by
hand**: nothing in this crate deletes a repository, the guard reaches
`willikins_providers_http::Http::delete` directly, and a leftover means the guard itself
could not (GitHub answers `DELETE /repos/{owner}/{repo}` with `204`, or `403` when an org
owner has configured the org to prevent members from deleting organization-owned
repositories).

| File | Endpoint | Shape source | Probe status |
| --- | --- | --- | --- |
| `repo_get_present.json` | `GET /repos/{owner}/{repo}` (200, ours, topic present) | OpenAPI `full-repository` schema fields we read (`visibility`, `topics`); other fields are typical values, not schema-derived | verified 2026-09-14 by `live_write_cycle` — every authored key present, same types; `visibility: private` and `topics: ["managed-by-willikins"]` as authored |
| `repo_get_foreign.json` | `GET /repos/{owner}/{repo}` (200, topic absent) | same schema, `topics: []` | unverified — no live call is authored for it (the cycle never creates a repository it does not own) |
| `repo_post_created.json` | `POST /orgs/{org}/repos` (201) | same schema | exercised live 2026-09-14 by `live_write_cycle` (the create landed and the repository read back ours), shape not recorded — the client discards this body |
| `repo_topics_put.json` | `PUT /repos/{owner}/{repo}/topics` (200) | `{"names": [...]}`, documented required field | exercised live 2026-09-14 by `live_write_cycle` (the topic was present on the following raw `GET`), shape not recorded — the client discards this body |
| `actions_secret_public_key.json` | `GET .../actions/secrets/public-key` (200) | `actions-public-key` schema; `key` is the OpenAPI spec's own example value | verified 2026-09-14 by `live_write_cycle` — `key_id` and `key` present, both strings, and the live key sealed a real secret GitHub accepted |
| `actions_secret_get_present.json` | `GET .../actions/secrets/{name}` (200) | `actions-secret` schema: `name`, `created_at`, `updated_at`, no value field, ever | verified 2026-09-14 by `live_write_cycle` — exactly those three fields live, no value field |
| — (no fixture) | `PUT .../actions/secrets/{name}` (201/204) | the endpoint answers no body worth authoring | exercised live 2026-09-14 by `live_write_cycle`: a first write and a second write of the same name both succeeded, and the secret read back `Present` between them |
| `error_basic_404.json` | any `GET` (404) | `basic-error` schema | status only (2026-09-14) — both live tests observed a `404` (before the create and after the delete); its body was not compared |
| `error_already_exists_422.json` | `POST /orgs/{org}/repos` (422, real captured example) | research note section 2, a user-pasted real body (marked `(unverified)` there too — a real capture, not an official doc, but the shape this crate's `already_exists` detection is built against) | unverified — the cycle never provokes a duplicate create |
| `error_custom_name_422.json` | `POST /orgs/{org}/repos` (422, GitHub's other "name taken" shape) | research note section 2, `errors[].code: "custom"` on `field: "name"` | unverified — same reason |
| `user.json` | `GET /user` (200) | *not* research-note-sourced (out of its scope, section 2 covers repos/topics/secrets only) — a handful of well-known `public-user`/`private-user` schema fields, for the live probe's key-set comparison only | verified 2026-09-14 by `live_probe` — every authored key present, same types |
| `org.json` | `GET /orgs/{org}` (200) | same caveat as `user.json`, the `organization-full` schema | verified 2026-09-14 by `live_probe` — every authored key present, same types |

`live/` holds both live tests' own recordings and **is gitignored**. They carry no secret
(checked after the 2026-09-14 runs: no credential-shaped bytes, no `dp.st.`-shaped bytes,
and no `token`/`secret`/`value`/`encrypted_value` field at all in any recording), but they
do carry the sandbox org's and the operator's own identity — which is exactly what
`WILLIKINS_SANDBOX_GITHUB_ORG` names — so they stay out of the repository. Nothing in this
crate depends on their presence; each test creates the directory itself.
