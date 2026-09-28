# Milestone 3e: adversarial pass over capability settings and `github.repo.get`, and the live capability cycle

**Date:** 2026-09-28
**Task:** the independent attacker's pass over tasks 1 and 2 of
`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, then the plan's live capability cycle, once.
**Subject:** T1 (`4fc51f2`, `f25b876`, `9c088e0`, `676c091`: `AppleCapabilitySetting`, the
capability/setting pairing enforced at `read`, the settings-aware read, the create body, the fake twin,
and `tests/live_capability_cycle.rs`) and T2 (`ac670a8`, `736e35c`: `github.repo.get`). T3, the Sample
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
Sample document) has a known key at plan time and is read, and `appstore.bundle_id_capability.ensure`'s
`read` refuses a missing parent with `NotFound`. Reproduced against the fake catalog with a two-node
document (the credential chain, `app_id` on a fresh identifier, `healthkit` downstream), added to
`tests/capability_documents.rs` for one run and removed again (`cmp` identical):

```
EXPERIMENT: plan failed: Tool { node: NodeName("healthkit"), error: ToolError { kind: NotFound,
message: "no App Store Connect bundle id has identifier `com.example.Fresh`; run
appstore.bundle_id.ensure first" } }
```

So the first `plan` of the Sample document on any fresh identifier fails before any write, decision
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
  `UNIVERSAL`; Sample defaults to `IOS`.
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

## Capability read fixes, independent review, 2026-09-28

**Subject:** `fcdb9da` (the key-only read rule), `a19247e` (`Absent` for an unregistered parent),
`2bb3773` (the plan-only graph test) and `3f88c4c` (the live cycle's set comparison), against the live
shape recorded in the plan's implementer addendum of the same date: setting entries `{key, options}`,
option entries `{key}` only, the selected option the single listed one, `settings: null` on rows
without a setting, row order unstable across reads, `IN_APP_PURCHASE` present by default.
**Method:** read the four commits, the tool, its fake twin, the parity and graph tests and the live
cycle; attack through mock and fake tests only. No live test and no provider call of any kind. Every
mutation was made by a helper that saved the file to the scratchpad, edited it, ran the narrowest test
binaries, then restored it with a plain byte copy **and a fresh mtime** (`copyfile` then `utime`, the
`copy2` lesson of section 3), and checked `cmp` against the saved copy and an empty `git diff --stat`
for that path. Every restore printed `cmp=0` and an empty diff; the post-mutation gate below rebuilt the
restored tree.

### Findings

1. **A row mixing `enabled` and bare options read `Present`** (`9acecc4`). `fcdb9da` chose the rule by
   whether *any* option carries `enabled`. The row `[{PROTECTED_UNTIL_FIRST_USER_AUTH, enabled: true},
   {COMPLETE_PROTECTION}]`, requesting the first, took the `enabled` branch, where a bare option counts
   as unselected, and read `Present` (red first:
   `read_reports_mismatch_when_an_enabled_option_sits_beside_a_bare_listed_one`, which observed
   `Present(capability: DATA_PROTECTION)`). The same commit's `client.rs` doc says a bare option is
   "simply listed", i.e. selected, so under the observed rule that row carries two selections. The
   rules disagree, a wrong `Present` plans `NoOp` silently, and a wrong `Mismatch` stops and asks: the
   `enabled` rule now applies only when **every** option carries the field, the key-only rule only when
   none does, and a mix is `Mismatch { setting }`. Never observed live; mock-only (the fake holds one
   option per identifier and capability). The same commit restores a `Present` case for the `enabled`
   branch (`..._one_enabled.json`), which had lost its only positive fixture when `fcdb9da` rebuilt the
   fixtures, and adds both fixtures' rows to the fixtures README.
2. **A survivor: the two-listed test requested the option listed second** (`79994c8`). Mutation A
   (`[only]` → `[only, ..]`, i.e. "the first listed option decides") passed all 21 mock tests, because
   `..._two_listed.json` lists `COMPLETE_PROTECTION` first and the test requested
   `PROTECTED_UNTIL_FIRST_USER_AUTH`. A second test requests the first-listed option; mutation A2, the
   same edit, now fails it. Row order is not stable across reads (`3f88c4c`), so option order must not
   decide the answer either.
3. **The graph test stopped at `plan`** (`a468df2`, a test, not a code defect). `2bb3773` proved both
   nodes plan as `Create`, not that `apply` orders the capability after its parent or that the
   document converges. A sibling test applies the same inlined document on the fake catalog, asserts
   `HEALTHKIT` landed on the freshly registered identifier, and re-plans with both nodes `NoOp`
   (acceptance 8's shape). Green on the unmutated tree; mutation E below turns it red.

### What held

- **Present for the wrong option, Mismatch for the right one**, on the observed shape: one listed
  option equal to the request is `Present`; one listed option that differs, zero options, `settings:
  []`, `settings: null`, the key absent, an option without `key`, and two listed options (either one
  requested, after finding 2) are all `Mismatch { setting }`. `settings: null` on a capability that
  requires a setting is `Mismatch`, never `Present`. A setting present on a row whose capability takes
  none is ignored: the request carries no setting, the pairing check allows none, and the read is
  `Present` — correct, since this tool could not change it anyway. Every option carrying `enabled` with
  exactly the requested one `true` is `Present`; two `true`, none `true`, or a different one `true` is
  `Mismatch`. `enabled: null` deserializes as absent. Three of these arms were concluded by reading
`observe` and have no fixture of their own: `settings: null` on the requested capability (mocks cover
`settings: []` and, in the parity test, `settings` absent), an option without `key`, and
`enabled: null`.
- **`Absent` for a missing parent never lets `ensure` write against it.** `ensure` re-resolves the
  parent itself; with none, its `Absent` arm returns `NotFound` naming `appstore.bundle_id.ensure`
  before the portal-configuration check and before any `POST` (`.expect(0)` on the create mock). The
  create-failure re-read passes the resolved parent. The fake mirrors both. The new apply test shows the
  engine runs the capability node after the registration node through the data edge.
- **Fake and live agree** on every case the fake can express: `Absent` for an unregistered parent,
  `NotFound` from `ensure` for one, `Present`/`Mismatch` on the setting, the pairing refusal.
  `fake_agrees_with_live` 22 green. The mixed, two-listed and `enabled`-field rows remain mock-only, as
  recorded above for two-enabled.
- **No provider text or credential in the new paths.** The `NotFound` message names the identifier (an
  input), the `Conflict` message the capability. No new error text quotes a response; non-`401`/`403`
  provider text still flows through `provider_says` by the workspace's existing design. `3f88c4c`'s
  failing `assert_eq!` prints only the rows' `capabilityType`, setting keys, option keys and
  `Option<bool>` — the schema the cycle already prints on success, never an id or credential.

### Mutations

| # | Mutation | Result |
| --- | --- | --- |
| A | key-only arm `[only]` → `[only, ..]` | **survived** (21/21 mock green) — finding 2 |
| A2 | the same, after `79994c8` | killed: `read_reports_mismatch_when_two_options_are_listed_and_the_first_is_requested` |
| B | key-only arm drops the key comparison (`[_only]`) | killed: `read_reports_mismatch_when_a_different_option_is_listed`, `ensure_refuses_a_setting_mismatch_with_conflict_and_never_posts`, parity `capability_setting_present_and_mismatch_agree` |
| C | live `observe` returns `Present` for a missing parent | killed: `read_reports_absent_when_the_parent_bundle_id_does_not_exist`, `ensure_refuses_when_the_parent_bundle_id_does_not_exist_and_never_posts`, parity `capability_absent_when_identifier_not_registered_agrees` and `capability_ensure_not_found_when_parent_missing_agrees_by_error_kind` |
| D | live `parent_not_found` raises `Conflict` instead of `NotFound` | killed: `ensure_refuses_when_the_parent_bundle_id_does_not_exist_and_never_posts`, parity `capability_ensure_not_found_when_parent_missing_agrees_by_error_kind` |
| E | the fake's `read` refuses a missing parent with `NotFound` again (the pre-`a19247e` behaviour) | killed: both graph tests in `capability_documents`, parity `capability_absent_when_identifier_not_registered_agrees` |
| F | the fake's `ensure` drops its missing-parent guard | killed: parity `capability_ensure_not_found_when_parent_missing_agrees_by_error_kind` (the graph tests stay green, as they should: the order is right) |
| G | finding 1 reverted (`carrying_enabled == options.len()` → `> 0`) | killed: `read_reports_mismatch_when_an_enabled_option_sits_beside_a_bare_listed_one` |

`3f88c4c`'s set comparison lives inside the live-only cycle and cannot be mutation-tested without a
live run; it was reviewed by reading only. It compares sorted `Debug` strings, a multiset, so a
duplicated row still counts; it keeps each setting's option order, which is moot while Apple lists one
option.

### Not settled

- **`Absent` hides a real `NotFound` until apply.** A document binding `identifier` to a literal or an
  input that is not registered (a typo, or no `appstore.bundle_id.ensure` node) used to fail `plan`
  with `NotFound`; it now plans `Create` and fails at that node's `ensure`, after upstream nodes may
  have written. This is the accepted `appstore.profile.ensure` precedent, and the tool cannot tell a
  predicted identifier from a literal (`read` sees only resolved values); closing it needs an engine
  rule (e.g. `check` requiring a capability node's `identifier` to come from a registering node), which
  is the coordinator's decision.
- **The capability list is read as one page with no `limit`.** `list_bundle_id_capabilities` does not
  paginate. If Apple's default page is smaller than a bundle id's capability count, a row past page one
  reads `Absent` and `ensure` `POST`s a duplicate whose result is undocumented (research note, section
  2). Pre-dates these commits; the unstable order makes it bite nondeterministically. A verify item:
  the default page size of `GET /v1/bundleIds/{id}/bundleIdCapabilities`.
- **Two rows of one `capabilityType`** would be resolved by `.find` on the first, and with unstable
  order nondeterministically. Never observed.
- **`DATA_PROTECTION` enabled by hand** with no option chosen, if Apple lists it with `settings: null`,
  reads `Mismatch` for every requested level, permanently. The shape of such a row is unobserved.
- **The mixed, two-listed and `enabled`-field shapes** are defended on the live tool's mocks only; the
  fake cannot express them.

### Gate after the pass, on `79994c8`

`RUST_TEST_THREADS=2`, `-j 2`, scoped: `willikins-providers-appstore` `bundle_id_capability_ensure_mock`
22, `fake_agrees_with_live` 22, `capability_documents` 3; `willikins-providers-fake --lib` 175;
`willikins-core --test secret_literal_guard` 15; `clippy --all-targets -D warnings` clean on both
provider crates; `fmt --check` clean on both. The full workspace gate was not run (host rule).
