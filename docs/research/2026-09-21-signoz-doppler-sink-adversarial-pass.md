# Adversarial pass: `signoz.ingestion_key.ensure` and `doppler.secret.set`

**Date:** 2026-09-21
**Subject:** the SigNoz mint / Doppler sink pair landed in `afefda9`..`c7527c1`, its
provenance rule, and its first live run.
**Verdict:** one blocking defect found and fixed; the provenance rule is real and
enforced; the live cycle ran end to end against the operator's production SigNoz
account and the sandbox Doppler workplace, and both were restored.

## 1. The blocking defect: the list envelope was never verified

`GET /api/v2/gateway/ingestion_keys` does **not** answer `{"status": ..., "data": [...]}`.
Observed live against the operator's account on 2026-09-21:

```
{"status": "success", "data": {"keys": [ ... ], "_pagination": { ... }}}
```

`IngestionKeyListEnvelope` declared `data: Vec<IngestionKeyListEntry>`, generalising from
create's own `{status, data}` envelope. Every `read` against the real API therefore failed
in `Http::finish`'s parse path with `could not parse the response body as the expected
shape (line 1, column 8)` — which is to say `signoz.ingestion_key.ensure` could never have
planned, let alone applied, against a real SigNoz account.

Nothing offline caught it because the two mock fixtures encoded the same guess. This is
the concrete answer to "does the fake agree with the live tool behaviourally rather than
by construction": `fake_agrees_with_live.rs` *is* behavioural (it drives read/ensure
through both tools and compares serialized `Observation`s), but both sides were being
compared against a mock of an API shape nobody had ever seen. Agreement with a fiction is
agreement by construction at one remove.

The research note had said so. `docs/research/2026-09-20-signoz-ingestion-keys.md`
section 5, "Verify before relying on them", opens with: *"The exact list/search response
shape and whether `name` is filterable server-side."* It was never verified, and the
implementing pass treated the task brief's "ALL THE SIGNOZ FACTS ARE ALREADY ESTABLISHED"
as covering it. The brief did enumerate the *entry* fields correctly; it did not describe
the envelope around them.

**Fixed** by `IngestionKeyListEnvelope { data: IngestionKeyListData }` /
`IngestionKeyListData { keys: Vec<IngestionKeyListEntry> }`, with both list fixtures and
the three inline bodies in `tests/redaction.rs` rewritten to the observed shape.

Proof of non-vacuity (mutation 1, and the strongest one available since it is a real
bug): rewriting the fixtures *first*, before the struct, turned 15 of the crate's tests
red with exactly the parse error a live call would have produced — 4 in
`fake_agrees_with_live.rs`, 8 in `ingestion_key_ensure_mock.rs`, 3 in `redaction.rs`.
Fixing the struct turned all 33 green.

`_pagination` is left undeclared: this client reads page one only. At eight keys the
operator's account returns it empty. A workplace past one page would make `read` report
`Absent` for a key that exists; the `409` belt-and-braces path in `ensure` catches that
and fails loudly rather than minting a duplicate, so the failure is safe but not one to
lean on. Recorded as remaining work rather than added to a tool being run live for the
first time.

## 2. The provenance rule: real, not documented

`doppler.secret.set`'s `config` port is `PortSpec::derived_only`, and `check` refuses four
binding kinds for it: a literal, a workflow input, a `for_each` item, and a *pure* node's
output. Two negative fixtures pin the first two:

- `workflows/fixtures/secret-set-literal-config.yaml`
- `workflows/fixtures/secret-set-input-config.yaml`

Both are refused with exactly one error, `UnderivedBinding { node: sink, port: config }`
(`crates/willikins-providers-doppler/tests/secret_set_provenance.rs`), and
`secret-set-derived-config.yaml` is accepted. Mutation 2: changing `is_derived_binding` to
return `true` unconditionally turns `underived_config_binding_is_refused` red; restoring
the file from a saved copy (not `git checkout`) leaves `git diff` empty and the test green.

Note what the mutation does *not* prove. A literal and a workflow input both reach the
`source: None` arm, so the mutation exercises that arm only. The pure-node exclusion —
the deliberate widening past the task brief's wording, aimed at a future pure tool that
could mint a `DopplerConfig` from literals and launder them through — has no fixture,
because no pure tool in the catalogue outputs a `DopplerConfig`. It is covered by
`check.rs`'s own unit tests, not by a document-level fixture, and that is worth saying
plainly rather than implying the fixtures cover it.

The tool's module docs are honest about the rule's limit: a document can still chain
`doppler.config.ensure` immediately before `secret.set` and write into a config it just
made. What the rule removes is the zero-cost attack — naming an existing, populated config
outright — and it makes every write name, in the approved plan, the specific config a node
in the same graph produced.

## 3. Secrets: where they can and cannot go

- **Never re-read.** `IngestionKeyListEntry` has no `value` field, so the list response's
  `value` (present live, absent from the OpenAPI schema) is discarded by serde before it
  is ever a `String` in the process. `read` and the already-present arm of `ensure` report
  the `key` output `Unknown`.
- **Never in a parse error.** `Http::finish` builds its 2xx parse failure from
  `err.line()`/`err.column()` only, never from the body — so a create response that failed
  to parse cannot carry the minted `value` into a `ToolError`. This was checked rather than
  assumed, because it is the one path where a 201 body holding a live key meets an error
  string.
- **Never in a 401/403.** `provider_error_from_body` drops those bodies outright.
- **Residual risk, reported not fixed.** For any *other* non-2xx status,
  `provider_error_from_body` builds the message from the response body's own
  `message`/`messages` text. `doppler.secret.set` is the first willikins tool that sends a
  secret in a request body, so a Doppler 4xx that echoed the rejected value back would put
  it into a `ToolError` and hence into the journal. Not observed (the live run's writes
  succeeded), not provoked, and not a regression — but it is new exposure that arrived with
  this tool, and it belongs to the sink, not to the HTTP layer's existing posture.

## 4. Live run

Document: `workflows/signoz-ingestion-key.yaml` — `doppler.config.ensure` →
`signoz.ingestion_key.ensure` → `doppler.secret.set`, with `config` bound to the config
node's own output as the provenance rule requires.

The project is an ordinary workflow input, and `doppler.project.ensure` is deliberately
*not* in this graph. An earlier draft of the document included it, and would have been
useless for the thing the operator actually wants: `doppler.project.ensure`'s `read`
reports `Observation::Foreign` for any project whose description is not willikins' own
`MANAGED_DESCRIPTION` marker, and `ensure` turns that into `already exists and is not
ours`. Every real project the operator already has fails that test. `doppler.config.ensure`
carries no ownership marker — it accepts any existing root config as `Present` and creates
one when the environment has none — so binding `config` to *its* output satisfies the
provenance rule without requiring willikins to have created the project.

SigNoz side: the operator's **production** account. Doppler side: the sandbox workplace
("Willikins - Test"), which held zero projects before and after.

See section 5 for the numbers. The minted key never appeared in the plan, the apply
output, or the journal: all three were scanned for a UUID-shaped run and for the key's own
length, and the scan is the evidence, not a visual reading.

## 5. Results

`plan --live` (read-only, and the moment the fixed envelope was first exercised against
the real API):

```
config (doppler.config.ensure): Create
key (signoz.ingestion_key.ensure): Create
    key: <unknown>
