# Milestone 1: the core, with no real providers

**Created:** 2026-09-11
**Design:** `docs/plans/2026-09-11-willikins-design.md`
**Research:** `docs/research/2026-09-11-m1-dependencies.md`

## Goal

A Rust workspace in which a YAML workflow is parsed into a typed graph, statically checked,
described to an agent, and planned against fake providers. Nothing talks to a real API. The
milestone is done when the checker provably rejects a workflow that routes a secret into a
template, and `plan` produces a concrete, approval-classified plan for the project-creation
fixture.

## Out of scope

Real providers, `apply`, persistence of the ledger, the MCP server, templates, authentication,
hosting. Those are milestones 2 and later. The MCP surface is designed here only as far as the
CLI mirrors it, so that milestone 2 is a transport, not a redesign.

## Workspace layout

```
Cargo.toml                 workspace, all dependency versions pinned here
LICENSE                    MIT
README.md
CLAUDE.md                  repo-specific agent guidance
crates/
  willikins-types/         domain types, naming scheme, slug proposal
  willikins-derive/        #[derive(DomainType)] proc macro
  willikins-core/          values, tool contract, catalog, graph, checker, describe, plan, ledger
  willikins-dsl/           YAML document -> graph, JSON schema publication
  willikins-providers-fake/ in-memory GitHub and Doppler tools
  willikins-cli/           validate, describe, plan, schema
workflows/
  new-rust-service.yaml    the positive fixture
  fixtures/                negative fixtures used by tests
```

Dependencies flow downward only: cli -> dsl, providers-fake -> core -> types -> derive.

## Crate contracts

### willikins-types

- `Word`: `[a-z][a-z0-9]*` or `[0-9]+`. `WordList`: non-empty, first word starts with a
  letter. `ProjectSlug`, `ComponentSlug`, `EnvironmentSlug` wrap a `WordList` with the
  intersection constraints from the research note (max length, reserved words: Rust keywords,
  Java keywords, provider-reserved names).
- `ProjectName`: free-form display name, non-empty, trimmed.
- Org configuration types: `GitHubOrg`, `Domain`, `ReverseDnsPrefix` (derived from `Domain`,
  fallback from `GitHubOrg`), `DopplerWorkspace`, `BuildkiteOrg`.
- Resource identity types: `GitHubRepo { owner, name }`, `DopplerProject`, `DopplerConfig`,
  `BuildkitePipeline`, `RailwayProject`, `RailwayService`, `BundleId`, `AndroidApplicationId`,
  `CargoPackageName`, `RustLibName`, `SwiftModuleName`, `EnvPrefix`.
- Credential types, secret by definition: `GitHubToken`, `DopplerServiceToken`,
  `BuildkiteApiToken`. Debug and Display print `[REDACTED <TypeName>]`. Serialize is only
  possible through an explicit `expose_for_sink` path, never through plain serde.
- `DomainType` trait: `parse(&str) -> Result<Self, ParseError>`, `type_name() -> &'static str`,
  `is_secret() -> bool`, `json_schema() -> schemars::Schema`, `description()`, `example()`.
- `naming::v1`: pure functions `(org, slug, component?, environment?) -> <target type>` for
  every row of the design doc's join table. `NamingScheme` enum with `V1` only.
- `propose_slug(&ProjectName) -> Result<ProjectSlug, ProposeError>`: NFKD, strip marks,
  ASCII lowercase, split on non-alphanumerics and case boundaries, join. Not on the
  idempotence path; may change between versions.
- `TypeCatalog`: the list of every domain type with name, secrecy, schema, description,
  example. Published as JSON.

### willikins-derive

`#[derive(DomainType)]` on a newtype over `String` or `WordList`, with
`#[domain(pattern = "...", max_len = N, min_len = N, secret, description = "...", example = "...")]`.
Generates `DomainType`, `FromStr`, `Display` (redacted when secret), `Debug` (redacted when
secret), `Serialize`/`Deserialize` (deserialize always parses; serialize is a compile error
for secret types unless the sink marker is used), and `JsonSchema`. Tested with `trybuild`
for the compile-fail cases.

### willikins-core

