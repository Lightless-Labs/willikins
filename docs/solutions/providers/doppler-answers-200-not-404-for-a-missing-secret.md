---
title: "Doppler answers 200 with a null value for a secret that does not exist, never 404"
category: providers
tags: [doppler, rest, error-handling, not-found, redaction, live-test]
module: willikins-providers-doppler
symptom: "doppler.secret.get on a name that is not in the config returns ToolErrorKind::Provider with 'could not parse the response body as the expected shape', instead of NotFound naming the key"
root_cause: "Doppler's GET /v3/configs/config/secret answers 200 with value.computed set to null for a missing secret; the response struct deserialized computed straight into DopplerSecretValue, so the null failed the whole body parse and the 404 arm that would have produced NotFound was never reached"
date: 2026-09-14
---

# Doppler's missing secret is a 200, not a 404

## Symptom

`doppler.secret.get` on a secret name that does not exist in the config fails with
`ToolErrorKind::Provider` and the message
`reading \`<project>/<config>#<NAME>\`: could not parse the response body as the expected shape
(line 1, column 70)`.

An operator reads that as "willikins is broken", not as "you asked for a secret that is not
there". The milestone 2 plan's port table promises the opposite: "404 -> `ToolError::NotFound`
naming the key".

## Root cause

Doppler does not answer `404` for a secret that is not in the config. It answers **`200`** with
the value object present and `computed` set to `null`:

```json
{"name": "WILLIKINS_DOES_NOT_EXIST", "value": {"raw": null, "computed": null, "note": ""}}
```

`SecretValueBody` deserialized `computed` straight into `DopplerSecretValue`, a non-optional
secret domain type. A `null` therefore failed the whole body parse inside
`willikins_providers_http::Http::finish`, which reports a deliberately content-free
`could not parse the response body as the expected shape (line N, column M)`. The `Some(404)`
arm in `DopplerSecretGet::lookup` — the one that produces `NotFound` — was unreachable against
the real API.

Nothing in the research note could have caught this: every `/reference/*` OpenAPI page Doppler
publishes defines a `200` response only. No fetched page defines a non-2xx schema for any
endpoint at all.

## How it was found

`crates/willikins-providers-doppler/tests/live_write_cycle.rs`, the opt-in live write cycle,
on its first run. Steps 1 through 7 passed; step 8 asserted `NotFound` and got `Provider`.

Two details made the diagnosis possible without ever reading the response body, which trust
boundary 5 forbids:

- the test **records the error kind and message as an observation before asserting on them**, so
  a failing assertion still leaves the evidence behind;
- the parse position in the message (`line 1, column 70`) is willikins' own observation, not
  response text, and counting the body forward from `{"name":"WILLIKINS_DOES_NOT_EXIST","value":{"raw":null,"computed":`
  puts column 70 at the end of the second `null` — which named the field without quoting it.

## Fix

`SecretValueBody::computed` is `Option<DopplerSecretValue>`, the same shape
`ProjectBody::description` and `ConfigBody::root` already use, so a `null` and a missing key
both land on one safe answer instead of failing the parse. `DopplerClient::get_secret` returns
`Result<Option<DopplerSecretValue>, ProviderError>` and `DopplerSecretGet::lookup` maps `None`
to `not_found(...)` naming the key.

A `computed` that is present but unusable (an empty string, a number) still fails the parse and
still reports `Provider`. That is a malformed response, not an absence, and reporting it as
"no such secret" would hide a real problem.

The live write cycle's step 8 now passes end to end: `doppler.secret.get` on
`WILLIKINS_DOES_NOT_EXIST` in a real config answers `NotFound` naming the key, which also
confirms the inferred body shape.

`willikins-providers-fake`'s `doppler.secret.get` has always answered `NotFound` for an
unseeded secret. Before this fix the live tool could not produce that error at all against the
real API, so the fake and the live tool disagreed in the one place a workflow author would
notice. They now agree.

## What to take from it

- **A provider's published OpenAPI pages document the happy path.** Doppler's define a `200`
  and nothing else. Any error-shape assumption drawn from them is a guess until a live call
  checks it, which is exactly what the milestone plan's "Verify before relying on them" list
  exists for. `404 -> NotFound` looked like a fact and was an assumption.
- **Observe before asserting in a live test.** An assertion that fails tells you what did not
  happen. A recorded observation tells you what did. The one-line change that printed the kind
  and message before the assertion is what turned a second run into a diagnosis.
- **Doppler uses `400` where a reader of its docs would expect `404` or `409`.** The same
  run found `POST /v3/projects` with an existing name answering `400`, `POST /v3/configs` with
  an unprefixed branch-config name answering `400`, and a `GET` of a project immediately after
  its own `DELETE` answering `400` on one project and `404` on the other in the same run. A
  test that pins an exact non-2xx status against this API is pinning something the API does
  not promise; assert what the status *means* (not readable, refused) and record the number as
  an observation.
- **A redaction list built from documentation misses what documentation omits.** The same run
  recorded a real `GET /v3/configs/config/tokens` response, whose every entry carries
  `"token_preview": "dp.st…<six characters>"` — six real characters of a live service token.
  The research note says that endpoint omits `key`, which is true, and Doppler's own documented
  example carries no preview at all, so nothing in the authored material named the field and
  nothing redacted it. The recording was written to disk with the preview intact. It is now in
  `tests/common/mod.rs`'s `REDACTED_FIELD_NAMES` beside `key`. The general lesson: a
  redact-by-field-name list is only as complete as the schema it was built from, so the first
  live recording of any endpoint deserves to be read by a human before it is trusted.
