# Willikins Handoff

Current state of the project and active work. Read this at session start. Update before
compaction, before handing off, after a milestone, and after a plan change or discovery.

**Last updated:** 2026-09-12 (evening)

## Current Status

### RESUME HERE (2026-09-12) — milestone 2 plan written, researched, and reviewed; no milestone 2 code yet

- **Live state:** `main` at 100 local commits, gates green at the last code change (699
  tests; no code has changed since). No remote is configured and nothing has been pushed.
- **What just happened:** the milestone 2 plan
  `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md` was written, backed by
  `docs/research/2026-09-12-m2-dependencies.md` (five parallel research passes with
  verbatim sources), reviewed by the document-review workflow (coherence, feasibility,
  security, scope, adversarial), and stamped Reviewed with 20 findings folded in. The design
  doc gained a "Milestone 2 decisions" section. The keyword lists were verified: Swift is
  missing `borrowing`, `consuming`, `nonisolated`; everything else matches its source.
- **Next action:** dispatch implementation per the plan's task table. Task 0 (three Swift
  keywords) and group A (1a+1b core serialization and the `Site` enum; 1c types and DSL
  bounds plus the YAML pre-scan; 1d describe labelling) can start at once in separate
  worktrees, sonnet implementing test-first and opus verifying, one Workflow per group as
  in milestone 1. Every `CONTEXT` string names the four bare `cargo` gates.
- **Ask the operator for** sandbox credentials (a throwaway GitHub org token and a Doppler
  service-account token) before task 8, so the read-only probe settles the undocumented
  Doppler facts early rather than at the live smoke run.
- **Do not** start the MCP server task (10b) before 1c lands: the YAML pre-scan and byte
  cap are what make accepting a document body over the network safe.

## Project State

Six crates, dependencies flowing downward only:

| Crate | Holds | State |
| --- | --- | --- |
| `willikins-types` | `DomainType`, `DomainObject`, the derive, slug grammar, 19 domain types, `TypeRegistry`, `naming::v1`, `propose_slug`, `SinkToken` | done, verified |
| `willikins-derive` | `#[derive(DomainType)]` for `String`, `SecretString`, and `FromStr + Display` storages | done, verified |
| `willikins-core` | `Value` with its JSON shape, `Tool` contract, `Catalog`, `Workflow`, `check` (21 error kinds), `describe`, `plan` | done, two adversarial passes |
| `willikins-providers-fake` | `FakeState` (JSON-seedable), ten tools per the port table | done |
| `willikins-dsl` | YAML document to `Workflow`, reference grammar, located errors, published document schema | done |
| `willikins-cli` | `validate`, `describe`, `plan`, `schema`, `propose-slug`; `--json`; text renders only through `Value::render()` | done, acceptance suite |

The CLI's first four subcommands are the milestone 2 MCP tools one to one. Nothing talks to a
real API. `apply` exists on the `Tool` trait but nothing calls it.

Fixtures: `workflows/new-rust-service.yaml` is the positive case; `workflows/fixtures/` holds
one document per negative case and `fixtures/state/` the fake-state files. Every fixture's
header comment names its acceptance test and exact expected error.

Research: `docs/research/2026-09-11-m1-dependencies.md` (crate and provider-grammar
research, with a correction block on its slug section),
`docs/research/2026-09-12-check-adversarial-pass-1.md`,
`docs/research/2026-09-12-e2e-adversarial-pass-2.md`.

## Architecture Gotchas

- **`SinkToken` is a lint, not a proof, inside the workspace.** Cargo unifies features, so
  once `willikins-core` enables `willikins-types/executor` every crate can see the
  constructor. `clippy.toml` disallows `SinkToken::new`; gates run `-D warnings`; every
  allowed call site is a test item. `Tool::read` takes no token, which is a structural aid.
  The feature gate protects external consumers only.
- **`cargo check -p willikins-types` is a real gate.** The crate enables its own `executor`
  feature through a self dev-dependency, so `--all-targets` never builds it the way its
  dependents see it. A cfg-gated bug slipped past the other three gates once.
- **Read the log body, never a captured exit code.** The RTK hook that once rewrote cargo
  commands is gone from this machine (2026-09-12); gates are bare `cargo`. `$?` after a pipe
  in zsh is the last command's status, so never pipe gate output.
- **Network tools work.** WebFetch, WebSearch, context7, `curl`, and `gh api` work for the
  main session and for agents. Prefer verbatim primary sources: GitHub's OpenAPI description,
  docs repos' raw markdown, Doppler's `<page>.md` twins, the `swiftlang/swift-book` DocC
  source for docs.swift.org.
- **`extern crate self as willikins_types`** in `willikins-types/src/lib.rs` exists so the
  derive's generated `::willikins_types::` paths resolve inside the crate itself.
- **The type registry and `type_infos()` come from one `domain_types!` invocation** in
  `lib.rs`. Add a domain type there or it is invisible to `check`, `describe`, and the CLI.
- **The registry refuses secret types for any literal or input before looking at the text**,
  including an empty list. Seeding secret values into fake state goes through serde
  `Deserialize` on the concrete type instead.
- **`Value::render()` is the only path from a value to text.** Debug and Serialize on `Value`
  go through it. A secret list prints one marker in Debug and one marker per element in JSON.
