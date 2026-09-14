# Willikins

Willikins is a provisioning butler. An agent writes typed project-provisioning workflows and
runs them. The agent does this through the Model Context Protocol (MCP) or through the CLI.
The workflows compose. A secret never reaches the agent.

The agent makes the plan. The butler does the work.

## Status

Willikins is pre-alpha.

Milestone 1 is complete. Willikins parses a YAML workflow into a typed graph. Willikins then
does a static check of the graph, describes it to an agent, and makes a plan against fake
providers. The CLI has the same commands as the milestone 2 MCP tools.

Milestone 2 is not complete. These parts are done:

- The apply executor. The executor asks a human to approve a plan, and it refuses a plan
  that shows drift.
- The append-only JSONL journal.
- The HTTP client with redaction.
- The live GitHub and Doppler providers.
- 1 live write cycle against a sandbox GitHub organization. The cycle created a repository,
  converged it, put a sealed secret in it, and then deleted the repository.

These parts are not done:

- The server library.
- The MCP server over stdio and Streamable HTTP.
- The CLI apply commands.

Milestone 1 had 2 adversarial passes. Milestone 2 has 1 adversarial pass. No pass found a path
for a secret byte to reach an output.

Read these documents for more information:

- `docs/plans/2026-09-11-willikins-design.md` gives the design.
- `docs/plans/2026-09-11-milestone-1-core.md` and
  `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md` give the milestone records.
- `todos/` lists the known open work.
- `docs/HANDOFF.md` gives the current state.

## Providers

GitHub and Doppler have live tools:

- GitHub: the repository and the Actions secret.
- Doppler: the project, the config, the service token, the service token rotation, and the
  secret read.

Buildkite is not live. Milestone 3 adds a Buildkite provider that makes 1 pipeline for each
repository. Railway and App Store Connect are possible providers after milestone 3.

## Try it

Run the CLI:

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

Run these 4 gates before each commit:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check -p willikins-types`

## Principles

- Each value that crosses a tool boundary is a nominal domain type. Each type is secret or
  not secret. A secret can only go into a sink that accepts secrets. Willikins does this
  static check before the workflow runs.
- Each tool is idempotent by its natural key. Each tool has a reversibility class. Willikins
  makes a plan from the observed state. A human must approve a plan before the butler
  applies it.
- A workflow is also a tool. Workflows compose because a workflow has the same typed
  interface as a tool.
- Willikins derives each provider name from a frozen project slug. The functions that derive
  the names are pure and have a version. A recorded override solves a name collision.
  Willikins does not add a suffix to make a name unique.

## License

Willikins uses the GNU Affero General Public License, version 3 or later
(`AGPL-3.0-or-later`). Read the `LICENSE` file. The source code is at
<https://github.com/Lightless-Labs/willikins>.
