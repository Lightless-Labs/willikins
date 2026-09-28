# Milestone 3e: adversarial pass over capability settings and `github.repo.get`, and the live capability cycle

**Date:** 2026-09-28
**Task:** the independent attacker's pass over tasks 1 and 2 of
`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, then the plan's live capability cycle, once.
**Subject:** T1 (`4fc51f2`, `f25b876`, `9c088e0`, `676c091`: `AppleCapabilitySetting`, the
capability/setting pairing enforced at `read`, the settings-aware read, the create body, the fake twin,
and `tests/live_capability_cycle.rs`) and T2 (`ac670a8`, `736e35c`: `github.repo.get`). T3, the Walter
document, was not written and was not touched.
**Method:** read the plan and every landed file, then attack through tests. Every mutation was made by
saving the file to the scratchpad, editing it, running the narrowest test target, restoring it from the
saved copy, and confirming byte identity with `cmp` and an empty `git diff --stat` (never `git
checkout`, `reset` or `stash`). The live run resolved the Apple credential out of the sandbox Doppler
workplace inside the command that used it and printed counts, statuses, key names and booleans only.

## 0. The state the pass started from

T1 and T2 were committed; the scoped test binaries they touch were green (capability mock 15, fake
parity 21, capability documents 1). `live_capability_cycle.rs` had been compiled with `--no-run` and
never run. T1 had added three data-protection fixtures without rows in
`crates/willikins-providers-appstore/fixtures/appstore/README.md`; they now have rows (`c054a09`).

## 1. What was attacked, and what held

### Only the four expressible `KEY=OPTION` pairs are accepted

`AppleCapabilitySetting`'s pattern is an alternation of the four pairs. The derive macro anchors every
pattern as `^(?:…)$` (`crates/willikins-derive/src/attrs.rs`, `anchored_pattern`), so the alternation
cannot degrade into a prefix match on the first member and a suffix match on the last. The type's own
tests refuse `ICLOUD_VERSION=XCODE_6`, a bare key, a bare option, lowercase, `KEY=`, `=OPTION`, the
cross pair `APPLE_ID_AUTH_APP_CONSENT=COMPLETE_PROTECTION` and the empty string, and mutation 1 shows
they bite. It is registered non-secret.

### A wrong pairing is refused at plan time, before any request or write

`read` and `ensure` both call `check_setting_pairing` before `client_for` mints a JWT. `plan` calls
`read` on every node whose key ports are known (`crates/willikins-core/src/plan.rs`, `plan_one`), so a
mismatched pair fails the plan before any node is ensured. The three mock tests arm every endpoint with
`.expect(0)`; mutation 2 (the check removed from `read`) turns all three red with `Provider` instead
of `Invalid`, because a request then went out. The negative fixture
`workflows/fixtures/appstore-capability-setting-mismatch.yaml` fails `plan` against the fake catalog with
`PlanError::Tool { healthkit, Invalid }` and no state written; mutation 6 (the check removed from the
fake) turns it red, and mutation 6b shows `fake_agrees_with_live` catches the same drift on its own.

### `APP_GROUPS`, `APPLE_PAY` and `ICLOUD` are still refused

Unchanged by T1: `ensure` refuses all three on the `Absent` path with `Invalid` and never `POST`s
(mutation 5), and converges them when already `Present`. `ICLOUD_VERSION` is outside the grammar, so no
iCloud setting can reach a request either.

### The create body

With a setting, an exact JSON matcher (`mockito::Matcher::Json`, full equality) pins `capabilityType`,
the one-element `settings` and the `bundleId` relationship; without one, the body has no `settings`
key at all. Mutation 4 (dropping `skip_serializing_if`, so the key is sent as `null`) turns the
no-setting test red.

### A secret never reaches an error or a log

Every message the pairing check builds names only the capability and the setting key. The live tool's
error paths go through `ProviderError`'s conversion, whose `401`/`403` arms carry the fixed
`UNAUTHENTICATED`/`MISSING_PERMISSION` text and never the body. The live cycle's `tool_ok` prints the
error kind and withholds any provider text; the credential decode drops the `FromUtf8Error` rather than
`Debug`-printing it. Over the three live logs of section 4: UUID shapes 0, `eyJ` 0, `PRIVATE KEY` 0,
runs of 24 or more hex characters 0, base64 runs of 200 or more 0, throwaway identifiers 0.

### `github.repo.get` never creates or writes, and handles 404 and archived as the plan says

`pure: true`, no key, `Reversible`; `ensure` is `read` with `changed: false`, and the only client call
either makes is `GitHubClient::get_repo` (a `GET`, retried only on a secondary rate limit). The mock
suite records only `GET`. `404` → `NotFound`; archived → `Conflict` (mutation 7 turns both the mock test
and `fake_agrees_with_live::agrees_on_archived` red); `401`/`403` → the fixed messages; a repository
without the ownership topic is `Present`. The shared `Http` sets `max_redirects(0)`, so a renamed or
transferred repository's `301` is a `Provider` error, never a `Present` for a repository under another
name: conservative, and a `plan` failure before any write. `LIVE_TOOL_NAMES` is 26 in
`crates/willikins-server/src/catalog.rs` and `crates/willikins-providers-doppler/tests/live_catalog.rs`
(which reuses the server's constant); no pinned `25` or "twenty-five" remains. The only
`GitHubRepoRecord { … }` literal outside the fake crate is the one `736e35c` fixed.

### Fake and live agree

`fake_agrees_with_live` covers the pairing refusal, `Present` with the requested option, and three
`Mismatch { setting }` arms (a different option, `settings: []`, `settings` absent). One limit, stated
rather than fixed: the fake stores one option per identifier and capability, so the "two options
enabled" row of finding 2 cannot be expressed on the fake side; that arm is pinned on the live tool's
mocks only.

## 2. Findings

1. **A capability read failed on another row's unexpected settings shape** (`c054a09`). T1 made every
   capability read parse every row's `settings` with `key`, `options` and `enabled` all required;
   before T1 the read parsed `capabilityType` alone. An `ICLOUD` row whose option omits `enabled`, or
   whose `options` is `null`, turned a `HEALTHKIT` read into `Provider: could not parse the response
   body` (red first:
   `read_of_a_capability_is_unaffected_by_another_rows_unexpected_settings_shape`). All four fields are
   now `Option` with `#[serde(default)]`; a missing `enabled` is "not selected", so a requested setting
   in that state is `Mismatch`, never `Present`.
