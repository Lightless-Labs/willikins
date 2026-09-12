# Workflow name and description have no domain type

**Filed:** 2026-09-12 (end-to-end adversarial pass 2)

`Workflow::name` and `Workflow::description` are bare `String`s while every tool port is a
domain type. A 200,000-character `name:` is echoed verbatim by `plan --json`. Give them
domain types (`WorkflowName` as a slug-like grammar, `Description` bounded like `Text`) and
apply the same to `InputSpec::description`.
