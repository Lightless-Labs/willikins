# Milestone 3g: adversarial pass — engine and types (E1, E2)

**Date:** 2026-09-30
**Task:** the independent attacker's pass over the "Engine and types" group of
`docs/plans/2026-09-30-milestone-3g-file-writing.md`. The attacker wrote none of it.
**Subject:** E1 — `9a8a0bb` (list bindings: parse and check), `f861b6f` (resolve and deliver each element
at plan and apply; list elements in the skip scan), `6e42750` (mcp_server schema snapshot). E2 — `87612aa`
(`RepoPath`, `GitBranchName`, `CommitHeadline`, `RepoFile`, `TemplateValue`, the
`AppleBundleIdentifier => TemplateValue` row), `fa17bf5` (`DisallowedInputType`, `RepoFileLiteral`),
`1f2a6a0` (`GitBranchName` refuses a trailing dot).
**Method:** read the plan in full, including its addenda, then the six commits and the code they touch
(`willikins-dsl` `document.rs`/`lib.rs`; `willikins-core` `check.rs`, `plan.rs`, `apply.rs`, `value.rs`,
`site.rs`; `willikins-types` `repo.rs`, `text.rs`, `registry.rs`, `lib.rs`), and every `Binding::` match
outside tests. Then mutations. Every mutation followed the same steps: copy the file into the pass's
scratchpad, apply one exact-text replacement (a script asserted it occurred exactly once), run the
narrowest test target, copy the saved file back, `touch` it, and confirm byte identity with `cmp` (exit 0
every time). Never `git checkout`, `reset` or `stash`. **No live call and no provider API was made.**
Nothing in this group talks to a provider.

## 0. The state the pass started from

`main` at `1f2a6a0`, clean apart from `goal.txt` and the host-maintenance todo (both left alone). Nothing
uncommitted, nothing red. Baseline, scoped: `cargo test -p willikins-types` (10 suites, 0 failed),
`cargo test -p willikins-core` (22 suites, 0 failed), `cargo test -p willikins-dsl --test acceptance` (5/5,
including `characterization_of_every_document`).

**Every existing document plans byte-identically.** `git diff aa555fe 1f2a6a0` touches nothing under
`workflows/` or `crates/willikins-dsl/tests/` (the characterization snapshot included), and the
characterization test passes against it with no `.snap.new` anywhere in the tree. So E1 and E2 changed
no existing document's `check` or `plan` output. `Binding`'s serde form for the old variants is unchanged
(the new variant serialises as `kind: list`).

## 1. The questions, and what the evidence says

### Can a secret or a `Text` reach a `RepoFile`? Not through anything E1 or E2 built

The routes into a committed file are the `RepoFile` type itself, the `TemplateSource` that renders it,
and the `TemplateValue`s substituted into it. Each route checked:

