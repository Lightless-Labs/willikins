---
title: "Milestone 3o adversarial pass: E1/E2 (the AGENTS.md documents)"
created: 2026-10-06
status: complete
area: willikins-dsl, willikins-core (check, compose), willikins-providers-fake
related:
  - docs/plans/2026-10-06-milestone-3o-agents-md.md
  - docs/research/2026-09-30-m3g-adversarial-pass-render-and-write.md
  - docs/research/2026-10-06-m2b-adversarial-pass.md
---

# Milestone 3o adversarial pass: E1/E2

Task X1. Attacked E1 (`example-org-agents-md.yaml` + `standalone-service.yaml`) and E2
(`monorepo-app.yaml`), as landed in `75d8f0f` and `0f60aec`. The reviewer wrote none of the code
under attack. No provider was called; nothing ran against a live account. Placeholders only
(`example-org`, `Example-Org/monorepo`, `sample`) -- no operator name anywhere in this file or in
any fixture.

Targets, from the plan's X1 row: caller text becoming prose by any route (an input, a default, a
`with:` literal, a list element, a conversion); a parent supplying or replacing the organisation's
template (`TemplateSource`/`RepoFile` inputs, literal outputs, shadowing); the two scaffolds
sharing a marker or one listing the other's marker or files; a seed path outside the app (`..`,
`.git`, `.github/workflows`); an overwrite; `{{` smuggled through a value.

## Fixtures and where each is refused

| # | Fixture | Attempt | Refused at | Error | Pinning test |
| --- | --- | --- | --- | --- | --- |
| 1 | `project-prose-default.yaml` | a root `TemplateValue` input's own *default* is prose | `load_document` (`parse_default`) | `DocumentError` naming `inputs.project.default` | `project_prose_default_is_refused_at_load` |
| 2 | `project-prose-list-boundary.yaml` | a `uses: with:` value is a YAML sequence (`Binding::List`), which `check_boundaries` never checks at all | `check`, downstream of substitution, once nested inside the child's own `values:` list | `CheckError::ListElementTypeMismatch` (not a grammar error naming the prose -- see "Known gap" below) | `project_prose_list_boundary_is_refused_as_a_nested_list` |
| 3 | `uses-boundary-no-conversion.yaml` | binds `project: TemplateValue` to an `AppleBundleIdentifier`, a type a registered conversion row could deliver at an ordinary port | `check` (`check_boundaries`, exact match only, no conversion probe) | `CheckError::UsesInputTypeMismatch { expected: TemplateValue, found: AppleBundleIdentifier }` | `uses_boundary_no_conversion_is_refused_even_though_a_row_exists` |
| 4 | `uses-boundary-unknown-input.yaml` | a `uses:` step tries to bind `template` directly, an input the child never declared -- an attempt to replace the organisation's prose from the calling side | `link` (`compose::validate_uses_step`), before `check` ever runs | `CheckError::UnknownUsesInput { node: agents_md, input: template }` | `uses_boundary_unknown_input_refuses_a_template_override_attempt` |
| 5 | `malicious-used-document-template-source-input.yaml` + `uses-boundary-disallowed-type.yaml` | a used document declares its own template as a caller-supplied `TemplateSource` input | `check` (`check_boundaries` runs the same `check_input_spec` a root input goes through) | `CheckError::DisallowedInputType { input: "bad/template", ty: TemplateSource }` | `uses_boundary_disallowed_type_mirrors_the_root_check` |
| 6 | `example-org-agents-md-literal-output.yaml` | the organisation document exports its rendered text as a plain literal output instead of a `Step`-bound `RepoFile` (the alternative decision (d3) was chosen over) | `check` (`check_outputs`), as a root | `CheckError::LiteralOutput { output: file }` | `example_org_agents_md_literal_output_variant_fails_check_outputs` |
| 7 | `monorepo-app-path-escape.yaml` | `agents_md.path` set to `../../AGENTS.md`, escaping the app's own directory | `check` (`check_literal` parses the `with:` literal against `RepoPath`) | `CheckError::InvalidLiteral { node: agents_md, port: path }` | `monorepo_app_path_escape_is_refused_invalid_literal` |
| 8 | `monorepo-app-shared-marker.yaml` | `agents.marker` collapsed onto `app_files.marker` | **not refused** -- `check` and `plan` both pass cleanly | n/a: a real, silent finding, not a refusal (see below) | `monorepo_app_shared_marker_silently_drops_agents_md` |
| 9 | `monorepo-app-fake-marker-file.yaml` | the second scaffold lists a rendered file at the *first* scaffold's marker path, trying to overwrite it as an ordinary file | `plan` (`github.scaffold.ensure`'s own `observe()`, byte-content guard) | `ToolErrorKind::Conflict`, message names `apps/sample/.willikins-scaffold` | `monorepo_app_a_second_scaffold_cannot_overwrite_the_first_markers_path` |
| 10 | (reuses `standalone-service.yaml`) | a different `AGENTS.md` already present at a brand-new standalone repository's root | `plan`, **once the repository already exists** (see the asymmetry below) | `ToolErrorKind::Conflict`, message names `AGENTS.md` | `standalone_service_a_different_existing_agents_md_fails_plan_naming_the_path` |

