# Milestone 1: the core, with no real providers

**Created:** 2026-09-11
**Addendum:** 2026-09-11 — pinned dependencies and slug grammar filled in from the research note.
**Reviewed:** 2026-09-11 (via document-review workflow: scope, feasibility, security, coherence, adversarial personas). 23 findings folded in; see "Review resolutions" at the end.
**Design:** `docs/plans/2026-09-11-willikins-design.md`
**Research:** `docs/research/2026-09-11-m1-dependencies.md`

## Goal

A Rust workspace in which a YAML workflow is parsed into a typed graph, statically checked,
described to an agent, and planned against fake providers. Nothing talks to a real API. The
milestone is done when every acceptance test below passes, including the adversarial passes,
and `plan` produces a concrete, approval-classified plan for the project-creation fixture
in which provider names are derived inside the graph and no secret byte reaches any output.

## Out of scope

Real providers, `apply`, the run ledger, the MCP server, org configuration as a binding
source, layer-gated inputs, `when` guards, authentication, hosting, and the templating
system (profile layers, versioned re-render, three-way merge). Those are milestones 2 and
later.

In scope despite the names: a minimal `template.render` fake tool, because it is the
non-secret sink the taint proof needs; and a `fake.irreversible.ensure` tool, because the
approval-class computation needs one irreversible node to test against.

The CLI mirrors the milestone 2 MCP surface one to one, so milestone 2 is a transport, not
a redesign.

## Pinned dependencies

From the research note. Caret requirements on the major; `Cargo.lock` pins the rest.
Versions confirmed against the registry on 2026-09-11.

| Crate | Requirement | Why |
| --- | --- | --- |
| `schemars` | `1.2` | JSON schema for every domain type and the document format; `rmcp` requires 1.x |
| `serde`, `serde_json` | `1` | serialization |
| `serde_yaml_ng` | `0.10` | maintained fork of `serde_yaml` with `Error::location()`; `serde_yml` is deprecated and has a RustSec advisory |
| `secrecy` | `0.10` | `SecretString` storage inside secret domain types; `serde` feature for deserialize only, never `SerializableSecret` |
| `thiserror` | `2` | error types |
| `petgraph` | `0.8` | topological order and cycle detection |
| `unicode-normalization` | `0.1` | NFKD in `propose_slug` |
| `indexmap` | `2` | ordered maps for ports, steps, inputs |
| `clap` | `4` | CLI |
| `syn`, `quote`, `proc-macro2` | `2`, `1`, `1` | derive macro |
| `proptest`, `trybuild`, `insta` | dev | property, compile-fail, and snapshot tests |
| `rmcp` | `3` (milestone 2) | official MCP SDK; features `server`, `macros`, `schemars`, `transport-io`, later `transport-streamable-http-server` plus `axum`. MSRV 1.88, edition 2024 |

Toolchain: Rust 1.97 stable, edition 2024.

## Slug grammar

From the research note, corrected for the word-list model.

- Word: `[a-z][a-z0-9]*` or `[0-9]+`. Slug: one or more words, first word starts with a
  letter, serialised with single hyphens. Regex over the serialisation:
  `^[a-z][a-z0-9]*(-[a-z0-9]+)*$`.
- `ProjectSlug` max serialised length 32. Binding constraint: a single-service project uses
  the slug as its Railway service name, and Railway caps service names at 32. Every other
  documented cap is looser: Cargo 64, GitHub repo 100, Buildkite 100. Bundle IDs, Doppler
  names, and Android segments have no documented cap.
- `ComponentSlug` max 32, same reason.
- `EnvironmentSlug` max 16. This is a design margin, not a sourced number: Doppler documents
  no cap. Environment slugs are inputs, never derived, so relaxing the limit later changes no
  derived name and is not a naming-scheme version bump.
- Reserved words, matched case-insensitively against a single-word slug: Rust strict and
  reserved keywords, Java keywords, Kotlin hard keywords, Swift keywords, Windows device
  names (`nul`, `con`, `prn`, `aux`, `com1`..`com9`, `lpt1`..`lpt9`). Multi-word slugs
  cannot collide because every join keeps the separator or the case boundary.
