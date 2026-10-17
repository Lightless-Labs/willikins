---
title: Rollback over MCP and on the approvals page
created: 2026-10-16
status: pending
priority: medium
area: server
related:
  - docs/plans/2026-10-05-milestone-3m-safe-rollback.md
  - crates/willikins-server/src/butler.rs
---

# Rollback over MCP and on the approvals page

Milestone 3m gave the CLI `willikins rollback`; its "Out of scope" left the MCP surface for later. `Butler` already
has `plan_rollback` and `apply_rollback`, and `decide` already resolves a teardown's `plan_id`. Missing:

- An agent-callable `rollback_plan` tool (safe: a teardown always needs a human's approval) and its schema in the
  tool list snapshot.
- A teardown-shaped rendering on the approvals page: steps in order, uncertain deletes apart, rides with, already
  gone, kept by the document, cannot be undone, made by hand; identifiers masked as everywhere else.
- `run_status` for a teardown run (`TeardownStarted` / `TeardownStepFinished` / `TeardownFinished`).

## Done when

An agent can plan a rollback over MCP, a human approves it on the approvals page seeing every step, and the agent
applies it and polls it to completion, with no value or full identifier in any MCP result.