Every route to "caller text becomes prose" traced back to one of three independent, pre-existing
facts, none added by 3o: `TemplateValue`'s grammar admits no space, quote, or brace at all;
exactly one conversion row targets it (`AppleBundleIdentifier => TemplateValue`), and a `uses:`
boundary never probes the conversion table regardless (row 3); and no `Text => TemplateValue` row
exists (pinned by `willikins-types`' own
`the_production_registry_has_no_text_to_template_value_conversion_row`). `TemplateSource` and
`RepoFile` cannot be caller inputs at all, root or boundary (`DisallowedInputType`, row 5), and
`RepoFile` has no legitimate literal (`RepoFileLiteral`, pre-existing, not re-tested here since no
new route to it exists in E1/E2).

## Known gap (not new, inherited from milestone 2b)

Row 2 is refused, but only by luck of the child's own shape. `check_boundaries`'s own doc comment
says plainly that a `Binding::List` at a `uses:` boundary is left unchecked: "every element --
literal or reference alike -- is copied unchanged by substitution and so reaches a real port
downstream with a real expected type of its own, *unless the used document never references this
input anywhere, in which case nothing types it at all.*" `example-org-agents-md`'s `project` input
is referenced exactly once, inside its own `values:` list, so the parent's one-element list ends
up nested inside the child's own one-element list, and `check_list_element`'s defensive
nested-list guard (never reached by `willikins-dsl` itself, which refuses a literal nested list at
parse time) catches it as `ListElementTypeMismatch` -- a type-shape error, not a grammar error
naming the prose. If a future used document declared an input it never referenced (not something
either real document does), a `uses:` step could bind that input to a `Binding::List` of
*anything*, unchecked, with no error at all. This is milestone 2b's own documented gap, not
introduced by 3o and out of scope for it (a new primitive the operator declined). Recorded here
because X1 asked for it, not as a 3o regression.

## A plan/apply asymmetry (github.scaffold.ensure, not E1/E2 specific)

Row 10 needed a second attempt. The first attempt seeded `FakeState` with a differing
`AGENTS.md` at the standalone repository's path but no repository record at all (plausible: the
document's own `repo` step creates it). `github.scaffold.ensure`'s `read()` maps
`BranchExistence::RepositoryAbsent` straight to `Observation::Absent` -- by design (milestone 3l,
decision (b): "so a document that creates its own repository in the same plan still plans
cleanly") -- **without ever consulting `state.scaffolds`**. So a pre-existing, differing file at a
seed path is invisible at `plan` time whenever the repository does not exist yet in the planner's
own snapshot, and would only surface once `ensure` actually runs, after the repository has been
created, at `apply`. Fixed by seeding the repository as already existing (matching the live
pattern this fixture exercises: a repository created in an earlier run, scaffolded in this one),
which makes row 10 comparable to row 9 and to the existing `monorepo_app_a_different_existing_...`
test. On the fake, "a repository with no record but existing scaffold content" is incoherent state
to begin with; live, the question is whether a freshly created repository can already hold content
at a seed path (a template repository, an auto-generated default branch file) -- the fake cannot
say, and nothing in 3o changes `github.scaffold.ensure`'s own read table. Noted as an open question
for milestone 3l, not for this plan.

## Trust assumptions (restated plainly, not new findings)

1. **The template is wholly document-trusted.** `TemplateSource`'s grammar is length-only (at most
   65,536 characters, no character pattern at all) -- `crates/willikins-types/src/text.rs`. Nothing
   about its *shape* is ever checked; only its origin (a literal in a trusted document, never an
   input or a conversion target) is.
2. **`message` is a literal in both documents.** `CommitHeadline` (the `message:` port on
   `github.scaffold.ensure`) is bound as an ordinary literal in every fixture here, parsed by its
   own grammar at `check` like any other port; no new route to it exists in E1/E2.
3. **A scaffold is bounded to the repository, never to a directory.** `RepoPath` refuses `..`,
   `.git`, and `.github/workflows/...`, but nothing bounds `apps/sample/`'s own scaffold to that
   prefix: `apps/other-app/AGENTS.md` is a perfectly valid `RepoPath` for either scaffold node in
   `monorepo-app.yaml`. This is not a bypass -- the document itself is trusted content, and the
   operator's own document author chooses every path -- but it is the honest answer to "a seed
   path outside the app": the type system bounds the *repository*, not the subtree.