sink (doppler.secret.set): Create
class: Reversible   requires_approval: false
```

First `apply --live`: `config: Unchanged`, `key: Created` (rendered
`[REDACTED SigNozIngestionKeyValue]`), `sink: Created`, `state: succeeded`.

The `config` node planned `NoOp`, not `Create`. Doppler creates a root config for each of
`dev`/`stg`/`prd` when the project itself is created, so the `prd` root config already
existed by the time this document ran. The provenance property is unaffected — the binding
is still to a non-pure node's output — but "a config the same graph created" is, on this
run, "a config the same graph ensured". Worth stating rather than glossing.

**The routing, proven rather than asserted.** The Doppler secret `SIGNOZ_INGESTION_KEY`
came back present and 36 characters long, and its SHA-256 equals the SHA-256 of the
ingestion key's own value as SigNoz's list endpoint reports it. So the value that reached
Doppler is byte-for-byte the key SigNoz minted, and neither value was printed, logged or
written anywhere to establish that: both were hashed in memory and only the comparison's
outcome was recorded.

**Leak scan.** The minted value, fetched separately for this check alone, appears zero
times in the plan output, both apply outputs, the journal, and the whole repository tree
(`grep -cF`, counts only). The journal does carry two 36-character alphanumeric runs —
they are substrings of SHA-256 workflow hashes, not the key.

Second `apply --live`, same inputs: `config: Unchanged`, `key: Unchanged` (`key:
<unknown>`), `sink: Converged`, `state: succeeded`. Every node planned `NoOp`. This is the
empirical vindication of the implementing pass's corrected decision about
`doppler.secret.set`'s `read`: had it reported `Absent` unconditionally, this second run
would have planned `Create` on the sink with an `Unknown` `value` and hard-failed
`ApplyError::UnknownInput`.

**Teardown.** The ingestion key was deleted (`204`) and the sandbox Doppler project
deleted (`200`). The SigNoz account re-lists at exactly seven keys, the same seven names
it held before: `infrastructure`, `pessimal-ios`, `pocket-companion-prd`,
`claude-pessimal-test`, `phil-connors-prd-ios`, `phil-connors-prd-backend`,
`Danksworth-Ingestion`.

The sandbox Doppler workplace ("Willikins - Test") does **not** re-list at zero, and the
one project left is not this session's. It is `app-store-connect`, created
`2026-09-21T20:13:04Z` — while this session's gate was compiling and long before this
session made any Doppler write — with a null description, so it is neither willikins-managed
nor anything this pass created. The baseline taken before any work here was zero projects.
Another session is evidently working in the same sandbox workplace. It was left untouched:
deleting a project this pass did not make is not this pass's call.

## 6. What was not built

`env.get` and credential-as-a-port, which the task brief asked for, are not in this work.
The design doc's 2026-09-21 addendum describes them, but no credential-port kind exists in
`willikins-core`'s `Tool`/`ToolSpec`: every live provider, the two new tools included,
still takes its credential by client construction from the process environment. The
implementing pass deferred it with that reasoning recorded. The reasoning is defensible and
the deferral is not ratified here — it is a task requirement that was not met, and it stays
open.
