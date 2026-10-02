# Milestone 3i: the bootstrap writer, and identifiers that print as prefixes

**Created:** 2026-10-02
**Gate:** OPEN for every task except **B8**, which waits on the coordinator (it changes thirteen characterization
entries; see "Needs the coordinator"). Nothing here calls a provider except the live bootstrap cycle (A7), which is
written by an implementer and run once by the coordinator against the SANDBOX Buildkite organisation.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (Trust model; Type system: "Secrecy is a property of the
type"; Tool contract: "The step set is closed"; Templates: "a secret can never be rendered into a committed file").
The design doc and CLAUDE.md's Invariants are the coordinator's to edit; "Sentences that change" below gives the
exact text.
**Depends on:** milestone 3a (`docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`, decision (a),
trust boundaries 7 and 8), milestone 3g (`docs/plans/2026-09-30-milestone-3g-file-writing.md`, decisions (e), (g),
(h), trust boundary 6), milestone 3h (`Tool::updates`, `Action::Update`).
**Operator's words (2026-10-02), the reason this milestone exists.** On the bootstrap paste: "What's this bootstrap
gate thing? Why is it done by hand?", and on the proposal "a tool that writes the stored bootstrap, accepting only
the bootstrap file the document itself renders: never a literal, never text from an input": "Makes sense, yeah."
Earlier: "I have no fucking intention of doing it by fucking hand." On printing, after `plan` printed the App Store
Connect issuer id and key id in full: "Willikins should take care of that. Never print a secret. Only prefixes of
non-secrets. No? Unless you pass a specific param / flag for example."

## Goal

1. **Part A.** `buildkite.pipeline.bootstrap.ensure` writes a willikins-owned Buildkite pipeline's stored YAML
   configuration, and only from a `RepoFile` the document renders. Sample's `bootstrap_gate` (the last manual step
   willikins could do itself) becomes a node that does it. The real read-only `plan --live` of Sample then reads 17
   NoOp, 1 Create (`sample_files`), 1 Update (`bootstrap`), 60 Compute and **1** Blocked (`m5_apns_key`, which stays:
   no Apple API creates an APNs key).
2. **Part B.** Account-revealing identifiers print as a short prefix on every output surface by default, decided by
   a property declared on the domain type beside its secrecy. `--reveal` on the CLI prints them whole. Secrets are
   unchanged.

## Out of scope

- Writing any other pipeline attribute: `name`, `slug`, `repository`, `cluster_id`, `description`, `env`, `steps`,
  `provider_settings`, `teams`, `tags`, `visibility`. Only `configuration` is ever sent.
- Reading a pipeline's configuration from the repository's head (3g decision (h)'s rejected alternative stays
  rejected: the document is the policy).
- Masking provider free text. An App Store Connect `detail` or Buildkite `message` that names a record id passes
  through bounded and escaped as today (verify item 7 records what live Apple errors actually carry).
- Masking a `for_each` instance key, a gate's `subject` or a replacing node's `subject`. All three are pre-rendered
  strings in plan data; decision (b8) pins by test that none of them can be identifier-typed today.
- `tracing` output. Nothing logs a `Value` today (verify item 9).
- Retyping `doppler.value.get` (task B8) until the coordinator signs off its characterization change.

## Trust boundaries (normative)

They extend milestone 3h's, which still hold.

1. **A stored pipeline configuration comes only from a `RepoFile`.** The tool's `configuration` port accepts
   `RepoFile` and nothing else. `check` already refuses a `RepoFile` literal, a `RepoFile` or `TemplateSource`
   workflow input or default, and no conversion row targets `RepoFile` (`Text` never converts to it). So the bytes
   come from a template in a trusted document, the trust level of the seed commit itself.
2. **Only `configuration` is written.** The `PATCH` body is exactly `{"configuration": <content>}`, pinned by a JSON
   body matcher. Never `name`: Buildkite's own documentation says a new `name` without a `slug` regenerates the slug.
3. **Only a pipeline willikins created is written.** Its `description` must equal `MANAGED_DESCRIPTION`; anything
   else is `Foreign` (plan refuses, `ensure` is `Conflict`).
4. **The stored configuration is compared, never echoed.** It never reaches an output, a `ToolError` message, the
   journal, `tracing` or `Debug`. The response struct that carries it has no `Debug`. The `PATCH` response, which
   carries `provider.webhook_url` and `configuration`, is deserialized into nothing (`serde::de::IgnoredAny`), so
   milestone 3a trust boundary 7 survives unchanged.
5. **Live Buildkite writes only in the sandbox organisation** (`WILLIKINS_SANDBOX_BUILDKITE_ORG`), only on a
   throwaway pipeline `willikins-bootstrap-<unix-seconds>` created and deleted by the same guarded test. Nothing is
   written to `example-bk-org` by an agent.
6. **Masking never touches what willikins reads back.** `Value::render()`, `Value`'s `Serialize`, the journal and
   plan fingerprints keep full identifier values; only output surfaces mask (decision (b4)).
7. **Secrets are unchanged.** A secret type stays `[REDACTED ...]` everywhere, with or without `--reveal`. No type is
   both secret and an identifier (a compile error).

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| New tool (A2) | `buildkite.pipeline.bootstrap.ensure` |
| Its inputs, in order | `org: BuildkiteOrg` (required), `slug: BuildkitePipelineSlug` (required), `configuration: RepoFile` (required), `token: BuildkiteToken` (optional) |
| Its outputs | `slug: BuildkitePipelineSlug` (pass-through) |
| Key / class / pure | `[org, slug]` / `Class::Destructive` / `false`; no `Gate` |
| Description string | `Write a willikins-owned Buildkite pipeline's stored YAML configuration from a bootstrap file this document renders.` |
| Path rule (A2) | at least two segments; the second-to-last segment is exactly `.buildkite`; the last segment ends in `.yml` or `.yaml` (lowercase) |
| Content rule (A2) | parses as strict YAML (`serde_yaml_ng::Value`, duplicate keys refused); the top level is a mapping holding a key `steps` whose value is a non-empty sequence |
| Client calls (A1) | `get_pipeline_bootstrap(org, slug) -> PipelineBootstrapBody { description: Option<String>, configuration: Option<String> }` (no `Debug`); `update_pipeline_configuration(org, slug, content: &str) -> Result<(), ProviderError>` |
| Write endpoint | `PATCH /v2/organizations/{org}/pipelines/{slug}`, body exactly `{"configuration": "<RepoFile content>"}`, response read as `serde::de::IgnoredAny`, retried like every `PATCH` (`Http::patch`) |
| `LIVE_TOOL_NAMES` | 38 → 39 (`crates/willikins-server/src/catalog.rs`), inserted right after `buildkite.pipeline.bootstrap.gate`; every site that pins the count |
| Sample node (A6) | `bootstrap`, replacing `bootstrap_gate`: `org: example-bk-org`, `slug: ${{ steps.pipeline.slug }}`, `configuration: ${{ steps.bootstrap_yml.file }}`, `token: ${{ steps.bk_token.value }}` |
| Live cycle (A7) | `crates/willikins-providers-buildkite/tests/live_bootstrap_cycle.rs`, own `[[test]]` with `required-features = ["live-tests"]`, `#[ignore]`, `WILLIKINS_LIVE_TESTS=1`; slug `willikins-bootstrap-<unix-seconds>`; bootstrap path `probe/.buildkite/bootstrap.yml`; content `steps:\n  - block: "willikins bootstrap probe"\n` (a block step: no command exists to run) |
| Type property (B1) | `DomainType::IS_IDENTIFIER: bool` (default `false`), beside `IS_SECRET` |
| Derive attribute (B1) | `#[domain(identifier)]`; `#[domain(secret, identifier)]` is a compile error |
| Object method (B1) | `DomainObject::is_identifier(&self) -> bool` (default `false`; the derive and `impl_domain_object_non_secret!` return `<T as DomainType>::IS_IDENTIFIER`) |
| Catalog field (B2) | `TypeInfo::identifier: bool`, serialized as `"identifier"` after `"secret"` |
| Disclosure (B1) | `willikins_types::Disclosure { Masked, Revealed }` |
| Prefix rule (B1) | `willikins_types::IDENTIFIER_PREFIX_CHARS = 4`; `willikins_types::mask_identifier(s)` = the first `min(4, chars(s) / 2)` characters, then `...` (three ASCII full stops) |
| Examples | `57246542-96fe-1a63-e053-0824d011072a` → `5724...`; `2X9R4HXF34` → `2X9R...`; `AB` → `A...` |
| The seven identifier types (B2) | `AppleIssuerId`, `AppleKeyId`, `AppleCertificateSerial`, `AppleCertificateId`, `AppleBundleIdId`, `AppleProfileId`, `BuildkiteClusterId` |
| Core (B4) | `Value::display(&self, Disclosure) -> Rendered`; `willikins_core::disclosure::mask_json(&mut serde_json::Value)`; JSON marker `"masked": true` beside `"value"` |
| CLI flag (B5) | `--reveal`, global, like `--json` |
| MCP (B6) | always masked; no reveal parameter; `willikins_server::mcp::Masked<T>` wraps every tool result |
| B8 (gated) | new type `DopplerValue` (identifier, not secret); `doppler.value.get`'s `value` output, and the `value` input of `apple.issuer_id.parse` and `apple.key_id.parse`, become `DopplerValue` |
| Characterization snapshot | Part A: Sample's entry changes in exactly four TYPES lines (decision (a9)); two new fixture entries are added. Part B (B1–B7): byte-identical. B8: thirteen entries change, gated |

