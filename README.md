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
- 2 live write cycles. The GitHub cycle created a repository in a sandbox organization,
  converged it, put a sealed secret in it, and then deleted the repository. The Doppler
  cycle created a project in a test workplace, minted and rotated a token, read a secret,
  and then deleted the project.
- The server library and the MCP server over stdio and Streamable HTTP.
- The CLI commands apply, approve, reject, runs, run, and serve.
- A container image. Railway builds it and runs it with the fake catalog.

These parts are not done:

- Adversarial pass 2 over the HTTP surface.
- The live smoke run with sandbox credentials.

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

## Deploy

Willikins runs as one Docker image. The `Dockerfile` at the repository root builds it.
Railway builds and runs this image for the `willikins` service. The image's own command is
`serve --http`. Every other choice comes from an environment variable, so one image serves
every environment.

### Infrastructure as code

The file `.railway/railway.ts` describes the Railway project in code: the service, its GitHub
source, its five variables (kept with `preserve()`, never written into the file), the
healthcheck path `/healthz`, 1 replica, and the volume mounted at `/data`. The Railway CLI
(version 5.57.2 or later) generated the file from the live project on 2026-09-15; the
healthcheck is the only addition. To plan or apply it, install the Railway TypeScript SDK
from the repository root, then run the CLI:

```
npm install
railway config plan --verbose
railway config apply
```

The plan is read-only and redacts variable values. Do not pass `--show-values` or
`--decrypt-variables`. Railway's own rule for the file is "omit means delete": a resource or
field that the file does not name can go away when someone applies the file. If a plan shows
a delete, regenerate the file with `railway config pull --force`, add the healthcheck again,
and read the plan again before you apply it. On 2026-09-15 the plan showed one change, the
healthcheck, and nothing to destroy; the operator approved it and `railway config apply`
set the healthcheck. A new plan now reports the configuration as up to date.

### No public domain, on purpose

Milestone 2 uses static bearer tokens for authentication. A static bearer token is not
strong enough to protect a public endpoint. For this reason, the `willikins` service has no
public domain. Reach it only over Railway's private network, or run `serve --stdio` on a
trusted machine. Milestone 3 adds a stronger authentication path. The operator can add a
public domain once that path exists.

### The two provider credentials

The server needs two credentials to reach live providers: `WILLIKINS_GITHUB_TOKEN` and
`WILLIKINS_DOPPLER_TOKEN`. Doppler's own Railway integration is the only way these two
credentials reach the service. Set up this integration in the Railway dashboard. Point it at
the Doppler config that holds both tokens. Do not set either variable by hand in the Railway
dashboard or through the Railway CLI. A hand-set credential does not rotate when the Doppler
config changes.

### The fake-catalog variable

Set `WILLIKINS_FAKE_CATALOG=1` to make the server serve the fake, in-memory catalog. No
tool call reaches GitHub or Doppler in this mode. Use this variable to check that a fresh
deployment starts and answers `/healthz`, before the two real credentials exist. Remove the
variable once the two real credentials are in place. Any value other than `1` refuses to
start, and the refusal names the variable.

### Minting and hashing a real token

Willikins never stores a bearer token in the clear. It stores each token's SHA-256 hash, as
64 lower-case hex characters. Mint a token and hash it with this command:

```
openssl rand -hex 32 | willikins hash-token
```

`hash-token` reads the token from standard input. `hash-token` never takes the token as a
command-line argument. A command-line argument sits in `ps` output and in shell history for
as long as the process runs. Standard input leaves neither trace. Copy the printed hash into
`WILLIKINS_AGENT_TOKEN_HASHES` (a comma-separated list, for more than one agent) or
`WILLIKINS_APPROVER_TOKEN_HASH`. Give the real, unhashed token only to the agent or the
approver who will present it.

### The volume and the journal

The journal is one JSONL file. Set `WILLIKINS_JOURNAL_PATH=/data/journal.jsonl`. Railway
stores this file on one persistent volume, mounted at `/data`. A Railway service with a
volume runs at most one replica; Railway does not allow more.

### Recovery after a crash

A crash during a write can truncate the journal's last line. `willikins-server` checks
every line when it starts, including the last one. `willikins-server` refuses to start when
the last line is truncated. This refusal names the line number and the reason
(`JournalError::Corrupt`). The refusal is fail-closed by design: willikins never guesses at
a partial record, and it never drops a line on its own.

To recover, remove the truncated last line from the journal file, then start the server
again. The runtime image has no shell, so an operator cannot fix the file inside the running
container.

One way to reach the file: attach the same volume to a temporary debug service that has a
shell. Fix the file there. Then move the volume back to the `willikins` service.

Task 12 did not test this detour. The installed Railway CLI lists `volume detach` and
`volume attach` commands. Nobody ran them for this service. The `willikins` service keeps
its own restart policy (`ON_FAILURE`, 10 retries) while its volume is away, so it keeps
restarting and failing the whole time. Read Railway's own volume documentation first. Test
the detour once, on a disposable environment. Do this before an operator relies on it during
a real incident.

A `willikins` subcommand that repairs a truncated journal does not exist yet. It would close
this gap without the detour. See `todos/2026-09-15-journal-repair-subcommand.md`.

### Environment variables

| Variable | Default | Required when | Format |
| --- | --- | --- | --- |
| `WILLIKINS_WORKFLOWS_DIR` | none | Always. The image sets this; do not set it again. | A directory path. |
| `WILLIKINS_JOURNAL_PATH` | none | Always. | A file path. |
| `WILLIKINS_AGENT_TOKEN_HASHES` | empty | `serve --http`. | Comma-separated 64-character lower-case hex hashes. |
| `WILLIKINS_APPROVER_TOKEN_HASH` | none | `serve --http`. | One 64-character lower-case hex hash. |
| `WILLIKINS_ALLOWED_HOSTS` | empty | `serve --http`. | Comma-separated hostnames. On Railway, the private-domain reference (see the Railway docs on variable references), not a resolved hostname. |
| `PORT` | none | `serve --http` with no `--bind`. Railway sets this. | A port number. |
| `WILLIKINS_APPROVAL_WINDOW_SECONDS` | `86400` (24 hours) | Never; optional. | A whole number of seconds. |
| `WILLIKINS_PLAN_TTL_SECONDS` | `3600` (1 hour) | Never; optional. | A whole number of seconds. |
| `WILLIKINS_PLAN_RATE_PER_MINUTE` | `10` | Never; optional. | A whole number. |
| `WILLIKINS_READ_RATE_PER_MINUTE` | `60` | Never; optional. | A whole number. |
| `WILLIKINS_GITHUB_TOKEN` | none | Live mode only (`WILLIKINS_FAKE_CATALOG` unset). | `github_pat_...` (fine-grained) or `ghp_...` (classic). |
| `WILLIKINS_DOPPLER_TOKEN` | none | Live mode only. | `dp.sa.<40-44 characters>` (service account) or `dp.pt.<40-44 characters>` (personal). |
| `WILLIKINS_FAKE_CATALOG` | unset | Never; optional. | Exactly `1`, or unset. Any other value refuses to start. |

These variables exist only for the opt-in live tests run by hand during development. Never
set any of them on a deployed service: `WILLIKINS_LIVE_TESTS`, `WILLIKINS_LIVE_PROBE`,
`WILLIKINS_LIVE_LEFTOVER_CHECK`.

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
