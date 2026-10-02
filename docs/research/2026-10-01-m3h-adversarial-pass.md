---
title: "Milestone 3h adversarial pass: the membership tool, Action::Update, and Walter's release pipeline"
created: 2026-10-01
status: complete
area: doppler, core, walter
related:
  - docs/plans/2026-10-01-milestone-3h-walter-release-pipeline.md
---

# Milestone 3h adversarial pass

An independent attack on everything milestone 3h landed (D1 through W7, `60e54d6`..`1276c58`), by a reviewer that
wrote none of it. Inputs: the plan, the monorepo cookbook
(`bande-a-bonnot/docs/solutions/ci-cd-patterns/2026-09-10-buildkite-self-hosted-ios-cicd-cookbook.md`), Pocket's and
Danksworth's `.buildkite/` and `tools/`, the pinned `tart-ci` (`11fc336`), and the 35 files the document renders for
the real identifiers. No provider was called. No credential was read.

At the start, nothing was uncommitted and nothing was red. The rendered set compiled under `/usr/bin/python3` 3.9.6,
its 35 offline tests passed, `bash -n` passed on both shell files, and every rendered `.yml` and `.json` parsed.

## Findings and fixes

Each fix was test-first: the new assertion was red against the old code, then green after the fix.

| # | Severity | Finding | Fix (commit) |
| --- | --- | --- | --- |
| 1 | Medium | `SigningKeychain` also used the Doppler-held `APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD` as the disposable keychain's password. The secret therefore reached four argv lists (`create-keychain -p`, `unlock-keychain -p`, `import -P`, `set-key-partition-list -k`), where decision (i) specifies "a random password" and W4's addendum says the password is "generated". Nothing logged or persisted it: `_run` and `_run_ignore` report only fixed strings and swallow `TimeoutExpired` (whose message carries the argv) with `from None`, and no stderr is printed. | The keychain gets its own `secrets.token_hex(24)`. The `.p12` password appears only in `security import -P`, which has no stdin form (Danksworth's Fastfile does the same). The rendered test pins it to exactly one recorded call. Before the fix that test found the password in calls `[2, 5, 6, 8]` (`2a36f1d`). |
| 2 | Low–medium (cookbook divergence; would not fail CI) | `_signing_checks` ran `assert_clean` only after a clean exit from the keychain block. A failing signing check or package build skipped it, but cookbook §6.1 point 7 and the §11 checklist require the check "on the failing path as well as the passing one". | `assert_clean` now runs in a `finally` whenever a search list was captured. If it also fails on an already-failing path, its error (a leak) supersedes the original one. The new rendered test makes the step fail inside the keychain. It was red before the fix ("called 0 times") (`5056115`). |
| 3 | Low (test gap) | Widening `DOPPLER_STAGE_NAMES["upload"]` to the signing set, which would hand the upload guest the certificate and its password, passed all 36 rendered tests. That mapping is trust boundary 6. | The `fetch_doppler` test pins the upload stage's names to `ASC_SECRET_NAMES` (`4b03410`). |
| 4 | Low (test gap) | A case-insensitive service-account name match (`eq_ignore_ascii_case`) survived all 40 tests in `project_member_ensure_mock.rs` and `fake_agrees_with_live.rs`. Dropping the member-type filter (`type == "service_account"`) also survived. | Added `only_a_byte_for_byte_name_match_resolves` (another case, a suffix and a prefix never resolve and never create ambiguity) and `a_member_of_another_type_with_the_same_slug_is_not_this_service_account` (`6148101`). |
| 5 | Low (test gap) | `ensure_on_needs_update_patches_the_sorted_union_and_never_deletes` requests a superset of the member's environments, so it cannot tell a union from a request-only `PATCH`. A mutant that dropped the existing environments was killed only by the unparseable-environment test, which has a different purpose. | Added `ensure_keeps_an_environment_the_request_never_named`: the member holds `dev`, the request names `prd`, and the `PATCH` body must be `["dev", "prd"]` (`5bb1532`). |

**Document budget.** With the three template fixes, `workflows/walter-ios-app.yaml` is 196,549 bytes, against W0's
196,608. The coordinator's literal edit replaces `REPLACE-WITH-CI-SERVICE-ACCOUNT` (31 characters). It still fits if
the real name is at most 90 characters, and the type caps names at 64. Any further template change must trim a script
first (plan risk 1). I bought the room for fix 2's test by dropping an explanatory comment. The test drives
`identities()` to fail by passing no runner.

## Mutations (each restored from a saved copy, `touch`ed, `cmp` byte-identical)

| Mutation | Result | Killed by |
| --- | --- | --- |
| M-a `resolve_slug`: `==` → `eq_ignore_ascii_case` | **survived** the 40 existing tests. After finding 4, **killed** | `only_a_byte_for_byte_name_match_resolves` |
| M-t `classify`: drop `member_type == "service_account"` | killed after finding 4 | `a_member_of_another_type_with_the_same_slug_is_not_this_service_account` |
| M-b `classify`: a role ranked above the request → `NeedsUpdate` (the tool would lower it) | killed | `role_ranks_above_requested_is_mismatch`, `ensure_on_mismatch_is_conflict_and_makes_no_write`, `ensure_never_issues_a_delete`, `fake_agrees_with_live::project_member_mismatch_on_a_higher_role_agrees` |
| M-c `union_environments`: ignore the existing environments | killed only incidentally (unparseable-environment test). After finding 5, also killed by the dedicated test | `ensure_keeps_an_environment_the_request_never_named` |
| M-d `plan_one`: ask `updates` before `replaces` | killed | `plan_updates::replaces_wins_over_updates` |
| M-e fake `classify`: ignore `access_all_environments` | killed | `fake_agrees_with_live::project_member_present_with_access_all_environments_agrees` |
| M-f `DopplerProjectRole` pattern: admit `admin` | killed | `doppler_project_role_refuses_every_other_identifier` |
| P-1 rendered `walter_signing.py` before fix 1 | the new assertion was red | (fix 1) |
| P-2 rendered `walter_ci.py` before fix 2 | the new test was red | (fix 2) |
| P-5 `child_env` forwards `DOPPLER_TOKEN` | killed | `test_child_env_never_carries_doppler_token` |
| P-6 upload allowlist widened to the signing set | **survived**. After finding 3, killed | the `fetch_doppler` test |
| P-3 `cmd_upload` skips `_verify_receipt` (hashes the IPA only) | **survives** (W5's flagged gap, confirmed) | none. Not fixed: a test costs bytes the document no longer has. `stage-ipa` still verifies the receipt on the host, and the guest re-inspects. |

## Checked and accepted (no change)

- **The membership tool cannot grant `admin` or `owner`.** The grammar refuses them (M-f). It never issues a `DELETE`,
  never lowers a role (M-b), never drops an environment (M-c), matches only an exact name and a
  `service_account`-typed member (M-a, M-t), and acts only on the `project` input. Outputs carry no slug.
- **`Action::Update` in `apply`.** `Update` runs `ensure` exactly as `Create` does. `check_drift` reports an approved
  `Create` that re-plans as `Update` (and the reverse) as `DriftKind::Action` (`plan_updates.rs`). The plan
  `Create`, then a mid-run state change, then a `PATCH` sequence is a TOCTOU window accepted by decision (b), since
  an update deletes nothing. No server or CLI code branches on `Action` beyond rendering and drift.
- **The fake versus the live tool on a missing project.** The live tool reads `Absent`, its `POST` fails, and the
  re-read returns the original error. The fake grants membership in a project its state never created. That matches
  every other fake Doppler tool (`doppler.config.ensure` and others do not check the project either). In Walter,
  `doppler` always precedes `ci_doppler_access`.
- **No route from a secret or `Text` into a committed file.** The only `TemplateValue` conversion is from
  `AppleBundleIdentifier`. Every `values:` binding is one of the three bundle-id inputs. There are 12 placeholders,
  all in quoted or identifier-only positions (test green). The final render contains no `/Users/`, no `team_id`, no
  token-shaped literal, no `shell=True`/`os.system`, and no `gh ` call.
- **Against the cookbook and Pocket.** Secret names match the real workplace (`APP_STORE_CONNECT_API_KEY_*`,
  `APPLE_DISTRIBUTION_CERTIFICATE_*`, `GH_CLONE_TOKEN`). The Doppler source is `walter`/`prd_deployment_ios`, the
  config `prd_config` creates. Profiles are fetched by `filter[name]` = bundle id, `IOS_APP_STORE`, `ACTIVE`, matching
  the profile nodes' `name` binding. `doppler_token_secret` appears on exactly the preflight, package and upload
  steps. The guest `env` allowlists are the seven identity names. The checkout helper is §3.3's, scoped to the
  monorepo URL, with the SSH→HTTPS `insteadOf`. The hook is byte-for-byte Pocket's shape, sourced, with no
  `exec`/`exit`. The pinned `tart-ci` exports the Doppler token only into the guest script, never into the host env
  that the hook inherits. `.ci-artifacts/` is ignored by the scaffolded `.gitignore`, so `stage-ipa`'s host writes
  keep the upload guest's `git status --porcelain` clean.
- **Entitlement and profile selection under `--config=beta --config=ios_device`.** `beta` uses `ci-base`, not `ci`,
  so `define ci=true` is unset. Entitlements resolve to the files and profiles to the `*_distribution_profile`
  targets. The validation build (`ci` plus `ios_sim`) matches two conditions that both yield `None`, which Bazel 8
  accepts.
- **IPA extraction with `zipfile` instead of Pocket's `ditto -x -k`.** Verified on this host: an ad-hoc-signed bundle,
  zipped and extracted with `zipfile` (mode bits lost), still passes `codesign --verify --strict --deep`.
- **The private clone.** `git clone --no-hardlinks <mount>` copies the whole object store (the monorepo's `.git` is
  308 MB) over virtiofs. Pocket clones `file://` with `--depth=1 --no-checkout`. This is a performance difference
  only, within the 600-second timeout.
- **`CFBundleVersion` `0.1.0.<n>` (four components).** It is the same `apple_bundle_version` shape Pocket and
  Danksworth use, and Pocket's build 20 uploaded with it.
- **m5** (`m5_apns_key`, `m5_apns_key_done`) is byte-identical to `9f0216e`. Every characterization hunk since
  `9f0216e` falls inside `workflows/walter-ios-app.yaml`'s own entry, and the three fixes here change no port.
- **The D4 harness** refuses unless `GET /v3/me` names `Willikins - Test - Sandbox` and no probe-prefixed name already
  exists. Not run here.

## Verify (could not be settled offline)

- **The shape of `com.apple.developer.devicecheck.appattest-environment` in an App Store profile.** `check_signing`
  requires equality with the entitlements file's string `production`. If Apple lists it as an array, the signing
  preflight fails closed with "the entitlement ... does not match", which is the job's purpose. No local profile was
  available to read.
- **WWDR G3 from `https://www.apple.com/certificateauthority/AppleWWDRCAG3.cer` through the non-redirecting opener**
  (plan verify item 11). A redirect fails the keychain lifecycle with a fixed message.
- **`bazel cquery --output=files` yields exactly one file for `ios_application`** (W5's note, plan verify item 4).