## Sources, verbatim

`https://buildkite.com/docs/apis/rest-api/pipelines.md`, fetched 2026-10-02 with `curl`, "Update a pipeline":

> To update a pipeline's YAML steps, make a PATCH request to the `pipelines` endpoint, passing the `configuration`
> attribute in the request body

> `configuration` | The YAML pipeline that consists of the build pipeline steps. Setting this attribute replaces the
> entire configuration for the pipeline, so include all existing steps and settings, not just the ones you want to
> change.

> This endpoint ignores `env`. To set environment variables, get the current `configuration` for the pipeline, add
> or update the top-level `env` key, then PATCH the complete `configuration` back.

> `name` | The name of the pipeline. If you provide a new name without a `slug` parameter, the slug will be
> automatically updated to match the new name.

> Required scope: `write_pipelines` / Success response: `200 OK` / `422 Unprocessable Entity` |
> `{ "message": "Validation Failed", "errors": [ ... ] }`

The response example returns the whole pipeline, `provider.webhook_url` and `configuration` included ("The response
only includes a webhook URL in `provider.webhook_url` if the user has edit permissions for the pipeline and the API
access token has the `write_pipelines` scope"). The data model lists `configuration` as "YAML pipeline configuration
(for YAML pipelines)". Sample's `bootstrap.yml` carries its `env` at the top level of the configuration, which is
exactly where the documentation says `env` must live for a YAML pipeline.

## Decisions, part A: the bootstrap writer

### (a1) Why the 3a decision moves, and how far

Milestone 3a decision (a) refused a configuration port because "a pipeline configuration becomes something an agent
machine executes with the agent's own credentials the moment a build is triggered, with no diff in between", and
because the rejected alternative, "document-supplied content treated as privileged", was then undistinguished from
a caller's YAML. Milestone 3g removed that distinction. A `RepoFile` exists only as `repo.file.render`'s output, from
a document-literal `TemplateSource` and `TemplateValue` substitutions whose grammar admits no whitespace, quote, `$`,
backtick, `;`, `|`, `&`, `<`, `>`, `{`, `}` or newline. `check` refuses every other route in. So a `RepoFile` is
document content, at the trust level 3g already accepts for committed files that CI executes (`upload-pipeline.sh`,
`pipeline.yml`). The approval of the plan, which prints the rendered `RepoFile` byte for byte, is the diff review,
the same argument 3g decision (l) made for the seed commit.

What does **not** move: no tool takes a command, a step, a URL, YAML text, `Text`, or any value from a workflow
input. `UPLOAD_CONFIGURATION` stays `buildkite.pipeline.ensure`'s create-time constant. The new tool writes nothing
but `configuration`.

### (a2) The tool: name, ports, observations

`buildkite.pipeline.bootstrap.ensure` (SHARED VALUES). One shared `analyze` computes, after validating `configuration`
(decision (a4)), one of four states:

| State | When | `read` | `updates()` | `ensure` |
| --- | --- | --- | --- | --- |
| `Missing` | `GET` answers `404` | `Absent { predicted: slug }` | `true` | `NotFound`, a static message naming no input |
| `Foreign` | `description` is not `MANAGED_DESCRIPTION` | `Foreign` | `false` | `Conflict` |
| `Equal` | stored `configuration` structurally equal to the `RepoFile`'s content | `Present(slug)` | `false` | `changed: false`, no write |
| `Different` | anything else, including a `null` or unparsable stored configuration | `Absent { predicted: slug }` | `true` | `PATCH`, then re-read: `Equal` → `changed: true`; anything else → `Provider` "the stored configuration does not equal the configuration written" |

So plan is `NoOp` when equal and `Update` when different. A `PATCH` that fails is resolved the way
`doppler.project_member.ensure` resolves one: re-read; `Equal` → `changed: false`; anything else → the original
error.

**Deviation from the task text, with its reason.** The brief asked for "a typed error when the pipeline is absent".
That is what `ensure` does. At **plan** time a `404` reads `Absent` and plans `Update` instead, because on a first
run the upstream `pipeline` node plans `Create`, the pipeline does not exist yet, and a plan-time error would make
every fresh document unplannable (Sample's first run, and `sample_document.rs`'s run 1, among them).
`doppler.project_member.ensure` treats a missing parent project the same way for the same reason. A document that
binds this tool without any node creating the pipeline still fails loudly, at apply, with `NotFound`.

### (a3) Structural comparison, shared with the gate

`structurally_equal` moves from `pipeline_bootstrap_gate.rs` to a crate-private `compare` module, used unchanged by
both tools, so the gate and the writer cannot disagree about "equal". Both sides parse as strict YAML (a duplicate
key on either side is "different"), then into a JSON value, then compare. The fake crate keeps its own copy (a fake
never depends on its live counterpart), and `fake_agrees_with_live` pins the two copies to the same answers.

### (a4) The path and content rule

`configuration`'s `RepoFile` path must have a second-to-last segment exactly `.buildkite` and a last segment ending
in `.yml` or `.yaml`. Its content must parse as strict YAML into a mapping with a non-empty `steps` sequence. Either
failure is `ToolErrorKind::Invalid`, raised in `read` (so at plan time) before any HTTP call, with a static message
that quotes neither the path nor the content.

Why this exact rule:
- **The `.buildkite/` directory is where the document also commits this file.** A stored bootstrap that
  is also a file under the repository's Buildkite directory is reviewable where every other CI file is.
- **Immediate parent, not any ancestor.** Sample's `apps/sample/.buildkite/plugins/stage-input/plugin.yml` is a
  plugin manifest, not a pipeline. "Any ancestor named `.buildkite`" would accept it; "the immediate parent" refuses
  it.
- **The extension is lowercase `.yml`/`.yaml`.** Buildkite's agent looks for exactly those names, and Sample's file is
  `bootstrap.yml`.
- **The content check is a backstop against a template typo.** Buildkite would answer `422` for most bad YAML, but
  only at apply time and after other nodes have run. Refusing at plan time costs one parse. JSON is YAML, so
  Sample's JSON-shaped `bootstrap.yml` passes.

What the rule cannot do: it cannot prove the file was also committed. Acceptance 9 pins, in Sample's document test,
that `bootstrap_yml.file` is bound to both `sample_files.files` and `bootstrap.configuration`.

### (a5) Class `Destructive`, approval always

The `Class` doc defines `Destructive` as "Destroys or overwrites something", and this tool overwrites the stored
configuration without keeping the old value. Willikins never reads it back out, so it cannot restore it.
`Irreversible` ("cannot be trivially undone, but is not destructive") understates that. Both classes require human
approval, and Sample is already `Destructive` through `appstore.profile.ensure`, so the choice does not move Sample's
approval. It matters for a smaller document: a plan whose only write is a bootstrap is never auto-approved.

The asymmetry an approver should know about: the plan shows the **new** content in full (the `RepoFile` is public by
type) but never the **old** stored configuration (trust boundary 4: it may hold an operator's own `env`). The
approval page and CLI text say `Update`, nothing more about what is replaced.

### (a6) Response structs and the `PATCH`

- `PipelineBootstrapBody { description: Option<String>, configuration: Option<String> }`: `Deserialize` only, no
  `Debug`, its own struct. `PipelineConfigurationBody` keeps its one field, and its test pinning exactly one field
  stays. `PipelineBody` keeps its six. `buildkite.pipeline.ensure` still never deserializes `configuration`.
- `update_pipeline_configuration` sends `#[derive(Serialize)] struct UpdateConfigurationBody<'a> { configuration:
  &'a str }` and reads the response as `serde::de::IgnoredAny`. `Http::patch` retries, which is right for an
  idempotent full replacement.
- Error mapping is the crate's shared one. The `422` `errors[]` array is never read (3a's rule). A `401`/`403` names
  `write_pipelines`, never the credential.

### (a7) The fake twin

`FakeBuildkitePipelineBootstrapEnsure` in `willikins-providers-fake`, with the identical `ToolSpec` (catalog parity
snapshot). It reads `BuildkitePipelineRecord::configuration` and `managed`, writes `configuration` on `ensure`,
records its read and write calls like every fake tool, and honours `fail_ensure_once`. It implements `updates()` with
the same four-state table.

### (a8) Registration

The live catalog inserts it after `buildkite.pipeline.bootstrap.gate`, and `LIVE_TOOL_NAMES` grows to 39. It joins
the set of `buildkite.*` tools whose unbound `token` makes `live_catalog_for_document` demand
`WILLIKINS_BUILDKITE_TOKEN`. The fake catalog registers it. The gate stays in both catalogs: a document whose
pipeline an operator manages by hand can still observe it.

### (a9) Sample

- `bootstrap_gate` is removed, and `bootstrap` (SHARED VALUES) takes its place at the same position. The data edges
  order it after `pipeline`, which is after `sample_files`, so the files `bootstrap.yml` runs exist on `main`
  before Buildkite is told to run them.
- The header's W1 paragraph on M7 gets a 3i paragraph: M7 is now a write, by the operator's decision of
  2026-10-02, quoted.
- `buildkite_readme`'s template drops "Paste it into the pipeline's Settings -> Steps page by hand ... Nothing here
  can do that paste for you." and says that `bootstrap.yml` is what `buildkite.pipeline.bootstrap.ensure` stores as
  the pipeline's configuration on every run of the document. `sample_files` has not landed on the real monorepo
  (`plan --live` reads it `Create`), so changing a seeded file now is safe. Its render snapshot is regenerated.
- **Characterization:** Sample's entry changes in exactly these four TYPES lines and nothing else (its PLAN section
  already ends at `issuer_id_text`'s `NotFound`):
  `bootstrap_gate.org: BuildkiteOrg`, `bootstrap_gate.slug: BuildkitePipelineSlug`,
  `bootstrap_gate.expected: RepoFile`, `bootstrap_gate.token: BuildkiteToken` become
  `bootstrap.org: BuildkiteOrg`, `bootstrap.slug: BuildkitePipelineSlug`, `bootstrap.configuration: RepoFile`,
  `bootstrap.token: BuildkiteToken`. This breaks the current boundary ("may change only by the addition of new
  documents"). Milestone 3h's W7 had the same exception for Sample's own entry. The coordinator signs it off (see
  "Needs the coordinator"). The node and port are **not** kept under their old names to make the bytes match:
  `bootstrap_gate.expected` on a writer would mislead every later reader.

### (a10) Negative fixtures

Two new documents under `workflows/fixtures/` (additions, which the characterization rule allows), each with a header
naming its acceptance test and exact error:
- `buildkite-bootstrap-literal-configuration.yaml`: a YAML string literal bound to `configuration`. `check` refuses
  a literal bound to a `RepoFile` port (3g E2's refusal), with exactly one error at `node.configuration`.
- `buildkite-bootstrap-text-configuration.yaml`: `operator.acknowledge`'s `step` output (`Text`) bound to
  `configuration`. Exactly one type-mismatch error, because no `Text => RepoFile` row exists.

## Decisions, part B: identifiers print as prefixes

### (b1) The property lives on the type, beside secrecy

`DomainType::IS_IDENTIFIER`, set by `#[domain(identifier)]`, read through `DomainObject::is_identifier()` and
published as `TypeInfo::identifier`. A property on the type, rather than a list in render code, makes the
classification the type author's decision at the type's definition, and widening it is a one-line attribute
change. `secret` and `identifier` together are a derive compile error, because secrecy already hides more. The default
is `false`, so the hand-written impls (all names, paths, content or secrets) need no change. The exact identifier
set is pinned by a registry test listing the seven names, so a widening is a reviewed diff.

### (b2) Which types, and which deliberately not

**Identifiers (masked by default).** Each one reveals an account or a record in it, and the operator never chooses
any of them:

| Type | Why |
| --- | --- |
| `AppleIssuerId` | the team's App Store Connect API issuer, one per account |
| `AppleKeyId` | names one API key on the account |
| `AppleCertificateSerial` | a distribution certificate's serial number |
| `AppleCertificateId` | Apple's record id for a certificate |
| `AppleBundleIdId` | Apple's record id for a bundle id |
| `AppleProfileId` | Apple's record id for a profile. A "profile UUID" has no domain type today; when one is added it is an identifier |
| `BuildkiteClusterId` | Buildkite's UUID for a cluster: an account record id in another provider |

No Doppler or GitHub domain type is a record id: Doppler's service-account slug (`DopplerSlug`) is internal to the
client and never a `Value`, and GitHub is addressed by names.

**Deliberately not identifiers (printed in full):**
- **Names the operator or document chose.** `ProjectSlug`, `ComponentSlug`, `EnvironmentSlug`, `ProjectName`,
  `WordList`, `WorkflowName`, `GitHubOrg`, `GitHubRepo`, `ActionsSecretName`, `EnvVarName`, `DopplerProject`,
  `DopplerConfigName`, `DopplerConfig`, `DopplerTokenName`, `SecretName`, `DopplerServiceAccountName`,
  `BuildkiteOrg`, `BuildkitePipelineSlug`, `BuildkiteClusterName`, `SigNozIngestionKeyName`, `AppleBundleIdName`,
  `AppleProfileName`. The coordinator's reading, given to the operator: "names, bundle identifiers and repo paths
  still print in full".
- **Bundle identifiers.** `AppleBundleIdentifier`: the same reading, by name.
- **Closed vocabularies.** `RepoVisibility`, `DopplerProjectRole`, `AppleBundleIdPlatform`, `AppleCapabilityType`,
  `AppleObservableCapabilityType`, `AppleCapabilitySetting`, `AppleCertificateType`, `AppleProfileType`,
  `OperatorAcknowledgement`. Each value is one of a fixed public list, so it reveals nothing about the account.
- **Content and paths.** `Text`, `TemplateSource`, `TemplateValue`, `RepoPath`, `GitBranchName`, `CommitHeadline`,
  `RepoFile`, `Description`. An approver must read these whole.
- **URLs.** `HttpsUrl`. Every one a tool produces today is built from names (a repository URL, a pipeline's web
  URL). A future URL embedding a record id gets its own type, not this one.
- **Secrets, already redacted.** `GitHubToken`, `DopplerServiceToken`, `DopplerSecretValue`, `BuildkiteToken`,
  `SigNozIngestionKeyValue`, `OpaqueSecret`, `AppleSigningKey`, `AppleProfileContent`. Each is `IS_IDENTIFIER =
  false`, and making one an identifier as well is a compile error.

**The gap this leaves, stated plainly.** The two lines the operator saw first, `issuer_id_text.value` and
`key_id_text.value`, are `doppler.value.get` outputs of type `Text`, and they print the issuer id and key id in full
one line above the masked typed values. B1–B7 alone do **not** fix what the operator saw. The type-driven fix is task
B8: a new identifier type `DopplerValue` for `doppler.value.get`'s output and for the `value` input of
`apple.issuer_id.parse` and `apple.key_id.parse`. All thirteen documents that call `doppler.value.get` feed its output
only into those two tools. That changes thirteen characterization entries (each `*_text.value` and parse-input TYPES
line), so B8 waits on the coordinator. `Text` itself is not made an identifier: it carries descriptions and rendered
content an approver must read.

### (b3) The prefix: four characters, never more than half, then `...`

`mask_identifier(s)` shows the first `min(4, chars(s) / 2)` characters followed by `...`. Four is enough to recognise
which certificate or key a line is about, and "never more than half" stops a two-character key id from printing
whole. The three ASCII full stops are chosen because no identifier grammar among the seven admits a `.`. A masked
value therefore never parses as its own type, and an agent that feeds one back as an input gets a loud
`ParseError`, never a silently wrong identifier. A property test pins this for all seven. ASCII rather than `…` keeps
every terminal and log readable.

### (b4) Where masking happens, and where it must not

- **Not in `Value::render()` or `Value`'s `Serialize`.** `Plan::fingerprint` (`plan.rs`) fingerprints outputs
  through `render()`, and apply's drift check compares fingerprints. Masked, two certificates sharing four characters
  would fingerprint equal and drift would go undetected. `Value`'s `Serialize` feeds `Redacted<T>`, and
  `apply --plan-id` re-parses recorded inputs from it (`ButlerError::RecordedInputUnreadable`). Masked, every approved
  plan with an identifier input would become unappliable.
- **Live values on an output surface** go through `Value::display(Disclosure)`. It returns `render()` unchanged
  unless the value is a known identifier and the disclosure is `Masked`, in which case each element is
  `mask_identifier`'d. Identifier-ness comes from the object (`DomainObject::is_identifier()`), by construction.
- **Already-serialized JSON** (CLI `--json`, MCP results and errors, journal-sourced `RunRecord`s, the approvals
  page) goes through `mask_json`. It walks the JSON, finds every object of `Value`'s wire shape (`type` a string,
  `list` a bool, `state` `"known"`, `value` present) whose `type` the registry marks `identifier`, replaces `value`
  with its masked form and adds `"masked": true`. A post-pass is the only single chokepoint that also reaches journal
  JSON, which is serialized before any output mode exists. A test builds a `Value` of every registered identifier
  type, serializes it, and asserts `mask_json` masks each one. A second test asserts that no non-identifier type and
  no schema-shaped object (`"type"` without `"list"` and `"state"`) is touched.
- **`Value`'s JSON schema** gains an optional `"masked": { "const": true }` (it is `additionalProperties: false`). That
  one change regenerates schema snapshots in three crates: `willikins-core`'s `schema_generation__*`,
  `willikins-journal`'s `schema_generation__*`, and `willikins-server`'s
  `mcp_server__the_tool_list_and_every_schema_is_snapshotted`. The commit runs all three crates' tests.
- **`Debug`.** `Value`'s `Debug` goes through `display(Masked)`. The derive's generated `Debug` for an identifier type
  prints `AppleIssuerId("5724...")`. `Display` stays the full canonical string, because API paths are built with it
  (`format!("/v1/certificates/{id}")`).

### (b5) The CLI: masked by default, `--reveal` prints whole

A global `--reveal` flag. Without it, every text renderer in `render.rs` goes through `display(Disclosure::Masked)`
(its `value_text` chokepoint takes the disclosure), `run_record_text` reads the journal JSON through `mask_json`, and
every `--json` print site goes through one helper, `print_json(&impl Serialize, Disclosure)`, which applies
`mask_json` unless revealed. With `--reveal`, both text and JSON print full identifiers. Secrets stay redacted either
way. `render.rs`'s module doc gains the rule: no function in it turns a `Value` into text except through
`value_text(value, disclosure)`.

### (b6) The MCP server: always masked, no reveal

Every MCP tool result is wrapped in `Masked<T>`. Its `Serialize` is `to_value(T)`, then `mask_json`, then serialize,
and its `JsonSchema` delegates to `T`'s (with `Value`'s new optional `masked`), so `outputSchema` keeps the same shape.
`domain_error`'s JSON goes through `mask_json` as well. There is **no** reveal parameter. The MCP caller is an agent,
and an agent's transcript is the place the operator least wants account identifiers to spread. An agent never needs a
full identifier: identifiers flow between nodes inside the graph, an identifier input such as Sample's
`serial_number` is one the caller already holds, and approval goes by `plan_id`. An operator who needs a full value
uses the CLI's `--reveal`.

