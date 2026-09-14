# GitHub fixtures

Recorded/authored response bodies for `willikins-providers-github`'s mock-server tests,
each shaped from `docs/research/2026-09-12-m2-dependencies.md` section 2, which quotes
GitHub's published OpenAPI description and troubleshooting docs verbatim.

**Status: partly verified (2026-09-14).** These were authored from the documented
schemas (property names, requiredness, the one real captured `422` example the research
note quotes), not recorded from a live call. Task 7's read-only live probe
(`tests/live_probe.rs`, `#[ignore]`, opt-in via `WILLIKINS_LIVE_PROBE=1`) has now been run
once against the sandbox org; it writes real responses to `live/` and checks that every
key an authored fixture names is present in the live one. The "probe status" column below
says what that run reached.

No authored fixture needed changing. `user.json`'s and `org.json`'s every field was
present in the live response with the same JSON type (`description` `null` on the org
included); the live responses carry 34 and 63 top-level fields against the 5 and 6
authored here, which the probe's deliberate subset comparison allows.

The repository and Actions public-key endpoints were **skipped, not failed**: the sandbox
org holds no repository, and both `GET /repos/{owner}/{repo}` and `GET
.../actions/secrets/public-key` need one. Re-run the probe once the org has a repository
to close `repo_get_present.json` and `actions_secret_public_key.json`.

| File | Endpoint | Shape source | Probe status |
| --- | --- | --- | --- |
| `repo_get_present.json` | `GET /repos/{owner}/{repo}` (200, ours, topic present) | OpenAPI `full-repository` schema fields we read (`visibility`, `topics`); other fields are typical values, not schema-derived | unverified — skipped, the sandbox org has no repository |
| `repo_get_foreign.json` | `GET /repos/{owner}/{repo}` (200, topic absent) | same schema, `topics: []` | unverified — no live call is authored for it |
| `repo_post_created.json` | `POST /orgs/{org}/repos` (201) | same schema | unverified — the probe is read-only |
| `repo_topics_put.json` | `PUT /repos/{owner}/{repo}/topics` (200) | `{"names": [...]}`, documented required field | unverified — the probe is read-only |
| `actions_secret_public_key.json` | `GET .../actions/secrets/public-key` (200) | `actions-public-key` schema; `key` is the OpenAPI spec's own example value | unverified — skipped, needs a repository |
| `actions_secret_get_present.json` | `GET .../actions/secrets/{name}` (200) | `actions-secret` schema: `name`, `created_at`, `updated_at`, no value field, ever | unverified — needs a repository and a secret |
| `error_basic_404.json` | any `GET` (404) | `basic-error` schema | status only (2026-09-14) — a live 404 was observed, its body was not compared |
| `error_already_exists_422.json` | `POST /orgs/{org}/repos` (422, real captured example) | research note section 2, a user-pasted real body (marked `(unverified)` there too — a real capture, not an official doc, but the shape this crate's `already_exists` detection is built against) | unverified — the probe is read-only |
| `error_custom_name_422.json` | `POST /orgs/{org}/repos` (422, GitHub's other "name taken" shape) | research note section 2, `errors[].code: "custom"` on `field: "name"` | unverified — the probe is read-only |
| `user.json` | `GET /user` (200) | *not* research-note-sourced (out of its scope, section 2 covers repos/topics/secrets only) — a handful of well-known `public-user`/`private-user` schema fields, for the live probe's key-set comparison only | verified 2026-09-14 — every authored key present, same types |
| `org.json` | `GET /orgs/{org}` (200) | same caveat as `user.json`, the `organization-full` schema | verified 2026-09-14 — every authored key present, same types |

`live/` holds the probe's own recordings and **is gitignored**. They carry no secret
(checked after the 2026-09-14 run: no credential-shaped bytes in either recording), but
they do carry the sandbox org's and the operator's own identity — which is exactly what
`WILLIKINS_SANDBOX_GITHUB_ORG` names — so they stay out of the repository. Nothing in
this crate depends on their presence; the probe creates the directory itself.
