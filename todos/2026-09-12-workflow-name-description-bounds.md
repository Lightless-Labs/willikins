---
title: "Workflow name and description have no domain type"
created: 2026-09-12
status: open
priority: low
area: types
related:
  - docs/research/2026-09-12-e2e-adversarial-pass-2.md
---

# Workflow name and description have no domain type

`Workflow::name` and `Workflow::description` are bare `String`s while every tool port is a
domain type. A 200,000-character `name:` is echoed verbatim by `plan --json`. Give them
domain types (`WorkflowName` as a slug-like grammar, `Description` bounded like `Text`) and
apply the same to `InputSpec::description`.