### (b7) The journal keeps full values; the approvals page masks

The journal stores `Redacted<T>` JSON exactly as today, full identifiers included, secrets redacted. Three reasons:
`apply --plan-id` rebuilds inputs from it; the stored fingerprint must compare full values (decision (b4)); and it is
the operator's local audit file, not printed output (verify item 8 confirms its file mode). Everything that prints
from it masks at display: the CLI's `run`/`runs` through `mask_json`, MCP's `run` through `Masked<T>`, and the
approvals page (`http/approvals.rs`, which pretty-prints `record.plan.as_json()`) through `mask_json`, with no reveal.
The page is served over the network, which makes it the least appropriate place for a full identifier, and a prefix
is enough to recognise the record being approved.

### (b8) Pre-rendered strings: pinned so none can be identifier-typed

`BlockedGate::subject`, `Replacing::subject`, a `for_each` instance key, and `describe`'s rendered `default` are
strings rendered from a `Value` before any output mode exists. Today none can carry an identifier: no gate subject
port and no key port in either catalog is identifier-typed, no shipped document has a `for_each` over a list of
identifier type, and no document declares an identifier-typed input default. Three guard tests pin this:
- catalog: no tool's key port, and no gate's subject port, is an identifier type (live and fake);
- documents: no `for_each` source in `workflows/` resolves to an identifier-typed list;
- documents: no input default is identifier-typed.