| Route | Wall | Proof |
| --- | --- | --- |
| A workflow input of type `RepoFile` or `TemplateSource`, with or without a default | `DisallowedInputType`, by `TypeId` | existing E2 tests |
| The same as `list<RepoFile>` / `list<TemplateSource>` (also a `for_each` source) | the same branch, because it keys on the element name | **new** tests; mutation M6 below survived without them |
| A literal on a `RepoFile` port, scalar or as a list element | `RepoFileLiteral` | existing E2 tests; mutation M4 |
| A secret as a `values` element | `SecretToNonSecretSink` at `node.values[i]`, before the type mismatch | existing tests; mutation M5 |
| `Text` (for example `doppler.value.get`'s public value) into `TemplateValue` | no `Text => TemplateValue` row, pinned | existing E2 registry test |
| `Text` into `TemplateSource` or anything into `RepoFile`, through a conversion | no row may target either | **new** registry test; mutation M8 below compiled and survived without it |
| A list-typed reference as an element (flattening) | `ListElementTypeMismatch` | existing E1 tests |
| A `for_each` item as an element | its source is an input or step output; a secret source is refused already, and `list<RepoFile>` inputs are refused | reading `check.rs` |
| A YAML alias expanding one value many times | `parse_document` refuses every anchor and alias before `serde_yaml_ng` runs | reading `willikins-dsl/src/lib.rs` |

Mutation M8 matters: `impl From<Text> for TemplateSource` plus a `Text => TemplateSource` row compiles
(both are public, both are bounded at 65,536 characters, so the `From` is total), and every test that
existed passed. It would have let a public `Text` read from Doppler become a template, and so the bytes
of a committed file. The new test closes that.

### Can a `TemplateValue` break out of YAML, JSON, plist, Starlark or bash? Not by its grammar

`TemplateValue` is `[A-Za-z0-9_][A-Za-z0-9._/-]*`, at most 255 characters, anchored as `^(?:…)$` by the
derive (the `regex` crate's `$` without the `m` flag matches only at the end of the text, so a trailing
newline cannot slip past). No quote, backslash, `$`, backtick, `#`, `:`, `<`, `>`, `&`, `{`, `}`,
whitespace or control character, and a first character that is never `-`, `.` or `/`. So a value
cannot end a quoted string in any of the five syntaxes, start a comment, a flag, an XML tag or an entity,
or inject a placeholder.

The E2 test for this sampled only decision (e)'s listed characters. Mutation M7 added `#` to the class
(a YAML and shell comment) and every one of the 13 existing `TemplateValue` tests, the proptests
included, still passed. The **new** test checks every ASCII character and a sample of non-ASCII ones,
in the first and a later position, against the grammar's exact class.

What the grammar does **not** stop, recorded for T1 and W1 (section 3): a value in a *command* or
*path* position of a template (`x/../../y`, `curl`), and a YAML-typed plain scalar (`true`, `null`, `1e3`)
where the template leaves the placeholder unquoted.

### Do list bindings keep the plan-and-apply rules? Yes

- **Skip scan.** `GateTracking::collect_causes` recurses into list elements, so a node whose element
  names a blocked gate plans `Skip` and is never read. Mutation M1 kills it.
- **Unknown propagation.** One `Unknown` element makes the whole value `Unknown` at `list<T>`. Mutation
  M3 kills it (in `plan` and in `apply`).
- **Apply re-resolution.** `binding_may_change_mid_run` re-resolves a list holding a `Step` or `Keyed`
  element against this run's own outputs, delivering each element through its own edge. Mutation M2
  kills it.
- **Plan-to-apply drift.** A list with only `Input`, `Item` and `Literal` elements is never re-resolved,
  exactly like the scalar rule; a list with a `Step` element moves from `Unknown` at plan to known at
  apply, exactly like a scalar `Step`. Nothing list-specific adds drift: the value and fingerprint come
  from the same per-element edges `check` recorded, and `plan`/`apply` never probe the conversion table.

### The scaffold overwriting anything, a moved head, and the stored pipeline configuration

These priorities belong to G1, G2 and B1, none of which has started. Nothing in E1 or E2 writes to a
provider, reads a head or touches Buildkite. What E2 contributes is correct as far as it goes:
`RepoPath` refuses `.git` segments and `.github/workflows/` (any case), `.`/`..` segments, empty
segments, a backslash and control characters; `GitBranchName` refuses `..`, `//`, `@{`, a leading `/`,
`-` or `.`, a trailing `/` or `.`, and a component starting with `.` or ending in `.lock`; `RepoFile`
never normalises its content, so G1's local blob sha can match byte for byte.

## 2. Mutations

| # | File | Mutation | Result | Killed by |
| --- | --- | --- | --- | --- |
| M1 | `willikins-core/src/plan.rs` | `collect_causes` ignores `Binding::List` | killed | `plan_gates::a_list_element_binding_on_a_blocked_gate_is_skipped_and_never_read` |
| M2 | `willikins-core/src/apply.rs` | `binding_may_change_mid_run` is `false` for a list | killed | `apply::conversions::an_unknown_list_element_plans_unknown_list_and_ensures_known_list` |
| M3 | `willikins-core/src/plan.rs` | `resolve_list_binding` drops an unknown element instead of making the list unknown | killed | `plan::list_binding_with_one_unknown_element_resolves_to_an_unknown_list` and the apply test above |
| M4 | `willikins-core/src/check.rs` | `check_list_literal` without the `RepoFile` refusal | killed; `check` then **accepted** a literal file as a scaffold element | `check::e2_a_literal_cannot_supply_a_repo_file_list_element` |
| M5 | `willikins-core/src/check.rs` | `check_list_element` without its taint check | killed (the secret is still refused, as a type mismatch, so this is defence in depth) | `check::a_secret_element_in_a_list_is_a_taint_violation`, `check::e2_a_secret_value_as_a_repo_file_render_values_element_is_exactly_one_taint_error` |
| M6 | `willikins-core/src/check.rs` | `DisallowedInputType` narrowed to scalar inputs (`!spec.ty.list && …`) | **survived** all seven existing E2 tests; a caller-supplied `list<RepoFile>` then checked clean bound to `github.scaffold.ensure.files` | new: `check::e2_a_list_of_template_source_workflow_input_is_rejected`, `check::e2_a_list_of_repo_file_workflow_input_bound_to_the_scaffold_is_rejected` |
| M7 | `willikins-types/src/text.rs` | `TemplateValue`'s later class gains `#` | **survived** all 13 existing `TemplateValue` tests | new: `text::tests::template_value_admits_exactly_its_grammar_character_by_character` |
| M8 | `willikins-types/src/lib.rs`, `text.rs` | `impl From<Text> for TemplateSource` and a `Text => TemplateSource` row | compiled; **survived** the existing registry pin | new: `registry::tests::the_production_registry_has_no_conversion_row_into_template_source_or_repo_file` |

Each of M6, M7 and M8 ran with the new test already in place, so one run showed the existing tests
passing and the new test failing. Every restore was confirmed with `cmp`. M7's restore reverted the one
pattern line and kept the new test; `diff` against the saved original showed additions only.

## 3. Findings

No real defect in E1 or E2: nothing lets a secret, a `Text` or a caller's command reach a committed
file today. Three test gaps (M6, M7, M8) are closed by tests in this pass. Recorded, not fixed:

1. **`RepoFileLiteral` has no element index.** A refused list element reports `node`/`port`, not the
   `node.port[i]` site decision (a) specifies for list-element errors. It is a precision gap, not a hole.
   Fixing it changes a `CheckError` variant's shape, which ripples into `willikins-cli/src/render.rs`
   and the mcp_server schema snapshot. Left for T1 or G2, whichever next touches that variant.
2. **`mark_blocked`'s `awaiting_inputs` sees only a top-level `Binding::Input`,** not one inside a list.
   It is the one `Binding` match in `plan.rs` that was not made list-aware. Unreachable today, since no
   gate has a `list<OperatorAcknowledgement>` port. If one ever does, its unsupplied acknowledgements
   will be missing from the blocked report.
3. **The `TemplateValue` grammar protects quoting, not placement.** A `TemplateValue` may be a workflow
   *input* (Walter binds its bundle identifiers that way). It may contain `/` and `..`, and it may be
   a bare word. So if a template puts a placeholder in a command position (`{{ 0 }} --flag`), a path
   position (`bash apps/{{ 0 }}/x.sh` with `x/../../../tmp`), or an unquoted YAML plain scalar (`true`,
   `null`, `1e3`), the caller chooses the command, the path, or the YAML type. Decision (e)'s
   "every command in a committed file is document-literal" therefore depends on how the template is
   written, not on the type. For T1 and W1's attacker: every Walter placeholder should sit inside a
   quoted string or an identifier-only position. W1's rendered snapshots are where this is reviewed.
4. **A list with one `Unknown` element hides its known elements from the approver.** This is as
   decision (a) specifies. But decision (f) says the operator's plan approval *is* the diff review, and
   that holds only while `walter_files.files` is fully known at plan time. It is today, because every
   render takes literal templates and input identifiers. W1 should pin it with a test that on a first
   fake run `walter_files.files` is known at plan.
5. **Deferred fixtures remain.** E2's addendum deferred six YAML fixtures to T1/G2:
   `secret-into-repo-file.yaml`, a `RepoFile` literal on a list element, and the four tool-free
   `TemplateSource`/`RepoFile` input and default fixtures. The refusals themselves are proved in
   `crates/willikins-core/tests/check.rs`.

## 4. Commits

- `480f3c2` Pin that check refuses a list of TemplateSource or RepoFile as a workflow input (M6).
- `42af70d` Pin TemplateValue's character class character by character (M7).
- `aa009b7` Pin that no conversion row may target TemplateSource or RepoFile (M8).
- This record and the plan's dated addendum.
