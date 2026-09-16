---
title: "Milestone 2: real providers, apply, ledger, MCP server (tracking todo)"
created: 2026-09-12
status: open
priority: high
area: planning
related:
  - docs/HANDOFF.md
  - docs/plans/2026-09-11-willikins-design.md
  - docs/plans/2026-09-11-milestone-1-core.md
---

# Milestone 2: write the plan, then build it

**2026-09-12:** plan drafted at `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`
with research in `docs/research/2026-09-12-m2-dependencies.md`. Scope item 6 (composition)
moved to its own future plan (design doc, milestone 2b); the site enum stayed in as task 1b.
Two additions to the scope below: `doppler.service_token.rotate` and a second positive
fixture, because the convergence claim is otherwise false for a minted token. Reviewed the
same day (20 findings folded in, stamped in the plan header).

**Progress.** 2026-09-13: tasks 0, 1a–1e, 2 and 3 landed on `main` (two Workflows; every task
opus-verified except 1e and 2). 2026-09-14: tasks 4, 5 and 6 (the executor's approval gate
reads the checked class; the journal replays through `Redacted<T>`; the HTTP client labels
provider text and caps `Retry-After`), then 7 and 8 (the live GitHub and Doppler providers;
the GitHub read-only probe ran against the sandbox org, the Doppler one waits for a
service-account token), then 9 (adversarial pass 1 over the executor, the journal and the
approval gate, recorded in `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`:
no attack reached a secret byte, one defect fixed, and the boundaries core leaves to the
server are `boundary_` tests plus `todos/2026-09-14-pass-1-items-for-task-10a.md`).

2026-09-14 (later): both live write cycles ran against the operator's sandbox org and
dedicated Doppler workplace (a repository and a project created, converged, and deleted through
the real tools; the Doppler one found and fixed two defects), then task 10a landed in two halves
and an Opus verify fixed a decision race, an unjournaled refusal and a read outside the lock.

2026-09-15: task 10b landed (restart-safe plans, `Reported` validate errors, the rmcp tool
surface over stdio, the Streamable HTTP transport with bearer and Basic auth, the approvals
page) and its Opus verify fixed a `Debug` hash leak, a zero-rate-limit panic, the approvals
body cap and the fake announcement over HTTP. The operator created the Railway project and
decided on no public domain until the auth path matures.

2026-09-15 (midday): task 11 landed (read-only journal replay, the CLI's apply, approve, reject,
runs, run, serve, `--live`, `--plan-id`, `--fake-state-out`, renderers) and its Opus verify fixed a
duplicate `message` key in every self-describing error and an unloadable fake-state dump.

2026-09-15 (afternoon): task 12 landed: the root Dockerfile builds and runs on Railway with the
fake catalog, the volume and the non-secret variables are set, `hash-token` and
`WILLIKINS_FAKE_CATALOG` exist, `deploy/teardown.sh` is tested against the real CLI's output, the
README has a Deploy section; the verify fixed the script's failed-run and refused-delete paths.

2026-09-15 (evening): task 13 landed: adversarial pass 2 over TCP against the real binary held (no
secret byte, no forged approval, no unapproved run, no escape); it fixed the template amplification
and the blocking-pool exhaustion, corrected three journal words additively behind a frozen
pre-change journal, and its completeness critic turned thin proofs into real ones without touching
source (`docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`). The Railway settings file was
regenerated, planned, read by the operator and applied; the redeploy passed `/healthz` live.

2026-09-16: task 14 part A landed: both credentials probed alive, acceptance test 18 written as the
opt-in `live_smoke` test plus its fake-catalog twin `smoke_parity` (in every gate); two plan words
corrected (root configs `Unchanged` on the first apply, `ci_secret` `Converged` on the second). Part
B waits on the operator granting `gh` the `delete_repo` scope the teardown script needs.

2026-09-16 (later): the milestone 2c plan is drafted and reviewed ahead of time
(`docs/plans/2026-09-16-milestone-2c-authorization.md`, research note
`docs/research/2026-09-16-m2c-authorization.md`); its two open decisions (identity provider, public
host) are the operator's.

**Next:** 14 part B (the live run in one command, teardown, leftover check, plan Completed). This
todo closes when the plan is marked Completed; milestone 2c then gets its own tracking todo.

## First action of the next session

Write `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md` with `Created`, `Design`,
and `Previous` headers, run the document-review workflow on it (scope, feasibility, security,
coherence, adversarial personas), fold the findings in, and stamp `Reviewed`. Only then
dispatch implementation.

## Scope (from the design doc's milestone list and the handoff runbook)

1. Real GitHub and Doppler providers behind the unchanged `Tool` contract and port table.
   Credentials held server-side; provider auth is execution context, never an input.
   Research first: GitHub REST for repos and Actions secrets (secrets need the repo public
   key and libsodium sealed boxes), Doppler v3 for projects, configs, service tokens,
   secrets.
2. `apply`: the executor is the only non-test `SinkToken` site. Approval gate before any
   node above `Reversible`. Run ledger with per-node status. Re-running a partially failed
   plan converges.
3. MCP server on `rmcp` 3 over stdio and Streamable HTTP: `validate`, `describe`, `plan`,
   `apply`, `list_tools`, `propose_slug`. The CLI already mirrors these one to one.
4. Authentication, TLS, append-only audit log, Railway deployment.
5. Prerequisites gated by other todos: scalar-alias amplification before accepting documents
   from agents; `CheckError` serialization for the `validate` tool; document-text labelling
   in agent-facing output; keyword-list verification before the first real run.
6. Composition: `Workflow` implements `Tool` with typed composite output ports; replace the
   `CheckError` sentinel sites with a site enum at the same time.

## Verify with a browser before relying on them

The current MCP authorization spec for HTTP transports, `rmcp` 3's Streamable HTTP server
API and its `axum` requirement, the GitHub Actions secrets encryption flow, Doppler's
service-token creation response shape.
