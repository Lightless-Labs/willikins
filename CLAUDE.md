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
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

Run all three before every commit. Builds on this host can be slow; run cargo in the
background with a generous timeout.

## Conventions

- TDD: write the failing test first.
- One task per commit. Commit messages describe the behaviour, not the diff.
- Fixtures live in `workflows/fixtures/`. Negative fixtures are as important as positive ones.
