# Milestone 3g: file-writing — Sample's files become something the document does

**Created:** 2026-09-30
**Addendum:** 2026-09-30 (coordinator) — the operator settled the open choice: "Commit straight." Sample's document commits directly to `main` of Example-Org/monorepo, matching the monorepo's own ruling of 2026-07-09. Tasks P1–P3 (branch and pull-request tools) are dropped from this milestone; the tool still writes to whatever branch a document names, so another document may choose differently.
**Addendum:** 2026-09-30 (E1) — decision (a)'s "`Site::Port` plus `index`, or a new `Site::ListElement`" choice
is `Site::ListElement`, as expected, but boxed: `Site::ListElement(Box<ListElementSite>)`, built with a new
`Site::list_element(node, port, index)` constructor, rather than an inline `{ node, port, index }` struct
variant. An inline third field made `Site` — embedded in `CheckError`, `PlanError`, and, through
`ApplyError::Plan`, every `apply()` call's `Result` — cross clippy's `result_large_err` default (128 bytes),
which `cargo clippy -p willikins-cli -p willikins-server --all-targets` surfaced at a call site in
`willikins-server/src/butler.rs` neither this task nor its own scoped gates touch; grepping `ApplyError`
across the tree found about two dozen files in five other crates that could have needed the same allow.
`willikins-core/src/apply.rs`'s own `ApplyError::Drift` already boxes its `DriftKind` for exactly this
reason (its doc comment names the lint), so boxing here matches the codebase's own established answer rather
than adding `#[allow(clippy::result_large_err)]` at every affected call site. Contained entirely within
`willikins-core` (`site.rs`, `check.rs`, `plan.rs`); no other crate's files were touched. Verified with
`cargo clippy -p willikins-cli -p willikins-server --all-targets -j 2 -- -D warnings`, green.
**Addendum:** 2026-09-30 (E2) — the five new types (`RepoPath`, `GitBranchName`, `CommitHeadline`,
`TemplateValue`, `RepoFile`) and the `AppleBundleIdentifier => TemplateValue` conversion landed in
`willikins-types`; the four check refusals landed in `willikins-core` as one `DisallowedInputType { input, ty }`
variant (covers "as a workflow input type" and "as an input default" for both `TemplateSource` and `RepoFile`
in one branch, so a defaulted disallowed input is still exactly one error) plus `RepoFileLiteral { node, port }`
(checked in both `check_literal` and `check_list_literal`, since `github.scaffold.ensure`'s own port is the
list form, `files: list<RepoFile>`). Two deviations from a literal reading of "their negative fixtures,
secret-into-repo-file.yaml" under E2's task row:
1. **No YAML fixture files were added** under `workflows/fixtures/`, including the four that need no tool
   (`TemplateSource`/`RepoFile` as an input type or a default) and would have characterized cleanly today.
   `T1` doesn't land `repo.file.render` and `G2` doesn't land `github.scaffold.ensure` until later tasks, so
   `secret-into-repo-file.yaml` and a `RepoFile`-literal fixture would characterize as `UnknownTool`, not the
   intended error, and whichever of `T1`/`G2` lands the real tool would then have to change an *existing*
   characterization entry — forbidden by this task's own boundary ("only by the addition of new documents").
   Keeping all six fixtures together, deferred to the tasks that land their tools (`T1` for
   `secret-into-repo-file.yaml`, `G2` for a `RepoFile`-literal-on-a-list-element fixture; the other four have
   no such dependency and can be added by either), was judged simpler and less error-prone than splitting them
   by dependency now. All four refusals, and the secret-into-`values` taint, are instead proved directly in
   `crates/willikins-core/tests/check.rs` against synthetic `test_catalog()` entries shaped exactly like `T1`'s
   `repo.file.render` and `G2`'s `github.scaffold.ensure` (SHARED VALUES table), the same pattern `E1` used for
   `fake.list_sink.ensure` before any real tool had a `list<T>` port.
