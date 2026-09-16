# Willikins: design

**Created:** 2026-09-11 (design conversation)
**Addendum:** 2026-09-11 — added the Naming section: slug canonicalisation, frozen derivation, overrides, scheme versioning.
**Addendum:** 2026-09-11 — added Workflow inputs: explicit checked signatures, decisions-only inputs, no secret inputs, describe as a pure resolution tool.
**Addendum:** 2026-09-11 — open questions decided (YAML, typed refs, Railway, web approval, Doppler vault, MIT, Cargo); milestone list added.
**Addendum:** 2026-09-11 — after dependency research: Railway service name is the component alone; Swift keywords join the reserved-word union.
**Addendum:** 2026-09-11 — task 2 verification: pascal is not injective for digit-only words; accepted, since pascal never feeds a natural key.
**Addendum:** 2026-09-14 — license changed from MIT to AGPL-3.0-or-later at the operator's request when the public repository (github.com/Lightless-Labs/willikins) was created; the decisions table row updated. CI decision recorded for milestone 3: every secret lives in Doppler and Buildkite holds one CI/CD Doppler service-account token, so no per-repository secret is ever pushed into CI; willikins must be able to provision the Buildkite pipeline when a workflow asks for it, so the Buildkite provider moves from milestone 4 to milestone 3 in the list below.
**Addendum:** 2026-09-15 — milestone 2c added to the list below: OAuth 2.1 on the MCP transport and a browser login on the approvals page, scheduled right after milestone 2 because static bearer tokens are sandbox-grade and the service gets no public domain until then.
**Addendum:** 2026-09-12 — milestone 2 plan: two kinds of secret (graph secrets behind `SinkToken`, execution-context credentials behind one `authorize` function and a clippy entry); TLS terminated at the platform edge; the remote server plans and applies by workflow name only; a tool refuses rather than reconciles a non-key attribute it should not change; composition split out of milestone 2 into its own plan. See "Milestone 2 decisions".

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

### Workflow inputs

A workflow must declare its inputs so an agent can gather them up front and call `plan`
once with a complete, typed set.

- **Explicit signature, statically checked against the graph.** Every tool input in the
  graph is bound to one of: an upstream output, a literal, org configuration, or a declared
  workflow input. The checker rejects a workflow with an unbound input, and warns on a
  declared input nothing consumes. Inputs are not inferred from free variables: editing the
  graph must never silently change the public signature. This is "workflows are tools"
  applied to the interface.
- **An input is public only if it is a decision, not a consequence.** Display name, slug,
  org, profile layers, visibility, license, environments, region, and per-target name
  overrides are decisions. Every provider name is a consequence and is derived inside the
  graph. `propose_slug` is a helper tool; the slug itself is a required input, so the frozen
  choice is explicit in the call rather than hidden in a default.
- **No secret input types.** The checker rejects a signature containing a secret type. When
  a workflow needs an existing secret, it takes a typed non-secret reference such as a
  Doppler or 1Password item path, and a secret-source tool resolves it inside the butler.
- **Conditional requirements are part of the signature.** Layers gate inputs: bundle-ID
  overrides only matter for an ios-app layer. The published JSON schema expresses this with
  conditional requirements rather than a flat required list.
- **Resolution is itself a tool.** `describe(workflow, partial_inputs)` is pure, makes no
  provider calls, parses what it is given into domain types, and returns validation errors
  plus the still-missing inputs, each with type, constraints, description, default, example,
  and a prompt string fit for asking a human. The agent loops on it until nothing is missing,
  then calls `plan` once. `plan` is where `read` runs and where "name taken" surfaces.
- **MCP elicitation** may let the butler ask the human directly for missing inputs. Verify
  the current spec and client support before relying on it; `describe` does not depend on it.

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

### Naming

The problem: one project needs a GitHub repo name, a Doppler project, a Buildkite pipeline
slug, Railway names, a bundle ID, a crate name, a Swift module name, and more, each with its
own grammar. Derived names are the natural keys that make tools idempotent, so if derivation
ever changes for an existing project, `read` misses and `ensure` creates duplicates. Naming is
therefore on the idempotence path and must be treated as frozen.

- **Display name and slug are different types.** `ProjectName` is free-form, mutable, and only
  used for labels and template text. `ProjectSlug` is the canonical identifier: an ordered list
  of words, serialised as kebab-case, immutable once the project exists. Every provider name is
  derived from the slug, never from the display name.
- **Two operations, only one of them deterministic by contract.** `propose_slug(name)` is a
  lossy heuristic run once at creation: NFKD, strip diacritics, ASCII lowercase, split on
  non-alphanumerics and case boundaries, join with hyphens. Its result is shown to the human or
  agent, confirmed, and persisted. It may improve over time because it is never re-run for
  existing projects. `derive_<target>(org, slug, ...)` is pure, total, ASCII-only, independent
  of time and of existing state, and versioned. It never changes for a given scheme version.
