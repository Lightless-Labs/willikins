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

   **Further resolved by task 11's library step (2026-09-15).** The other two nested-list
   variants named alongside this item get the same per-element treatment
   `ValidateResponse` (item 5) already has: `ButlerError::Check.errors` (`Vec<CheckError>`)
   and `ButlerError::Input.errors` (`Vec<InputError>`) each serialize through
   `crate::read_ops::serialize_reported` (now `pub(crate)`, reused rather than
   reimplemented), pinned by `check_errors_each_carry_kind_and_message` and
   `input_errors_each_carry_message_and_the_outer_object_still_carries_kind` in
   `crates/willikins-server/src/error.rs`. `CheckError` is itself an internally tagged
   enum, so its elements carry `kind` *and* `message`, same as `ValidateResponse.errors`.
   `InputError` is a plain struct with no internal tag -- there was no `kind` to begin
   with -- so its elements gain only `message`; a new `impl fmt::Display for InputError`
   in `crates/willikins-core/src/describe.rs` (`"input `{input}`: {error}"`) is what
   `Reported` needed to wrap it at all. `ButlerError::Input.missing` (`Vec<MissingInput>`)
   is untouched: the task narrowing this work named `errors` on both variants, not
   `missing`, and `MissingInput` is a *successful*-response shape elsewhere (item 1), not
   an error list. `ButlerError` itself derives no `JsonSchema` (only `Serialize`), so
   there is no generated schema to update to match, unlike `ValidateResponse`'s.

   **The literal collision this item names is still open**, and is a `willikins-dsl` wire
   change, not a `willikins-server`/`willikins-core` one: renaming
   `DocumentErrorKind::{Yaml,Semantic}`'s `message` field so `Reported<DocumentError>`
   can be built directly would touch every fixture header quoting the exact error, the
   CLI's own document-error JSON, and the DSL's schema snapshots -- out of scope for task
   11's three named library changes (willikins-journal, willikins-core, willikins-server).
   Left for whoever next touches `willikins-dsl`'s error shape.

   **Measured by adversarial pass 2 (2026-09-15): the collision is real in the type system
   and unreachable on every surface that ships.** A quote- and escape-aware duplicate-key
   scan over the *raw* response bytes, at every nesting level (not only the top one task
   11's helper checked), was run over ten provoked errors across the whole MCP surface --
   malformed YAML, a semantic document error, an over-large document, an unknown workflow,
   a rejected input, a missing input, an unknown plan, an unknown run, an invalid project
   name, and a document that fails `check`. None carries a duplicate key at any level,
   because `DocumentError` is never itself wrapped in `Reported`: it reaches a caller as
   the *value* of `ButlerError::Document`'s `error` field, so its own flattened `message`
   sits one level below the one `Reported` adds. The scan is
   `no_error_any_mcp_surface_emits_carries_a_duplicate_key` in
   `crates/willikins-server/tests/adversarial_13.rs` and is the guard that says so; the
   rename stays worth doing the next time `willikins-dsl`'s error shape is open, but it is
   no longer blocking anything. See
   `docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`, finding 14.
3. **Closed on the library side by task 11 (2026-09-15).** `willikins-cli`'s
   `check_error_variant_name` duplicated `CheckError`'s own variant names by hand, one
   `match` arm per variant, with nothing forcing it to stay in step with the enum. Added
   `willikins_core::CheckError::kind(&self) -> &'static str` (the enum naming its own
   variants once), pinned against both the existing test macro's `check_error_kind_of` and
   the serialized `"kind"` tag by `kind_agrees_with_the_serialized_tag_for_every_variant`
   in `crates/willikins-core/src/check.rs`. **Closed by task 11's second step
   (2026-09-15):** `check_error_variant_name` is deleted from
   `crates/willikins-cli/src/render.rs`; `check_error_line` now calls `error.kind()`
   directly, so the CLI's text output can no longer drift from `CheckError`'s own variant
   names.
4. **`Butler::propose_slug` resolved by task 10a**: it returns
   `willikins_server::ProposeSlugResponse { slug }` or a kind-tagged `ButlerError`
   (`InvalidProjectName`/`SlugProposal`, both walked by the same variant test as item 2),
   through `willikins_types::propose_slug` exactly as the CLI does -- `ProposeError` itself
   also gained a `kind` tag (`crates/willikins-types/src/propose.rs`) so this wrapping needs
   no bespoke mapping. **Closed by task 11's second step (2026-09-15):** `willikins
   --json propose-slug` now prints `{"slug": "..."}` on success (matching
   `ProposeSlugResponse`) and, on failure, builds the same `ButlerError` variant
   `Butler::propose_slug` itself would return and prints it through `Reported` -- no
   `Butler` instance needed, since both variants are constructed directly from the same
   parse the subcommand already ran. `crates/willikins-cli/tests/acceptance_11_parity.rs`'s
   module doc (and its one propose-slug test, which never used `--json`) needed no change.

5. **Closed by task 10b (2026-09-14).** `willikins_server::ValidateResponse.errors` and
   `.warnings` now serialize each element through `willikins_core::Reported` on the wire
   (`#[serde(serialize_with = "serialize_reported")]` per field in
   `crates/willikins-server/src/read_ops.rs`, with a matching `schema_with` so the
   generated JSON schema requires `message` too), so every element carries `kind` *and*
   `message`, exactly as the CLI's own `check_errors_json`/`check_warnings_json` do. The
   field types themselves (`Vec<CheckError>`/`Vec<CheckWarning>`) are unchanged; only the
   struct's own `Serialize` output differs, so code that serializes a field's `Vec` on its
   own (bypassing `ValidateResponse`'s derived `Serialize`) still gets the bare shape --
   `crates/willikins-cli/tests/acceptance_11_parity.rs`'s `check_failure_parity` now
   serializes the whole response and reads `["errors"]` back out of it, and pins the two
   surfaces' JSON equal rather than equal-except-`message`. The plan's task 10a addendum
   records this decision.

Naming questions from the task 1d verifier, for task 1e or 10a to settle:

- `Plan.workflow` is document-authored text published as `workflow`, not `document_name`
  as trust boundary 4's wording suggests; `MissingInput.default` is document-authored and
  not named `document_*` (always `None` today).
- `Description.resolved` and every `Plan` value mix document defaults and literals with
  caller-supplied values without provenance. Values-as-values is the plan's intent; record
  the decision.
- No snapshot pins `describe`'s JSON field names; the `Description` schema snapshot from
  task 1a should be checked to carry `document_description`.
