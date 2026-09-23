# Milestone 3c: adversarial pass over App Store signing, and the live cycle

**Date:** 2026-09-22 to 2026-09-23
**Task:** 3 of `docs/plans/2026-09-22-milestone-3c-app-store-signing.md` ("Verify and fly")
**Subject:** everything tasks 1 and 2 landed or left in the working tree:
`crates/willikins-providers-appstore` (`appstore.certificate.get`, `appstore.profile.ensure`, the
client calls under them, the certificate-write guard, the live harnesses),
`willikins-providers-http`'s 401/403 split, the seven new types in `willikins-types`, the two fake
twins, the catalogs, `workflows/appstore-signing-profile-from-doppler.yaml` and its three negative
fixtures.
**Method:** read the plan, both research notes and every landed file; attack through tests; then,
once the tree was green, fly against the operator's live production account. Every claim below is
a run, not a reading. Mutations were made by editing the source, running the named test target,
and restoring from a copy saved before the edit (never `git checkout`, never `git reset`), with
`git diff` confirmed empty after each restore. Every live command resolved the Apple credential out
of the sandbox Doppler workplace (`app-store-connect/prd`) in the same shell command that used it:
the Doppler token reached `curl` on its standard input (`-K -`), each value reached `jq` through a
pipe from the `printf` builtin, and nothing was printed. The live logs were scanned afterwards, by
count, for every leak shape (section 4).

## 0. The state the pass started from

Tasks 1 and 2 were reported as landed. The first gate run said otherwise.

- **Task 1's six commits had never passed the gates.** `cargo clippy` failed on
  `willikins-types/src/appstore.rs` (a doc line beginning `+ serial_number` read as a list item) and
  on the certificate-write guard itself (two lints). `cargo test` failed **all 14** of
  `certificate_get_mock.rs`'s tests: its list mock set no `match_query`, a mockito mock without one
  does not match a request carrying a query string, and every request the tool sends carries one,
  so each test read mockito's own `501`. The guard's own `a_turbofish_delete_to_certificates_is_flagged`
  failed (`::<()>` has a `(` inside the turbofish, which the guard's `[^>(]*` refused).
  `pure_tools_agree` failed: a pure tool was registered without a case. The fake-catalog and
  type-catalog snapshots had never been regenerated.
- **Every certificate and profile fixture spelled its dates `...000+0000`.** `chrono`'s RFC 3339
  parser refuses that offset (`input contains invalid characters`), so every arm that reached the
  date check read `Provider` instead of `Present` or `Conflict` -- 12 more failures across
  `certificate_get_mock`, `fake_agrees_with_live`, `profile_ensure_mock` and `redaction`. The live
  account spells it `+00:00`: all 5 certificates' and all 13 profiles' `expirationDate` parse as
  RFC 3339 (section 4). The fixtures were wrong, not the parser; all 21 were rewritten.
- **Task 2's uncommitted work** added two clippy failures (`type_complexity`,
  `single_match_else`) and a `too_many_lines` in the live harness; a positive document the
  server's two "exactly these documents" tests did not list; and a secret catalog example, the
  bare word `example`, which the type-catalog snapshot's secret-example redaction then replaced in
  **every other type's `"example"` key** (77 lines of the snapshot rewritten to
  `[ELIDED SECRET EXAMPLE]`).

All of it was repaired before anything else, and the repaired tree was gated as a whole: `fmt`,
workspace `clippy`, workspace tests (170 test targets, one failure -- in this pass's own new test,
fixed and re-run by name), `check -p willikins-types`. Commits `680c4ad`, `e245eb8`, `71b74b4` and
`643ce4a` carry the repair and task 2's work, each saying it was gated as part of that tree rather
than alone.

## 1. What was attacked, and what held

### The 401/403 split holds for every provider, and neither carries the body

`provider_error_from_body` returns `UNAUTHENTICATED` for a `401` and `MISSING_PERMISSION` for a
`403` before the body is parsed at all, and `From<ProviderError> for ToolError` repeats both arms
for a `ProviderError` a provider built itself. No provider crate maps either status on its own: a
search of every `crates/*/src` for `Some(401)`, `Some(403)`, `== 401`/`== 403` outside
`willikins-providers-http` finds only `404` and `409` handling. GitHub's rate-limited `403` is
distinguished by `looks_rate_limited()` on the *headers*, never the body. Mutation 4 below proves the
shared test pins the `401` wording.

### Certificate selection refuses on zero and on many, and says nothing of the operator's

`find_one` filters by `filter[certificateType]` and `filter[serialNumber]`, pages, and compares
both fields byte for byte; zero is `NotFound`, more than one is `Conflict` naming the count and no
id, expired and `activated: false` are `Conflict`. Mutation 3 proves the many-refusal is pinned.
One leak was found (finding 3): every refusal quoted the serial the document asked about.