`describe` additionally renders a default through `display(Masked)`, which costs nothing and closes the last route.

### (b9) Conversions stay disclosure-monotone

`conversions!` gains a compile-time assertion beside the secrecy one: a row from an identifier type must target an
identifier or a secret type. Otherwise a single conversion would re-label an account id as printable `Text`. No row
has an identifier source today. A trybuild case shows a violating row fails to compile.

### (b10) Errors

`ToolError` messages already never quote an input value (its documented contract), and the derive's parse errors
never quote input. The audit this plan ran (every `format!` in the App Store Connect and Buildkite tools) found no
message that interpolates an identifier value. `cluster \`{name}\`` is a name, and `malformed ... id: {err}`
wraps a derived `ParseError` with no input in it. B7's implementer re-runs the audit over `willikins-core`
(`PlanError`, `ApplyError`, `CheckError`), `willikins-server` (`ButlerError`) and every provider crate. Any variant
that renders a `Value` into its message renders it through `display(Masked)` at construction. Error text is never
revealed, even with `--reveal`, and the plan records the result of the audit as an addendum.

## Sentences that change (for the coordinator)

### CLAUDE.md (and AGENTS.md, byte-identical)

1. *"Secret types never implement plain `Display` or serde `Serialize`. Redaction is by construction, not by
   remembering to scrub. `Value` renders through `render()` everywhere."* The last sentence becomes: "`Value` renders
   through `render()` everywhere; an output surface uses `Value::display(Disclosure)` or `mask_json`, which are
   `render()` plus identifier masking." Then a new bullet: "Identifier types (`#[domain(identifier)]`: the App Store
   Connect issuer id, key id and certificate serial, and Apple and Buildkite record ids) print as a short prefix on
   every output surface: CLI text and JSON, MCP results and errors, the approvals page. Only the CLI's `--reveal`
   prints them whole. The journal and plan fingerprints keep full values, because apply and drift detection read them
   back. No type is both secret and an identifier."
