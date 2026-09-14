# Willikins

A provisioning butler: typed, composable project-provisioning workflows an agent can author
and run over MCP or the CLI, without ever touching a secret. Rust workspace, Cargo, AGPL-3.0-or-later; public at
<https://github.com/Lightless-Labs/willikins>.

## Session Continuity

**Read `docs/HANDOFF.md` at the start of every session.** Its "RESUME HERE" block holds the
live state and the next action. Update it before context compaction, before handing off,
after a milestone, and after a plan change or discovery. The active tracking todo is under
`todos/` (currently `todos/2026-09-12-milestone-2-plan.md`).

## Key Directories

- `docs/HANDOFF.md` - **Read first.** Project state, architecture gotchas, open work, how
  work is verified. Update before compaction or at session end.
- `todos/` - One file per pending work item, `YYYY-MM-DD-slug.md` with YAML frontmatter
  (`title`, `created`, `status`, `priority`, `area`, `related`).
- `docs/plans/` - Design and milestone plans. The design doc holds the invariants; each
  milestone gets its own plan with `Created`, `Reviewed`, `Addendum`, `Completed` headers.
- `docs/research/` - Dependency research and the adversarial-pass records.
- `docs/solutions/` - Documented learnings with YAML frontmatter. Search before debugging
  anything in a documented area.
- `workflows/` - The positive fixture and, under `fixtures/`, one document per negative case
  and the fake-state files under `fixtures/state/`.

## Invariants

Read `docs/plans/2026-09-11-willikins-design.md` before changing anything in `crates/`.

- No bare `String` in a tool port. Every port is a domain type from `willikins-types`.
- Secret types never implement plain `Display` or serde `Serialize`. Redaction is by
  construction, not by remembering to scrub. `Value` renders through `render()` everywhere.
- A secret output may only bind to a secret-accepting input. `check` enforces it; do not
  add a bypass. Everything knowable statically is rejected by `check`; `plan` only backstops.
- Naming derivation (`naming::v1`) is frozen. Fix bugs in it by adding `v2`, never by editing
  `v1`. Adding a row for a new provider is not a version bump.
- No tool may take a raw URL, shell command, or arbitrary API path as input.
- `SinkToken::new` is disallowed by `clippy.toml` outside the apply executor; tests opt in with
  a narrowly scoped `#[allow(clippy::disallowed_methods)]`. `Tool::read` never receives one.
- The type registry refuses secret types for any literal or input, regardless of element count.
- Workflow documents and templates are privileged content: run only from a trusted ref.

## Commands

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
```

Run all four before every commit. The last one matters because `willikins-types` enables its
own `executor` feature through a self dev-dependency, so `--all-targets` never builds the crate
the way its dependents see it. `-j 2` and `RUST_TEST_THREADS=2` are load-bearing on this host
(11 GB of RAM shared with other sessions): a wider build, or the doctest harness compiling
doctests per CPU, gets the run killed for memory. Never run two cargo commands at once.

Read the log body, never just a captured exit code. In zsh, `$?` after a pipe is the last command's
status; do not pipe gate output through `tail` or `tee`. This host is slow: run cargo in the
background with a 600000 ms timeout. The trybuild suite alone takes about 90 seconds.

Try the CLI:

```
cargo run -p willikins-cli -- plan workflows/new-rust-service.yaml \
  --input slug=third-thoughts --input org=lightless-labs
```

## Process

Follows the monorepo process (sub-agents per task, a dedicated plan per milestone, plan
headers updated on review, addendum, and completion). Specific to this repo:

- Implementation is delegated: sonnet implements a task test-first, opus attacks it. Every
  adversarial pass is recorded under `docs/research/` and every bypass it finds becomes a
  fixture under `workflows/fixtures/` plus an acceptance test.
- Parallel tasks that touch the same crate run in separate git worktrees and are merged on
  `main` by the coordinator; the worktree directory is gitignored.
- Agents keep their own commit attribution; the coordinator's commits carry its model.
- WebFetch, WebSearch, context7, `curl`, and `gh api` all work for the main session and for
  agents (verified 2026-09-12; an earlier hook that blocked WebFetch is gone). Prefer a
  primary source fetched verbatim (an OpenAPI description, a docs repo's raw markdown,
  Doppler's `<page>.md` twins) over a rendered page's paraphrase. A fact that could not be
  fetched verbatim goes into the plan's "verify" list, not into frozen code.

## Conventions

- TDD: write the failing test first.
- One behaviour per commit. Commit messages describe the behaviour, not the diff.
- Negative fixtures are as important as positive ones. Each carries a header comment naming
  its acceptance test and the exact error it must produce.
- Favour ast-grep over grep when researching code.