- **`naming::v1` is frozen.** Adding a provider adds rows; changing a row is `v2`. Pascal is
  not injective for digit-only words (`foundry-2` and `foundry2` both give `Foundry2`);
  accepted because pascal never feeds a natural key.
- **`check` uses sentinel sites:** errors on a workflow output use the node name `outputs`,
  errors on a `for_each` binding use the port name `for_each`. Three bugs came from the
  collision; see `todos/2026-09-12-check-error-site-enum.md`.
- **`for_each` expands at plan time**, keyed by each item's canonical string. `check` can only
  verify shapes; `plan` reports `KeyNotInForEach`, `ForEachUnknown`, `DuplicateForEachKey`.
  Duplicates in a statically known default are caught by `check` as `DuplicateForEachDefault`.
- **`Observation::Absent { predicted }`**: a tool fills every output it can derive from its
  inputs so downstream nodes can still `read` at plan time. A token's value is `Unknown` on
  both `Absent` and `Present` because Doppler cannot re-read it.
- **serde_yaml_ng keeps the last duplicate mapping key silently.** The DSL deserializes every
  map through a unique-map visitor; duplicate keys surface as YAML errors with a location.
- **`#[serde(deny_unknown_fields)]`** is on documents and fake state. The published document
  schema carries `additionalProperties: false` at every level; regenerate the insta snapshot
  if that changes.
- **The trybuild `.stderr` files** quote rustc diagnostics and are toolchain-sensitive.
  Regenerate with `TRYBUILD=overwrite` on a toolchain bump rather than hand-editing.
- **`CheckError` and `CheckWarning` derive no `Serialize`**; the CLI hand-builds their JSON.
  Milestone 2's MCP surface needs the derive; see `todos/2026-09-12-check-error-serialize.md`.
- **Workflow-as-tool (composition) is deferred to milestone 2.** Nothing in milestone 1
  exercises it, and it needs typed composite output ports.

## Open TODOs

Every open todo except the last is now a numbered task in the milestone 2 plan; close each
when its task lands.

| File | Priority | Milestone 2 task |
| --- | --- | --- |
| `todos/2026-09-12-milestone-2-plan.md` | high | the tracking todo; plan written and reviewed, implementation next |
| `todos/2026-09-12-verify-keyword-lists.md` | high | task 0 (verification done; three Swift words to add) |
| `todos/2026-09-12-yaml-scalar-alias-amplification.md` | high | task 1c |
| `todos/2026-09-12-prompt-text-from-documents.md` | medium | task 1d |
| `todos/2026-09-12-check-error-site-enum.md` | medium | task 1b |
| `todos/2026-09-12-check-error-serialize.md` | medium | task 1a |
| `todos/2026-09-12-workflow-name-description-bounds.md` | low | task 1c |
| `todos/2026-09-12-fake-state-write-only.md` | low | task 4 |
| `todos/2026-09-11-propose-slug-digit-letter-tokens.md` | low | not scheduled |

## How Work Is Verified

- Four gates before every commit, bare `cargo`, in the background with a 600,000 ms
  timeout, reading the log body.
- Each task: sonnet implements test-first, opus attacks it with new tests and fixes what it
  breaks, one commit per fix. The Workflow scripts from this session are under the session's
  `workflows/scripts/` directory and follow one shape: `CONTEXT` string with gates and rules,
  implement stage with a structured `REPORT`, verify stage with a structured `VERDICT`.
- Adversarial passes are acceptance test 12 of the milestone plan and are recorded under
  `docs/research/`. Every bypass becomes a fixture plus an acceptance test.
- Plans get the document-review workflow (scope, feasibility, security, coherence,
  adversarial personas on sonnet, merged and ranked) before implementation starts. The
  milestone 1 review produced 23 findings, five of them blockers; all are resolved in the
  plan's "Review resolutions" section.

## Recent Context

- 2026-09-11: design conversation, design doc, dependency research, milestone 1 plan and its
  review, scaffold, tasks 2 and 3 (slug grammar, derive macro).
- 2026-09-12: tasks 4 through 13. Notable findings on the way: `gen` missing from the Rust
  keyword list; feature unification defeating the `SinkToken` gate; an input's default value
  bypassing the sink check; an empty list literal bypassing the secret refusal; duplicate
  `for_each` keys; a 10 MB rejected input echoed in full; unknown document fields silently
  ignored. No attack in either adversarial pass reached a secret byte.
- Costs: nine Workflow runs, about thirty agents, roughly 5.7M subagent tokens.
- 2026-09-12, second session: milestone 2 plan, research note, design addenda, and document
  review (five research agents, five reviewer agents). Decisions worth knowing before
  reading the plan: two kinds of secret (graph secrets behind `SinkToken`, provider
  credentials behind one `authorize` function and a clippy entry); `Tool::ensure` returns
  `Ensured { outputs, changed }` and every live `ensure` reads first; `apply` over MCP
  returns a `run_id` and runs in the background; a plan has an approval window and an
  apply window; the remote server plans and applies by workflow name only; visibility
  mismatches are refused, not reconciled; composition moved to its own future plan.

## Next: Milestone 2 Runbook

The plan is the runbook: `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`, its
"Tasks" table (groups A, B, C, D run in parallel worktrees), its 19 acceptance tests, and
its "Verify before relying on them" list. Two adversarial passes are tasks 9 and 13 and get
recorded under `docs/research/`. Task 14 (the live smoke run) needs the operator's sandbox
credentials and is the completion gate.
