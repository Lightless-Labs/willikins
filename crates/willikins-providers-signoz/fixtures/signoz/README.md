# SigNoz fixtures

Recorded and hand-authored response bodies used by this crate's mock-server
tests. See `docs/research/2026-09-20-signoz-ingestion-keys.md` for the
primary source these shapes rest on — every fact it states was confirmed
live against the operator's own account on 2026-09-21 (that research
document's own header).

None of these carry a real API key or a real minted ingestion key value.
`ingestion_keys_list_present.json`'s `value` field is a deliberately
distinctive, obviously-fake marker (`FIXTURE-VALUE-MARKER-DO-NOT-LEAK`),
used only to prove `tests/ingestion_key_ensure_mock.rs`'s
`read_reports_present_with_an_unknown_key_when_listed` and
`tests/redaction.rs` never let it reach an output, an error message, or the
journal — never a real SigNoz ingestion key.

| File | Verified | Notes |
| --- | --- | --- |
| `ingestion_keys_list_absent.json` | Documented shape | `{status, data: []}` — an empty list, from the research note's documented `GET .../ingestion_keys` envelope. |
| `ingestion_keys_list_present.json` | Documented shape, `value` field confirmed live | One entry, with every field the research note's "confirmed live" list names (`created_at, expires_at, id, limits, name, tags, updated_at, value, workspace_id`). `value`'s presence here is the whole reason `signoz.ingestion_key.ensure` exists in the shape it does — see the tool's own module doc. |
| `ingestion_key_post_created.json` | Documented shape | `{status, data: {id, value}}` — create's `201` response. `value` here is `SIGNOZ_KEY_PLACEHOLDER`, a plain non-secret-shaped string that still satisfies `SigNozIngestionKeyValue`'s permissive parse (no exact grammar is documented, unlike a Doppler or GitHub token), so no runtime substitution is needed the way the Doppler fixtures' token placeholder needs. |
| `error_409_already_exists.json` | Documented shape, confirmed live | `{status: "error", error: {type: "already-exists", code: "already_exists", message: "key: <name> already exists"}}` — the research note's own quoted shape for a duplicate `name`. |
| `error_403_missing_scope.json` | Documented, not verbatim | The research note states a `403` names one of `ingestion-key:create`/`ingestion-key:list`/`ingestion-key:delete` verbatim and was observed live, but its exact body text was not re-quoted into the research note itself. This fixture is a representative shape, not a captured one — and it costs this crate nothing either way: `willikins_providers_http::http::provider_error_from_body` drops every `401`/`403` body outright before any of it becomes a `String`, so no willikins code branches on this fixture's exact text (`tests/ingestion_key_ensure_mock.rs`'s `a_403_is_the_fixed_missing_permission_message_and_echoes_no_scope_text` pins that). |
| `error_5xx.json` | Undocumented shape | SigNoz documents no `5xx` body shape at all (matching every other provider this workspace calls); used generically for `5xx` in tests since no willikins code parses it beyond the bare status. |

**Written, compiled, not yet run: the live write cycle against the
operator's account.** `tests/live_write_cycle.rs` is implemented (a
panic-safe `KeyGuard`, a precondition that the account holds exactly the
seven expected keys before minting anything, a postcondition that it
holds exactly those seven again after cleanup) and compiles under
`--features live-tests` (`cargo check -p willikins-providers-signoz
--features live-tests --tests`), but has not been run against the real,
**production** account — that is deliberately left for the operator to
run themselves, with the exact command in the test file's own doc
comment. When it does run, its own recordings go under
`fixtures/signoz/live/` (gitignored — never committed, since a real
recording would carry the operator's own account identity and, before
redaction, real minted key values), and this table gains rows the same
way `willikins-providers-buildkite/fixtures/buildkite/README.md`'s does
once its own live write cycle ran.

**Deliberately not modelled**: `GET .../ingestion_keys/search`, `PUT
.../ingestion_keys/{keyId}`, and the `/limits` family. `DELETE
.../ingestion_keys/{keyId}` *is* modelled
(`SigNozClient::delete_ingestion_key`), used only by the live write
cycle's own cleanup — no tool in this crate calls it, exactly as
`BuildkiteClient::delete_pipeline` is used only by its own crate's live
write cycle. None of the milestone's tool needs the other three; `read`
always lists and filters client-side rather than searching, since
whether `name` is filterable
server-side was never confirmed live (research note, "Verify before
relying on them").