- `Value`: a typed runtime value carrying its domain type name, its secrecy, and one of
  `Known(...)` or `Unknown` (unknown until apply). Secrecy is read from the type, never set
  by hand.
- `ToolSpec`: name, description, typed input ports and output ports (each a domain type name
  and secrecy), natural key ports, `Class::{Reversible, Irreversible, Destructive}`, required
  credential capabilities.
- `Tool` trait: `spec()`, `read(inputs) -> Observation::{Absent, Present(outputs), Foreign}`
  where `Foreign` means "exists but is not ours" (name taken), `ensure(inputs) -> outputs`
  (unused until milestone 2 but part of the contract).
- `Catalog`: tools by name plus the type catalog. Serializable to JSON for MCP `list_tools`.
- `Workflow`: declared inputs (typed, optional with default, gated by layer), nodes (tool
  name, input bindings), outputs. `Binding::{Input(name), Step(node, port), Literal(Value),
  Org(key)}`. Edges are derived from `Step` bindings. Workflows implement `Tool` so they
  compose.
- `check(&Workflow, &Catalog) -> Result<Checked, Vec<CheckError>>`. Errors: unknown tool,
  unknown port, unbound input, undeclared workflow input, type mismatch, secret-to-non-secret
  flow, cycle, secret-typed workflow input, unused declared input (warning). `Checked` carries
  a topological order and the approval class of the whole graph (max over nodes).
- `describe(&Checked, partial_inputs) -> Description { errors, missing: Vec<MissingInput>,
  resolved }` with no provider calls. Each `MissingInput` has name, type, schema, description,
  default, example, prompt.
- `plan(&Checked, inputs, &Catalog) -> Plan`: runs `read` per node in topological order,
  propagates `Unknown`, produces `Action::{Create, NoOp, Foreign}` per node, and fails the
  plan with a typed "name taken, provide override" error on `Foreign`. Plan output is
  redacted by construction because secret `Value`s render as `[REDACTED]`.
- `Ledger` trait with an in-memory implementation recording plans and per-node status.

### willikins-dsl

- YAML document -> `Workflow`. Format:

```yaml
name: new-rust-service
inputs:
  display_name: { type: ProjectName }
  slug: { type: ProjectSlug }
  org: { type: GitHubOrg }
  visibility: { type: RepoVisibility, default: private }
  environments: { type: list<EnvironmentSlug>, default: [dev, stg, prd] }
steps:
  repo:
    tool: github.repo.ensure
    with: { org: ${{ inputs.org }}, name: ${{ inputs.slug }}, visibility: ${{ inputs.visibility }} }
  doppler:
    tool: doppler.project.ensure
    with: { name: ${{ inputs.slug }} }
  configs:
    tool: doppler.config.ensure
    for_each: ${{ inputs.environments }}
    with: { project: ${{ steps.doppler.project }}, name: ${{ item }} }
  token:
    tool: doppler.service_token.ensure
    with: { config: ${{ steps.configs[prd].config }} }
  ci_secret:
    tool: github.actions_secret.ensure
    with: { repo: ${{ steps.repo.repo }}, name: DOPPLER_TOKEN, value: ${{ steps.token.token }} }
outputs:
  repo_url: ${{ steps.repo.url }}
```

- References are the only expression form: `inputs.<name>`, `steps.<node>.<port>`,
  `steps.<node>[<key>].<port>` for `for_each` results, `item`, `org.<key>`. No operators.
- `when: ${{ inputs.<bool> }}` and `for_each` over a list input or list output.
- Errors carry line and column. The document JSON schema is generated and published.

### willikins-providers-fake

`github.repo.ensure`, `github.actions_secret.ensure`, `doppler.project.ensure`,
`doppler.config.ensure`, `doppler.service_token.ensure`, `template.render` (accepts only
non-secret inputs). In-memory state, seedable so tests can stage "already exists" and
"foreign" scenarios. `doppler.service_token.ensure` returns a secret `DopplerServiceToken`.

### willikins-cli

`willikins validate <file>`, `willikins describe <file> [--input k=v]...`,
`willikins plan <file> --input k=v... [--fake-state <json>]`, `willikins schema [--document | --catalog]`.
Output is JSON with `--json`, human text otherwise. The four commands are the milestone 2 MCP
tools one to one.

