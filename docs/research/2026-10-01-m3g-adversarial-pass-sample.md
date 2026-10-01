# Milestone 3g: adversarial pass — Sample (B1, L1, W1, and `2b36422`)

**Date:** 2026-10-01
**Task:** the independent attacker's pass over the "Sample" group of
`docs/plans/2026-09-30-milestone-3g-file-writing.md` and the coordinator's commit `2b36422`. The
attacker wrote none of it.
**Subject:** B1 — `b019d94`, `64cddfa`, `83f4075`, `8396e27`, `6a4dae0`
(`buildkite.pipeline.bootstrap.gate`, its fake twin, registration, addendum). L1 — `f0af1b9`, `7def9db`
(`crates/willikins-providers-github/tests/live_scaffold_cycle.rs`). W1 — `5a54226`, `735a619`, `b881a42`
(`workflows/sample-ios-app.yaml`'s seventeen renders, `sample_files`, `bootstrap_gate`, the tests and
rendered snapshots). `2b36422` (Sample no longer copies its three App Store profiles into Doppler).
**Method:** read the plan in full, including every addendum and both earlier passes' carried items; then
the document, `sample_document.rs`, the live and fake gate, the live and fake scaffold tool, the shared
HTTP client's body handling, and the L1 harness. Then mutations. Every mutation followed the same steps:
copy the file into the pass's scratchpad, apply one exact-text replacement, run the narrowest test target,
copy the saved file back, `touch` it, and confirm byte identity with `cmp` (exit 0 every time). Never `git
checkout`, `reset` or `stash`. **No live call and no provider API was made.** L1 was compiled and its
offline tests run; its live cycle was not run (the coordinator's).

## 0. The state the pass started from

`main` at `2b36422`, clean apart from `goal.txt` and the host-maintenance todo (another session's; both
left alone). Nothing uncommitted, nothing red: `cargo test -p willikins-cli --test sample_document` green
before any change.

## 1. The questions, and what the evidence says

### Every placeholder in Sample's seventeen templates is in a quoted or identifier-only position: yes, now pinned

Nine placeholders in all, in two files' worth of positions:

- `ios/BUILD.bazel`: `profile_name = "{{ 0|1|2 }}"` (three) and `bundle_id = "{{ 0|1|2 }}"` (three), each
  a whole Starlark double-quoted string. `TemplateValue` holds neither `"` nor `\`, so a value can neither
  close the string nor escape inside it.
- `Sample.entitlements`, `SampleNotificationService.entitlements`, `SampleWidgets.entitlements`:
  `<string>group.{{ 0 }}</string>`, an XML text node. `TemplateValue` holds neither `<` nor `&`.

No placeholder sits in `upload-pipeline.sh`, `pipeline.yml`, `bootstrap.yml`, `provider-settings.json`,
`README.md`, the Swift sources or the Info.plists; every command in an executed file is document-literal.
The only values ever bound are the three `inputs.*_identifier` (`AppleBundleIdentifier`, grammar
`[A-Za-z0-9.-]+`) through the one conversion row. Both earlier passes left placement to W1's review; it
was reviewable only by reading snapshots, and `cargo insta accept` would erase any snapshot's catch. Now
`every_placeholder_sits_in_a_quoted_or_identifier_only_position` (`2f150b6`) is a strict allowlist: a
placeholder anywhere else, or in any other file, fails it, so a template change that adds one must change
the test. Mutations 1 and 2 below.

### Can a secret or a `Text` reach a committed file? No

Unchanged from the render-and-write pass, and re-checked at document level: every `values` element in the
document is `${{ inputs.app_identifier | nse_identifier | widgets_identifier }}`; no render binds a step
output, a Doppler value, or a `Text`. `sample_files.files` binds the seventeen render outputs and nothing
else; `message` is a literal headline. The write token (`gh_write_token`) reaches only
`sample_files.token`, a credential port; it is redacted in plan and applied JSON (`assert_no_secret_leaked`
covers `ghp_write_example` on every run).

### Is `sample_files.files` fully known at plan on a first fake run? Yes, now pinned

The renders are pure and bind only inputs, so `plan` computes every file. The scaffold tool (live and fake)
refuses an unknown `files` as `Invalid`, so a plan could never have proceeded with unknown content; but
nothing asserted the property that matters for approval: that the plan the operator approves carries every
byte the commit will write. `sample_files_files_are_fully_known_at_plan_on_a_first_run` (`2f39b5b`):
`sample_files` plans `Create`, `inputs.files` is known, 17 elements, each structurally the same `RepoFile`
(path and content) as its render node's planned `file`, in binding order.

### Can the scaffold overwrite or delete anything, a foreign marker included? No

The tool's write is one `createCommitOnBranch` of additions only, against `expectedHeadOid` equal to the
head the re-read saw; it has no deletion. A seed path is written only when it read `Absent` (or byte-equal)
at that exact head. Once the marker is present with the header, no seed path is ever read again.
Two document-level gaps W1 left are now pinned:

- **Acceptance 10's "every seeded file edited" clause was untested.** W1's run 2/run 3 re-plan over the
  *unchanged* landed files, which cannot tell "the marker alone decides" from "the seeds still match".
  `a_rerun_with_every_seeded_file_edited_plans_sample_files_noop` (`fedbfb3`) lands the scaffold, rewrites
  all seventeen seeds in the fake state, and asserts `NoOp` at plan, `Unchanged` at apply, and every edit
  plus the marker byte-identical afterwards. Mutation 3 is the one it alone kills.
- **A foreign marker.** `a_foreign_marker_fails_plan_and_writes_nothing` (`e201cca`): a marker path holding
  someone else's file fails `plan` with `NameTaken` on `sample_files`, its key naming the marker, and the
  fake state is unchanged. Mutation 4.

### Does the bootstrap gate leak the stored configuration, or ever write? No; one false `Present` fixed

- **Write:** the gate's only request is `GET /v2/organizations/{org}/pipelines/{slug}`
  (`only_get_is_ever_recorded_across_read_and_ensure_present_and_absent`); its `ensure` is a read.
- **Leak:** `PipelineConfigurationBody` deserializes one field, has no `Debug`, and is dropped in `observe`.
  A 2xx body that does not parse becomes a position-only message (the shared client never echoes serde's
  text); a non-2xx keeps only a bounded `message` field, never `configuration`. The redaction tests cover
  `Present`, a `500` with the marker in `configuration`, and a wrong-typed `configuration`. The `Absent`
  branch is leak-proof by type: `Observation::Absent { predicted }` carries `Outputs`, and the gate's only
  output is `slug`; `BlockedGate` carries the static `need`/`how`, the `subject` ports `org` and `slug`, and
  input names. Nothing renders `expected` either (a public `RepoFile` of the document's own, visible in the
  plan's `inputs` as the approver should see it).
- **Defect, fixed (`8191313`): a configuration that repeats a key compared equal.** `structurally_equal`
  parses straight into `serde_json::Value`, whose map keeps a duplicate key's last value silently.
  Probed: `"a: 1\na: 2\n"` equals `"a: 2\n"`. So a stored configuration naming `command` twice, the last
  one Sample's, read `Present`, although which value runs is Buildkite's decision, not this gate's. Both
  twins now refuse a side that YAML's own `Value` cannot parse (it rejects a duplicate key, probed at the
  top level, nested, and in JSON flow style) and report the pair as different, the safe direction. Test
  first: `structurally_equal_is_false_when_either_side_repeats_a_key` in each twin, red on both before the
  fix, green after; the whole of both crates' suites and `sample_document` green after.
- **Recorded, not fixed (low):** an empty stored configuration equals an all-comment or `null` expected
  (both parse to `Null`; probed `true`). Unreachable for Sample, whose bootstrap is a JSON object, and
  arguably correct (an empty bootstrap equals an empty one). B1's own deviation 4 (an `expected` that does
  not parse blocks forever) stands as recorded there.

### Can L1 touch anything but a throwaway sandbox repository, or run without its feature and credentials? Two gaps, both closed

- **Without the feature:** `cargo test -p willikins-providers-github --test live_scaffold_cycle` refuses,
  "target `live_scaffold_cycle` in package `willikins-providers-github` requires the features:
  `live-tests`". With it, the cycle is `#[ignore]`d, then returns unless `WILLIKINS_LIVE_TESTS=1`, then
  panics before any call without `WILLIKINS_GITHUB_TOKEN` (`credential_from_env`).
- **Defect, fixed (`6de9a6f`): the org was whatever the environment said.** `sandbox_org()` parsed
  `WILLIKINS_SANDBOX_GITHUB_ORG` with no check, so a mis-set variable would point the create, the raw
  `PUT`, the raw GraphQL commit and the delete at a real organisation; trust boundary 1 names one org.
  `sandbox_org_from` now refuses anything but `Willikins-Test` (case-insensitive) before step 1. Checked
  first, as one bit printed and no value, that the sandbox environment's org is `Willikins-Test`, so the
  guard does not block the coordinator's run. Offline test
  `sandbox_org_from_refuses_every_org_but_the_sandbox`.
- **Defect, fixed (`a0179bf`): a `4xx` create answer left the guard armed.** The guard is armed before
  the create `POST`, rightly, since a transport failure or `5xx` may still have created the repository.
  But a `422` means the name already belongs to a repository this run did not create, and the armed guard
  would then `DELETE` it: trust boundary 3, "Delete only the repository the same run created". Step 1's
  leftover refusal narrows this to a race (two runs in the same second, or a repository of that name made
  between steps 1 and 2), but the guard is the last line. `create_failure_keeps_guard_armed` now disarms on
  any `4xx` before the panic; offline test
  `a_4xx_create_answer_disarms_the_guard_and_anything_else_keeps_it`.
- Otherwise sound: the repository name is fresh per run and recorded before any assertion; every raw call
  names that one repository; step 9 deletes by that name, confirms `404`, and recounts. Step 7 prints key
  names, `errors[].type` and a status only; step 8 drops the token's plaintext immediately and never
  formats it.

### Can a moved head duplicate or lose a commit? No (unchanged)

Re-read, not re-tested: `ensure` decides from a fresh `observe` after any failure, the compare-and-swap
forbids a second commit on a moved head, and the retry is bounded at three. The render-and-write pass's
mutations of the same-head guard and the attempt bound still stand.

### Plan-to-apply drift? None from this group

`sample_files`' outputs are pass-through (`repo`, `branch`, `marker`); the gate's output is `slug`. Neither
carries a head sha, a commit oid or a configuration. If the scaffold lands between plan and apply, `ensure`
re-reads `Present` and reports `changed: false`; if a conflicting file appears, it refuses loudly.

### Did `2b36422` leave a dangling reference, comment or test claim? Only stale comments, and they were W1's

- `profile_to_doppler` survives only in dated history (two plans, one research note): correct.
- The fake state fixture never seeded the copies; the characterization diff removes exactly the nine
  `*_profile_to_doppler.*` port lines.
- Run 3's claim, "nothing is `Created`", is real: mutation 5 re-adds one copy and is killed (by
  `holds_back`, before run 3 is reached); mutation 5b adds an independent write-only sink and is killed by
  run 2's exact `Created` set. Any always-`Created` sink therefore fails run 2 before run 3 can see it; run
  3's assertion is the converged-run statement, run 2's is what kills first.
