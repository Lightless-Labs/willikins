# Willikins Handoff

Current state of the project and active work. Read this at session start. Update before
compaction, before handing off, after a milestone, and after a plan change or discovery.

**Last updated:** 2026-09-12 (evening)

## Current Status

### RESUME HERE (2026-09-14) — milestone 2 tasks 0 through 6 landed; tasks 7 and 8 (live providers) are next

- **Live state:** `main` at 194 local commits, gates green at HEAD (1142 tests, 3 ignored: two by-hand
  measurements). No remote is configured and nothing has been pushed.
- **What just happened:** tasks 4 (apply executor), 5 (`willikins-journal`) and 6
  (`willikins-providers-http`) landed through Workflows `wf_99b69f88-80c` (killed by an
  OAuth outage mid-verification) and `wf_e61c2e5e-4a2` (which inherited and finished it).
  The verifiers' real finds: the executor's approval gate read only the plan's
  `requires_approval` flag (now also the checked class); an `unreachable!` on an unknown
  required input bound to a workflow input (now `UnknownRequiredInput`); the journal listed
  already-run plans as pending and journaled a failed re-plan as a run; the HTTP client
  echoed response bodies into error messages, kept 401/403 bodies, followed redirects,
  slept on an uncapped `Retry-After`, panicked on a huge HTTP-date, and exposed the bearer
  through a public `authorize`. All fixed and pinned. Plan addendum dated 2026-09-14
  records the deviations; two new todos hold the deferred items.
- **Group A landed (2026-09-12, late evening):** Workflow `wf_c7a1d060-cec` ran three
  worktree lanes. Lane 1 (1a, 1b) and lane 3 (task 0, 1d) merged onto `main` with gates
  green. Lane 2 (1c) was lost: the coordinator's stop message meant for a duplicate agent
  was read by the real one, which halted with an uncommitted partial diff; that diff is
  saved as `lane2-partial-1c.patch` in the session scratchpad (two finished type files,
  `WorkflowName` and `Description`, plus token-literal reshaping) and 1c is re-run on
  `main`. Tasks 1c, 1e, 2 and 3 run sequentially on `main`, one Workflow. Operator
  credentials are in
  `~/.config/willikins/sandbox.env` (mode 600, outside the repo): the Doppler one is a
  config-scoped service token (`dp.st.`), enough for auth and `doppler.secret.get`
  against `willikins-test/dev`, not for creating projects, environments or tokens; task
  8's create-side probe and task 14 need a `dp.sa.` or `dp.pt.` token from the operator.
- **Next action:** task 7 (`willikins-providers-github`), task 8
  (`willikins-providers-doppler` plus the opt-in read-only live probe), then task 9
  (adversarial pass 1, opus, recorded under `docs/research/`), one sequential Workflow on
  `main`, sonnet implementing test-first and opus verifying, every `CONTEXT` string naming
  the four gates exactly as CLAUDE.md now spells them (`-j 2`, `RUST_TEST_THREADS=2`).
  Then 10a, 10b and 11, 12, 13, 14. Groups run one lane at a time (memory, below). The
  Workflow tool needs the operator's opt-in per session ("use a workflow" or
  "ultracode"); without it, dispatch through plain Agent calls. Agents must never message
  the coordinator mid-run (a reply resumes a duplicate of them).
- **Credentials for the probe and the smoke run** live in `~/.config/willikins/sandbox.env`
  (mode 600, outside the repo; source it before a live run): a GitHub fine-grained PAT
  scoped to the test organization `Willikins-Test` (read-only checks passed 2026-09-13)
  and a Doppler service token that can only read `willikins-test/dev`; the create-side
  probe and task 14 still need a `dp.sa.` or `dp.pt.` Doppler token from the operator.
  Agents get the file path, never the values, and never print them.
- **Ask the operator for** sandbox credentials (a throwaway GitHub org token and a Doppler
  service-account token) before task 8, so the read-only probe settles the undocumented
  Doppler facts early rather than at the live smoke run.
- **Do not** start the MCP server task (10b) before 1c lands: the YAML pre-scan and byte
  cap are what make accepting a document body over the network safe.

## Project State

Nine crates, dependencies flowing downward only:

| Crate | Holds | State |
| --- | --- | --- |
| `willikins-types` | `DomainType`, `DomainObject`, the derive, slug grammar, 21 domain types (now `WorkflowName`, `Description`), `TypeRegistry`, `naming::v1`, `propose_slug`, `SinkToken` | done, verified |
| `willikins-derive` | `#[derive(DomainType)]` for `String`, `SecretString`, and `FromStr + Display` storages | done, verified |
| `willikins-core` | `Value` with its JSON shape, `Tool` contract (`ensure -> Ensured`, `Observation::Mismatch`), `tool::helpers`, `Catalog`, `Workflow` (typed name and description), `check` (21 error kinds, `Site`), `describe` (`document_description`), `plan` (`AttributeMismatch`, `Plan::fingerprint`), `apply` (the one `SinkToken::new` site; `Approval`, `ApplyError`, `ApplyObserver`, `PrincipalId`, `Timestamp`), `Reported`, JSON schemas for the MCP result types, `testing` generators behind `test-support` | done through task 4 |
| `willikins-tools` | `naming.v1` and `template.render`, pure, moved out of the fake crate; `register(&mut Catalog)` | done |
| `willikins-journal` | `PlanId`/`RunId` (uuid v7), `Event`/`Entry`, `Redacted<T>`, `Journal` trait with fold-based views (`PlanRecord`, `RunRecord`), `FileJournal` (JSONL, fd-lock, `sync_data`, validated replay), `MemoryJournal`, `JournalObserver`, `run_and_journal` | done, verified |
| `willikins-providers-http` | `Credential` (env-sourced, redacted, crate-private `authorize`), `Http` over ureq 3 (retry on 429/5xx/transport for GET/PUT/DELETE, never POST; `Retry-After` capped at 60 s; no redirects), `ProviderError` -> `ToolError` with `provider says:` labelling and 256-char bound, `testing` module behind `test-support` | done, verified |
| `willikins-providers-fake` | `FakeState` (JSON-seedable, one-way redacted; `next_token`, `fail_ensure_once`, `ensure_calls`, `read_calls`), nine tools plus the two from `willikins-tools` (`doppler.service_token.rotate` is Destructive); every `ensure` reads first, `doppler.project.ensure` seeds `dev`/`stg`/`prd`, `github.repo.ensure` refuses a visibility mismatch | done through task 4 |
| `willikins-dsl` | YAML document to `Workflow`, reference grammar, located errors, published document schema, 256 KiB cap, anchor/alias/BOM pre-scan, typed `name`/`description` | done through task 1c |
| `willikins-cli` | `validate`, `describe`, `plan`, `schema`, `propose-slug`; `--json` through `Reported`; text renders only through `Value::render()` and `single_line` escaping; `document says:` prefix | done, acceptance suite |

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
- **This host has 11 GB of RAM and 6 CPUs, shared with other sessions.** Three parallel
  worktree lanes each building their own `target/` took 6.2 hours on 2026-09-12 and the
  coordinator's gate run was killed for memory, every time in the doctest phase, whose
  harness compiles doctests per CPU regardless of `-j`. Run cargo with `-j 2` and the test
  gate with `RUST_TEST_THREADS=2`, one lane at a time on `main`, never two cargo commands
  at once. A full test gate is 30 to 45 minutes here.
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
- **Error locations are a `Site`** (`Port { node, port }`, `ForEach { node }`, `Output { name }`),
  serialized as `{"kind": "port" | "for_each" | "output", ...}` and displayed as
  `node.port`, `node[for_each]`, `workflow.outputs.name`. The old `outputs`/`for_each`
  sentinels are gone; no test may compare against them.
- **Every error serializes as `{"kind", "message", ...fields}`** through `Reported<T>`; the
  `variant_kinds!` list in the core tests is the exhaustiveness guard (a new variant fails
  to compile until it is listed). Gaps that remain are in
  `todos/2026-09-12-error-json-uniformity-gaps.md`.
- **`AttributeMismatch` carries a `Site` and no instance key**, so a `for_each` node cannot
  say which instance mismatched; milestone 3 with `Action::Update`.
- **The YAML pre-scan** (`saphyr-parser`) refuses anchors, aliases and a leading BOM before
  `serde_yaml_ng` runs, fails closed on its own scan errors, and compensates for
  `Marker::col` being 0-based despite its docs. It is a second full parse of the document,
  bounded by the 256 KiB cap that `load_document` applies to the file read as well.
- **`willikins_types::Description` and `willikins_core::describe::Description` share a name**
  on purpose; use qualified paths.
