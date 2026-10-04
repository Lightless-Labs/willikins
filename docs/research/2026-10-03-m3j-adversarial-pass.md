---
title: "Milestone 3j adversarial pass, second and independent: the secret-name gate, the underscore widening, the pass-through edge"
created: 2026-10-04
status: complete
area: providers-doppler, providers-fake, types, dsl, cli
related:
  - docs/plans/2026-10-03-milestone-3j-apns-key-observed.md
  - docs/research/2026-10-04-m3j-adversarial-pass.md
---

# Milestone 3j adversarial pass (second, independent)

A second attack over everything milestone 3j landed, by a reviewer who wrote none of it, run after the first pass
(`docs/research/2026-10-04-m3j-adversarial-pass.md`, mutations M1 to M7). This pass does not repeat those
mutations. Its numbering continues from M8. No provider was called. Every mutation ran against the fake and the mock
server only.

At the start the tree was clean, except for the pre-existing untracked `goal.txt` and the cargo-target todo. Neither
was touched. `main` was at `82427f7`, and nothing was uncommitted or red.

## Findings and fixes

| # | Severity | Finding | Fix |
| --- | --- | --- | --- |
| 1 | Low (test gap, surviving mutant) | **Nothing tested the fake gate's top-level existence check.** `observe_state` reads `Absent` for a config missing from `doppler_configs`. That matches the live tool, whose names call on a missing config gets the missing-project answer and reads `Absent` without walking. Every missing-config case (`read_reports_absent_when_the_config_does_not_exist`, and `fake_agrees_with_live.rs`'s `secret_name_gate_config_missing_agrees`) starts from an empty `FakeState`, so nothing is ever listed. Deleting the check (M8) left all 258 lib tests and every integration test of `willikins-providers-fake` green. The first pass fixed the same gap one level down, in the walk. This is its top-level twin. | `f0fe7a5`. The test is written first: `read_reports_absent_when_the_config_does_not_exist_even_if_a_secret_is_seeded_under_its_key`. It is red under M8 and green with the check restored. |
| 2 | None (a conclusion, now pinned) | **Plan-to-apply drift through `inherit` cannot make the gate's `Present` stale.** The gate observes at plan time, and `apply` reuses a pure node's planned outputs (decision (c2)). If `inherit` could *replace* a config's inheritance with a set that drops the base holding the name, the run would end green and only the next run would block. Decision (c2) and the gate's `how` only cover the `Blocked` direction. The other direction is closed by `doppler.config.inherits.ensure` being add-only. A config that already inherits a base outside the requested set reads `Mismatch`, live (`config_inherits_ensure.rs`'s `observe`) and fake alike, and `plan` refuses with `AttributeMismatch`. | `f43bc2a`. `a_config_inheriting_a_base_the_document_does_not_name_refuses_to_plan` runs over the tracked document. Under M14, with the fake's `Mismatch` arm disabled, the same state plans `inherit` `Create`, replacing `[other_keys/prd]` with `[shared_keys/prd]`, and plans `apns_key` `Compute`: exactly the stale `Present` described above. The test kills that mutant. |
| 3 | Hygiene | **`client.rs` still said verify item 1 was unresolved and the gate was "not yet written".** It also kept two `#[allow(dead_code)]` attributes whose reason no longer held. Both earlier reviewers flagged this. | `3da9800`, doc only: the settled answer replaces the "unresolved" paragraph, and both `allow`s are gone. Clippy with `-D warnings` is green without them. |

## Mutations (each saved to the scratchpad before editing, restored after, `touch`ed, and `cmp` byte-identical)

| # | Mutation | Result | Killed by |
| --- | --- | --- | --- |
| M8 | Fake `doppler_secret_name_gate.rs::observe_state`: `if !state.doppler_configs.contains(&key)` → `if false && ...` | **survived** the whole fake crate, then killed once finding 1's test landed | `read_reports_absent_when_the_config_does_not_exist_even_if_a_secret_is_seeded_under_its_key` (new) |
| M9 | Live `secret_name_gate.rs`, the walk's error arm: `Err(err) if looks_like_a_missing_project(&err) => {}` → `if true \|\| ...`, so every base error contributes nothing | killed | `unlisted_base_answers_403_is_err` |
| M10 | Live `secret_name_gate.rs`, the direct call's missing-project arm: `return Absent` → fall through to the walk | killed | `names_on_config_answers_404_reads_absent_with_no_walk` and its `400` twin. They fail on a `501` from the unmocked base route their config fixture names. They do not fail on the config mock's `.expect(0)` assertion, which the panic reaches first. With a fixture whose `inherits` is empty, `.expect(0)` would be the assertion that kills it. Either way the mutant dies. |
| M11 | `willikins-types` `DopplerProject` pattern → the flat `[a-z0-9_-]+` | killed | `doppler_project_accepts_and_refuses_the_a1_rows`, `doppler_config_agrees_with_the_regex_on_every_a1_row`, `doppler_project_rejects_leading_hyphen`, `catalog_json_snapshot` (run with `INSTA_UPDATE=no`, so no `.snap.new` was written) |
| M12 | `client.rs::secret_name_listed`: `listed == name.as_str()` → `listed.eq_ignore_ascii_case(name.as_str())` (case, where the first pass's M1 tested a prefix) | killed | `client::tests::a_prefix_suffix_or_case_near_miss_never_counts_as_listed` (the `example_apns_key` row) |
| M13 | `workflows/doppler-inherited-secret-gate.yaml`: `apns_key.config` → `${{ steps.prd_config.config }}` (decision (c2)'s declined-fallback edge) | killed | `apns_key_config_binds_inherit_config`; `a_non_inheritable_base_blocks_its_gate_and_skips_inherit_and_the_key` (`apns_key` plans `Blocked`, not `Skip`: two blocked gates instead of one); `characterization_of_every_document`. The snapshot pins the edge, not only the test. |
| M14 | Fake `doppler_config_inherits_ensure.rs::observe`: the `Mismatch` arm disabled (`if false && actual.difference(wanted)...`) | killed | `a_config_inheriting_a_base_the_document_does_not_name_refuses_to_plan` (new, finding 2) |

M9 and M12 were live together in one run. They sit in different files and kill different tests. Every other
mutation ran alone.

## Checked and accepted (no change)

- **No secret value, response body or other listed name reaches an output, error or log.** The names response
  parses into `SecretNamesBody` (no `Debug`, pinned by the first pass) and reduces to a `bool`. A `2xx` that fails to
  parse becomes `Http::finish`'s content-free `could not parse the response body as the expected shape (line N,
  column M)`. serde's own message is never used, because it quotes the offending value. The gate then rewraps that
  error as a message naming only `config` and `name`. A non-`2xx` keeps only a bounded `message`/`messages` field, and
  a `401`/`403` body is dropped entirely (`provider_error_from_body`). Neither provider crate has a `tracing` call.
  The live cycle prints only step lines, throwaway project names and the one verify-item-1 boolean.
- **No `Present` for a name that was not found.** The comparison is byte-exact (M1, M12). An empty list is `false`. A
  `null` or missing `names` is a `2xx` parse error, so the read fails with an error. Every error status except the
  missing-project pair fails loudly, on the direct call (first pass's M2) and in the walk (M9). The missing-project
  pair reads `Absent`, never `Present` (M10). Doppler's reference documents no paging for the names endpoint. If the
  list were ever paged, a name on a later page would read `Absent`: a false block, never a false `Present`.
- **The widened `DopplerProject` admits nothing Doppler would not.** Leading, trailing, doubled and mixed separators,
  uppercase, `%`, `&`, `?`, `#`, `/`, `+`, a space and a trailing newline are all refused (M11). The derive anchors
  the pattern as `^(?:...)$`, and the published schema pattern is anchored too. Every admitted character is an RFC
  3986 unreserved character, so the unescaped `?project={project}` in every request line cannot smuggle a parameter.
  A `ConfigRefBody` parsed from a response goes through the same grammar.
- **`naming::v1` is unchanged.** `git diff a96fce5..HEAD -- crates/willikins-types/src/naming.rs` is empty.
- **The iOS app document's ordering.** The gate binds `inherit.config` in the tracked document (M13). In the gitignored iOS app
  document, its `m5` node is `doppler.secret_name.gate` with `config: ${{ steps.inherit.config }}`. That document no
  longer declares the acknowledgement input. The input's name survives only in comments and in one assertion that it
  is gone.
- **The fake agrees with the live tool** on every branch `fake_agrees_with_live.rs` expresses. With findings 1 and
  M7, a secret seeded under a config or base the fake does not know exists can no longer make the fake read
  `Present`.
- **Characterization.** `git diff a96fce5..HEAD` over the snapshot holds C1's six signed-off lines (`plan_json` and
  `fingerprint` of the three inheriting documents, nine `inherit` instances, each gaining only the known `config`
  output) plus C2's four new entries. It also holds one header line, `assertion_line: 463`, added by `7f0ec2d`. That
  is insta metadata, present in about twenty other snapshots in the workspace, and not a document entry, so it is
  left alone. This pass changed no document.
- **Privacy.** `git log -p` over every commit from `a96fce5` to this pass's last one, messages included, has zero
  matches against the local hooks' pattern list, and the tree at `HEAD` has none either.

## Design notes for the coordinator (not defects)

- **Verify item 1 is settled: the names endpoint lists inherited names.** Under that answer, the walk runs only when
  Doppler's own direct listing says the name is not visible. It can turn that answer into `Present` only if a base
  lists the name while the config's own listing does not. That happens in a race (the secret is stored between the
  two reads, and then `Present` is true), or if Doppler's two endpoints ever disagree. The walk's real cost is
  elsewhere. A base that answers `403` or `5xx` turns a correct `Absent` into an `Err`, so the run fails instead of
  blocking. Whether to keep the walk as a backstop or drop it is a reviewed change of its own. This pass does not
  make it.
- **A branch config's own root config.** Doppler's branch configs inherit their environment root's secrets. The fake
  models only explicit config inheritance (`doppler_config_inherits`), not root-to-branch. If the live endpoint lists
  root names on a branch config (likely, but unverified), the fake reads `Absent` where live reads `Present`. That is
  a false block in fake-driven tests only, never a false `Present`. No document in the tree gates on a root-held name.
