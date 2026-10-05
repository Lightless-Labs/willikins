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
  construction, not by remembering to scrub. `Value` renders through `render()` everywhere;
  an output surface uses `Value::display(Disclosure)` or `mask_json`, which are `render()` plus
  identifier masking.
- Identifier types (`#[domain(identifier)]`: the App Store Connect issuer id, key id and
  certificate serial, Apple certificate, bundle id and profile record ids, the Buildkite
  cluster id, and `DopplerValue`) print as a short prefix on every output surface: CLI text
  and JSON, MCP results and errors, the approvals page. Only the CLI's `--reveal` prints them
  whole. The journal and plan fingerprints keep full values, because apply and drift
  detection read them back. No type is both secret and an identifier.
- A secret output may only bind to a secret-accepting input. `check` enforces it; do not
  add a bypass. Everything knowable statically is rejected by `check`; `plan` only backstops.
- Naming derivation (`naming::v1`) is frozen. Fix bugs in it by adding `v2`, never by editing
  `v1`. Adding a row for a new provider is not a version bump.
- No tool may take a raw URL, shell command, or arbitrary API path as input. Content a
  document renders itself is the one exception. A `RepoFile`, built only by `repo.file.render`
  from a document-literal template and typed substitutions and never from a literal, input,
  default or `Text`, may be committed by `github.scaffold.ensure` or stored as a
  willikins-owned Buildkite pipeline's configuration by `buildkite.pipeline.bootstrap.ensure`,
  whose path must be a YAML file directly under a `.buildkite/` directory.
- `SinkToken::new` is disallowed by `clippy.toml` outside the apply executor; tests opt in with
  a narrowly scoped `#[allow(clippy::disallowed_methods)]`. `Tool::read` never receives one.
- The type registry refuses secret types for any literal or input, regardless of element count.
- A conversion is registered only through `conversions!`, only from a total `From` impl, never
  secret-to-public and never identifier-to-plain (both compile errors), and is resolved by a single probe, never a search.
- Workflow documents and templates are privileged content: run only from a trusted ref.
- No provider-token-shaped literal anywhere in the tree, source or docs alike: any of Doppler's
  six kinds (`dp.sa./pt./ct./st./scim./audit.`) or GitHub's six prefixes
  (`ghp_`/`github_pat_`/`gho_`/`ghu_`/`ghs_`/`ghr_`) followed by a long run. Assemble it from
  parts (`concat!("dp.st.", "...")`) instead. The lists are the *detector's*, taken from
  GitHub's published pattern tables, not willikins' own `CREDENTIAL_PATTERN`s — a shape this
  workspace never authenticates as still blocks a push. `secret_literal_guard.rs` enforces it;
  the fix for a blocked push is to stop writing the literal, never to bypass the scanner with
  the operator's own credential.
- No file this workspace runs invokes the operator's own `gh` CLI. Every provider call
  authenticates as a credential the project holds instead (e.g. `WILLIKINS_GITHUB_TOKEN`
  through `curl`). `no_gh_writes_guard.rs` enforces it — the operator's `gh` credential can
  administer every repository they can touch, so it is never spent without them there.

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

Each crate's default-built integration tests compile as one binary, `tests/it/main.rs`, one module per
former `tests/<stem>.rs` file (milestone 3k). Run one test module with
`cargo test -p <crate> --test it <module>:: -j 2`, note the trailing `::`: `--exact` now needs the module
prefix too (`<module>::<test_name>`), or it silently matches nothing instead of failing. A new
`tests/it/<stem>.rs` must get a `mod <stem>;` line in `tests/it/main.rs` or it is silently never compiled;
`crates/willikins-core/tests/it/test_layout_guard.rs` catches both lapses. The `required-features =
["live-tests"]` targets and, in `willikins-cli`, the gitignored `operator_*` targets stay their own
top-level binaries and are unaffected.

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
