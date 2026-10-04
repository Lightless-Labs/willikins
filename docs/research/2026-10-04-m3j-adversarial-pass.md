---
title: "Milestone 3j adversarial pass: the secret-name gate, the underscore widening, and the pass-through edge"
created: 2026-10-04
status: complete
area: providers-doppler, providers-fake, dsl, cli
related:
  - docs/plans/2026-10-03-milestone-3j-apns-key-observed.md
---

# Milestone 3j adversarial pass

An attack over everything milestone 3j landed before this task: part A's underscore widening (`DopplerProject`,
`DOPPLER_CONFIG_PATTERN`), part B's `doppler.secret_name.gate` (client, live tool, fake twin, registration, the
written-but-unrun live cycle), and part C's `doppler.config.inherits.ensure` pass-through (task C1) plus the
tracked document and its three negative fixtures (task C2). The reviewer wrote none of it. No provider was called;
every mutation ran against the fake and the mock server only.

At the start, the tree was clean except for the pre-existing untracked `goal.txt` and the cargo-target todo,
neither touched. `main` was at `a21bfdc`.

## Findings and fixes

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | Low (test gap, live/fake disagreement) | **The fake `doppler.secret_name.gate`'s one-level walk could read `Present` for a base config that does not exist.** `observe_state`'s top-level check (`!state.doppler_configs.contains(&key)` → `Absent`) only ever covered the config being asked about. The walk over `doppler_config_inherits` then tested each base purely by membership in `doppler_secrets`/`doppler_values`/`doppler_secret_writes` (`listed()`), with no check that the base itself was seeded into `doppler_configs`. Live can only ever learn a name from a base by calling that base's own `GET .../secrets/names`, and a base that does not exist 404s — `looks_like_a_missing_project` turns that into "contributes nothing," never into a leak of whatever happens to be seeded under the base's key. The module's own doc *claimed* "a base absent from `doppler_configs` contributes nothing, because nothing is ever seeded under a key for a config that does not exist" — a seed-discipline promise about how fixtures are written, not something the type checked or the walk itself enforced. Every existing `Present`-via-a-base test happens to seed the base's existence first, so nothing caught a seed that got this backwards (the tracked fixture, `workflows/fixtures/state/inherited-secret-name.json`, is itself clean: its one secret key, `shared_keys/prd#EXAMPLE_APNS_KEY`, sits under a config also listed in `doppler_configs`). | `3483f52`. Test first: `read_reports_absent_when_the_inherited_base_does_not_exist_even_if_a_secret_is_seeded_under_its_key` seeds `EXAMPLE_APNS_KEY` under a base config that is never passed to `with_doppler_config`, and asserts `Absent`. It failed on the old code (`Present`) — pinned as mutation M7 below. The fix adds the same `doppler_configs.contains(base)` check to the walk's per-base test that the top-level config already had. |
| 2 | Low (unenforced trust-boundary claim) | **Nothing pinned that `SecretNamesBody` carries no `Debug`.** Trust boundary 2 (SHARED VALUES, and the struct's own doc comment) says this response type "deliberately" derives no `Debug`, so a listed name other than the one asked about can never reach a panic message or an `assert_eq!` failure. But no test checked it: acceptance 6's source guard (`tests/secret_name_gate_mock.rs`'s `the_tool_never_names_a_value_reading_endpoint`) greps only `secret_name_gate.rs`, never `client.rs`, and no test anywhere formats a `SecretNamesBody` with `{:?}` (there is nothing to format — the field holding the other names is never exposed). A future `#[derive(Debug)]` landing on this struct would have been silent at compile time and at test time, found only by a future code-reviewer noticing the derive, or by a later caller that actually used it to print a config's full secret-name list. The sibling crate `willikins-providers-buildkite` already has the exact pattern for `PipelineBootstrapBody` (`pipeline_bootstrap_body_has_no_debug`); `willikins-providers-doppler` had nothing equivalent for this struct. | `037f8eb`. `secret_names_body_has_no_debug`, the same ambiguous-trait negative: it compiles only while `SecretNamesBody` has no `Debug` impl. Confirmed red under mutation M6 below (`error[E0283]`, not merely a failing assertion — a derive here is now a compile error in the test target). |

## Mutations (each saved to the scratchpad before editing, restored after, `cmp` byte-identical)

| # | Mutation | Result | Killed by |
| --- | --- | --- | --- |
| M1 | `client.rs::secret_name_listed`: `listed == name.as_str()` → `listed.starts_with(name.as_str())` | killed | `client::tests::a_prefix_suffix_or_case_near_miss_never_counts_as_listed` (`EXAMPLE_APNS_KEY_OLD` starts with `EXAMPLE_APNS_KEY`, flips `listed` to `true`) |
| M2 | `secret_name_gate.rs::observe`, the *direct* listing's error arm: `Err(err) if looks_like_a_missing_project(&err) => Absent` (with the fall-through `Err(err) => Err(..)` removed) → `Err(_) => Absent` unconditionally | killed | `config_answers_401_is_err`, `config_answers_403_is_err`, `config_answers_500_is_err` (all three now read `Absent` instead of erroring), plus two collateral failures from the same test binary (`ensure_on_absent_never_writes_and_reports_unchanged`, `malformed_names_body_is_a_provider_error_naming_config_and_name_only` — the fixture these two use also hits this arm) |
| M4 | `secret_name_gate.rs::observe`, the final "every base read, none listed" `Absent` arm: `predicted: Self::outputs_for(config)` → `predicted: Outputs::new()` | killed | `unlisted_with_no_inherits_reads_absent`, `ensure_on_absent_never_writes_and_reports_unchanged` (both call `assert_config_output(&predicted)`, which panics on `Option::unwrap()` over the now-missing `config` port) |
| M5 | `config_inherits_ensure.rs::observe`'s two `Absent` arms (the mismatch-free unequal-set case and the missing-project case): `predicted: Self::outputs_for(config)` → `predicted: Outputs::new()` | killed | `read_reports_config_known_in_absent_predicted`, `read_reports_config_known_in_absent_predicted_on_404` — task C1's own acceptance-9 tests, confirming the `Unknown`-in-`Absent` gap decision (c2) warns about is already closed |
| M6 | `client.rs`: `#[derive(Deserialize)]` on `SecretNamesBody` → `#[derive(Debug, Deserialize)]` | killed (compile error, not a failing assertion) | `secret_names_body_has_no_debug` fails to compile: `error[E0283]: type annotations needed ... multiple impls satisfying client::SecretNamesBody: AmbiguousIfDebug<_>` |
| M7 | `doppler_secret_name_gate.rs` (fake), the walk: `state.doppler_configs.contains(base) && Self::listed(state, base, name)` → `Self::listed(state, base, name)` (finding 1's own fix, reverted) | killed | `read_reports_absent_when_the_inherited_base_does_not_exist_even_if_a_secret_is_seeded_under_its_key` (the new test; this is also the red run finding 1's own fix is based on) |

Six mutations in all, each restored from the scratchpad copy and confirmed `cmp`-identical to the committed file
before the next mutation was applied. No two mutations were live in the tree at once except M1 and M6, which sit in
the same file (`client.rs`) but touch disjoint code paths and were verified together in one run before both were
reverted together.

## Checked and accepted (no change)

- **`ConfigRefBody` cannot smuggle a base through the query string.** A `grep` for `project: String` / `config:
  String` in `client.rs` turns up only request-side bodies (`CreateProjectBody`, `SetInheritableBody`,
  `ConfigRefRequest`, `SetSecretsBody`, …) built from an already-validated `DopplerProject`/`DopplerConfigName` via
  `.to_string()`. The response-side type the gate's walk actually reads, `ConfigRefBody` (one entry of a config's
  `inherits` array), deserializes `project: DopplerProject` and `config: DopplerConfigName` directly — its own doc
  comment says why ("deserialized straight into `DopplerProject`/`DopplerConfigName`, so an entry naming something
  outside either grammar fails the parse"). The gate's walk (`secret_name_gate.rs`) calls
  `secret_name_listed(&base.project, &base.config, name)` straight off that struct, so this only compiles because
  the types are already right; nothing downstream of a malformed base entry could reach the query string unparsed.
- **No `tracing` call anywhere in either provider crate.** `grep -rn "tracing::" crates/willikins-providers-doppler/src
  crates/willikins-providers-fake/src` is empty. Verify item 9's own answer from the 3i pass ("the only `tracing`
  call in non-test code... logs no `Value`") still holds; this milestone added none.
- **Fake key formats agree with each other.** `doppler_config_key` (`state.rs:680`, `config.to_string()`) and
  `doppler_secret_key` (`state.rs:692`, `format!("{config}#{name}")`) are the only two join shapes
  `with_doppler_config`/`with_doppler_config_inherits`/`with_doppler_secret` write through, and the gate's own
  `listed()` builds its lookup key the same way (`format!("{config_key}#{name}")` over an already-`doppler_config_key`'d
  string). No third join character is used anywhere this gate reads.
- **`include_dynamic_secrets=false` and `include_managed_secrets=false` stay pinned.** `names_query()`'s
  `mockito::Matcher::AllOf` in both `client.rs`'s own tests and `secret_name_gate_mock.rs`'s literal `names_path`
  helper assert the full four-parameter query, so a refactor dropping either flag fails every test in both files,
  not only one.
- **Characterization diff from `a96fce5` (the plan's own base commit) to C2's last commit (`a21bfdc`), `git diff
  -U0` over the one snapshot file.** The diff has exactly: C1's six lines (the three inheriting documents'
  `plan_json` and `fingerprint`, each gaining the one known `config` output in all nine `inherit` instances
  combined — matching the coordinator's sign-off exactly), plus C2's four new entries (the tracked positive
  document `workflows/doppler-inherited-secret-gate.yaml` and the three negative fixtures). No other document's
  entry changed. Nothing in this pass added, removed, or touched a document, so the snapshot is otherwise
  untouched by X1.
- **`DOPPLER_CONFIG_PATTERN` and `DopplerConfig::parse` agreement.** Already covered, including the one documented
  divergence at the `max_len` boundary, by task A1's own addendum and
  `doppler_config_regex_and_parse_diverge_on_the_max_len_boundary`; this pass re-read that test and found nothing
  further to attack in it.
- **`Foreign`/`NameTaken` are not reachable from the gate.** `doppler.secret_name.gate`'s `observe` has exactly four
  terminal shapes (`Present`, `Absent` via the direct call, `Absent` via the walk, `Err`) and never constructs
  `Observation::Foreign`; its own `ensure` treats any other `Observation` variant as `unreachable!()`. Grepping the
  tool's source for `Foreign` finds only the module doc's own paragraph explaining why it is never returned.

## Residual for the coordinator (not a defect against the plan)

- **Stale `#[allow(dead_code)]` comments.** `secret_name_listed` and `SecretNamesBody` in `client.rs` still carry
  `#[allow(dead_code)] // ... not yet written` comments dating from before task B2 landed; the gate is now the real
  caller, so the attribute and its comment are themselves dead. Hygiene only, left for the coordinator's own pass
  rather than folded into a behaviour commit here.
- **Verify item 1 (whether the names endpoint lists inherited names) is the coordinator's own line item** per the
  plan's "Then the coordinator" section; this pass did not call a provider and records nothing new about it.