- Every join is total on this grammar: kebab satisfies GitHub `[A-Za-z0-9._-]`, Buildkite
  `[a-zA-Z0-9][a-zA-Z0-9-]*`, Doppler's lowercase-hyphen convention, Cargo, Railway's DNS
  label use, and Apple's `[A-Za-z0-9.-]`; snake satisfies Android `[a-zA-Z0-9_]` with a
  leading letter and Rust identifiers; pascal satisfies Swift and Xcode module names.

## Type model

This section is normative for every crate.

- **Domain type.** A nominal scalar type implementing `DomainType` from `willikins-types`.
  Every domain type serializes as, and parses from, its canonical string form via
  `Display`/`FromStr`, regardless of internal storage (`String`, `WordList`, a struct, an
  enum, or `SecretString`). JSON schema is always `{"type": "string", ...}` with pattern,
  length, description, and example. Non-secret types implement `Serialize` through
  `Display`. Secret types implement no `Serialize` at all.
- **Secret types** store a `secrecy::SecretString`. `Debug` and `Display` print
  `[REDACTED <TypeName>]`. The value is reachable only through
  `expose(&self, &SinkToken) -> &str`. `SinkToken` is defined in `willikins-core` with a
  private constructor; it is created only by the apply executor (milestone 2) and, behind
  the `test-util` feature, by tests. `Tool::read` never receives one, so a `read`
  implementation provably cannot expose a secret.
- **Enum domain types** (`RepoVisibility`) are hand-written in milestone 1: a Rust enum
  whose canonical strings are its variants.
- **Structured identities** (`GitHubRepo { owner, name }`, `DopplerConfig { project, name }`)
  are hand-written domain types whose canonical string is a documented join
  (`owner/name`, `project/name`) and whose parse splits it.
- **Type reference.** A port or workflow input is typed by
  `TypeRef { name: TypeName, list: bool }`. `list<T>` is a cardinality flag on the port or
  input, not a separate domain type. A list is secret iff its element type is secret.
- **Port type.** `PortType::Exact(TypeRef)` or `PortType::AnySecret`. `AnySecret` accepts
  any secret scalar type and exists only for sinks such as `github.actions_secret.ensure`'s
  `value`. A non-secret value bound to an `AnySecret` port is a `TypeMismatch`.
- **Value.** `Value { ty: TypeRef, state: ValueState }` with
  `ValueState::{Unknown, Known(Known)}` and `Known::{Scalar(Arc<dyn DomainObject>),
  List(Vec<Arc<dyn DomainObject>>)}`. `DomainObject` is the object-safe view of a domain
  type: `type_name()`, `is_secret()`, `render() -> Rendered::{Plain(String), Redacted}`,
  `as_any()`. `Known` always holds the parsed domain-typed object, never a string snapshot.
  `Value`'s own `Debug` and `Serialize` go through `render()`, so a secret `Value` prints
  `[REDACTED <TypeName>]` in every container that derives `Debug` or `Serialize`. Cloning a
  `Value` clones the `Arc`.

## Workspace layout

```
Cargo.toml                 workspace, all dependency versions pinned here
LICENSE                    MIT
README.md
CLAUDE.md                  repo-specific agent guidance
crates/
  willikins-types/         domain types, naming scheme, slug proposal, type catalog
  willikins-derive/        #[derive(DomainType)] proc macro
  willikins-core/          values, tool contract, catalog, graph, checker, describe, plan
  willikins-dsl/           YAML document -> graph, JSON schema publication
  willikins-providers-fake/ in-memory GitHub and Doppler tools, naming.v1, test tools
  willikins-cli/           validate, describe, plan, schema, propose-slug
workflows/
  new-rust-service.yaml    the positive fixture
  fixtures/                negative and scenario fixtures used by tests
```

Dependencies flow downward only: cli -> dsl, providers-fake -> core -> types -> derive.

## Crate contracts

### willikins-types

- `Word`, `WordList` with joins `kebab`, `snake`, `screaming_snake`, `pascal`, `flat`.
  `ProjectSlug`, `ComponentSlug`, `EnvironmentSlug` over `WordList` per the Slug grammar.
