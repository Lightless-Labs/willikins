---
title: "Derive Serialize for CheckError and CheckWarning"
created: 2026-09-12
status: open
priority: medium
area: core
related:
  - crates/willikins-core/src/check.rs
  - crates/willikins-cli/src/render.rs
---

# Derive Serialize for CheckError and CheckWarning

Every other error and result type in `willikins-core` derives `Serialize`; `CheckError` and
`CheckWarning` do not, so the CLI hand-builds their JSON in `render.rs` as
`{"kind": "<Variant>", "message": "..."}`. The milestone 2 MCP `validate` tool needs the same
JSON and should not duplicate that code. Derive `Serialize` with the same externally tagged
PascalCase shape `PlanError` uses, then make the CLI use it, keeping the existing CLI tests as
the compatibility check.