2. **The read accepted a second enabled option** (`92dec9e`). Decision (d) says `Present` needs exactly
   one enabled option and that it be the requested one; the code asked only whether the requested one
   was among the enabled. A row with `COMPLETE_PROTECTION` and `PROTECTED_UNTIL_FIRST_USER_AUTH` both
   enabled read `Present` (red first:
   `read_reports_mismatch_when_another_option_is_enabled_beside_the_requested_one`). It now collects
   every enabled option under the key, across every entry carrying it, and reads `Present` only for
   exactly the requested one.
3. **The live cycle could not settle verify item 2 as written** (`d4abcf4`). It recorded an absent
   `enabled` as `false` and an absent `settings` as `[]`. Both are now `Option`s, and the cycle prints
   Apple's JSON field names (schema, never values).
4. **The live cycle recorded the shape only after asserting convergence** (`1328d59`, after the live
   run). The run stopped at that assertion, so the shape was lost. The cycle now records the row, its
   field names, the JSON kind of `settings`, and the read-back variant straight after the create,
   before any further assertion. Written, not run.

### A T3-blocking finding, recorded and not fixed

`plan` calls `read` on every node whose key ports are known. `appstore.bundle_id.ensure`'s `Absent`
observation predicts its `identifier` output from its input, so a capability node that binds
`identifier: ${{ steps.app_id.identifier }}` (decision (c), and every capability node in the planned
Walter document) has a known key at plan time and is read, and `appstore.bundle_id_capability.ensure`'s
`read` refuses a missing parent with `NotFound`. Reproduced against the fake catalog with a two-node
document (the credential chain, `app_id` on a fresh identifier, `healthkit` downstream), added to
`tests/capability_documents.rs` for one run and removed again (`cmp` identical):

```
EXPERIMENT: plan failed: Tool { node: NodeName("healthkit"), error: ToolError { kind: NotFound,
message: "no App Store Connect bundle id has identifier `com.example.Fresh`; run
appstore.bundle_id.ensure first" } }
```