- `ProjectName`: free-form display name, trimmed, non-empty, max 100, no control characters.
- Milestone 1 domain types, all non-secret unless marked:
  `GitHubOrg`, `GitHubRepo { owner: GitHubOrg, name: ProjectSlug }`, `RepoVisibility`
  (`private` | `public`), `HttpsUrl`, `ActionsSecretName` (`[A-Z_][A-Z0-9_]*`, not starting
  with `GITHUB_`), `DopplerProject`, `DopplerConfig { project, name }`, `DopplerTokenName`,
  `SecretName` (`[A-Z_][A-Z0-9_]*`), `Text` (free-form, max 64 KiB), `TemplateSource`
  (free-form, max 64 KiB), `DopplerServiceToken` (secret, `dp.st.` prefix),
  `DopplerSecretValue` (secret, any non-empty string).
  Buildkite, Railway, Apple, Android, Cargo, and Swift identity types are added by the
  milestone that adds their provider. The joins they need are already property-tested on
  `WordList`. Adding a row to `naming::v1` never changes an existing row, so it is not a
  scheme version bump.
- `DomainType` trait as scaffolded in `lib.rs`. `DomainObject` (object-safe view) and
  `Rendered` live here too so `Value` can hold any domain type.
- `naming::v1`: `github_repo(&GitHubOrg, &ProjectSlug) -> GitHubRepo`,
  `doppler_project(&ProjectSlug) -> DopplerProject`,
  `doppler_root_config(&DopplerProject, &EnvironmentSlug) -> DopplerConfig`.
  `NamingScheme::V1`. Pure, total, frozen.
- `propose_slug(&ProjectName) -> Result<ProjectSlug, ProposeError>`: NFKD, strip marks,
  ASCII lowercase, split on non-alphanumerics and case boundaries, join. Not on the
  idempotence path; may change between versions. Consumed by the `propose-slug` CLI
  subcommand in this milestone and by an MCP tool in milestone 2.
- `type_infos() -> Vec<TypeInfo>`: every domain type with name, secrecy, schema,
  description, example.

### willikins-derive

`#[derive(DomainType)]` on a newtype with one of three storages, chosen by the inner type:

- `String`: `#[domain(pattern = "...", min_len = N, max_len = N, description, example)]`.
  Generates `DomainType`, `FromStr`, `Display`, `Debug`, `Serialize` (via `Display`),
  `Deserialize` (via `parse`), `JsonSchema` (string with pattern and lengths).
- `WordList`: the same attributes plus `max_len` applying to the kebab serialisation.
  Serialize, Display, and schema use the kebab string, never the word array.
- `secrecy::SecretString`, required when `#[domain(secret, ...)]` is present: validation
  runs on the raw `&str` before wrapping. Generates `DomainType` with `IS_SECRET = true`,
  `FromStr`, redacted `Display` and `Debug`, `Deserialize` (via `parse`), `JsonSchema`,
  and `expose(&self, &SinkToken) -> &str`. Generates no `Serialize`.

Generated code reaches serde, schemars, and secrecy through `willikins_types::__private`
re-exports so users of the derive need no extra dependencies. Compile-fail tests with
`trybuild` live in `crates/willikins-types/tests/derive/`: `secret` on a `String` newtype,
`pattern` that is not a valid regex, a non-newtype struct, and calling `serde_json::to_string`
on a secret type.

### willikins-core

- `TypeRef`, `PortType`, `Value`, `ValueState`, `Known`, `SinkToken` per the Type model.
- `Class::{Reversible, Irreversible, Destructive}`, ordered. `requires_approval()` is true
  above `Reversible`.
- `ToolSpec { name, description, inputs: IndexMap<PortName, PortSpec>, outputs:
  IndexMap<PortName, TypeRef>, key: Vec<PortName>, class, pure: bool }`. `PortSpec { ty:
  PortType, required: bool }`. A pure tool has no external state: `read` computes its
  outputs, `ensure` is the identity, its plan action is `Compute`, and it never requires
  approval.
- `Tool` trait: `spec()`, `read(&Inputs) -> Result<Observation, ToolError>`,
  `ensure(&Inputs, &SinkToken) -> Result<Outputs, ToolError>` (unused until milestone 2).
  `Observation::{Absent, Present(Outputs), Foreign}`, where `Foreign` means the natural key
  exists but the resource is not ours. Implementations must not persist, log, or include a
  secret input in any `Observation`, `Outputs`, or `ToolError`; `ToolError` carries a
  message and a kind, never a value. `read` cannot expose secrets by construction.
