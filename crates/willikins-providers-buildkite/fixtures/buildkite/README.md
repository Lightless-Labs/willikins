# Buildkite fixtures

Recorded and hand-authored response bodies used by this crate's mock-server
tests. See `docs/research/2026-09-16-m3a-buildkite.md` for the primary
sources these shapes rest on, and the milestone plan's "Verify before
relying on them" section (updated 2026-09-20 with the live probe's results)
for what remains open.

**The live probe (task 5) ran on 2026-09-20** against the real
`willikins-test` organisation: `GET /v2/access-token` (no scope needed),
`GET /v2/organizations/willikins-test/clusters`, and `GET
.../pipelines/willikins-probe-does-not-exist`. All three passed
(`cargo test -p willikins-providers-buildkite --test live_probe -- --ignored
--nocapture`, `WILLIKINS_LIVE_PROBE=1`). The real, redacted responses are
under `fixtures/buildkite/live/` (gitignored — never committed, since they
carry the organisation's and the operator's own identity); the table below
records what each answered, not the bytes themselves.

None of these carry a real credential or a real webhook secret. The
`provider.webhook_url` value in `pipeline_get_present.json` is a
deliberately distinctive, obviously-fake marker
(`FIXTURE-WEBHOOK-MARKER-DO-NOT-LEAK`), used only to prove
`tests/redaction.rs` never lets it reach an output, an error message, or
the journal — never a real Buildkite delivery URL.

| File | Verified | Notes |
| --- | --- | --- |
| `pipeline_get_present.json` | Documented shape | Modelled on the create response example in `docs/apis/rest-api/pipelines.md#create-a-yaml-pipeline`, extended with a `provider.webhook_url` and non-empty `steps`/`configuration` so `tests/redaction.rs` has a credential-bearing value and forbidden fields to prove are never parsed. Not yet cross-checked against a live `GET` of an owned pipeline — that needs a create, which is task 12's live write cycle, not the read-only probe. |
| `pipeline_get_foreign.json` | Documented shape | Same object with `description: null` — Buildkite's create response shows `description` as nullable. |
| `pipeline_get_mismatch_repo.json` | Documented shape | Same object with a different `repository`. |
| `pipeline_get_mismatch_cluster.json` | Documented shape | Same object with a different `cluster_id`. |
| `pipeline_post_created.json` | Documented shape | The create response's required fields, from the same page. Not yet cross-checked against a live `POST`; task 12. |
| `error_5xx.json` | Documented shape | The bare `{"message": "..."}` shape the pipelines page documents for `403`; used generically for `5xx` in tests since Buildkite documents no `5xx` body at all. |
| `error_403.json` | Documented, verbatim | `docs/apis/rest-api/pipelines.md`'s own `403 Forbidden` example body. |
| `clusters_list_page.json` | **Live-verified shape** (2026-09-20) | One cluster object from `docs/apis/rest-api/clusters.md`'s field table. The live probe's real `GET /v2/organizations/willikins-test/clusters` returned exactly one cluster, named `Default cluster`, with every field this fixture's shape names present (plus several this crate never reads — `color`, `created_by`, `maintainers`, `emoji`, and others, all correctly ignored by `ClusterBody`'s two-field shape). |

**Live-verified, not fixture-shaped** (2026-09-20, `tests/live_probe.rs`, recorded redacted under
`fixtures/buildkite/live/`, gitignored): `GET /v2/access-token` answered with the plural scope
spellings (`read_pipelines`, `write_pipelines`, `read_clusters` all present) and an `expires_at`
matching the seven-day sandbox window; `GET .../pipelines/<absent-slug>` answered `404` with a
`message` field present. See the milestone plan's "Verify before relying on them" section for the
full readout.

**Still to record from a live call** (task 12's live write cycle, not yet run): a real
`pipeline_get_present`-shaped response from a pipeline this crate actually created, and the real
create response body. Until then, `pipeline_get_present.json` and `pipeline_post_created.json`
rest on the documented shape only, exactly as the table says, and no willikins code branches on
anything these fixtures do not also justify (decision (d): `read` and `ensure` never parse a
create failure's body, and the `404` arm rests on the status alone — both now live-confirmed for
the read side).
