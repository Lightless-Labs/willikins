---
title: "Replace the outputs/for_each sentinel sites in CheckError with a site enum"
created: 2026-09-12
status: open
priority: medium
area: core
related:
  - docs/research/2026-09-12-check-adversarial-pass-1.md
---

# Replace the outputs/for_each sentinel sites in CheckError with a site enum

Errors on a workflow output binding use the sentinel node name `outputs`, and errors on a
`for_each` binding use the sentinel port name `for_each`. Three bugs so far came from the
collision with real nodes or ports of those names. Introduce `Site::{Port { node, port },
ForEach { node }, Output { name } }` and change every variant's fields accordingly.

**2026-09-12:** scheduled as task 1b of
`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`, decoupled from composition
(which moved to its own plan) because the MCP `validate` result needs unambiguous sites
regardless. Acceptance test 16 there pins it.