### Profile content never reaches a non-secret port, a plan, an error, or a journal

- **A non-secret port:** `workflows/fixtures/appstore-profile-content-into-template.yaml` binds
  `steps.profile.content` to `template.render`'s `value`; `check` returns exactly
  `SecretToNonSecretSink`.
- **A plan:** the positive document's fake plan renders the profile node's `content` as
  `[REDACTED AppleProfileContent]` (`profile_documents.rs`).
- **An error or a `Debug`:** `ProfileAttributes::profile_content` deserializes straight into
  `AppleProfileContent`, whose `Debug` is the marker; `Http::finish` builds a parse failure from a
  line and column only, never serde's text; `redaction.rs` pins both.
- **The journal, stdout, and the fake state:** nothing tested these -- the document test planned
  and never applied. This pass added `crates/willikins-cli/tests/appstore_profile_apply_redaction.rs`,
  which applies the positive document through the built binary with a file journal and
  `--fake-state-out`, so the fake really creates a profile and `doppler.secret.set` really writes
  its content; the content appears in none of stdout, stderr, the journal or the dump. The dump
  **did** print it before finding 5 was fixed.

### An out-of-scope profile type is refused by type

`AppleProfileType`'s grammar is the single member `IOS_APP_STORE`;
`workflows/fixtures/appstore-profile-development-type.yaml` fails `check` with `InvalidLiteral` on
`profile_type`, and the create body's JSON matcher pins that no `devices` key is ever sent.

### The profile read compares exactly and pages, and never trusts `filter[name]`

The read never sends `filter[name]`: it resolves the identifier exactly, then reads the
*relationship* `GET /v1/bundleIds/{id}/profiles`. `read_finds_an_exact_name_match_on_page_two`
serves a same-prefix neighbour (`willikins-example-profile-extra`) alone on page one and the exact
match on page two, with page two's mock matched only on its own `cursor` query; mutations 1 and 2
prove the page-two test and the neighbour test each fail without, respectively, the pagination and
the byte-exact compare.

## 2. Findings, each now a test

1. **The certificate-write guard let eight honest shapes through** (`c99f456`). It flagged a
   statement only when the write token and `/certificates` sat in it together, split statements on
   every `}` including a format string's, and knew `.post(`/`.patch(`/`.delete(` only as method
   calls. Each of these, now a red-then-green unit test, got past it: a path bound by `let` and
   written in the next statement (the way `list_certificates` already spells its `GET`), twice-bound
   transitively, or bound by `const`; `Http::delete(&self.http, ..)`; `Method::DELETE`; `PUT`; a
   path-taking write helper called with the certificates path -- the live harness's own
   `raw_post(.., path, ..)` was exactly that shape; `format!("{BASE}/v1/certificates/{id}")`, whose
   first placeholder's `}` split the statement; and `revoke_certificate(&id)` with no path in
   sight. The guard now splits outside string literals only, treats any called identifier with a
   write verb as a part as a write, follows `let` bindings transitively within a function and
   `const`/`static` items across the file, and flags a call whose own name pairs a write verb with
   `certificate`. **Stated limits, in its module doc:** a path assembled from fragments that never
   spells `/certificates` contiguously, a path passed into a helper whose name carries no write
   verb, or one read from outside the source. The trust boundary forbids those; the guard only
   makes the honest version fail a gate.
2. **A real `201` from `POST /v1/profiles` would not have parsed** (`68f2c60`). App Store Connect is
   a JSON:API server: without an `include`, a relationship carries `links` and `meta` but no
   `data`, and a create takes no `include`. The client declared
   `relationships.certificates.data` required on every `profiles` resource, so the create's `201`
   failed to parse **after Apple had made the profile**; `ensure` then re-read, found it, and
   reported `changed: false` for a create; and the live cycle, which records a profile's id only
   from a successful `ensure`, would have left a throwaway profile on the operator's account that
   trust boundary 4 forbids it to find by name and delete. Now the relationship and its `data` are
   optional on the read side, and only the `include=certificates` instance read must carry `data`,
   its absence a `Provider` error naming the relationship, never `Mismatch { certificate }`. Three
   tests over Apple-shaped links-only fixtures, red first. The live create then parsed first time.
3. **`appstore.certificate.get` quoted the operator's serial in every refusal** (`7d2d1fd`).
   Non-secret, but the operator's (boundary 8), and a `ToolError` travels further than an input:
   an agent's transcript, a harness's panic, a journal's failure record. Now "the requested
   serial"; the fake twin says the same. One test over every refusal arm, red first.
