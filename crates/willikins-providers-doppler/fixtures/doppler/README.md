# Doppler fixtures

Authored response bodies for `willikins-providers-doppler`'s mock-server tests, each shaped
from `docs/research/2026-09-12-m2-dependencies.md` section 3, which quotes Doppler's published
OpenAPI schemas verbatim, and since 2026-09-14 checked against real responses.

## How they are verified

`tests/live_write_cycle.rs` (opt-in: the crate's `live-tests` feature, `#[ignore]`,
`WILLIKINS_LIVE_TESTS=1`) provisions a real Doppler project, drives all five tools against it,
records every response it can see under `live/` (redacted, gitignored) and compares each with
the authored fixture of the same name by top-level key set, then deletes the project again.
A `Drop` guard deletes every project the run created on every exit path.

The two project names it uses are fixed:

- `willikins-live-write-cycle`
- `willikins-live-write-cycle-foreign`

Anything by those names in the test workplace is that test's. The cycle **refuses to start** if
either already exists, because a leftover from an aborted run is the operator's to **delete by
hand** — the test never deletes something it did not create in that run. A second `#[ignore]`
test in the same file, gated on `WILLIKINS_LIVE_LEFTOVER_CHECK=1`, confirms both are gone.

`tests/live_probe.rs` is the read-only half. It ran for the first time on 2026-09-14 and
verified nothing: the dedicated test workplace holds no persistent project, so every check that
needs one answered `404`. It stays for a workplace that keeps one.

The comparison is a subset check — every key the authored fixture names must be present in the
live response — not equality. Live Doppler returns more fields than any fixture here models;
those are listed under "What the live responses carry that these fixtures do not" below, and
are deliberately left out because no tool reads them.

## Status