2. **The ripple beyond `willikins-core`'s own scoped gate was fixed, matching E1's own addendum precedent**:
   `willikins-cli/src/render.rs`'s exhaustive `check_error_detail` needed two new arms (plus
   `#[allow(clippy::too_many_lines)]`, already needed independently by `Display::fmt`'s own match in
   `check.rs`, both over clippy's 100-line default), and
   `willikins-server/tests/snapshots/mcp_server__the_tool_list_and_every_schema_is_snapshotted.snap` needed
   regenerating (diff confirmed as exactly the two new variants' schemas, nothing removed). Verified with
   `cargo clippy -p willikins-cli -p willikins-server --all-targets -j 2 -- -D warnings`,
   `cargo test -p willikins-cli --bin willikins`, `cargo test -p willikins-server --test mcp_server`, and
   `cargo test -p willikins-dsl --test acceptance` (characterization snapshot confirmed byte-identical, since
   no document yet declares either new type), all green.
**Addendum:** 2026-09-30 (adversarial pass, engine and types) — an independent attack on E1 and E2,
recorded in `docs/research/2026-09-30-m3g-adversarial-pass-engine-and-types.md`. Eight mutations: five
were killed by existing tests (the skip scan, apply re-resolution and `Unknown` propagation of list
elements, the list-element `RepoFile` literal refusal, list-element taint). Three survived, and each is
now killed by a new test: `DisallowedInputType` narrowed to scalars (a caller-supplied `list<RepoFile>`
then checked clean on `github.scaffold.ensure.files`), `TemplateValue`'s class widened by `#`, and a
compiling, total `Text => TemplateSource` conversion row (now: no row may target `TemplateSource` or
`RepoFile`). No defect in the shipped code; every existing document's characterization is
byte-identical. For later tasks: `RepoFileLiteral` carries no element index; `mark_blocked`'s
`awaiting_inputs` ignores inputs inside a list (unreachable today); the `TemplateValue` grammar protects
quoting, not placement, so T1/W1 must keep every placeholder out of command, path and unquoted-YAML
positions; W1 should pin that `sample_files.files` is known at plan, since approval is the diff review.
**Addendum:** 2026-09-30 (T1) — `repo.file.render` landed in `willikins-tools`
(`crates/willikins-tools/src/repo_file_render.rs`): `path`/`template` required, `values` an optional
`list<TemplateValue>` read by hand (no `list<T>`-reading helper existed yet; G2's `files: list<RepoFile>`
may want its own, but adding one to `willikins-core::tool::helpers` speculatively was judged wider than
this task). Placeholders are found by a manual scanner, never a regex dependency, matching the crate's
existing style (`RepoPath`/`GitBranchName` are hand-written for the same reason): every `{{` must open
exactly `{{ N }}` (one space each side, `N` a decimal index, no required leading-zero canonicalisation)
or the whole call refuses, before checking `N` against the `0..=15` range; index-vs-value matching and
the amplification bound (mirroring `template.render`'s own arithmetic-before-allocation fix) come after.
The task row said "One commit, willikins-tools + catalog pins"; acceptance 5's own "both catalogs
validate; LIVE_TOOL_NAMES pinned" made that pins plural in practice, so the same commit also touches:
`willikins-server/src/catalog.rs` (`LIVE_TOOL_NAMES` 34 → 35, `insert_pure_tools`, both inline
pure-tool-name lists in its tests, doc-comment counts), `willikins-providers-fake/src/lib.rs` (its own
`catalog()` and `catalog_registers_every_fake_tool`, 36 → 37) and `tests/pure_tools_agree.rs` (a new
case, required by that file's own completeness assertion over every pure tool in the fake catalog), and
`willikins-providers-doppler/tests/live_catalog.rs` (its local `LIVE_TOOL_NAMES` copy's array-length
annotation, which does not compile otherwise, plus its doc-comment counts). No behaviour changed in any
of the three catalog-assembly crates beyond registering the one new tool; every ripple is mechanical and
was required for the touched crates' own existing tests to stay green, not a design choice. Verified with
`cargo fmt --all --check`, `cargo clippy --all-targets -D warnings` and `cargo test`, each scoped to every
touched crate (`willikins-tools`, `willikins-server`, `willikins-providers-fake`,
`willikins-providers-doppler`), plus `cargo check -p willikins-types` and
`cargo test -p willikins-dsl --test acceptance` for the characterization snapshot (byte-identical: no
shipped document binds `repo.file.render` yet) — all green.
**Addendum:** 2026-09-30 (G1) — the GitHub client's reads and `createCommitOnBranch` write landed in
`crates/willikins-providers-github/src/client.rs`, exactly the two commits the task row named: commit 1
(`get_branch_head`, `get_commit_root_tree`, `resolve_tree_paths` — the non-recursive walk memoised per
directory sha, returning `Absent`/`Blob { mode, sha }`/`NonBlob` per declared `RepoPath` — `get_blob`, and
`git_blob_sha` pinned against three `git hash-object` vectors) and commit 2 (`create_commit_on_branch`,
one fixed mutation text over `POST /graphql`, additions sorted by path before the request is built,
every failure body-free including a `502` — stricter than `willikins-providers-http`'s shared REST
handling, via a `suppress_graphql_response_body` step that leaves `401`/`403` and a transport failure
untouched since both are already body-free). One "Fix commit" followed advisor review: the read test's
tree-walk fixture had a symlink and a submodule as declared paths but no tree named as a path's own
*last* segment, so a mutation collapsing that case into `Blob` would have survived; fixed by adding
`apps/sample/ios/Resources` itself as a declared path, and the tree mocks now pin `Matcher::Missing` on
the query string so a stray `?recursive=1` could not silently satisfy them (the exact mock-precision
lesson milestone 3e already paid for once).

Three deviations/notes, none changing decision (b)'s own three-state read table:

1. **A new dependency, not named in this plan or its pre-flight**: `sha1 = "0.10"`, added directly to
   `willikins-providers-github/Cargo.toml` (not `[workspace.dependencies]`), matching that crate's own
   `rand_core` precedent for a dependency only it needs. `0.10.6` was already vendored in the local
   registry cache and needed no network fetch; `0.11` (available) was not taken because nothing else in
   the workspace pulls RustCrypto's `digest 0.11` line yet (`sha2` here is still `0.10`-family through its
   own transitive deps) and there was no reason to be first.
2. **`github.scaffold.ensure` (task G2) does not exist yet**, so every new item in this commit is
   unreachable from the crate's own plain `lib` build and would fail `-D warnings`' `dead_code` lint on
   its own. Each of the 31 new items carries `#[allow(dead_code)]` pointing back to `get_branch_head`'s own
   explanation, the same shape `willikins-cli::render`'s `applied_node_line` already uses while awaiting
   its own caller ("awaits task 11's caller"). This is temporary: when G2 lands and calls through these
   methods, most or all of these `#[allow]`s stop being needed and should be removed in that task's own
   commit, not carried forward as a permanent style.
3. **Two things for G2 and L1 to settle, flagged rather than pre-decided here**, since G1's own task
   boundary is "the client reads and the write, exactly as decision (b) describes" and both of these are
   the tool's or the live harness's own concern:
   - `resolve_one_path` folds "an intermediate path segment exists but is not a directory" (a file or a
     symlink sitting where a declared path needs a subdirectory) into the same `PathEntry::Absent` a
     genuinely-missing path gets. Decision (b)'s read table has no fourth state for this, and folding it
     into `Absent` is the literal reading of "each path yields: absent; a blob entry …; or a non-blob
     entry" — but `Absent` is exactly the state `github.scaffold.ensure`'s own read table (decision (b))
     turns into a `createCommitOnBranch` attempt, and what GitHub's Git database does when a commit's
     `additions` names a path underneath an existing *file* is one of this milestone's own open verify
     items in spirit, not something G1 tested. G2 should decide, with a live-probed fact if needed,
     whether this case needs its own `PathEntry` variant before it ships.
   - The five read methods, `create_commit_on_branch`, `PathEntry`, and `git_blob_sha` are `pub(crate)`,
     reachable only from within this crate. Task L1's own harness
     (`crates/willikins-providers-github/tests/live_scaffold_cycle.rs`) is a separate integration-test
     crate and cannot call a `pub(crate)` item at all; it will need either these made `pub` (and
     re-exported from `lib.rs`, which would also delete every `#[allow(dead_code)]` from note 2 above) or
     to drive them through `github.scaffold.ensure`'s own `Tool` instead. Left to whichever of G2/L1 lands
     first to decide, since G1's own acceptance (6, 7) needed no more than crate-internal visibility.

Verified with `cargo fmt --all --check`, `cargo clippy -p willikins-providers-github --all-targets -D
warnings`, and `RUST_TEST_THREADS=2 cargo test -p willikins-providers-github`, all green; `cargo check -p
willikins-types` green. `cargo test -p willikins-dsl --test acceptance` was not run: `willikins-dsl`
carries no dependency on `willikins-providers-github` at all (checked directly in its `Cargo.toml`), so
this task cannot have moved that snapshot.

**Addendum:** 2026-09-30 (G2) — `github.scaffold.ensure` landed in both crates, exactly the task row's two
commits: commit 1, the live tool (`crates/willikins-providers-github/src/tools/scaffold_ensure.rs`) and
its mocks; commit 2, the fake twin, `catalog_parity.rs`'s new spec-equality/registry/snapshot tests, and
catalog registration (`LIVE_TOOL_NAMES` 35 → 36, `GITHUB_TOOL_NAMES` 3 → 4). Two small deviations from this
section's own text, neither changing the tool's observable behaviour:

- **Trees are memoised per `resolve_tree_paths` call, not across the marker-then-seeds pair of calls.**
  Decision (b) says a directory is "fetched each ... once", and within either call that still holds; what
  it does not do is thread one cache across both calls, so in the `Absent` case every ancestor directory
  the marker and the seed paths share (for Sample: `apps`, `apps/sample`) is fetched twice — once resolving
  the marker alone, once resolving the seeds. This is deliberate, not an oversight: decision (b)'s own
  "Present issues no tree walk past the marker" requires resolving the marker by itself first, and G1's
  client built `resolve_tree_paths` as one self-contained call with its own cache, not a cache-threading
  API. The cost is at most a handful of extra `GET`s on a scaffold's first run only (a repeat run is
  `Present` after the marker's own single lookup) — accepted rather than widening G1's client for a second
  caller this milestone doesn't otherwise need.
- **A new crate dependency**, `sha1 = "0.10"` on `willikins-providers-fake` (already in `Cargo.lock` since
  G1 added it to `willikins-providers-github`, so no new fetch), so the fake tool computes the same git
  blob sha the live client does and writes a byte-identical marker — pinned in
  `scaffold_fake_agrees_with_live.rs`'s `the_written_marker_is_byte_identical_between_fake_and_live`.

Two "Fix commit"s followed advisor review, landing after the two commits above.

The first: `validate_shape` only bounded `files` to 1–64 entries and refused duplicate/colliding paths,
all independent of any single path's length — but 64 files each near `RepoPath`'s own 1,024-character
bound can still produce a marker over `RepoFile`'s 65,536-character bound, a refusal decision (b)'s table
means to make "before any request" like every other shape refusal, not from inside the commit attempt. A
new `validated_marker_file` helper builds and validates the marker right after `validate_shape` in both
`read` and `ensure` (all of it is knowable from inputs alone), and `ensure`'s retry loop now reuses that
one built value instead of rebuilding it on every attempt. A new test
(`an_over_long_marker_is_invalid_before_any_request_on_read_and_ensure`) pins `Invalid` on both entry
points, against an unreachable client so a request would surface as `Provider` instead if the ordering
regressed. The same pass added four more mock tests decision (b)'s table implied but the first commit's
suite had not yet exercised: two differing seed paths named together in one `Conflict` (not only the
first found), a `100755` marker (right content, wrong mode) reported `Foreign`, a `100755` seed with a
sha this tool would otherwise have written as `Conflict` (mode alone is enough), and a symlink seed path
as `Conflict` too.

The second, a follow-up advisor review of the first: the marker-length check above landed only on the
live tool, so a document would plan `Create` against the fake catalog and fail `Invalid` only once it
reached the live one — acceptance 8's "the fake twin agrees" broken by the very commit meant to close a
gap. The fake tool gained its own `validated_marker_file` (identical shape, `RepoFile::new` over the same
`marker_content`), called from both `read` and `ensure`, with `ensure`'s map write now sourced from that
validated value rather than a bare string. `scaffold_fake_agrees_with_live.rs` gained
`agrees_on_an_over_long_marker`, pinning `Invalid` on both `read` and `ensure` on both sides.

**Addendum:** 2026-09-30 (adversarial pass, render and write) — an independent attack on T1, G1 and G2,
recorded in `docs/research/2026-09-30-m3g-adversarial-pass-render-and-write.md`. Eight mutations: four
killed by existing tests (T1's render bound, the same-head guard, the additions sort, the three-attempt
bound); two survived and are now killed by new tests (the placeholder's closing space, `eac5c41`; a marker
first line that merely starts with the header, `9dec0ce`); two confirm the fixes below. Four real defects,
fixed test-first on both the live tool and the fake: G1's open item (a path beneath an existing file read
`Absent`, so the commit could only fail or replace that file with a directory) is settled as
`PathEntry::UnderNonDirectory`, a `Conflict` for a seed and `Foreign` for the marker, with no live probe
needed since refusing is safe whichever way GitHub behaves (`9422d3e`); a `truncated` tree listing is a
`Provider` failure, never `Absent` (`281e0ef`); the fake now agrees on directories, closing a pre-existing
acceptance-8 gap where a seed path that is a directory refused live but planned `Create` on the fake
(`9422d3e`); and declared paths nested under one another are `Invalid` before any request (`3b4120e`).
The six acceptance-4 negative fixtures E2 deferred to T1 and G2 were missing and are added with their
tests (`3e3d766`); the characterization snapshot changes only by their six added entries, and every
existing document plans byte-identically. No secret, `Text` or caller byte reaches a committed file; a
moved head can neither duplicate nor lose a commit. B1's configuration leak is out of this group.

**Addendum:** 2026-09-30 (B1) — `buildkite.pipeline.bootstrap.gate` landed in both crates, exactly the task
row's two commits: commit 1, the live tool (`crates/willikins-providers-buildkite/src/tools/pipeline_bootstrap_gate.rs`),
a new `BuildkiteClient::get_pipeline_configuration` and `PipelineConfigurationBody` (deserializing only
`configuration`, no `Debug` impl), and redaction tests; commit 2, the fake twin, catalog registration
(`LIVE_TOOL_NAMES` 36 → 37, `BUILDKITE_TOOL_NAMES` 2 → 3), and a `configuration` field added to
`BuildkitePipelineRecord` (`#[serde(default)]`, so no existing fixture breaks). Step 0 (the sandbox probe of
verify item 9) was skipped per the coordinator's own instructions: it was already settled read-only on
2026-09-30 (`GET` on Sample's real pipeline answered `200` with a `configuration` field, currently the
frozen upload bootstrap). A third, small "Fix commit" followed, and owns its own miss plainly: commit 2
(`64cddfa`) was committed before its own scoped gate was run against it, and that gate then failed --
`willikins-providers-fake/src/lib.rs`'s own whole-catalog JSON-listing snapshot (separate from
`catalog_parity.rs`'s per-tool ones) still held the pre-B1 catalog. `64cddfa` is not green in isolation;
`83f4075` fixes it (diff confirmed as exactly the new tool's one entry, reviewed before accepting), and the
tree is green again from `83f4075` onward. The lesson, same as `docs/plans/2026-09-23-milestone-3c-app-store-signing.md`'s
own verifier note: run the scoped gate *before* the commit it belongs to, not after.

One acceptance-9 clause this task cannot prove: "appears in no `BlockedGate` or journal line" needs a
`plan` run over a document that actually binds this gate, which does not exist yet -- that is W1's own
acceptance 10 ("a first fake run creates the scaffold and blocks on the bootstrap gate"), not B1's. This
task proves the narrower, provable half: the marker appears in no `Observation`, `Ensured`, or `ToolError`.

Four deviations from a literal reading of this section's own text:

1. **`structurally_equal` parses YAML into a `serde_json::Value`, not a `serde_yaml_ng::Value`** — a
   literal reading of decision (h)'s "both strings parsed as YAML into a JSON value and compared for
   equality". Both crates already depend on `serde_json`, so this added no further dependency.
2. **The gate's `need`/`how` strings are generic, not Sample-specific.** This section's own prose quotes
   Sample's own wording ("the sample pipeline's stored bootstrap...", "...apps/sample/.buildkite/bootstrap.yml
   from the monorepo") to explain the gate in context, but the tool itself serves any document, and decision
   (j)'s own rule ("a gate never authors a string from its inputs") means `need`/`how` cannot name a
   document-specific path anyway. The landed strings describe the gate generically: "this pipeline's stored
   configuration equals the bootstrap this document renders" / "in the pipeline's Settings, Steps page,
   replace the YAML with the rendered bootstrap file this document committed, save, then re-run this
   document" — "renders", not "committed", would have been more honest in the `need` text (nothing is
   committed by a first run that blocks), but the shipped `how` already says "committed" in its second half;
   left as a known wording nit rather than re-litigated here, since neither `need` nor `how` is otherwise
   wrong and both are `&'static str` a later task can still tighten.
3. **A new dependency, `serde_yaml_ng`, added directly to both `willikins-providers-buildkite` and
   `willikins-providers-fake`'s `Cargo.toml`** (not named in this plan or its pre-flight) — already a
   workspace dependency (`willikins-dsl`, `willikins-cli`), so `Cargo.lock` only gained two new
   `dependencies` edges, no new `[[package]]` entries and no network fetch.
4. **An `expected` that fails to parse as YAML reads `Absent` forever**, with the gate's own static `how`
   telling the operator to paste YAML that can never make it equal — a document bug knowable from inputs
   alone. `github.scaffold.ensure`'s own precedent (an over-long marker refused `Invalid` before any
   request) would argue for the same here, but `expected` is a `RepoFile` already validated by
   `repo.file.render`'s own bound, not a caller-controlled string this gate parses itself for shape; adding
   a plan-time YAML-validity check was judged outside this task's own scope (the tool table names no
   `Invalid` outcome for this gate) and is left as an open item for whichever task next touches this tool.

One further note, not a deviation: `BuildkitePipelineRecord`'s new `configuration` field carries the
struct's existing derived `Debug`/`Serialize`, where the live crate's `PipelineConfigurationBody`
deliberately has neither. Acceptable: the fake's state is test-only synthetic data, never a real operator's
bootstrap, and every other field on the same struct (`repository`, `cluster_id`) is already derived the
same way.

Verified with `cargo fmt --all --check`; `cargo clippy -p willikins-providers-buildkite --all-targets -j 2
-- -D warnings` and `RUST_TEST_THREADS=2 cargo test -p willikins-providers-buildkite -j 2` both green in
isolation (commit 1 alone, via `git stash`, before commit 2 landed) and again after; `cargo clippy -p
willikins-providers-fake --all-targets` and its own test suite; `cargo clippy -p willikins-server
--all-targets` and `cargo test -p willikins-server` (the mcp meta-tool schema snapshot unchanged); `cargo
test -p willikins-providers-doppler --test live_catalog`; `cargo check -p willikins-types`; `cargo test -p
willikins-dsl --test acceptance` (characterization snapshot byte-identical: no document binds this tool
yet) — all green.