- **The slug grammar is the intersection of every target.** Words match `[a-z][a-z0-9]*` or
  `[0-9]+`, the first word starts with a letter, single hyphens separate words, no leading or
  trailing hyphen, length bounded by the tightest target after prefixes are accounted for.
  Because `ProjectSlug::parse` enforces the intersection, every derivation is total and cannot
  fail at plan or apply time. Validate against the union of all known targets, not just the
  current profile, so a project can add a layer later without renaming.
- **Derivation is a table of joins over the word list**, one row per target, never a parse of
  a target name back into words.

| Target | Join | Example for `third-thoughts` |
| --- | --- | --- |
| GitHub repo | kebab | `third-thoughts` |
| Doppler project | kebab | `third-thoughts` |
| Buildkite pipeline slug | kebab | `third-thoughts` |
| Railway project | kebab | `third-thoughts` |
| Railway service | component alone, scoped to the Railway project; slug when there is no component | `api` |
| Cargo package | kebab | `third-thoughts` |
| Rust lib / env prefix | snake | `third_thoughts` / `THIRD_THOUGHTS_` |
| Swift module / Xcode product | pascal | `ThirdThoughts` |
| Bundle ID | org reverse-DNS prefix + `.` + kebab | `com.lightlesslabs.third-thoughts` |
| Bundle ID, app extension | prefix + `.` + kebab + `.` + component | `com.lightlesslabs.third-thoughts.widget` |
| Android applicationId | org reverse-DNS prefix + `.` + snake | `com.lightlesslabs.third_thoughts` |

- **The reverse-DNS prefix comes from a domain the org owns, not from the GitHub org slug.**
  `lightlesslabs.com` gives `com.lightlesslabs`; the GitHub org `lightless-labs` would give a
  valid but mismatched prefix that every app group, iCloud container, keychain access group,
  and extension bundle ID would inherit. The org record carries a typed `Domain` and derives
  `ReverseDnsPrefix` from it, falling back to the GitHub org slug only when no domain is
  recorded. The prefix is frozen in the org record: bundle IDs cannot be deleted and changing
  one means a new app.
- **Android is the strictest target.** A Java package segment allows no hyphens, cannot start
  with a digit, and cannot be a Java keyword, so `native` or `default` must be rejected as a
  project slug at parse time if the project might ever gain an Android layer.
- **Structured identifiers, not string concatenation.** Derivation takes `(org, project_slug,
  component?, environment?)`. Components and environments are word-list types with the same
  grammar. Org-level parts such as the reverse-DNS prefix and the GitHub, Doppler, and Buildkite
  org slugs are typed org configuration set once, not per-project inputs.
- **Collisions are resolved by recorded overrides, never by auto-suffixing.** Derivation cannot
  solve global namespaces: a repo name may be taken in the org, a crate name squatted, a bundle
  ID registered elsewhere. When `read` finds a resource that exists but is not ours, the plan
  fails with "name taken, provide an override". The override is typed, must parse as the target
  type, and is persisted in the project record. The natural key for that resource is then the
  override. Auto-suffixing with `-2` depends on what exists at run time, which is exactly the
  non-determinism the model forbids.
- **The scheme is versioned and recorded.** The project record carries the slug, display name,
  org, naming scheme version, profile layers, and overrides. It lives in the butler's ledger and
  in the repo, alongside the template answers, so every derived name is reproducible from the
  repo alone. A new scheme version applies only to new projects. Renaming an existing project
  is an explicit destructive-class workflow, not a side effect of a rules change.
- **Reserved words are a real constraint.** Rust keywords cannot be lib names, Java keywords
  cannot be package segments, Swift keywords collide with the pascal join (`self` becomes
  `Self`), and some targets reserve names such as Windows device names. Treat them as part of the slug grammar's reject list so the failure happens at
  parse time, not at apply time.
- **Property-test the contract.** For every valid `ProjectSlug`, every derivation succeeds and
  every result parses as its target type. Kebab and snake round-trip back to the same word
  list. Pascal does not: a digit-only word fuses with its predecessor, so `foundry-2` and
  `foundry2` both become `Foundry2`. This is accepted. Pascal feeds only Swift module and
  Xcode product names, which are never natural keys of a provisioned resource; every identity
  that is a natural key uses kebab or snake and stays distinct.

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
- Exact name grammars and length limits for GitHub repos, Doppler projects and configs,
  Buildkite pipeline slugs, Railway projects and services, bundle IDs, and crates.io, so the
  slug grammar can be set to the true intersection.
- Whether a maintained Railway API client or provider exists.
- Whether App Store Connect exposes every step needed for app, bundle ID, certificate, and
  profile creation.

## Decisions on the open questions

**Decided:** 2026-09-11. Each can be revisited, but code proceeds on these.

