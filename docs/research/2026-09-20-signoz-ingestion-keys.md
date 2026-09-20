# SigNoz ingestion keys: everything the tool needs

**Date:** 2026-09-20
**For:** a `signoz.ingestion_key.ensure` tool, and the `doppler.secret.set` sink it needs to be
useful. No further research should be required before building either.
**Sources:** SigNoz's own OpenAPI description, fetched 2026-09-20 from
<https://signoz.io/api/api-reference-openapi/latest/> (846 KB, YAML, found via
<https://signoz.io/llms.txt>), and the ingestion-keys page's markdown twin at
<https://signoz.io/docs/ingestion/signoz-cloud/keys.md>.

## 0. The fact that decides whether the tool exists at all

> "You don't need ingestion keys for self-hosted/community edition of SigNoz."
> "Ingestion keys are only applicable for cloud account of SigNoz."
> — <https://signoz.io/docs/ingestion/signoz-cloud/keys/>, fetched 2026-09-20

The operator uses SigNoz Cloud (confirmed 2026-09-20), so the tool is worth building. A
self-hosted deployment would need no such tool: it needs an endpoint, which is configuration.

## 1. Authentication

`securitySchemes` declares two. The relevant one:

```yaml
api_key:
  description: API Keys
  in: header
  name: SigNoz-Api-Key
  type: apiKey
```

So the credential goes in a `SigNoz-Api-Key` header, not `Authorization`. That matters for
`willikins-providers-http`: `Credential::authorize` sets `Authorization` and is the only sanctioned
outgoing-credential site, so this provider needs either a second sanctioned method on `Credential`
(the shape `authorize_basic` already established for the approvals client) or an explicit,
documented exemption. Do not solve it by building the header by hand at the call site.

Operations declare scopes, e.g. create is `api_key: [ingestion-key:create]`, so a scoped key is
possible and the tool's documentation should say which scopes it needs.

**A SigNoz API key is not an ingestion key.** The docs are emphatic: ingestion keys are write-only
and safe in client code; API keys "allow querying and reading data from your account. Never expose
API keys in frontend or client-side code." willikins holds the API key; it mints ingestion keys.

## 2. The operations

`servers` is templated — `https://{host}:{port}{base_path}` — so the region host is deployment
configuration, never a literal.

| Operation | Method and path |
| --- | --- |
| `CreateIngestionKey` | `POST /api/v2/gateway/ingestion_keys` |
| `GetIngestionKeys` | `GET /api/v2/gateway/ingestion_keys` |
| `SearchIngestionKeys` | `GET /api/v2/gateway/ingestion_keys/search` |
| `GetIngestionKey` | `GET /api/v2/gateway/ingestion_keys/{keyId}` |
| `UpdateIngestionKey` | `PUT /api/v2/gateway/ingestion_keys/{keyId}` |
| `DeleteIngestionKey` | `DELETE /api/v2/gateway/ingestion_keys/{keyId}` |
| limits | `.../{keyId}/limits`, `.../limits/{limitId}`, `/api/v2/gateway/ingestion_limits` |

## 3. Create: what goes in, what comes back

Request (`GatewaytypesPostableIngestionKey`): `name` **required**; `expires_at` (date-time) and
`tags` (array of string, nullable) optional.

Response `201` is `{status, data}` where data is `GatewaytypesGettableCreatedIngestionKey`:

```yaml
properties:
  id: {type: string}
  value: {type: string}
required: [id, value]
```

**The key's value is returned on create and only on create.** The read schema for an existing key
(`GatewaytypesGettableIngestionKeys`) carries no `value`. Documented failures on create: 401, 403,
500.

## 4. What that means for willikins, which is the useful part

This is the **exact shape willikins already handles twice**: a secret that exists, can be listed by
name, and whose value can never be re-read. `doppler.service_token.ensure` is that tool. So:

- Key: the ingestion key's `name`, not its id, because the id is only known after creation.
- `read`: list or search by name. Found → `Present` with the `key` output `Unknown`. Not found →
  `Absent` with `Unknown`. The same pattern, and the same reason, as the Doppler service token.
- `ensure`: create, and the create response is the one moment the value exists. It binds to a
  secret-accepting input only, and `check` already refuses anything else. `SinkToken` discipline
  applies unchanged.
- Class `Reversible`; a rotation tool, if ever wanted, is `Destructive` like the Doppler one.

**The missing link, and it is not SigNoz's.** A minted ingestion key is useful only if it lands
somewhere the app reads. The operator keeps every secret in Doppler, and willikins has **no tool
that writes a secret into a Doppler config** — `doppler.secret.get` reads, and the only sink in the
whole catalogue is `github.actions_secret.ensure`, which this operator does not use. So the SigNoz
work is two tools, not one:

1. `signoz.ingestion_key.ensure` — mints the key, outputs it as a secret.
2. `doppler.secret.set` — the sink that receives it, in a named config.

The second is what makes willikins' central claim (an agent routes a secret it can never read)
true in this operator's actual world for the first time. `POST /v3/configs/config/secrets` is the
Doppler endpoint; confirm its exact shape from the existing Doppler research before building, as
that half was not re-fetched today.

## 5. Verify before relying on them

- The exact list/search response shape and whether `name` is filterable server-side, which decides
  whether `read` lists and filters client-side or searches. Read `GatewaytypesGettableIngestionKeys`
  and the `/search` parameters from the same spec.
- What a duplicate `name` answers. Not documented in the operation's response list (401/403/500
  only), so it must be observed live, and until it is, `read`-then-create rather than
  create-and-catch, exactly as every other provider in this workspace does.
- The Doppler secret-write endpoint's shape and whether it replaces or merges.
- Whether a scoped API key limited to `ingestion-key:*` can also be used for the read path.