- `Catalog`: tools by name plus `type_infos()`. Serializable to JSON for `list_tools`.
- `Workflow { name, description, inputs: IndexMap<InputName, InputSpec>, nodes:
  IndexMap<NodeName, Node>, outputs: IndexMap<OutputName, Binding> }`.
  `InputSpec { ty: TypeRef, default: Option<Value>, description }`.
  `Node { tool, for_each: Option<Binding>, with: IndexMap<PortName, Binding> }`.
  `Binding::{Input(InputName), Step { node, port }, Keyed { node, key, port }, Item,
  Literal(String)}`. `Item` is valid only inside a `for_each` node and has the element type
  of the source. `Step` on a `for_each` node yields `list<port type>` (every instance);
  `Keyed` yields the scalar port type and resolves at plan time. Edges are derived from
  `Step`, `Keyed`, and `for_each` bindings.
- `check(&Workflow, &Catalog) -> Result<Checked, Vec<CheckError>>`. Variants, each with the
  fields the tests assert on:
  `UnknownTool { node, tool }`, `UnknownPort { node, tool, port }`,
  `UnknownNode { node, port, referenced }`, `UnboundInput { node, port }`,
  `UndeclaredInput { node, port, input }`, `InvalidLiteral { node, port, error }`,
  `TypeMismatch { node, port, expected: PortType, found: TypeRef }`,
  `SecretToNonSecretSink { from: (NodeName, PortName), to: (NodeName, PortName) }`,
  `SecretWorkflowInput { input, ty }`, `SecretForEachSource { node }`,
  `ForEachOverScalar { node }`, `ItemOutsideForEach { node, port }`,
  `KeyedOnScalarNode { node, port, referenced }`, `Cycle { nodes }`,
  `DuplicateNode { node }`. Warnings, returned alongside a successful check:
  `UnusedInput { input }`. `Checked` carries the workflow, a topological order, and
  `class: Class` (max over nodes, pure nodes excluded).
- `describe(&Checked, &PartialInputs) -> Description { errors: Vec<InputError>, missing:
  Vec<MissingInput>, resolved: Inputs }` with no provider calls. `MissingInput { name, ty,
  schema, description, default, example, prompt }`. Inputs with defaults are never
  missing.
- `plan(&Checked, &Inputs, &Catalog) -> Result<Plan, PlanError>`. Runs `read` per node in
  topological order, expanding a `for_each` node into one instance per item keyed by the
  item's canonical string, and propagating `Unknown` through outputs.
  `Plan { nodes: Vec<PlannedNode>, class, requires_approval }`, `PlannedNode { name,
  instance: Option<String>, tool, action: Action::{Compute, Create, NoOp}, inputs: Inputs,
  outputs: Outputs }`. `PlanError::{NameTaken { node, tool, key: Inputs }, KeyNotInForEach {
  node, key }, Tool { node, error }, MissingInput { input }}`. `Foreign` from `read`
  becomes `NameTaken`; it never appears inside a returned `Plan`. Plan output is redacted by
  construction because every value inside it is a `Value`.

### willikins-dsl

- YAML document -> `Workflow`. References are the only expression form. Format:

```yaml
name: new-rust-service
description: Provision a GitHub repository and Doppler project for a Rust service.
inputs:
  slug: { type: ProjectSlug, description: Canonical project slug }
  org: { type: GitHubOrg }
  visibility: { type: RepoVisibility, default: private }
  environments: { type: list<EnvironmentSlug>, default: [dev, stg, prd] }
steps:
  names:
    tool: naming.v1
    with: { org: ${{ inputs.org }}, slug: ${{ inputs.slug }} }
  repo:
    tool: github.repo.ensure
    with: { repo: ${{ steps.names.github_repo }}, visibility: ${{ inputs.visibility }} }
  doppler:
    tool: doppler.project.ensure
    with: { project: ${{ steps.names.doppler_project }} }
  configs:
    tool: doppler.config.ensure
    for_each: ${{ inputs.environments }}
    with: { project: ${{ steps.doppler.project }}, environment: ${{ item }} }
  token:
    tool: doppler.service_token.ensure
    with: { config: ${{ steps.configs[prd].config }}, name: ci }
  ci_secret:
    tool: github.actions_secret.ensure
    with: { repo: ${{ steps.repo.repo }}, name: DOPPLER_TOKEN, value: ${{ steps.token.token }} }
outputs:
  repo_url: ${{ steps.repo.url }}
```