| File | Endpoint | Verified |
| --- | --- | --- |
| `project_get_present.json` | `GET /v3/projects/project` (200, ours) | live 2026-09-14, write cycle |
| `project_get_foreign.json` | `GET /v3/projects/project` (200, not ours) | live 2026-09-14, write cycle |
| `project_post_created.json` | `POST /v3/projects` | live 2026-09-14, write cycle (recorded from the foreign project's own create) |
| `config_get_present.json` | `GET /v3/configs/config` (200, `root: true`) | live 2026-09-14, write cycle |
| `environment_post_created.json` | `POST /v3/environments` | live 2026-09-14, write cycle, through a `GET /v3/environments/environment` of what the create just made: Doppler answers the same `{"environment": {...}}` envelope for both, and `create_environment` discards its own response body |
| `service_tokens_list_present.json` | `GET /v3/configs/config/tokens` (200, one token) | live 2026-09-14, write cycle |
| `service_tokens_list_absent.json` | `GET /v3/configs/config/tokens` (200, empty) | live 2026-09-14, write cycle |
| `secret_get.json` | `GET /v3/configs/config/secret` (200, a value) | live 2026-09-14, write cycle |
| `secret_get_absent.json` | `GET /v3/configs/config/secret` (200, **no such secret**) | behaviour verified live 2026-09-14: with `computed` optional the tool answers `NotFound` naming the key against the real endpoint. The body's own shape is inferred from the live parse position — see below |
| `service_token_post_created.json` | `POST /v3/configs/config/tokens` | **not recorded on purpose**: the response carries a live token. The proof is that the typed parse into `DopplerServiceToken` succeeds against the real endpoint, which it did |
| `service_token_delete.json` | `DELETE /v3/configs/config/tokens/token` | **not recordable**: `Http::delete_with_body` collapses every 2xx into `Ok(())` and keeps no body. The rotation succeeded live, so the endpoint and its request body are confirmed; only the response shape is not |
| `error_404.json` | any `GET` (404) | **partly confirmed** live 2026-09-14: a real 404 carried a `messages` array, which the shared client labelled `provider says:`. The body itself is never recordable — `ProviderError` drops it at construction (trust boundary 5) — so the `success: false` half stays unconfirmed |
| `error_5xx.json` | any request (5xx) | same caveat as `error_404.json` |
| `service_tokens_list_project_missing.json` | `GET /v3/configs/config/tokens` (404, project does not exist yet) | **confirmed live 2026-09-16** by the first live smoke run (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s task 14 record): planning the positive fixture against a fresh Doppler account failed at node `token` with exactly this message, because at plan time `doppler.project.ensure` had not yet run. Same `success: false` caveat as `error_404.json`; the message names `willikins-smoke` (the smoke run's project) while the mock tests below reuse `third-thoughts` as the project in their `LIST_PATH` — harmless, since the tool never reads the body's text, only the status code. Used by `service_token_ensure_mock.rs` and `service_token_rotate_mock.rs` to pin that a token `read` now tolerates a missing parent project or config instead of failing the whole plan |

## What the live run changed

- **`doppler.secret.get` on a secret that does not exist.** The milestone plan's port table says
  `404 -> ToolError::NotFound`. Doppler does not answer `404`: it answers **`200` with
  `value.computed` set to `null`**. Before the fix the tool reported
  `ToolErrorKind::Provider` with "could not parse the response body", which names no cause an
  operator can act on, and the plan's `NotFound` row could never fire live.
  `SecretValueBody::computed` is now `Option<DopplerSecretValue>` and `None` maps to `NotFound`
  naming the key, which is also what `willikins-providers-fake`'s tool of the same name has
  always answered for an unseeded secret — so the live tool and the fake now agree.
  `secret_get_absent.json` is that body. Its exact bytes were not read (the client drops them);
  its shape is the one the live parse position points at (`line 1, column 70`, the end of a
  `null` following `"computed":`). The live cycle's step 8 now passes end to end.
- **Every `project` field in every response is the project's *name*.** The research note
  (section 3) says the environment and config objects' `project` field "holds the project's
  opaque id, not its name (e.g. `ed0c2a68b6`)" and that the project object has no `slug`.
  Neither holds: live, `project.id`, `project.name` and `project.slug` are all the project
  name, and every other object's `project` field is that name too. The authored fixtures
  carried Doppler's documentation example `ed0c2a68b6` in seven files; all seven now carry a name.
- **The service-token list carries a partial token.** The research note says the list omits
  `key`, which is true, and also says it omits `access`, which is not: live entries carry
  `access`, `last_seen_at`, and **`token_preview`** (`"dp.st…"` plus six real characters of the
  token). Nothing in the authored material named that field, so nothing redacted it.
  `token_preview` is now in `tests/common/mod.rs`'s `REDACTED_FIELD_NAMES`, beside `key`.

- **`doppler.service_token.ensure` and `.rotate` failed to plan against a project that does not
  exist yet.** Found 2026-09-16 by the first live smoke run (four seconds, nothing created):
  planning the positive fixture against a fresh Doppler account failed at node `token` with
  `{"kind":"NotFound","message":"provider says: Could not find requested project
  'willikins-smoke'"}`, because Doppler's token-list endpoint 404s when its `project` or
  `config` query parameter names something that does not exist, and both tools' `read`
  propagated that 404 as a hard `ToolError` instead of reading it as "no token is listed" — the
  same 404 `doppler.config.ensure` already maps to `Observation::Absent` two tools over. At plan
  time, before `doppler.project.ensure` and `doppler.config.ensure` have run, the parent
  genuinely does not exist yet; that is not a reason to fail, it is the reason a plan exists.
  Fixed by treating a 404 from the token list the same way as an empty list: in
  `doppler.service_token.ensure`, `is_listed` (shared by `read` and `ensure`, so `ensure` also
  tolerates it before falling through to the mint); in `doppler.service_token.rotate`, only
  `read` (its `ensure` still fails outright on a listing 404, since a rotate that cannot list
  the tokens it is about to revoke must not mint a replacement either). Either way, a project or
  config still missing by *apply* time (ordering should already have created it) still fails for
  real, at the mint call that actually needs it to exist — nothing is silently swallowed.
  `service_tokens_list_project_missing.json` records the exact body the live run saw.
  No live tool's *live-write-cycle* test caught this, because it creates the project before
  ever listing tokens; the mock-server tests added alongside this fix
  (`service_token_ensure_mock.rs`, `service_token_rotate_mock.rs`) are what pin the 404 case now.

  **What that 404 cannot settle**, recorded so it is not re-derived: Doppler documents no
  error-body schema for any non-2xx response, so the status is all there is. It covers "no such
  project or config" — the smoke run proved that much — but it cannot be shown *not* to cover
  "one this Service Account is not granted" (its permission set is granular within a workplace,
  and another workplace's projects lie outside it entirely). Which status an out-of-grant project
  actually answers was never observed here, by the write cycle, the probe or the smoke run: a
  `403` is as plausible as a `404`. GitHub's own 404 is documented to conflate the two, which is
  what `github.repo.ensure` records; for Doppler the claim stays the weaker one, that a 404
  cannot prove absence.
  `Observation::Foreign` would be the wrong reading of the ambiguity anyway — `plan` turns
  `Foreign` into `NameTaken` and refuses everything, which is the defect again for the ordinary
  case — and `Absent` reaches nothing a `200` did not already reach, since the ownership gate is
  `doppler.project.ensure`'s `managed-by: willikins` marker upstream, never this tool. A parent
  that really is unreachable fails one call later, at the mint `POST`. The tolerance is one
  status wide on purpose: a `400` — this API's *other* answer for a project that is not there,
  recorded under "Undocumented facts the cycle settled" below — still refuses the plan, because
  whether this endpoint ever answers it was never observed.

## What the live responses carry that these fixtures do not

Recorded 2026-09-14; left out because no tool reads them, and a fixture that models every field
Doppler sends is a fixture that breaks whenever Doppler adds one.

- A top-level **`"success": true`** on every 200.
- `project`: `slug`.
- `config`: `slug` (a UUID, unlike `name`), `inheritable`, `inheritedBy`, `inheriting`,
  `inherits`. Doppler sets `locked: true` on an auto-created root config; the fixture's `false`
  is Doppler's own documented example and is left as the other case.
- `environment`: `slug` (equal to `id`) and `personal_configs`. Doppler's own default
  environments carry a display `name` ("Development") beside the slug `dev`;
  `doppler.config.ensure` sets `name` and `slug` to the same derived string, which Doppler
  accepts.
- A listed service token: `access`, `last_seen_at`, `token_preview` (redacted on the way to
  disk).
- A secret's `value`: `computedValueType`, `computedVisibility`, `rawValueType`,
  `rawVisibility` beside `raw`, `computed`, `note`.
- The environment listing: `page`.

## Facts the live run settled

- **Review resolution 15, live.** Doppler creates `dev`, `stg` and `prd` with the project, so
  `doppler.config.ensure` reads `Present` for each and `ensure`s `changed: false`. The
  mock-free proof that no `POST` happened is the project's environment listing being
  byte-identical before and after all six calls. It was.
- **A project that is not ours.** A project whose description is not `managed-by: willikins`
  reads `Foreign`, `ensure`s as `Conflict`, and is byte-identical afterwards.
- **A service token's value is issued once.** `read` after a mint is `Present` with the value
  `Unknown`; a second `ensure` is `changed: false` and mints nothing. A rotation leaves exactly
  one token of that name, under a new slug and with different bytes.
- **`DOPPLER_PROJECT`** is auto-injected into every config and holds the project's own name.

## Undocumented facts the cycle settled

Doppler's published OpenAPI pages define a `200` for every endpoint and nothing else, so none
of the following is written down anywhere. All were observed live on 2026-09-14 and are the
open items of the milestone plan's "Verify before relying on them" list, item 4, plus its
"Notes for milestone 3" item 1.

| Question | Answer |
| --- | --- |
| What does a 404 error body look like? | It carries a `messages` array, which the shared client renders as `provider says: ...`. Nothing more is observable: the body is dropped at construction |
| What does `POST /v3/projects` answer for a name that already exists? | **`400`**, with a provider message. Not a `409`, and not a second project: the duplicate create makes nothing |
| What character class does an environment slug accept? | The underscore is accepted. `doppler.config.ensure` created an environment whose `name` and `slug` are both `pre_prod`, the snake join `naming::v1::doppler_root_config` derives for the environment `pre-prod`. **`naming::v1` needs no `v2` row for multi-word environments.** The project ended the run holding `dev`, `stg`, `prd`, `qa`, `pre_prod` |
| Does `POST /v3/configs` prefix a branch config's name with `<environment>_` itself? | **No — the caller must supply the prefixed name.** `name: "probe"` under environment `dev` answered `400`; `name: "dev_probe"` under `dev` was stored as `dev_probe` with `root: false`. A `naming::v2` branch-config row must emit `<environment>_<name>` itself, and must budget the 60-character config-slug cap with the prefix included |
| What does a `GET` of a project answer right after that project's own `DELETE`? | Either `400` or `404`, and which one varies inside a single run: the cycle's two deletes answered `400` and `404` respectively. A `GET` a minute later is `404`. A `DELETE` of a project that is not there answers `400`. The cycle's step 10 therefore asserts "not readable" rather than a literal status, and the guard treats both as gone |

Not answered, and deliberately not asked: whether two service tokens may share a name in one
config. Minting a second token to find out means a second live secret in a raw response body,
which is not worth the answer.

`live/` holds the recordings and is gitignored: they carry the sandbox workplace's own
identity.
