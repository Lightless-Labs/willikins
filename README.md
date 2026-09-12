# Willikins

A provisioning butler. An agent, over MCP or the CLI, authors and runs typed, composable
project-provisioning workflows against GitHub, Doppler, Buildkite, Railway, and App Store
Connect, and seeds new repositories from templates. The agent never touches a secret.

The agent plans; the butler acts.

## Status

Pre-alpha. Milestone 1 is complete: a YAML workflow is parsed into a typed graph, statically
checked, described to an agent, and planned against fake providers, with a CLI that mirrors
the milestone 2 MCP surface. Nothing talks to a real API yet. Two adversarial passes found
no path for a secret byte to reach any output.

See `docs/plans/2026-09-11-willikins-design.md` for the design,
`docs/plans/2026-09-11-milestone-1-core.md` for the milestone record, and `todos/` for what
is known and not yet done.

## Try it

```
cargo run -p willikins-cli -- validate workflows/new-rust-service.yaml
cargo run -p willikins-cli -- describe workflows/new-rust-service.yaml
cargo run -p willikins-cli -- plan workflows/new-rust-service.yaml \
  --input slug=third-thoughts --input org=lightless-labs
cargo run -p willikins-cli -- --json plan workflows/fixtures/secret-get.yaml \
  --input project=widgets --fake-state workflows/fixtures/state/secret-seeded.json
cargo run -p willikins-cli -- schema --document
cargo run -p willikins-cli -- propose-slug "Third Thoughts"
```

Gates: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `cargo check -p willikins-types`.

## Principles

- Every value crossing a tool boundary is a nominal domain type. Secrecy is a property of
  the type, and a secret can only flow into a sink that accepts secrets. This is checked
  statically before anything runs.
- Tools are idempotent by natural key and carry a reversibility class. Plans are computed
  from observed state and gated for approval before apply.
- Workflows are tools: same typed interface, so they compose.
- Provider names derive from a frozen project slug through versioned pure functions.
  Collisions are resolved by recorded overrides, never by auto-suffixing.

## License

MIT. See `LICENSE`.
