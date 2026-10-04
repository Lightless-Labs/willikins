# Milestone 3j: the APNs key is observed, not acknowledged

**Created:** 2026-10-04 (from the operator's request of 2026-10-03)

## Goal

1. **Part A.** `DopplerProject` admits underscores, so a project Doppler itself names with one (a shared base
   project such as `shared_keys`) can be written in a document, sent in a request and parsed back from a response.
   `naming::v1` is unchanged: only the parse grammar widens.
2. **Part B.** A new gate, `doppler.secret_name.gate`, observes that a named secret is visible in a Doppler config,
   either set there or inherited. It reads secret **names** only, never a value.
3. **Part C.** The operator's iOS app document (gitignored, under `private/workflows/`) stops asking the operator to
   acknowledge the APNs key. The shared base config that holds the team's key, `shared_keys/prd` in this plan, joins
   `base_configs`. The `m5_apns_key` acknowledgement and its `m5_apns_key_done` input are replaced by the new gate,
   which checks the key's name on the app's deployment branch config after inheritance is set.

The real names (the base project, its config, the secret) appear only in the private document, its fake state and
its gitignored tests. Every tracked file, this plan included, uses the placeholders below.

## The operator's words (2026-10-03)

On "It's an acknowledgement, so willikins takes your word for it": "It shouldn't. It should check." Then: the APNs
key is stored in a dedicated Doppler project under two inheritable configs, a sandbox one and `prd`, under one
secret name. And: "now it's a team-wide APNS key, stored in a inheritable config, well that should be much easier".

## Out of scope

- Wiring the sandbox (development) APNs config. Nothing in the app's document reads a development config: the only
  config that inherits base configs is the deployment branch config under `prd`.
- Reading, validating or moving the key's value. The gate proves a name is visible. The CI signing preflight is
  where the key's content is used.
- Creating the key. No Apple API creates or downloads an APNs auth key.
- Changing `naming::v1`. It derives kebab-case project slugs and stays frozen.
- Nested inheritance beyond one level (decision (b4); verify item 2).

## Trust boundaries (normative)

These extend milestone 3i's boundaries, which still hold.

1. **Names, never values.** The gate calls only `GET /v3/configs/config/secrets/names` and `GET
   /v3/configs/config`. It never calls `/v3/configs/config/secret`, `/v3/configs/config/secrets` or
   `/v3/configs/config/secrets/download`. It never asks Doppler to issue a dynamic-secret lease.
2. **The list never leaves the client.** The client compares the listed names to the one asked for and returns a
   `bool`. The response struct has no `Debug`. No listed name other than the one asked for reaches an output, a
   `ToolError`, the journal, `tracing` or a panic message. The other names in a config are part of the operator's
   layout, and the gate has no reason to print them.
3. **A gate never writes.** `ensure` re-observes and returns `changed: false`, like every gate.
4. **Live Doppler writes only in the sandbox workplace**, only on a throwaway project pair created and deleted by the
   same guarded test. Nothing is written to the operator's real workplace by an agent.
5. **No provider-token-shaped literal** anywhere (CLAUDE.md). The live cycle's marker value is plain text.

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| `DopplerProject` pattern (A1) | `[a-z0-9]+(?:[-_][a-z0-9]+)*`, max 64 (unchanged), example `third-thoughts` (unchanged) |
| `DOPPLER_CONFIG_PATTERN` (A1) | `^[a-z0-9]+(?:[-_][a-z0-9]+)*/[a-z0-9_-]+$` |
| Placeholder base config | `shared_keys/prd` |
| Placeholder secret name | `EXAMPLE_APNS_KEY` |
| New tool (B2) | `doppler.secret_name.gate` |
| Its inputs, in order | `config: DopplerConfig` (required), `name: SecretName` (required) |
| Its outputs | `config: DopplerConfig` (pass-through) |
| Key / class / pure | `[]` / `Class::Reversible` / `true`; declares a `Gate` |
| Description string | `A gate: whether a secret name is visible in a Doppler config, set there or inherited.` |
| Gate `need` | `this secret name is visible in this Doppler config, set there or inherited from a config it inherits` |
| Gate `how` | `store the secret in this config, or in an inheritable config this one inherits (doppler.config.inherits.ensure); if this run also changed what this config inherits, re-run: a gate observes before the run applies; a config that does not exist yet, or one this token cannot see (Doppler answers both the same way), also blocks here until it is created or this token is granted read access` |
| Gate `subject` | `["config", "name"]` |
| Client call (B1) | `DopplerClient::secret_name_listed(&self, project: &DopplerProject, config: &DopplerConfigName, name: &SecretName) -> Result<bool, ProviderError>`, `pub(crate)` |
| Names request | `GET /v3/configs/config/secrets/names?project={project}&config={config}&include_dynamic_secrets=false&include_managed_secrets=false` |
| Names response struct | `struct SecretNamesBody { names: Vec<String> }`, `Deserialize` only, no `Debug` |
| `LIVE_TOOL_NAMES` (B4) | 39 → 40, `doppler.secret_name.gate` inserted right after `doppler.config.inheritable.gate`; the `[&str; 39]` copy in `crates/willikins-providers-doppler/tests/live_catalog.rs` becomes `[&str; 40]` |
| Fake catalog (B3) | 41 → 42 tools |
| Pass-through (C1, gated) | `doppler.config.inherits.ensure` gains output `config: DopplerConfig`, known in `Present`, in `Absent`'s `predicted` and in every `Ensured` |
| Tracked positive document (C2) | `workflows/doppler-inherited-secret-gate.yaml`, fake state `workflows/fixtures/state/inherited-secret-name.json` |
| Tracked negative fixtures (C2) | `workflows/fixtures/secret-name-gate-secret-into-name.yaml`, `workflows/fixtures/secret-name-gate-lowercase-name.yaml`, `workflows/fixtures/doppler-project-doubled-underscore.yaml` |
| Live cycle (B5) | `crates/willikins-providers-doppler/tests/live_secret_name_gate_cycle.rs`, own `[[test]]` with `required-features = ["live-tests"]`, `#[ignore]`, `WILLIKINS_LIVE_TESTS=1`; base project `willikins_names_probe_<unix-seconds>`, child project `willikins-names-probe-<unix-seconds>`, secret `WILLIKINS_NAMES_PROBE`, value `willikins names probe, not a secret` |
| Private node (C3) | `m5_apns_key`: `tool: doppler.secret_name.gate`, `config: ${{ steps.inherit.config }}`, `name:` the real secret name |
| Characterization snapshot | A1, B1–B5: byte-identical. C1: three `plan_json` lines change (decision (c2)), gated. C2: four new entries. Nothing else |

## Sources, verbatim

`https://docs.doppler.com/reference/secrets-names.md`, fetched 2026-10-04 with `curl`:

> `GET /v3/configs/config/secrets/names` — "List Names", "Secret Names". Query: `project` (required, "Unique
> identifier for the project object."), `config` (required, "Name of the config object."),
> `include_dynamic_secrets` (boolean, default `false`, "Whether or not to issue leases and include dynamic secret
> values for the config"), `include_managed_secrets` (boolean, default `true`, "Whether to include Doppler's
> auto-generated (managed) secrets"). `200`: `{"names": ["STRIPE", "ALGOLIA", "DATABASE", "USER"]}`, schema
> `names: array of string`.

No non-2xx response is documented. Nothing on the page says whether inherited names are listed.

`https://docs.doppler.com/docs/config-inheritance.md`, fetched 2026-10-04:

> Once this inheritance is setup, whenever secrets in the child config are fetched, they will include the secrets
> from the parent config it is inheriting.

> Dynamic Secrets are not yet supported by Config Inheritance. This means that you cannot inherit from or enable
> inheritance for a config that contains any Dynamic Secrets.

The page does not say whether an inheritable config may itself inherit.

Facts from earlier milestones that this plan relies on:
- Doppler keeps underscores in project slugs. A sandbox project named `willikins_probe_delete_me` got exactly that
  slug, with configs `dev`, `stg` and `prd` (2026-10-03 probe).
- A reader of a config sees secrets inherited from a base config without any access to the base project (milestone
  3h, through the download and single-secret endpoints).
- Doppler answers `404`, or `400` "does not have access to requested project", both for a project that does not
  exist and for one outside the token's grant (`looks_like_a_missing_project`'s doc;
  `docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`).
- Setting a config's inheritance and the inheritable gate both read the base config, so willikins needs read access
  to `shared_keys/prd` before the app's next real run (operator, pending).

## Decisions, part A: underscores in a project slug

### (a1) The grammar: structured, one more separator

`[a-z0-9]+(?:[-_][a-z0-9]+)*`. An underscore is admitted wherever a hyphen already is: between two runs of letters
and digits.

Why structured and not `DopplerConfigName`'s flat `[a-z0-9_-]+`:
- It is the smallest widening that admits every project slug the evidence shows (`shared_keys`,
  `willikins_probe_delete_me`, every kebab slug `naming::v1` derives).
- It keeps every existing refusal: `doppler_project_rejects_leading_hyphen` stays green unchanged, and a leading,
  trailing or doubled separator stays refused. The flat form would admit `-x`, `_`, `a--b`, none of which any probe
  showed and none of which a document needs.
- `DopplerConfigName` was flat from its first commit. Its form is its own history, not a rule to copy.

**Accepted:** `shared_keys`, `willikins_probe_delete_me`, `third-thoughts`, `a_b-c`, `x`, `app2_shared`, 64 × `a`.
**Refused:** `_shared`, `shared_`, `shared__keys`, `shared_-keys`, `-third`, `Shared_Keys`, `shared.keys`,
`shared/keys`, `shared_keys&project=other`, `shared_keys?x`, `shared_keys#x`, `shared%5Fkeys`, `shared keys`,
`shared+keys`, `shared_keys\n`, `..`, the empty string, 65 × `a`.

### (a2) Every place a `DopplerProject` meets a path, a URL, a response or `naming::v1`

| Place | What happens to an underscore | Change |
| --- | --- | --- |
| `client.rs` request lines | Every request puts the project in the query string (`?project={project}`), including `update_project_member`, whose only path segment is the service-account slug. `_` is an RFC 3986 unreserved character and needs no encoding | none. A mock pins `?project=shared_keys&config=prd` as sent (acceptance 2) |
| `client.rs` request bodies | JSON strings (`CreateProjectBody`, `SetInheritsBody`'s `ConfigRefRequest`, member bodies) | none |
| `client.rs` `ConfigRefBody.project: DopplerProject` | **Breaks today.** Once a config inherits `shared_keys/prd`, every `GET /v3/configs/config` on that config fails to deserialize: `doppler.config.inherits.ensure`'s read and re-read, and the new gate's walk. The widening fixes it | a fixture `config_get_inherits_underscore.json` and mock tests (acceptance 2) |
| `DopplerConfig::parse` | Delegates to `DopplerProject::parse`, so it widens with it | none |
| `DOPPLER_CONFIG_PATTERN` | Hand-written schema pattern, not derived | updated (SHARED VALUES), with a test that the regex and `parse` agree on every (a1) row prefixed onto `/prd` (acceptance 1) |
| `willikins-types` catalog snapshot | `DopplerProject`'s and `DopplerConfig`'s `pattern` lines | exactly two lines change |
| `naming::v1::doppler_project` | Derives a kebab slug from a `ProjectSlug`, always inside the widened grammar | none; `naming_v1_properties.rs` keeps pinning it |
| Fake state keys | `project`, `project/config`, `project/config#NAME`: joins on `/` and `#`, never `_` | none; a seed with `shared_keys/prd` keys must load (acceptance 2) |
| `BuildkiteOrg`, `DopplerTokenName` | Share the old pattern text | untouched; their own refusal tests stay green |

## Decisions, part B: the gate

### (b1) Name, ports, key, class

`doppler.secret_name.gate` (SHARED VALUES). The name says what is read: a secret's *name*, not the secret. Both
inputs are domain types already in the registry. The output passes `config` through, like
`doppler.config.inheritable.gate`, so a document can order a consumer after it. No key: a gate is pure, and a pure
tool has none. `Reversible`, like every gate. The subject is both ports, so a blocked report names exactly which
config lacks which name. Neither is secret or identifier-typed, so 3i's decision (b8) guard stays green.

### (b2) What it observes

One shared `observe(config, name)`:

1. `secret_name_listed(config.project, config.name, name)`.
   - `Ok(true)` → `Present`.
   - `Ok(false)` → step 2.
   - an error that `looks_like_a_missing_project` → `Absent` (decision (b3)). No walk.
   - any other error → `Err`.
2. `get_config(config.project, config.name)`, then for each entry of its `inherits`, in order,
   `secret_name_listed(entry.project, entry.config, name)`.
   - the first `Ok(true)` → `Present`. The remaining bases are not read.
   - `Ok(false)`, or an error that `looks_like_a_missing_project`, → that base contributes nothing; continue.
   - any other error → `Err`.
   - every base read and none listed (or `inherits` empty or absent) → `Absent`.
   - `get_config` itself answering the missing-project shape → `Absent`.

`Present` and `Absent` both carry `config` as the output. `ensure` calls `observe` and returns `changed: false`
either way.

A `2xx` whose body does not parse is a `Provider` error with a static message naming `config` and `name` (both
non-secret domain types), never the response text, like `doppler.secret.get`'s malformed-body arm. Names are compared
byte for byte with `SecretName::as_str`. Listed names are never parsed as `SecretName`, so a name Doppler allows and
willikins' grammar refuses does not fail the read.

**Why `include_managed_secrets=false`:** managed names are Doppler's own auto-injected `DOPPLER_*` variables, which
an operator never stores. A gate on one would prove nothing, and leaving them out keeps every config's answer about
what the operator put there. A gate on a `DOPPLER_*` name therefore always reads `Absent`. That is stated in the
tool's doc. **Why `include_dynamic_secrets=false` explicitly:** `true` issues leases. The default is already
`false`, but the mock's `match_query` pins it, so a refactor cannot drop it silently.

### (b3) A config it cannot read: `Absent` for the ambiguous pair, `Err` for the rest, never `Foreign`

The brief asked for "Foreign or an error for a config it cannot read". This plan deviates for one pair of answers,
and here is why. On a fresh run, the app's document creates the project and the deployment branch config in the same
plan, so the gate reads a config that does not exist yet. Doppler answers that with the same `404`, or `400` "does
not have access to requested project", that it gives for a project outside the token's grant. The two cannot be told
apart. A plan-time error there would make every fresh document unplannable (the same reasoning as milestone 3i
decision (a2)'s deviation, and the precedent `doppler.config.inheritable.gate`'s
`read_reports_absent_when_this_token_cannot_see_the_project` already pins). So the pair reads `Absent`, the run
reports a blocked gate, and the gate's `how` names both readings.

Everything Doppler *can* distinguish fails loudly: `401`, `403`, any other `4xx`, `5xx`, transport failure, a
malformed `2xx`. `Foreign` is never returned. `plan_one` turns `Foreign` into `PlanError::NameTaken` ("name taken by
a resource this workflow did not create"), which is meaningless for a read-only observation of someone else's
config.

### (b4) If the names endpoint does not list inherited names

Nothing in Doppler's reference says either way (verify item 1). The walk in step 2 makes the gate correct under both
answers, so no task waits on the answer:
- **If the endpoint lists inherited names,** step 1 already answers `Present` for an inherited key. The walk only runs
  when the key is visible nowhere. It reads each base's names and finds nothing, because a name in a base the config
  actually inherits would have been listed in step 1. So the walk cannot produce a `Present` that step 1 missed. Its
  only cost is one extra read per base when the key is genuinely missing.
- **If the endpoint omits inherited names,** step 1 answers `false` for an inherited key, and the walk finds it in the
  base the config actually inherits (read from Doppler's own `inherits` array, not from the document). That is what
  makes the gate `Present`.

The walk reads the config's real inheritance, never the document's intent. A name in a base the document *asks*
`inherit` to add, before `inherit` has applied, is not visible yet and reads `Absent`. That is true, and it is why
the gate's `how` says to re-run.

**One level only.** An inheritable config that itself inherits would hide a name two levels up. Whether Doppler
permits that is verify item 2. If it does, the gate reads `Absent` for such a key: a false block, never a false
`Present`. The fix then would be walking further, in its own reviewed change.

**Access cost.** The walk needs read access to the base configs, which a reader of the config does not otherwise
need (milestone 3h's probe). A document that inherits a base already needs that access to set the inheritance and to
pass `doppler.config.inheritable.gate`, so the walk adds no grant. A base the token cannot see contributes nothing.
The gate then blocks with a `how` that names access, and does not fail.

**How the live cycle settles it.** Step 8 of the live cycle (below) reads the names endpoint on the child config
through a test-local struct and prints one line: whether the inherited probe name was listed. It asserts nothing
about that boolean. Step 9 asserts the gate reads `Present` either way. The coordinator records the boolean as an
addendum here and in `docs/solutions/providers/`. If it is `true`, the walk is a backstop. If it is `false`, the walk
is load-bearing, and acceptance 5's walk rows are the only offline proof of that path.

### (b5) The fake twin

`willikins_providers_fake::tools::DopplerSecretNameGate`, identical `ToolSpec`. A name is visible in a config when
its key `config#NAME` (`doppler_secret_key`) is a member of `doppler_secrets`, `doppler_values` or
`doppler_secret_writes`. Membership is the only fact read, never a value. If none holds it, the fake walks
`doppler_config_inherits[config]` one level with the same membership test. A config absent from `doppler_configs`
reads `Absent` with no walk. There is no new seed field: a seed that wants a name visible seeds `doppler_secrets` as
existing seeds already do. `SecretsMap` gains `contains_key(&str) -> bool` if it lacks one. The fake records its read
calls (`record_read_call`) like every fake tool.

### (b6) Registration

The live catalog inserts the tool right after `doppler.config.inheritable.gate`, and `LIVE_TOOL_NAMES` grows to 40.
As a `doppler.*` tool, it makes `live_catalog_for_document` demand `WILLIKINS_DOPPLER_TOKEN` exactly as its siblings
do; a test pins that. The fake catalog registers it (42 tools). The crate's `Cargo.toml` `description`, `lib.rs` and
`tools/mod.rs` docs, the README's Doppler tool list, and `tests/catalog_parity.rs`'s module doc name it.

## Decisions, part C: the app's document

### (c1) `shared_keys/prd` joins `base_configs`

It is appended last to `base_configs`' default. Doppler gives earlier entries precedence on a name conflict, so
appending leaves the existing three bases' order and precedence alone. `base_config_gate` gains a fourth instance,
which checks that the config exists and is inheritable. `inherit`'s next real run sees a missing entry with nothing
extra, so it plans `Create` and sends the four-entry set.

### (c2) The data edge: `inherit` passes `config` through (needs the coordinator)

The DSL orders nodes only by bindings. `doppler.config.inherits.ensure` declares no output today, so nothing can bind
"after inheritance". The honest edge is a pass-through: `inherits.ensure` gains output `config: DopplerConfig`, and
the gate binds `config: ${{ steps.inherit.config }}`.

The pass-through must be **known in `Absent`'s `predicted`**, not only in `Present`. Today `predicted` is
`Outputs::new()`, and `fill_outputs` would make the port `Unknown`. The gate's `get(inputs, "config")` would then fail
at plan time with `Invalid` on every run where `inherit` plans `Create`, which includes every fresh run.

What the edge buys, precisely: `apply.rs` reuses a pure node's planned outputs and never re-reads it, so a gate
observes **at plan time only**, before anything applies. Ordering therefore does not make the gate see the new
inheritance in the same run. It does decide what happens when `inherit` cannot run. If any `base_config_gate` instance
is blocked (today: willikins cannot yet read `shared_keys/prd`), `inherit` plans `Skip` (it binds the aggregate), and
the gate plans `Skip` too, because it binds `inherit`. The run reports one actionable item, the base config, rather
than a second, misleading "the key is not visible". Without the edge, it reports both.

**The characterization cost**, predicted exactly: three entries, `workflows/doppler-backend-for-ios.yaml`,
`workflows/doppler-backend.yaml` and `workflows/doppler-ios.yaml`, each change in their one `plan_json:` line, where
each `inherit` instance's `"outputs":{}` becomes
`"outputs":{"config":{"type":"DopplerConfig","list":false,"state":"known","value":"third-thoughts/<env>"}}` (nine
instances in all). No `TYPES:`, `OUTPUTS:` or error line changes. The boundary for this milestone forbids that
change, so task C1 waits for the coordinator's sign-off (Needs the coordinator, item 1). Two spec snapshots also
change: `catalog_parity__doppler_config_inherits_ensure_spec.snap` and the fake catalog snapshot, each by the one
output.

**If the coordinator declines,** the gate binds `config: ${{ steps.prd_config.config }}` (the deployment branch
config node's own pass-through). It is then a sibling of `inherit`, not after it. Everything still works, but a
blocked base config produces two blocked gates instead of one. C2's and C3's skip-propagation tests are then dropped,
and HANDOFF says so.

### (c3) The two-run shape, stated so nobody mistakes it for a bug

On the app's next real run, once willikins can read `shared_keys/prd`: `inherit` plans `Create` (adding the fourth
base), and the gate reads the deployment branch config before that applies, so it plans `Blocked`. The run reports it,
with the gate's `how` saying to re-run. The re-run reads the key through the new inheritance: the gate is `Compute`,
and every node is `NoOp` or `Compute`. This is the same idempotent "re-running is the resume" the document already
relies on. The private tests pin both runs.

Expected real `plan --live` shapes, hedged (the CLI's own counts are the arbiter; today's baseline is 19 NoOp,
60 Compute, 1 Blocked):
- **Before the operator grants read access:** the new `base_config_gate` instance is Blocked; `inherit` and the gate
  are Skip. That is one fewer NoOp (`inherit`), the same Compute, 1 Blocked, 2 Skip.
- **Granted, first run:** `inherit` Create, the new base-gate instance Compute (61), the gate Blocked.
- **Granted, second run:** 19 NoOp, 62 Compute, 0 Blocked.

### (c4) The private document's edits

All in gitignored paths. No commit, by design.
- `m5_apns_key` keeps its node name (reports and HANDOFF already say "m5"), now `tool: doppler.secret_name.gate`,
  `config: ${{ steps.inherit.config }}`, `name:` the real secret name.
- The `m5_apns_key_done` input is removed.
- `base_configs`' default gains the real base config, last.
- The header's M5 bullet, and the 3h paragraph that kept M5 as an acknowledgement by the operator's earlier
  instruction, gain a dated **3j (2026-10-03)** paragraph. It says that M5 is now observed, quotes "It shouldn't. It
  should check.", and explains why the sandbox config is not wired.
- The fake state seeds the base config as existing and inheritable, and its secret under `doppler_secrets`.
- The gitignored tests: no `m5_apns_key_done` anywhere. Blocked sets lose the acknowledgement and gain the gate where
  (c3) says. The "third run with the acknowledgement" becomes a plain re-run. `BASE_CONFIGS` has four entries. Two new
  tests: the two-run shape, and base-not-seeded → base gate instance Blocked, `inherit` Skip, `m5_apns_key` Skip.

### (c5) The tracked proof

Part C's own edits are never committed, so the tracked proof is a small positive document,
`workflows/doppler-inherited-secret-gate.yaml`, with the same shape: `naming.v1` → project → `prd` root config → a
branch config → `base_config_gate` over a defaulted `base_configs: [shared_keys/prd]` → `inherit` → `apns_key`
(`doppler.secret_name.gate`, `config: ${{ steps.inherit.config }}`, `name: EXAMPLE_APNS_KEY`). Its fake state,
`workflows/fixtures/state/inherited-secret-name.json`, seeds `shared_keys/prd` as existing and inheritable, holding
`EXAMPLE_APNS_KEY`. Its literal `shared_keys/prd` also exercises (a1) through `check`. Three negative fixtures
(SHARED VALUES), each with the header comment naming its acceptance test and exact error. Four new characterization
entries, nothing else.

## Acceptance tests

Part A:
1. **Grammar** (A1). Every (a1) accepted row parses as `DopplerProject` and round-trips through serde. Every refused
   row fails. For every row `r`, `DopplerConfig::parse("{r}/prd")` succeeds iff `r` was accepted, and the
   `DOPPLER_CONFIG_PATTERN` regex matches iff `parse` succeeds. `doppler_project_rejects_leading_hyphen` and
   `doppler_config_refuses_a_smuggled_query_parameter_in_its_name` are unchanged and green.
   `naming_v1_properties.rs` is unchanged and green. The types catalog snapshot differs in exactly two `pattern` lines.

   **Addendum:** 2026-10-04 (task A1) — this acceptance's "the regex matches iff `parse` succeeds, for every row"
   holds for every (a1) row except `65 × a`. `DOPPLER_CONFIG_PATTERN` carries no length bound (that lives in the
   schema's separate `maxLength`), so `65 × a/prd` matches the regex while `parse` still refuses it through
   `DopplerProject`'s `max_len = 64`. The implementing test pins the agreement over every row but `65 × a`, and pins
   this one divergence explicitly and separately
   (`doppler_config_regex_and_parse_diverge_on_the_max_len_boundary`), rather than silently dropping it from the
   loop.
2. **Where it meets a request or a response** (A1). A `GET /v3/configs/config?project=shared_keys&config=prd` mock
   (`match_query` pinning the raw query) answers `doppler.config.inheritable.gate`'s read. The new fixture
   `config_get_inherits_underscore.json` (a config inheriting `shared_keys/prd`) makes `doppler.config.inherits.ensure`
   read `Present` for `[shared_keys/prd]` and `Absent` for `[shared_keys/prd, other/prd]`. A fake-state JSON with
   `shared_keys/prd` keys deserializes and the fake inheritable gate reads it `Present`.

Part B:
3. **Client** (B1, unit tests in `client.rs` with `MockProvider`). The request matches SHARED VALUES exactly, with
   `match_query` and `.expect(1)`. `{"names": ["A", "EXAMPLE_APNS_KEY"]}` → `true`. `{"names": []}` → `false`.
   `{"names": ["EXAMPLE_APNS_KEY_OLD", "XEXAMPLE_APNS_KEY", "example_apns_key"]}` → `false` (no prefix, suffix or case
   match). `{"names": null}` and `{}` → `Err` with a 2xx status. `404`, `400` no-access, `401`, `403` and `500` →
   `Err` carrying the status. A marker name `WILLIKINS_LEAK_MARKER_NAME` in a listed body never appears in any `Err`
   message or `Debug` output.

   **Addendum:** 2026-10-04 (task B1) — every mock asserts `.expect(1)` except the `500` one: `Http::get` retries a
   `5xx` up to three more times with real backoff, so that mock may be hit up to four times, and the test pins the
   query and the final status only, as this crate's other GET-vs-5xx tests (for example
   `branch_config_ensure_mock.rs`'s `read_maps_a_5xx_to_a_bounded_provider_error`) already choose to. Also: "a
   listed marker never appears in any `Err`" is structurally vacuous for a non-2xx response —
   `provider_error_from_body` builds a message from the body's `message`/`messages` field alone and never reads
   `names` — so the marker in those five bodies proves nothing by itself; it is kept anyway, as a cheap check that
   stays true. The path that actually exercises the trust boundary is a `2xx` body that lists the marker and fails
   to parse (a non-string entry), which
   `a_listed_marker_in_a_malformed_2xx_body_never_reaches_the_error` covers.
4. **Spec** (B2). Validates against the registry. Key empty, pure, `Reversible`, gate subject `["config", "name"]`,
   one output `config`. A `Catalog` accepts it.
5. **Observation rows** (B2, `tests/secret_name_gate_mock.rs`), one per (b2) branch, each mock with `.expect(n)` so an
   unexpected request fails:
   listed directly → `Present`, no config read;
   unlisted, `inherits` empty → `Absent`;
   unlisted, `inherits: [shared_keys/prd]`, base lists it → `Present`;
   unlisted, two bases, second lists it → `Present` after reading both;
   unlisted, first base lists it → `Present`, second base never read (`.expect(0)`);
   unlisted, base answers `404` → `Absent`;
   unlisted, base answers `403` → `Err`;
   config answers `404` → `Absent`, no walk;
   config answers `400` no-access → `Absent`, no walk;
   config answers `401` / `403` / `500` → `Err`;
   `get_config` answers `500` → `Err`;
   malformed names body → `Provider`, message names `config` and `name`, no body text.
   Every `Present`/`Absent` carries `config` as output. `ensure` on `Absent` and on `Present` → `changed: false`,
   with no request other than the mocked reads.
6. **Names, never values** (B2). A source guard in `tests/secret_name_gate_mock.rs` reads
   `src/tools/secret_name_gate.rs` and `client.rs`'s `secret_name_listed` (via `include_str!`) and asserts the tool
   never names `get_secret`, `get_value`, `/secret?`, `/secrets?`, `/secrets/download` or
   `include_dynamic_secrets=true`. `tests/redaction.rs` gains a case: a listed marker name and a marker in the config
   body reach no output, `ToolError`, `Debug` of anything returned, or captured `tracing`.
7. **Fake parity** (B3). `catalog_parity`'s new snapshot is identical for live and fake. `fake_agrees_with_live` gains
   one row per (b2) branch the fake can express (direct, inherited, absent, missing config, missing base). The fake
   catalog snapshot gains one tool. The fake count pin reads 42.
8. **Registration** (B4). `LIVE_TOOL_NAMES` has 40 names in order, and so does the `live_catalog.rs` copy. A document
   using only this tool demands `WILLIKINS_DOPPLER_TOKEN`.

Part C:
9. **Pass-through** (C1, gated). `inherits.ensure`'s `read` carries `config` in `Present` and in `Absent`'s
   `predicted`; `ensure` carries it after a write, after a converged read and after a failed-write re-read. Live and
   fake agree. The characterization diff is exactly the nine instances of decision (c2), in three lines.
10. **Tracked document** (C2, `crates/willikins-cli/tests/inherited_secret_gate_document.rs`, against the fake
    state). Run 1: `inherit` Create, `apns_key` Blocked, and the blocked report names `config` and `name`. Apply,
    then run 2: `inherit` NoOp, `apns_key` Compute, nothing Blocked. Base not seeded inheritable: `base_config_gate`
    Blocked, `inherit` Skip, `apns_key` Skip, and the only blocked gate is the base. Base seeded without the secret:
    after run 1's apply, run 2 still has `apns_key` Blocked. `apns_key.config` binds `inherit.config`.
11. **Negative fixtures** (C2). `secret-name-gate-secret-into-name.yaml` (a `doppler.secret.get` value bound into
    `name`) → exactly one type error at `apns_key.name`, `DopplerSecretValue` into `SecretName`.
    `secret-name-gate-lowercase-name.yaml` (`name: example_apns_key`) → exactly one `InvalidLiteral` for `SecretName`.
    `doppler-project-doubled-underscore.yaml` (`config: shared__keys/prd`) → exactly one `InvalidLiteral` for
    `DopplerConfig`. The characterization snapshot gains exactly the four C2 entries.
12. **The private document** (C3, gitignored tests, run but never committed). Decision (c4)'s list, including the
    two-run shape and the skip propagation. No `m5_apns_key_done` anywhere in the private document or its tests.
13. **Guards stay green.** `secret_literal_guard`, `no_gh_writes_guard`, `no_certificate_writes_guard`, and the 3i
    decision-(b8) subject guard.

## The live names cycle (written by B5, run once by the coordinator)

`crates/willikins-providers-doppler/tests/live_secret_name_gate_cycle.rs`, with credentials sourced only in the
command that runs it:

```text
source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
  cargo test -p willikins-providers-doppler --features live-tests --test live_secret_name_gate_cycle \
  -j 2 -- --ignored --nocapture
```

1. Both project names (SHARED VALUES, one `<unix-seconds>` for both) read `404` through a raw `GET`. Arm a guard that
   deletes the **child first, then the base** on every exit path, recorded before any assertion. The order matters
   because Doppler may refuse to delete a project whose config another config inherits.
2. `doppler.project.ensure` creates both. A test-local `GET /v3/projects/project` on the base reads its slug equal to
   the underscored name: (a1) live, through the real tool.
3. `doppler.config.inheritable.ensure` on `<base>/prd`; `doppler.config.inheritable.gate` on it reads `Present`.
4. The gate on `<child>/prd` for `WILLIKINS_NAMES_PROBE` reads `Absent`.
5. A raw `POST /v3/configs/config/secrets` sets the probe secret in `<base>/prd`, with the SHARED VALUES plain-text
   marker as its value. The gate on `<base>/prd` reads `Present` (direct listing).
6. The gate on `<child>/prd` still reads `Absent` (nothing inherited yet).
7. `doppler.config.inherits.ensure` makes `<child>/prd` inherit `[<base>/prd]`: `changed: true`, then `read`
   `Present`. This proves the config body with an underscored `inherits` entry parses live.
8. **Verify item 1.** A test-local `GET /v3/configs/config/secrets/names` on `<child>/prd` (same query as SHARED
   VALUES), deserialized into a local struct with no `Debug`, prints exactly one line:
   `names endpoint lists inherited names: <true|false>`. Nothing else from the body is printed.
9. The gate on `<child>/prd` reads `Present`. This must hold under either answer to step 8.
10. The gate on `<child>/prd` for a never-set name, `WILLIKINS_NAMES_PROBE_ABSENT`, reads `Absent`.
11. The gate on `willikins-names-probe-missing-<unix-seconds>/prd` reads `Absent`, not `Err`.
12. Delete the child, then the base. Both re-read `404`. Disarm the guard.
13. A second `#[ignore]` test, gated on `WILLIKINS_LIVE_LEFTOVER_CHECK=1`, lists the workplace's projects (names only,
    test-local struct) and asserts no `willikins-names-probe-` or `willikins_names_probe_` name remains.

Nothing prints a token, a secret value, or any project other than the throwaway pair.

## Verify before relying on them

1. **Does `GET /v3/configs/config/secrets/names` list inherited names?** Live cycle step 8. The gate is correct either
   way (decision (b4)). Recorded as an addendum here and in `docs/solutions/providers/`.
2. **Can an inheritable config itself inherit?** The docs are silent. If it can, the one-level walk can falsely block
   on a key two levels up (decision (b4)). It is settled by the next sandbox probe that needs it, not by this
   milestone.
3. **`include_managed_secrets=false` drops only `DOPPLER_*` names.** The live cycle's step 5 `Present` shows an
   operator-set name survives it.
4. **What the names endpoint answers for a token with project access but not to that environment.** If `403`, the
   gate fails loudly (`Err`); if `404`/`400`, it blocks (decision (b3)). Either is acceptable. Record which.
5. **willikins' real service account can read `shared_keys/prd`** (operator, pending; real name in the private
   document). Before the app's next real run, a read-only `plan --live` shows the new `base_config_gate` instance as
   `Compute`.
6. **The real base config is inheritable** (operator: "Both configs are inheritable"). The same `plan --live` shows
   it.
7. **The real secret name matches `SecretName`'s grammar** (`[A-Z_][A-Z0-9_]*`). It does on its face; C3's test
   parses it.

## Gates

Per task, scoped, never the full workspace gate (the coordinator runs that). Before **each** cargo command, wait for 3
consecutive seconds in which both `pgrep -x cargo` and `pgrep -f cargo-sweep` print nothing, polled every second,
then start the command in the same shell. Use `-j 2` and `RUST_TEST_THREADS=2`. Run fmt, clippy and tests as separate
commands, in the background with a 600,000 ms timeout, and read the output file's body, never piped through `tail` or
`tee`. A linker "missing .rcgu.o" or `E0463` means the host's cargo-sweep ran: `cargo clean -p <crate>` and rebuild.
Never edit tracked files while cargo builds.

```
cargo fmt --all --check
cargo clippy -p <crate> [-p <crate>...] --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test -p <crate> [-p <crate>...] -j 2 --no-fail-fast
cargo check -p willikins-types -j 2        # whenever willikins-types changed
```

Commit as soon as a commit's scoped gates are green, with `git commit --only <paths>`. Local hooks refuse any commit
or message naming the operator's private setup: never `--no-verify`; rewrite with placeholders.

## Tasks

One lane at a time on `main`, in this order. Each task is test first, one behaviour per commit, green alone, and
commits by path with `git commit --only` (never `git add -A`, `commit -a`, stash, `checkout --` or `reset`), with the
implementer's own `Co-Authored-By` trailer. Nobody pushes.

| # | Task | Delegate to |
| --- | --- | --- |
| A1 | **Underscores in `DopplerProject`** (decisions (a1), (a2); acceptance 1, 2). Commit 1, `willikins-types`: the pattern, `DOPPLER_CONFIG_PATTERN`, the accept/refuse and regex-agrees-with-parse tests, the type doc's grammar-source paragraph (cite the 2026-10-03 slug probe), and the catalog snapshot (two lines). Scoped: `-p willikins-types`, then `cargo check -p willikins-types`. Commit 2, `willikins-providers-doppler` and `willikins-providers-fake` tests only: the fixture `config_get_inherits_underscore.json`, the mock tests, and the fake-state load test. Scoped: `-p willikins-providers-doppler -p willikins-providers-fake` | sonnet implements |
| B1 | **Client** (decision (b2)'s reads, trust boundaries 1–2; acceptance 3). One commit, `client.rs`: `SecretNamesBody`, `secret_name_listed`, unit tests. Scoped: `-p willikins-providers-doppler` | sonnet implements, opus attacks |
| B2 | **The live gate** (decisions (b1)–(b4); acceptance 4–6). One commit: `src/tools/secret_name_gate.rs`, exported from `tools/mod.rs` and `lib.rs`, unit tests, `tests/secret_name_gate_mock.rs` with the source guard, and the `tests/redaction.rs` case. Not yet in any catalog. Scoped: `-p willikins-providers-doppler` | sonnet implements, opus attacks |
| B3 | **Fake twin and parity** (decision (b5); acceptance 7). One commit: `crates/willikins-providers-fake/src/tools/doppler_secret_name_gate.rs`, `SecretsMap::contains_key` if needed, fake catalog registration and snapshot (42), `catalog_parity.rs` plus its new `.snap`, and `fake_agrees_with_live.rs` rows. Scoped: `-p willikins-providers-fake -p willikins-providers-doppler` | sonnet implements |
| B4 | **Registration** (decision (b6); acceptance 8). One commit: `crates/willikins-server/src/catalog.rs` (`LIVE_TOOL_NAMES` 40, the insertion, every count pin), `crates/willikins-providers-doppler/tests/live_catalog.rs` (`[&str; 40]`), the doppler crate's `Cargo.toml` description, and the README's Doppler tool list. Any snapshot listing live tools is regenerated and diff-reviewed (one added name). Scoped: `-p willikins-server -p willikins-providers-doppler`, then `-p willikins-cli` tests only if a CLI snapshot lists live tools | sonnet implements |
| B5 | **Live names cycle** (the section above), written and compiling under `--features live-tests`, never run by the implementer. One commit: the test file and its `[[test]]` entry. Scoped: `cargo clippy -p willikins-providers-doppler --features live-tests --all-targets -j 2 -- -D warnings` | sonnet writes, coordinator runs once |
| C1 | **GATED: `inherits.ensure` passes `config` through** (decision (c2); acceptance 9). Waits for the coordinator's sign-off. One commit: live and fake `doppler.config.inherits.ensure` (the output, known in every branch), their mock and fake tests, `catalog_parity__doppler_config_inherits_ensure_spec.snap`, the fake catalog snapshot, and the characterization snapshot (exactly the three `plan_json` lines, diff-reviewed). Scoped: `-p willikins-providers-doppler -p willikins-providers-fake`, then `-p willikins-dsl` | sonnet implements, opus attacks |
| C2 | **Tracked document and fixtures** (decision (c5); acceptance 10, 11). Commit 1: the three negative fixtures with headers, their acceptance tests beside the existing fixture tests, and three characterization entries. Commit 2: the positive document, its fake state, `crates/willikins-cli/tests/inherited_secret_gate_document.rs`, and its characterization entry. Scoped: `-p willikins-dsl -p willikins-cli` | sonnet implements |
| C3 | **The operator's iOS app document** (decisions (c1), (c3), (c4); acceptance 12). Gitignored paths only, **no commit**: the private document, its fake state, its `crates/willikins-cli/tests/operator_*.rs` tests and their snapshots. The real names come from the coordinator's brief, never from this plan. Scoped: `-p willikins-cli`, the `operator_*` test targets | sonnet implements |
| X1 | **Adversarial pass**, recorded under `docs/research/2026-10-0x-m3j-adversarial-pass.md`, placeholders only. Every bypass becomes a fixture plus a test. At least four mutations restored from saved copies (`cmp` for byte identity). Priority targets: any read of a value, or any endpoint outside trust boundary 1; a dynamic-secret lease; a listed name other than the asked one reaching any output, error or `Debug`; a `Present` from a prefix, suffix, case or substring match; a `Present` from a base the config does not actually inherit; `Foreign`/`NameTaken` reachable; an `Err` where (b3) says `Absent`, or `Absent` where it says `Err`; an underscore project smuggling a query parameter; the pass-through `Unknown` in `Absent` | opus |

Then the coordinator:
- signs off C1's characterization change, or declines it (decision (c2)'s fallback);
- runs B5 once and records verify item 1 here and in `docs/solutions/providers/`;
- asks the operator for, then confirms, verify items 5 and 6 with a read-only `plan --live`;
- runs the full gate;
- shows the operator the app's real `plan --live` (decision (c3)'s shapes) before any real apply.

## Risks

- **The first real run after the change blocks on the key it is about to make visible** (decision (c3)). It is the
  documented two-run shape, not a defect. The gate's `how` says so, and the private test pins it.
- **The walk needs base read access a plain reader does not** (decision (b4)). Every document that inherits already
  needs that access. A base the token cannot see only blocks.
- **A name is not a value.** An empty or wrong key under the right name passes the gate. CI's signing preflight is
  where the key's content is used and fails if it is wrong.
- **Nested inheritance** (verify item 2) can cause a false block, never a false `Present`.
- **Doppler's names list grows unbounded.** The response is names only, bounded by `Http`'s own body limit. A config
  large enough to hit it fails loudly as `Provider`.
- **C1's characterization change.** If the coordinator declines it, the edge falls back to the sibling binding, and
  a blocked base reads as two blocked gates.
- **Privacy.** This milestone's real names live only in gitignored paths. A task that pastes one into a tracked file
  or a message is refused by the hooks, and must be rewritten with placeholders, never bypassed.
- **Host contention.** Other sessions run cargo almost continuously. If no 3-second quiet window comes within 40
  minutes, the task stops and reports the contention.

## Needs the coordinator

1. **Sign off C1's characterization change** (decision (c2)): three `plan_json` lines, nine `inherit` instances
   gaining `config`. The brief's own instruction ("ordered after inheritance is set") requires it, and the boundary
   forbids it. If it is declined, use the fallback edge.
2. **Run B5 once** with the sandbox Doppler token, and record verify item 1.
3. **The operator grants willikins' real service account read access** to the shared APNs base config's `prd` (the
   brief's pending request). Then confirm verify items 5 and 6 read-only.
