# Milestone 3h: Sample's release pipeline, and the CI Doppler grant as something the document does

**Created:** 2026-10-01
**Gate:** OPEN. Nothing here needs the operator before implementation starts. Three operator items block only
the real apply (see "Needs the operator"), and one literal (the CI service account's name) is the coordinator's.
**Design:** `docs/plans/2026-09-11-willikins-design.md` (Templates; Trust model: "Workflow definitions and
templates are privileged content"; the 2026-09-21 addendum "Credentials are ports, resolvers are nodes";
"Policy lives in the workflow, never in the tool").
**Depends on:** milestone 3g (`docs/plans/2026-09-30-milestone-3g-file-writing.md`): list bindings, `repo.file.render`,
`github.scaffold.ensure` (a seed, written once, never overwritten), `buildkite.pipeline.bootstrap.gate`, and Sample's
seventeen templates. Milestone 3e's replace-when-INVALID (`Tool::replaces`, `Action::Replace`), whose shape decision
(b) reuses.
**Reference:** the monorepo's own cookbook,
`/Users/operator/Projects/example-org/docs/solutions/ci-cd-patterns/2026-09-10-buildkite-self-hosted-ios-cicd-cookbook.md`
(cited below as "cookbook §N"), and the two apps that already ship through it, `apps/app-three` and
`apps/app-two` (read-only, `main` at `9ac10a8b`).
**Addendum (2026-10-01, D1):** `DopplerProjectRole` is implemented exactly as SHARED VALUES specifies, with the
pattern spelled as the explicit group `(?:viewer|collaborator)`. But `willikins_derive::attrs::anchored_pattern`
already wraps *any* unanchored pattern (including a bare `viewer|collaborator`) in its own `(?:...)` before
anchoring it with `^...$` -- so the premise in SHARED VALUES' D1 row ("an unwrapped alternation under the derive's
`^…$` anchors would read as `^viewer` or `collaborator$` and admit `viewer '`) does not hold against the derive as
it exists today; a bare alternation would in fact anchor correctly either way. The explicit group is kept as a
second, visible guard rather than relying solely on the derive's own wrapping, and `"viewer "` is pinned by test
either way. No code or acceptance criterion changes; this is a note for the next reader (and the adversarial pass)
so the two don't have to reconcile it themselves.
**Addendum (2026-10-01, D2):** the task text asked for `pub(crate)` client methods
(`list_service_accounts`, `list_project_members`, `add_project_member`, `update_project_member`),
but D2's own mock tests (`tests/project_member_client_mock.rs`) must call them directly, since the
tool that will (task D3) does not exist yet in this task. A `tests/*.rs` target links this crate as
an external dependency and cannot see a `pub(crate)` item at all, so all four are `pub` instead,
each documented with the same visibility-override rationale
`willikins_providers_buildkite::BuildkiteClient::delete_pipeline` already uses for the identical
reason. `DopplerSlug` (the response-side identifier `add_project_member`/`update_project_member`
address a member by) and `ServiceAccountEntry`/`ProjectMemberEntry` (what the two list methods
return) are `pub` for the same reason, and are re-exported from `lib.rs`. `LIST_PER_PAGE` and `LIST_MAX_PAGES`
(the per-page size and page bound both list methods share) are `pub` and re-exported too, so the
two "refuses past the bound" tests pin `.expect(LIST_MAX_PAGES as usize)` and the product
`LIST_MAX_PAGES * LIST_PER_PAGE` by name rather than repeating the literals `50`/`5000`. No code or
acceptance criterion changes; `pub` costs nothing a `pub(crate)` method with a real second caller
(D3) would not also expose by then.
**Addendum (2026-10-01, E1):** the Tasks table's split ("Commit 1 ... Commit 2: ... regenerate ... both schema
snapshots") does not hold: `schema_generation__action_schema_generates.snap` and
`schema_generation__planned_node_schema_generates.snap` live in `crates/willikins-core/tests/`, alongside
`plan_schema_generates.snap`, and all three embed `Action`'s own schema -- adding the variant changes all three,
not one, and commit 1's own scoped gate (`cargo test -p willikins-core`) is red until they are regenerated, since
nothing in commit 2 (`willikins-cli`, `willikins-server`) touches them. Commit 1 therefore carries all three
`willikins-core` schema snapshots, each diff-reviewed to confirm the one new `"update"` enum entry is the only
change; commit 2 is unchanged (the `willikins-cli` `render.rs` arm plus the `willikins-server` MCP tool-list
snapshot). `cargo clippy -p willikins-core --all-targets` on commit 1 alone still compiles cleanly, because the one
exhaustive match on `Action` (`willikins-cli`'s `render.rs`) is outside this crate; it goes red only once commit 2
adds the arm, which is why commit 2 exists at all.
**Addendum (2026-10-01, D3):** two deviations from the task text, neither changing acceptance 4.
1. "Remove D2's dead_code allows" names nothing that exists: D2's own addendum already made the four client methods,
   `DopplerSlug`, and the two list-return types `pub` (not `pub(crate)`) specifically so no `#[allow(dead_code)]`
   would ever be needed once D3 existed, and a search of both crates for `dead_code` before this commit finds no
   hits. Nothing was removed because there was nothing to remove.
2. `ensure`'s `PATCH` union (decision (a): "`environments` = existing ∪ requested, sorted") assumed every existing
   environment string re-parses as an `EnvironmentSlug` on the way back out to `DopplerClient::update_project_member`,
   whose signature is typed `&[EnvironmentSlug]` (D2). D2's own `ProjectMemberEntry` doc already flags that a listed
   member's `environments` may hold a string outside this crate's grammar (too long, or shaped unlike a slug) --
   exactly the case the decision table's "extra environments ... are never a mismatch" row exists for. Silently
   dropping such an entry from the union to make it parse would be a narrowing `PATCH`, which trust boundary 2
   forbids outright. `DopplerProjectMemberEnsure::union_environments` instead refuses with `Conflict`, naming the
   one environment it cannot safely re-express, rather than sending a `PATCH` that would drop it. No acceptance-4 test
   exercises this path (none of the specified cases name a foreign existing environment on a `NeedsUpdate` row), so
   it is recorded here rather than claimed as tested; the live member cycle (D4) and a future adversarial pass are
   the next places this could be probed for real.

## Goal

The operator, 2026-10-01, verbatim: "Sample is meant to get its fucking pipeline. It should already have it. And I
have no fucking intention of doing it by fucking hand. And no, you shouldn't fucking remove m5. It's fucking blocking
manual fucking step." And: "all CI pipelines use the same Doppler service account, and CI agents have access to its
secret: DOPPLER_SERVICE_ACCOUNT_TOKEN".

Three things, exactly:

1. **(a)** A new tool, `doppler.project_member.ensure`, that makes a named service account a member of a Doppler
   project with a given role on given environments. It adds or raises access and never removes any.
2. **(b)** Sample's scaffold carries its full CI/CD pipeline: credential-free validation, a signing preflight, a
   signed and inspected package, an upload-only job, and the release graph that chains them. These are more seed
   templates under `apps/sample/`. They must be in the document **before** the real apply, because once the marker
   `apps/sample/.willikins-scaffold` lands, willikins never touches those files again (3g decision (c)). No real apply
   has happened yet.
3. **(c)** Sample's document: m6 ("the Buildkite CI Doppler service account has been granted read access to project
   sample") stops being an `operator.acknowledge` leaf and becomes a node built on (a). **m5 stays exactly as it is**:
   the APNs `.p8` key, which no Apple API can create or download, is still a blocking manual acknowledgement.

The milestone is done when tasks D1 through W7 are green under scoped gates, the coordinator has run the live member
cycle (D4) once in the **sandbox** workplace with every count equal before and after, verify items 1, 2, 4, 9 and 13
are settled, and each piece has had an independent attack. The real apply is not part of this milestone.

## Out of scope

- **Writing Buildkite's stored configuration or provider settings.** Milestone 3a decision (a) stands:
  `buildkite.pipeline.ensure` sends no `configuration` or `provider_settings`. M7 (pasting the stored bootstrap) stays
  an observed gate whose paste is manual (3g decision (h)). This plan changes the bootstrap's content, which only
  changes what the gate compares against.
- **Push-triggered validation.** The monorepo's `.github/workflows/ci.yml` already runs `bazel build --config=ci //...`
  on every push to `main`, Sample included. Buildkite's job here is release. A path filter like AppThree's
  `select_buildkite_validation.py` (477 lines) is a follow-up, if the operator wants one (decision (f)).
- **Edits outside `apps/sample/`** (3g's rank-2 rule): no row in the root `fastlane/Fastfile`'s AppThree-only `IOS_APPS`,
  no root `.gitignore` line, no `build/visibility` row.
- **Removing project members, lowering a role, revoking anything.** Decision (a).
- **Doppler workplace permissions for willikins' own service account.** That is the operator's grant (see "Needs the
  operator").
- **The executable bit.** The scaffold writes mode `100644` only (3g). Decision (e) shows the one hook does not need it.
- **SigNoz** (key expired 2026-09-23; nothing here calls it). **Railway** (no command).

## Trust boundaries (normative)

These extend 3g's eight and 3e's nine, which still hold.

1. **Live Doppler writes only in the sandbox workplace.** The harness refuses to start unless `GET /v3/me` names the
   workplace exactly `Willikins - Test - Sandbox` (the probe's own assertion). It writes only to throwaway projects
   and service accounts named `willikins-probe-delete-me-*`, which it creates and deletes in the same guarded run.
2. **The tool never removes access.** No `DELETE` on a member, no `PATCH` that lowers a role or drops an environment,
   and nothing that can express `admin` or `owner`. The grammar refuses those roles (decision (a)).
3. **No credential in a committed byte.** Every committed file is a document-literal `TemplateSource` plus
   `TemplateValue`s, which are the three bundle identifiers only. The pipeline names a Buildkite secret
   (`DOPPLER_SERVICE_ACCOUNT_TOKEN`) and Doppler secret **names**, never a value.
4. **Rendered CI code never invokes a shell on a value.** No `shell=True`, `os.system`, `os.popen` or
   `subprocess.getoutput` in any rendered Python, and no placeholder in any command, path or unquoted position
   (decision (h)). Configuration values reach subprocesses only as argv list elements.
5. **The guest never receives the Buildkite agent's credential**, and no `GIT_CONFIG_*` name or clone token appears in
   a `vm-ci-plugin` `env` allowlist (cookbook §2.2, §11).
6. **Validation is credential-free.** Only the signing preflight, package and upload steps name
   `doppler_token_secret`, and each stage takes only the Doppler names on its allowlist.
7. **No Apple team id and no host path in any template** (3g trust boundary 8). AppTwo's
   host-specific credential-helper path is not copied; checkout follows cookbook §3.3.
8. **Apple: read-only, except one upload.** The rendered scripts only `GET` App Store Connect (`/v1/profiles`,
   `/v1/apps`, `/v1/builds`). The single write is the upload job's `altool` upload. They never touch
   `/v1/certificates` and never create a profile (CI fetches profiles by name; 2b36422).

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| New tool (D3) | `doppler.project_member.ensure`: inputs `project: DopplerProject` (key, required), `service_account: DopplerServiceAccountName` (key, required), `role: DopplerProjectRole` (required), `environments: list<EnvironmentSlug>` (required, 1–16 distinct entries); outputs `project`, `service_account` (pass-through only); key `[project, service_account]`; `Class::Reversible`; not pure |
| New types (D1) | `DopplerServiceAccountName`: pattern `[A-Za-z0-9]+(?:[ ._-][A-Za-z0-9]+)*`, max 64, example `buildkite-ci`. `DopplerProjectRole`: pattern `(?:viewer\|collaborator)`, **grouped**, because an unwrapped alternation under the derive's `^…$` anchors would read as `^viewer` or `collaborator$` and admit `viewer `; example `viewer` |
| Role order (D3) | `no_access` < `viewer` < `collaborator`; every other identifier the API returns (`admin`, `owner`, any custom role) is "unrankable" |
| Engine change (E1) | `Action::Update` and `Tool::updates(&self, inputs) -> Result<bool, ToolError>`, default `Ok(false)`, consulted only for a non-pure, non-gate node whose `read` is `Absent` and whose `replaces` is `false` |
| Doppler endpoints (D2) | `GET /v3/workplace/service_accounts?page=N&per_page=P`; `GET /v3/projects/project/members?project=X&page=N&per_page=P`; `POST /v3/projects/project/members?project=X` body `{"type":"service_account","slug":S,"role":R,"environments":[...]}`; `PATCH /v3/projects/project/members/member/service_account/{slug}?project=X` body `{"role":R}` plus `"environments":[...]` only when the member does not already have all environments |
| `LIVE_TOOL_NAMES` | 37 → 38 (`crates/willikins-server/src/catalog.rs`), and every site 3g's T1/G2/B1 addenda list as pinning a count |
| Live cycle (D4) | `crates/willikins-providers-doppler/tests/live_project_member_cycle.rs`, its own `[[test]]` with `required-features = ["live-tests"]`, `#[ignore]`, `WILLIKINS_LIVE_TESTS=1`; prefix `willikins-probe-delete-me` |
| Sample m6 node (W7) | `ci_doppler_access`: `doppler.project_member.ensure`, `project: ${{ steps.doppler.project }}`, `service_account: REPLACE-WITH-CI-SERVICE-ACCOUNT` (placeholder literal; the coordinator sets the real name), `role: viewer`, `environments: [prd]` |
| Buildkite secret | `DOPPLER_SERVICE_ACCOUNT_TOKEN`, through `vm-ci-plugin`'s `doppler_token_secret` (exported in the guest as `DOPPLER_TOKEN`; cookbook §3.1) |
| Sample's Doppler source | project `sample`, config `prd_deployment_ios` (inherits `appstore-connect/deploy_ios` and `github/example-org`) |
| Doppler names per stage | signing-preflight and package: `APP_STORE_CONNECT_API_KEY_ISSUER_ID`, `APP_STORE_CONNECT_API_KEY_ID`, `APP_STORE_CONNECT_API_KEY_BASE64`, `APPLE_DISTRIBUTION_CERTIFICATE_P12_BASE64`, `APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD`; upload: the three `APP_STORE_CONNECT_API_KEY_*` only; host checkout: `GH_CLONE_TOKEN` (via the git helper, never a script) |
| Pipeline constants (copied from W1's `pipeline.yml`, which copied AppTwo) | queue `ci-macos-apple-silicon`; concurrency group `ci-host/vm-ci-plugin-macos`, `concurrency: 1`; plugin `github.com/Lightless-Labs/vm-ci-plugin#11fc336384a3c2a88209dfba3c429bbf61c1dac7`; image `ci-macos-rust-bazel-ios-20260910-v2`; guest `env` allowlist exactly `BUILDKITE_COMMIT`, `BUILDKITE_JOB_ID`, `BUILDKITE_BUILD_ID`, `BUILDKITE_BUILD_NUMBER`, `BUILDKITE_PIPELINE_SLUG`, `BUILDKITE_BUILD_URL`, `CI` |
| Guest toolchain (cookbook §4.1) | `DEVELOPER_DIR=/Applications/Xcode_26.3.app/Contents/Developer`; `PATH=/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`; `LANG`/`LC_ALL` `en_US.UTF-8`; Python is the system `/usr/bin/python3` (3.9): no `match`, no runtime `X \| Y` unions, standard library only |
| Monorepo URLs | SSH (what `buildkite.pipeline.ensure` stores, 3a's frozen form): `git@github.com:Example-Org/monorepo.git`; HTTPS: `https://github.com/Example-Org/monorepo.git` |
| Step keys | `sample-bootstrap`, `sample-validation`, `sample-signing-preflight`, `sample-package`, `sample-upload` |
| Selector variable | `SAMPLE_CI_ACTION` ∈ {`validation` (default when unset), `signing-preflight`, `release`} |
| Artifacts root | `apps/sample/.ci-artifacts/` (ignored by a scaffolded `apps/sample/.gitignore`), subdirectories `validation/`, `signing-preflight/`, `package/`, `upload-input/`, `upload/` |
| Package outputs | `apps/sample/.ci-artifacts/package/Sample.ipa`, `.../package/package-receipt.json`, `.../package/inspection-report.json` |
| Version policy | `tools/release-config.json`: marketing version `0.1.0`, `build_number_offset: 0`; build number = `BUILDKITE_BUILD_NUMBER + offset`; `--embed_label=<marketing>.<build>`, so `CFBundleVersion` is `<marketing>.<build>` (W1's `apple_bundle_version`) |
| Size budgets (W0 pins them) | the whole document ≤ 196,608 bytes (192 KiB, three quarters of `MAX_DOCUMENT_BYTES`); any one template ≤ 20,480 characters; `sample_files` ≤ 64 files (`github.scaffold.ensure`'s bound) |
| Characterization | every entry byte-identical **except** `workflows/sample-ios-app.yaml`'s own, which the operator's 2026-10-01 decision changes (as 3g's acceptance 14 did) |

## Facts settled before writing (sources)

- **Doppler project members** (`https://docs.doppler.com/reference/project_members-{list,get,add,update}.md`,
  fetched verbatim 2026-10-01): list `GET /v3/projects/project/members` (`project` required; `page`, `per_page`
  default 20); each member `{type, slug, role: {identifier}, access_all_environments, environments: [..]}`. Add `POST`,
  body `required: ["type", "slug"]`, `role` "Identifier of the project role", `environments` "Environment slugs to grant
  the member access to". Update `PATCH /v3/projects/project/members/member/{type}/{slug}` with `role` and
  `environments`. `type` ∈ `workplace_user | group | invite | service_account`.
- **Service accounts** (`.../service_accounts-list.md`): `GET /v3/workplace/service_accounts` (`page`, `per_page`
  default 20); each `{name, slug, created_at, workplace_role}`.
- **Role identifiers** (`https://docs.doppler.com/docs/project-permissions.md`): "three levels of project-based
  access: Viewer, Collaborator, Admin, and None", with None as slug `no_access`. Collaborators "can manage secrets,
  trusted IPs, and service tokens". Viewer is enough for CI to read.
- **The sandbox probe, 2026-10-01** (`doppler_creator_probe.py`): an account holding only `create_enclave_project`
  becomes admin of each project it creates but gets 403 on listing members and on adding a service account. With the
  read-only workplace permissions `team` and `service_accounts` added, `POST ... {"type":"service_account",
  "role":"viewer","environments":["prd"]}` answered 200 **immediately after the project was created**. A new project's
  default `prd` environment therefore already exists, so binding `project` to `steps.doppler.project` is a sufficient
  data edge. The member then listed `viewer`, `access_all_environments: false`, `environments: ["prd"]`.
- **Buildkite agent hooks** (agent `v3.123.0`, `internal/job/hook/type.go` and `wrapper.go`, fetched 2026-10-01): a
  hook with a POSIX-shell shebang (or none) is `TypeShell` and is **sourced** by a `0700` wrapper
  (`. "{{.PathToHook}}"` between two `env dump`s). A `100644` hook therefore runs. Two consequences: no `exec` and no
  `exit` on its success path, or the wrapper's after-dump never runs.
- **`vm-ci-plugin` at `11fc336`** (`hooks/pre-command`, local checkout `/Users/operator/Projects/lightless-labs/public/vm-ci-plugin`):
  the checkout is a live directory share (`tart run --dir checkout:$BUILDKITE_BUILD_CHECKOUT_PATH`), and the plugin
  schema has no artifact option. A later local plugin's `pre-command` that writes into the host checkout is visible to
  the guest command. That is how AppThree's `upload-input` works.
- **The pipeline's repository URL is SSH.** `buildkite.pipeline.ensure` stores `git@github.com:<owner>/<repo>.git`
  (`crates/willikins-providers-buildkite/src/client.rs`, `ssh_repository_url`). A `credential.<https url>.helper`
  never runs for an SSH clone, so §3.3's override needs one more entry, decision (d).
- **App Store validation** (cookbook §6.4) rejects a minimal `ios_application` with no icons, an incomplete
  `UISupportedInterfaceOrientations` for iPad, or an alpha-channel 1024 icon. Sample's W1 `Info.plist` and
  `BUILD.bazel` have no icon. Decision (g).

## Decisions

### (a) `doppler.project_member.ensure`: add or raise, never remove; admin is not expressible

**Ports** are in SHARED VALUES. `service_account` is a **name**, which is how the operator identifies the CI account
and what the coordinator will write. The tool resolves it to Doppler's slug itself, so the slug never appears in a
document, a plan or an output.

**Read**, in order:

1. Shape, before any request: `environments` is 1–16 entries with no duplicates, otherwise `Invalid`.
2. Resolve the name: page through `GET /v3/workplace/service_accounts` with `per_page=100` (verify item 7), stopping
   at a short page and refusing past 50 pages. Compare `name` byte for byte.
   - Zero matches: `ToolErrorKind::NotFound`, "no Doppler service account in this workplace is named `<name>`".
   - More than one: `ToolErrorKind::Conflict`, "`<n>` service accounts are named `<name>`; this tool will not guess
     between them; rename until the name is unique".
   - A `403` from this list: `Provider`, with a fixed message naming the missing workplace permission: "the willikins
     Doppler service account cannot list service accounts: its workplace role needs View Service Accounts
     (`service_accounts`)". The body is never echoed.
3. List the project's members, paged the same way. If `looks_like_a_missing_project` (the crate's existing 404 / 400
   predicate), the project does not exist yet: `Absent`, which plans `Create` on a first run. A `403` is `Provider`,
   "cannot list this project's members: the workplace role needs View Team (`team`) and the account must be admin of
   the project".
4. Find the entry with `type == "service_account"` and the resolved slug, then decide:

| Member state | Observation | Plan |
| --- | --- | --- |
| absent | `Absent` (predicted `project`, `service_account`) | `Create` |
| role == requested, and `access_all_environments` or `environments ⊇ requested` | `Present` | `NoOp` |
| role == requested, some requested environment missing, not `access_all_environments` | `Absent`, `updates() = true` | `Update` |
| role ranks below requested (`no_access` → `viewer`, `viewer` → `collaborator`) | `Absent`, `updates() = true` | `Update` |
| role ranks above requested (`collaborator` when `viewer` is asked) | `Mismatch { port: role }` | `PlanError::AttributeMismatch` |
| unrankable role (`admin`, `owner`, any custom role) | `Mismatch { port: role }` | `PlanError::AttributeMismatch` |

Extra environments the member already has are never a mismatch. They are left alone.

**Ensure** re-reads, then:

- `Present`: `changed: false`.
- Absent and not a member: `POST` with the requested role and environments.
- Absent and a member (the `Update` rows): `PATCH` with `role` = the requested role and `environments` = existing ∪
  requested, sorted. When the member already has `access_all_environments`, omit `environments`, so the `PATCH` cannot
  narrow access (verify item 3).
- `Mismatch`: `Conflict`, "this member holds a role this tool would have to lower or cannot express; it changes
  neither; change it by hand or ask for that role".
- Any write failure: re-read. `Present` gives `changed: false`. Anything else returns the original error. The client
  never retries (the crate's existing rule).

`updates()` runs the same read and returns `true` exactly for the two `Update` rows.

**Why the role is an enum of two.** Granting `admin` would hand another identity the project: it could manage
members, delete configs, and grant itself more. Granting `no_access` is meaningless as an ensure. `owner` is a
workplace role. So the type admits exactly `viewer | collaborator` and the grammar is the refusal, as with
`AppleProfileType`. Sample asks for `viewer`, the least that lets CI read.

**Why the name, and why exact.** A display-name match is how a human identifies the account, and an exact match with
a typed error on zero or several is the "one exact match, else a typed error" rule the task states. Whether Doppler
permits duplicate service-account names is not documented. The live cycle records it (verify item 6), and the
`Conflict` arm covers it either way.

**Class `Reversible`**, like every other Doppler ensure. A grant can be undone by hand, and nothing here deletes. The
plan line shows the node's `role` and `environments`, which is what an approver reads.

**Permissions this needs** (from the probe): willikins' own Doppler service account must be admin of the project
(true for any project it created, which includes `sample`; verify item 10) and its workplace role must hold `team`
and `service_accounts`, both read-only. The real account lacks the second pair today: an operator item.

### (b) `Action::Update`: the `Tool::replaces` shape, again

`Action` has `Compute | Create | Replace | NoOp | Blocked | Skip`, with no `Update`. Two options:

1. Plan an update as `Create`. That needs no engine change, but the approver sees "Create" for a member that already
   exists. Rejected: the plan is the approval (3g decision (f)).
2. **Mirror milestone 3e's replace-when-INVALID.** `Tool::updates(&self, inputs) -> Result<bool, ToolError>`,
   defaulting to `Ok(false)`. `plan_one` calls it only for a non-pure, non-gate node whose read is `Absent` and whose
   `replaces` is `false`, and plans `Action::Update` when it returns `true`. **Chosen.** It is the codebase's own
   answer to the same problem (`crates/willikins-core/src/tool.rs`, `Tool::replaces`'s doc explains why a hook beats a
   new `Observation` variant). A tool that never updates pays nothing, and every shipped document plans
   byte-identically.

`apply` needs **no new mid-run guard**. Its pre-run re-plan already reports a `Create`↔`Update` change as
`DriftKind::Action`, and an update never deletes anything, so the case `refuse_unplanned_replacement` exists for
(an unapproved destruction mid-run) has no counterpart here. `render.rs` prints `Update`. The published plan schema
gains one enum value: the two schema snapshots (`schema_generation__plan_schema_generates.snap`, the MCP tool-list
snapshot) change by that value only. No `Plan::updating` report is added: the node's line and inputs already say what
changes.

### (c) The release pipeline's shape: AppThree's stages, with less machinery

The monorepo's two apps that ship through Buildkite separate **validation** (no credentials), **package** (sign, build,
inspect, publish immutable bytes) and **upload** (a later job that sends exactly those bytes and never rebuilds), and
chain them in a `release.yml` (cookbook §1, §6, §9; AppThree's and AppTwo's `release.yml`). Sample follows that
shape. app-four signs and uploads in one job (cookbook §9.4), which needs no host hook. It was rejected because
an upload retry would rebuild and re-sign, and because the operator asked for the pipeline "the way the monorepo's
other apps have one".

Sample is smaller than its siblings in three places, each a deliberate cut:

1. **No cross-build release envelopes.** AppThree's upload can run in a later build than its package, so it
   authenticates predecessor jobs through the agent API (`prepare_buildkite_upload.py`, 628 lines). Sample's upload
   only ever runs inside the same `release.yml` build, `depends_on: sample-package`. The host hook checks that step's
   outcome is `passed`, downloads that step's artifacts from this build only, and checks the receipt's commit, size
   and SHA-256 against the bytes. The guest checks them again.
2. **A signing preflight that is a mode, not three pipelines.** AppThree has `signing-preflight.yml`,
   `signing-check.yml` and `signing-lifecycle.yml`. Sample has one `signing-preflight.yml`, which runs the same checks
   the package job runs first: Doppler names present, keychain lifecycle with an emptiness check, profiles fetched by
   name, identity∩profile intersection (cookbook §6.3), and entitlement coverage. No build.
3. **Python standard library, no Fastlane.** The root `fastlane/Fastfile` is AppThree-only (`IOS_APPS` holds one app),
   and a Sample row would edit a file outside `apps/sample/`. A Sample-local Fastfile would mean AppTwo's 591 lines
   of Ruby plus the root Gemfile chain, which cookbook §6.2 calls the source of "four of five failures" on Pessimal's
   first release. Cookbook §6.2 gives the direct route: an ES256 JWT signed with `openssl`, `GET /v1/profiles` by name,
   and `xcrun altool --upload-app` with the API key (verify item 5 names `iTMSTransporter` as the fallback).

### (d) Checkout: cookbook §3.3, plus one rewrite for the SSH URL

W1 left out AppTwo's override because it names a host path. Cookbook §3.3 settles the question: the clone token
is a Doppler-held fine-grained PAT, fetched on the host by a job-env credential helper scoped to this one repository
URL. Here that is `GH_CLONE_TOKEN`, inherited into `sample/prd_deployment_ios` from `github/example-org`, read with
the Buildkite cluster secret `DOPPLER_SERVICE_ACCOUNT_TOKEN`.

Sample's pipeline repository is the SSH URL (facts above), and a credential helper never runs for SSH. So the env
carries a third entry that rewrites exactly that URL to HTTPS, and git then selects the scoped helper. The entry uses
`url.<base>.insteadOf`, which git applies to fetch and clone URLs by prefix. The prefix is the full monorepo URL, so
`vm-ci-plugin`'s HTTPS plugin fetch and every other repository keep the host's own credential. The block, identical in
`bootstrap.yml` and in every uploaded pipeline file (cookbook: "Put it in both places"):

```json
"env": {
  "GIT_CONFIG_COUNT": "3",
  "GIT_CONFIG_KEY_0": "credential.https://github.com/Example-Org/monorepo.git.helper",
  "GIT_CONFIG_VALUE_0": "",
  "GIT_CONFIG_KEY_1": "credential.https://github.com/Example-Org/monorepo.git.helper",
  "GIT_CONFIG_VALUE_1": "!buildkite-agent secret get DOPPLER_SERVICE_ACCOUNT_TOKEN | sed 's/^/Authorization: Bearer /' | curl -fsS --max-time 20 -H @- 'https://api.doppler.com/v3/configs/config/secret?project=sample&config=prd_deployment_ios&name=GH_CLONE_TOKEN' | python3 -c 'import json, sys; print(\"username=x-access-token\"); print(\"password=\" + json.load(sys.stdin)[\"value\"][\"computed\"])'",
  "GIT_CONFIG_KEY_2": "url.https://github.com/Example-Org/monorepo.git.insteadOf",
  "GIT_CONFIG_VALUE_2": "git@github.com:Example-Org/monorepo.git"
}
```

These are cookbook §3.3's rules, unchanged. The empty reset comes first. Both helper keys are scoped to the
repository URL. The helper value contains no `$`, so it survives the stored step and the uploaded file alike. It is
JSON text, so the leading `!` cannot be read as a YAML tag (all of Sample's `.yml` files are JSON text, as W1's and
AppTwo's are). No `vm-ci-plugin` `env` allowlist names any of these. Both tokens travel by pipe, never through argv,
the environment or the log. Verify item 1 proves the rewrite and the helper selection with a fake helper and no real
token. Verify item 2 checks that `DOPPLER_SERVICE_ACCOUNT_TOKEN`'s cluster policy lets the `sample` pipeline read it.

### (e) Files, and which sibling each mirrors

All under `apps/sample/`, mode `100644`, seeded once. Templates hold no placeholder except
`tools/release-config.json`'s three (decision (h)).

| Path | New/changed | Purpose | Mirrors |
| --- | --- | --- | --- |
| `.buildkite/bootstrap.yml` | changed (W6) | stored step; adds decision (d)'s `env`; still one host step running `bash apps/sample/.buildkite/upload-pipeline.sh` | AppTwo `bootstrap.yml`; app-four `bootstrap.yml` (cookbook §3.3) |
| `.buildkite/upload-pipeline.sh` | changed (W6) | selector over `SAMPLE_CI_ACTION`; `release` refused unless `BUILDKITE_BRANCH` is `main` and `BUILDKITE_PULL_REQUEST` is `false`; unknown actions exit 2; `exec buildkite-agent pipeline upload --no-interpolation <file>` | AppThree and AppTwo `upload-pipeline.sh` |
| `.buildkite/pipeline.yml` | changed (W6) | credential-free validation step; adds the `env`; command `python3 apps/sample/tools/sample_ci.py validate`; `artifact_paths` `apps/sample/.ci-artifacts/validation/**/*` | AppTwo and AppThree `pipeline.yml` |
| `.buildkite/signing-preflight.yml` | new (W6) | one step, `doppler_token_secret: DOPPLER_SERVICE_ACCOUNT_TOKEN`, `timeout_in_minutes: 15`, command `... sample_ci.py signing-preflight` | AppThree `signing-preflight.yml` |
| `.buildkite/release.yml` | new (W6) | `sample-validation` → `sample-package` (`depends_on`, `doppler_token_secret`, timeout 150) → `sample-upload` (`depends_on`, `doppler_token_secret`, local plugin `./apps/sample/.buildkite/plugins/stage-input`, timeout 120); every step `retry: {automatic: false, manual: false}` | AppThree and AppTwo `release.yml` |
| `.buildkite/plugins/stage-input/plugin.yml` | new (W6) | local plugin, no configuration properties, requirements `python3`, `buildkite-agent` | AppThree `plugins/upload-input/plugin.yml` |
| `.buildkite/plugins/stage-input/hooks/pre-command` | new (W6) | host side, sourced: `set -euo pipefail`; requires `BUILDKITE_BUILD_CHECKOUT_PATH`; runs `python3 "$BUILDKITE_BUILD_CHECKOUT_PATH/apps/sample/tools/sample_ci.py" stage-ipa --checkout "$BUILDKITE_BUILD_CHECKOUT_PATH"`; **no `exec`, no `exit`** | AppThree `plugins/upload-input/hooks/pre-command` |
| `.buildkite/provider-settings.json` | unchanged | the record of intent: triggers disabled | AppTwo |
| `.buildkite/README.md` | changed (W6) | what each file does, how to start a release (`New Build` on `main` with `SAMPLE_CI_ACTION=release`), the signing preflight, the version policy, never re-upload blindly, the M7 paste | AppThree and AppTwo `README.md` |
| `.gitignore` | new (W2) | `/.ci-artifacts/` | the root `.gitignore`'s per-app receipt lines |
| `tools/release-config.json` | new (W2) | the one data file: bundle ids (placeholders), marketing version, `build_number_offset`, Doppler project/config, Bazel target | the role of AppThree's and AppTwo's `release-config.json` |
| `tools/sample_ci_common.py` | new (W3) | guest env, source identity and private clone, artifact paths, strict JSON, SHA-256, exclusive receipts, Doppler fetch with per-stage allowlist | AppTwo `buildkite_deployment_secrets.py`; AppThree `run_buildkite_recovery.py`'s checkout checks |
| `tools/sample_asc.py` | new (W3) | ES256 JWT via `openssl`; read-only App Store Connect client: profile by name, app id by bundle id, builds by version, the processing wait | cookbook §6.2; AppThree `check_testflight_build.rb` |
| `tools/sample_signing.py` | new (W4) | keychain lifecycle (§6.1), identity∩profile (§6.3), profile install, entitlement coverage | AppThree and AppTwo `signing_keychain.py`, `preflight_buildkite_signing.py` |
| `tools/sample_ipa.py` | new (W4) | IPA inspection | AppThree `inspect_testflight_ipa.py`; AppTwo `inspect_buildkite_ipa.py` |
| `tools/sample_ci.py` | new (W5) | entry point: `validate`, `signing-preflight`, `package`, `upload`, `stage-ipa` | AppTwo `run_buildkite_{validation,package,upload}.py`, `prepare_buildkite_stage.py` |
| `tools/tests/test_sample_ci_common.py`, `test_sample_asc.py` | new (W3) | offline unit tests | `tools/tests/` in both apps |
| `tools/tests/test_sample_signing.py`, `test_sample_ipa.py` | new (W4) | offline unit tests | same |
| `tools/tests/test_sample_ci.py` | new (W5) | offline unit tests | same |
| `ios/tools/generate_app_icon.py` | new (W1) | standard-library generator: an opaque 1024×1024 RGB PNG and the asset catalog's `Contents.json` files | cookbook §6.4 ("Generating the icon from a committed script") |
| `ios/BUILD.bazel` | changed (W1) | a `genrule` producing the catalog; `app_icons = [":app_icon"]` on `Sample` | AppThree `app_icons = glob([...AppIcon.xcassets/**])` |
| `ios/Resources/Info.plist` | changed (W1) | `UISupportedInterfaceOrientations` with all four (AppTwo's exact list) | AppTwo `Info.plist` |

17 existing + 15 new = 32 files plus the marker, within `github.scaffold.ensure`'s 64.

### (f) How a release starts, and why not on push

A release is a build of a reviewed `main` commit with `SAMPLE_CI_ACTION=release`, started from the Buildkite UI
("New Build", Environment Variables) or the API. That click is the human's release decision, not setup. It does not
depend on the pipeline's provider settings, which willikins does not write (3a). A tag route (AppThree's) needs
`build_tags: true` in provider settings, which would be a manual setting. Push-triggered validation duplicates
`ci.yml`. Without a path filter it would also spend a VM on each of the monorepo's roughly 500 monthly pushes,
contending on `ci-host`'s concurrency group, so it is left out (follow-up). Verify item 13 reads the pipeline's
actual provider settings, read-only, so the README tells the truth.

**The branch check is not authorization** (cookbook §3.2: "Exact-SHA verification proves identity, not
authorization"). Release credentials are bounded by who can start a build on `example-bk-org/sample`. The README
says so.

### (g) App Store validation prerequisites are part of the scaffold

An upload that App Store validation rejects is not a release, and the seed cannot be revised after it lands. So W1
pre-empts cookbook §6.4's list inside the scaffold:

- **Icon: generated, since the seed cannot hold a PNG.** `RepoFile` refuses NUL and the scaffold writes no binary
  file (3g). `ios/tools/generate_app_icon.py`, standard library only (`zlib`, `struct`, `json`), writes three
  outputs:
  - `Resources/AppIcon.xcassets/Contents.json`;
  - `Resources/AppIcon.xcassets/AppIcon.appiconset/Contents.json`, one `universal` `ios` image of `1024x1024`, the
    single-size form;
  - `Resources/AppIcon.xcassets/AppIcon.appiconset/AppIcon-1024.png`, colour type 2 (RGB, no alpha), one flat
    colour.

  The `genrule` named `app_icon` declares all three as `outs`, so `actool` sees one catalog. Contents.json is
  generated as well because a source `Contents.json` would land in a different root from a generated PNG. Its `cmd`
  is `python3 $(location tools/generate_app_icon.py) $(OUTS)`. The script maps each output to its content by
  basename and parent directory and refuses any other set.
- `CFBundleIconName` comes from `actool`'s partial Info.plist, which `rules_apple` merges, as with AppThree and
  AppTwo, whose `Info.plist`s do not set it. `sample_ipa.py` asserts it is present in the built bundle.
- `UISupportedInterfaceOrientations` lists all four, because `families` contains `ipad`.
- `UIRequiredDeviceCapabilities` already lists `arm64` (W1).

The only test of App Store validation is the first real upload (verify item 9). The build test is verify item 4, a
re-run of 3g's verify item 8 over the new rendered set: `ci.yml` builds `//...` on the push, so a broken `genrule`
turns `main` red.

### (h) Placeholders: one data file, quoted values only

The scripts need the three bundle identifiers, because each profile's name is its bundle identifier and the
inspection compares ids. Rather than templating Python, `tools/release-config.json` holds them:

```json
{
  "schema": 1,
  "bundle_ids": {
    "app": "{{ 0 }}",
    "nse": "{{ 1 }}",
    "widgets": "{{ 2 }}"
  },
  "marketing_version": "0.1.0",
  "build_number_offset": 0,
  "doppler": {"project": "sample", "config": "prd_deployment_ios"},
  "bazel_target": "//apps/sample/ios:Sample"
}
```

Each placeholder is a whole JSON string value. `TemplateValue`'s grammar admits no `"` or `\`, so a value cannot
leave the string. W2 extends `every_placeholder_sits_in_a_quoted_or_identifier_only_position`'s strict allowlist by
exactly one shape, `"<key>": "{{ N }}",?`, with `<key>` ∈ {`app`, `nse`, `widgets`}, allowed in exactly this file. The
count goes from 9 to 12. The test is extended, not loosened.

`build_number_offset` "can only go up" (cookbook §1); the README says so. Sample has no previous provider, so it is 0.

`{{` anywhere else in a template is refused by `repo.file.render`. **Every template author must avoid it**: no Python
f-string with an escaped brace, and no Jinja-looking text in the README.

### (i) The CI scripts, specified

Python 3.9, standard library, offline-testable. Every subprocess takes an argv list (never `shell=True`) and an
injected runner, so tests never touch `security`, `bazel`, `codesign`, the network or a keychain. No value from
Doppler is ever printed, logged or written to a file, except the `.p12` and `.p8`, which go to `0600` files in a
`0700` temporary directory, deleted in `finally`. Errors are a small fixed set of messages, never a response body.
Missing Doppler **names** may be printed (names are not secret).

**`sample_ci_common.py`** (≤ 12 KiB)
- `GUEST_PATH`, `DEVELOPER_DIR`, `IDENTITY_NAMES` (the seven).
- `child_env()`: a fresh dict with only `PATH`, `DEVELOPER_DIR`, `LANG`, `LC_ALL`, `HOME`, `TMPDIR`, `CI`. It never
  carries `DOPPLER_TOKEN`.
- `load_release_config(repo)`: strict JSON (duplicate keys refused), exact key set, ids `[A-Za-z0-9.-]+`, semver
  `marketing_version`, integer `build_number_offset ≥ 0`.
- `verify_source(mount)`:
  - `BUILDKITE_COMMIT` is 40 hex;
  - the mount's `HEAD` equals it;
  - `git status --porcelain` is empty;
  - clone `--no-hardlinks` into `$TMPDIR/sample-src-<BUILDKITE_JOB_ID>`, check out detached at the commit, and verify
    it again.
  
  It returns the private clone path. Builds run in the private clone. Only artifacts go back to the mount.
- `artifact_dir(mount, stage)`: `apps/sample/.ci-artifacts/<stage>/`, refusing any symlinked parent.
- `sha256_file`, `write_json_exclusive` (`O_CREAT|O_EXCL`), `read_json_strict`.
- `fetch_doppler(stage)`:
  - pops `DOPPLER_TOKEN` from `os.environ`;
  - one `GET https://api.doppler.com/v3/configs/config/secrets/download?project=sample&config=prd_deployment_ios&format=json`
    through a proxy-free opener that refuses redirects, with verified TLS, a 30-second timeout and a 1 MiB cap;
  - returns only the stage's allowlisted names (SHARED VALUES), failing with the missing names if any are absent.

**`sample_asc.py`** (≤ 10 KiB)
- `der_to_raw_signature` (cookbook §6.2) and `make_jwt(issuer, key_id, key_path, now)`. The header is
  `{"alg":"ES256","kid":…,"typ":"JWT"}`, `exp = now + 1140`, `aud` `appstoreconnect-v1`, signed by
  `openssl dgst -sha256 -sign` with the signing input on stdin.
- `AscClient`, taking an injected opener, exposes `GET` only, on a fixed path set:
  - `profile_content(name)`: `/v1/profiles` with `filter[name]` = name, `filter[profileType]=IOS_APP_STORE` and
    `filter[profileState]=ACTIVE`; exactly one, else a fixed error naming the profile name; then `/v1/profiles/{id}`
    for `profileContent`, base64-decoded.
  - `app_id(bundle_id)`: `/v1/apps` with `filter[bundleId]`.
  - `builds(app_id, marketing, build_version)`: `/v1/builds` with `filter[app]`, `filter[version]` and
    `filter[preReleaseVersion.version]`.
  - `wait_valid(...)`: polls every 30 seconds for up to 60 minutes. `VALID` passes. `INVALID` or `FAILED` fails. A
    timeout fails "outcome unknown; reconcile in App Store Connect before another release".
- No `limit` parameter anywhere (3e: an Apple endpoint rejected it).

**`sample_signing.py`** (≤ 14 KiB), cookbook §6.1 in order:
- `parse_search_list(output)` uses `re.findall(r'"([^"]*)"')`, never line concatenation.
- `SigningKeychain`, a context manager over `~/Library/Keychains/sample-signing.keychain-db` with a random password:
  - capture the user search list before anything else;
  - delete any same-named keychain left over;
  - `create-keychain`, then `set-keychain-settings -lut 7200`;
  - `list-keychains -d user -s <captured…> <new>`, one argument per entry;
  - `unlock-keychain`;
  - import the `.p12` with `-T /usr/bin/codesign -T /usr/bin/security`;
  - import Apple's WWDR G3 intermediate from `https://www.apple.com/certificateauthority/AppleWWDRCAG3.cer`
    (verify item 11);
  - `set-key-partition-list -S apple-tool:,apple:,codesign: -s`.
  
  `__exit__` (which also runs on exceptions and on `SIGTERM`, through a handler that raises) restores the captured
  list verbatim and deletes the keychain. `assert_clean(before)` then requires the search list to be byte-identical,
  the file to be gone, and nothing named `sample-signing*` to remain in `~/Library/Keychains`. It reports any captured
  entry that does not exist.
- `identities(keychain)`: SHA-1 set from `find-identity -v -p codesigning <keychain>`.
- `profile_facts(profile_bytes)`: `security cms -D` to plist (`plistlib`), giving `Name`, `UUID`, `TeamIdentifier`
  (compared, never printed), `Entitlements`, `ExpirationDate`, and certificate SHA-1s
  (`hashlib.sha1(der).hexdigest().upper()` over `DeveloperCertificates`).
- `install_profile(bytes)` writes `~/Library/MobileDevice/Provisioning Profiles/<UUID>.mobileprovision` (verify
  item 8).
- `check_signing(identities, profiles, entitlements_files)`:
  - each of the three profiles intersects the identities (cookbook §6.3, failing "NO MATCH for <profile name>");
  - not expired within 7 days;
  - `Name` equals the bundle id;
  - each target's `.entitlements` keys are present in its profile with equal values, except that
    `com.apple.security.application-groups` must be a subset.
  
  It prints counts and booleans, never fingerprints or a team id.

**`sample_ipa.py`** (≤ 10 KiB), `inspect(ipa, config, build_version) -> report`:
- the zip is a regular file under 2 GiB, with no absolute, `..` or symlink entries;
- `Payload/Sample.app` has `CFBundleIdentifier` equal to the app id and `CFBundleVersion` equal to `build_version`,
  plus `CFBundleIconName`, four orientations and `arm64`;
- `PlugIns/*.appex` are exactly the NSE and widgets ids;
- every bundle's `embedded.mobileprovision` `Name` equals its bundle id;
- `codesign --verify --strict --deep` passes on the extracted app;
- `lipo -archs` of each main executable is exactly `arm64`.

The report holds booleans, ids and the hash, never certificate data.

**`sample_ci.py`** (≤ 18 KiB). `argparse`, one subcommand per stage, each writing an exclusive receipt
`{schema, stage, commit, build_id, job_id, build_number, ok, checks: {...}}` into its artifact dir:
- `validate`:
  - refuses if `DOPPLER_TOKEN` is set (validation is credential-free);
  - runs cookbook §4.1's preflight (`xcodebuild -version`, both SDK paths, `bazel --version` against `.bazelversion`);
  - `verify_source`;
  - in the private clone: `bazel build --config=ci --host_macos_minimum_os=15.0 --macos_minimum_os=15.0 -- //apps/sample/...`
    (cookbook §7.5), then the same with `--config=ios_sim` for `//apps/sample/ios:Sample`, then
    `python3 -m unittest discover -s apps/sample/tools/tests -t .`.
- `signing-preflight`: `verify_source`, then `fetch_doppler("signing")`, then a JWT, then the three profiles by name
  (from `release-config.json`, name = id), installed, then `with SigningKeychain` around `check_signing`, then
  `assert_clean`. No build.
- `package`:
  - `verify_source`, then the build number (`BUILDKITE_BUILD_NUMBER + offset`), then `build_version`;
  - `fetch_doppler("package")`;
  - the App Store Connect availability check: no build exists for that version (cookbook §1: an observation, not a
    reservation);
  - inside `SigningKeychain`: the same signing checks, then
    `bazel build //apps/sample/ios:Sample --config=beta --config=ios_device --ios_multi_cpus=arm64 --embed_label=<marketing>.<build> --host_macos_minimum_os=15.0`;
  - the IPA is resolved with `bazel cquery … --output=files` (cookbook §7.3), never a `bazel-bin` guess, and must be a
    regular file;
  - after the keychain exits, `assert_clean`; inspect; copy to the mount's `package/Sample.ipa` and rehash;
  - write `inspection-report.json` and `package-receipt.json` with the commit, `build_version`, size and SHA-256.
- `stage-ipa` (**host**, called by the hook):
  - no Doppler, no Apple;
  - `buildkite-agent step get outcome --step sample-package` must equal `passed`;
  - `buildkite-agent artifact download 'apps/sample/.ci-artifacts/package/*' <fresh temp dir> --step sample-package --build "$BUILDKITE_BUILD_ID"`;
  - `Sample.ipa` and `package-receipt.json` must be regular, non-symlinked files, and the receipt's commit must equal
    `BUILDKITE_COMMIT` with matching size and SHA-256;
  - moves them into the checkout's `upload-input/` and prints the hash.
- `upload`:
  - `verify_source`; the input from `upload-input/` with the receipt, commit, size and hash re-verified; re-inspect;
  - `fetch_doppler("upload")` (App Store Connect only), then the availability check again;
  - **write `upload-attempt.json` before sending** (cookbook §6, step 6);
  - write the `.p8` as `AuthKey_<id>.p8` in a `0700` directory named by `API_PRIVATE_KEYS_DIR`;
  - `xcrun altool --upload-app -f <ipa> -t ios --apiKey <id> --apiIssuer <issuer>`, never retried;
  - `wait_valid`, then `upload-receipt.json`.
  
  It never calls `bazel` and never creates a keychain.

## The Sample document, after this milestone

- **Removed:** input `m6_ci_doppler_access_done`; node `m6_ci_doppler_access`.
- **Added:** `ci_doppler_access` (SHARED VALUES); fifteen `repo.file.render` nodes (decision (e)); each one's `file`
  appended to `sample_files.files`.
- **Changed templates:** `build_bazel_ios`, `info_plist`, `pipeline_yml`, `upload_pipeline_sh`, `bootstrap_yml`,
  `buildkite_readme`.
- **Untouched, and pinned untouched by a test:** input `m5_apns_key_done` and node `m5_apns_key` (tool, `step` text,
  `acknowledged` binding), byte for byte.
- **Header:** a dated "3h" section. It says the pipeline exists, m6 is a node, m5 stays, and the service-account name
  is the coordinator's literal. The m5/m6 bullet becomes m5 alone.
- **The commit headline stays** `feat(sample): scaffold the iOS app, NSE, widgets and Buildkite files`: it is still
  accurate, and the operator has seen it.

## Acceptance tests

1. **Types** (D1): `DopplerServiceAccountName` accepts `buildkite-ci`, `Buildkite CI`, `ci.service_account-2`, and
   the placeholder `REPLACE-WITH-CI-SERVICE-ACCOUNT`. It refuses empty, a leading or trailing space or separator, two
   adjacent separators, a control character, `/`, and 65 characters. `DopplerProjectRole` accepts exactly `viewer`
   and `collaborator`, and refuses `admin`, `owner`, `no_access`, `Viewer`, `viewer ` and the empty string. Neither
   type is secret. Both are registered (`domain_types!`), with examples that parse.
2. **`Action::Update`** (E1), with in-test tools: `Absent` with `updates` true plans `Update`; `replaces` true wins
   over `updates` true (`Replace`); `Present` plans `NoOp` and `updates` is never called; a pure or gate tool never
   has `updates` called (a panicking implementation proves it); `apply` with an approved `Create` that re-plans as
   `Update` fails `DriftKind::Action`; `render` prints `Update`; every characterization entry is byte-identical; the
   two schema snapshots change by the one enum value only.
3. **Client** (D2), with mocks that pin exact paths, queries and JSON bodies (the 3e lesson): paging stops at a short
   page and refuses past 50; `403` on each list maps to its fixed permission message; no response body appears in any
   error (a seeded marker in a mocked body never surfaces); `POST` and `PATCH` bodies are exactly as SHARED VALUES
   says, including `environments` omitted when `access_all_environments`; the client never retries.
4. **The tool** (D3), one test per row of decision (a)'s table plus: zero and two name matches are `NotFound` and
   `Conflict`; a missing project is `Absent`; duplicate or empty `environments` are `Invalid` before any request; the
   `PATCH` sends the sorted union; a failed write whose re-read is `Present` is `changed: false`; no `DELETE` is ever
   recorded; outputs never contain a slug. The fake twin agrees on every row (`fake_agrees_with_live`). Specs are
   equal (`catalog_parity`). `LIVE_TOOL_NAMES` is pinned at 38.
5. **Live member cycle** (D4, run once by the coordinator), below.
6. **Test structure and budgets** (W0): every count in `sample_document.rs` derives from one `SAMPLE_FILES` table; the
   document is ≤ 196,608 bytes; every template is ≤ 20,480 characters; `sample_files` holds ≤ 64 files; the
   `#[ignore]`d `render_sample_files_to_dir` writes every rendered file under `WILLIKINS_SAMPLE_RENDER_DIR`; existing
   tests are unchanged in substance.
7. **App Store prerequisites** (W1): the rendered `ios/BUILD.bazel` has the `app_icon` `genrule` with exactly three
   `outs` and `app_icons = [":app_icon"]`, and passes `every_rendered_starlark_file_uses_only_valid_escapes`; the
   rendered `Info.plist` has the four orientations; `generate_app_icon.py`, run on the rendered set, writes an RGB
   1024×1024 PNG (IHDR colour type 2, no `tRNS`) and parseable `Contents.json` files; insta snapshots are reviewed.
8. **Placeholders** (W2): the allowlist test accepts exactly the three new JSON-value lines and still refuses one in
   any other file or shape (a negative case built in the test); the placeholder count is 12; rendered
   `release-config.json` parses and holds the three real ids.
9. **Scripts** (W3–W5), each run over the rendered set:
   - every `.py` compiles under `/usr/bin/python3 -m py_compile`;
   - `/usr/bin/python3 -m unittest discover -s apps/sample/tools/tests -t .` passes, offline;
   - a Rust test refuses `shell=True`, `os.system`, `os.popen`, `subprocess.getoutput`, `getstatusoutput`, `match `
     at a line start, `/Users/`, `team_id` and `{{` in every rendered `.py`.

   The Python tests cover:
   - `der_to_raw_signature` against an `openssl genpkey` P-256 key, verified with `openssl dgst -verify` (cookbook
     §6.2; skipped only if `openssl` is missing);
   - `parse_search_list` on the indented, quoted form;
   - the keychain context manager's call order, including restore on an exception;
   - `check_signing`'s NO MATCH and entitlement-subset cases;
   - `fetch_doppler`'s allowlist and redirect refusal, against a fake opener;
   - receipt and hash mismatch refusals in `stage-ipa` and `upload`;
   - `upload` writing `upload-attempt.json` before calling `altool`;
   - `validate` refusing a set `DOPPLER_TOKEN`.
10. **Buildkite files** (W6), in `sample_document.rs`:
    - every rendered `.yml` and `.json` parses with `serde_json`;
    - the top-level `env` of `bootstrap.yml`, `pipeline.yml`, `signing-preflight.yml` and `release.yml` is the same
      object, equal to decision (d)'s block;
    - `VALUE_1` starts with `!`, has no `$`, and names `DOPPLER_SERVICE_ACCOUNT_TOKEN` and
      `project=sample&config=prd_deployment_ios&name=GH_CLONE_TOKEN`;
    - every `vm-ci-plugin` step pins SHARED VALUES' plugin, image, queue and concurrency group, and its `env` allowlist is
      exactly the seven identity names;
    - `doppler_token_secret` appears on exactly the preflight, package and upload steps, always
      `DOPPLER_SERVICE_ACCOUNT_TOKEN`;
    - `release.yml` chains `sample-validation` → `sample-package` → `sample-upload`, with retries off on every step;
    - every script path a command or hook names is in `sample_files`;
    - the hook contains neither `exec` nor `exit`;
    - `bash -n` passes on `upload-pipeline.sh` and the hook (rendered-set gate);
    - `bootstrap_gate.expected` is still `bootstrap_yml.file`.
11. **The Sample document** (W7):
    - checks clean against the fake catalog;
    - no `operator.acknowledge` node for m6, and no `m6_*` input;
    - `m5_apns_key` and `m5_apns_key_done` are byte-identical to before (pinned by exact text);
    - `ci_doppler_access` binds `project` from `steps.doppler.project`, with `role` literal `viewer` and
      `environments` the literal list `[prd]`;
    - a first fake run plans `ci_doppler_access` `Create` and grants it, and a re-run plans it `NoOp`;
    - the fake is seeded with the service-account name **read from the document**, so the coordinator's literal edit
      needs no test edit;
    - `sample_apply_blocked_redaction.rs` still passes.
12. **Guards**: `secret_literal_guard`, `no_gh_writes_guard` and `no_certificate_writes_guard` stay green. Templates
    contain no `gh ` invocation and no token-shaped literal.
13. **Characterization**: every entry byte-identical except `workflows/sample-ios-app.yaml`'s own.

## The live member cycle (written by D4, run once by the coordinator)

`crates/willikins-providers-doppler/tests/live_project_member_cycle.rs`, modelled on the coordinator's 2026-10-01
sandbox probe, sandbox only. It sources `~/.config/willikins/sandbox.env` in the same command, prints statuses,
roles, counts and booleans only, and greps its own output at the end, as the 3c harnesses do.

**The probe script itself lives in a session scratchpad an implementer will not have.** These are the facts the
cycle needs from it. All calls go to `https://api.doppler.com` with `Authorization: Bearer <token>` and
`Content-Type: application/json`, and every token is resolved in-process and never printed.

| Fact | Value |
| --- | --- |
| Sandbox guard | `GET /v3/me` → `workplace.name` must equal `Willikins - Test - Sandbox` |
| The admin token | `WILLIKINS_DOPPLER_TOKEN` from `sandbox.env` (sandbox workplace) |
| Create a service account | `POST /v3/workplace/service_accounts`, body `{"name": N, "workplace_role": {"permissions": ["create_enclave_project", "team", "service_accounts"]}}` (inline role) or `{"name": N, "workplace_role": {"identifier": "no_access"}}`. If the `no_access` form answers ≥ 300, retry with `{"name": N}` alone. The slug is `service_account.slug` |
| Give it a token | `POST /v3/workplace/service_accounts/service_account/{slug}/tokens`, body `{"name": "probe", "expires_at": "<UTC now + 1 h, %Y-%m-%dT%H:%M:%SZ>"}`. The token is `api_key`, else `api_token.api_key`, else `api_token.token` |
| Create a project | `POST /v3/projects`, body `{"name": P, "description": "probe, delete me"}`; slug `project.slug` (else the name) |
| List or add members | as SHARED VALUES (`/v3/projects/project/members?project=P`) |
| Delete a project | `DELETE /v3/projects/project`, body `{"project": P}` |
| Delete a service account | `DELETE /v3/workplace/service_accounts/service_account/{slug}` |
| Counts | `GET /v3/projects?per_page=100` and `GET /v3/workplace/service_accounts?per_page=100`; filter names containing the prefix |

1. `GET /v3/me` must name `Willikins - Test - Sandbox`, or the run refuses. Count the projects and service accounts.
   Refuse if any `willikins-probe-delete-me*` already exists.
2. Create `willikins-probe-delete-me-creator-<unix>` with workplace permissions `create_enclave_project`, `team` and
   `service_accounts` (the real account's intended set), and give it a one-hour token. Create
   `willikins-probe-delete-me-ci-<unix>` with role `no_access` and a one-hour token. Arm the guard with every name
   **before any assertion**.
3. As the creator, create project `willikins-probe-delete-me-<unix>`, then a branch config `prd_ci` under `prd`. As
   the admin, create base project `willikins-probe-delete-me-base-<unix>` with an inheritable config holding one
   secret, `WILLIKINS_PROBE_INHERITED`, with a random value never printed, and make `prd_ci` inherit it.
4. As the creator: the tool's `read` is `Absent`; `ensure` gives `changed: true`; an independent `GET` lists the CI
   account as `viewer`, `access_all_environments: false`, `[prd]`.
5. `read` is `Present`, and a second `ensure` gives `changed: false`.
6. As the admin, raw-`PATCH` the member to `environments: ["dev"]`. Then `read` is `Absent` with `updates` true;
   `ensure` gives `changed: true`; the member now lists `[dev, prd]`, so `dev` was kept. This settles verify item 3
   for the list case.
7. Raw-`PATCH` the role to `collaborator`. `read` is `Mismatch { role }`; `ensure` is `Conflict`; the member is
   unchanged.
8. **The point of m6:** with the CI account's own token, `GET /v3/configs/config/secrets/download?project=…&config=prd_ci`
   answers 200 and its key set contains `WILLIKINS_PROBE_INHERITED`, and
   `GET /v3/configs/config/secret?…&name=WILLIKINS_PROBE_INHERITED` answers 200 with `value.computed` present. Only
   statuses and booleans are recorded. This settles verify item 12.
9. Try to create a second service account with the CI account's exact name and record the status. If it was
   created, `read` is `Conflict`. A name with no match is `NotFound`. This settles verify item 6.
10. The guard deletes both projects and every service account by recorded name. Confirm `404`s, and confirm the
    counts equal step 1's.

## Verify before relying on them

1. **The checkout override works for an SSH-URL pipeline** (decision (d)). On this host, with a fake helper that
   prints `username=probe` / `password=probe` in `GIT_CONFIG_VALUE_1`'s place, run
   `GIT_TERMINAL_PROMPT=0 GIT_TRACE=1 git ls-remote git@github.com:Example-Org/monorepo.git` with the env. The trace
   shows the HTTPS URL and the fake helper running, and the expected `401` or `403`. Then run `git ls-remote
   https://github.com/Lightless-Labs/vm-ci-plugin` and confirm the fake helper is **not** consulted. No real token. Then
   check the agent host's git version supports `GIT_CONFIG_COUNT` (2.31 or later; cookbook: the Mini has 2.39.5).
2. **`DOPPLER_SERVICE_ACCOUNT_TOKEN` is readable by the `sample` pipeline.** Read the cluster secret's policy,
   read-only, key names only (cookbook §2.2: an empty policy is readable by every pipeline).
3. **`PATCH` semantics**: an `environments` list replaces the set (D4 step 6), and omitting it leaves
   `access_all_environments` intact. Step 6 covers the list case. The omit case is pinned by mocks and is otherwise
   the coordinator's to probe if wanted.
4. **The rendered set builds** (3g verify item 8, again): W1–W6's rendered files in a scratch clone of the monorepo
   pass `bazel build --config=ci //apps/sample/...`, including the generated icon catalog.
5. **`altool --upload-app` with an API key works on Xcode 26.3.** If it does not, the fallback is `xcrun
   iTMSTransporter` with the same key. Decided on the first release's log.
6. **Duplicate service-account names** (D4 step 9).
7. **Doppler list paging**: whether `per_page=100` is accepted on both lists (D4 records it). If not, use the
   default 20 and keep the 50-page bound.
8. **`rules_apple` 4.3.3 `local_provisioning_profile` searches `~/Library/MobileDevice/Provisioning Profiles`.**
   Read `apple/internal/local_provisioning_profiles.bzl` at that tag, verbatim, before W4 lands. If it searches the
   Xcode 16 `UserData` path too or instead, install there as well.
9. **App Store validation passes** for the generated icon and the plist keys. Only the first real upload can say. The
   README names this.
10. **willikins' real Doppler service account is still admin of project `sample`.** The token was rotated (401 on
    2026-10-01). If the replacement is a different account, it is not admin, and the operator makes it admin or
    re-creates nothing (operator).
11. **The image needs WWDR G3**, and it is fetchable from the guest. If the image already trusts it, the import is
    harmless.
12. **A project-scoped viewer reads inherited secrets through the inheriting config** (D4 step 8). If not, m6's grant
    is not enough. The answer is a grant on the base projects too, which needs an operator decision.
13. **Sample's pipeline as Buildkite stores it**: `repository` (expected SSH, per `ssh_repository_url`) and
    `provider_settings`, read-only, key names and booleans only, so the README is accurate.
14. **The real CI service account's name parses** as `DopplerServiceAccountName`. If not, widen the grammar in a
    reviewed change, never the literal.
15. **`createCommitOnBranch` at Sample's new size** (3g verify item 5). About 32 files and 100–130 KiB of content,
    base64 in one request. The coordinator may run L1's harness against a throwaway sandbox repository with the
    rendered set.

## Needs the operator (blocks the real apply, not implementation)

1. **Grant willikins' real Doppler service account** (workplace "Example Workplace") the read-only workplace
   permissions **View Team (`team`)** and **View Service Accounts (`service_accounts`)**. Without both,
   `ci_doppler_access` fails at plan with the tool's fixed 403 message (probe, 2026-10-01).
2. **The replacement for `WILLIKINS_REAL_DOPPLER_TOKEN`** (401 "Invalid Auth token" since 2026-10-01; already in
   HANDOFF). If it belongs to a different service account, that account must be admin of project `sample` (verify
   item 10).
3. **The CI service account's exact name.** The coordinator looks it up and edits one literal.

M5 (the APNs `.p8`) remains the operator's, unchanged, by their instruction. M7 (pasting the rendered bootstrap)
remains a manual paste behind an observed gate (3a decision (a)).

## Gates

Scoped, per the host rules. Before every cargo command, `pgrep -x cargo` and `pgrep -f cargo-sweep` print nothing.
Use `-j 2` and `RUST_TEST_THREADS=2`. Run in the background with a 600,000 ms timeout, and read the log body. Never
pipe through `tail` or `tee`. Never edit tracked files while cargo builds. A linker "missing .rcgu.o" or `E0463` is
the host sweep: `cargo clean -p <crate>` and rebuild.

```
cargo fmt --all --check
cargo clippy -p <touched crate> --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test -p <touched crate> -j 2 --no-fail-fast
cargo check -p willikins-types -j 2
RUST_TEST_THREADS=2 cargo test -p willikins-dsl --test acceptance -j 2      # characterization
```

The template tasks (W1–W6) add the rendered-set gate:

```
WILLIKINS_SAMPLE_RENDER_DIR=<scratchpad>/sample-render RUST_TEST_THREADS=2 \
  cargo test -p willikins-cli --test sample_document -j 2 -- --ignored render_sample_files_to_dir
/usr/bin/python3 -m py_compile <every rendered .py>
(cd <scratchpad>/sample-render && /usr/bin/python3 -m unittest discover -s apps/sample/tools/tests -t .)
bash -n <rendered upload-pipeline.sh> <rendered hooks/pre-command>
```

The full workspace gate is the coordinator's.

## Tasks

One lane at a time on `main` (one cargo at a time on this host), in this order. Each task commits by path with
`git commit --only`, test first, one behaviour per commit, and is green alone.

| # | Task | Delegate to |
| --- | --- | --- |
| D1 | **Types** (decision (a); acceptance 1). One commit, `willikins-types`: `DopplerServiceAccountName`, `DopplerProjectRole`, registered. The mechanical ripple in any type-catalog snapshot (e.g. the MCP server's) is diff-reviewed and accepted | sonnet implements, opus attacks |
| E1 | **`Action::Update`** (decision (b); acceptance 2). Commit 1, `willikins-core`: `Tool::updates`, `plan_one`'s branch, tests with in-test tools, the drift case. Commit 2: `willikins-cli` `render.rs` arm and both schema snapshots (diff = one enum value) | sonnet implements, opus attacks |
| D2 | **Doppler client** (acceptance 3). One commit, `willikins-providers-doppler/src/client.rs`: the two paged lists, `POST`, `PATCH`, the two 403 messages, with mocks | sonnet implements, opus attacks |
| D3 | **`doppler.project_member.ensure`** (decision (a); acceptance 4). Commit 1: the live tool, `updates()`, and mocks for every table row. Commit 2: the fake twin (`FakeState` gains service accounts by name and project members; a `with_doppler_service_account(name)` seeder), `fake_agrees_with_live`, `catalog_parity`, registration and every count pin | sonnet implements, opus attacks |
| D4 | **Live member cycle** (acceptance 5), written and compiling, not run | sonnet writes, coordinator runs once |
| W0 | **Test structure** (acceptance 6). One commit, `crates/willikins-cli/tests/sample_document.rs` only: the `SAMPLE_FILES` table driving every count, the size-budget tests, `render_sample_files_to_dir`. No document change; every existing assertion holds | sonnet implements |
| W1 | **App Store prerequisites** (decision (g); acceptance 7). One commit: `generate_app_icon.py` render node, the `genrule` and `app_icons` in `build_bazel_ios`, orientations in `info_plist`, their `SAMPLE_FILES` row and snapshots | sonnet implements, opus attacks |
| W2 | **`.gitignore` and `release-config.json`** (decision (h); acceptance 8). One commit, including the allowlist extension | sonnet implements |
| W3 | **`sample_ci_common.py`, `sample_asc.py` and their tests** (decision (i); acceptance 9). One commit | sonnet implements, opus attacks |
| W4 | **`sample_signing.py`, `sample_ipa.py` and their tests** (acceptance 9). One commit, after verify item 8 is read | sonnet implements, opus attacks |
| W5 | **`sample_ci.py` and its tests** (acceptance 9). One commit | sonnet implements, opus attacks |
| W6 | **Buildkite files** (decisions (c), (d), (f); acceptance 10). Commit 1: changed `bootstrap.yml`, `pipeline.yml`, `upload-pipeline.sh`, `README.md`. Commit 2: new `signing-preflight.yml`, `release.yml`, the `stage-input` plugin and hook, with the acceptance-10 tests | sonnet implements, opus attacks |
| W7 | **m6 becomes `ci_doppler_access`** (acceptance 11). One commit: the node, removal of the input and leaf, the header, `sample_document.rs` and `sample_apply_blocked_redaction.rs` updates, the characterization diff reviewed (Sample's entry only) | sonnet implements, opus attacks |

Then the coordinator:
- sets the real service-account literal;
- runs D4 once;
- settles verify items 1, 2, 4 and 13 (and 15, if it chooses);
- runs the full gate.

Each piece gets an independent opus attack, with at least four mutations restored from saved copies (`cmp`
confirming byte-identity), recorded under `docs/research/2026-10-0x-m3h-adversarial-pass*.md`. Priority targets:
- the tool lowering a role or dropping an environment by any path;
- a slug or response body leaking into an error or output;
- `Update` planned where `Create` or `Mismatch` is right;
- a Doppler value reaching a log, argv or file in the rendered scripts;
- a placeholder outside a JSON string value;
- the checkout override capturing `vm-ci-plugin`'s URL;
- the hook's sourcing semantics;
- the keychain surviving a failure path.

## Risks

1. **The document size limit.** At 256 KiB, the document has room for this pipeline only if the scripts stay small.
   W0 pins the 192 KiB budget. Exceeding it means trimming a script, not raising `MAX_DOCUMENT_BYTES`.
2. **`main` goes red** if the `genrule` or a template breaks `bazel build //...`. Verify item 4 runs before the real
   apply. After landing, only a developer commit can fix it (the seed rule).
3. **The first release finds an App Store validation complaint** not pre-empted (verify item 9). It costs one build
   number and a template-equivalent fix by a developer commit in the monorepo, since the seed has landed.
4. **The seed cannot be revised.** Anything missing from W1–W6 at apply time becomes a developer's hand edit, which
   is exactly what the operator refuses. That is why this lands before the real apply, and why W6's tests
   cross-check every command against the scaffolded paths.
5. **The engine change (E1) is shared by every document.** It is a default-false hook, attacked for byte-identity.
6. **The inherited-secret read-through fails** (verify item 12). m6 would then need grants on the base projects:
   recorded, not guessed.
7. **Build-number collisions** (cookbook §1): availability is checked twice but is not a reservation. Retries are
   disabled, and an uncertain upload is reconciled by hand before another release (README).
8. **Host disk at about 94%.** Verify item 4's Bazel build needs the operator's go-ahead, as 3g's item 8 did.