## Acceptance tests

These are the tests the milestone cannot ship without.

1. **Taint rejection.** `workflows/fixtures/secret-into-template.yaml` binds
   `steps.token.token` to `template.render`. `check` returns exactly one
   `SecretToNonSecretSink` error naming both ports. This is the proof of the central claim.
2. **Secret workflow input rejection.** A workflow declaring an input of type
   `DopplerServiceToken` fails `check`.
3. **Unbound and undeclared inputs.** Fixtures for each, with the error naming the node and
   port.
4. **Cycle.** A two-node cycle fails `check`.
5. **Describe loop.** `describe` on the positive fixture with no inputs lists every required
   input with prompts; with all inputs it lists none and resolves them into domain types.
6. **Plan against empty state.** Every node is `Create`, `token` output is `Unknown`, the
   plan's approval class is the max over nodes, no secret value appears anywhere in the
   serialized plan (assert by searching the JSON for the fake token's bytes).
7. **Plan against seeded state.** Repo exists and is ours: `NoOp`. Repo exists and is
   foreign: plan fails with `NameTaken { tool, key }`.
8. **Redaction.** `format!("{:?}", token)` and `to_string()` never contain the value.
   Serializing a secret through plain serde does not compile (`trybuild`).
9. **Naming.** Property test: every valid `ProjectSlug` derives successfully for every target
   and every result parses as its target type. Snake and pascal round-trip to the word list.
   Golden tests for the join table examples in the design doc.
10. **Reserved words.** `native`, `default`, `type`, `match` are rejected as project slugs.

## Gates

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`,
`#![forbid(unsafe_code)]` in every crate. Run after every task, before every commit.

## Tasks

Dependency order. Tasks on the same line are independent and may run in parallel in separate
worktrees; everything else is sequential on `main`.

| # | Task | Depends on | Delegate to |
| --- | --- | --- | --- |
| 1 | Scaffold workspace: manifests with pinned versions, crate stubs, LICENSE, README, CLAUDE.md, `DomainType` trait signature, gates passing on empty crates | research note | inline |
| 2 | `Word`, `WordList`, slug types with grammar and reserved words, `propose_slug`, property tests | 1 | sonnet |
| 3 | `willikins-derive` proc macro with trybuild tests | 1 | sonnet |
| 4 | Org, resource identity, and credential types using the derive; redaction tests | 2, 3 | sonnet |
| 5 | `naming::v1` and `TypeCatalog`; golden and property tests | 4 | sonnet |
| 6 | `Value`, `ToolSpec`, `Tool`, `Catalog` | 4 | sonnet |
| 7 | `Workflow`, bindings, `check` with every error kind; fixture-driven tests | 6 | sonnet, verified by opus |
| 8 | `describe` and `plan` with `Unknown` propagation and `Foreign` handling; in-memory ledger | 7 | sonnet, verified by opus |
| 9 | Fake providers with seedable state | 6 | sonnet |
| 10 | DSL parser, reference resolution, `for_each`, `when`, document JSON schema, line/column errors | 7 | sonnet |
| 11 | CLI over dsl, core, fakes | 8, 9, 10 | sonnet |
| 12 | Fixtures and end-to-end acceptance tests 1 through 10 | 11 | sonnet |
| 13 | Adversarial verification of the checker and redaction: agents try to construct a workflow or value path that leaks a secret past `check` or into plan output | 12 | opus |

Tasks 2 and 3 run in parallel; 5, 6, 9 run in parallel; 8 and 10 run in parallel.

## Risks

- **The derive macro is the widest surface.** If it slips, fall back to a declarative macro
  or hand-written impls for milestone 1 and keep the proc macro as a follow-up. The type
  contract does not depend on how the impls are produced.
- **Secrecy through serde.** Making serialize a compile error for secret types while keeping
  deserialize needs care. If the compile-time route proves brittle, the runtime route is a
  serializer wrapper that panics on secret types outside a sink context, covered by test 6.
- **Machine load.** Builds on this host have been slow in this session. Every cargo
  invocation in agents runs in the background with a generous timeout.
