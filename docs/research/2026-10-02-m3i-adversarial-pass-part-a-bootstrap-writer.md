---
title: "Milestone 3i adversarial pass, part A: buildkite.pipeline.bootstrap.ensure"
created: 2026-10-02
status: complete
area: buildkite
related:
  - docs/plans/2026-10-02-milestone-3i-bootstrap-writer-and-identifier-masking.md
---

# Milestone 3i adversarial pass, part A

An independent attack (task X1) on everything task group A landed (A1 through A8): the client's
`get_pipeline_bootstrap`/`update_pipeline_configuration`, the four-state `analyze` in
`buildkite.pipeline.bootstrap.ensure`, the shared `structurally_equal`, the fake twin, catalog
registration, the two negative fixtures, and Walter's `bootstrap` node. By a reviewer that wrote
none of it. No provider was called (the live bootstrap cycle, task A7, is the coordinator's to
run). Priority targets, from the plan's task X1 row: a `PATCH` body with any key but
`configuration`; any `RepoFile` route from a literal, input, default or `Text`; a path outside the
rule accepted; the stored configuration or a webhook URL reaching any output, error, `Debug` or
journal; `Update` where `Foreign` or `NotFound` is right.

At the start, nothing was uncommitted and `cargo test -p willikins-providers-buildkite` was green
(54 lib tests, plus the integration suites, all passing; the recorded figures below are from the
same run).

## Finding and fix

| # | Severity | Finding | Fix (this pass) |
| --- | --- | --- | --- |
| 1 | Medium | `BuildkitePipelineBootstrapEnsure::analyze`'s ownership check (`body.description.as_deref() != Some(MANAGED_DESCRIPTION)`) is exact equality, matching trust boundary 3 and the client's own doc. But no test distinguished that from a weaker *substring* check: every existing `Foreign` test used `description: null` or the exact marker, never a near-miss. A pipeline named `"managed-by: willikins-production"` (the marker as a prefix of a longer, foreign description) or `"custom (managed-by: willikins) pipeline"` (as an embedded substring) would, under a `.contains()`-style ownership check, read `Present`/`Equal` instead of `Foreign` whenever its stored configuration happened to match the document's rendered content — and `ensure` would then silently report `changed: false` with zero `PATCH` on a pipeline this crate never created, rather than refusing with `Conflict`. This is exactly the shape of bug the plan's own trust boundary 3 exists to rule out, and the sibling tool `buildkite.pipeline.ensure` (`pipeline_ensure.rs`) has the identical exact-match code and the identical test gap (not fixed here: out of this milestone's scope, noted below under "Checked and accepted"). | A new test, `a_description_that_only_contains_the_marker_as_a_substring_is_still_foreign` (`tests/pipeline_bootstrap_ensure_mock.rs`), pins three near-miss descriptions (prefix, embedded substring, and a trailing-space variant) each paired with a stored `configuration` set equal to the document's own rendered content — so a weakened check would read `Present`/`Equal`, not merely a wrong `Foreign`/`Conflict`, catching both failure shapes in one test. Confirmed red against the mutation below and green against the real code; no production code changed, since the real code already does exact equality. |

## Mutations (each restored from a saved copy, `cmp` byte-identical)

| # | Mutation | Result | Killed by |
| --- | --- | --- | --- |
| M-1 | `pipeline_bootstrap_ensure.rs`: the ownership check `!= Some(MANAGED_DESCRIPTION)` weakened to `!body.description.as_deref().is_some_and(|d| d.contains(MANAGED_DESCRIPTION))` (a substring test) | **survived** the full existing suite (`cargo test -p willikins-providers-buildkite`: all suites green, 0 failed) | nothing, until this pass's new test (see Finding 1) |
| M-2 | `pipeline_bootstrap_ensure.rs`'s `ensure`: the `BootstrapState::Foreign` arm changed from `Err(conflict(...))` to `Ok(Ensured { outputs: Self::outputs_for(&slug), changed: false })` — `ensure` silently no-ops on a foreign pipeline instead of refusing | killed | `foreign_reads_foreign_and_ensure_conflicts_with_zero_patch` (`tool.ensure(&inputs, &token).unwrap_err()` panicked on the now-`Ok` value) |
| M-3 | `client.rs`'s `UpdateConfigurationBody` given a second field, `name: &'a str`, and `update_pipeline_configuration` set it to the pipeline's own slug (the exact "helpfully echo the slug back" shape Buildkite's own docs warn regenerates it) — the `PATCH` body becomes `{"configuration": ..., "name": "..."}` | killed | `update_pipeline_configuration_sends_exactly_the_configuration_key`'s exact `mockito::Matcher::Json` body pin (not a subset match), and transitively `different_ensure_patches_then_re_reads_equal_and_reports_changed` (the PATCH mock it also sent against went unmatched, returning an unregistered-route failure) |
| M-4 | `pipeline_bootstrap_ensure.rs`'s path rule weakened from "the second-to-last segment is exactly `.buildkite`" to "any segment equals `.buildkite`" (accepting any ancestor, not only the immediate parent) | killed | `rejects_a_path_not_directly_under_a_buildkite_directory` (the exact case decision (a4)'s own doc names: `apps/walter/.buildkite/plugins/stage-input/plugin.yml`, a plugin manifest nested under `.buildkite/`, not a pipeline configuration) and, at the full-tool level, `refused_paths_make_zero_http_calls_and_name_neither_path_nor_content` |

## Checked and accepted (no change)

- **`buildkite.pipeline.ensure`'s own ownership check has the same exact-equality code and the
  same test gap** (`pipeline_ensure.rs`: `body.description.as_deref() != Some(MANAGED_DESCRIPTION)`,
  with only a `description: null` `Foreign` test, no near-miss). That tool predates milestone 3i
  (milestone 3a) and is out of task X1's scope (A1–A8 and the bootstrap writer specifically); left
  as a follow-up rather than fixed here, since fixing it would touch a file no task in this
  milestone's table names.
- **The `PATCH` response's `provider.webhook_url` and `configuration` markers never reach an
  output, error, `Debug`, or the journal.** Already proven by `tests/redaction.rs`'s
  `a_patch_response_marker_reaches_no_ensured_debug_or_error` and
  `a_configuration_marker_reaches_no_observation_ensured_or_error`, and structurally by
  `PipelineBootstrapBody` and `PipelineConfigurationBody` deriving no `Debug` at all (pinned by
  `client.rs`'s own `AmbiguousIfDebug` compile-time negative). Not re-attacked here beyond reading
  both structs and confirming neither derives or hand-writes one.
- **No `RepoFile` route into `configuration` from a literal, input, default, or `Text`.**
  `check_literal`/`check_list_literal` (`willikins-core/src/check.rs`) refuse a `RepoFile` literal
  outright (`CheckError::RepoFileLiteral`) before ever handing the text to a parser;
  `check_workflow_inputs` refuses `RepoFile` as a declared input type *or* default in one branch
  (`CheckError::DisallowedInputType`), whether or not a default is also present; and no
  `conversions!` row targets `RepoFile` at all (`Text` has no path to it), so a `Binding::Step`
  from a `Text`-typed node output fails `check`'s own type-mismatch test. The two negative fixtures
  (`buildkite-bootstrap-literal-configuration.yaml`, `buildkite-bootstrap-text-configuration.yaml`)
  already exercise exactly these two routes end to end, each pinning one `check` error. Verified by
  reading `check.rs`'s `RepoFileLiteral`/`DisallowedInputType` branches and the registry's
  `conversions!` table (no row with `to: RepoFile`), not merely assumed from the plan's own prose.
- **`RepoPath`'s grammar already refuses path traversal, absolute paths, double slashes, trailing
  slashes, and a `.git`/`.github` segment** (`willikins-types/src/repo.rs`'s own unit tests:
  `a/../b`, `a//b`, `/a/b`, `a/b/`, `.git/x`, `.github/workflows/ci.yml`, all refused at parse
  time, before `validate_configuration` ever runs) — so `buildkite.pipeline.bootstrap.ensure`'s own
  path rule never has to defend against a traversal sequence smuggled through a syntactically valid
  segment; it only has to decide *which* otherwise-legal path is accepted, which M-4 above attacks
  directly.
