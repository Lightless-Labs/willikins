---
title: "Close the gaps in the uniform {kind, message} error JSON before the MCP surface ships"
created: 2026-09-12
status: open
priority: medium
area: core, cli, dsl
related:
  - docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md
  - crates/willikins-core/src/reported.rs
  - crates/willikins-cli/src/render.rs
---

# Error JSON uniformity gaps found by the task 1a and 1b verifiers

Task 1a gave `CheckError`, `CheckWarning`, `PlanError` and `DocumentErrorKind` a `kind` tag and
`Reported<T>` adds `message`. Four places still fall short of "one JSON object shape", all
found by the verifiers and deliberately left for the task that owns them:

1. **Resolved by task 10a.** `InputError`/`MissingInput` are not an error at all:
   `willikins_server::Butler::describe` returns `willikins_core::Description` directly
   (never an `Err` on bad inputs), so `errors`/`missing` are result fields of a successful
   response, pinned by `describe_reports_missing_and_rejected_inputs_as_result_fields_not_errors`
   in `crates/willikins-server/tests/acceptance_read_ops.rs`. The CLI's own
   `willikins --json describe` output is unchanged (still `{input, error: {type_name,
   reason}}` for `InputError`, since `describe`'s `Description` type itself did not change)
   and is exactly what the parity test in `crates/willikins-cli/tests/acceptance_11_parity.rs`
   now pins `Butler::describe`'s JSON equal to.
2. **Partially resolved by task 10a, at the `Butler` boundary only.** Every
   `willikins_server::ButlerError` variant -- `Document { error: DocumentError }` among them
   -- now serializes with an outer `kind`/`message` through `Reported<ButlerError>`
   (`ButlerError::Display` for `Document` delegates to the inner `DocumentError`'s own
   `Display`), pinned by
   `every_variant_is_represented_and_serializes_with_its_kind_and_message` in
   `crates/willikins-server/src/error.rs`. The underlying collision this item names --
   `DocumentErrorKind`'s own `message` field not being its `Display`, so `Reported<
   DocumentError>` directly (not wrapped in a `ButlerError`) still cannot be built -- is
   untouched; still open for whichever of task 10b (the `validate` MCP tool's own result
   shape) or a `willikins-dsl` fix owns it.
3. Untouched; still `crates/willikins-cli/src/render.rs`'s own hand-maintained
   `check_error_variant_name`. Task 11 (CLI renderers) owns folding it.
4. **`Butler::propose_slug` resolved by task 10a**: it returns
   `willikins_server::ProposeSlugResponse { slug }` or a kind-tagged `ButlerError`
   (`InvalidProjectName`/`SlugProposal`, both walked by the same variant test as item 2),
   through `willikins_types::propose_slug` exactly as the CLI does -- `ProposeError` itself
   also gained a `kind` tag (`crates/willikins-types/src/propose.rs`) so this wrapping needs
   no bespoke mapping. The CLI's own `willikins --json propose-slug` subcommand is
   unchanged (still hand-built `{"error": ...}`/`{type_name, reason}`, and its *success*
   path ignores `--json` entirely and always prints plain text -- see
   `crates/willikins-cli/tests/acceptance_11_parity.rs`'s module doc). Task 11 owns aligning
   the CLI subcommand itself to `Butler::propose_slug`'s shape.

Naming questions from the task 1d verifier, for task 1e or 10a to settle:

- `Plan.workflow` is document-authored text published as `workflow`, not `document_name`
  as trust boundary 4's wording suggests; `MissingInput.default` is document-authored and
  not named `document_*` (always `None` today).
- `Description.resolved` and every `Plan` value mix document defaults and literals with
  caller-supplied values without provenance. Values-as-values is the plan's intent; record
  the decision.
- No snapshot pins `describe`'s JSON field names; the `Description` schema snapshot from
  task 1a should be checked to carry `document_description`.
