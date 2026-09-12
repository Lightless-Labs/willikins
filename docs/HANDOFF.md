# Willikins Handoff

Current state of the project and active work. Read this at session start. Update before
compaction, before handing off, after a milestone, and after a plan change or discovery.

**Last updated:** 2026-09-12

## Current Status

### RESUME HERE (2026-09-12) — milestone 1 complete and verified; milestone 2 has no plan yet

- **Live state:** `main` at 95 local commits, gates green, 699 tests. No remote is configured
  and nothing has been pushed. Working tree clean apart from the gitignored `nohup.out`.
- **What just happened:** the whole of milestone 1 was built in one session on 2026-09-11 and
  2026-09-12 through nine Workflow runs (sonnet implementing, opus verifying). The plan record
  is `docs/plans/2026-09-11-milestone-1-core.md`, marked Completed.
- **Next action:** write the milestone 2 plan (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`)
  following the tracking todo `todos/2026-09-12-milestone-2-plan.md`. Do not start milestone 2
  code before the plan exists and has been reviewed with the document-review workflow.
- **Before any real provisioning run:** verify the Swift and Kotlin reserved-word lists
  (`todos/2026-09-12-verify-keyword-lists.md`). The slug grammar is frozen once a project exists.
- **Before the MCP server accepts documents from agents:** fix YAML scalar-alias amplification
  (`todos/2026-09-12-yaml-scalar-alias-amplification.md`).

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
- **The RTK hook can summarize a failing cargo build into "No issues found".** Always
  `rtk proxy cargo ...` and read the log body. `$?` after a pipe in zsh is the last command's
  status.
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

| File | Priority | Gates |
| --- | --- | --- |
| `todos/2026-09-12-milestone-2-plan.md` | high | the next session's first action |
| `todos/2026-09-12-verify-keyword-lists.md` | high | any real provisioning run |
| `todos/2026-09-12-yaml-scalar-alias-amplification.md` | high | the MCP server accepting documents |
| `todos/2026-09-12-prompt-text-from-documents.md` | medium | agent-facing output in milestone 2 |
| `todos/2026-09-12-check-error-site-enum.md` | medium | composite output ports in milestone 2 |
| `todos/2026-09-12-check-error-serialize.md` | medium | the MCP `validate` tool |
| `todos/2026-09-12-workflow-name-description-bounds.md` | low | |
| `todos/2026-09-12-fake-state-write-only.md` | low | |
| `todos/2026-09-11-propose-slug-digit-letter-tokens.md` | low | |

## How Work Is Verified

- Four gates before every commit, through `rtk proxy cargo`.
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

## Next: Milestone 2 Runbook

Write the plan first. Its scope, from the design doc's milestone list and this session's
findings:

1. Real GitHub and Doppler providers behind the same `Tool` contract as the fakes, with the
   port table unchanged. Credentials held server-side; provider auth is execution context,
   never an input.
2. `apply`: the executor is the only non-test `SinkToken` site. Approval gate before any
   node whose class is above `Reversible`. Run ledger with per-node status; re-running a
   partially failed plan converges because every tool is idempotent.
3. MCP server with `rmcp` 3 over stdio and Streamable HTTP: `validate`, `describe`, `plan`,
   `apply`, `list_tools`, `propose_slug`. The CLI already mirrors these.
4. Authentication, TLS, and an append-only audit log for the remote deployment (Railway).
5. Prerequisites from the todos: scalar-alias amplification, `CheckError` serialization,
   document-text labelling in agent-facing output, and the keyword-list verification.
6. Composition: `Workflow` implements `Tool` with typed composite output ports, which is also
   when the sentinel sites in `CheckError` get replaced by a site enum.

Verify with a browser before relying on them: the current MCP authorization spec for HTTP
transports, `rmcp` 3's Streamable HTTP server API, and the Swift and Kotlin keyword lists.
