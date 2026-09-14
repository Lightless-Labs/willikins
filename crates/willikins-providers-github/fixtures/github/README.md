# GitHub fixtures

Recorded/authored response bodies for `willikins-providers-github`'s mock-server tests,
each shaped from `docs/research/2026-09-12-m2-dependencies.md` section 2, which quotes
GitHub's published OpenAPI description and troubleshooting docs verbatim.

**Status: unverified.** None of these has been checked against a real GitHub response.
They are authored from the documented schemas (property names, requiredness, the one
real captured `422` example the research note quotes) rather than recorded from a live
call. Task 7's read-only live probe (`tests/live_probe.rs`, `#[ignore]`, opt-in via
`WILLIKINS_LIVE_PROBE=1`) writes real responses to `live/` and diffs their top-level key
set against these; when that probe has run once against the sandbox org, this note
should be updated to say which fixtures it confirmed, or these files replaced with the
real shapes if they differ.

| File | Endpoint | Shape source |
| --- | --- | --- |
| `repo_get_present.json` | `GET /repos/{owner}/{repo}` (200, ours, topic present) | OpenAPI `full-repository` schema fields we read (`visibility`, `topics`); other fields are typical values, not schema-derived |
| `repo_get_foreign.json` | `GET /repos/{owner}/{repo}` (200, topic absent) | same schema, `topics: []` |
| `repo_post_created.json` | `POST /orgs/{org}/repos` (201) | same schema |
| `repo_topics_put.json` | `PUT /repos/{owner}/{repo}/topics` (200) | `{"names": [...]}`, documented required field |
| `actions_secret_public_key.json` | `GET .../actions/secrets/public-key` (200) | `actions-public-key` schema; `key` is the OpenAPI spec's own example value |
| `actions_secret_get_present.json` | `GET .../actions/secrets/{name}` (200) | `actions-secret` schema: `name`, `created_at`, `updated_at`, no value field, ever |
| `error_basic_404.json` | any `GET` (404) | `basic-error` schema |
| `error_already_exists_422.json` | `POST /orgs/{org}/repos` (422, real captured example) | research note section 2, a user-pasted real body (marked `(unverified)` there too — a real capture, not an official doc, but the shape this crate's `already_exists` detection is built against) |
| `error_custom_name_422.json` | `POST /orgs/{org}/repos` (422, GitHub's other "name taken" shape) | research note section 2, `errors[].code: "custom"` on `field: "name"` |
| `user.json` | `GET /user` (200) | *not* research-note-sourced (out of its scope, section 2 covers repos/topics/secrets only) — a handful of well-known `public-user`/`private-user` schema fields, for the live probe's key-set comparison only |
| `org.json` | `GET /orgs/{org}` (200) | same caveat as `user.json`, the `organization-full` schema |

`live/` holds the probe's own recordings (gitignored is not needed — they carry no
secret, per the plan's redaction step — but nothing in this crate depends on their
presence; the probe creates the directory itself).
