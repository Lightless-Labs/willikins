# Milestone 3a: adversarial pass over the Buildkite provider and the real workflow

**Date:** 2026-09-20
**Task:** 11 of `docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`
**Subject:** `crates/willikins-providers-buildkite`, `willikins-providers-fake`'s two Buildkite
tools, `workflows/new-rust-service-buildkite.yaml` and its fixtures.
**Method:** read the plan and the research note in full, then attack the code that landed. Every
claim below is a run, not a reading: four mock tests were proved non-vacuous by mutation, and a
fifth mutation found a real hole. Each mutation was made by editing the source, running the named
test target, and restoring from a copy saved before the edit (never `git checkout`, never
`git reset`); `git diff` was confirmed empty after every restore.

## What was attacked, and what held

### The types refuse what their grammars forbid

`BuildkiteOrg`, `BuildkitePipelineSlug` and `BuildkiteClusterId` are `#[derive(DomainType)]`
string types. `willikins-derive`'s `anchored_pattern` wraps every pattern in `^(?:...)$` unless the
author anchored it, and Rust's `regex` `$` matches only the end of the haystack — it has no Perl
"before a final newline" behaviour, which `client.rs`'s own
`rejects_a_trailing_newline` pins for the credential pattern. So none of the three can carry a
`/`, `?`, `&`, a newline, or anything else into a request line. `BuildkiteClusterName` is
hand-written and refuses control characters, the invisible/bidi set `Description` refuses, an
empty string, and anything over 255 characters.

**No type here is a `String` in disguise.** The one that comes closest, `BuildkiteClusterName`, is
a human-written lookup key with real refusals, and the plan argues its case explicitly.

**A parse refusal never names the offending value**, for any of the three derived types:
`willikins-derive`'s `checks` emits `"does not match the required pattern"`,
`"must be at most {n} characters long"`, and `"must be at least {n} characters long"` — the
constraint, never the input. `BuildkiteClusterName`'s hand-written refusals name a *character*
(`found {c:?}`, escaped through `Debug`) and a length, never the whole string. That is stricter
than the milestone 3a plan's own wording ("names the offending value") and it is the right way
round: a message that echoed the value would be a second place a mis-bound credential could
surface. The value an agent needs in order to fix its document is quoted one layer up, by
`willikins_types::quoted`, which is bounded and escaped.

### `read` distinguishes the four observations, and "ours" is decidable

`observe` branches in the plan's order: description not exactly `managed-by: willikins` →
`Foreign`; repository different → `Mismatch { repo }`; `cluster_id` different → `Mismatch
{ cluster }`; otherwise `Present`; `404` → `Absent`. **A pipeline someone else made at the derived
slug reads `Foreign`**, because ownership is exact equality against one frozen marker and any
other description — including `null`, including `"managed-by: Willikins"` with a capital — fails
it. `ensure` then returns `Conflict` naming the address and never touches the pipeline. Proved by
mutation 1 below and by `read_reports_foreign_when_description_is_null`.

`cluster_id` is `Option<String>` on the response struct and compared as
`body.cluster_id.as_deref() != Some(cluster.to_string().as_str())`, so a `null` `cluster_id` — the
shape Buildkite's own stale create example returns — is `Mismatch { cluster }`, not a panic and not
a false `Present`.

### The configuration decision is enforced by a type, not a comment

There is no configuration port, no command port, and no YAML port, on either tool. The only
command-shaped string in the crate is `UPLOAD_CONFIGURATION`, a `const` built from no input.
**An attempt to get a caller-supplied command through was made and there is no path for one:**
`CreatePipelineBody` is a private struct with six `String` fields, all six assigned from constants
or already-parsed domain types inside `create_pipeline`; the crate has no `PATCH` and
`willikins-providers-http` has no `patch` method to call; and `buildkite.pipeline.ensure`'s
`ToolSpec` declares four input ports whose types are all in the registry, so `check` refuses a
document that binds anything else (`unknown-port.yaml`'s existing acceptance test is the general
proof). What stops a caller-supplied command is therefore the *absence of a port*, backed by the
spec-versus-registry validation, not a comment.

`repository` is the same shape: built by `ssh_repository_url` from an already-parsed `GitHubRepo`,
never accepted as a URL.

