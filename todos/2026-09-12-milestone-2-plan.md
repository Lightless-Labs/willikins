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
same day (20 findings folded in, stamped in the plan header). **2026-09-13:** tasks 0, 1a–1e, 2 and 3 landed on `main` (two Workflows; every task
opus-verified except 1e and 2). Next: task 4 (executor), then 5, 6, sequentially. This todo
closes when the plan is marked Completed.

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
