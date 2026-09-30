# Milestone 3e: adversarial pass 6 — T3f (Apple's real bundle-id name rule)

**Date:** 2026-09-30
**Task:** the independent attacker's pass over task T3f, which landed on `main` for
`docs/plans/2026-09-27-milestone-3e-new-ios-app.md` after pass 5
(`docs/research/2026-09-30-m3e-adversarial-pass-5.md`). The attacker wrote none of it.
**Subject:** `f71e405` (the live probe), `cfbc0df` (Walter's literal names), `7a87b4a` (fmt), `0f15efd`
(the `AppleBundleIdentifier => AppleBundleIdName` row removed, new negative fixture), `4de0b09`
(`AppleBundleIdName` tightened), `9d3b2f2` (the T3f addendum), `e6bd4ac` (two stale doc comments).
**Method:** read the T3f addendum, the seven commits and the code they touch; re-fetch every Apple
source T3f cites; read the monorepo (read-only) for the profile-name evidence; then attack with
mutations. Every mutation followed the same steps: copy the file into the pass's scratchpad, apply one
exact-text replacement (a script asserted it occurred exactly once), run the narrowest test target
with `--no-fail-fast` (`INSTA_UPDATE=no` where a snapshot could move), copy the saved file back,
`touch` it, and confirm byte identity with `cmp` (exit 0 every time) and a clean `git status`. Never
`git checkout`, `reset` or `stash`. **No live test ran and no provider API was called.** The only
network traffic was to Apple's public documentation and its published OpenAPI specification.

## 0. The state the pass started from

`main` at `e6bd4ac`, clean apart from `goal.txt` and the host-maintenance todo (both left alone).
Nothing uncommitted, nothing red: every mutation run below passed every test the mutation did not
touch, and the restored tree passed `walter_document` (4/4) and all of `willikins-types`.

## 1. The questions, and what the evidence says

### Does the tightened grammar refuse everything the probes saw Apple refuse, and accept everything Apple accepted? Yes

`is_bundle_id_name_char` admits `[A-Za-z0-9 -]`; `AppleBundleIdName::parse` requires 1 to 255 of them.

| probe (2026-09-30, live, T3f) | Apple | `AppleBundleIdName::parse` |
|---|---|---|
| `Probe.Dot.Name` | 409 | refused (`.`) |
| `Probe - Hyphen` | 201 | accepted |
| `Probe's Apostrophe` | 409 | refused (`'`) |
| `Probe & Ampersand` | 409 | refused (`&`) |
| `1Probe Digit` | 201 | accepted |
| name equal to its own dotted identifier | 409 | refused (`.`) |

Also witnessed accepted, and also accepted by the grammar: `willikins-live-write-cycle-probe` (a hyphen
without spaces), created `201` by every earlier live write cycle through `probe_bundle_name()`; and the
account's own names `Barnum`, `Danksworth - Keyboard`, `Phil Connors - NSE`, `XC com bande-a-bonnot
philconnors`. The JSON schema's `pattern` (`^(?:[A-Za-z0-9 -]+)$`) says the same as `parse`.

**M1** re-admitted `.` in `is_bundle_id_name_char`: `bundle_id_name_refuses_a_dot_probed_live_2026_09_30`
and `a_dotted_bundle_identifier_is_no_longer_a_valid_bundle_id_name` failed (427 passed, 2 failed,
`willikins-types --lib`). **M1b** dropped `' '`: `..._accepts_a_hyphen_surrounded_by_spaces_...` and
`..._accepts_a_digit_leading_name_...` failed (427/2). Both restored, `cmp` 0.

**Outside every probe, in both directions (recorded, not changed):** an all-space name, a hyphen-only
name, a leading or trailing space, doubled spaces, and any length above about 32 characters. The
grammar admits all of them, and no probe showed what Apple does with them. Admitting one Apple refuses
is the live-breaking direction T3f's own doc names, so these are verify items for the next throwaway
probe run, not facts. A trailing space has a second risk: if Apple trims it, `read`'s byte-exact
compare would see a drifted name on every run.

**Checked non-issue:** `read` never parses Apple's returned `name`. `bundle_id_ensure.rs` compares the
raw string (`resource.attributes.name != name.as_str()`), so the tightening cannot break a read of an
existing identifier whose name was made elsewhere with a character outside the grammar. Such an
identifier reads `Mismatch`, and a document can only converge it to a grammar-valid name.

### Is each claim sourced verbatim or probed? Yes, with one citation that does not say what it is cited for (fixed)

- **Re-fetched 2026-09-30:** the ASC OpenAPI specification
  (`https://developer.apple.com/sample-code/app-store-connect/app-store-connect-openapi-specification.zip`,
  `info.version` `4.5`) declares `BundleIdCreateRequest.data.attributes.name` as `{"type": "string"}`,
  `BundleIdUpdateRequest`'s as `{"type": "string", "nullable": true}`, and
  `ProfileCreateRequest`'s as `{"type": "string"}`. None has a `pattern` or a `maxLength`. The
  help pages say, verbatim, "Enter a name or description for the App ID in the Description field."
  and "Enter a profile name, then click Generate.". Neither states a character rule.