### The guards

Both gate-enforced guards were run by name and are green with everything this pass added:
`willikins-core`'s `secret_literal_guard` (which walks the whole tree, the two new documents and
the new crate included) and `willikins-cli`'s `no_gh_writes_guard`. A separate hand sweep for
`bk[a-z][a-z]_` across `.rs`, `.yaml`, `.json`, `.md` and `.toml` found only: the README's
documentation row, `CREDENTIAL_PATTERN` (a pattern, not a literal), doc comments naming `bkua_`
and `bkct_` as prefixes, short non-matching test values (`bkua_testtoken`), the `concat!`-joined
marker in `tests/redaction.rs`, and the research note's verbatim quotations from Buildkite's own
published prefix table. No file spells a token-shaped literal.

## The mutation tests

Baseline: `cargo test -p willikins-providers-buildkite` green (13 + 4 + 6 + 15 + 3 tests, two
ignored live targets). Each mutation below was applied alone.

| # | Mutation | Named test that must fail | Result |
| --- | --- | --- | --- |
| 1 | `MANAGED_DESCRIPTION` → `"managed-by: Willikins"` (one byte) | `read_reports_present_when_marker_repository_and_cluster_all_match` | **failed** (with 7 others) |
| 2 | A seventh field (`visibility`) added to `CreatePipelineBody` | `ensure_creates_the_pipeline_with_exactly_the_six_documented_fields` | **failed**, alone |
| 3 | `ensure`'s create-failure arm returns the error instead of re-reading | `an_error_after_create_re_reads_and_reports_unchanged_when_present` | **failed** (with `..._reads_foreign_conflicts`) |
| 4 | `cluster_get`'s short-page test `len < PER_PAGE` → `len <= PER_PAGE` | `a_match_on_the_second_page_...` | **failed** (with `eleven_full_pages_...`) |

Mutation 2 also settles a question the plan's acceptance test 3 rests on without saying so:
`willikins_providers_http::testing::json_body` is `mockito::Matcher::Json`, **exact** JSON
equality, not `PartialJson`. The "the body carries no `steps`, `env`, `provider_settings`,
`teams`, `tags` or `visibility` key" half of that test is therefore real, and mutation 2 is the
proof.

Mutation 3 also settles that mockito's two-mocks-on-one-path sequencing is real rather than
order-independent: without the re-read, both ambiguous-create tests fail.

## The hole: the fake and the live tool agreed only by construction

**Mutation 5.** `willikins-providers-fake`'s private copy of `ssh_repository_url` was rewritten to
emit `https://github.com/{owner}/{name}.git` instead of `git@github.com:{owner}/{name}.git`.

`cargo test -p willikins-providers-fake -p willikins-providers-buildkite` stayed **completely
green**. A grep for `git@github.com` across the tree confirmed why: the live crate's form is pinned
by `client.rs`'s own unit test and by the mock fixtures, the fake's copy is pinned by nothing, and
nothing compares the two.