- **The executor's approval gate reads the checked class**, not only the plan's
  `requires_approval` flag: a plan replayed from the journal is data. Refusals before any
  provider write (`ApprovalRequired`, `Plan`, `Drift`) carry no partial result; mid-run
  failures (`UnknownInput`, `UnknownRequiredInput`, `Tool`) carry the partial `Applied`.
- **`Plan::fingerprint` does not cover node inputs**, and core never compares the approved
  plan's workflow name with the checked one. The server's `(name, document sha256)` plan
  identity is what closes that; see `todos/2026-09-14-plan-identity-must-cover-inputs.md`.
- **The journal replays through `Redacted<T>`** because core `Value` has no `Deserialize`:
  a sealed wrapper storing the already-redacted JSON. There is no hash chain, by decision.
  Replay reads the path, not the locked descriptor (a rename over it orphans appends).
- **Provider text is labelled `provider says:`** and bounded to 256 characters; a 401/403
  body is dropped at construction; `Retry-After` is capped at 60 s; redirects are refused;
  POST is never retried. GitHub's secondary rate limit is a 403 with `retry-after`, which
  the shared client does not retry: task 7's tools handle it.
- **Both `#[allow(clippy::disallowed_methods)]` in the derive are load-bearing**: clippy
  does lint macro expansions, and the derive has two `expose_secret` sites (`expose` and
  the generated `PartialEq`). `expose_secret_mut` is disallowed too. Two syn-based tests
  walk every `.rs` file cargo compiles to pin the call sites of both secrets' escape
  hatches.
- **`std::env::set_var` is `unsafe` in edition 2024 and the workspace forbids `unsafe`**;
  tests build credentials through `Credential::for_testing` behind `test-support`.
- **`for_each` expands at plan time**, keyed by each item's canonical string. `check` can only
  verify shapes; `plan` reports `KeyNotInForEach`, `ForEachUnknown`, `DuplicateForEachKey`.
  Duplicates in a statically known default are caught by `check` as `DuplicateForEachDefault`.
- **`Observation::Absent { predicted }`**: a tool fills every output it can derive from its
  inputs so downstream nodes can still `read` at plan time. A token's value is `Unknown` on
  both `Absent` and `Present` because Doppler cannot re-read it. `Observation::Mismatch
  { port }` is ours-but-different; `plan` refuses it symmetrically.
- **`Tool::ensure` returns `Ensured { outputs, changed }` and reads first.** Comparable-state
  resources create only what is missing; the GitHub Actions secret always writes; pure
  tools answer `ensure` exactly as `read`. The fake `doppler.project.ensure` seeds the three
  default configs, so the positive fixture's `configs[dev|stg|prd]` are `Unchanged` on a
  first apply.
- **serde_yaml_ng keeps the last duplicate mapping key silently.** The DSL deserializes every
  map through a unique-map visitor; duplicate keys surface as YAML errors with a location.
- **`#[serde(deny_unknown_fields)]`** is on documents and fake state. The published document
  schema carries `additionalProperties: false` at every level; regenerate the insta snapshot
  if that changes.
- **The trybuild `.stderr` files** quote rustc diagnostics and are toolchain-sensitive.
  Regenerate with `TRYBUILD=overwrite` on a toolchain bump rather than hand-editing.
- **Workflow-as-tool (composition) is milestone 2b**, its own plan; nothing in milestone 2
  exercises it.

## Open TODOs

| File | Priority | Owner |
| --- | --- | --- |
| `todos/2026-09-12-milestone-2-plan.md` | high | the tracking todo; tasks 0–6 done, task 7 next |
| `todos/2026-09-14-plan-identity-must-cover-inputs.md` | high | task 10a and adversarial pass 1 |
| `todos/2026-09-12-error-json-uniformity-gaps.md` | medium | task 10a (`InputError`, `DocumentError` shapes) and 10b |
| `todos/2026-09-13-apply-tests-on-the-real-fake-catalog.md` | low | any core task after 4 |
| `todos/2026-09-14-journal-follow-ups.md` | low | task 10a |
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
- 2026-09-13 and 14: tasks 0 through 6 landed through five Workflows (about 8.5M subagent
  tokens); one lane lost to a misdirected stop message, one verifier killed by an OAuth
  outage and its work inherited by the next; the host's memory limit found and bounded.
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