2. *"No tool may take a raw URL, shell command, or arbitrary API path as input."* Append: "Content a document renders
   itself is the one exception. A `RepoFile`, built only by `repo.file.render` from a document-literal template and
   typed substitutions and never from a literal, input, default or `Text`, may be committed by
   `github.scaffold.ensure` or stored as a willikins-owned Buildkite pipeline's configuration by
   `buildkite.pipeline.bootstrap.ensure`, whose path must be a YAML file directly under a `.buildkite/` directory."
3. *"A conversion is registered only through `conversions!`, only from a total `From` impl, never secret-to-public (a
   compile error), ..."* becomes "... never secret-to-public and never identifier-to-plain (both compile errors), ...".

### Design doc (`docs/plans/2026-09-11-willikins-design.md`)

- A header line: `**Addendum:** 2026-10-02 — milestone 3i: a Buildkite pipeline's stored configuration is written
  from a document-rendered RepoFile; identifier types print as prefixes on every output surface. See
  docs/plans/2026-10-02-milestone-3i-bootstrap-writer-and-identifier-masking.md.`
- Trust model, *"The agent receives handles and identifiers only, never secret values."* Append: "Account identifiers
  reach it as a short prefix; only the operator's own `--reveal` prints them whole."
- Type system, after *"Secrecy is a property of the type, not a wrapper. ..."*, a new bullet: "Disclosure is a
  property of the type too. An identifier type prints as a prefix on every output surface, decided where the type is
  defined, never in render code; what willikins reads back (the journal, plan fingerprints) keeps the full value."
- Tool contract, after *"The step set is closed."*: "A Buildkite pipeline's stored configuration is the one executed
  artifact willikins writes outside a commit, and only from a `RepoFile` the document renders (milestone 3i)."
- Templates, *"The type system guarantees a secret can never be rendered into a committed file."* becomes "... into a
  committed file or a stored pipeline configuration."

### Earlier plans (task A8 adds the addenda; it edits no decision text)