4. **The `Binding::List` boundary gap is refused only because the child references the input.**
   Restated from "Known gap" above.

## Gates run

`cargo fmt --all --check` was red once, on two sites inside the test module added here (both
ordinary line-length wraps rustfmt prefers); `cargo fmt --all` fixed them and the committed file
is formatted. `cargo clippy -p willikins-cli --all-targets -j 2 -- -D warnings` was clean.
`cargo test -p willikins-cli --test it -j 2 -- agents_md` passed all 18. The plan's E1/E2 gate row
also names `test_layout_guard` beside that same `willikins-cli` run; it actually lives in
`willikins-core/tests/it/test_layout_guard.rs` (it walks the whole workspace tree, not one
crate's own), so it was run there instead: `cargo test -p willikins-core --test it -j 2
test_layout_guard`, 18 passed, including `the_real_workspace_tree_satisfies_the_layout_guard`
against the tree with these fixtures already in it. `cargo test -p willikins-dsl --test it -j 2`
passed all 60, `characterization_of_every_composition_document` unchanged. `cargo test -p
willikins-server --test it -j 2 image_contents` passed all 7, including
`the_image_workflows_directory_holds_exactly_the_seventeen_positive_documents` -- these fixtures
live under `workflows/fixtures/agents-md/`, which that count does not scan.

## Mutations

Each mutation was applied to a file saved beforehand under the scratchpad. The file was then
restored with `cp` followed by `touch`, and `cmp` against the saved copy printed nothing
(byte-identical). Only one mutation was in the tree at a time. Every run was
`cargo test -p willikins-cli --test it -j 2 agents_md` with `RUST_TEST_THREADS=2`.

| # | File | Mutation | Result |
| --- | --- | --- | --- |
| M1 | `github_scaffold_ensure.rs` (fake) | `observe()`'s `Some(_) => conflicts.push(...)` arm becomes `Some(_) => {}` (a differing pre-existing file no longer conflicts) | **killed**, exactly three: `monorepo_app_a_different_existing_agents_md_fails_plan_naming_the_path`, `standalone_service_a_different_existing_agents_md_fails_plan_naming_the_path`, `monorepo_app_a_second_scaffold_cannot_overwrite_the_first_markers_path`. Nothing else moved (the byte-equal test's own `already_equal` path is untouched). |
| M2 | `check.rs` | `check_boundaries`'s `Binding::Literal` arm becomes `{}` (a `uses:` boundary literal is never parsed against the child's declared type) | **killed**, exactly one: `project_prose_root_is_refused_invalid_literal_naming_the_with_input`. `project_prose_list_boundary_is_refused_as_a_nested_list` stayed green under this mutation -- it does not exercise the `Binding::Literal` arm at all, confirming it is a genuinely separate, uncovered path (the "Known gap" above), not redundant coverage of the same line. |
| M3 | `github_scaffold_ensure.rs` (fake) | `ToolSpec::new`'s `key` drops `port("marker")` | **survived**, 18/18, nothing moved. Neither `check`, `plan`, nor `apply` dedupes two nodes on `ToolSpec.key` at all -- it shapes journal and approval identity only. This sharpens, rather than weakens, the shared-marker finding (row 8): nothing in the engine itself refuses two `github.scaffold.ensure` nodes naming one marker: the second, distinct marker in `monorepo-app.yaml` is the *only* defence against the silent drop, not a backstop behind some other check. No change was made to `willikins-core` to add key-deduplication; that is a different, larger decision than this task's scope. |


## Summary

Nothing here forced a change to `willikins-core`, `willikins-types`, or `willikins-dsl`: every
route to prose reaching a committed file, every attempt to supply or replace the organisation's
template from a `uses:` boundary, and every seed-path escape was already refused by machinery
milestone 3d, 3g, and 2b put in place before 3o existed. Two things are worth carrying forward,
neither a 3o regression:

1. **The shared-marker finding (row 8) is real and silent, not a refusal.** `check` and `plan`
   both pass a document that collapses the second scaffold's marker onto the first's; within one
   `apply`, the second node observes the first's just-landed marker and reports `Unchanged`, and
   `AGENTS.md` never lands, with no error anywhere. `monorepo-app.yaml`'s own second, distinct
   marker (decision (d4)) is confirmed by M3 to be the *only* thing standing between this document
   shape and that outcome -- nothing downstream of the document author's own choice of marker name
   catches it.
2. **The plan/apply asymmetry for a brand-new repository** (the "A plan/apply asymmetry" section
   above) is a `github.scaffold.ensure` / milestone 3l read-table question, not a 3o one, and is
   left for that plan to pick up if it is worth closing.

The milestone 2b `Binding::List` boundary gap (row 2 / "Known gap") is pre-existing, documented in
`check.rs` by milestone 2b itself, and out of scope for 3o's own decisions.