- **F1 (fixed):** `AppleBundleIdName`'s doc and the T3f addendum cite the `.md` twin
  `.../bundleidcreaterequest/data-data.dictionary/attributes-data.dictionary.md` as where the
  specification "declares `name` as a bare string". Fetched, that page renders only the object's title
  and one sentence ("Attributes that you set that describe the new resource."). It gives no type for
  `name`. The claim is true, but the source is the specification zip. The type's doc now cites the zip
  (`9fa6512`). The addendum is left as written, and this pass's addendum corrects it.
- The six probe results are recorded as status/code/title only, from one guarded run. That is the
  probe's own report shape (`raw_post_bundle_id` never prints `errors[].detail`). I did not re-run it.

### Is every remaining conversion row total against the provider's real rule? `=> Text` has no provider; `=> AppleProfileName`'s dot is witnessed only on portal-made profiles

Two rows remain in `conversion_rows()`.

- **`AppleBundleIdentifier => Text`:** `Text::parse` refuses only length over 65536. Every `Text`
  input port belongs to a willikins pure tool (`template.render.value`, `operator.acknowledge.step`,
  `apple.key_id.parse.value`, `apple.issuer_id.parse.value`). `doppler.value.get` only *outputs*
  `Text`. No conversion leads out of `Text`, so no provider ever receives one. There is no external
  rule for the row to be total against.
- **`AppleBundleIdentifier => AppleProfileName` (F2, recorded, doc corrected):** the row is total
  against willikins' own `AppleProfileName` grammar (non-empty, at most 255, no control or
  invisible/bidi character). **M2** made `AppleProfileName::parse` refuse `.`:
  `every_bundle_identifier_is_a_valid_profile_name` and
  `over_the_identifier_alphabet_every_identifier_converts_byte_for_byte` failed (427/2), and
  `every_string_a_bundle_identifier_accepts_a_profile_name_also_accepts` **survived**: `.*` strings
  are almost never bundle identifiers, so that test barely exercises the implication it states.
  Against **Apple's** rule the evidence is thinner than the T3f commits say ("Apple accepts dots in a
  profile's `name`", `lib.rs`; "that row's own probe evidence", `0f15efd`). What is witnessed: the
  monorepo's Danksworth and Pocket Claw `BUILD.bazel` reference distribution profiles named exactly
  after their dotted identifiers (`com.bande-a-bonnot.danksworth`, `...danksworth.keyboard`,
  `...stickers`, `...share`, `com.bande-a-bonnot.pocket-companion`), and the account holds 13
  `IOS_APP_STORE` profiles (11 active, per the M3e recount). None of these was created through the API by anything we can see: the
  monorepo's fastlane calls `get_provisioning_profile(... readonly: true)`, so it only downloads them.
  willikins' own live cycles `POST /v1/profiles` with `willikins-probe-delete-me-<unique>`, which has
  letters, digits and hyphens and no dot. The real Walter apply stopped at the first bundle id, so no
  profile node ever ran live. **A dotted `name` on `POST /v1/profiles` has never been observed.** This
  is the same inference ("the grammar admits it, so Apple does") that just failed for bundle-id names.
  Portal-made dotted profiles make it likely that the API accepts dots, but that is not proof. The
  row's doc and `lib.rs` now say exactly this. Recommended before the next real Walter apply (the
  coordinator's, since this pass makes no live call): one throwaway `POST /v1/profiles` whose `name`
  equals its own dotted `com.willikins.probe.delete-me.*` identifier, with the existing DISTRIBUTION
  certificate relationship, deleted by returned id. `raw_post_profile` in `live_write_cycle.rs`
  already has that shape. The 255 bound on both sides is also this crate's own guess, but real
  identifiers are far shorter, so it does not bear on Walter.

