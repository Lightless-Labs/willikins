# Willikins

A provisioning butler. An agent, over MCP or the CLI, authors and runs typed, composable
project-provisioning workflows against GitHub, Doppler, Buildkite, Railway, and App Store
Connect, and seeds new repositories from templates. The agent never touches a secret.

The agent plans; the butler acts.

## Status

Pre-alpha. Milestone 1 (typed core, checker, planner, fake providers) is in progress.
See `docs/plans/2026-09-11-willikins-design.md` for the design and
`docs/plans/2026-09-11-milestone-1-core.md` for the current milestone.

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