- Reference forms: `${{ inputs.<name> }}`, `${{ steps.<node>.<port> }}`,
  `${{ steps.<node>[<key>].<port> }}`, `${{ item }}`. Anything else in a `with` value is a
  literal string, parsed against the port type at check time. A `with` value that is not a
  string (YAML list or map) is a document error.
- Input types: `<TypeName>` or `list<TypeName>`. Defaults are parsed against the declared
  type at document load.
- Errors carry line and column from `serde_yaml_ng::Error::location()`. The document JSON
  schema is generated with schemars from the document structs and published by the CLI.

### willikins-providers-fake

In-memory state, seedable from JSON so tests can stage "already exists", "foreign", and
known secret values. Every tool's ports are listed here; tasks 9 and 10 build against this
table, not against the fixture prose.

| Tool | Inputs | Outputs | Key | Class | Notes |
| --- | --- | --- | --- | --- | --- |
| `naming.v1` | `org: GitHubOrg`, `slug: ProjectSlug` | `github_repo: GitHubRepo`, `doppler_project: DopplerProject` | none | pure | wraps `naming::v1` |
| `github.repo.ensure` | `repo: GitHubRepo`, `visibility: RepoVisibility` | `repo: GitHubRepo`, `url: HttpsUrl` | `repo` | Reversible | `url` is `https://github.com/<owner>/<name>` |
| `github.actions_secret.ensure` | `repo: GitHubRepo`, `name: ActionsSecretName`, `value: AnySecret` | none | `repo`, `name` | Reversible | `read` checks existence by key only |
| `doppler.project.ensure` | `project: DopplerProject` | `project: DopplerProject` | `project` | Reversible | |
| `doppler.config.ensure` | `project: DopplerProject`, `environment: EnvironmentSlug` | `config: DopplerConfig` | `project`, `environment` | Reversible | root config named after the environment |
| `doppler.service_token.ensure` | `config: DopplerConfig`, `name: DopplerTokenName` | `token: DopplerServiceToken` (secret) | `config`, `name` | Reversible | `Present` yields `token` as `Unknown`: values cannot be re-read |
| `doppler.secret.get` | `config: DopplerConfig`, `name: SecretName` | `value: DopplerSecretValue` (secret) | `config`, `name` | pure | `read` returns the seeded value as `Known`; the redaction proof path |
| `fake.secret_list` | `config: DopplerConfig` | `tokens: list<DopplerServiceToken>` (secret) | none | pure | test tool for `SecretForEachSource` |
| `fake.irreversible.ensure` | `key: ProjectSlug` | none | `key` | Irreversible | test tool for approval class |
| `template.render` | `template: TemplateSource`, `value: Text` | `rendered: Text` | none | pure | the non-secret sink for the taint proof; replaces `{{ value }}` only |

### willikins-cli

`willikins validate <file>`, `willikins describe <file> [--input k=v]...`,
`willikins plan <file> [--input k=v]... [--fake-state <json>]`,
`willikins schema (--document | --catalog)`, `willikins propose-slug <name>`.
List inputs are comma-separated. Output is JSON with `--json`, human text otherwise. Text
rendering of any value goes through `Value`'s `Display`, never a bespoke formatter, so the
redaction guarantee holds for both output modes. The first four subcommands are the
milestone 2 MCP tools one to one.

## Acceptance tests

The milestone cannot ship without every one of these.

1. **Taint rejection.** `workflows/fixtures/secret-into-template.yaml` binds
   `steps.token.token` to `template.render`'s `value`. `check` returns exactly one error,
   `SecretToNonSecretSink { from: (token, token), to: (readme, value) }`.
2. **Secret workflow input.** A workflow declaring an input of type `DopplerServiceToken`
   fails `check` with `SecretWorkflowInput`.