| Question | Decision | Rationale |
| --- | --- | --- |
| DSL syntax | YAML, validated by a published JSON schema | Agents are fluent in it, validators exist everywhere |
| Expressions | Typed references only, `${{ steps.repo.url }}`. No expression language | Add CEL or similar only when a real workflow needs it |
| Hosting | Railway first | Small always-on box later if the circularity bites |
| Approval | Web page plus push notification | Chat integration later |
| Credential storage | Doppler as the vault, one bootstrap token | Already trusted by the org |
| License | AGPL-3.0-or-later (was MIT until 2026-09-14) | The operator's call when the public repository was created; matches third-thoughts |
| Build | Cargo | Matches every public sibling; Bazel only if the monorepo pulls it in |

## Milestone 2 decisions

**Decided:** 2026-09-12, while writing the milestone 2 plan. Each refines a section above.

- **Two kinds of secret** (refines Trust model and Type system). *Graph secrets* are values
  that flow through tool ports; they are secret domain types, and the only way to read their
  bytes is `expose(&SinkToken)`, which the apply executor alone can mint. *Execution-context
  credentials* are the butler's own provider tokens. They are not domain types, never enter
  the registry, a `Value`, a plan, or the ledger, and are held as `secrecy` newtypes whose
  bytes are read in exactly one function per HTTP layer, the one that sets the
  `Authorization` header. That function is the only allowed call site of
  `secrecy::ExposeSecret::expose_secret` outside the derive's generated `expose` and tests,
  enforced by the same `clippy.toml` mechanism that already guards `SinkToken::new`. The
  two gates are separate on purpose: `Tool::read` needs a credential and must never hold a
  `SinkToken`.
- **TLS at the edge** (refines Hosting and "its own TLS"). Railway terminates TLS; the
  binary listens on plain HTTP bound to the platform's port and never exposes that listener
  without an edge in front. A self-hosted deployment puts it behind a reverse proxy. The
  service still owns authentication and the audit log.
- **Name-only planning on the remote server** (refines "Workflow definitions are privileged
  content"). Over the network, `plan` and `apply` accept a workflow name resolved in a
  directory that is a checkout of a trusted ref; only the pure `validate` and `describe`
  accept a document body, for the authoring loop. Locally, the CLI and the stdio server
  accept a path, because the caller already holds the machine that holds the credentials.
- **Refuse, do not reconcile, a non-key attribute the tool should not change** (refines
  Tool contract). The reversibility class is static per tool, but some attribute changes
  are not reversible in spirit (a private repository turned public). Such a tool reports the
  existing resource as a mismatch, the plan stops with the attribute named, and `ensure`
  returns a conflict. Reconciliation of an attribute is a per-tool decision recorded in the
  tool's description.
- **Composition moves to its own plan.** Milestone 1 deferred workflow-as-tool to
  milestone 2; milestone 2 does not need it and it needs typed composite output ports and a
  `read` semantic over a sub-graph, so it gets a plan of its own once milestone 2 is
  complete.
- **Approval channel narrowed for now** (refines "Web page plus push notification"). The
  page ships in milestone 2; push notification is deferred because one operator polling a
  page is enough to prove the gate and the notification service is a product choice not
  yet made. The decision stands; only its second half waits. A pending plan may wait a day
  for its human, and an approved plan must be applied within the hour, so the human's pace
  and the plan's staleness are bounded separately.
- **"One bootstrap token" lives in the platform, not the binary** (refines "Doppler as the
  vault, one bootstrap token"). The butler's own provider credentials stay in Doppler, and
  Doppler's native Railway integration syncs them into the service's environment; the
  binary reads two environment variables and never holds a Doppler bootstrap token or
  fetches its own credentials at runtime. Fewer moving parts at startup, and the vault
  decision holds. Locally, the operator exports the same variables (from `doppler run` if
  they like).

## Milestones

1. Core with no real providers: domain types and derive macro, tool contract, graph checker
   with taint and approval classes, `describe` and `plan` against fake providers, one
   project-creation workflow as the test fixture. Plan: `docs/plans/2026-09-11-milestone-1-core.md`.
   **Completed:** 2026-09-12.
2. Real GitHub and Doppler providers, `apply`, run ledger, approval gate, MCP server over
   stdio and Streamable HTTP. Plan: `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`.
2b. Composition: `Workflow` implements `Tool` with typed composite output ports. Plan to be
   written when milestone 2 completes.
2c. Authorization: OAuth 2.1 on the MCP transport (the server as an OAuth resource server
   with protected-resource metadata, tokens validated against the operator's identity
   provider) and a browser login on the approvals page in place of Basic auth; short-lived
   credentials, no static bearer tokens. Decided 2026-09-15 ("We're going to need OAuth");
   it is what unblocks a public domain for the service, so it runs before 2b and 3. Plan:
   `docs/plans/2026-09-16-milestone-2c-authorization.md` (drafted from
   `docs/research/2026-09-16-m2c-authorization.md` and reviewed 2026-09-16; implementation
   starts only after milestone 2 is marked Completed); the identity provider and the public
   host are its two open decisions, both the operator's.
3. Templates and versioned re-apply, the project record, recorded naming overrides, and the
   Buildkite provider (a pipeline per repository; moved here from milestone 4 on 2026-09-14
   at the operator's request).
4. Railway.
5. App Store Connect.