**Addendum:** 2026-09-30 (L1) — the live scaffold cycle harness landed, written and compiling, never run:
`crates/willikins-providers-github/tests/live_scaffold_cycle.rs`, gated by a new `live-tests` feature on
this crate (`Cargo.toml`'s own `[[test]] name = "live_scaffold_cycle" required-features = ["live-tests"]`,
mirroring `willikins-providers-doppler`/`-buildkite`/`-appstore`'s own feature exactly) plus `#[ignore]`
plus `WILLIKINS_LIVE_TESTS=1` — the same three-deep gate every other live write cycle in this workspace
uses. This crate's two older live tests (`tests/live_write_cycle.rs`, `tests/live_probe.rs`) predate the
`live-tests` feature and are untouched: still `#[ignore]`d but always compiled, exactly as before.

Nine steps, one function each, following the plan's own "The live scaffold cycle" section literally: count
and refuse a leftover (1); a raw `POST .../orgs/{org}/repos` with `auto_init: true`, guard armed before the
call (2 — `github.repo.ensure`'s own `create_repo` never sets `auto_init`, and an uninitialized repository
gives `createCommitOnBranch` no branch to land on); `github.scaffold.ensure` creates the scaffold,
independently confirmed by a raw `GET` of the resulting commit (one parent, the init commit;
`verification.verified` recorded, settling verify item 2) and its tree (every file plus the marker,
`100644`, the expected git blob sha) (3); a second `ensure` converges (4); a raw `PUT .../contents/{path}`
edits one seeded file directly, and it survives untouched (5); a second scaffold naming the edited path
with its original content is refused `Conflict` (6); a client-level `createCommitOnBranch` with a stale
`expectedHeadOid` fails, recorded by status/key-set/`errors[].type` only, never `message` (7, settling
verify item 4); one `read` through the bound `token` port against an *uncredentialed* default client, so
the read can only have succeeded through the port (8); delete, confirm `404`, confirm the repository count
returns to step 1's (9). Three further `#[test]`s, not `#[ignore]`d, run today under the feature with no
network call and actually pass: `ProjectSlug::parse` accepts this cycle's own repository-name grammar,
`blob_sha` matches git's well-known empty-blob sha1, and `expected_marker_content` sorts by path under the
required header.

Two choices G1's own addendum (note 3) left "to whichever of G2/L1 lands first" — G2 landed first and
chose not to widen anything, so this task follows that answer rather than reopening it:

1. **The five read/write methods on `GitHubClient` stay `pub(crate)`.** Step 3's tree/commit verification
   and step 7's stale-`expectedHeadOid` mutation are both raw, hand-built requests against the shared
   `Http` client instead — the same shape `tests/live_write_cycle.rs`'s own `repo_path` comment and
   `tests/scaffold_ensure_mock.rs`'s own duplicated `blob_sha` already use for a `pub(crate)` algorithm or
   path an external test crate cannot reach. This file carries its own copies of `client::git_blob_sha`,
   `scaffold_ensure::marker_content` (as `expected_marker_content`), and `client.rs`'s own fixed GraphQL
   mutation text, on purpose: step 3 and step 7 are meant to check independently of what the tool itself
   believes, not call back into it.
2. **Step 8 reads `WILLIKINS_GITHUB_TOKEN` directly, once, via `std::env::var`** — the one thing
   `tests/live_write_cycle.rs`'s own doc comment says its cycle never needs to do. This cycle does: a bound
   `token` port needs an actual `GitHubToken`, and a `Credential` has no sanctioned way to hand its bytes to
   a caller outside `willikins-providers-http`. The plan's own words sanction exactly this ("the `token`
   port with the same sandbox PAT resolved in-process"). The plaintext `String` is parsed into a
   `GitHubToken` and dropped immediately; it is never pushed onto the sweep, printed, or formatted.

One deviation from a literal reading of the plan's own step list: step 1 and step 9's repository count is a
single `GET /orgs/{org}/repos?per_page=100&type=all`, which refuses (panics, naming the org) rather than
silently miscounting if a page ever comes back full — this harness implements no pagination, on the
judgement that the sandbox org's repository count stays well under 100 and a loud refusal is safer than a
quiet undercount.

Verified with `cargo fmt --all --check`; `cargo clippy -p willikins-providers-github --features live-tests
--tests -j 2 -- -D warnings` green (one `clippy::doc_markdown` fix and one `clippy::too_many_lines` split,
factoring step 3's own tree/commit verification out into `verify_landed_scaffold`); `cargo test -p
willikins-providers-github --features live-tests --test live_scaffold_cycle --no-run -j 2` green (compiles;
never executed), and the same test run without `--no-run` shows its three non-`#[ignore]`d unit tests pass
while `github_live_scaffold_cycle` itself is correctly filtered out; `cargo clippy -p
willikins-providers-github --all-targets -j 2 -- -D warnings` and `RUST_TEST_THREADS=2 cargo test -p
willikins-providers-github -j 2 --no-fail-fast` both green without the feature, confirming
`live_write_cycle.rs`/`live_probe.rs` and every existing test are unaffected; `cargo check -p
willikins-types -j 2` green. `cargo test -p willikins-dsl --test acceptance` was not run: `willikins-dsl`
carries no dependency on `willikins-providers-github` at all (checked directly in its `Cargo.toml`), so this
task cannot have moved that snapshot.

This harness is written, not run: per the task table, opus runs it once against the sandbox org
`Willikins-Test` (`WILLIKINS_SANDBOX_GITHUB_ORG`), sourcing `~/.config/willikins/sandbox.env` in the same
command as the run itself.

A "Fix commit" after advisor review: the four facts this harness is supposed to *record* for verify items 2
and 4 (step 3's `commit.verification.verified`, step 7's response top-level keys, its `errors[0]` key set
and `type`, and its transport-failure status) were going through `Cycle::note`, which only keeps a string
for the final redaction sweep and never prints it -- so a `--nocapture` run would have shown "step 3 …
pass" / "step 7 … pass" and settled neither verify item, with the facts themselves dropped at exit
alongside `sweep`. All four now go through `Cycle::say` instead (still swept, now also printed); nothing in
them is secret (a `bool`, JSON key names, and an `errors[].type` enum string -- exactly the "key names
only, never `message`" rule already in force). Verified again with the same four scoped gates (`cargo fmt
--all --check`; `cargo clippy -p willikins-providers-github --features live-tests --tests -j 2 -- -D
warnings`; `cargo test -p willikins-providers-github --features live-tests --test live_scaffold_cycle
--no-run -j 2`; the same run without `--no-run`, its three offline tests still passing), all green.

**Addendum:** 2026-09-30 (W1) — the Sample document landed, exactly the task row's two commits:
commit 1, `workflows/sample-ios-app.yaml` edited in place (the seventeen `repo.file.render`
templates of decision (i), `sample_files`, the new write-token chain, the `pipeline.repo`
rebinding, the two removed acknowledgements and their input, and the header's own new dated
section), plus the minimal edits to the two existing tests that exercise it
(`sample_document.rs`, `sample_apply_blocked_redaction.rs`) and the characterization state
fixture, so the tree stayed green at every step (the document's own `gates_unmet_then_satisfied_then_acknowledged`
scenario now stands in for the operator's M7 paste between its run 2 and run 3 by writing the
rendered bootstrap into the fake pipeline's own stored `configuration`, extracted from an
applied node's own output rather than re-typed by hand); commit 2, five new acceptance-10/11
tests (the data-protection/entitlement class pin, the pre-landing conflict case, the
`pipeline`-ordered-after-`sample_files` pin, the all-seventeen-renders pin) and one `insta`
snapshot per rendered file, read at plan time against the real identifiers (`repo.file.render`
is pure, so `plan` itself carries every render's known `file` output -- no apply needed), each
reviewed by hand before accepting.

Two deviations from a literal reading of this section's own text:

1. **The `.buildkite/bootstrap.yml` and `pipeline.yml` templates omit the `GIT_CONFIG_*`
   host-credential-helper override** AppTwo's own files carry (decision (i)'s own
   "operator item"). AppTwo's override names an absolute path on the operator's own Mac
   (`/Users/operator/buildkite-ci-smoke/github-credential-helper.sh`); committing that path into
   Willikins' own public repository, or into the monorepo through this document, was judged a
   worse trade than leaving it out and reporting the gap (trust boundary 8's same instinct:
   "reproduce nothing identifying from the monorepo that this workspace does not already
   hold"). Flagged for the operator: confirm whether Sample's pipeline needs the same host
   override AppTwo's does, and if so add it to the committed files by hand (or extend this
   document) before the real apply.
2. **The acceptance-10 "pre-landing conflict" test seeds a differing file directly** through
   `FakeState::with_scaffold_files` at a path with no marker present, rather than through a
   first real `apply` followed by a hand-edit -- the shorter, more direct way to reach the same
   `Absent`-with-a-conflicting-seed state decision (b)'s read table describes, and the one
   `github.scaffold.ensure`'s own G2 tests already use for the identical case.

One fact, not a deviation: the write token's Doppler config is `github/example-org_willikins`
(project `github`, config name `example-org_willikins`), exactly decision (l)'s own
recommended name; `DopplerConfigName`'s grammar (`[a-z0-9_-]+`) accepts it without further
widening, matching verify item 11's own expectation.

Verified with `cargo fmt --all --check`; `cargo clippy -p willikins-cli --all-targets -j 2 -- -D
warnings`; `RUST_TEST_THREADS=2 cargo test -p willikins-cli -j 2 --no-fail-fast`; `cargo check -p
willikins-types -j 2`; `cargo test -p willikins-dsl --test acceptance -j 2` (characterization
snapshot diff confirmed as exactly `workflows/sample-ios-app.yaml`'s own entry gaining the new
nodes' port types and losing `m3_repo_files`/`m7_bootstrap`'s, the `PLAN ERROR` line at
`issuer_id_text` unchanged -- acceptance 14); `cargo test -p willikins-core --test
secret_literal_guard -j 2` — all green. Not done by this task, per its own boundary: the
operator's real write token (decision (l)'s own "does not exist yet"), the live scaffold cycle
run (L1, the coordinator's), and marking this plan Completed.

**Addendum:** 2026-10-01 — Sample no longer copies its three App Store profiles into Doppler (the
operator's decision, asked "what's the point of storing the profiles in doppler?"). The monorepo's
CI fetches each profile from Apple by name at build time — the root `fastlane/Fastfile`'s
`get_provisioning_profile(provisioning_name: ..., readonly: true)`, authenticated by the App Store
Connect key `appstore-connect/deploy_ios` already holds; AppTwo names every profile after its
bundle identifier, as Sample does — so the copies were redundant, went stale whenever Apple
invalidated a profile, and kept a converged run from ever reading NoOp: `doppler.secret.set` is a
write-only sink and reports `Created` on every apply. `app_profile_to_doppler`,
`nse_profile_to_doppler` and `widgets_profile_to_doppler` are removed; the three
`appstore.profile.ensure` nodes stay. `sample_document.rs`'s run 3 now asserts nothing at all is
`Created`, with no exclusion list. The three stored secrets in `sample/prd_deployment_ios` are to
be deleted by hand once the real Doppler token authenticates again (it answered 401 on
2026-10-01). `AppleProfileContent` stays secret-typed for now; whether it still needs to be is
its own reviewed change.

**Addendum:** 2026-10-01 (coordinator) — L1's live scaffold cycle ran once in the sandbox org `Willikins-Test`
(`cargo test -p willikins-providers-github --features live-tests --test live_scaffold_cycle -- --ignored`): all
nine steps pass, the redaction sweep over 17 strings passes, and the org's repository count returned to 0. Verify
item 2 is settled: the scaffold commit reads `verification.verified: true`. Verify item 4 is settled: a stale
`expectedHeadOid` answers with `data` and `errors`, `errors[0].type` `STALE_DATA`. Items 1, 3, 5 (at Sample's size),
6, 7, 8 and 11 remain for the real apply; item 8 was not run on this host (disk at 97%) pending the operator.

**Addendum:** 2026-10-01 (coordinator, later) — four more verify items settled before the real apply.
Item 8: W1's 17 rendered files, written into a local clone of the monorepo at `9ac10a8b`, failed
`bazel build --config=ci //apps/sample/...` at loading: `apps/sample/ios/BUILD.bazel` carried `"\d+"` in
`apple_bundle_version`'s capture groups, an escape Starlark rejects, because the template's YAML literal block
keeps a backslash as written (AppTwo and App Three write `\\d`). With the escape doubled the build
completed: 7 targets (the app, both extensions, their three Swift libraries, the bundle version), 112
actions. Fixed in `498e8d4`, with a test that every backslash in a rendered Starlark file starts an escape
Starlark accepts. Item 11: the operator created `github/example-org_willikins` (environment
`example-org`, not inheritable) holding `GH_CONTENTS_WRITE_TOKEN`; Doppler accepted the name. Item 7: that
token, read from Doppler, sees `Example-Org/monorepo` (expires 2027-10-02). Item 6: `main` is not
protected (`GET .../branches/main` reads `protected: false`); `GET .../rules/branches/main` answers that rulesets
need GitHub Pro or a public repository, so no ruleset applies; classic protection reads 403 for this token.
A direct commit to `main` is therefore allowed. Items 1, 3 and 5 settle on the real apply's first write.

**Addendum:** 2026-10-01 (adversarial pass, Sample) — an independent attack on B1, L1, W1 and `2b36422`,
recorded in `docs/research/2026-10-01-m3g-adversarial-pass-sample.md`. Nine mutations, all killed; one
(the fake scaffold re-checking seeds once the marker is present) survived every existing test and is
killed only by a new one. Four W1 gaps are now pinned in `sample_document.rs`: every placeholder sits in a
quoted or identifier-only position, by strict allowlist (`2f150b6`); `sample_files` plans with all
seventeen files known on a first run (`2f39b5b`); a foreign marker fails plan and writes nothing
(`e201cca`); a re-run with every seed edited plans `sample_files` `NoOp` (`fedbfb3`, acceptance 10's
untested clause). Three real defects, fixed test-first: the bootstrap gate read `Present` for a stored
configuration that repeats a key, since a JSON map keeps the last value silently; both twins now report
it different (`8191313`). L1 took its org from the environment unchecked, and now refuses any org but
`Willikins-Test` (`6de9a6f`); its delete guard stayed armed after a `4xx` create answer, so a name
collision would have deleted someone else's repository, and now disarms (`a0179bf`). No secret,
`Text` or configuration reaches a committed file, plan, report or error; the gate only ever `GET`s; a
moved head can neither duplicate nor lose a commit; `2b36422` left only stale comments (fixed); every
other document plans byte-identically. Open: the `GIT_CONFIG_*` override stays the operator's; an empty
stored configuration equals an all-comment expected (low, unreachable for Sample).

**Addendum:** 2026-10-02 (milestone 3i) — decision (h) ("writing it stays out") and trust boundary 6
("compared, never written") are superseded: the bootstrap paste is no longer manual, written by
`buildkite.pipeline.bootstrap.ensure` from a `RepoFile` the document renders. The gate
(`buildkite.pipeline.bootstrap.gate`) stays in the catalog. See
`docs/plans/2026-10-02-milestone-3i-bootstrap-writer-and-identifier-masking.md`.

**Gate:** OPEN — two operator decisions are pending (see "Operator decisions pending"): direct commit versus
branch plus pull request on `Example-Org/monorepo`'s `main` (recommended: direct), and the write
credential (a new fine-grained token in a Doppler config no app inherits). Tasks E1 through B1 and the
sandbox live cycle do not depend on either; task W1's `branch` literal and its token chain do.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (Templates: "The type system guarantees a secret can
never be rendered into a committed file" and "Record template version and answers in the repo"; Trust
model: "Workflow definitions and templates are privileged content"; the 2026-09-21 addendum "Credentials
are ports, resolvers are nodes"; "Policy lives in the workflow, never in the tool").
**Research:** `docs/research/2026-09-20-project-survey-and-workflow-library.md` (rank 1, rank 2, Sample's
entry), `docs/research/2026-09-16-m3a-buildkite.md` (the `configuration` string), and this plan's own
pre-flight, fetched verbatim 2026-09-30 and quoted below with its sources.
**Depends on:** milestone 3e (gates, `Tool::replaces`, `github.repo.get`, the GitHub token port, the Sample
document), Completed; milestone 3a's frozen bootstrap decision (a), which this plan keeps.
**Todo:** `todos/2026-09-29-file-writing.md`. Related: `todos/2026-09-20-ios-scaffolding-step-and-entitlements-as-inputs.md`.
**Next:** milestone 2b (composition: a document calls another with `uses:`, typed outputs), which splits
Sample into a Example-Org organisation document and an ios-app document. Decision (k) records what this
milestone does so nothing here fights that split.

## Goal

The operator's decision, 2026-09-30, verbatim: "Nope. We'll add file capabilities, update the document
with them, and use willikins' idempotency to run that." Sample's manual step M3 — "Add Sample's files
under apps/sample/ in the monorepo: BUILD.bazel (ios_application embedding two ios_extensions), three
entitlements files, Info.plist with both HealthKit usage strings and the two extension points, and
.buildkite/ with provider triggers disabled" — stops being an `operator.acknowledge` leaf and becomes a
node of `workflows/sample-ios-app.yaml` that writes those files as **one commit** and converges on
re-runs. M7 (the stored Buildkite bootstrap) stops being an acknowledgement and becomes an **observed**
gate (decision (h)).

The milestone is done when tasks E1 through W1 are green under scoped gates, the attacker has run the
live scaffold cycle (task L1) once against a **throwaway repository in the sandbox org `Willikins-Test`**
with every count equal before and after, and each piece has had an independent attack. The first write
into `Example-Org/monorepo` is **not** part of this milestone: it is the operator's real apply, after
the two pending decisions and the pre-real-apply verify items (6, 7, 8) are settled.

## Out of scope

- **Rank 2, structured edits to files other projects own** (a TOML `members` array, a `package_group`
  row). The survey below finds that adding Sample needs **none** today (see "The monorepo, surveyed").
  When Sample later depends on `//platform/ios` or `//platform/ffi`, or adds a Rust crate, the edit is
  the developer's code change in the same commit as the code that needs it, not provisioning.
- **Managed files and `Replace`.** Re-rendering an already-landed file when a template changes (the design
  doc's "re-render, three-way merge, and open a PR per repo when an org convention changes") is a later
  milestone. This one only seeds (decision (c)); the marker it writes records the base that milestone
  needs.
- **Deleting, renaming or moving files; binary files; the executable bit** (mode `100644` only: the
  monorepo's own AppTwo `upload-pipeline.sh` is `100644` and is run as `bash …`).
- **`.github/workflows/`**: refused by the path type (decision (g)), so the write token never needs the
  Workflows permission.
- **Writing Buildkite's stored `configuration`**: milestone 3a decision (a) stands (decision (h)).
- **Branch creation and pull requests**, unless the operator chooses the PR route (tasks P1–P3 are then
  added; decision (f)).
- **A real template engine** (loops, conditionals, filters): decision (d).
- **SigNoz.** The key expired on 2026-09-23; nothing in this milestone calls it.
- **Railway.** No Railway command.

## Trust boundaries (normative)

They extend milestone 3e's nine, which still hold.

1. **Live GitHub writes only in the sandbox org `Willikins-Test`**, only on a throwaway repository named
   `willikins-files-<unix-seconds>` created and deleted by the same guarded test. Nothing is ever written
   to `Example-Org/*` or `Lightless-Labs/*` by an agent in this milestone.
2. **The operator's own `gh` credential is never used**; the sandbox PAT (`WILLIKINS_GITHUB_TOKEN` in
   `~/.config/willikins/sandbox.env`) is resolved in the same command that uses it, on stdin, never argv.
3. **Delete only the repository the same run created**, by the exact name it recorded before any
   assertion, through a drop guard (milestone 2's `DeleteGuard` in
   `crates/willikins-providers-github/tests/live_write_cycle.rs`).
4. **No committed byte comes from a caller.** File content is a document-literal `TemplateSource` plus
   substitutions of `TemplateValue`s, whose grammar admits no whitespace, quote, `$`, backtick, `;`,
   `|`, `&`, `<`, `>`, `{`, `}` or newline (decision (e)). A `TemplateSource` or a `RepoFile` can never be
   a workflow input or an input default.
5. **No secret reaches a committed file**, by type (decision (e)); nothing a test prints, records or
   commits contains a credential-shaped string (`secret_literal_guard` covers docs and fixtures).
6. **Buildkite's stored `configuration` is compared, never written, echoed, logged or output** (decision (h)).
7. **Nothing under `/Users/operator/Projects/example-org` is written.** It is read for its pattern.
8. **Reproduce nothing identifying from the monorepo that this workspace does not already hold:** no Apple
   team id (decision (j) drops `team_id` from the template), no host path from its stored bootstraps in
   this plan (the template's copy of that override is decision (i)'s open item).

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| Engine change (E1) | a YAML sequence under `with:` parses to `Binding::List(Vec<Binding>)`; elements are references or scalar literals, never nested |
| New types (E2), all public | `RepoPath`, `GitBranchName`, `CommitHeadline`, `TemplateValue`, `RepoFile` (grammars in decision (g) and (e)) |
| New conversion (E2) | `AppleBundleIdentifier => TemplateValue` (total by grammar containment); **no** row from `Text`, ever |
| Check refusals (E2) | `TemplateSource` or `RepoFile` as a workflow input type; a default of either; a literal bound to a `RepoFile` port |
| New pure tool (T1) | `repo.file.render` in `willikins-tools`: inputs `path: RepoPath` (required), `template: TemplateSource` (required), `values: list<TemplateValue>` (optional); output `file: RepoFile`; `Reversible`, pure, no key |
| Placeholder syntax (T1) | exactly `{{ N }}`, one space inside each brace, `N` a decimal index `0`…`15`; at most 16 values |
| New tool (G2) | `github.scaffold.ensure`: inputs `repo: GitHubRepo`, `branch: GitBranchName`, `marker: RepoPath` (the key, in that order), `files: list<RepoFile>` (required, 1–64 entries), `message: CommitHeadline` (required), `token: GitHubToken` (optional credential port); outputs `repo`, `branch`, `marker` (pass-through only); **`Irreversible`**; not pure |
| Marker content | first line exactly `managed-by: willikins`, then one line per seeded file, `<40-hex blob sha> <path>`, sorted by path, trailing newline |
| New gate (B1) | `buildkite.pipeline.bootstrap.gate`: inputs `org: BuildkiteOrg`, `slug: BuildkitePipelineSlug`, `expected: RepoFile`, `token: BuildkiteToken` (optional); output `slug`; pure; a gate; `subject` = `org`, `slug` |
| `LIVE_TOOL_NAMES` | 34 → 37 (`crates/willikins-server/src/catalog.rs` line 62), and every site that pins the count |
| GitHub write endpoint | `POST https://api.github.com/graphql`, one fixed mutation text (`createCommitOnBranch`), variables only |
| GitHub read endpoints | `GET /repos/{owner}/{repo}/git/ref/heads/{branch}`, `GET …/git/commits/{sha}`, `GET …/git/trees/{sha}` (non-recursive), `GET …/git/blobs/{sha}` (the marker only) |
| Sample scaffold node | `sample_files`, `repo: Example-Org/monorepo`, `branch: main` (**pending operator decision 1**), `marker: apps/sample/.willikins-scaffold` |
| Sample commit headline | `feat(sample): scaffold the iOS app, NSE, widgets and Buildkite files` (the monorepo's own conventional style) |
| Sample write token (pending decision 2) | real Doppler workplace, project `github`, branch config `example-org_willikins` (not inheritable), secret `GH_CONTENTS_WRITE_TOKEN` |
| Sandbox throwaway repository | `Willikins-Test/willikins-files-<unix-seconds>` (26 characters; `ProjectSlug` allows 32) |
| Live cycle harness (L1) | `crates/willikins-providers-github/tests/live_scaffold_cycle.rs`, its own `[[test]]` with `required-features = ["live-tests"]`, `#[ignore]`, `WILLIKINS_LIVE_TESTS=1` |
| Characterization snapshot | every existing entry byte-identical **except** `workflows/sample-ios-app.yaml`'s own, which the operator's 2026-09-30 decision changes (acceptance 14) |

## Pre-flight: sources, verbatim

Fetched 2026-09-30, read-only. GitHub's OpenAPI description `github/rest-api-description`
`descriptions/api.github.com/api.github.com.json` (version 1.1.4); `github/docs` markdown under `content/`;
`github/docs` `src/github-apps/data/fpt-2022-11-28/fine-grained-pat-permissions.json`; `github/docs`
`src/graphql/data/fpt/schema.docs.graphql`; `buildkite/docs` `pages/apis/rest_api/pipelines.md`.

### Contents API versus the Git database API

- **Contents API: one commit per file.** `PUT /repos/{owner}/{repo}/contents/{path}`, "Creates a new file or
  replaces an existing file in a repository", and: "If you use this endpoint and the 'Delete a file'
  endpoint in parallel, the concurrent requests will conflict and you will receive errors. You must use
  these endpoints serially instead." Answers `200, 201, 404, 409, 422`. Sample's seventeen files would be
  seventeen commits, and a failure part-way leaves a half-scaffold on `main`. **Rejected for writing.**
  `GET …/contents/{path}` also has a trap for reading: "If the content is a symlink and the symlink's
  target is a normal file in the repository, then the API responds with the content of the file" — a
  symlink at a seeded path would read as a file. Reads therefore use trees (below).
- **Git database: several files as one commit.** The guide
  (`content/rest/guides/using-the-rest-api-to-interact-with-your-git-database.md`): "Get the current commit
  object · Retrieve the tree it points to · … post a new blob object … Post a new tree object … Create a new
  commit object with the current commit SHA as the parent and the new tree SHA … Update the reference of
  your branch to point to the new commit SHA." And: "The REST API will return a `409 Conflict` if the Git
  repository is empty … For an empty repository, you can use the `PUT /repos/{owner}/{repo}/contents/{path}`
  REST API endpoint to create content and initialize the repository." `POST …/git/trees`' `base_tree`:
  "If not provided, GitHub will create a new Git tree object from only the entries defined in the `tree`
  parameter. If you create a new commit pointing to such a tree, then all files which were a part of the
  parent commit's tree and were not defined in the `tree` parameter will be listed as deleted by the new
  commit." Tree entry `mode`: "one of `100644` for file (blob), `100755` for executable (blob), `040000` for
  subdirectory (tree), `160000` for submodule (commit), or `120000` for a blob that specifies the path of a
  symlink."
- **A ref update can refuse rather than clobber.** `PATCH /repos/{owner}/{repo}/git/refs/{ref}` body:
  `force` — "Indicates whether to force the update or to make sure the update is a fast-forward update.
  Leaving this out or setting it to `false` will make sure you're not overwriting work." Answers `200, 409,
  422`. A commit whose only parent is the head read earlier is a fast-forward only while the branch has not
  moved, so `force: false` is a compare-and-swap in effect.
- **GraphQL `createCommitOnBranch`: one call, exact compare-and-swap, signed.** `schema.docs.graphql`:
  "Appends a commit to the given branch as the authenticated user. This mutation creates a commit whose
  parent is the HEAD of the provided branch and also updates that branch to point to the new commit." Input
  `expectedHeadOid: GitObjectID!` — "The git commit oid expected at the head of the branch prior to the
  commit". `FileAddition { contents: Base64String!, path: String! }`. "A commit created by a successful
  execution of this mutation will be authored by the owner of the credential which authenticates the API
  request. The committer will be identical to that of commits authored using the web interface." And:
  "Commits made using this mutation are automatically signed by GitHub if supported and will be marked as
  verified in the user interface."

### Branch protection, rulesets, and an API commit

- `about-protected-branches.md`, "Require signed commits": "contributors and bots can only push commits that
  have been signed and verified to the branch." "Restrict who can push to matching branches": "only users,
  teams, or apps that have been given permission can push to the protected branch … People, teams, and apps
  that have permission to push to a protected branch will still need to create a pull request when pull
  requests are required." "By default, the restrictions of a branch protection rule don't apply to people
  with admin permissions to the repository or custom roles with the 'bypass branch protections' permission."
- `available-rules-for-rulesets.md`: "Require a pull request before merging — You can require that all
  changes to the target branch be associated with a pull request." "Restrict updates — If selected, only
  users with bypass permissions can push to branches or tags whose name matches the pattern you specify."
  "Require signed commits — … With both methods, we use the `verified_signature?` to confirm if a commit has
  a valid signature. If not, the update is not accepted."
- `data/reusables/repositories/required-signed-commits.md`: "unsigned commits on the head branch can block a
  squash merge, even though GitHub would sign the final squash commit."
- `about-commit-signature-verification.md`: GitHub signs web-interface commits; "Signature verification for
  bots will only work if the request is verified and authenticated as the GitHub App or bot and contains no
  custom author information, custom committer information, and no custom signature information, such as
  Commits API." So a Git database commit made with a personal token is **unsigned**, and a
  `createCommitOnBranch` commit is signed.
- The rules actually in force on a branch are readable with Metadata read only:
  `GET /repos/{owner}/{repo}/rules/branches/{branch}` (fine-grained permission `metadata: read`, from
  `fine-grained-pat-permissions.json`).

### What a fine-grained token needs

From `fine-grained-pat-permissions.json`, verbatim fields `{permission, verb, requestPath, access,
additional-permissions}`:

| Endpoint | Permission | Access |
| --- | --- | --- |
| `GET /repos/{owner}/{repo}/git/ref/{ref}` | contents | read |
| `GET …/git/commits/{commit_sha}`, `GET …/git/trees/{tree_sha}`, `GET …/git/blobs/{file_sha}` | contents | read |
| `POST …/git/blobs`, `POST …/git/trees`, `POST …/git/commits` | contents | write |
| `PATCH …/git/refs/{ref}`, `POST …/git/refs` | contents **and** workflows | write, `additional-permissions: true` |
| `PUT …/contents/{path}` | contents **and** workflows | write, `additional-permissions: true` |
| `POST /repos/{owner}/{repo}/pulls` | pull_requests | write |
| `GET …/rules/branches/{branch}` | metadata | read |

The Workflows entries are the additional permission the OpenAPI text states for the contents endpoint:
"The `workflow` scope is also required in order to modify files in the `.github/workflows` directory." A
token that never touches `.github/workflows/` needs **Contents: read and write** only (Metadata: read is
granted to every fine-grained token). GraphQL, `content/graphql/guides/forming-calls-with-graphql.md`: "The
data that you are requesting will dictate which scopes or permissions you will need"; the same Contents
permission governs `createCommitOnBranch` (verify item 3).

### Buildkite's stored configuration

`buildkite/docs` `pages/apis/rest_api/pipelines.md`: the create request's example sends
`"configuration": "env:\n \"FOO\": \"bar\"\nsteps:\n - command: …"` and the create **response** example
returns `"configuration": "env:\n \"FOO\": \"bar\"\n\"steps\":\n - command: …"` — the key came back
re-quoted, so the stored string is **not** guaranteed byte-identical to what was sent. The "Get a pipeline"
response example shows no `configuration` field at all. Both facts shape decision (h) and verify item 9.

## The monorepo, surveyed read-only

`/Users/operator/Projects/example-org` (remote `Example-Org/monorepo`), 2026-09-30, `main` at `9ac10a8b`.

- **How it lands changes: directly on `main`.** `CLAUDE.md` (and `AGENTS.md`), "Monorepo & Trunk-Based
  Development (owner ruling 2026-07-09)": "Trunk-based development. Simple, reliable. Work on `main`, commit
  directly to `main`, push to `main`. Do **not** create feature branches. Do not leave work unmerged on a
  branch." History agrees: 494 commits since 2026-09-01, **none** merged through a pull request, every one
  unsigned (`%G?` = `N`). So `main` accepts unsigned direct pushes from the operator's own identity today.
  Whether a ruleset exists that the operator bypasses as an administrator is not observable from files
  (verify item 6).
- **No CODEOWNERS** anywhere. `.github/workflows/` holds `ci.yml` and two AppTwo audits.
- **`ci.yml` builds everything on every push to `main`:** `bazel build --config=ci -- //...
  -//apps/app-two/ios/...` and `bazel test` the same, on `getmac-tahoe`. App Three's iOS app is
  built by that line. **A committed `apps/sample/ios/BUILD.bazel` whose targets do not build turns `main`
  red on the first push**, so the scaffold must build under `--config=ci` (entitlements `None`, profiles
  `None`), which requires minimal Swift entry points and the two extensions' `Info.plist`s (decision (j);
  verify item 8).
- **Layout of an iOS app.** `apps/<app>/BUILD.bazel` is a two-line name reservation (every sibling has one;
  Sample's directory is empty and, being empty, **does not exist on GitHub at all**). The app lives in
  `apps/<app>/ios/BUILD.bazel` with `config_setting`s scoped to `//apps/<app>:__subpackages__` (`ci_build`,
  `internal_build`, `ios_simulator_build`, `beta_build`, …), `apple_bundle_version`,
  `local_provisioning_profile`s (one local wildcard, one distribution profile per identifier named exactly
  after it), `swift_library` per module, and `ios_application` / `ios_extension` with entitlements and
  profiles chosen by `select()`. Resources in `ios/Resources/`: `Info.plist`, `<Target>.entitlements`,
  `PrivacyInfo.xcprivacy`, extension `…-Info.plist`s. App Three (`apps/app-three`) is the
  single-target shape; AppTwo is the multi-extension shape (its `extensions = []` is deliberate;
  Sample's host embeds both).
- **`.buildkite/`** per app: `pipeline.yml`, `upload-pipeline.sh` (`exec buildkite-agent pipeline upload
  --no-interpolation <file>`), `bootstrap.yml` (the stored bootstrap, checked in "because Buildkite runs it
  before the repository exists and nothing else records it"; JSON text: a `GIT_CONFIG_*` override selecting
  the Mac host's credential helper, one step on queue `ci-macos-apple-silicon` running `bash
  apps/<app>/.buildkite/upload-pipeline.sh`), `provider-settings.json` (every provider trigger `false`),
  `README.md`. App Four in this monorepo is a name reservation only.
- **Does adding `apps/sample/` need an edit outside it? No**, file by file:
  - `build/visibility/BUILD.bazel`'s `ios` `package_group` lists `//apps/app-two/...`,
    `//apps/app-four/...`, `//apps/app-six/...`, `//apps/app-five/...`. It gates only targets that
    declare `visibility = ["//build/visibility:ios"]`, i.e. `//platform/ios` and an app's own targets.
    App Three is **not** in the group and builds, because neither it nor AppTwo depends on any
    `//platform` target. A SwiftUI-only scaffold needs no row. A row becomes necessary only when Sample
    depends on `//platform/ios` or `//platform/ffi` (the Rust core through UniFFI) — a rank 2 edit that
    stays the developer's, in the commit that adds the dependency.
  - Root `Cargo.toml` `members`: only when Sample adds a Rust crate. Same answer.
  - `MODULE.bazel` already carries `rules_apple` 4.3.3, `rules_swift` 3.4.1, `apple_support`,
    `bazel_skylib`; `.bazelrc`'s `ci`/`ios_sim` configs are global. No edit.
  - No Buildkite pipeline list in the repository (pipelines live in Buildkite; the document creates it).
  - Root `BUILD.bazel` exports four files; no app list.

## Decisions

### (a) Many files, one node: a list binding in `with:` (engine change E1)

A commit node needs N (path, content) pairs, and `Binding` is `Input | Step | Keyed | Literal`; the D1
addendum of milestone 3e already records the gap ("the document format has no syntax to bind a list literal
to a `with:` port"). Three ways out:

1. **`Binding::List`** — a YAML sequence under `with:` whose elements are references or scalar literals.
   **Chosen.** It is the general primitive, it closes D1's `base_configs` gap for free, and it keeps one
   file per node in the document, which reads the way the operator reads the monorepo.
2. Chained accumulator nodes (`files.add { into, path, content } -> { into }`), no engine change: a
   17-link chain whose order is load-bearing and whose intermediate type exists only to work around the
   DSL. Thrown away the day (1) lands. Rejected.
3. One multi-file bundle template in an invented container format: still needs (1) for its values, and
   invents a format willikins would own forever. Rejected.

Rules for (1), all in `willikins-dsl` and `willikins-core`:

- **Parse.** A `with:` value that is a YAML sequence becomes `Binding::List(Vec<Binding>)`; each element
  is parsed by the existing `parse_with_value` (`${{ … }}` reference, else literal). A nested sequence or a
  mapping element is a parse error naming the node and port. `outputs:` and `for_each:` do not accept
  sequences.
- **Check.** The port must be `PortType::Exact` of a **list** type `list<T>` (`AnySecret` refuses lists
  already). Each element is checked as a scalar binding against `T`, with the one-hop conversion rule of
  milestone 3d recorded per element edge. A list-typed reference as an element is a type mismatch (no
  flattening). **Taint per element**: a secret element bound into a non-secret `T` is
  `SecretToNonSecretSink` (reported before any type mismatch, as today). The error `Site` gains an element
  index (`Site::Port` plus `index`, or a new `Site::ListElement`; E1's choice, rendered `node.port[i]`).
  An input referenced only inside a list counts as consumed.
- **Edges.** Every `Step`/`Keyed` element is a data edge (ordering, cycle detection).
- **Plan and apply.** Elements resolve in order; if any element is `Unknown`, the whole value is
  `Value::unknown(list<T>)`; otherwise a known list. **The skip scan of decision (j) (3e) must include list
  elements**: a node any of whose list elements names a blocked or skipped node plans `Skip` and is never
  read. `apply`'s input resolution resolves list bindings the same way.
- **Byte-identity.** No shipped document uses a sequence under `with:` (they fail to load today), so every
  characterization entry is unchanged; `Binding`'s serde form for the existing variants is unchanged.

### (b) The tool: `github.scaffold.ensure`, one commit

Name and shape in SHARED VALUES. The resource it ensures is **"this scaffold has landed on this branch"**,
keyed by `(repo, branch, marker)`. `files` is the content, not the key.

**Write API: GraphQL `createCommitOnBranch`.** Both candidates land several files as one commit and both
can refuse when the branch moved (REST `PATCH` with `force: false` is fast-forward-only;
`expectedHeadOid` is an exact compare-and-swap). Three facts decide it: it is **one** call where the Git
database needs four kinds (`blobs`, `trees`, `commits`, `PATCH refs`) and 20-odd requests; its commits are
**signed by GitHub**, so a "Require signed commits" rule on `main` does not refuse it, where a Git database
commit made with a personal token is unsigned and would be; and its compare-and-swap is exact. The costs,
accepted: a new transport in the client (a `POST /graphql` with one fixed mutation text and variables — the
no-arbitrary-API-path invariant holds, as it does for every fixed REST path), and GraphQL's
**200-with-`errors`** failure shape. The client parses `data.createCommitOnBranch.commit.oid` and treats
any non-empty `errors`, any missing `data`, or any non-200 as a failure; it **never echoes `errors[].message`
or any response body** (the 3c lesson on bodies) and never interprets it: every failure is resolved by
re-reading, below. Author: the token's owner; committer: GitHub's web-flow identity (quoted above).

**Reads: the REST tree, pinned to one commit.** `GET …/git/ref/heads/{branch}` → head commit sha `H`
(404 → `NotFound`: the tool never creates a branch); `GET …/git/commits/{H}` → root tree; then walk
**non-recursive** trees only along the directories of the declared paths, memoised per directory (the
monorepo's recursive tree could hit the "100,000 entries … 7 MB" truncation; Sample's paths touch about
eight directories). Each path yields: absent; a blob entry with mode and blob sha; or a non-blob entry
(tree, symlink `120000`, submodule `160000`). A file's content is compared by **git blob sha**, computed
locally as SHA-1 over `blob <byte length>\0<bytes>`, so content is never downloaded. The marker is the only
blob ever fetched (`GET …/git/blobs/{sha}`, base64), to check its first line.

**`read`:**

| State at head `H` | Observation |
| --- | --- |
| marker present, first line exactly `managed-by: willikins` | `Present` (the scaffold has landed; no seeded path is read) |
| marker present, any other first line, or the marker path is not a `100644` blob | `Foreign` → `PlanError::NameTaken` |
| marker absent; every seed path absent, or present as a `100644` blob with the rendered blob sha | `Absent` (predicted outputs `repo`, `branch`, `marker`) |
| marker absent; any seed path present with a **different** blob sha, a different mode, or as a non-blob | **refused**: `ToolErrorKind::Conflict` naming each such path (never its content) and saying the scaffold would overwrite it |
| a declared path appears twice, the marker is one of `files`, or `files` is empty or over 64 | `Invalid`, before any request |

**`ensure`:** re-read at the current head `H'`. `Present` → `changed: false`. `Absent` → one
`createCommitOnBranch` with `expectedHeadOid: H'`, `additions` = every seed file not already byte-equal at
`H'` plus the marker, headline `message`, body `Seeded by willikins. Marker: <marker>.` Success →
`changed: true`. **Any** failure → re-read: `Present` → `changed: false` (someone landed it);
`Absent` with a head different from `H'` → the branch moved under us and nothing of ours landed, so
commit again against the new head, at most **three** attempts in all (safe because the decision is made from
the re-read, never from the error body, and the compare-and-swap forbids a duplicate); `Absent` at the same
head, or a conflict → the original failure as `Provider` (or the `Conflict` above), "re-run this document".
A busy trunk (494 commits in September) is why the bounded retry exists.

**Outputs are pass-through only** (`repo`, `branch`, `marker`). The characterization snapshot shows the
plan fingerprint includes every node's **outputs**; an output carrying the head sha or the new commit's
oid would make every plan-to-apply window on a busy `main` drift. The commit oid is not an output.

**Class: `Irreversible`.** A commit on a shared branch cannot be undone without a force push, but the tool
never overwrites or deletes anything (decision (c)), so not `Destructive`. `Irreversible` requires
approval (`Class::requires_approval`), which is right for writing to someone's `main`; Sample already
requires approval on every run (`appstore.profile.ensure` is `Destructive`).

### (c) What Present, Absent and "different content" mean: a seed, owned by the repository once landed

The decisive fact: Sample's document is **re-run as its resume** (milestone 3e decision (j)), and the
developer — and the operator's agents — start editing `BUILD.bazel`, `Info.plist` and the rest the day the
scaffold lands. Every candidate that compares the seeded files on later runs makes the document un-re-runnable
after the first edit:

- **Refusal** on a differing file: every later plan fails at this node. Stuck.
- **A gate**: every later run ends `Blocked` (exit 3) forever. Stuck.
- **`Replace` shown in the plan** (the replace-when-INVALID precedent, `Tool::replaces`, `Plan::replacing`):
  every later plan offers to destroy the developer's work, and approval is all-or-nothing, so the operator
  can neither approve it nor apply the rest. Stuck, and one mis-click from data loss.

So a scaffold is a **seed**: willikins writes it once and it then belongs to the repository. The "landed"
signal must survive edits, moves and deletions of individual files, so it is a **marker file** willikins
writes in the same atomic commit — the codebase's ownership-marker precedent (`managed-by-willikins` topic on
repositories, `managed-by: willikins` pipeline description) and the design doc's own "Record template
version and answers in the repo so a later run can re-render, three-way merge". Its per-file blob shas are
exactly the merge base that later milestone needs. Its path is a port: the document chooses it (Sample:
`apps/sample/.willikins-scaffold`), so the tool imposes no location.

**Overwriting is never silent, because it never happens**: once landed, seeded files are never read or
written again; before landing, a path that already holds other content is a **refusal** naming the path —
willikins has never owned that file, so only a human can decide whether to delete it or change the template.
Byte-equal files already present are skipped, so a scaffold that someone half-applied by hand from the same
templates still converges. Deleting the marker is the deliberate way to ask for a re-seed, and it then
refuses on every file that differs — loudly, never destructively.

`Replace` and managed files wait for the re-render milestone, which will have the marker's base to merge
against and a pull request to put the result in front of a human.

### (d) Templates: where they live and how values substitute

**Where: inline in the document, as YAML block scalars bound to `repo.file.render.template`.** Workflow
documents are already the privileged, trusted-ref content (design doc, Trust model); a template inside one
is reviewed in the same diff, run from the same ref, characterised by the same snapshot, and moves with its
document when 2b splits Sample (the ios-app document takes its templates with it). The alternative — template
files beside the document, loaded by a DSL include — adds a loader, path confinement and a second privileged
artifact for readability alone; it is the right move when several documents share one template, which
nothing does yet. Rejected for now, recorded as the follow-up.

**How: a small multi-value extension, not a template engine.** `template.render` replaces exactly one
fixed placeholder with a `Text`; it stays as it is (it renders report text, not files). The new pure tool
`repo.file.render` takes `values: list<TemplateValue>` (bound with decision (a)'s list syntax) and
positional placeholders `{{ 0 }}` … `{{ 15 }}`. It refuses, at plan time since it is pure: an index with no
value; a value no placeholder uses; any other `{{` in the template (so a stray or mistyped placeholder can
never reach a file; escaping is out of scope, and no Sample file needs a literal `{{`); a rendered file over
65,536 characters (checked by arithmetic before allocating, as `template.render` does since adversarial pass
2). Output `file: RepoFile`. Named placeholders would read better but need a record type in the port
system; the document states each node's value order in a comment beside it. A real engine (loops,
conditionals) is rejected: the DSL is deliberately non-Turing-complete, and logic inside privileged
templates is logic nobody type-checks.

### (e) A secret can never reach a committed file, and neither can a caller's command

**By construction, through types.** The only producer of `RepoFile` is `repo.file.render`; its inputs are a
`TemplateSource` (a document literal) and `TemplateValue`s. Both new types are public, and conversions are
secrecy-monotone and compile-time checked (milestone 3d), so no secret type can ever convert into
`TemplateValue`; a secret bound to a `TemplateValue` port or element is `SecretToNonSecretSink` at `check`.
The existing tests that prove the rule for a non-secret sink are
`secret_into_template_fails_check_with_exactly_one_taint_error` (`crates/willikins-dsl/tests/acceptance.rs`)
and `acceptance_1_taint_rejection_reports_exactly_the_secret_to_non_secret_sink`
(`crates/willikins-core/tests/check.rs`), with the `for_each` variants in
`crates/willikins-core/tests/check_adversarial.rs`. E2 adds `workflows/fixtures/secret-into-repo-file.yaml`
(a `doppler.secret.get` value as an element of `repo.file.render.values`: exactly one
`SecretToNonSecretSink` at `node.values[i]`) and a check that a `RepoFile` literal is refused.

**The `doppler.value.get` wall.** That tool returns *public* `Text` from Doppler ("wall two"), so a value
an author declared public could reach `template.render`. It cannot reach a committed file: there is **no
`Text => TemplateValue` row**, and a test pins that none may be registered. The only route into a file is a
reviewed conversion row from a grammar-constrained identifier type (this milestone:
`AppleBundleIdentifier` only). Secrecy inference (`todos/2026-09-22-secrecy-inference.md`) is where the
remaining `Text` question closes; file-writing does not depend on it.

**No caller-controlled command in an executed file.** The scaffold writes `upload-pipeline.sh`,
`bootstrap.yml` and `pipeline.yml`, which CI executes. `TemplateValue`'s grammar is exactly
`[A-Za-z0-9_][A-Za-z0-9._/-]*`, at most 255 characters: no whitespace, quote, `$`, backtick, `;`, `|`, `&`,
`<`, `>`, `{`, `}`, `*`, `?`, `[` or newline, and no leading `-`. A substituted value therefore cannot end
a quoted string, start a command or a flag, or inject a placeholder. Every command in a committed file is
document-literal — exactly the route milestone 3a decision (a) reserved: "A workflow that wants different CI
behaviour changes the repository's own `.buildkite/pipeline.yml`, which the template half of milestone 3
renders." `check` refuses `TemplateSource` and `RepoFile` as workflow input types and as input defaults (by
the registry entry's `TypeId`, never by name, the 3d rule), and a literal bound to a `RepoFile` port. No
shipped document declares either, so nothing moves.

### (f) Direct commit or branch plus pull request: a document choice; direct recommended — **OPEN, operator**

Policy lives in the workflow. The tool writes to whatever `branch` the document names and never creates
one, so both routes are documents:

- **Direct**: `github.scaffold.ensure { branch: main }`. Matches the monorepo's own owner ruling
  (trunk-based, "commit directly to `main`", "Do not create feature branches") and its history (494 of 494
  commits since 2026-09-01 direct). The operator's plan approval is the diff review (decision (l)); CI
  runs on the push.
- **Pull request**: `github.branch.ensure` (create a ref from the base head if absent, never move one),
  `github.scaffold.ensure { branch: <that branch> }`, `github.pull_request.ensure { head, base }`. Needs
  Pull requests: write, leaves an unmerged branch the monorepo's ruling forbids, and complicates
  idempotence: after the merge the branch is deleted, so the scaffold's read must also look for the marker
  on the base. Tasks P1–P3, added only if chosen.

**Recommendation: direct**, `branch: main`. Implementation does not wait: E1–B1 and L1 are identical under
both; only W1's literal and the optional P tasks depend on the answer.

### (g) Paths and branches are types

- `RepoPath`: `/`-separated segments, each `[A-Za-z0-9._@+-]+` and neither `.` nor `..`; no leading or
  trailing `/`, no empty segment, at most 32 segments and 1,024 characters. Refused: any segment `.git`
  (any case), and any path whose first two segments are `.github/workflows` (any case). So the write token
  never needs the Workflows permission, and GitHub Actions workflow files — executed on push, with no stored
  configuration to compare — stay out of reach for the same reason the Buildkite configuration does.
- `GitBranchName`: `[A-Za-z0-9._/-]+`, at most 100 characters, a subset of `git check-ref-format`: no
  `..`, no `//`, no leading `/`, `-` or `.`, no trailing `/`, `.` or `.lock`, no `@{`.
- `CommitHeadline`: one line, 1–72 characters, no control character.
- `RepoFile`: canonical form `<RepoPath>\n<content>`; content is at most 65,536 characters (`Text`'s
  bound) and contains no NUL. Rendering a `RepoFile` prints the whole file, which is what an approver
  should see in the plan's JSON; it is public by type.

### (h) M7: the stored bootstrap becomes an observed gate; writing it stays out

**Writing it is not automatable without breaking 3a decision (a).** A stored bootstrap is YAML whose steps
carry commands; the tool has no configuration port by design, and the reasoning stands unchanged: a
pipeline configuration "becomes something an agent machine executes … the moment a build is triggered,
with no diff in between", and CLAUDE.md forbids a tool that takes a shell command as input. A second
frozen form parameterised by typed values (queue, selector path, credential helper path) was considered in
3e decision (f) and rejected there: the helper path is a program `git` runs.

**What becomes automatable is the check.** The operator's own cookbook step is "Read back stored
configuration and compare it with the checked-in copy before dispatching". `buildkite.pipeline.bootstrap.gate`
(SHARED VALUES) reads the pipeline and compares its stored `configuration` with the bootstrap `RepoFile`
the document renders: equal → `Present` (`Compute`); pipeline absent, or configuration different →
`Absent` (`Blocked`), need "the sample pipeline's stored bootstrap equals the bootstrap this document
commits", how "In the pipeline's Settings → Steps, replace the YAML with apps/sample/.buildkite/bootstrap.yml
from the monorepo, save, then re-run this document". The paste stays manual; the `m7_bootstrap_done`
acknowledgement input goes away.

- **Compared structurally**: both strings parsed as YAML into a JSON value and compared for equality,
  because Buildkite's own documentation shows a stored configuration coming back re-quoted. A stored
  configuration that does not parse is "different".
- **Against the document's rendering, not the repository's current file.** The bootstrap is Sample's CI
  policy, and policy lives in the document; if the developer changes `bootstrap.yml` and reseeds Buildkite,
  the gate says so and the template is updated in the document. The alternative — read the file at the
  head of `main` — needs a GitHub content read into the graph and on the first run has no file to read.
  Recorded as the operator's to prefer.
- **First run**: the pipeline does not exist at plan time (or holds the frozen bootstrap) → `Blocked`,
  which is true: the paste is still owed. It is a leaf (its `slug` output is bound by nothing).
- **Trust boundary 7 of milestone 3a widens, narrowly.** `PipelineBody` keeps its six fields and
  `buildkite.pipeline.ensure` still never deserialises `configuration`. The gate uses its own
  `PipelineConfigurationBody { configuration: Option<String> }`, compares, and drops it: the value never
  reaches an output, an error, the journal, `tracing` or `Debug` (a stored bootstrap may carry an operator's
  `env`). Tests seed a secret-shaped value into the mocked configuration and assert it appears nowhere.
- **Conditional on verify item 9.** If Buildkite's REST `GET` does not return `configuration` for a YAML
  pipeline, B1 stops after its sandbox probe, M7 stays an acknowledgement, and the attacker records the
  finding (Buildkite's GraphQL `pipeline.steps.yaml` is the fallback to research, not to build here).

### (i) Sample's files

Seventeen seeded files plus the marker, all under `apps/sample/`; value order `0` = app, `1` = NSE,
`2` = widgets identifier, each through `AppleBundleIdentifier => TemplateValue`:

| Path | Values | Content, from the survey's pattern |
| --- | --- | --- |
| `BUILD.bazel` | — | the two-line name reservation every sibling carries |
| `ios/BUILD.bazel` | 0, 1, 2 | `config_setting`s scoped to `//apps/sample:__subpackages__`; `apple_bundle_version`; `local_provisioning_profile` local wildcard plus three distribution profiles named exactly `{{ 0 }}`, `{{ 1 }}`, `{{ 2 }}` (no `team_id`, decision (j)); `swift_library` per target; `ios_extension` `SampleNotificationService` and `SampleWidgets`; `ios_application` `Sample` with `extensions` both; entitlements and profiles by `select()`, `None` for `ci_build` and simulator builds; `minimum_os_version` and `families` as AppTwo's |
| `ios/Sample/Sources/SampleApp.swift` | — | a SwiftUI `@main` app with one placeholder view |
| `ios/SampleNotificationService/Sources/NotificationService.swift` | — | a `UNNotificationServiceExtension` that delivers the content unchanged |
| `ios/SampleWidgets/Sources/SampleWidgets.swift` | — | a `@main` `WidgetBundle` with one static placeholder widget |
| `ios/Resources/Info.plist` | — | `NSHealthShareUsageDescription`, `NSHealthUpdateUsageDescription` (App Store requirement, 3e pre-flight) |
| `ios/Resources/SampleNotificationService-Info.plist` | — | `NSExtensionPointIdentifier` `com.apple.usernotifications.service`, principal class |
| `ios/Resources/SampleWidgets-Info.plist` | — | `NSExtensionPointIdentifier` `com.apple.widgetkit-extension` |
| `ios/Resources/Sample.entitlements` | 0 | `com.apple.developer.healthkit`; `aps-environment` `production`; `com.apple.security.application-groups` `group.{{ 0 }}`; `com.apple.developer.default-data-protection` `NSFileProtectionCompleteUntilFirstUserAuthentication` (3e decision (e)); `com.apple.developer.devicecheck.appattest-environment` `production` (the App Attest gate, host only) |
| `ios/Resources/SampleNotificationService.entitlements` | 0 | app group `group.{{ 0 }}` |
| `ios/Resources/SampleWidgets.entitlements` | 0 | app group `group.{{ 0 }}` |
| `ios/Resources/PrivacyInfo.xcprivacy` | — | privacy manifest, tracking false, health data declared (App Review 5.1.3) |
| `.buildkite/pipeline.yml` | — | one credential-free validation step shaped as AppTwo's (queue, `vm-ci-plugin` plugin pin and image copied from it), building `//apps/sample/...` under `--config=ci` |
| `.buildkite/upload-pipeline.sh` | — | `exec buildkite-agent pipeline upload --no-interpolation apps/sample/.buildkite/pipeline.yml` |
| `.buildkite/bootstrap.yml` | — | AppTwo's shape with key `sample-bootstrap`, queue `ci-macos-apple-silicon`, `bash apps/sample/.buildkite/upload-pipeline.sh`; the same `GIT_CONFIG_*` host override (**operator item**: it would put that host path into willikins' public repository; the alternative is omitting it if 3e verify item 10 answers that the helper works without it) |
| `.buildkite/provider-settings.json` | — | AppTwo's: every provider trigger `false` |
| `.buildkite/README.md` | — | what the files are, triggers disabled, how to reseed the stored bootstrap |

`data_protection` becomes a **literal** on the `data_protection` node (the operator's "Just update the doc"),
and a Sample document test pins that the literal and the entitlement's value name the same class, so the
two can no longer drift through a caller's input. `healthkit.access` and `healthkit.background-delivery`
are omitted (3e rows 4 and 5: only if used).

### (j) The scaffold must build on `main`, and carries no team id

`ci.yml` builds `//...` on every push to `main`, so W1's rendered set is **snapshot-tested** (`insta`, one
file per snapshot) — the reviewable artefact — and verify item 8 is building that rendered set under
`bazel build --config=ci //apps/sample/...` on a Mac before the real apply (this workspace may not write
into the monorepo, and the host cannot build it). The monorepo's `local_provisioning_profile` rules carry a
`team_id`; 3e trust boundary 7 keeps the Apple team id out of this workspace, and `rules_apple` documents
`team_id` as a disambiguator only when profiles of the same name exist on different teams (verify item 10).
The template omits it. If the operator wants it, the honest source is App Store Connect's `seedId`
attribute on the bundle identifier (a new output and type), not a literal.

### (k) Composition-ready (milestone 2b)

Every scaffold input is a value an ios-app document can receive: `repo: GitHubRepo` and the branch from
the organisation document, the identifiers as its own inputs, the token from its own Doppler chain
(recommended for 2b: pass a public `DopplerConfig` across `uses:` and resolve the credential inside, rather
than a secret output crossing a document boundary). Templates are inline, so they move with the ios-app
document. The marker path derives from the app directory. Nothing in this milestone assumes one document.

### (l) Credentials: a new, narrow write token, from Doppler

The operator's rule: every provider credential is resolved from Doppler through the document; only the
Doppler token is outside it. Sample's GitHub chain today reads `GH_CLONE_TOKEN` from
`github/example-org` (a fine-grained token of unknown scopes). **Do not widen it**: `github/example-org`
is one of Sample's `base_configs`, inherited by `sample/prd_deployment_ios` and by every other app's
deployment config, so anything in it reaches CI jobs; giving that token Contents write hands every such job
write access to the monorepo. The write token is new:

- **Fine-grained personal access token**, resource owner `Example-Org`, repository access **only**
  `monorepo`, permissions **Contents: Read and write** (Metadata: Read is automatic). No Workflows (paths
  refuse `.github/workflows/`), no Administration, no Pull requests unless decision (f) goes PR (then Pull
  requests: Read and write). An expiry the operator chooses. It authenticates as the operator, so commits
  are authored by them and signed by GitHub.
- **Stored** in the real workplace at project `github`, a branch config **no app config inherits**:
  recommended `example-org_willikins` under environment `example-org`, not marked inheritable, secret
  `GH_CONTENTS_WRITE_TOKEN`; the willikins Doppler service account needs read on it.
- **Resolved in the document** exactly as R4 did: `gh_write_token_secret` (`doppler.secret.get`) →
  `gh_write_token` (`github.token.parse`) → `sample_files.token`. `monorepo_ref` keeps the clone token.
- **Sandbox, for tests**: the `Willikins-Test` PAT already in `~/.config/willikins/sandbox.env` can create
  and delete repositories there (milestone 2); the live cycle uses it as the tool's default credential and
  once through the `token` port.

## The Sample document, after this milestone

Changes to `workflows/sample-ios-app.yaml`, edited in place (task W1), in one contiguous, commented block so
2b can lift it:

- **Removed:** inputs `m3_repo_files_done`, `m7_bootstrap_done`, `data_protection`; nodes `m3_repo_files`,
  `m7_bootstrap`.
- **Added:** `gh_write_token_secret`, `gh_write_token`; one `repo.file.render` node per file of decision (i)
  (`values` bound as a list of `${{ inputs.*_identifier }}` references); `sample_files`
  (`github.scaffold.ensure`, `repo: ${{ steps.monorepo_ref.repo }}`, `branch: main` pending decision (f),
  `marker: apps/sample/.willikins-scaffold`, `files: [ …all render outputs… ]`, the headline, `token:
  ${{ steps.gh_write_token.value }}`); `bootstrap_gate` (`buildkite.pipeline.bootstrap.gate`, `slug:
  ${{ steps.pipeline.slug }}`, `expected: ${{ steps.bootstrap_yml.file }}`, `token: ${{ steps.bk_token.value }}`).
- **Rebound:** `pipeline.repo: ${{ steps.sample_files.repo }}`, so the pipeline is created only after its
  `.buildkite/` files exist (the survey's rank 1 complaint: "The graph today provisions a pipeline that
  cannot run"). On a first run `sample_files` plans `Create` with `repo` predicted, so the pipeline still
  plans.
- **The header comment** gains the M3/M7 story and loses the acknowledgement lines. The remaining manual
  steps: M1 app record (gate), M2 app groups (gates), M2b App Attest (gate), M5 APNs key and M6 CI Doppler
  grant (acknowledgements), M7 (observed gate; the paste is manual).

## Acceptance tests

1. **List binding, parse and check** (E1): a sequence of references and literals binds a `list<T>` port;
   a nested sequence, a mapping element, a list-typed element, a sequence on a scalar port and a sequence
   under `outputs:`/`for_each:` each fail with the named error; a secret element into a public list is
   exactly one `SecretToNonSecretSink` at `node.port[i]`; a conversion is recorded per element; an input
   used only inside a list raises no unused-input warning; every existing characterization entry is
   byte-identical.
2. **List binding, plan and apply** (E1): known elements give a known list in order; one `Unknown` element
   makes the value `Unknown`; a node whose list element names a blocked gate plans `Skip` and its `read` is
   never called (a panicking in-test tool proves it); `apply` resolves the same list.
3. **Types** (E2): each new type's accepts and refusals — `RepoPath` refuses `..`, `.`, empty segment,
   leading `/`, `.git/x`, `a/.GIT/b`, `.github/workflows/ci.yml`, `.GitHub/Workflows/x`, a backslash, a
   control character; `GitBranchName` refuses `a..b`, `-x`, `x.lock`, `a//b`, `@{`; `TemplateValue`
   refuses every metacharacter listed in decision (e) and a leading `-`; `RepoFile` round-trips and refuses
   a NUL and an over-long content. Proptest: every `AppleBundleIdentifier` parses as a `TemplateValue`.
   A test asserts no conversion row has source `Text` and target `TemplateValue`.
4. **Check refusals** (E2): negative fixtures for a `TemplateSource` input, a `RepoFile` input, a default of
   each, and a literal on a `RepoFile` port, each exactly one error; `secret-into-repo-file.yaml` exactly
   one `SecretToNonSecretSink`.
5. **`repo.file.render`** (T1): substitutes every occurrence of each index; refuses an index without a
   value, an unused value, a stray `{{`, `{{0}}` without spaces, index 16, and an amplified render before
   allocating; no error message echoes template or value text; pure; both catalogs validate;
   `LIVE_TOOL_NAMES` pinned.
6. **Client reads** (G1, mocks with exact paths and queries pinned, the 3e lesson): ref → commit → the
   non-recursive trees along declared paths only, each directory fetched once; blob sha computed locally
   matches git's for an empty file, an ASCII file and a UTF-8 file (known vectors); a symlink, a tree and a
   submodule at a path are reported as non-blobs; only the marker blob is ever fetched.
7. **Client write** (G1): the GraphQL request body is pinned by a JSON matcher (the fixed mutation text,
   `expectedHeadOid`, base64 additions sorted by path); 200 with `data` → the oid; 200 with `errors`, 200
   without `data`, 401, 403 and 502 → failures whose messages contain no body text (a seeded marker in the
   mocked body never appears); never retried by the client.
8. **`github.scaffold.ensure`** (G2), each row of decision (b)'s table against mocks: `Present` issues no
   tree walk past the marker; `Foreign` is `NameTaken`; `Absent` predicts the three outputs; the refusal
   names every differing path and never content; `Invalid` cases make no request; `ensure` commits only the
   non-equal files plus the marker, whose content is exactly the SHARED VALUES format; a failed commit
   whose re-read is `Present` is `changed: false`; a moved head retries against the new head at most three
   times; a same-head failure is `Provider`; outputs never contain a sha; the fake twin agrees
   (`fake_agrees_with_live`); `LIVE_TOOL_NAMES` pinned.
9. **The bootstrap gate** (B1): equal (including a re-quoted but structurally equal stored string) →
   `Compute`; different, unparsable, missing `configuration`, or pipeline `404` → `Blocked` with the static
   need and how and subject `org`, `slug`; a seeded secret-shaped string in the mocked configuration
   appears in no output, error, `Debug`, `BlockedGate` or journal line; `PipelineBody` still has exactly six
   fields; only `GET` is ever recorded; fake twin agrees.
10. **The Sample document** (W1, `crates/willikins-cli/tests/sample_document.rs`): checks clean against the
    fake catalog; no `operator.acknowledge` node remains for M3 or M7; `sample_files` binds all seventeen
    renders; a first fake run creates the scaffold and blocks on the bootstrap gate (plus the existing
    gates); a second run with the marker present and every seeded file edited in the fake state plans
    `sample_files` `NoOp`; a pre-landing fake state with a differing `apps/sample/ios/BUILD.bazel` fails
    plan naming that path; `pipeline` is ordered after `sample_files`; the `data_protection` literal and
    the entitlement agree.
11. **Rendered files snapshot** (W1): one `insta` snapshot per rendered file for the real identifiers
    `com.example-org.sample`, `.nse`, `.widgets` — the artefact the operator reviews and builds (verify
    item 8).
12. **Guards**: `secret_literal_guard`, `no_gh_writes_guard`, `no_certificate_writes_guard` green; the
    rendered snapshot and every new fixture contain no Apple team id and no token-shaped literal.
13. **The live scaffold cycle** (L1, run once by the attacker), below.
14. **Characterization**: every existing entry byte-identical except `workflows/sample-ios-app.yaml`'s own,
    which changes by the operator's 2026-09-30 decision to update the document; the new fixtures add
    entries.

## The live scaffold cycle (written by L1, run once by the attacker)

`crates/willikins-providers-github/tests/live_scaffold_cycle.rs`, sandbox only:

1. Count `Willikins-Test` repositories (read-only); refuse to start if any `willikins-files-*` exists.
2. Create `Willikins-Test/willikins-files-<unix-seconds>`, private, with `auto_init: true` (a raw `POST`
   in the harness, as the guard's `DELETE` is raw: the git database API answers `409` on an empty
   repository). Record the name in the drop guard **before any assertion**.
3. `github.scaffold.ensure` with three rendered files (one in a nested directory) and marker
   `app/.willikins-scaffold`: `read` is `Absent`; `ensure` is `changed: true`; an independent `GET` shows
   exactly one new commit whose parent is the init commit, whose `verification.verified` is recorded
   (settles verify item 2), and whose tree holds every path as `100644` with the expected blob sha plus the
   marker.
4. `read` is `Present`; a second `ensure` is `changed: false`; the head is unchanged.
5. The harness edits one seeded file with a raw `PUT …/contents/{path}` (a developer's edit): `read` stays
   `Present`, `ensure` is `changed: false`, the edit survives (its blob sha unchanged afterwards).
6. A second scaffold (marker `other/.willikins-scaffold`) whose file list includes the edited path with
   the original content: `read` refuses naming that path; the head is unchanged.
7. A client-level `createCommitOnBranch` with a deliberately stale `expectedHeadOid` fails and the head is
   unchanged (settles verify item 4; the shape is recorded by key names only).
8. Once through the `token` port with the same sandbox PAT resolved in-process, one read.
9. Guard: `DELETE` the repository, a following `GET` is `404`, the repository count equals step 1.

No credential, token-shaped string or response body is printed; the harness greps its own output at the end
as the 3c harnesses do. B1's own sandbox probe (verify item 9) is separate: create a throwaway pipeline in
`willikins-test` through `buildkite.pipeline.ensure`, read it with the gate (`Blocked`), set its
configuration with a raw harness `PATCH` to the expected bootstrap, read again (`Compute`), delete the
pipeline; counts equal.

## Credentials

**Needed now (L1 and B1's probe):** present — the sandbox GitHub PAT for `Willikins-Test` (can create and
delete repositories, milestone 2) and the renewed sandbox Buildkite token (HANDOFF: "far broader than
needed"). Nothing else.

**For the real apply (the operator's):** the new write token of decision (l), stored where decision (l)
says, readable by the willikins Doppler service account; everything Sample already needed.

## Operator decisions pending

1. **Direct commit to `main` or branch plus pull request** on `Example-Org/monorepo`. Recommended:
   direct (decision (f)). If PR: tasks P1–P3 are added and W1 binds the branch they create.
2. **The write token** (decision (l)): create it, store it, grant the service account read.
3. Two smaller items, not blocking any task: whether Sample's `bootstrap.yml` copies AppTwo's host
   credential-helper override into willikins' public repository (decision (i)); and whether the M7 gate
   should compare against the repository's current file instead of the document's rendering (decision (h)).

## Verify before relying on them

1. **`createCommitOnBranch` accepts a fine-grained token with Contents: Read and write** and no other
   permission (L1 uses a classic sandbox token if that is what `sandbox.env` holds; the real token settles
   it on its first plan's read and apply).
2. **The commit is signed and verified** for a personal-token author (schema: "automatically signed by
   GitHub if supported"). L1 records `verification.verified`.
3. **Which permission GraphQL checks for `createCommitOnBranch`** — assumed Contents write; not stated in the
   permissions data, which covers REST only.
4. **The shape of a stale-`expectedHeadOid` failure** (HTTP status, `errors[].type`). The tool never
   depends on it (it re-reads), but the harness records it.
5. **Request size limits** for `createCommitOnBranch` (Sample's set is about 20 KB of base64; unknown
   ceiling). Recorded by L1 as observed-fine at that size.
6. **The rules in force on `Example-Org/monorepo` `main`**: a read-only
   `GET /repos/Example-Org/monorepo/rules/branches/main` (Metadata read) and the classic protection state,
   before the real apply — whether a pull request is required, whether updates are restricted, whether the
   operator's token-authored commit bypasses as an administrator.
7. **The write token authenticates and sees the repository** (a read-only `plan --live` of Sample after it
   is stored).
8. **The rendered scaffold builds**: copy W1's snapshot into a scratch clone of the monorepo and run
   `bazel build --config=ci //apps/sample/...` on a Mac with Xcode, before the real apply, because `ci.yml`
   builds `//...` on the push.
9. **Buildkite's `GET` pipeline returns `configuration` for a YAML pipeline**, and in what form. The docs'
   GET example omits it; the create response shows it re-quoted. B1's sandbox probe settles it before B1's
   tool is written.
10. **`rules_apple` 4.3.3's `local_provisioning_profile` accepts no `team_id`** and resolves a uniquely named
    profile (decision (j)).
11. **Doppler accepts the branch config name `example-org_willikins`** (hyphenated environment,
    underscore branch separator); R1 settled hyphens in root and branch names in the sandbox.

## Gates

Scoped, per the host rules: `pgrep -x cargo` and `pgrep -f cargo-sweep` print nothing before every cargo
command; `-j 2`, `RUST_TEST_THREADS=2`; in the background with a 600,000 ms timeout; read the log body;
never pipe through `tail` or `tee`; never edit tracked files while cargo builds.

```
cargo fmt --all --check
cargo clippy -p <touched crate> --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test -p <touched crate> -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
```

plus `RUST_TEST_THREADS=2 cargo test -p willikins-dsl --test acceptance -j 2` for the characterization
snapshot. The full workspace gate is the coordinator's. A linker "missing .rcgu.o" or `E0463` is the host
sweep: `cargo clean -p <crate>` and rebuild.

## Tasks

One lane at a time on `main` (one cargo at a time on this host), in order; each commits by path with
`git commit --only`, test first, one behaviour per commit.

| # | Task | Delegate to |
| --- | --- | --- |
| E1 | **List binding** (decision (a); acceptance 1, 2, 14). Commit 1, `willikins-dsl` + `willikins-core` check: parse, per-element typing, conversion and taint, edges, consumed inputs, the new site form. Commit 2, `willikins-core` plan/apply: resolution, `Unknown` propagation, list elements in the skip scan | sonnet implements, opus attacks |
| E2 | **Types and refusals** (decisions (e), (g); acceptance 3, 4). Commit 1, `willikins-types`: `RepoPath`, `GitBranchName`, `CommitHeadline`, `TemplateValue`, `RepoFile`, the conversion row with its `From` impl and proptest, the no-`Text`-row test. Commit 2, `willikins-core`: the four check refusals by `TypeId`, their negative fixtures, `secret-into-repo-file.yaml` | sonnet implements, opus attacks |
| T1 | **`repo.file.render`** (decision (d); acceptance 5). One commit, `willikins-tools` + catalog pins | sonnet implements, opus attacks |
| G1 | **GitHub client** (decision (b); acceptance 6, 7). Commit 1: the pinned reads and local blob sha. Commit 2: the GraphQL transport and `createCommitOnBranch`, body never echoed | sonnet implements, opus attacks |
| G2 | **`github.scaffold.ensure`** (decisions (b), (c); acceptance 8). Commit 1: the tool and mocks. Commit 2: the fake twin, parity, catalog registration, `LIVE_TOOL_NAMES` | sonnet implements, opus attacks |
| B1 | **Bootstrap gate** (decision (h); acceptance 9). Step 0, before any code: the sandbox probe of verify item 9 (read-only except its own throwaway pipeline). Then commit 1: the tool, its compare-only body, mocks. Commit 2: fake twin, parity, registration | sonnet implements, opus attacks and runs the probe |
| L1 | **Live scaffold cycle** (acceptance 13), written, not run | sonnet writes, opus runs once |
| W1 | **The Sample document** (decisions (i), (j), (l); acceptance 10, 11, 12, 14). Commit 1: the templates, render nodes, `sample_files`, the rebinding, the removed inputs and nodes, the header. Commit 2: `sample_document.rs` tests, the rendered snapshots, fake-state fixtures under `workflows/fixtures/state/` | sonnet implements, opus attacks |
| P1–P3 | **Only if decision (f) goes PR**: `github.branch.ensure`, `github.pull_request.ensure`, and the scaffold's read of the base branch's marker | sonnet implements, opus attacks |

Then the attack: every piece gets an independent opus pass by an agent that did not write it, with at least
four mutations each restored from saved copies (`cmp` confirming byte-identity), recorded under
`docs/research/2026-09-30-m3g-adversarial-pass*.md`; the attacker runs L1 and B1's probe once each.
Priority targets: a secret or a `Text` reaching a `RepoFile` by any route (list elements, conversions,
`for_each`, defaults); a `TemplateValue` breaking out of a quoted string in any of the four file syntaxes;
the scaffold overwriting anything; a moved head producing a duplicate or a lost commit; the configuration
leaking from the gate; a plan-to-apply drift from a busy `main`.

## Risks

1. **The monorepo refuses a token commit to `main`** (a ruleset the operator bypasses as an admin, but a
   token does not). Verify item 6 finds it before the real apply; the answer is the PR route (P1–P3), not a
   broader token.
2. **`main` goes red** if the scaffold does not build. Verify item 8 before the real apply; the fix is a
   template edit in the document.
3. **Buildkite does not return `configuration`.** B1 stops after its probe, M7 stays an acknowledgement
   (decision (h)).
4. **Positional placeholders are misnumbered.** Refused at plan time when an index or a value is unused;
   a swapped pair of identifiers is caught only by the rendered snapshot review (acceptance 11).
5. **The engine change (E1) is the widest piece.** Its attack targets the skip scan and taint per element;
   every existing characterization entry must stay byte-identical.
6. **Build time and disk.** Seven crates touched across the lanes, one at a time; never gate across the
   04:00 sweep.