So the first `plan` of the Walter document on any fresh identifier fails before any write, decision
(c)'s "the backstop, not the mechanism" fires at plan time, and acceptance 8 ("plans against the fake
catalog with throwaway inputs … a fake `apply` then a second `apply` reads every node `Unchanged`")
cannot pass as written. The in-crate precedent for the other choice is `appstore.profile.ensure`, which
reads `Absent` when its identifier is not registered (`fake_agrees_with_live`,
`profile_absent_when_identifier_not_registered_agrees`). Which way to go (`Absent` with predicted
outputs when the parent is missing, keeping `NotFound` for `ensure`; or an engine change) is the
coordinator's decision, not this pass's; the tool is unchanged.

## 3. Mutations

| # | Mutation | Test that failed |
| --- | --- | --- |
| 1 | `AppleCapabilitySetting`'s pattern also admits `ICLOUD_VERSION=XCODE_6` | `willikins-types` `capability_setting_refuses_icloud_version_a_bare_key_a_bare_option_lowercase_and_a_mismatched_pair` |
| 2 | `check_setting_pairing` removed from the live tool's `read` | the three `read_refuses_…_and_makes_no_request` mock tests (`Provider` instead of `Invalid`: a request went out) |
| 3 | `&& option.enabled` dropped from the settings match | `read_reports_mismatch_when_a_different_option_is_enabled`, `ensure_refuses_a_setting_mismatch_with_conflict_and_never_posts` |
| 4 | `skip_serializing_if` dropped from the create body's `settings` | `ensure_creates_an_ordinary_capability_when_absent` (exact JSON matcher) |
| 5 | `APP_GROUPS` removed from `CAPABILITIES_NEEDING_PORTAL_CONFIGURATION` | `ensure_refuses_app_groups_apple_pay_and_icloud_when_absent_and_never_posts` |
| 6 | the fake's `check_setting_pairing` removed from `read` | `capability_documents::the_setting_mismatch_fixture_checks_cleanly_but_fails_plan_naming_healthkit` |
| 6b | the same, parity binary alone | `fake_agrees_with_live::capability_setting_pairing_refusal_agrees_by_error_kind` |
| 7 | `github.repo.get`'s archived arm disabled | `repo_get_mock::read_reports_conflict_when_archived`, `fake_agrees_with_live::agrees_on_archived` |

Every kill was on a freshly built mutant: each edit gave the file a new mtime. **But the restore did
not force a rebuild.** The helper restored with Python's `shutil.copy2`, which copies the saved file's
*old* mtime back, so cargo judged the restored source older than the mutant artefact and kept the
mutant. It surfaced when finding 1's green run failed `capability_setting_pairing_refusal_agrees_by_error_kind`
with the fake answering `NotFound`: the fake crate was still mutation 6b's build. Every restored source
was then `touch`ed (bytes unchanged, `cmp` identical), the helper switched to `copyfile` plus a fresh
mtime, and the scoped pre-live gate below is what re-verified the restored tree.

**Scoped pre-live gate, on `d4abcf4`** (`RUST_TEST_THREADS=2`, `-j 2`): `willikins-providers-appstore`
every target green (capability mock 17, fake parity 21, capability documents 1,
`no_certificate_writes_guard` 27, the rest unchanged); `willikins-types` `capability_setting` 5;
`willikins-providers-github` every target green (`repo_get_mock` 9 among them); `secret_literal_guard`
15; `no_gh_writes_guard` 7; `willikins-dsl --test acceptance` 3 (the characterization snapshot
unchanged); `fmt` clean; `clippy -p willikins-providers-appstore --all-targets` with and without
`live-tests` clean.

## 4. Live run