3. **Static errors.** One fixture each for `UnboundInput`, `UndeclaredInput`,
   `InvalidLiteral` (`visibility: internal`), `TypeMismatch` (`inputs.slug` bound to
   `github.repo.ensure`'s `repo`), `UnknownTool`, `UnknownPort`, `Cycle`. Each error names
   the node and port.
4. **for_each.** `SecretForEachSource` for a `for_each` over `fake.secret_list`'s `tokens`;
   `ForEachOverScalar` for a `for_each` over a scalar input; `ItemOutsideForEach`;
   `KeyedOnScalarNode`. On the positive fixture, `steps.configs[prd].config` type-checks
   as `DopplerConfig` and `steps.configs.config` as `list<DopplerConfig>`.
5. **Describe loop.** `describe` on the positive fixture with no inputs lists `slug` and
   `org` as missing with prompts, and neither `visibility` nor `environments` because they
   have defaults. With `slug=third-thoughts org=lightless-labs` it lists nothing and resolves
   every input into domain types, including the default list.
6. **Plan against empty state.** `names` is `Compute`; `repo`, `doppler`, `token`,
   `ci_secret` are `Create`; `configs` expands to three instances keyed `dev`, `stg`,
   `prd`, all `Create`; `token`'s output is `Unknown`; `ci_secret`'s `value` is `Unknown`;
   `repo`'s `url` is `https://github.com/lightless-labs/third-thoughts`; `class` is
   `Reversible` and `requires_approval` is false. A second fixture adding
   `fake.irreversible.ensure` yields `Irreversible` and `requires_approval` true.
7. **Plan against seeded state.** Repo exists and is ours: `NoOp`, and its `url` is `Known`.
   Repo exists and is foreign: `plan` returns `NameTaken { node: repo, tool:
   github.repo.ensure, key }`. `environments=[dev, qa]` with `configs[prd]` referenced:
   `KeyNotInForEach { node: token, key: prd }`.
8. **Redaction by construction.** Two tests, neither depending on empty state:
   a. Construct `Value::known(DopplerServiceToken::parse("dp.st.fake-secret-bytes"))`
      directly, embed it in `Inputs`, `Outputs`, `Observation::Present`, `PlannedNode`,
      `Plan`, and a `ToolError`, and assert the literal bytes appear in none of
      `serde_json::to_string`, `format!("{:?}")`, `to_string()`, or the CLI text renderer.
   b. A fixture using `doppler.secret.get` with the value seeded: the resulting `Plan`
      holds a `Known` secret, and its JSON and CLI text output contain
      `[REDACTED DopplerSecretValue]` and not the seeded bytes.
9. **Naming.** Golden tests for every `naming::v1` function against the design doc's join
   table, and property tests that every valid `ProjectSlug` derives successfully for every
   target and every result parses as its target type. `WordList` joins are property-tested
   for their character classes, and snake and pascal round-trip to the word list.
10. **Reserved words.** `native`, `default`, `type`, `match`, `self`, `nul` are rejected as
    project slugs; `type-system` and `self-hosted` are accepted.
11. **Compile-time guarantees.** `trybuild`: `serde_json::to_string(&token)` does not
    compile; constructing a `SinkToken` outside `willikins-core` without the `test-util`
    feature does not compile; `#[domain(secret)]` on a `String` newtype does not compile.
12. **Adversarial passes.** Two, recorded in `docs/research/`: one on `check` immediately
    after task 7, one end to end after task 12. Each attempts to construct a workflow, a
    value path, a fake-state file, or a CLI invocation that leaks a secret past `check` or
    into any output. The milestone is done only when every found bypass is fixed and has
    become a fixture.

## Gates

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`. `unsafe_code = "forbid"` and pedantic clippy are set at the
workspace level. Run after every task, before every commit.

## Tasks

Dependency order, re-sequenced so that every task compiles against finished dependencies.
Tasks in the same parallel group touch disjoint files and run in separate worktrees; the
coordinator merges.

| # | Task | Depends on | Group | Delegate to |
| --- | --- | --- | --- | --- |
| 1 | Scaffold: manifests, crate stubs, `DomainType` trait, LICENSE, README, CLAUDE.md, fixtures | research | | inline. Done. |
| 2 | `Word`, `WordList`, slug types, reserved words, `ProjectName`, `propose_slug` | 1 | A | sonnet, verified by opus. In progress. |
| 3 | `willikins-derive` for `String`, `WordList`, `SecretString` storages; `__private` re-exports; trybuild suite in `willikins-types/tests/derive/` | 1 | A | sonnet, verified by opus |
| 4 | Milestone 1 domain types, `DomainObject`, `Rendered`, `type_infos()`; redaction unit tests | 2, 3 | | sonnet |
| 5 | `naming::v1` with golden and property tests | 4 | | sonnet |
| 6 | Core: `TypeRef`, `PortType`, `Value`, `SinkToken`, `Class`, `ToolSpec`, `Tool`, `Observation`, `Catalog`; acceptance test 8a | 4, 5 | | sonnet, verified by opus |
| 7 | Core: `Workflow`, `Binding`, `check` with every variant, `Checked`; fixture-driven tests | 6 | B | sonnet, verified by opus |
| 7b | Adversarial pass on `check` (acceptance test 12, first pass) | 7 | | opus |
| 9 | Fake providers per the port table, seedable state | 6 | B | sonnet |
| 8 | Core: `describe`, `plan` with `for_each` expansion, `Unknown` propagation, `PlanError` | 7b, 9 | C | sonnet, verified by opus |
| 10 | DSL: parser, references, `list<T>`, defaults, document JSON schema, line and column errors | 7b | C | sonnet |
| 11 | CLI over dsl, core, fakes; text rendering through `Value` | 8, 9, 10 | | sonnet |
| 12 | All fixtures and acceptance tests 1 through 11 as integration tests in the CLI crate | 11 | | sonnet |
| 13 | Adversarial pass end to end (acceptance test 12, second pass); fix and fixture every bypass | 12 | | opus |

Groups: A = {2, 3}; B = {7, 9}; C = {8, 10}. Everything else is sequential on `main`.

## Risks

- **The derive macro is the widest surface.** If it slips, hand-write the impls for
  milestone 1 and keep the proc macro as a follow-up. The type contract does not depend on
  how the impls are produced.
- **`for_each` at plan time.** Expansion happens after inputs are known, so `check` can only
  verify shapes. The `KeyNotInForEach` plan error and test 7 cover the gap.
- **Machine load.** Builds on this host have been slow in this session. Every cargo
  invocation in agents runs in the background with a generous timeout.

## Review resolutions

How each review finding was resolved, by number in the merged findings list.

1. Type model section added: `list<T>` is a cardinality flag, enums are hand-written,
   `when` and bools are out of scope.
2. Derive contract now has a `SecretString` storage with validation before wrapping.
3. Tasks re-sequenced: 6 depends on 4 and 5; 9 follows 6; groups verified.
4. Acceptance test 8 rewritten as a direct-construction test plus a seeded known-secret
   plan; test 6 no longer claims to prove redaction.
5. Out-of-scope section states that `template.render` and `fake.irreversible.ensure` are in.
6. `Binding::Org` and `org.<key>` removed from milestone 1.
7. `naming.v1` is a pure tool in the graph; fake tool ports are typed with identity types;
   identity types scoped to GitHub and Doppler for milestone 1.
8. `plan` returns `Result<Plan, PlanError>`; every error variant named with fields;
   `Action::Foreign` dropped.
9. Type model states canonical-string serialization for every storage.
10. One mechanism: secret types implement no `Serialize`; `expose` needs a `SinkToken`.
11. `Value` representation specified.
12. `EnvironmentSlug` 16 marked as a design margin with the reason it is safe to change.
13. Layer gating removed from the milestone 1 `Workflow`.
14. `for_each` semantics specified: `Keyed` binding, plan-time expansion, test 4 and 7.
15. CLI text rendering goes through `Value`'s `Display`; test 8 covers text and JSON.
16. `SecretForEachSource` added with a fixture and the `fake.secret_list` tool.
17. Identity types and `naming::v1` scoped to milestone 1 providers with the reason.
18. Adversarial passes are acceptance test 12 and appear twice in the task order.
19. Ledger cut from milestone 1.
20. `when` cut from milestone 1.
21. `propose-slug` CLI subcommand added as the consumer; tests already in task 2.
22. Port table added for every fake tool.
23. `Tool` contract forbids retaining or echoing secrets; `read` cannot expose by
    construction.