- **The content rule's "non-empty `steps` sequence" check cannot be weakened to accept a present-
  but-wrong-shaped `steps` key (a scalar, a mapping, or an empty sequence) without `as_sequence()
  .is_some_and(|s| !s.is_empty())` already catching it**: `rejects_content_whose_steps_is_empty`
  (`steps: []`) is the discriminating case — any mutation that drops the emptiness check or the
  sequence-type check turns this specific test red, confirmed by reasoning through the exact
  mutation (`mapping.get("steps").is_some()` alone) against it rather than a separate run, since it
  is the same code path M-4 already exercised a live mutation against.
- **The duplicate-top-level-key refusal, shared by `structurally_equal` and
  `validate_configuration`'s own YAML parse, already has its own adversarial-pass provenance**
  (`compare.rs`'s own doc and test, dated 2026-10-01, from the milestone 3g/3h Walter group's pass):
  parsing straight into a `serde_json::Value` silently keeps a duplicate key's last value, so a
  mutation that changed the early-return's `.any(...)` to `.all(...)` (both sides must fail to
  parse, not just one) would let a side with a repeated `command:` key compare equal to a single-
  command rendering by its last value alone — reasoned through against
  `structurally_equal_is_false_when_either_side_repeats_a_key`'s own fixture
  (`repeated`/`rendered`, whose last values are deliberately equal) rather than re-run, since it is
  the exact scenario that test's own doc comment says it exists to catch.
- **`Missing` deliberately reads `Absent` and plans `Update`, rather than failing at plan time**,
  by decision (a2)'s own design (so a fresh document's first run, where the upstream `pipeline`
  node has not created the pipeline yet, remains plannable) — not a bypass: `ensure` on a
  genuinely-missing pipeline still fails loudly with `NotFound` (`missing_reads_absent_updates_true_and_ensure_not_found_with_zero_patch`),
  and a `404` is the only status this tool treats as `Missing` (any other provider error,
  including a `403`, propagates as-is through `Err(err) => Err(err.into())`).
- **The gate (`buildkite.pipeline.bootstrap.gate`) and the writer never disagree about "equal"**:
  both call the same `crate::tools::compare::structurally_equal`, pinned by
  `the_gate_and_the_writer_agree_on_equal_across_a_shared_table`'s four-row table (re-quoted equal,
  exact equal, different, unparsable). Not re-attacked beyond confirming both call sites really do
  share the one function (`grep` for `structurally_equal` call sites in both tool files).

## Verify (not settled by this pass; belongs to the coordinator)

- Verify items 1–6 of the plan (the live `PATCH` semantics: only `configuration` changes;
  re-quoting; no build triggered; the real pipeline's provider settings; the real
  `PIPELINE_CREATION_TOKEN`'s scope; the largest accepted configuration) all require the live
  sandbox Buildkite organisation, which task A7 writes but does not run and this pass does not
  call either.
