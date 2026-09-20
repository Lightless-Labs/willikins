# Buildkite fixtures

Recorded and hand-authored response bodies used by this crate's mock-server
tests. See `docs/research/2026-09-16-m3a-buildkite.md` for the primary
sources these shapes rest on, and that note's section 5 ("Verify with a
browser") for what remains open until the live probe (task 5) runs.

None of these carry a real credential or a real webhook secret. The
`provider.webhook_url` value in `pipeline_get_present.json` is a
deliberately distinctive, obviously-fake marker
(`FIXTURE-WEBHOOK-MARKER-DO-NOT-LEAK`), used only to prove
`tests/redaction.rs` never lets it reach an output, an error message, or
the journal — never a real Buildkite delivery URL.

| File | Verified | Notes |
| --- | --- | --- |
| `pipeline_get_present.json` | Documented shape | Modelled on the create response example in `docs/apis/rest-api/pipelines.md#create-a-yaml-pipeline`, extended with a `provider.webhook_url` and non-empty `steps`/`configuration` so `tests/redaction.rs` has a credential-bearing value and forbidden fields to prove are never parsed. Not yet cross-checked against a live `GET`; task 5's live probe supersedes this once it runs (sandbox token issued 2026-09-16, expires ~2026-09-23). |
| `pipeline_get_foreign.json` | Documented shape | Same object with `description: null` — Buildkite's create response shows `description` as nullable. |
| `pipeline_get_mismatch_repo.json` | Documented shape | Same object with a different `repository`. |
| `pipeline_get_mismatch_cluster.json` | Documented shape | Same object with a different `cluster_id`. |
| `pipeline_post_created.json` | Documented shape | The create response's required fields, from the same page. |
| `error_5xx.json` | Documented shape | The bare `{"message": "..."}` shape the pipelines page documents for `403`; used generically for `5xx` in tests since Buildkite documents no `5xx` body at all. |
| `error_403.json` | Documented, verbatim | `docs/apis/rest-api/pipelines.md`'s own `403 Forbidden` example body. |
| `clusters_list_page.json` | Documented shape | One cluster object, from `docs/apis/rest-api/clusters.md`'s field table; not a captured live response. The `404` body for an unknown pipeline slug, the real `GET /v2/access-token` scope strings, and the real cluster list shape are all in the research note's "Verify with a browser" list and are answered by the live probe (task 5), not by this fixture set. |

**Still to record from a live call** (task 5's live probe, then task 12's
live write cycle): `access_token_probe.json` (scrubbed — no token bytes),
`pipeline_get_absent_404.json` (status and body, if any), a real
`clusters_list.json`. Until then, every test above rests on the
documented shape only, exactly as this table says, and no willikins code
branches on anything these fixtures do not also justify (decision (d):
`read` and `ensure` never parse a create failure's body, and the `404`
arm rests on the status alone).
