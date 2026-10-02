---
title: "Milestone 3i independent adversarial pass: the bootstrap writer and identifier masking"
created: 2026-10-02
status: complete
area: buildkite, cli, core, server, fake
related:
  - docs/plans/2026-10-02-milestone-3i-bootstrap-writer-and-identifier-masking.md
  - docs/research/2026-10-02-m3i-adversarial-pass-part-a-bootstrap-writer.md
  - docs/research/2026-10-02-m3i-adversarial-pass-part-b-identifier-masking.md
---

# Milestone 3i independent adversarial pass

This is a third pass over everything milestone 3i landed (A1 to A8, B1 to B8, and X1's two passes). The
reviewer wrote none of it. It does not repeat the part A and part B records: their mutations (part A's
M-1 to M-4, part B's M-1 to M-4) are not re-run, and the mutations here target different code. No
provider was called.

At the start, the tree was clean (only the untracked `goal.txt` and the cargo-target todo, neither
touched), `main` was at `07588b5`, and nothing was red.

## Findings and fixes

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | High | **A failed run's `error:` line printed full identifiers in CLI text.** `render::run_record_text` masked the per-node and workflow output maps through `mask_json`, but printed the run's `error` as `error.as_json().to_string()`, unmasked. That JSON is the journal's `Redacted<ApplyError>`. An `ApplyError::Tool` carries the partial `Applied` built before the failure, which holds every earlier node's outputs in full, because the journal keeps full values (decision (b7)). So `apply --plan-id`, `run <id>` and `runs` printed a freshly created record's full id in their default text output, with no `--reveal`, on any run that failed after an identifier-producing node. That is Sample's common failure shape: profiles or bundle-id records are created, then a later node fails. `ApplyError::Tool` (and `UnknownInput`, which also carries the partial `Applied`) are the variants that reach a `RunRecord`. The Butler refuses drift before `RunStarted` and journals it as `ApplyRefused` (`butler.rs`, `first_drift`), so an `ApplyError::Drift` with its two whole `Value`s normally never becomes `RunRecord.error`. If one ever did (core `apply` re-checks drift inside the run), the fix masks it the same way. `--json` was already masked (`print_json`), and so were MCP's `run_status` (`Masked<T>`) and the approvals page (which never renders a run). The CLI text path was the only gap. Neither the B5 tests nor the part B pass covered it: the one existing failed-run test used `ApprovalRequired`, which carries no `Applied`. | `758d100`. Test first: `render::tests::run_record_text_masks_an_identifier_inside_a_failed_runs_error_unless_revealed` builds `ApplyError::Tool` with a `Created` node holding an `AppleIssuerId`. It asserts the journal JSON keeps the full value (so the test proves something), that `Disclosure::Masked` text holds `5724...` and never the full id, and that `Revealed` shows it. It failed on the old code (the log showed the full `57246542-...` inside `error:`). The fix clones the error JSON and runs `mask_json` over it unless revealed, the same chokepoint `redacted_map_lines` uses. |
| 2 | Low (test gap) | **Nothing pinned that only a `404` reads `Missing` on the bootstrap writer.** Mutation M5 (below) made every GET error read `Missing`. With it, a `403` (a token without `read_pipelines`), a `500`, or a parse failure read `Absent`, so the tool planned an `Update` over a pipeline nobody had looked at. All of `pipeline_bootstrap_ensure_mock.rs` still passed. One test caught it only by accident: `redaction.rs`'s `a_writer_configuration_marker_reaches_no_observation_or_error_on_the_read_side`, whose purpose is redaction but which happens to `expect_err` on a `500`. The production code was already correct. | `0f8a2ec`. `a_read_the_token_is_not_allowed_fails_and_never_reads_missing` makes the GET answer `403` (exactly three calls, since a 403 is not retried) and checks that `read`, `updates` and `ensure` each fail with `Provider` and that zero `PATCH`es are sent. It fails under M5 at line 131. |

## Mutations (each restored from a saved copy in the scratchpad, `touch`ed, `cmp` byte-identical)

| # | Mutation | Result | Killed by |
| --- | --- | --- | --- |
| M5 | `pipeline_bootstrap_ensure.rs` `analyze`: `Err(err) if err.status == Some(404) => Missing` plus `Err(err) => Err(err.into())` replaced by `Err(_) => Missing`. Every read failure becomes "pipeline does not exist", so the tool plans `Update` | killed, but only by accident before this pass | before: only `redaction.rs`'s `a_writer_configuration_marker_reaches_no_observation_or_error_on_the_read_side`. After `0f8a2ec`, also `a_read_the_token_is_not_allowed_fails_and_never_reads_missing` (re-run under the mutation: 15 passed, 1 failed) |
| M6 | `pipeline_bootstrap_ensure.rs` `ensure`: the post-`PATCH` re-read dropped, so `Ok(()) => changed: true` unconditionally (a write Buildkite normalised away, or one that landed elsewhere, reports success) | killed | `different_whose_patch_succeeds_but_re_read_still_differs_reports_provider`, `different_ensure_patches_then_re_reads_equal_and_reports_changed` (its registered GET/PATCH/GET sequence goes unmet), `fake_agrees_with_live`'s `bootstrap_ensure_agrees_when_different_and_both_write_then_converge`, and `redaction.rs`'s `a_patch_response_marker_reaches_no_ensured_debug_or_error` |
| M7 | Fake twin (`willikins-providers-fake/.../buildkite_pipeline_bootstrap_ensure.rs`) `ensure`: the `Foreign` arm merged into `Different`, so the fake overwrites a pipeline that is not ours while the live tool answers `Conflict` | killed | the fake's own `foreign_reads_foreign_updates_false_and_ensure_conflict`, and the parity test `fake_agrees_with_live::bootstrap_ensure_agrees_when_foreign` |
| M8 | CLI `render::redacted_map_lines`: the `mask_json` call removed, so journal-sourced node and workflow outputs print whole in `run` and `runs` text | killed | `render::tests::run_record_text_masks_an_identifier_output_unless_revealed` and `tests/identifier_masking.rs`'s `run_and_runs_over_a_journal_mask_the_same_way_as_plan` (end to end, against the built binary) |
| (M9) | Finding 1's fix reverted, which is the code as it stood before `758d100` | killed | the new `run_record_text_masks_an_identifier_inside_a_failed_runs_error_unless_revealed` (its red run is recorded above) |

## Checked and accepted (no change)

- **No `RepoFile` reaches `configuration` except from a document template.** A grep of every live and
  fake tool's port declarations shows that `repo.file.render` is the only tool with a `RepoFile` output.
  No tool outputs `TemplateSource` or `list<RepoFile>`. That leaves no step output to forward, and no
  "file read from a repository" route at all. The other routes are refused as follows:
  - `for_each`: a `list<RepoFile>` workflow input is refused by `check_workflow_inputs`
    (`DisallowedInputType`). A literal `for_each` source is `ForEachOverScalar`. A `Keyed` reference to a
    `for_each` instance of `repo.file.render` is still a document template.
  - Conversions: the registry tests
    `the_production_registry_has_no_text_to_template_value_conversion_row` and
    `the_production_registry_has_no_conversion_row_into_template_source_or_repo_file` pin that no
    conversion targets `TemplateValue` from `Text`, or `TemplateSource` or `RepoFile` from anything.
  - A literal, and `Text`: refused by A5's two fixtures (characterization lines below).
  - `PortType` has only `Exact` and `AnySecret`, so no generic port could relabel a value as `RepoFile`.
- **The writer acts only on the pipeline the document names, and `PATCH`es only `configuration`.** The
  path is `format!("/v2/organizations/{org}/pipelines/{slug}")`. `BuildkiteOrg` is
  `[a-z0-9]+(?:-[a-z0-9]+)*` and `BuildkitePipelineSlug` is `[a-z0-9][a-z0-9-]*`. The derive anchors
  both patterns (`codegen.rs` builds the `Regex` from an anchored form), so neither admits `/`,
  `.`, `?`, `%` or `#`, and the path cannot leave the pipeline it names. Sample binds `org` as a literal
  and `slug` from `steps.pipeline.slug`, which comes from `naming.v1` over the literals `Example-Org`
  and `sample`, so no workflow input steers either one. The body is pinned by part A's M-3.
- **The stored configuration never leaks through an error.** `Http`'s JSON parse failure reports line
  and column only (`http.rs`, "could not parse the response body as the expected shape"). A
  transport-error message is ureq 3.4's `Display`, read in the vendored source: none of its arms
  includes the URL or a body. `PipelineBootstrapBody` has two `Option<String>` fields, so a string
  `configuration` always deserializes, and serde's "invalid type" message, which quotes values, cannot
  quote it.
- **Plan-to-apply drift.**
  - `apply --plan-id` compares the document's SHA-256 with the recorded one (`butler.rs`,
    `document_sha256 != record.document_sha256`), so a template edited after approval cannot be what
    gets written.
  - If the stored configuration becomes equal between plan and apply, the re-plan's `NoOp` differs from
    the approved `Update`, and `DriftKind::Action` refuses the run.
  - If the pipeline becomes foreign, the re-plan's `NameTaken` refuses it at plan, and `ensure`'s own
    re-analysis would answer `Conflict` (M7, and part A's M-2).
- **The fake and the live tool.** Two differences, neither visible to a plan or an apply. The fake's
  `updates()` records no read call, where the live `updates()` sends a GET. The fake's
  `fail_ensure_once` fires before validation, where the live tool validates first. The four-state table,
  the validation copy and the structural comparison copy are textually identical to the live ones, and
  M7 shows the parity tests compare the outcomes, not only the shape.
- **Characterization, diffed from `06ac9f9` (3i's base) to `HEAD` with `git diff -U0`.** The diff has
  exactly three parts:
  - the two new A5 fixture entries (`buildkite-bootstrap-literal-configuration.yaml` with one
    `RepoFileLiteral`, and `buildkite-bootstrap-text-configuration.yaml` with one `TypeMismatch`
    `Text` into `RepoFile`);
  - Sample's four A6 lines (`bootstrap_gate.{org,slug,expected,token}` replaced by
    `bootstrap.{org,slug,configuration,token}`, same types);
  - B8's ten `Text` to `DopplerValue` lines across four documents, which B8's own addendum explains and
    the coordinator signed off.

  No other line in any other entry changed, and this pass changed no document.
- **`--reveal` never reveals a secret.** `Value::display(Revealed)` is `render()`, which prints a secret
  as `[REDACTED ...]`. Under `--reveal`, `print_json` skips `mask_json` and keeps `Value`'s `Serialize`,
  which redacts as well. The new `error:` path keeps the same symmetry.
- **Verify item 9 (`tracing`).** The only `tracing` call in non-test code is
  `tracing::info!(bind = %bind, ...)` in `willikins-server/src/http/mod.rs`, and it logs no `Value`.
- **Other error text.** The App Store Connect tools' `format!` messages (`certificate_get.rs` and its
  siblings) name the certificate type, a count or "the requested serial", never the serial or a record
  id. `Debug` formatting in non-test code covers only `Class`, `Action`, raw names that failed to parse,
  and the operator's own `--input` argument (`describe.rs`: `"{s:?} is not name=value"`). The last one
  quotes the operator's own command-line text back to them. Noted, not changed.

## Residual for the coordinator (not a defect against the plan)

- **The `for_each` instance key, `DuplicateForEachKey`'s `key`, a gate's `subject` and `Replacing::subject`
  are rendered strings.** They print whole if a document ever builds them from an identifier-typed value.
  The plan puts this out of scope, and B7's guards pin it only for the shipped documents
  (`workflows/` and `workflows/fixtures/`). A new document with a `for_each` over a `list<AppleProfileId>`
  workflow input, which is a legal input type, would print full ids as instance keys. Making it
  impossible, rather than only absent today, takes a `check` refusal of an identifier-typed `for_each`
  source. That changes what `check` accepts, so it is the coordinator's decision.