- Milestone 3a decision (a) ("a frozen constant, no port") and **trust boundary 8** ("the crate contains no `PATCH`,
  no shell, and no caller-supplied YAML") are superseded for `configuration` only. **Trust boundary 7**
  (credential-bearing response values never leave the client) is unchanged and is restated: the `PATCH` response is
  read as `IgnoredAny`. The brief names "trust boundary 7", but the sentence that changes is 8, and 7 is what
  decision (a6) preserves.
- Milestone 3g decision (h) ("writing it stays out") and **trust boundary 6** ("compared, never written") are
  superseded. The paste is no longer manual, and the gate stays in the catalog.

## Acceptance tests

Part A:
1. **Client** (A1, mocks). `get_pipeline_bootstrap` deserializes `description` and `configuration` and nothing else.
   An exhaustive destructure pins two fields, and the struct has no `Debug` (a `static_assertions`-style negative or
   a trybuild case). `update_pipeline_configuration` sends `PATCH` with a JSON body matcher pinning exactly
   `{"configuration": "<content>"}`, with no `name`, `slug`, `steps`, `env` or `description`. A response carrying a
   `provider.webhook_url` marker and a `configuration` marker leaves neither in any `Debug`, error or return value.
   A `422` with an `errors[]` array maps to `Provider` carrying only the bounded `message`.
2. **Tool rows** (A2, mocks), one per row of decision (a2)'s table. `Missing` → read `Absent`, `updates()` `true`,
   `ensure` `NotFound` with zero `PATCH`. `Foreign` → read `Foreign`, `ensure` `Conflict` with zero `PATCH`. `Equal`
   (re-quoted, as Buildkite returns it) → `Present`, `ensure` `changed: false` with zero `PATCH`. `Different` →
   `Absent`, `updates()` `true`, `ensure` one `PATCH` then a re-read `Equal` → `changed: true`. The same with the
   re-read still different → `Provider`. A `PATCH` `500` then a re-read `Equal` → `changed: false`. A `null` stored
   configuration → `Different`. Each write mock pins `.match_query` and `.expect(n)`.
3. **Path and content rule** (A2). Accepted: `apps/sample/.buildkite/bootstrap.yml`, `.buildkite/pipeline.yaml`
   (two segments). Refused with `Invalid` and zero HTTP calls: `apps/sample/.buildkite/plugins/stage-input/plugin.yml`,
   `apps/sample/bootstrap.yml`, `apps/sample/.buildkite/bootstrap.YML`, `apps/sample/.buildkite/upload-pipeline.sh`,
   `apps/sample/.Buildkite/bootstrap.yml`; content `steps: []`, `env: {}` with no `steps`, a top-level sequence,
   unparsable text, and a duplicate key. No refusal message contains the path or the content.
4. **Never echoed** (A2, `tests/redaction.rs` extended). A secret-shaped marker seeded into the mocked stored
   configuration, and another into the `PATCH` response, appear in no output, `ToolError` message, `Debug` of
   anything returned, journal line or captured `tracing`.
5. **Spec** (A2). Validates against the registry. Key `[org, slug]`, `Destructive`, not pure, no gate, one output
   `slug`. The `configuration` port's type is exactly `RepoFile`.
6. **Gate and writer agree** (A2). For every row of a shared table of (stored, rendered) pairs, the gate's `Present`
   equals the writer's `Equal`.
7. **Fake parity** (A3). The `catalog_parity` snapshot of the new spec is identical for live and fake. In
   `fake_agrees_with_live`, every decision-(a2) row reads, `updates()`s and `ensure`s alike. The fake catalog snapshot
   gains one tool. `fail_ensure_once` at the fake → `Failed`, then a re-plan `Update`, then a re-apply
   `changed: true`.
8. **Registration** (A4). `LIVE_TOOL_NAMES` has 39 names in order. Every count pin is updated. A document with an
   unbound `token` on this tool demands `WILLIKINS_BUILDKITE_TOKEN`, and one with it bound does not.
9. **Sample** (A6). `bootstrap` binds `pipeline.slug` and `bootstrap_yml.file`, and `bootstrap_yml.file` is also an
   element of `sample_files.files`. No node is named `bootstrap_gate`. Fresh fake state: run 1's plan has `bootstrap`
   `Update` and its apply writes the fake pipeline's configuration, so the blocked set is the App Store gates plus
   `m5_apns_key`, never a bootstrap gate. After the App Store gates open, the blocked set is `{m5_apns_key}`. A third
   run with `m5_apns_key_done=done` is NoOp/Compute everywhere, `bootstrap` `NoOp`. The plan class is `Destructive`.
   `sample_apply_blocked_redaction.rs` is updated to match. The README render snapshot no longer says "by hand".
10. **Characterization** (A5, A6). Two new fixture entries (A5). Sample's entry differs in exactly the four lines of
    decision (a9) (A6). Every other entry is byte-identical.
11. **Fixtures** (A5). `buildkite-bootstrap-literal-configuration.yaml` → exactly one `check` error at
    `<node>.configuration` (the 3g literal-into-`RepoFile` refusal). `buildkite-bootstrap-text-configuration.yaml` →
    exactly one type error, `Text` into `RepoFile`.
12. **Guards stay green.** `secret_literal_guard`, `no_gh_writes_guard` and `no_certificate_writes_guard`.

Part B:
13. **Types** (B1, B2). `#[domain(identifier)]` sets `IS_IDENTIFIER`. `#[domain(secret, identifier)]` fails to
    compile (trybuild). The registry's identifier set is exactly the seven SHARED VALUES names (pinned). For each of
    the seven, `mask_identifier(example)` matches SHARED VALUES' rule and does **not** parse as the type (a proptest
    over generated valid values too). A derived identifier's `Debug` prints the prefix and its `Display` the full
    value. `catalog__catalog_json_snapshot` gains `"identifier"` on every entry, `true` on exactly seven.
14. **Monotone conversions** (B3). A trybuild row `AppleIssuerId => Text` fails to compile with the new message.
    The three production rows still build.
15. **Core** (B4). `display(Revealed)` equals `render()` for every type. `display(Masked)` masks known identifier
    scalars and lists and leaves everything else (secrets included) equal to `render()`. `Value`'s `Debug` is masked.
    `render()`, `Serialize` and `Plan::fingerprint` are byte-identical before and after, pinned by a test that
    fingerprints a plan holding an identifier and asserts the full value is present. `mask_json` masks a serialized
    `Value` of every identifier type (scalar and list), adds `"masked": true`, and leaves non-identifier `Value`s,
    secrets and schema-shaped objects untouched. The `Value` schema accepts `"masked": true` and rejects
    `"masked": false`.
16. **CLI** (B5). Against fake state seeded with a certificate and a profile (`apple-signing-credential-from-doppler`
    or an equivalent fake-state document), `plan` text shows `5724...`-style prefixes for every identifier output and
    no full identifier anywhere in stdout. `--json` gives the same prefixes and `"masked": true`. `--reveal` and
    `--reveal --json` show full values. `run`/`runs` over a recorded journal mask the same way. Secrets stay
    `[REDACTED ...]` under `--reveal`.
17. **MCP** (B6). `plan`, `describe` and `run` over MCP, against the same fake state: no full identifier in
    `structured_content` or in the text content; `"masked": true` present. A domain error whose JSON embeds a `Value`
    is masked. The tool-list snapshot changes only by the schema's new `masked` property.
18. **Approvals page** (B6). The rendered page for a recorded plan holding an identifier shows the prefix and never
    the full value. The journal line for the same plan holds the full value.
19. **Journal unchanged** (B4–B6). `apply --plan-id` of an approved plan with an `AppleCertificateSerial` input
    still rebuilds and applies (no `RecordedInputUnreadable`).
20. **Pre-rendered strings** (B7). The three decision-(b8) guards, plus `describe` rendering an identifier default
    masked.
21. **Characterization** (B1–B7). Byte-identical.
22. **B8 (gated).** `doppler.value.get`'s `value` and the two parse tools' `value` input are `DopplerValue`. Sample's
    `plan` text prints `issuer_id_text.value: 5724...`-style prefixes. All thirteen documents still check. The
    thirteen characterization entries change only in their `*_text.value`/parse-input TYPES lines and the
    `doppler.value.get`/parse-tool spec snapshots.

## The live bootstrap cycle (written by A7, run once by the coordinator)

`crates/willikins-providers-buildkite/tests/live_bootstrap_cycle.rs`, with its credentials sourced only in the
command that runs it:

```text
source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
  cargo test -p willikins-providers-buildkite --features live-tests --test live_bootstrap_cycle \
  -j 2 -- --ignored --nocapture
```

1. Count the sandbox organisation's pipelines: `GET /v2/organizations/{org}/pipelines?page=N&per_page=100` into
   `Vec<serde::de::IgnoredAny>`, paged to the first short page, through the crate's `http_client` (nothing is parsed
   but the count). Record `N0`.
2. Slug `willikins-bootstrap-<unix-seconds>`. It must read `404`. Arm a `PipelineGuard` that deletes exactly that
   slug on every exit path, recorded before any assertion.
3. `buildkite.cluster.get` resolves `Default cluster`. `buildkite.pipeline.ensure` creates the pipeline (the frozen
   bootstrap, `managed-by: willikins`). The count is `N0 + 1`.
4. `buildkite.pipeline.bootstrap.ensure` with the SHARED VALUES `RepoFile`: `read` is `Absent`, `updates()` is `true`.
   The gate with the same file reads `Absent`.
5. `ensure` → `changed: true`. `read` → `Present`. A second `ensure` → `changed: false`. The gate now reads `Present`.
6. `buildkite.pipeline.ensure`'s `read` is still `Present`: repository, cluster and description are unchanged by the
   `PATCH`.