4. **The live harness would have misbehaved on its own failure paths** (`47af375`). It recorded
   profile one's id in the cleanup guard only after asserting `changed` (so finding 2 would have
   orphaned the profile); spent one JWT, minted at the start, on the cycle and on the guard's
   cleanup; `.expect()`ed `ToolError`s whose messages can quote Apple's `detail` text (which can name
   an id) and `expect_err`ed the final `404` reads (whose `Ok` would have printed a deleted
   profile's whole record, content and all); and documented a recipe that put the Doppler token on
   `curl`'s argv and the private key in a zsh here-string, which zsh spills to a temporary file.
   All fixed: the id is recorded first, every client is built from a fresh JWT when it runs,
   `tool_ok` prints a `ToolError`'s message only when it is one of the two fixed `401`/`403`
   messages and `<withheld: may quote provider text>` otherwise, statuses only elsewhere, and the
   recipe is stdin-and-pipe. `raw_post` became `raw_post_profile` with its path fixed.
5. **The fake state dumped profile content in plaintext** (`346987a`). `FakeState`'s serialization
   is documented as a one-way redacted view in which every seeded secret prints only its marker,
   but `AppleProfileRecord::content` was a plain `String` deriving `Serialize` and `Debug`. Now
   the marker in both. Red first, and the CLI apply test above covers it end to end.

## 3. Mutations

Each: saved copy, edit, the named test target, restore, `git diff --stat` empty.

| # | Mutation | Test that failed |
| --- | --- | --- |
| 1 | `list_bundle_id_profiles` returns after page one instead of following `links.next` | `profile_ensure_mock::read_finds_an_exact_name_match_on_page_two` |
| 2 | `find_profile_row` compares `name.starts_with(requested)` instead of `==` | `profile_ensure_mock::read_reports_absent_when_only_a_substring_neighbor_name_matches` |
| 3 | `certificate_get::find_one` returns the first of several exact matches instead of `Conflict` | `certificate_get_mock::read_reports_conflict_naming_the_count_when_two_certificates_match` |
| 4 | `provider_error_from_body` gives a `401` the `MISSING_PERMISSION` message | `willikins-providers-http` `adversarial_messages::a_401_or_403_body_never_reaches_the_provider_error_message` |
| 5 | a `let path = format!("/v1/certificates/{id}"); self.http.delete(&path)` planted in `src/client.rs` | `no_certificate_writes_guard::only_get_ever_touches_the_certificates_path_in_this_crate` |
| 6 | `AppleProfileRecord::content` serialized plainly again (the pre-finding-5 fake) | `willikins-cli` `appstore_profile_apply_redaction` ("fake-state-out carries the profile's content") |

Mutation 2's test failed on the follow-up `GET` for the neighbour's id rather than on its own
`Absent` assertion -- the neighbour matched, which is the point. Mutation 5 is the shape the first
guard let through (its unit test was red before the rewrite). The three guards were run by name
afterwards: `secret_literal_guard` 15 passed, `no_gh_writes_guard` 7 passed,
`no_certificate_writes_guard` 27 passed.

## 4. Live run

The operator's **production** developer account, Team key, 2026-09-23. Four runs, each its own
process with its own JWT:

**(a) Read-only counts, before any write** (`appstore_counts_and_leftovers_probe`, new, `GET` only):

- certificates **5** (`meta.paging.total` agrees): 4 `DEVELOPER_ID_APPLICATION_G2`, 1 `DISTRIBUTION`;
- profiles **13** (agrees): 11 `IOS_APP_STORE`/`ACTIVE`, 2 `IOS_APP_STORE`/`INVALID`;
- bundle identifiers **21** (agrees);
- throwaway leftovers: 0 identifiers, 0 profiles;
- grammars: 5/5 certificate and 13/13 profile `expirationDate`s parse as RFC 3339; 13/13 real
  `profileContent`s parse as `AppleProfileContent`. Checked **before** the write so that a content or
  date shape the create's `201` would fail to parse was found before a profile existed, not after.

**(b) The write cycle, once** (`appstore_live_write_cycle`, `live-tests`, `WILLIKINS_LIVE_TESTS=1`):

```
WRITE-CYCLE counts BEFORE: certificates=5 profiles=13 bundle_ids=21
certificate selected (id length 10)
profile one created: content length 16380, profileState ACTIVE
profile two: status 409, errors[].code ENTITY_ERROR
cleanup: 1 profile(s) then the identifier deleted by recorded id
WRITE-CYCLE counts AFTER: certificates=5 profiles=13 bundle_ids=21
independent read: the identifier and every profile id answer 404
write cycle complete: identifier and 1 profile(s) deleted
test result: ok. 1 passed; 0 failed
```

Between those lines, asserted and passed: the selection went through `appstore.certificate.get`
(the harness read the one usable `DISTRIBUTION` certificate's serial into memory and never printed
it); `appstore.profile.ensure` created the profile (`changed: true`) on a throwaway `IOS` identifier
`com.willikins.probe.delete-me.<pid>-<unix>`, named `willikins-probe-delete-me-<pid>-<unix>`; its
content base64-decoded, contained `ExpirationDate`, and contained **neither `ProvisionedDevices` nor
`ProvisionsAllDevices`** (TN3125's App Store test); length 16380 characters, under 65536; the
re-read was `Present`; the re-ensure reported `changed: false`. A second `POST` with the same name
on the same identifier was refused `409 ENTITY_ERROR`, so only one profile was ever created. The
guard never fired (no `GUARD` line): the explicit cleanup deleted the profile, then the identifier,
each by the id its own create returned.

**(c) Independent recount** (the same probe as (a), a separate process): certificates 5, profiles
13 (11 `ACTIVE`, 2 `INVALID`), identifiers 21, throwaway leftovers 0 and 0. Equal to (a).

**(d) `plan --live` of the positive document**, through the real CLI, read-only: the credential
resolved out of Doppler through the document's own `doppler.value.get`/`doppler.secret.get` chain;
`appstore.certificate.get` ran at plan time and selected the certificate (`Compute`);
`bundle_id` `Create` (a throwaway identifier nothing matches), `profile` `Create` with `profile` and
`content` `<unknown>`, `destination` `Create` (a throwaway sandbox Doppler project nothing matches),
`store` `Create`; `class: Reversible`, `requires_approval: false`; exit 0. A recount afterwards: 5,
13, 21, leftovers 0 and 0 -- the plan wrote nothing.

**Leak evidence, by count.** Across the four logs and the plan output: occurrences of the selected
serial (fed to `grep -F -f` through a file descriptor, never argv): 0; runs of 24 or more hex
characters: 0; UUID shapes: 0 in the harness logs; `eyJ` (a JWT's head): 0; `PRIVATE KEY`: 0;
base64 runs of 200 or more characters: 0.

**Two boundary-8 observations, recorded rather than fixed:**

- `plan --live` renders the Apple **issuer id and key id** in plain text: they are non-secret types
  by the credential-ports design (`doppler.value.get` into `apple.issuer_id.parse`), and the plan
  prints every non-secret node output. The same document's `certificate` node prints the selected
  **certificate id**. Neither is a credential that signs anything, but both are the operator's, and
  the plan output is where an agent reads. The verifier's own redaction filter over that output
  missed the issuer id's UUID shape and showed it in this session's transcript (never in a file, a
  commit, or this record); the file was deleted. The planned secrecy-inference milestone
  (`todos/2026-09-22-secrecy-inference.md`) is where "which of these should render" belongs.
- The CLI takes inputs only as `--input` arguments, so `serial_number` was on the `plan` process's
  argv for the length of the run -- resolved in-process from the certificate listing, never typed
  and never printed, but visible to `ps` on this host meanwhile. An input file or stdin option for
  the CLI would close that.

## 5. What was not built, and why

- **The optional sandbox Doppler round trip of the profile's content** (verify item 7). The live
  cycle was the priority on this host (one full gate took about two hours); the plan's
  ~19,000-character sizing stands on `DopplerSecretValue`'s own 65536 bound, and Doppler's own
  per-value limit remains unmeasured.
- **Fragment-assembled certificate paths** in the guard: stated as its limit, not chased.
- **A second throwaway identifier** to settle profile-name uniqueness per team: out of this
  cycle's scope by the plan's own decision (h); the tool's `Conflict` arm covers either answer.

## 6. Host conditions worth knowing

One full gate took about two hours on this host (11 GB of RAM, swap near its 6 GB ceiling
throughout). The final gate's first attempt died on `ENOSPC`, not on a failure: the data volume
filled while an unrelated agent's `cargo test --target-dir target/pi-check` ran alongside it, and
for a while no shell command could even open its own output file. The verifier freed only its own
logs, the other run ended, space came back, and the gate was re-run. A `cargo-sweep` over every
project on the host, running at the same time, then deleted some of this workspace's compiled
dependencies mid-build (`E0463`, "extern location ... does not exist"); once it finished, the four
crates it had broken were cleaned by name and the gate ran through: `fmt` and workspace `clippy`
clean, workspace tests 171 targets passed and none failed, `check -p willikins-types` clean. Nothing
the live cycle made was in flight at any point: the account had been recounted clean beforehand.