### Does any other document or fixture still bind a dotted value to a bundle id name? No

Every `appstore.bundle_id.ensure` `name` binding in `workflows/`: Walter's three literals;
`${{ inputs.name }}` / `${{ inputs.bundle_name }}` (typed `AppleBundleIdName`, no default, so a dotted
value is refused at input parsing) in `appstore-bundle-id-from-inputs`, `-from-doppler`,
`appstore-signing-profile-from-doppler`, the three profile fixtures and
`appstore-bundle-id-text-into-identifier`; and
`${{ inputs.identifier }}` in the new negative fixture, which is meant to fail `check`. Every fake-state
bundle-id name (`workflows/fixtures/state/*.json`) is `willikins-demo`. The test harnesses name bundle
ids `willikins-live-write-cycle-probe`, and nothing binds a dotted value.

**M3** re-added the row and a `From` impl (`Self(identifier.as_str().to_owned())`), the exact code
`0f15efd` removed: `appstore_bundle_id_identifier_into_name_is_rejected` failed (bundle_id_documents
4 passed, 1 failed). `willikins-types --test catalog` **passed**: that catalog snapshot does not list
conversion rows, and nothing in `willikins-types` checks a row's `From` against its target's `parse`.
So the new fixture is the only thing standing between a re-added row and a green `willikins-types`.
It does its job. Restored, `cmp` 0 on both files.

### Does Walter still check and plan over the fake catalog, with the new names and unchanged profile names? Yes

`walter_document.rs` does more than check. `gates_unmet_then_satisfied_then_acknowledged` asserts
`app_id`/`nse_id`/`widgets_id` plan `Create` and apply `Created` on run 1 and plan `NoOp` / apply
`Unchanged` on run 2, and that the three `*_profile` nodes are `Skipped` on run 1 and `Created` on run 2.
Their names still bind from each gate's `identifier` through the profile row. The restored tree passed
4/4.

- **M4a** `"Walter - NSE"` → `"Walter.NSE"`: three tests failed with `InvalidLiteral { node: nse_id,
  port: name, ... (found '.') }`. `check` now refuses the exact value class that answered 409 live.
- **M4b** `"Walter - NSE"` → `${{ inputs.nse_identifier }}` (the pre-T3f binding): three tests failed
  with `TypeMismatch { node: nse_id, port: name, expected: Exact(AppleBundleIdName), found:
  AppleBundleIdentifier }`.
- **M5** `"Walter"` → `"Barnum"` **survived** (4/4). Nothing pins the three literals. That is
  deliberate: they are the coordinator's unconfirmed proposal, and pinning a proposal would be wrong.
  Once the operator confirms them, they belong in `the_document_reads_the_real_layout_by_name` beside
  the other operator-layout literals.

All restored, `cmp` 0.

## 2. Findings

- **F1 (fixed, doc):** `AppleBundleIdName`'s citation for the specification's `name` type pointed at
  a page that does not state it. It now cites the specification zip, version 4.5.
- **F2 (recorded, doc corrected):** the profile-name row's totality against Apple rests on
  portal-made dotted profiles, not on an API write. A dotted `name` on `POST /v1/profiles` is
  unprobed. The row's doc now says so and names the throwaway probe that would settle it. No code
  change: the row is total against willikins' grammar and nothing contradicts it.
- **F3 (recorded):** `every_string_a_bundle_identifier_accepts_a_profile_name_also_accepts` (`.*`)
  survived M2. The grammar-generated tests carry the proof. The row's doc now says so.
- **F4 (recorded):** nothing in `willikins-types` ties a registered row's `From` to its target's
  `parse`. M3's re-added row was caught only by the document-level fixture. A cheap guard would parse
  every row's source `example()`, apply the row, and re-parse the rendered result as the target. It
  would not have caught T3b's original error, because the target grammar was wrong then, so it is a
  suggestion, not a fix.
- **F5 (recorded):** `AppleBundleIdName` admits unprobed shapes: all-space, hyphen-only,
  leading/trailing or doubled spaces, and lengths over about 32. These are verify items for the next
  probe run.

## 3. Gates (scoped; the full workspace gate is the coordinator's)

After the doc commit `9fa6512`: `cargo fmt --all --check`; `cargo clippy -p willikins-types --all-targets -j 2 --
-D warnings`; `cargo test -p willikins-types` (every suite); `cargo check -p willikins-types`;
`cargo test -p willikins-cli --test walter_document` (4 passed). Neither snapshot moved: the pass
changed doc comments only, and no document.
