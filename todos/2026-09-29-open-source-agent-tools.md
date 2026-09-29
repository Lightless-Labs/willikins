---
title: Provision the operator's two open-source, self-hostable tools for AI agents
created: 2026-09-29
status: pending
priority: medium
area: workflows
related:
  - workflows/new-rust-service-buildkite.yaml
  - docs/research/2026-09-20-project-survey-and-workflow-library.md
  - todos/2026-09-20-remote-mcp-from-a-phone.md
---

# Provision the two open-source agent tools

Two of the operator's upcoming projects (named 2026-09-20) are open-source, self-hostable tools
for AI agents: a CLI plus an OAuth remote MCP server, perhaps a web app. Their shape is close to
what `workflows/new-rust-service-buildkite.yaml` already provisions (repository, Doppler project and
configs, Buildkite pipeline), so most of the work is documents, not tools.

What is likely new: inheriting from the operator's shared base configs the way Sample does
(`github/lightless-labs`, `open-telemetry/prd_signoz`); gates for the steps willikins cannot do yet
(repository files, deploy target setup); and possibly Railway for hosting, which willikins does not
provision today. Settle each tool's exact shape with the operator before planning. Prove it the
way milestone 3e proved Sample: a read-only `plan --live`, then a real apply only with the
operator's go-ahead.
