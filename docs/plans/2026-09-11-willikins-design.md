# Willikins: design

**Created:** 2026-09-11 (design conversation)

Willikins is an open-source provisioning butler. An agent, over MCP or the CLI, authors and
runs reusable, composable project-provisioning workflows against GitHub, Doppler, Buildkite,
Railway, and App Store Connect, and seeds new repos from templates. The agent never touches
a secret. The agent plans; the butler acts.

## Decisions

### Trust model

- Remote-first. Willikins is an MCP server over Streamable HTTP running on its own host. The
  trust boundary is a network boundary, so the agent's shell access on its own machine is
  irrelevant to the butler's credentials. The same binary offers stdio as a local mode.
- The agent receives handles and identifiers only, never secret values.
- Redaction is applied at the boundary to plan output, run logs, and provider error bodies.
- Provider credentials are held server-side and are never workflow inputs. Provider auth is an
  implicit execution context, not a parameter.
- Workflow definitions and templates are privileged content. A rendered CLAUDE.md is an
  instruction set every future agent in that repo will obey. Changes go through git and a
  human-approved diff, and the butler runs only from a trusted ref.
- The agent's credential to reach the butler must have a boring blast radius: propose runs that
  a human must approve, plus execute the auto-approved reversible set. Nothing more.
- The service needs its own authentication, TLS, and an append-only audit log from day one.

### Type system

- Parse, don't validate. Every value crossing a tool boundary is a nominal domain type. A bare
  string never appears in a tool signature. `DopplerConfigName`, `BuildkitePipelineSlug`,
  `BundleId`, `GitHubRepo { owner, name }`, not `String` or `Url`.
- Secrecy is a property of the type, not a wrapper. `GitHubToken` is secret by definition.
  Debug and Display print a redacted marker. Serialization is permitted only into sink
  contexts. The taint check reads the property off the type.
- Format validation is pure and happens at parse time. Liveness and scope validation is a
  butler-side operation on the type that verifies against the provider without exposing the
  value.
- Verified credentials carry their capabilities. A tool declares the permissions it needs, and
  the planner rejects a plan before it runs rather than surfacing a 403 halfway through.
- Resource identities are types, and they are the natural keys. Idempotence and typing are the
  same mechanism.
- A derive macro per provider crate gives each type its format rule, secrecy property,
  redaction, and JSON schema in one place. The type catalog is published over MCP so the
  agent authors against named types with known constraints.

### Tool contract

- Typed inputs and typed outputs, drawn from the type catalog.
- A natural key, an `ensure` semantic, and a `read` that observes current state. `read`
  before write lets the plan report "already exists, no-op".
- A declared reversibility class: reversible, irreversible, or destructive. The class is
  static, so approval gating is computed from the graph before anything runs.
- No tool may reduce to "call an API with my credentials" or "run a command". The step set is
  closed. An open step would make the isolation theatre: a prompt-injected agent could mint a
  token and ship it anywhere.
- A secret output may only flow to a secret-accepting input. No coercion. Template rendering
  accepts no secrets at all.

### Workflow DSL

- Declarative, non-Turing-complete, schema-validated document. Edges are derived from data
  flow, not wired explicitly, so ordering and parallelism fall out of the graph.
- Control flow is limited to `when` guards and `for_each` over lists.
- Workflows are tools. Same typed interface, so a composite is a node in a larger graph.
  Primitives are Rust, composites are DSL, one abstraction.
- The document JSON schema and the tool catalog are published over MCP so an agent can
  validate a workflow locally before submitting it.

### Execution

- Plan then apply. Plan is a pure function of workflow, inputs, and observed state, which
  makes it testable offline against fake providers.
- Values unknown until apply are represented explicitly and propagate. Most outputs derive
  from natural keys, so plans stay concrete.
- Idempotence via natural-key lookup rather than a state file. State files are where secrets
  end up on disk.
- A per-node run ledger. Because every tool is idempotent, re-running a partially failed plan
  converges instead of duplicating.
- Approval is out-of-band from the agent session: plan goes pending, a human approves through
  a notification or a small page, the agent polls or is notified.
- Scope is deliberately narrow. No triggers, no long-running steps, no loops beyond
  `for_each`. This is a provisioning planner, not a general workflow engine.

### Templates

- Template rendering is an ordinary typed step that consumes non-secret outputs from earlier
  steps. Provisioning and templating live in one graph because they reference each other:
  the Buildkite YAML needs the pipeline slug, CLAUDE.md names the Doppler project, README
  badges need the repo.
- The type system guarantees a secret can never be rendered into a committed file.
- The second run is the feature. Record template version and answers in the repo so a later
  run can re-render, three-way merge, and open a PR per repo when an org convention changes.
- Profiles compose as layers: base, plus rust-crate or ios-app, plus open-source or private.
  Each layer contributes files and steps. Habits such as branch protection, required checks,
  CODEOWNERS, PR templates, and Renovate config straddle files and API settings, which is why
  one profile owns both.
- Org conventions may already live in the sibling foundry repo. Templates should reference
  them rather than duplicate them.

### Hosting

- Nothing requires macOS. The App Store Connect API is REST and CSR generation is plain
  crypto. Signing stays in CI on Tart runners, with the butler delivering certificates and
  profiles through Doppler or a match-style store.
- Candidate hosts: a small always-on box, or Railway with the mild circularity of
  provisioning Railway from Railway.

### Scope order

1. Broker, GitHub, Doppler, and one end-to-end workflow that proves the typed-secret flow and
   the approval gate.
2. Buildkite and Railway.
3. App Store Connect last. Certificates and private keys are exactly the secrets in question,
   and parts of that flow are not API-reachable.

### Prior art

- Backstage scaffolder: nearest concept, not agent-first, secrets in its backend.
- Dagger's Secret type: nearest secret model.
- OpenTofu providers for GitHub, Doppler, Buildkite: rejected as the engine. Provisioning
  needs perhaps ten operations per provider, wrapping inherits state-file secrets, and there
  is no App Store Connect provider.
- fastlane: covers App Store Connect and is the reference for certificate handling.
- copier: reference for versioned template re-apply.

## To verify before relying on them

These came up from memory during the conversation and have not been checked.

- Current MCP authorization spec for HTTP transports and whether it mandates OAuth 2.1.
- Maturity of Rust CEL implementations, if CEL is chosen as the expression language.
- Fit of the `secrecy` crate as the underlying redaction primitive.
- Exact token prefix formats for GitHub, Doppler, and Buildkite.
- Whether a maintained Railway API client or provider exists.
- Whether App Store Connect exposes every step needed for app, bundle ID, certificate, and
  profile creation.

## Open questions

- DSL surface syntax: YAML, JSON, KDL, or CUE.
- Expression language for interpolation and `when`: CEL or a smaller custom one.
- Hosting target for the first deployment.
- Approval channel: push notification, web page, chat.
- Server-side credential storage: OS keychain, 1Password service account, Doppler as vault.
- License.
- Build: the monorepo standard is Bazel, sibling public repos use Cargo directly.