The operator's **production** developer account, Team key, 2026-09-28, three processes, each with its
own JWT, the credential resolved from sandbox Doppler `app-store-connect/prd` in the same command
(token on `curl`'s standard input; values through `printf` pipes, never here-strings).

**(a) Independent count, before** (`live_probe::appstore_counts_and_leftovers_probe`, `GET` only):
certificates **5** (4 `DEVELOPER_ID_APPLICATION_G2`, 1 `DISTRIBUTION`), profiles **13** (11
`IOS_APP_STORE`/`ACTIVE`, 2 `IOS_APP_STORE`/`INVALID`), bundle identifiers **21**, `meta.paging.total`
agreeing on all three; throwaway leftovers 0 identifiers, 0 profiles. The same numbers milestone 3c
recorded.

**(b) The capability cycle, once** (`live_capability_cycle`, `live-tests`, `WILLIKINS_LIVE_TESTS=1`,
binary built from `d4abcf4`, which includes findings 1 and 2), on one `UNIVERSAL` throwaway
`com.willikins.probe.delete-me.<pid>-<unix>`:

```
CAPABILITY-CYCLE counts BEFORE: bundle_ids=21 certificates=5 profiles=13
HEALTHKIT: created, then converged
PUSH_NOTIFICATIONS: created, then converged
STOP at data protection re-ensure: Conflict: <withheld: may quote provider text>
GUARD deleted the throwaway identifier by its recorded id
test result: FAILED. 0 passed; 1 failed
```

`HEALTHKIT` and `PUSH_NOTIFICATIONS` were each created (`changed: true`) and re-ensured `changed:
false`. `DATA_PROTECTION` with `PROTECTED_UNTIL_FIRST_USER_AUTH`: the create carrying
`settings: [{"key": "DATA_PROTECTION_PERMISSION_LEVEL", "options": [{"key":
"PROTECTED_UNTIL_FIRST_USER_AUTH", "enabled": true}]}]` was accepted (`changed: true`); the
re-ensure returned `Conflict`. With one parent, `Conflict` on that path can only be
`setting_mismatch_conflict`: the tool read the row it had just created as `Mismatch { setting }`.
That is trust boundary 6's surprise, and risk 1 of the plan exactly. The guard deleted the identifier
by the id its own create returned; no second run was made.

**(c) Independent recount, after** (the same probe, a separate process): certificates 5, profiles 13
(11 `ACTIVE`, 2 `INVALID`), identifiers 21, leftovers 0 and 0. Equal to (a). The cycle's own step 8
(its counts and the `404` read of the deleted id) never ran; the recount and the zero leftovers stand in
for it, and the id was never printed, so the `404` read cannot be made after the fact.

### What the `Conflict` does and does not tell

The row's shape was not recorded (finding 4). The candidates, and how the pass's own commits bear on
each:

- `settings` absent or `null` on the list endpoint → `Mismatch` with or without this pass's commits.
  **If this is it, decision (d)'s read-back mechanism is unworkable, not merely mis-parsed**: the list
  endpoint would never show which option is set, and the tool needs another source or a different
  convergence rule. That is a design decision.
- a different option enabled (Apple applied its own default and ignored the requested one) →
  `Mismatch` either way. The create would then be doing less than it claims.
- no option enabled → `Mismatch` either way.
- the requested option enabled beside another → `Present` before `92dec9e`, `Mismatch` after. Finding
  2's tightening could be what fired.
- `enabled` absent on every option → a `Provider` parse error before `c054a09`, `Mismatch` after.

Only a run that records the shape first can discriminate, and the cycle now does (`1328d59`). Whether
to run it again is the coordinator's call.

## 5. Verify items

- **1 (a create carrying `settings` succeeds):** settled, **yes** — accepted, `changed: true`. Whether
  Apple *applied* the option is item 2.
- **2 (how a row reports its selected option):** **unsettled**, as above.
- **3 (can the Team key enable capabilities):** settled, **yes** — `HEALTHKIT` and `PUSH_NOTIFICATIONS`
  created and converged, and the `DATA_PROTECTION` create was accepted. The probe identifier was
  `UNIVERSAL`; Walter defaults to `IOS`.
- **4, 6:** not exercised (the tool refuses `DATA_PROTECTION` with no setting before any request, and
  list-then-branch never re-`POST`s).
- **11:** unsettled; the probe prints no platforms.

## 6. Not settled

- Verify item 2, and with it whether decision (d)'s read-back works at all (section 4).
- The T3-blocking plan-time `NotFound` (section 2).
- Any `APPLE_ID_AUTH` behaviour live: never exercised, by trust boundary 4.

## 7. Host conditions worth knowing

The data volume stood at 97 % (14 GiB free) throughout; no build died. No `cargo-sweep` ran during
the pass. Each capability-crate test rebuild cost one to two minutes because `willikins-server` is a
dev-dependency. The `copy2` lesson of section 3 applies to every future mutation pass on this host:
restore with a fresh mtime (`touch` after `cp -p`, or a plain `cp`), or cargo keeps the mutant.
