# Willikins

Rust workspace. Read `docs/plans/2026-09-11-willikins-design.md` before changing anything in
`crates/`; it holds the invariants the code must keep.

## Invariants

- No bare `String` in a tool port. Every port is a domain type from `willikins-types`.
- Secret types never implement plain `Display`, and never serialize through plain serde.
  Redaction is by construction, not by remembering to scrub.
- A secret output may only bind to a secret-accepting input. `check` enforces it; do not
  add a bypass.
- Naming derivation (`naming::v1`) is frozen. Fix bugs in it by adding `v2`, never by editing
  `v1`.
- No tool may take a raw URL, shell command, or arbitrary API path as input.

## Commands

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p willikins-types
```

Run all four before every commit. The last one matters because `willikins-types` enables its
own `executor` feature through a self dev-dependency, so `--all-targets` never builds the crate
the way its dependents see it. Builds on this host can be slow; run cargo in the
background with a generous timeout.

## Conventions

- TDD: write the failing test first.
- One task per commit. Commit messages describe the behaviour, not the diff.
- Fixtures live in `workflows/fixtures/`. Negative fixtures are as important as positive ones.