7. Delete through `BuildkiteClient::delete_pipeline`. Re-read → `404`. The count is `N0`. Disarm the guard.
8. A second `#[ignore]` test, `the_bootstrap_cycles_pipeline_is_gone`, gated on `WILLIKINS_LIVE_LEFTOVER_CHECK=1`,
   lists the organisation's pipelines and asserts that no `willikins-bootstrap-*` slug remains.

`Foreign` cannot be proved live (making a pipeline foreign needs a `description` `PATCH`, which this crate never
sends), so the mocks prove it. Nothing prints a token, the configuration read back, or any pipeline other than the
throwaway one.

## Verify before relying on them

1. **`PATCH` with only `configuration` leaves `name`, `slug`, `repository`, `cluster_id` and `description` unchanged.**
   The live cycle's step 6 answers it.
2. **The stored configuration after a `PATCH` re-reads structurally equal to what was sent.** Step 5's second
   `ensure` answers it. If Buildkite normalises beyond re-quoting, the post-write check fails loudly as `Provider`,
   and the comparison is revisited.
3. **A `PATCH` does not itself trigger a build.** Record `scheduled_builds_count`/`running_builds_count` in a
   test-local struct in the live cycle (never in the crate) before and after step 5.
4. **The real `example-bk-org/sample` pipeline's triggers (coordinator, read-only, outside the client).** Apply
   order is `sample_files` → `pipeline` → `bootstrap`, so the seed commit lands on `main` before the `PATCH`. If the
   pipeline has a live GitHub webhook or branch trigger, that push builds with the configuration stored at that
   moment (today the frozen `buildkite-agent pipeline upload`, which reads the monorepo root's
   `.buildkite/pipeline.yml`, not Sample's). Read the pipeline's provider settings and whether a webhook exists
   before the real apply. Do not read them through this crate (trust boundary 4 and 3a's boundary 7).
5. **The real organisation's `PIPELINE_CREATION_TOKEN` (`buildkite/prd`) carries `write_pipelines`.** It already
   created the pipeline, so it should. The real apply is the first `PATCH` it sends.
6. **The largest configuration Buildkite accepts.** `RepoFile` admits 65,536 characters, and Sample's bootstrap is
   under 2 KB. An oversize `422` is `Provider` with a bounded message.
7. **What live App Store Connect error `detail` strings carry.** If they name record ids, a follow-up decides
   whether the App Store Connect client should print `code` and `title` only (decision (b10)'s residual).
8. **The journal file's mode** is `0600` or narrower on this host, since it keeps full identifiers by design.
9. **No `tracing` call records a `Value` or a `Debug` of one.** A grep at B4. If one exists, it inherits
   `Debug`'s masking from B4.

## Gates

Per task, scoped, never the full workspace gate (the coordinator runs that). Before each cargo command, wait for 20
consecutive seconds in which both `pgrep -x cargo` and `pgrep -f cargo-sweep` print nothing, polled every second in
one backgrounded shell loop that then starts the command in the same shell. Use `-j 2` and `RUST_TEST_THREADS=2`.
Run in the background with a 600,000 ms timeout and read the output file's body, never piped through `tail` or
`tee`. A linker "missing .rcgu.o" or `E0463` means the host's cargo-sweep ran: `cargo clean -p <crate>` and rebuild.

```
cargo fmt --all --check
cargo clippy -p <crate> [-p <crate>...] --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test -p <crate> [-p <crate>...] -j 2 --no-fail-fast
cargo check -p willikins-types -j 2        # whenever willikins-types or willikins-derive changed
```

Commit as soon as a commit's scoped gates are green, with `git commit --only <paths>`.

## Tasks

One lane at a time on `main`, in this order. Each task is test first, one behaviour per commit, green alone, and
commits by path with `git commit --only` (never `git add -A`, `commit -a`, stash, `checkout --` or `reset`), with the
implementer's own `Co-Authored-By` trailer. Nobody pushes.

| # | Task | Delegate to |
| --- | --- | --- |
| A1 | **Client** (decision (a6); acceptance 1). One commit, `crates/willikins-providers-buildkite/src/client.rs` (+ `lib.rs` re-exports only if a test needs them) and a new `tests/bootstrap_client_mock.rs`: `PipelineBootstrapBody`, `get_pipeline_bootstrap`, `UpdateConfigurationBody`, `update_pipeline_configuration`. Both are `pub(crate)` unless a `tests/*.rs` target must call them, in which case `pub` with the visibility rationale `delete_pipeline` already documents. Scoped: `-p willikins-providers-buildkite` | sonnet implements, opus attacks |
| A2 | **The live tool** (decisions (a2)–(a5); acceptance 2–6). Commit 1: move `structurally_equal` into `src/tools/compare.rs` (crate-private), the gate calls it, no behaviour change. Commit 2: `src/tools/pipeline_bootstrap_ensure.rs` with the four-state `analyze`, `read`/`updates`/`ensure`, the path and content rule, unit tests, `tests/pipeline_bootstrap_ensure_mock.rs`, and `tests/redaction.rs` extended. Exported from `tools/mod.rs` and `lib.rs`. Not yet in any catalog. Scoped: `-p willikins-providers-buildkite` | sonnet implements, opus attacks |
| A3 | **Fake twin and parity** (decision (a7); acceptance 7). One commit: `crates/willikins-providers-fake/src/tools/buildkite_pipeline_bootstrap_ensure.rs`, registration in the fake catalog, the fake catalog snapshot, `catalog_parity.rs` plus its new `.snap`, and `fake_agrees_with_live.rs` rows. Every fake count pin is updated. Scoped: `-p willikins-providers-fake -p willikins-providers-buildkite` | sonnet implements, opus attacks |
| A4 | **Registration** (decision (a8); acceptance 8). One commit: `crates/willikins-server/src/catalog.rs` (`LIVE_TOOL_NAMES` 39, live insertion, the token-demand set, every count pin), the README's tool list and environment reference, and the buildkite crate's `Cargo.toml` `description`. Any MCP or catalog snapshot that lists live tools is regenerated and diff-reviewed (one added name). Scoped: `-p willikins-server -p willikins-cli` | sonnet implements |
| A5 | **Negative fixtures** (decision (a10); acceptance 11, and acceptance 10's two additions). One commit: the two fixture documents with headers, their acceptance tests (next to the 3g `RepoFile` refusals in `crates/willikins-dsl/tests/acceptance.rs` or `crates/willikins-cli/tests/`), and the characterization snapshot with exactly two new entries. Scoped: `-p willikins-dsl -p willikins-cli` | sonnet implements |
| A6 | **Sample** (decision (a9); acceptance 9, 10). **Coordinator sign-off on the four-line characterization change is needed first** (see "Needs the coordinator"). One commit: `workflows/sample-ios-app.yaml` (`bootstrap` replaces `bootstrap_gate`, a 3i header paragraph, the `buildkite_readme` template), `crates/willikins-cli/tests/sample_document.rs`, `crates/willikins-cli/tests/sample_apply_blocked_redaction.rs`, the README render snapshot, and the characterization diff (Sample's four lines only). Scoped: `-p willikins-cli -p willikins-dsl` | sonnet implements, opus attacks |
| A7 | **Live bootstrap cycle** (the section above), written and compiling under `--features live-tests`, never run by the implementer. One commit: the test file and its `[[test]]` entry. Scoped: `cargo clippy -p willikins-providers-buildkite --features live-tests --all-targets` | sonnet writes, coordinator runs once |
| A8 | **Addenda to the 3a and 3g plans** ("Earlier plans" above). One commit, docs only: an `**Addendum:** 2026-10-02 (milestone 3i)` line in each header naming what is superseded and pointing here. No decision text is edited | sonnet |
| B1 | **The disclosure mechanism** (decisions (b1), (b3); acceptance 13's mechanism half). One commit, `willikins-derive` and `willikins-types`: `IS_IDENTIFIER`, `#[domain(identifier)]`, the secret-plus-identifier compile error (trybuild), `DomainObject::is_identifier`, `Disclosure`, `IDENTIFIER_PREFIX_CHARS`, `mask_identifier` with its unit tests, and the derive's masked `Debug` for identifier types. No type is marked yet. Scoped: `-p willikins-derive -p willikins-types`, then `cargo check -p willikins-types` | sonnet implements, opus attacks |
| B2 | **The seven types** (decision (b2); acceptance 13). One commit, `willikins-types`: `#[domain(identifier)]` on the seven, `TypeInfo::identifier`, the pinned identifier-set test, the never-parses property tests, and `catalog__catalog_json_snapshot` regenerated. Grep for any other snapshot holding `"secret":` and regenerate it in the same commit. Scoped: `-p willikins-types`, then the crate of any other snapshot found | sonnet implements |
| B3 | **Monotone conversions** (decision (b9); acceptance 14). One commit, `willikins-types/src/registry.rs` (`conversions!`) plus a trybuild case | sonnet implements, opus attacks |
| B4 | **Core** (decision (b4); acceptance 15, 19). Commit 1, `willikins-core`: `Value::display`, `Value`'s masked `Debug`, `disclosure::mask_json` with its every-identifier-type test, `describe`'s default through `display(Masked)`, and the fingerprint/serialize byte-identity tests. Commit 2: the `Value` schema's optional `masked`, regenerating `willikins-core`'s, `willikins-journal`'s and `willikins-server`'s schema snapshots (diff = the one property). Scoped: commit 1 `-p willikins-core`; commit 2 `-p willikins-core -p willikins-journal -p willikins-server` | sonnet implements, opus attacks |
| B5 | **CLI** (decision (b5); acceptance 16). One commit, `willikins-cli`: global `--reveal`, `value_text(value, disclosure)`, every text renderer threaded, `print_json`, `run_record_text` through `mask_json`, and the CLI tests. Scoped: `-p willikins-cli` | sonnet implements, opus attacks |
| B6 | **MCP and approvals page** (decisions (b6), (b7); acceptance 17, 18). One commit, `willikins-server`: `Masked<T>` on every tool result, `domain_error` masked, the approvals page through `mask_json`, and their tests. Scoped: `-p willikins-server` | sonnet implements, opus attacks |
| B7 | **Pre-rendered strings and the error audit** (decisions (b8), (b10); acceptance 20, 21). Commit 1: the three guard tests (catalog in `willikins-server` or `willikins-cli` tests, documents in `willikins-cli` tests). Commit 2, only if the audit finds a variant: mask it at construction, with a test. Then an addendum to this plan recording the audit's result | sonnet implements |
| B8 | **GATED: `DopplerValue`** (decision (b2)'s gap; acceptance 22). Waits for the coordinator. Commit 1, `willikins-types`: the `DopplerValue` type (`#[domain(identifier, max_len = 65536)]`, not secret, description "A non-secret value read from Doppler, of unknown shape; printed as a prefix."), registered. Commit 2: `doppler.value.get` live and fake, `apple.issuer_id.parse`, `apple.key_id.parse`, their spec snapshots, and the thirteen characterization entries, each diff-reviewed to touch only those TYPES lines. Scoped: the crates touched, one cargo at a time | sonnet implements, opus attacks |
| X1 | **Adversarial passes**, one per part, recorded under `docs/research/2026-10-0x-m3i-adversarial-pass-*.md`. Every bypass becomes a fixture plus a test. Each pass restores at least four mutations from saved copies (`cmp` for byte identity). Priority targets: a `PATCH` body with any key but `configuration`; any `RepoFile` route from a literal, input, default or `Text`; a path outside the rule accepted; the stored configuration or a webhook URL reaching any output; `Update` where `Foreign` or `NotFound` is right; a full identifier reaching any output surface without `--reveal`; masking leaking into the fingerprint, journal or recorded inputs; a secret printed under `--reveal` | opus |

Then the coordinator:
- signs off A6's and B8's characterization changes;
- edits CLAUDE.md, AGENTS.md and the design doc ("Sentences that change");
- runs A7 once and records it here as an addendum;
- reads verify items 4, 5 and 8;
- runs the full gate;
- shows the operator the real `plan --live` of Sample (expected: 17 NoOp, 1 Create, 1 Update, 60 Compute,
  1 Blocked) before any real apply.

## Risks

- **A push-triggered build between the seed commit and the `PATCH`** (verify item 4). The window is one apply long,
  and the build it could start runs the configuration that is stored today, not anything new. The mitigation is
  reading the trigger settings first, not reordering the graph: the `PATCH` must come after the files exist.
- **The approver never sees what is replaced.** Decision (a5). The old configuration may hold an operator's `env`,
  so it is never echoed. A pipeline whose stored configuration a human hand-edited loses that edit on the next apply.
  That is the operator's decision ("the document is the policy"), and the plan's `Update` line is where an approver
  notices.
- **Buildkite normalises more than quoting** (verify item 2). Then every run plans `Update`, and the post-write
  check fails loudly rather than looping silently.
- **`mask_json` is a shape match.** A future type that serializes the `Value` wire shape for something that is not a
  `Value` would be masked. That fails closed (over-masking), never open. The every-identifier-type test catches the
  opposite failure, a `Value` whose shape drifts away from what the matcher expects.
- **Over-masking breaks an agent workflow.** An agent that needs a full certificate serial must get it from the
  operator. That is the intent (decision (b6)). It would surface as an agent asking, not as a silent failure,
  because a masked value never parses.
- **B8's characterization change is large.** It touches thirteen entries, and without it the operator's own
  complaint stands (decision (b2)). If the coordinator declines, the plan's Goal 2 is met for the typed values only,
  and HANDOFF should say so.
- **Snapshot churn across crates in one commit** (B4 commit 2, A4). The diff of each snapshot must be the one
  property or the one tool name. Anything else stops the task.
- **Host contention.** Other projects run cargo almost continuously. If no 20-second quiet window comes within 40
  minutes, the task stops and reports the contention rather than running alongside.

## Needs the coordinator

1. **Sign off A6's characterization change**: Sample's entry, exactly four TYPES lines (decision (a9)). The task's
   own instruction (Sample replaces `bootstrap_gate`) requires it, and the current boundary forbids it.
2. **Decide B8**: thirteen characterization entries change. Without B8, `issuer_id_text.value` and
   `key_id_text.value` still print the issuer id and key id in full, which is the output the operator objected to.
3. **Edit CLAUDE.md, AGENTS.md and the design doc** with "Sentences that change".
4. **Run A7** with the sandbox Buildkite token, and **read verify items 4, 5 and 8** before the real apply.