This is exactly the failure mode the milestone 3a plan names ("Does the fake genuinely agree with
the live tool, or do they agree by construction?") and the one that cost milestone 2 its first live
smoke run. `tests/catalog_parity.rs` pins the two `ToolSpec`s equal, but a `ToolSpec` says nothing
about what a tool does with the ports it declares, and both specs are hand-written to match. The
consequence in the field: a document planned against the fake predicts a pipeline that the live
tool, applied to the same world, reports as `Mismatch { repo }` — a plan that says `Create` and an
apply that refuses.

**Fix:** `crates/willikins-providers-buildkite/tests/fake_agrees_with_live.rs`. For each of the
five observations `buildkite.pipeline.ensure` can report, the fake's state is seeded and the live
tool's provider is mocked to describe *the same world*, and the two tools' `Observation` shape and
rendered outputs are asserted equal. The fake side is seeded through
`FakeState::with_buildkite_pipeline` using the **live crate's own** `ssh_repository_url` and
`MANAGED_DESCRIPTION`, so a divergence in either copy of a frozen form fails here.
`buildkite.cluster.get`'s single-match, not-found and ambiguous arms are compared the same way,
including the exact `ToolError` message.

Re-running mutation 5 against the new file fails
`present_agrees_and_pins_the_frozen_repository_form` and
`mismatch_on_the_cluster_agrees_and_repo_is_checked_first`. The hole is closed and the closure is
itself proved.

## The smaller finding: an assertion the plan asks for was missing

Plan acceptance test 5 requires that the `Link` header's documented `api_key` query parameter
reaches no output, error, journal or `tracing` output. `cluster_get_mock.rs` *served* such a
header in `a_match_on_the_second_page_...`, with a marker in it, but never asserted the marker's
absence anywhere — the test only checks that paging used explicit `page`/`per_page` parameters, so
it would pass unchanged if the header's bytes leaked.

**Fix:** `the_ignored_link_headers_api_key_reaches_no_output_debug_or_error`, over all four arms
the tool can reach (one match, none, two, a `500`).

**Stated plainly, and in the test's own doc comment:** this assertion *cannot fail today*.
`willikins-providers-http`'s `response_facts` reads three headers by name (`Retry-After`,
`x-ratelimit-remaining`, `x-ratelimit-reset`) and `Link` is not one of them, so no header byte can
reach a `ProviderError` at all. It is a regression guard, not a discovery — and a pointed one,
because the plan's own risk list contemplates growing `response_facts` to read Buildkite's
`RateLimit-Reset` and `RateLimit-User-Reset`. This test is what makes that change fail loudly if it
ever generalises to "record the headers" instead of "read two more named ones".

## The live write cycle: three changes before running it

1. **It proved neither `Foreign` nor `Mismatch`.** The plan's acceptance test 16 does not require
   them, but the task's own attack list does, and both `Mismatch` arms are provable **read-only**
   against a pipeline that really exists: bind a different `repo`, or a different `cluster`, and
   `read` compares locally. Added as step 5, together with the `ensure`-refuses-with-`Conflict`
   half and a final assertion that the pipeline is untouched afterwards.
   **`Foreign` is not provable live and the cycle says so:** making a real pipeline foreign means
   editing its `description`, which needs a `PATCH` this crate deliberately does not have and will
   not grow. The mock suite proves that arm.
2. **One recording was vacuous.** `record_and_compare("clusters_list_page", ...)` was called with a
   JSON array rebuilt from the one id the tool returned, and its result was discarded with `.ok()`.
   `common::top_level_keys` returns the empty set for an array, so the comparison was empty-versus-
   empty and could not fail. Removed, with the reason written where it was.
3. **There was no independent after-the-run check.** `willikins-providers-doppler`'s own cycle has
   `the_cycles_projects_are_gone`, gated behind a second variable so the two never run together.
   Added `the_cycles_pipeline_is_gone`, which goes through `Http` directly rather than through the
   tool — so a pipeline that exists but that the tool would call `Foreign` still counts as a
   leftover — and prints the organisation's pipeline and cluster names afterwards, so the run's
   blast radius is reported rather than assumed.

## Noted, not fixed

- **`ensure`'s re-read can mask the original error.** On a create failure the tool re-reads; if
  that re-read itself fails with a non-`404`, `?` propagates the *re-read's* `ProviderError`, not
  the create's. The plan's decision (d) enumerates `Present`/`Foreign`/`Mismatch`/`Absent` and does
  not cover a failing re-read. Both errors are `Provider`, both are bounded, neither leaks: the
  difference is which message an operator reads. Left alone rather than changed under an
  adversarial pass, because the plan is the specification and this is a gap in it, not a departure
  from it. Worth a sentence in 3b's plan.
- **The fake's `buildkite.cluster.get` ignores its `org` port** (its own comment says so, and every
  other fake tool's single-workplace state does the same). It means the fake resolves a cluster for
  any organisation. Consistent with the rest of the crate; not a divergence this pass would fix
  alone.

## Gates

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -j 2 -- -D warnings`,
`RUST_TEST_THREADS=2 cargo test --workspace -j 2 --no-fail-fast`, and
`cargo check -p willikins-types -j 2`, plus
`cargo clippy -p willikins-providers-buildkite --all-targets --features live-tests -j 2 --
-D warnings`, which is the only way the live write cycle is compiled at all.
