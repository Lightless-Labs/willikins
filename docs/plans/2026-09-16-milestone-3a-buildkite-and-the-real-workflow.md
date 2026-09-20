# Milestone 3a: the Buildkite provider and the operator's real workflow

**Created:** 2026-09-16
**Addendum:** 2026-09-20 — tasks 1–9 landed (secret guard, types, naming::v1's tool-facing
output, the willikins-providers-buildkite crate with both tools and their mock tests, the fake
tools, and the server/CLI/README wiring); the live probe (task 5) ran against `willikins-test`
and answers five of the plan's "Verify before relying on them" items — see that section below,
updated in place. Tasks 11 (adversarial pass) and 12 (live write cycle) remain; see the
coordinator's handoff for what is still open.
**Addendum:** 2026-09-20 — task 10 landed: `workflows/new-rust-service-buildkite.yaml` (decision
e, unchanged from the design above) and its acceptance tests 11–14
(`crates/willikins-cli/tests/acceptance_m3a_buildkite.rs`), plus the three deliberately-wrong
fixtures under `workflows/fixtures/` proving the typed-secret and typed-port guarantees hold on
the Buildkite provider's own ports. Two things this task touched that the plan itself did not
name: `crates/willikins-server/tests/acceptance_13_trusted_directory.rs` also pins the real
`workflows/` directory's exact contents (not only `image_contents.rs`'s container-image glob
test) and needed the same update, since it starts a `Butler` against that directory directly;
and `willikins-cli`'s `Cargo.toml` gained `willikins-providers-http`'s `test-support` dev-feature,
needed for `Credential::for_testing` in the type-parity check between the fake and live-shaped
catalogs. Auto-approval is asserted at the `willikins_core::apply` level
(`Approval::Auto` is accepted, `requires_approval` is `false`) but the plan's own wording that
this "journals `ApprovalAutomatic`" is not separately exercised for this document — that event is
`Butler`'s generic behaviour, already pinned for the milestone 1 document, and was judged out of
scope for this task's own tests rather than silently dropped.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (type system, tool contract, naming,
"policy lives in the workflow, never in the tool")
**Research:** `docs/research/2026-09-16-m3a-buildkite.md` — every Buildkite fact below is quoted
verbatim there with its URL; a fact that could not be fetched is in that note's "Verify with a
browser" list and in this plan's own, never in frozen code.
**Depends on:** milestone 2 (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`,
Completed 2026-09-16), whose provider sections are the shape every provider follows and whose
"Notes for milestone 3" carry the operator decisions this slice inherits.

## Goal

The operator can provision one of their four waiting projects with the process they actually
run. Today's positive fixture stores a Doppler token as a GitHub Actions secret; their
organisation runs Buildkite with self-hosted agents, every secret lives in Doppler, and Buildkite
holds one CI/CD Doppler service-account token, so that step is wrong for them in kind, not in
detail. This slice adds a Buildkite provider and a second workflow document that matches their
process: a GitHub repository, a Doppler project with one root config per environment, and a
Buildkite pipeline in an existing cluster, pointed at the new repository, running the repository's
own `.buildkite/pipeline.yml`.

The milestone is done when every acceptance test below passes and one opt-in live cycle against
the `willikins-test` organisation has created a pipeline, re-read it as `Present`, re-ensured it
with `changed: false`, and deleted it.

A property of the new document worth stating because it is the point: **no secret flows through
this graph at all.** Every node is `Reversible`, no `SinkToken` is minted, and nothing in the run
is redacted because nothing in it is secret. The milestone 1 fixture stays exactly as it is — it
is the proof of the typed-secret flow, and this document does not replace it.

## Out of scope

- **`doppler.project_member.ensure`** — granting the organisation's CI service account access to
  the new Doppler project. It is the missing half of the operator's process (milestone 2's notes,
  "CI credential topology", answered 2026-09-16: a grant is per project and names environments),
  but it is a Doppler tool, not a Buildkite one, and this slice is the Buildkite provider. Until
  it lands, **the grant is manual**: a pipeline provisioned by this document cannot read the new
  project's secrets until the operator adds the grant in Doppler. Named here rather than left
  implicit because it is the one thing that makes the document less than the whole process. Next
  slice, 3b.
- **Templates, and the `.buildkite/pipeline.yml` the pipeline uploads.** Milestone 3's template
  half writes that file. Until then a provisioned pipeline has nothing to run, which is harmless
  (a repository with no commits triggers no build) and is the reason the create tool's
  configuration is the upload bootstrap rather than any steps of its own.
- **Pipeline update, delete, and archive tools.** Nothing in the operator's process calls them,
  the tool set is judged by sufficiency rather than completeness, and `willikins-providers-http`
  has no `patch` method at all — a fact this slice deliberately does not change, since a tool
  that cannot `PATCH` cannot silently move a pipeline's URL by re-deriving its slug (research
  note, section 1, "Update and delete"). The live test deletes what it creates through the client
  directly, not through a tool.
- **Queues, cluster creation, and agent tokens.** A queue is named in the pipeline YAML, never in
  the REST pipeline object, so queue targeting is template content. A cluster created through the
  API gets no default queue, so `cluster.ensure` would be a three-call affair for a resource an
  organisation creates once. An agent token is a **cluster-level credential** whose value is
  returned only once and whose creation is keyed on nothing but a free-text description: it is
  neither idempotent by natural key nor something a provisioning run should be able to mint
  (milestone 2's rule for workplace-level credentials), and it is unnecessary — one agent token
  already registers agents for every queue in its cluster, so a new pipeline in the operator's
  existing cluster is reachable by the agents already running.
- **Webhooks and team assignment.** Team changes are GraphQL-only, and a webhook's delivery URL
  is a credential this provider must never handle.
- **Buildkite pipeline templates, `provider_settings`, `tags`, `visibility`, `emoji`, branch
  filters and timeouts as ports.** No document in the operator's process binds them, and a port
  no document calls is a port that cannot be right. Buildkite's own defaults (private, build
  branches, build pull requests, publish commit status) are what an omitted `provider_settings`
  yields, which is what the operator's process wants.
- **Multi-organisation credential routing.** One `WILLIKINS_BUILDKITE_TOKEN`, one Buildkite
  organisation, exactly as milestone 2 serves one GitHub org and one Doppler workplace. The
  routing shape proposed in milestone 2's notes covers all three providers when it lands.
- **Railway, App Store Connect** (milestones 4 and 5). No Railway command of any kind is run by
  this slice.
- **`deploy/teardown.sh`.** Unchanged: the live cycle deletes its own pipeline in-process, and
  the end-to-end smoke run still uses the milestone 1 fixture, which creates no pipeline.

## Trust boundaries

Milestone 2's five boundaries still hold. This slice adds three, each normative and each with an
acceptance test.

6. **A third execution-context credential, with no least-privilege split.**
   `WILLIKINS_BUILDKITE_TOKEN` is a Buildkite **API access token** (`bkua_`), held exactly like
   the other two: a `Credential` in `willikins-providers-http`, never a domain type, never a
   `Value`, never in the registry, a plan, or the journal, with `Credential::authorize` still the
   only `expose_secret` site. It needs `read_pipelines`, `write_pipelines` and `read_clusters`,
   and **that is the whole grant available**: Buildkite's scope table has a Delete column and the
   Pipelines row is `false` in it, so `write_pipelines` covers create, update *and* delete. A
   credential that can create a pipeline can destroy any pipeline in the organisation it reaches.
   This is a real widening compared with GitHub and Doppler and it cannot be narrowed by asking
   for less; it is narrowed only by which organisation the token reaches, which is an operator
   decision recorded in the README. `read_organization_settings` is deliberately **not**
   requested: it would be needed only to observe the org's default cluster, and this slice always
   sends `cluster_id` instead (decision (b)).
7. **Two Buildkite response values are credential-bearing and never leave the client.**
   `provider.webhook_url` is a delivery URL whose path segment is the shared secret, returned
   whenever the token can edit the pipeline; the pagination `Link` header's documented example
   embeds an `api_key` query parameter. So: the pipeline response struct deserializes exactly six
   fields — `id`, `slug`, `web_url`, `repository`, `cluster_id`, `description` — and **no**
   `provider`, `steps`, `configuration`, or `env`; and the cluster listing pages with explicit
   `page` and `per_page` query parameters and **never reads, follows, or logs `Link`**. Neither
   value can reach an output, an error message, the journal, or `tracing`, because neither is ever
   parsed.
8. **The pipeline configuration is willikins' own frozen constant.** Exactly one string in the
   crate looks like commands, it is `const UPLOAD_CONFIGURATION`, it is never built from an input,
   and there is no second one: the crate contains no `PATCH`, no shell, and no caller-supplied
   YAML. The repository URL is likewise constructed from an already-parsed `GitHubRepo`, never
   accepted as a URL. See decision (a).

## Decisions

### (a) The configuration string: a frozen constant, no port

A Buildkite pipeline carries a `configuration`: a YAML document of steps, and steps carry
commands. CLAUDE.md's invariant is flat — *no tool may take a raw URL, shell command, or
arbitrary API path as input* — so the question is what willikins accepts.

**Decision: the tool has no configuration port at all.** Every pipeline it creates is created with
one frozen constant, the bootstrap Buildkite's own documentation gives for exactly this purpose:

```
steps:
 - command: "buildkite-agent pipeline upload"
```

**What it forbids.** A document cannot supply steps, commands, plugins, `env`, an alternative
upload path, or any YAML at all. There is no validated-YAML port and no privileged-content port.
A workflow that wants different CI behaviour changes the repository's own
`.buildkite/pipeline.yml`, which the template half of milestone 3 renders.

**Why the invariant is satisfied rather than bent.** The two rejected alternatives each bend it.
A *validated YAML shape* would still be a caller-supplied command list — validation of the
document's structure says nothing about what the commands do, and the invariant is about the
command, not the YAML. *Document-supplied content treated as privileged*, the way templates are,
is the more serious argument: documents are privileged content run only from a trusted ref, and
willikins already lets a document carry a `TemplateSource` that renders a CLAUDE.md, which is a
strictly more dangerous artifact than a build step. But the two are not the same in one respect
that decides it: a template's content becomes a file in a repository, reviewed as a diff by the
human who merges it, while a pipeline configuration becomes something an agent machine executes
with the agent's own credentials the moment a build is triggered, with no diff in between. The
design doc's reason for the closed step set is precisely this — "an open step would make the
isolation theatre: a prompt-injected agent could mint a token and ship it anywhere" — and a
`command:` port is the open step, wearing YAML. Under this decision the *only* command string in
willikins' Buildkite surface is one constant in its own source, reviewed like code, identical for
every pipeline, and replaceable only by a commit.

**What it costs, and where the policy went.** Policy still lives in the workflow, one level down:
*what CI runs* is the repository's `.buildkite/pipeline.yml`, authored by a template the document
chooses; the constant only decides *where the definition is read from*, and Buildkite documents
that bootstrap as the way to avoid writing a pipeline in a single API string. An organisation
that wants its pipeline defined in Buildkite rather than in the repository cannot express that
here — and that is the narrowing this decision accepts, recorded so a later slice can revisit it.
The upgrade path is additive and needs no redesign: a closed `BuildkitePipelineBootstrap` port
with named variants (upload from the default path, upload from a named path) is a spec change the
parity tests would catch, and none of it is a raw command.

**Two consequences for `read`.** First, `configuration` is not a port, so `read` never compares
it: a human who edits the pipeline's YAML in Buildkite is neither `Foreign` nor `Mismatch`, and
`ensure` will not overwrite the edit, because the crate has no call that could. That is the
intended division — willikins owns the pipeline's existence and address, the repository owns what
it runs. Second, the same applies to `name`: the tool sends `name` equal to the slug and never
compares it, because a display name is a decision with no port here and a mismatch on it would be
a reconcile question this tool declines to have.

**The repository URL.** `repository` is built inside the client as
`git@github.com:{owner}/{name}.git` from an already-parsed `GitHubRepo` — a second frozen form,
recorded as a decision (an HTTPS clone URL, or a non-GitHub provider, would be a closed enum port
later, never a string). `read` compares it exactly, which is how a pipeline pointed at the wrong
repository is caught.

### (b) The cluster: required, supplied explicitly, never discovered or defaulted

A pipeline must belong to a cluster; `cluster_id` is a required create property and it is an
opaque UUID. Three shapes were available.

**Decision: `buildkite.pipeline.ensure` takes a required `BuildkiteClusterId` port and always
sends it.** It never discovers a cluster, never defaults to "the only one", and never omits the
field. A companion read-only tool resolves a human-written cluster *name* to that id inside the
graph (decision (c)), so a document says `Default cluster` and the id is a consequence derived
where consequences belong.

**Why not discover.** "The organisation has one cluster, so use it" is exactly the policy a tool
may not hold, and it is unsafe by construction: correct until someone adds a second cluster, after
which every run either fails or silently picks differently with no document having changed.

**Why not omit and let the organisation default answer.** Three independent objections. It
relocates policy into mutable Buildkite org settings that neither the document nor a plan's reader
can see. `plan` could not report where the pipeline will land without `read_organization_settings`
*and* organisation-administrator rights — a blast-radius increase in the butler's own credential
to buy less information. And the behaviour is not even established: `cluster_id` is documented as
required on both create endpoints while another page describes omission working, and the create
example returns `"cluster_id": null` after a request that set it. Sending the value explicitly
makes the contradiction moot instead of something willikins must resolve.

**Why the id rather than the name on the tool.** A cluster name is mutable and nowhere documented
unique, and there is no name-keyed lookup; only the UUID is stable, which is what a value compared
on every `read` has to be. When `Binding::Org` lands, the cluster id is textbook org configuration
and moves there — the port does not change, only where it is bound from.

### (c) The tool set: two tools, `buildkite.pipeline.ensure` and `buildkite.cluster.get`

`buildkite.pipeline.ensure` is the provisioning step; it needs no justification beyond the goal.

`buildkite.cluster.get` earns its place by naming the workflow step that calls it: the
`buildkite_cluster` node of the new document, which turns the input `cluster: "Default cluster"`
into the `BuildkiteClusterId` the pipeline node binds. Without it, a document must carry a literal
UUID — expressible, and deliberately still expressible, since the pipeline tool takes the id
either way — but unreadable, unexplainable in a plan a human approves, and obtainable only by the
operator running a `curl` by hand. It is modelled one-for-one on `doppler.secret.get`: `pure:
true`, empty key, `Reversible`, a provider read whose `ensure` is the identity of its `read`. It is
emphatically **not** an `ensure`: it creates nothing, and its output is not an idempotence key.

Everything else is left out, with the reasons in "Out of scope": queue, cluster and agent-token
tools (no document would call them in this process, and the last is a credential willikins should
not be able to mint), and update, delete, archive, webhook and team tools.

### (d) Idempotence: key, ownership, and the four observations

**The key is `(org, slug)`** — the pipeline's address, which is exactly what Buildkite's own
by-slug endpoint takes. The slug is derived by `naming::v1` and sent explicitly on create, never
left to Buildkite's lossy derivation from `name`.

**What makes a pipeline ours is its `description`, set to `managed-by: willikins`, compared by
exact equality.** The same marker string and the same exact-match rule as
`doppler.project.ensure`, for the same reason its source records: a pipeline whose description a
human has edited reads `Foreign` rather than being claimed silently. A tag was the alternative and
was rejected because the docs give tags no grammar, while `description` is a documented optional
create property returned on the object.

**What `read` observes** (`GET /v2/organizations/{org}/pipelines/{slug}`):

- `404` → `Absent { predicted }`, predicting `slug` and `url` —
  `https://buildkite.com/{org}/{slug}`, the documented `web_url` form, so a downstream node can
  still plan. *(The 404 body is undocumented; the tool branches on the status alone, which is why
  it is safe to rest on — see "Verify".)*
- `200`, description equal to the marker, `repository` equal to the derived SSH URL, `cluster_id`
  equal to the `cluster` input → `Present`.
- `200`, marker present, `repository` different → `Mismatch { port: repo }`.
- `200`, marker present, repository equal, `cluster_id` different → `Mismatch { port: cluster }`.
- `200`, no marker (or a different description) → `Foreign`.
- anything else → `ToolError::Provider`, message bounded to 256 characters, built from the status
  and the provider's own `message` field only.

`configuration` and `name` are not compared, per decision (a). `Mismatch` is checked in the order
`repo`, then `cluster`, because `Observation::Mismatch` carries one port and a pipeline pointed at
the wrong repository is the more alarming of the two.

**What `ensure` does.** `Present` → `Ensured { changed: false }`. `Foreign` → `Conflict`:
"`{org}/{slug}` already exists and is not ours". `Mismatch` → `Conflict` naming the port and
saying the tool will not change it (the design doc's refuse-do-not-reconcile rule). `Absent` →
`POST /v2/organizations/{org}/pipelines` with `name`, `slug`, `cluster_id`, `repository`,
`description` and `configuration`, and nothing else. **On any create failure the tool re-reads
rather than parsing the error body** — Buildkite documents no status or body for a duplicate
create at all, and three different 422 shapes elsewhere, so nothing in this crate may branch on
one: `Present` → `changed: false`, `Foreign` or `Mismatch` → `Conflict`, still `Absent` → the
original `Provider` error. `POST` is never retried (the shared client's rule), which is what makes
the re-read the only recovery path.

### (e) The real workflow document

`workflows/new-rust-service-buildkite.yaml`, a second positive fixture. The milestone 1 document
stays untouched: it is the acceptance proof that a minted token reaches neither the agent nor the
journal, and deleting that proof to model a process that has no secret in it would trade a
guarantee for a demonstration.

```yaml
name: new-rust-service-buildkite
inputs:
  slug:          ProjectSlug
  org:           GitHubOrg
  buildkite_org: BuildkiteOrg
  cluster:       BuildkiteClusterName   # default: Default cluster
  visibility:    RepoVisibility         # default: private
  environments:  list<EnvironmentSlug>  # default: [dev, stg, prd]
```

| Node | Tool | Binds |
| --- | --- | --- |
| `names` | `naming.v1` | `org` ← `inputs.org`; `slug` ← `inputs.slug` |
| `repo` | `github.repo.ensure` | `repo` ← `steps.names.github_repo`; `visibility` ← `inputs.visibility` |
| `doppler` | `doppler.project.ensure` | `project` ← `steps.names.doppler_project` |
| `configs` | `doppler.config.ensure`, `for_each: inputs.environments` | `project` ← `steps.doppler.project`; `environment` ← `item` |
| `buildkite_cluster` | `buildkite.cluster.get` | `org` ← `inputs.buildkite_org`; `name` ← `inputs.cluster` |
| `pipeline` | `buildkite.pipeline.ensure` | `org` ← `inputs.buildkite_org`; `slug` ← `steps.names.buildkite_pipeline_slug`; `repo` ← `steps.repo.repo`; `cluster` ← `steps.buildkite_cluster.cluster` |

Outputs: `repo_url` ← `steps.repo.url`, `pipeline_url` ← `steps.pipeline.url`.

Three things this shape decides. The `repo` binding is what orders the pipeline after the
repository — the edge is data flow, not a wired dependency, and the pipeline's `repository` field
is therefore derived from the repository willikins just created rather than from a string.
`buildkite_org` is a declared input rather than a literal because it is a decision (which
Buildkite organisation), while the cluster is a declared input **with a default** because it is a
decision the organisation makes once and an agent should not have to know a UUID to run a
workflow. And the graph mints nothing: `token` and `ci_secret` are simply gone, because the
organisation's CI holds one Doppler service-account token for the life of the organisation, so
there is no per-project token to mint and no CI secret store to push into.

The plan's class is `Reversible` throughout, so it auto-approves — correct, and asserted, because
a plan that creates a repository, a project and a pipeline and destroys nothing is exactly the
reversible set the approval gate was drawn around.

## Pinned dependencies (new)

None. The Buildkite provider uses the same `ureq` client, `serde`, `serde_json`, `indexmap`,
`regex` and `thiserror` the other two providers use, through `willikins-providers-http`. No new
third-party crate enters the workspace.

## Workspace layout (additions)

- `crates/willikins-providers-buildkite/` — `src/lib.rs`, `src/client.rs`,
  `src/tools/{mod,pipeline_ensure,cluster_get}.rs`, `fixtures/buildkite/*.json`, `tests/`, with a
  `live-tests` feature gating one `[[test]]` target exactly as `willikins-providers-doppler` does.
- `workflows/new-rust-service-buildkite.yaml` — the second positive fixture.
- `workflows/fixtures/state/buildkite-cluster.json` — fake state seeding one cluster named
  `Default cluster`, so the new document's default resolves offline.

## Crate contracts

### willikins-types (additions)

Four new domain types (table below), registered in `registry::domain_types!` after
`DopplerSecretValue`, and one new `naming::v1` function. Nothing existing changes. `naming::v1` is
frozen for existing rows; the design doc states that adding a row for a new provider is not a
version bump, and the design doc's own join table already carries the row.

### willikins-tools (change)

`naming.v1` gains one output port, `buildkite_pipeline_slug: BuildkitePipelineSlug`, derived from
the `slug` input alone. **No new input**: the Buildkite organisation is not part of the derived
slug, so every existing document keeps checking unchanged. Its description string and both catalog
snapshots are regenerated. Note that `Plan::fingerprint` hashes each node's outputs, so a plan
recorded before this change and applied after it drifts at the `names` node — which is the correct
behaviour of the drift check, not a defect, and `Plan::fingerprint` itself is not modified.

### willikins-providers-buildkite (new)

- `CREDENTIAL_VAR = "WILLIKINS_BUILDKITE_TOKEN"`,
  `CREDENTIAL_PATTERN = "^bkua_[A-Za-z0-9_-]{20,}$"`, and a `BuildkiteCredentialError` with
  `Missing` and `WrongKind`, exactly the shape `willikins-providers-doppler` uses — including its
  rule that the raw value is never pre-inspected to build a better message. `WrongKind`'s message
  says an API access token (`bkua_`) is needed and that an agent token (`bkct_`) cannot provision.
  The body class is deliberately tolerant (`[A-Za-z0-9_-]`, unbounded above): Buildkite masks
  every published token body, and a pattern that refuses a real token blocks the operator, while
  a pattern that accepts a malformed one costs one clear `401`.
- `BUILDKITE_API_BASE_URL = "https://api.buildkite.com"`; `http_client(credential)` builds an
  `Http` with no extra default headers (Buildkite needs only the bearer `Authorization` header,
  and `Http::post` sends its body with `send_json`, which sets `Content-Type` itself).
- `MANAGED_DESCRIPTION = "managed-by: willikins"` — the same bytes as the Doppler marker.
- `UPLOAD_CONFIGURATION` — the frozen bootstrap of decision (a), with a doc comment quoting the
  research note.
- `BuildkiteClient` with exactly four calls, each building its path from already-parsed domain
  types so none can smuggle a `/`, `?` or `&` into a request line: `get_pipeline`,
  `create_pipeline`, `list_clusters_page`, and `delete_pipeline` (`pub(crate)` and used only by
  the opt-in live test, which must clean up after itself; no tool calls it).
- Response structs deserialize only the six pipeline fields and the two cluster fields named in
  trust boundary 7. `list_clusters_page` takes an explicit page number and `per_page=100`, and the
  caller stops at the first short page or at `MAX_CLUSTER_PAGES` (10, i.e. 1,000 clusters), past
  which it returns `Provider` saying the organisation has more clusters than this tool will page.
- Error mapping is the shared one: `404 -> NotFound`, `409`/`422`-already-exists -> `Conflict`,
  `401`/`403` -> `Provider` naming the missing permission and never the credential, everything
  else -> `Provider`, message bounded and escaped. The Buildkite-specific addition is that the
  `errors[]` array is never read: its `code` field holds a human sentence, the 422 body has three
  different documented shapes, and nothing may branch on either.

### willikins-providers-fake (changes)

`FakeState` gains a cluster table (name to id) and a pipeline table keyed on `(org, slug)`.
`FakeBuildkitePipelineEnsure` and `FakeBuildkiteClusterGet` mirror the live `ToolSpec`s exactly
(parity test) and model the same four observations, including `Foreign` (a pipeline in state with
no marker) and both `Mismatch` arms. The fake catalog grows from eleven tools to thirteen.

### willikins-server (changes)

`live_catalog_with` takes a third `Http`; `live_catalog` a third `Credential`;
`live_catalog_from_env` reads `WILLIKINS_BUILDKITE_TOKEN` and gains `LiveCredentialError::Buildkite
{ error }` in the same kind-tagged shape; `LIVE_TOOL_NAMES` becomes eleven, with
`buildkite.pipeline.ensure` and `buildkite.cluster.get` appended. The Buildkite credential is
**required at startup** like the other two: a server that cannot build its whole catalog should
fail at startup naming the variable, not at the first `plan`.

### willikins-cli (changes)

Documentation only: `--live`'s help and `commands.rs`' doc comment name the third variable, and
the test helper that scrubs provider variables from a child process's environment scrubs it too.

### willikins-core (test only)

`tests/secret_literal_guard.rs` learns the Buildkite token family: a `BUILDKITE_TOKEN` constant
matching `bk` + one of the nine published acronyms + `_` + a long alphanumeric run with a floor,
joined into the combined matcher. The floor is chosen, not derived — every published body is
masked with asterisks, so no length may be inferred — and the constant's doc comment says so. No
file in this repository may spell such a literal; every test that needs one assembles it at
runtime with `concat!`, as the existing guard tests do.

### README (change)

The environment-variable reference gains `WILLIKINS_BUILDKITE_TOKEN` with its three scopes and
one sentence of blast radius: `write_pipelines` is also delete.

## The type table

Every port below is a domain type, never a `String`. The "why" column answers the design doc's
test: a type earns its place when parsing at the boundary makes a whole class of failure
impossible later.

| Type | Grammar | Secret | Why a type, not a `String` |
| --- | --- | --- | --- |
| `BuildkiteOrg` | `[a-z0-9]+(?:-[a-z0-9]+)*`, max 100 | no | It is a path segment in every Buildkite request line. Parsing at the boundary is what makes it impossible for a `/`, `?` or `&` to reach a URL — the same argument that keeps `DopplerProject` a type. Buildkite documents no grammar for the organisation slug at all, so this is the shape Buildkite issues (`willikins-test`), chosen conservatively; an org slug with an underscore or a capital is refused at parse time with a named error rather than mis-routed. See "Verify", item 5. |
| `BuildkitePipelineSlug` | `[a-z0-9][a-z0-9-]*`, max 100 | no | The natural key of the pipeline: idempotence and typing are the same mechanism. The documented regex is `\A[a-zA-Z0-9]+[a-zA-Z0-9\-]*\z` with a 100-character cap; this type is that grammar **with uppercase removed**. Not policy narrowing but natural-key canonicalisation: Buildkite lowercases a slug it derives, and whether an explicitly supplied uppercase slug is preserved or folded is undocumented — a key that might not round-trip is not a key. `naming::v1` emits only lowercase, so nothing is lost. |
| `BuildkiteClusterId` | `[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`, exactly 36 | no | Compared on every `read` to decide `Mismatch`. Lowercase hex only, because a mis-cased id would compare unequal and report a cluster mismatch that does not exist. A free string here would make a typo a provider error at apply time instead of a parse error at `describe` time. |
| `BuildkiteClusterName` | 1 to 255 characters, no control character other than space, and none of the invisible or bidirectional characters `Description` refuses | no | A lookup key a human writes and reads (`Default cluster`, with its space). Hand-written rather than derived, following `Description`: the text is compared against provider-returned names and quoted back in a `NotFound` message an agent reads, so it may not carry anything that hides or reorders what a reader sees. Never a natural key — the name is mutable and nowhere documented unique. |

No composite `BuildkitePipeline { org, slug }` type is added. `GitHubRepo` and `DopplerConfig`
exist because something binds them as one value; here nothing does — the address is two ports on
one tool, and a composite would only restate them.

## The tool table

Credential: a Buildkite API access token (`bkua_`) in `WILLIKINS_BUILDKITE_TOKEN`, needing
`read_pipelines`, `write_pipelines` and `read_clusters`. Ownership marker: the pipeline
`description`, exactly `managed-by: willikins`. If a human edits the description, the pipeline
reads `Foreign` and the plan stops with the conflict named; the tool's description says so. Facts
below are from the research note, sections 1 to 3.

| Tool | Inputs | Outputs | Key | Class | Pure |
| --- | --- | --- | --- | --- | --- |
| `buildkite.pipeline.ensure` | `org: BuildkiteOrg`, `slug: BuildkitePipelineSlug`, `repo: GitHubRepo`, `cluster: BuildkiteClusterId` (all required) | `slug: BuildkitePipelineSlug`, `url: HttpsUrl` | `org`, `slug` | `Reversible` | no |
| `buildkite.cluster.get` | `org: BuildkiteOrg`, `name: BuildkiteClusterName` (both required) | `cluster: BuildkiteClusterId` | — | `Reversible` | yes |

| Tool | `read` | `ensure` |
| --- | --- | --- |
| `buildkite.pipeline.ensure` | `GET /v2/organizations/{org}/pipelines/{slug}`: `404` -> `Absent` with `slug` and `url` predicted (`https://buildkite.com/{org}/{slug}`, the documented `web_url` form); `200` with `description` exactly `managed-by: willikins`, `repository` equal to `git@github.com:{owner}/{name}.git` and `cluster_id` equal to the `cluster` input -> `Present`; `200` with the marker and a different `repository` -> `Mismatch { repo }`; `200` with the marker, the same repository and a different `cluster_id` -> `Mismatch { cluster }`; `200` without the marker -> `Foreign`; any other status -> `Provider`. `configuration` and `name` are never compared: neither is a port, and this crate has no call that could change them | `Present` -> `Ensured { changed: false }`; `Foreign` -> `Conflict` naming the address; `Mismatch` -> `Conflict` naming the port and refusing to change it; `Absent` -> `POST /v2/organizations/{org}/pipelines` with `name` (equal to the slug), `slug`, `cluster_id`, `repository`, `description` (the marker) and `configuration` (the frozen upload bootstrap), and no other field. A `POST` is never retried; **any** create failure is resolved by re-`read`ing, never by parsing the error body (`Present` -> `changed: false`, `Foreign`/`Mismatch` -> `Conflict`, `Absent` -> the original `Provider` error) |
| `buildkite.cluster.get` (pure) | `GET /v2/organizations/{org}/clusters?page=N&per_page=100`, pages 1 upward, stopping at the first short page and refusing past 10 pages; `Link` is never read, because its documented form can carry an `api_key`. Exactly one cluster whose `name` equals the input -> `Present` with its `id`; none -> `ToolError::NotFound` naming the cluster name it looked for; two or more -> `ToolError::Conflict` naming the name and the count, never the ids, because a mutable non-unique name cannot be disambiguated by this tool | identity of `read` |

## The `naming::v1` row

One row added; no existing row changes; no version bump (the design doc: "Adding a row for a new
provider is not a version bump", and its own join table already carries this target).

| Target | Join | Example for `third-thoughts` |
| --- | --- | --- |
| Buildkite pipeline slug | kebab | `third-thoughts` |

```rust
pub fn buildkite_pipeline_slug(slug: &ProjectSlug) -> BuildkitePipelineSlug
```

Total and panic-free: `ProjectSlug`'s kebab form (`[a-z][a-z0-9]*(-[a-z0-9]+)*`, at most 32
characters) is inside `BuildkitePipelineSlug`'s grammar and its 100-character cap, which the
property test pins for every valid slug. The Buildkite organisation is not an argument, because
the slug does not depend on it — which is why `naming.v1` needs no new input port.

## Acceptance tests

The slice cannot ship without every one of these. Each names the exact error, status, or
observation it expects.

1. **Catalog parity.** For every tool name in both catalogs the two `ToolSpec`s are equal, pinned
   by insta snapshots; `LIVE_TOOL_NAMES` is exactly the eleven names in order; the fake catalog
   registers thirteen; both catalogs' specs validate against the registry.
2. **`buildkite.pipeline.ensure` read arms** against a mock server: `404` -> `Absent` predicting
   `url` = `https://buildkite.com/willikins-test/third-thoughts`; marker + matching repository +
   matching cluster -> `Present`; marker + `"repository": "git@github.com:willikins-test/other.git"`
   -> `Mismatch { port: repo }`; marker + a different `cluster_id` -> `Mismatch { port: cluster }`;
   `"description": null` -> `Foreign`; `500` -> `ToolError::Provider` with a message at most 256
   characters carrying no body text beyond the provider's `message`; a `429` then `200` ->
   success after retry; a transport timeout -> `Provider`.
3. **The create body.** A JSON matcher pins the `POST` body to exactly `name`, `slug`,
   `cluster_id`, `repository`, `description`, `configuration`: `repository` is
   `git@github.com:lightless-labs/third-thoughts.git`, `configuration` is the frozen upload
   bootstrap byte for byte, and the body carries no `steps`, `env`, `provider_settings`, `teams`,
   `tags` or `visibility` key. The call count asserts exactly one `POST` (never retried).
4. **Ambiguous create.** `POST` answers `500`, the re-`read` answers `200` with the marker ->
   `Ensured { changed: false }` and exactly one `POST`; re-`read` answers `404` -> the original
   `ToolError::Provider`; re-`read` answers `200` without the marker -> `ToolError::Conflict`.
5. **Credential-bearing response values.** A fixture pipeline body carrying a populated
   `provider.webhook_url` with a distinctive marker, served with a `Link` header whose value
   carries `api_key=<another marker>`: neither marker appears in any output, in a `ToolError`
   message, in the `Debug` of anything the tool returns, in the journal file, or in captured
   `tracing` output. A second assertion pins that the response struct has no `provider`, `steps`,
   `configuration` or `env` field at all.
6. **`buildkite.cluster.get`.** One match -> `Present` with the id; zero -> `ToolError::NotFound`
   whose message names `Default cluster`; two -> `ToolError::Conflict` naming the name and `2`;
   a match on the second page -> `Present`, with the recorded requests showing `page=1` then
   `page=2`, both `per_page=100`, and the served `Link` header ignored; eleven full pages ->
   `ToolError::Provider` naming the page bound; `403` -> `Provider` naming the missing permission
   and never the credential.
7. **Credential handling.** `WILLIKINS_BUILDKITE_TOKEN` unset -> `Missing` naming the variable; a
   value assembled at runtime with `concat!` in the agent-token shape (`bkct_` + a long run) ->
   `WrongKind`, whose message says an API access token is needed and names `bkct_`; `Debug` prints
   the redacted marker; a `Credential` built from a distinctive marker value never leaks it through
   `Debug`, a `ProviderError`, a `ToolError`, or a recorded mock request outside the
   `Authorization` header. A trybuild case shows `Serialize` and `Display` still do not compile.
8. **The secret guard learns Buildkite.** The combined matcher flags each of the nine published
   prefixes followed by a long run (each assembled at run time, never spelled in source); does not
   flag `bkup_` followed by a long run, which Buildkite does not publish; does not flag an escaped
   regex pattern constant; and the whole-tree walk stays green with the two new documents and the
   new crate in the tree.
9. **Type tests.** `BuildkitePipelineSlug` accepts `third-thoughts` and a 100-character slug, and
   refuses an uppercase letter, a leading hyphen, an underscore, a dot, an empty string and 101
   characters — each a `ParseError` naming `BuildkitePipelineSlug`. `BuildkiteClusterId` accepts
   the documented example and refuses uppercase hex, a missing group and a trailing character.
   `BuildkiteClusterName` accepts `Default cluster` and refuses a tab, a newline, U+2028, an empty
   string and 256 characters. `BuildkiteOrg` refuses `/`, `?`, `&`, `_` and an empty string. Every
   type's own `example()` parses as itself, through the existing catalog test.
10. **`naming::v1`.** Golden: `buildkite_pipeline_slug` of `third-thoughts` is `third-thoughts`.
    Property: for every valid `ProjectSlug`, the result parses as a `BuildkitePipelineSlug`.
11. **The document checks and loads.** `workflows/new-rust-service-buildkite.yaml` checks clean
    against both catalogs with the same `Checked::types`; the `Butler`'s startup load accepts the
    whole workflows directory; `describe` with no inputs reports exactly `slug`, `org` and
    `buildkite_org` missing (`cluster`, `visibility` and `environments` have defaults); the plan's
    class is `Reversible`, so it auto-approves and journals `ApprovalAutomatic`. The document
    also reaches the container image, so `willikins-server`'s
    `the_image_workflows_directory_holds_exactly_the_two_positive_documents` — which hard-codes
    the two names the `COPY workflows/*.yaml` glob admits — is updated to expect three and
    renamed; nothing under `workflows/fixtures/` may reach the image, which the sibling test
    still pins.
12. **Executor happy path** against empty fake state seeded with one cluster named `Default
    cluster`: `names` and `buildkite_cluster` `Computed`; `repo`, `doppler` and `pipeline`
    `Created`; `configs[dev|stg|prd]` `Unchanged`; `repo_url` and `pipeline_url` `Known`; and no
    value in the run's plan, journal or rendered output carries the redaction marker, because
    nothing in this graph is secret.
13. **Convergence.** `fail_ensure_once` at `pipeline`: the apply stops with `pipeline` `Failed`;
    a new plan shows `NoOp` for the finished nodes and `Create` for `pipeline`; the second apply
    finishes `Created`. A third plan against the finished state shows `NoOp` everywhere.
14. **A cluster that is gone.** Fake state with no cluster of that name: `plan` fails at
    `buildkite_cluster` with the `NotFound` naming the name, and no node after it runs.
15. **Live probe** (`#[ignore]`, `WILLIKINS_LIVE_PROBE=1`, read-only, no feature needed):
    `GET /v2/access-token` records the real scope spellings and `expires_at` and asserts the three
    needed scopes are present, printing neither the token nor a boolean derived from its bytes
    beyond whether the `bkua_` prefix holds; `GET /clusters` records the real cluster shape;
    `GET /pipelines/<absent-slug>` records the status and body of a not-found pipeline. The test
    fails if a recorded fixture's shape disagrees with what the crate parses.
16. **Live write cycle** (`live-tests` feature plus `WILLIKINS_LIVE_TESTS=1`, `#[ignore]`): create
    the pipeline, re-`read` it as `Present`, re-`ensure` it for `changed: false`, delete it through
    the client, re-`read` it as `Absent`. The credential is sourced only inside the single command
    that runs the test and is never printed.
17. **The `gh` guard stays green.** No file this slice adds names the GitHub CLI; the live cycle
    and the probe authenticate as `WILLIKINS_BUILDKITE_TOKEN` alone.

## Gates

Run all four before every commit, exactly as CLAUDE.md spells them, bare `cargo`, each in the
background with a 600,000 ms timeout, reading the log body and never piping it through `tail` or
`tee`:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
```

Never two cargo commands at once; `pgrep -x cargo` before the first. The live probe and the live
write cycle are `#[ignore]`d and run by hand.

## Tasks

Dependency order. This host runs one lane at a time on `main`; no worktrees.

| # | Task | Depends on | Delegate to |
| --- | --- | --- | --- |
| 0 | This plan and `docs/research/2026-09-16-m3a-buildkite.md` | research | planner |
| 1 | `secret_literal_guard` learns the nine Buildkite prefixes, test-first (acceptance test 8). First, so every later test that mentions a token shape is written under a guard that already knows it | 0 | sonnet |
| 2 | `willikins-types`: the four Buildkite domain types, registry entries, and their tests (acceptance test 9) | 0 | sonnet, verified by opus |
| 3 | `willikins-types` + `willikins-tools`: the `naming::v1` row, `naming.v1`'s new output, snapshots (acceptance test 10) | 2 | sonnet |
| 4 | `willikins-providers-buildkite`: credential, `Http` wiring, client (`get_pipeline`, `create_pipeline`, `list_clusters_page`, `delete_pipeline`), response structs, error mapping, fixtures (acceptance tests 7, and 5's struct half) | 2 | sonnet, verified by opus |
| 5 | The read-only live probe (acceptance test 15). **Runs the day it is written**: the sandbox token expires seven days from 2026-09-16, and every "Verify" item the docs left open is answered here or not at all | 4 | sonnet, then the coordinator runs it |
| 6 | `buildkite.pipeline.ensure` and its mock tests (acceptance tests 2, 3, 4, 5) | 4, 5 | sonnet, verified by opus |
| 7 | `buildkite.cluster.get` and its mock tests (acceptance test 6) | 4, 5 | sonnet, verified by opus |
| 8 | `willikins-providers-fake`: cluster and pipeline state, both fake tools, the seeded state fixture, parity snapshots (acceptance test 1, fake half) | 6, 7 | sonnet, verified by opus |
| 9 | `willikins-server` and `willikins-cli`: third credential, `LIVE_TOOL_NAMES`, `LiveCredentialError::Buildkite`, startup refusal, help text, README environment reference (acceptance test 1, live half) | 6, 7 | sonnet |
| 10 | `workflows/new-rust-service-buildkite.yaml` and its acceptance tests (11, 12, 13, 14), including the container-image workflow-glob test that names the admitted documents one by one | 8, 9 | sonnet, verified by opus |
| 11 | Adversarial pass over the new provider and document, recorded under `docs/research/`; every bypass becomes a fixture plus a test | 10 | opus |
| 12 | Live write cycle against `willikins-test` (acceptance test 16), refresh recorded fixtures, mark this plan Completed | 10, 11 | coordinator with the operator |

## Risks

- **The sandbox token expires seven days from 2026-09-16.** Everything that needs it is tasks 5
  and 12, and task 5 is ordered as early as the client allows precisely so the undocumented
  facts (the 404 body, the real scope spellings, the cluster shape) are recorded while the
  credential lives. If it expires first, tasks 6 and 7 still land on mock tests and the verify
  list stays open.
- **`write_pipelines` is also delete.** There is no narrower grant. The mitigation is the
  organisation the credential reaches, not the scope, and it is stated in the README and in trust
  boundary 6 rather than discovered later.
- **Buildkite sends no `Retry-After`.** Its own headers are `RateLimit-Reset` and
  `RateLimit-User-Reset`, in relative seconds, which the shared client does not read. A `429` is
  still retried three times with jittered backoff, and a provisioning run makes about five calls
  against limits of 200/minute per organisation and 50/minute per user, so the exposure is a
  looping live test rather than a real apply. `response_facts` grows to read the two Buildkite
  headers only if the probe actually hits a limit — a change to a shared crate is not worth making
  on a hypothetical.
- **A cluster name is mutable and not documented unique.** The lookup's ambiguity arm is therefore
  reachable in principle and is a hard failure rather than a pick; and a renamed cluster turns a
  working document into a `NotFound` at plan time, which is the loud failure the alternative
  (silently binding a different cluster) would not give.
- **Three undocumented response shapes** (404 body, duplicate-create status, 401 body). Nothing
  branches on any of them: `read` branches on the status alone and `ensure` resolves every create
  failure by re-reading. The probe records them; the plan does not depend on them.
- **The provisioned pipeline will not build** until the organisation's GitHub integration is in
  place and a `.buildkite/pipeline.yml` exists in the repository. Neither is willikins' job in
  this slice, and an empty repository triggers no build, so the failure mode is "nothing happens",
  not a red build.
- **`naming.v1` gains an output**, so a plan recorded before the change drifts when applied after
  it. That is `Plan::fingerprint` working, not breaking; the wire format and the function are
  unchanged.
- **Build time.** One more crate on a host with 11 GB of RAM. `-j 2` and `RUST_TEST_THREADS=2`
  stay load-bearing; never two cargo commands at once.

## Verify before relying on them

The research note's section 5 is the full list with its reasoning. The five that change code if
they come back differently — **items 1 and 2 were answered by the live probe on 2026-09-20**,
against the real `willikins-test` organisation (`tests/live_probe.rs`, run with the sandbox
token, output recorded — redacted of nothing since it named no credential — in this addendum;
the raw responses themselves are under `fixtures/buildkite/live/`, gitignored):

1. ~~**The `404` body and status for an unknown pipeline slug**~~ **Answered.** `GET
   /v2/organizations/willikins-test/pipelines/willikins-probe-does-not-exist` answered `404` with
   a body carrying a `message` field (the shared client's `provider_error_from_body` found one to
   label). The `Absent` arm's reliance on the status alone is confirmed correct and unchanged.
2. ~~**The real scope strings from `GET /v2/access-token`**~~ **Answered.** The plural spelling:
   the sandbox token's `scopes` array contains `read_pipelines`, `write_pipelines`, and
   `read_clusters` verbatim (among many others the token was issued with beyond what this crate
   needs), settling the `read_pipelines`/`read_pipeline` ambiguity in favour of the scope table's
   spelling, not the one worked example's singular. `expires_at` was `2026-09-23T19:38:35Z`,
   confirming the seven-day sandbox window named throughout this plan. The token's `bkua_` prefix
   was never inspected directly — `credential_from_env`'s own pattern match already proved it
   before the probe ran, which is the point of relying on that pattern rather than re-deriving the
   fact from the raw value.
3. **Whether an explicitly supplied `slug` is stored verbatim**, which is what makes the natural
   key round-trip (task 12's create-then-read is the test). Still open: the probe is read-only and
   makes no `POST`.
4. **Whether `cluster_id` is truly rejected when omitted.** Moot under this design, which always
   sends it; it would only soften an error message.
5. **The Buildkite organisation slug's grammar**, undocumented anywhere. `BuildkiteOrg`'s pattern
   is chosen conservatively; an organisation whose slug does not match is refused at parse time
   with a named error, which is a fixable type change, never a mis-routed request. The real
   organisation slug used throughout this milestone, `willikins-test`, parses under this pattern
   without incident, which is as much confirmation as a read-only probe can give.

Also observed, beyond the plan's own numbered list: the real `Default cluster` in `willikins-test`
carries a non-null `default_queue_id` — this organisation's cluster was created through the
Buildkite interface (which does seed a default queue), not through the API (which the research
note says does not), so this is not in tension with that fact. Irrelevant to this slice, which
never reads or sends a queue.

## Notes for the next slice (3b)

- `doppler.project_member.ensure` — inputs `project`, a service-account slug, a project role and a
  list of environments; `read` through `GET /v3/projects/project/members`, `ensure` through
  `POST /v3/projects/project/members`, class `Reversible`. Milestone 2's notes settled the shape
  against the operator's test workplace; adding it makes
  `workflows/new-rust-service-buildkite.yaml` the operator's whole process rather than most of it.
- The two Doppler inheritance tools and an environment-creation tool, also from milestone 2's
  notes, are what let a document express CI and deploy as environments rather than configs.
- A closed `BuildkitePipelineBootstrap` port, if any real process needs a second bootstrap shape;
  additive, and still not a command.
- Multi-organisation credential routing now has three providers to route, not two.