- `sample_document.rs`'s module doc still described "four" `operator.acknowledge` leaves and inputs,
  W1's leftover that `2b36422` passed by. Corrected (comment-only commit).

### Does every other document plan byte-identically? Yes

`git diff 2ddbdc9..HEAD` over `acceptance__characterization_of_every_document.snap` touches only
`workflows/sample-ios-app.yaml`'s entry (W1's new port lines, the two removed acknowledgements, and
`2b36422`'s nine removed lines) plus insta's own `assertion_line: 284` header metadata, which is not an
entry. This pass changes no document; `cargo test -p willikins-dsl --test acceptance` green after it.

## 2. Mutations

| # | Mutation (file) | Killed by |
| --- | --- | --- |
| 1 | `upload_pipeline_sh` gains `values: [app_identifier]` and `.../{{ 0 }}.yml` in its `exec` line (`sample-ios-app.yaml`) | `every_placeholder_…` (the rendered snapshot also fails, but `cargo insta accept` would silence it); `check` and every other test accept it |
| 2 | `bundle_id = "x", tags = [{{ 1 }}],`: an unquoted Starlark position inside an allowed file | `every_placeholder_…` alone (run filtered) |
| 3 | fake scaffold: `Present` only when every seed still matches (`github_scaffold_ensure.rs`) | `a_rerun_with_every_seeded_file_edited_…` alone; **survived all 234 tests of `willikins-providers-fake`** and every other `sample_document` test |
| 4 | fake scaffold: any marker is `Present`, first line unchecked | `a_foreign_marker_fails_plan_…` and the fake's own `read_reports_foreign_when_the_marker_is_not_ours` |
| 5 | re-add `app_profile_to_doppler` (`doppler.secret.set`) | `gates_unmet_then_satisfied_then_acknowledged` (`holds_back`) |
| 5b | add an independent `doppler.secret.set` sink | `gates_unmet_then_satisfied_then_acknowledged` (run 2's exact `Created` set) |
| 6 | live gate: byte equality instead of structural (`pipeline_bootstrap_gate.rs`) | four tests: two unit, two `fake_agrees_with_live` |
| 7a | L1: `sandbox_org_from` accepts any org | `sandbox_org_from_refuses_every_org_but_the_sandbox` |
| 7b | L1: the guard always stays armed after a failed create | `a_4xx_create_answer_disarms_the_guard_…` |

Plus the duplicate-key fix's own red run (its tests failing on the unfixed code in both twins). Every
restored file `cmp`-identical to its saved copy; no `.snap.new` left behind (mutation 1's was deleted).

## 3. Open, for the coordinator or the operator

- **The `GIT_CONFIG_*` host credential-helper override** (W1's open question) is still the operator's;
  this pass did not add it.
- **L1's two newest commits change the harness the coordinator will run**: it now refuses any org but
  `Willikins-Test`, which the sandbox environment matches.
- **`6de9a6f` was committed as a textual subset of `a0179bf`'s tree and not compiled on its own**; the
  four `sample_document` commits likewise (each a strict subset of the gated file).
- Low: an empty stored configuration equals an all-comment expected (above). B1's deviation 4 unchanged.
