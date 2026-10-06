# Milestone 3l: a document creates a brand-new repository and scaffolds it

**Created:** 2026-10-05 (from the operator's question of 2026-10-04: what happens for a new repository, or one that
is not cloned locally)
**Reviewed:** 2026-10-05 (portfolio review of the five plans of 2026-10-05: 3k, 2b, 3l, 3m, 3n)
**Addendum:** 2026-10-05 — portfolio review. Order: 3k, then 3n F1–F3, then this milestone, then 2b, 3n's S and G
parts, and 3m. D1 adds a top-level document under `workflows/`, which two exact-set pins in `willikins-server` list
(`acceptance_13_trusted_directory.rs`, twice, and `image_contents.rs`), so D1 now updates them and gates them. 3m's
P1 extends the same `RepoBody` and the same exhaustive destructure in `repo_ensure.rs` after this milestone; S1's new
field stays `#[serde(default)]` so 3m's fixtures keep parsing. After 3k, new integration tests live under `tests/it/`
(Gates).
**Addendum:** 2026-10-05 (operator's decision) — no new credential mechanism: repositories in a real organisation are created with an org-scoped PAT stored in Doppler, as every other provider credential already is ("We already use a PAT for everything for now. Don't start making up new requirements."). The GitHub App option is dropped from this plan.
**Addendum:** 2026-10-06 (task S3 implementation) — a case decision (a)/(b) are silent on: after `ensure`'s existing commit-retry loop (3g) gets a `createCommitOnBranch` failure and re-observes to decide what to do next, that re-observe can in principle read `ScaffoldState::Empty` again (the branch's head it just compared against vanished, and the repository reverted to having no branches at all, between the first observe and this re-read). Decided the same way the loop already treats an unmoved head: report the original commit failure, rather than re-entering S3's own initialisation a second time from inside the 3g retry loop. Not covered by an acceptance test (no scenario in this milestone's table reaches it); flagged for X1's attack pass.
**Addendum:** 2026-10-06 (independent opus adversarial pass, `docs/research/2026-10-06-m3l-adversarial-pass-independent.md`) — five mutations survived the whole suite: a `403` from `GET /repos` read as absent, a failing branches listing read as empty, the root file chosen by declaration order, the S4 suffix on exhausted `409`s, and reworded fake messages (parity compared kinds only). All five are closed by tests (`e9304f8`, `2dd6573`); no source changed. Open for the coordinator: after the scaffold's own `2xx` `PUT`, ref lag that shows as a `404`/`409` ref plus a non-empty branches listing fails at once with the missing-branch `NotFound` instead of polling (finding 5; L1 steps 6 and 8 will show whether GitHub does this). Also open: the `409` retry `PUT`s 2 s after its last emptiness check, and pausing before the re-observe would narrow that (finding 6). Verify list gains: `branches?per_page=1` on a new repository answers `200 []`, not `409` (L1 step 3).

## Goal

One document, run once against an organisation where the repository does not exist yet, plans without error,
applies, and leaves a new repository whose default branch holds the document's scaffold and its marker. A second run
plans every node `NoOp`. Concretely:

1. **Plan time.** `github.scaffold.ensure` reads `Absent` (not `NotFound`) for a repository that does not exist yet,
   and for an empty repository (no branches). Today `plan` calls the scaffold's `read` with `github.repo.ensure`'s
   *predicted* outputs (`willikins-core/src/plan.rs`, `plan_one`), `get_branch_head` answers `404`, and the whole plan
   fails with `PlanError::Tool { NotFound: "branch ... does not exist ...; this tool never creates one" }` before
   anything applies.
2. **Apply time.** On an empty repository, the scaffold's own first write initialises it: one seed file through the
   Contents API (the only API GitHub documents for an empty repository), then the rest plus the marker through
   `createCommitOnBranch`, as today. A crash between the two writes converges on the next run.
3. **The fake models it.** The fake scaffold never consulted a repository's existence or branches, which is why a
   fake run of a new-repository document "passed" while the live one could not. It learns both.
4. **Proven live in the sandbox** by a guarded cycle: create, scaffold, converge, resume after a half-finished
   scaffold, delete.
5. **The operator's question is answered** in this plan (section "Why willikins never needs a local clone").

`github.repo.ensure` does not change: no `auto_init`, no new port, the same create body.

## The handoff's recorded answer, and why this plan departs from it

`docs/HANDOFF.md` (RESUME HERE, 2026-10-04) records: "a brand-new repository needs `github.repo.ensure` to initialise a
branch before a scaffold can commit". The obvious way to do that is `auto_init: true` on create. This plan does not,
on purpose:

- `auto_init` writes a file: "Pass `true` to create an initial commit with empty README." A scaffold that seeds
  `README.md` (nearly every scaffold) then finds a seed path holding content it did not put there, and milestone 3g
  decision (c) makes that a refusal, never an overwrite. `auto_init` would forbid the most common scaffold.
- A repository a document creates but does not scaffold stays exactly as asked: empty. Initialisation is a
  consequence of the first write, so it belongs to the tool that writes.
- `github.repo.ensure`'s create body is pinned by its mocks (`repo_ensure_mock.rs`) and its spec by three crates'
  catalog snapshots. Leaving it alone costs nothing.

## Out of scope

- **A branch-protection or ruleset tool** (decision (e)). A follow-up todo, opened by the coordinator.
- **Creating a branch on a repository that already has one.** The scaffold still refuses a missing branch on a
  non-empty repository, unchanged.
- **Choosing or renaming the default branch** (decision (c)).
- **Repository settings beyond today's** (merge options, `has_*` flags, templates, `.gitignore`/license templates).
- **The real organisation.** Every live write in this milestone is in the sandbox org. A real-org new repository waits
  on the operator's credential decision ("Needs the operator").

## Trust boundaries (normative)

These extend milestone 3g's, which still hold.

1. **The project's own GitHub credential, never the operator's.** Every call authenticates as `WILLIKINS_GITHUB_TOKEN`
   or a document-bound `token` port. Nothing invokes the operator's `gh` (`no_gh_writes_guard`). The live cycle sources
   the sandbox PAT from `~/.config/willikins/sandbox.env` in the command that runs it.
2. **Live GitHub writes only in the sandbox org `Willikins-Test`**, only on repositories the same run created, deleted
   by the same run's guard. The harness refuses any other org (the 3g `sandbox_org_from` check, copied).
3. **No new write surface beyond one endpoint.** The only new write is `PUT /repos/{owner}/{repo}/contents/{path}`,
   only on a repository the scaffold has just observed to be empty, only for a path from `files`, and never with a
   `sha` (so it can create and never replace). No tool gains a raw path, URL or command input.
4. **Response bodies stay outside errors.** The Contents API's failure body is suppressed exactly as
   `create_commit_on_branch` suppresses GraphQL's (3g): a static message and the status, never body text. The
   ruleset diagnostic (task S4) reports rule `type` strings only, each checked against `^[a-z_]{1,40}$`.
5. **No provider-token-shaped literal** anywhere, the live cycle included (CLAUDE.md).

## SHARED VALUES

Implementers read this table, never their prompts, for these values.

| What | Value |
| --- | --- |
| `ToolSpec` changes | **None.** Every `catalog_parity` snapshot, the fake catalog snapshot and `LIVE_TOOL_NAMES` stay byte-identical. The scaffold's `description` string is not edited (rustdoc only) |
| `RepoBody` (S1) | gains `#[serde(default)] pub(crate) default_branch: Option<String>`. Existing fixtures without it keep parsing |
| Branches call (S1) | `GitHubClient::has_any_branch(&self, repo: &GitHubRepo) -> Result<bool, ProviderError>`, `pub(crate)`: `GET /repos/{owner}/{name}/branches?per_page=1`, body `Vec<serde::de::IgnoredAny>`, `true` iff non-empty. Retried through `retry_secondary_limit` like every `GET` here |
| First-file call (S1) | `GitHubClient::create_first_file(&self, repo: &GitHubRepo, branch: &GitBranchName, file: &RepoFile, message: &str) -> Result<(), ProviderError>`, `pub(crate)`: `PUT /repos/{owner}/{name}/contents/{encoded path}` with exactly the keys `message`, `content` (standard base64 of the file's bytes), `branch`. Never `sha`, `committer` or `author`. Response parsed as `serde::de::IgnoredAny` |
| Path encoding (S1) | Each `/`-separated segment percent-encoded: every byte outside `[A-Za-z0-9._-]` as `%XX`, uppercase hex; `/` kept. `+` → `%2B`, `@` → `%40` (the only other characters `RepoPath` admits) |
| First-file failure message (S1) | `FIRST_FILE_FAILURE_MESSAGE = "GitHub refused to create the first file of an empty repository"`, carried with the status for every non-2xx except `401`/`403` (whose shared fixed messages already carry no body). Never body text |
| Rules call (S4) | `GitHubClient::branch_rule_types(&self, repo: &GitHubRepo, branch: &GitBranchName) -> Result<Vec<String>, ProviderError>`, `pub(crate)`: `GET /repos/{owner}/{name}/rules/branches/{branch}`; each element's `type`, kept only if it matches `^[a-z_]{1,40}$`, deduplicated, sorted; any other element counts once as `unrecognised` |
| Pause hook (S3) | `GitHubClient::pause(&self, duration: Duration)`, `pub(crate)`, delegating to the client's own `Sleeper`, so mock tests record waits instead of sleeping |
| Root file (S3) | The element of `files` whose `RepoPath::as_str()` is smallest in byte order (the order the marker already uses) |
| Root commit message (S3) | `"{headline}\n\nThe first commit of an empty repository, seeded by willikins. Marker: {marker}."` |
| Waits (S3) | After a `409` from the first-file `PUT`: re-observe, then up to `MAX_UNAVAILABLE_ATTEMPTS = 5` attempts in total, `UNAVAILABLE_WAIT = 2 s` apart. After a successful `PUT` whose re-observe still reads empty: up to `MAX_REF_VISIBLE_POLLS = 10` re-observes, `REF_VISIBLE_WAIT = 300 ms` apart (the 3g cycle's own figures) |
| Repository-absent message (S3, `NotFound`) | `` `{repo}` does not exist; this tool never creates a repository (github.repo.ensure does) `` |
| Default-branch mismatch (S2, `Conflict`) | `` `{repo}` is empty, and its first commit can only land on its default branch `{default}`, not `{branch}`; name `{default}` in this document, or change the organisation's default branch name before the repository is created `` |
| Unavailable (S2, `Provider`) | `` `{repo}` is not available yet (GitHub may still be creating it); re-run this document `` |
| Ref not yet visible (S3, `Provider`) | `` the first commit of `{repo}` landed, but `{branch}` is not visible yet; re-run this document to finish the scaffold `` |
| Rule suffix (S4) | appended to a refused-commit error: `` ; rules in force on `{branch}`: {types joined by ", "} ``. Omitted when the list is empty or the rules read itself fails. Never replaces the original message |
| Missing branch on a non-empty repository | unchanged: `` branch `{branch}` does not exist on `{repo}`; this tool never creates one `` |
| Fake record fields (F1) | `GitHubRepoRecord` gains `branches: Option<Vec<GitBranchName>>` and `default_branch: Option<GitBranchName>`, both `#[serde(default, skip_serializing_if = "Option::is_none")]`. `None` branches = legacy "initialised, every branch exists"; `Some([])` = empty; `Some(list)` = exactly those. `None` default branch = `main` |
| Fake helper (F1) | `FakeState::with_empty_repo(self, repo: &GitHubRepo, visibility: RepoVisibility, default_branch: Option<&GitBranchName>) -> Self` (ours, `branches: Some([])`) |
| Fake create (F1) | fake `github.repo.ensure`'s create path inserts `branches: Some([])`, `default_branch: None` |
| Tracked document (D1) | `workflows/github-new-repository-scaffold.yaml` |
| Its fake state (D1) | `workflows/fixtures/state/new-repository-empty-trunk.json` (the repository exists, ours, empty, default branch `trunk`) |
| Its test (D1) | `crates/willikins-cli/tests/new_repository_scaffold_document.rs` |
| Document's scaffold | `branch: main`, `marker: .willikins-scaffold`, files `.editorconfig` (`root = true\n`) and `README.md` (`# Scaffolded by willikins\n`), message `chore: scaffold the repository`. Root file: `.editorconfig` |
| Live cycle (L1) | `crates/willikins-providers-github/tests/live_new_repository_cycle.rs`, own `[[test]]` with `required-features = ["live-tests"]`, `#[ignore]`, `WILLIKINS_LIVE_TESTS=1`. Repositories `willikins-newrepo-<unix-seconds>-a`, `-b`, `-c` in `Willikins-Test`, private |
| Live seed files | `.tool+versions` (`rust 1.0\n`), `README.md` (`# probe\n`), `docs/guide.md` (`guide\n`); marker `.willikins-scaffold`; root file `.tool+versions` (exercises `%2B`) |
| Characterization snapshot | byte-identical except one new entry for the D1 document |

## Sources, verbatim

Fetched 2026-10-05 with `curl`: GitHub's OpenAPI description
`github/rest-api-description/main/descriptions/api.github.com/api.github.com.json` (version 1.1.4); `github/docs`
markdown under `content/` and `data/reusables/`; `github/docs` `src/graphql/data/fpt/schema.docs.graphql`;
`github/docs` `src/github-apps/data/fpt-2022-11-28/fine-grained-pat-permissions.json`.

**Creating a repository** (`POST /orgs/{org}/repos`, responses `201, 403, 422`). Body keys: `allow_auto_merge,
allow_merge_commit, allow_rebase_merge, allow_squash_merge, auto_init, custom_properties, delete_branch_on_merge,
description, gitignore_template, has_downloads, has_issues, has_projects, has_wiki, homepage, is_template,
license_template, merge_commit_message, merge_commit_title, name, private, squash_merge_commit_message,
squash_merge_commit_title, team_id, use_squash_pr_title_as_default, visibility`. There is **no default-branch key**.
`auto_init`: "Pass `true` to create an initial commit with empty README." (default `false`).

**An empty repository refuses the Git database.**
- `POST /repos/{owner}/{repo}/git/refs`: "You are unable to create new references for empty repositories, even if the
  commit SHA-1 hash used exists. Empty repositories are repositories without branches."
- `content/rest/guides/using-the-rest-api-to-interact-with-your-git-database.md`: "The REST API will return a `409
  Conflict` if the Git repository is empty or unavailable. An unavailable repository typically means GitHub is in the
  process of creating the repository. For an empty repository, you can use the `PUT
  /repos/{owner}/{repo}/contents/{path}` REST API endpoint to create content and initialize the repository so you can
  use the API to manage the Git database."
- `GET /repos/{owner}/{repo}/git/ref/{ref}`: "If the `:ref` doesn't match an existing ref, a `404` is returned."
  Responses `200, 404, 409`.

**GraphQL cannot start a branch.** `CreateCommitOnBranchInput.expectedHeadOid: GitObjectID!` — "The git commit oid
expected at the head of the branch prior to the commit" (non-nullable). `CreateRefInput.oid: GitObjectID!` — "The
GitObjectID that the new Ref shall target. Must point to a commit." `createCommitOnBranch`: "Commits made using this
mutation are automatically signed by GitHub if supported and will be marked as verified in the user interface."

**The Contents API** (`PUT /repos/{owner}/{repo}/contents/{path}`, responses `200, 201, 404, 409, 422`): "Creates a
new file or replaces an existing file in a repository." Required `message`, `content` ("The new file content, using
Base64 encoding."). `sha`: "**Required if you are updating a file**. The blob SHA of the file being replaced."
`branch`: "The branch name. Default: the repository’s default branch." `committer`: "Default: the authenticated
user." "If you use this endpoint and the "Delete a file" endpoint in parallel, the concurrent requests will conflict
and you will receive errors. You must use these endpoints serially instead."

**The default branch.**
- `data/reusables/branches/new-repo-default-branch.md`: "When you create a repository with content on GitHub, GitHub
  creates the repository with a single branch. This first branch in the repository is the default branch."
- `content/pull-requests/reference/branches.md`: "By default, GitHub names the default branch `main` in any new
  repository."
- `content/organizations/.../managing-the-default-branch-name-for-repositories-in-your-organization.md`: "When a member
  of your organization creates a new repository in your organization, the repository contains one branch, which is
  the default branch. You can change the name that GitHub uses for the default branch in new repositories that
  members of your organization create." Set in the organisation's settings, under "Repository default branch".
- `.../changing-the-default-branch.md`, Prerequisites: "To change the default branch, your repository must have more
  than one branch."
- `PATCH /repos/{owner}/{repo}` `default_branch`: "Updates the default branch for this repository."

**Rules and protection.**
- `GET /repos/{owner}/{repo}/rules/branches/{branch}`: "Returns all active rules that apply to the specified branch.
  The branch does not need to exist; rules that would apply to a branch with that name will be returned. All active
  rules that apply will be returned, regardless of the level at which they are configured (e.g. repository or
  organization)." Rule `type`s in the schema include `creation`, `update`, `deletion`, `pull_request`,
  `required_signatures`, `required_status_checks`, `non_fast_forward`, `commit_message_pattern`,
  `file_path_restriction`, `max_file_size`.
- `PUT /repos/{owner}/{repo}/branches/{branch}/protection`: "Protected branches are available in public repositories
  with GitHub Free and GitHub Free for organizations, and in public and private repositories with GitHub Pro, GitHub
  Team, GitHub Enterprise Cloud, and GitHub Enterprise Server." "Protecting a branch requires admin or owner
  permissions to the repository."
- `POST /repos/{owner}/{repo}/rulesets`: "Create a ruleset for a repository."

**Signing.** `about-commit-signature-verification.md`: "GitHub will automatically use GPG to sign commits you make
using the web interface." "Signature verification for bots will only work if the request is verified and
authenticated as the GitHub App or bot and contains no custom author information, custom committer information, and
no custom signature information, such as Commits API." Nothing says whether a Contents API commit made with a
personal token is signed (verify item 4).

**Fine-grained permissions** (`fine-grained-pat-permissions.json`, `{permission, verb, path, access,
additional-permissions}`):

| Endpoint | Permission(s) |
| --- | --- |
| `POST /orgs/{org}/repos` | `administration` write (additional: true); `repository_creation` write (additional: true) |
| `DELETE /repos/{owner}/{repo}` | `administration` write |
| `PATCH /repos/{owner}/{repo}` | `administration` write |
| `PUT /repos/{owner}/{repo}/contents/{path}` | `contents` write (additional: true); `workflows` write (additional: true) |
| `GET /repos/{owner}/{repo}`, `GET .../branches`, `GET .../rulesets` | `metadata` read |
| `POST /repos/{owner}/{repo}/rulesets`, `PUT .../branches/{branch}/protection` | `administration` write |

`RepoPath` already refuses `.github/workflows/...` (3g decision (e)), so no willikins write can need `workflows`.

## Decisions

### (a) How the first branch comes to exist: the scaffold's own first write, through the Contents API

The candidates, each against the sources above:

| Route | Verdict |
| --- | --- |
| `auto_init: true` on create | **Rejected.** Its README collides with any scaffold that seeds `README.md` (3g decision (c) refuses). See "The handoff's recorded answer" |
| Git database (`blobs`/`trees`/`commits`/`refs`) | **Impossible.** "unable to create new references for empty repositories"; `409` on an empty repository |
| GraphQL `createCommitOnBranch` / `createRef` | **Impossible.** Both need an existing commit oid (`GitObjectID!`) |
| Template repository (`POST /repos/{template}/generate`) | **Rejected.** The scaffold's content would live in another repository, not in the document (templates are privileged document content, design doc "Templates") |
| Contents API `PUT` of one seed file, then `createCommitOnBranch` | **Chosen.** The route GitHub's own guide names for an empty repository |

On an empty repository, `ensure`:

1. Checks that `branch` equals the repository's `default_branch` (decision (c)), or refuses, having written nothing.
2. `create_first_file` with the root file (the byte-order-smallest path in `files`) and the root commit message
   (SHARED VALUES), on `branch`.
3. Whatever the `PUT` answered, **re-observes** and decides only from that read. It never decides from the `PUT`'s own
   body.
   - `Absent { head, already_equal }`: the root file is byte-equal, so it is in `already_equal`. The existing loop
     lands every other file plus the marker as one `createCommitOnBranch`, compare-and-swapped on `head`.
   - Still empty after a `409`: wait and retry the `PUT` (SHARED VALUES waits). Otherwise, still empty after a `2xx`:
     poll the re-observe (ref visibility lags creation; the 3g cycle saw it). Exhausted: the "not visible yet" error.
   - Still empty after any other failure: report the original failure.
   - `Present` or `Foreign`, or a conflict: as today.

**Why it converges.** A `PUT` without `sha` can only create, never replace ("Required if you are updating a file"),
so `Http::put`'s own retries, and a `PUT` that landed although its response was lost, are both harmless: a repeated
attempt on an existing file fails and the re-observe sees the file. A run that stops between the two writes leaves a
non-empty repository with no marker and one seed file byte-equal to what the document renders; the next run reads
`Absent` with that file in `already_equal` and finishes with one commit. Nobody else's content can be overwritten:
the existing seed rules (refuse a differing path) apply to the second write exactly as before.

**The cost, stated.** A scaffold on a new repository is two commits, not one; the first is made through the REST API
and may not be signed (verify item 4). `changed` is `true`. A one-file scaffold's second commit holds only the marker.

### (b) The read table: a repository that does not exist yet, or is empty, is `Absent`

`get_branch_head` stays the **first** call, so a repository with the branch issues exactly the requests it does today
and every existing mock's `.expect(n)` holds. Only when it fails:

| `git/ref/heads/{branch}` | `GET /repos/{owner}/{name}` | `GET .../branches?per_page=1` | `read` | `ensure` |
| --- | --- | --- | --- | --- |
| `200` | not called | not called | as today | as today |
| `404` or `409` | `404` | not called | `Absent { predicted }` | `NotFound`, repository-absent message |
| `404` or `409` | `200`, `default_branch == branch` | `[]` | `Absent { predicted }` | decision (a) |
| `404` or `409` | `200`, `default_branch != branch` | `[]` | `Conflict`, mismatch message | the same `Conflict`, nothing written |
| `404` | `200` | non-empty | `NotFound`, missing-branch message (unchanged) | the same |
| `409` | `200` | non-empty | `Provider`, unavailable message | the same |
| `404` or `409` | `200`, `default_branch` absent or not a `GitBranchName` | — | `Provider` (static message naming `repo`) | the same |
| any other error | — | — | `to_tool_error` | the same |

A `301` from `GET /repos` (renamed away) is "any other error" here and fails loudly, never `Absent`.

**Why `Absent` for a repository that does not exist.** On a fresh document `github.repo.ensure` creates it in the same
plan, so the scaffold reads against a predicted repository. GitHub answers `404` both for a missing repository and
for one this token cannot see, so the two cannot be told apart. A plan-time error there makes every new-repository
document unplannable. Milestone 3j decision (b3) made the same call for Doppler, for the same reason.

**What this costs.** A document that scaffolds into an *existing* repository named with a typo now plans `Create` and
fails at apply with `NotFound`, not at plan. The mitigation already exists: bind `repo` from `github.repo.get`, which
fails at plan on a `404` (the operator's iOS app document does this for its monorepo). The tool's rustdoc says so.

**Empty is GitHub's own definition:** "Empty repositories are repositories without branches", read with one
`per_page=1` listing, never the `size` field ("Size is calculated hourly").

### (c) The default branch: GitHub names it; the document states it; the scaffold checks it

- `POST /orgs/{org}/repos` takes no default-branch key. GitHub names the first branch from the organisation's
  "Repository default branch" setting, `main` unless changed. Renaming it needs a second branch ("your repository
  must have more than one branch"). So willikins does not choose it, and has no cheap way to change it at birth.
- The document writes the branch as a literal (`branch: main`). It must: `branch` is part of the scaffold's key, and
  a key bound to a not-yet-known output is `PlanError::KeyUnknown` at plan. `github.repo.ensure` therefore gains no
  `default_branch` output.
- On an empty repository the scaffold compares the literal with `default_branch` and refuses on a mismatch, at `read`
  when the repository already exists, and at `ensure` before any write on a fresh document. The message says both
  names and both remedies.
- This plan does not rely on `PUT .../contents` with a `branch` other than the default on an empty repository; that
  is undocumented. The live cycle probes it (step 9, repository `-c`) and records what happens. If GitHub creates that
  branch and makes it the default, a later addendum may lift the refusal so the document owns the name (policy lives
  in the workflow). Not before the probe.

### (d) Idempotence and drift

- **Repository.** Unchanged: `github.repo.ensure` reads `Present` for its own repository (topic) at the requested
  visibility, empty or not, and refuses a visibility mismatch.
- **Scaffold.** Unchanged once landed: the marker alone decides `Present` (3g decision (c)). A seed is a seed: later
  edits to seeded files are the repository's own.
- **Half-landed.** Decision (a)'s convergence argument.
- **Drift of the default branch after landing** (someone renames `main`): the scaffold's `branch` no longer exists on a
  non-empty repository, so `plan` fails with the unchanged missing-branch `NotFound`. Loud, never a second scaffold.
- **Second run of the document:** `repo` `NoOp`, `scaffold` `NoOp`, no write (acceptance 9).

### (e) Branch protection on a new repository: not a tool in this milestone; the order is fixed

- **No `github.ruleset.ensure` here.** A ruleset is a policy surface of its own (rules, bypass actors, enforcement),
  needs `administration` write, and is unavailable on private repositories of a free organisation. It deserves its own
  plan. The coordinator opens a todo for it.
- **The order is fixed now.** A protection or ruleset tool must consume the scaffold's `branch` output, so the data edge
  orders it after the seed. A rule requiring pull requests that existed before the seed would refuse the seed itself.
- **Organisation rulesets apply from birth.** "All active rules that apply will be returned, regardless of the level
  at which they are configured (e.g. repository or organization)". A `creation`, `update`, `pull_request` or
  `required_signatures` rule targeting the default branch can refuse the very first commit, unless this token's user
  is a bypass actor, which the rules endpoint does not say. So the scaffold does not pre-flight rules at plan time (it
  would refuse falsely for a bypass actor). Instead (task S4), when a write is refused and the re-read shows nothing
  landed, the error names the rule types in force. Types only, never a body.

### (f) The trust model

- **The sandbox needs nothing new.** The sandbox PAT already creates, writes and deletes repositories in
  `Willikins-Test` (3g's live scaffold cycle, milestone 2's write cycle).
- **The real organisation needs a broader credential than any willikins holds today.** Creating a repository needs
  `administration` write at organisation scope. A token restricted to selected repositories cannot, by its nature,
  name one that does not exist yet (verify item 7), so the scaffold's `contents` write on the new repository also
  implies organisation-wide access. Such a token can `DELETE /repos/{owner}/{repo}` on every repository in the
  organisation. Better options, in order: a GitHub App installed on the organisation (installation tokens expire in an
  hour, are not the operator's identity, and are where milestone 3's multi-org note already points); or a
  fine-grained PAT owned by the organisation, stored in Doppler, bound through the `token` port, and used by no other
  document. This milestone ships neither. It is the operator's decision ("Needs the operator").
- **Never the operator's `gh`.** `no_gh_writes_guard` stays green; the live cycle uses the sandbox PAT through
  `Http`, as every cycle does.

## Why willikins never needs a local clone (the operator's question)

The question, 2026-10-04: what happens for a new repository, or one that is not cloned locally. Answer: nothing
different, because willikins never touches a working tree. Exactly why:

1. **Every read is the REST Git database, pinned to one commit.** `git/ref/heads/{branch}` gives the head;
   `git/commits/{sha}` gives its root tree; `git/trees/{sha}` (never recursive) walks only the directories a declared
   path passes through. The only blob willikins ever downloads is its own marker.
2. **Content is compared without downloading it.** A seed file's bytes are rendered in memory from the document
   (`repo.file.render`), its git blob sha is computed locally (`git_blob_sha`), and compared with the sha the tree
   already lists.
3. **Every write is a server-side commit.** `createCommitOnBranch` sends the file contents, GitHub builds the tree and
   the commit, signs it, and moves the branch only if its head still equals `expectedHeadOid`. On an empty repository,
   the Contents API's `PUT` does the same for one file. No object is built on willikins' host.
4. **Nothing in a repository is ever executed.** No build, no hook, no script. A clone is only needed to run things,
   and willikins' closed tool set runs nothing (design doc: "No tool may reduce to ... 'run a command'").
5. **A clone would break the trust model.** willikins is remote-first: an MCP server on its own host. A clone needs a
   git credential on that host's disk and a writable working tree, and a push from it is unsigned unless willikins
   held a signing key too. The API route keeps the credential in memory, scoped per request, and gets GitHub's
   signature for free.

What would need a clone, willikins does not do: building the scaffolded code (3g's verify item 8, a `bazel build` over
rendered files, is the operator's check, not willikins'), rewriting history, or merging. A repository that exists on
GitHub but has never been cloned anywhere is the same case as the operator's monorepo today.

## Acceptance tests

Client (S1, unit tests in `client.rs` with `MockProvider`, `match_query` and `.expect(1)` on every non-5xx mock):
1. `has_any_branch`: the exact request line; `[]` → `false`; `[{"name": "main"}]` → `true`; `404` → `Err` with the
   status. `get_repo` parses a body with and without `default_branch`.
2. `create_first_file`: the request is `PUT` on the encoded path, the body is exactly `{message, content, branch}`
   (`json_body`, so an extra key fails), `content` is the standard base64 of the file's bytes. Paths `a+b/c@d.txt` →
   `/contents/a%2Bb/c%40d.txt`. `201` → `Ok`. `409`, `422` and `404` → `Err` with the status and exactly
   `FIRST_FILE_FAILURE_MESSAGE`; a marker string in each error body never appears in the error's message or `Debug`.
   `401`/`403` keep the shared fixed messages.

Read table (S2, `tests/scaffold_ensure_mock.rs`, one test per row of decision (b), each mock with `.expect(n)`):
3. Every row of the table, including: a missing repository and an empty one each read `Absent` with the pass-through
   outputs; the mismatch row's `Conflict` names `repo`, `branch` and the default; the non-empty missing-branch row keeps
   today's message; the `409`-non-empty row is `Provider`; no row after `200` issues `GET /repos` or the branches call
   (`.expect(0)`). `read_reports_not_found_when_the_branch_does_not_exist` gains the two mocks it now needs and keeps
   its assertion.

Ensure on an empty repository (S3, mock tests, a recording `Sleeper`):
4. **Fresh.** Empty repository, three files: exactly one `PUT` (the root file), then one `createCommitOnBranch` whose
   `additions` are the other two files plus the marker and whose `expectedHeadOid` is the root commit's head.
   `changed: true`.
5. **Resume after the root commit.** The repository already holds only the root file, byte-equal, no marker: no
   `PUT`; one `createCommitOnBranch` without the root file. `changed: true`.
6. **Lost response.** The `PUT` answers `502` four times (`Http::put`'s retries) while the re-observe then sees the root
   file: the scaffold finishes. A `PUT` answering `422` (the file already exists) with a re-observe showing it: the same.
7. **Unavailable.** The `PUT` answers `409` twice, then `201`: two recorded waits of `UNAVAILABLE_WAIT`, then the
   normal commit. Five `409`s: `Err`, `MAX_UNAVAILABLE_ATTEMPTS` attempts, no commit.
8. **Ref lag.** After a `201`, the re-observe reads empty three times, then `Absent`: three recorded waits of
   `REF_VISIBLE_WAIT`. Eleven empty reads: the "not visible yet" `Provider` error, no commit.
9. **Refusals write nothing.** Mismatched default branch, repository absent, a one-file scaffold whose single file is
   the root (it then commits only the marker), and the shape refusals: no `PUT`, no `POST /graphql` (`.expect(0)`),
   except the one-file case's single commit.
10. **Nothing else changes.** Every existing `scaffold_ensure_mock.rs` test passes with its mocks unchanged except
    the one named in acceptance 3.

Rules diagnostic (S4):
11. A refused `createCommitOnBranch` whose re-read shows an unmoved head, with rules `[{"type": "pull_request"},
    {"type": "required_signatures"}, {"type": "Pull Request!"}]`: the error is the original message plus `; rules in
    force on `main`: pull_request, required_signatures, unrecognised`. Rules read failing: the original message
    alone. A refused first-file `PUT` (non-409) gets the same suffix. The rules body's other fields never appear.

Fake (F1):
12. `GitHubRepoRecord` with neither new field round-trips byte-identically through `FakeState`'s JSON; every
    existing fixture under `workflows/fixtures/state/` loads unchanged. Fake `github.repo.ensure`'s create records
    `branches: Some([])`. The fake scaffold follows decision (b)'s table for the cases it models (no record; empty
    with the default; empty with another branch; `Some(list)` without the branch; `None` = legacy) and records the
    branch after an empty-repository ensure.
13. `scaffold_fake_agrees_with_live.rs` gains one row per modelled case, fake and live answering the same shape.

Document (D1, `crates/willikins-cli/tests/new_repository_scaffold_document.rs`, over the fake):
14. **Fresh.** An empty `FakeState`: run 1 plans `repo` `Create` and `scaffold` `Create` (no `PlanError`). Apply
    (approving the scaffold's irreversible class the way existing CLI tests do), then run 2: both `NoOp`, and the fake
    holds both files, the marker and branch `main`.
15. **Default branch `trunk`.** `new-repository-empty-trunk.json`: run 1 fails at plan with the mismatch `Conflict`
    attributed to `scaffold`.
16. `scaffold.repo` binds `repo.repo` (a `Binding::Step`), pinning the order.
17. The characterization snapshot gains exactly the D1 entry.

Live (L1, run once by the coordinator):
18. The live new-repository cycle below, every step passing.

Guards (all tasks):
19. `secret_literal_guard`, `no_gh_writes_guard`, `no_certificate_writes_guard` green; no operator name in any
    tracked file (the local hooks).

## The live new-repository cycle (written by L1, run once by the coordinator)

`crates/willikins-providers-github/tests/live_new_repository_cycle.rs`, with credentials sourced only in the command
that runs it:

```text
source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
  cargo test -p willikins-providers-github --features live-tests --test live_new_repository_cycle \
  -j 2 -- --ignored --nocapture
```

1. Refuse any org but `Willikins-Test`. Count its repositories (read-only); refuse to start if a
   `willikins-newrepo-` repository exists. Arm one guard that deletes `-a`, `-b` and `-c` on every exit path, before
   any write.
2. **Plan-time fix, live.** The scaffold's `read` on `-a`, which does not exist: `Absent`.
3. `github.repo.ensure` creates `-a` through the tool: `read` `Absent`, `ensure` `changed: true`. Raw reads record
   `default_branch` (verify item 2) and that `branches?per_page=1` is `[]`.
4. A raw `GET git/ref/heads/{default}` on `-a`: print the status, `404` or `409` (verify item 1).
5. The scaffold's `read` on `-a` with branch `not-the-default`: `Conflict` naming both branches; `branches` still `[]`.
6. The scaffold on `-a` with the live seed files: `read` `Absent`; `ensure` `changed: true`. Independent raw reads:
   the branch is the default; exactly two commits; the root commit has no parent and its tree holds exactly
   `.tool+versions` (verify item 5, `%2B`); the second commit's parent is the root and its tree holds every path as
   `100644` with the expected blob sha, plus the marker; print both commits' `verification.verified` (verify item 4).
7. **Converged.** `read` `Present`; `ensure` `changed: false`; head unchanged. `github.repo.ensure` again:
   `changed: false`.
8. **Resume.** Create `-b` through `github.repo.ensure`; a raw `PUT .../contents/.tool+versions` with the same
   content (a run that stopped after its first write); the scaffold's `ensure`: `changed: true`, exactly one new
   commit, whose tree adds the other two files and the marker and leaves `.tool+versions`' blob unchanged.
9. **Probe** (verify item 3). Create `-c` through `github.repo.ensure`; a raw `PUT .../contents/probe.txt` with
   `branch: willikins-probe`. Print one line: the status, whether `willikins-probe` exists afterwards, and the
   repository's `default_branch`. No assertion beyond "nothing outside `-c` changed".
10. One raw `GET .../rules/branches/{default}` on `-a`: it parses as a list; print its length only.
11. Delete `-a`, `-b`, `-c`; each re-reads `404`; the repository count equals step 1's. Disarm the guard.

Nothing prints a token, a body, or any repository but these three.

## Verify before relying on them

1. **What `git/ref/heads/{branch}` answers on an empty repository**, `404` or `409`. Decision (b) handles both; record
   which (cycle step 4) in `docs/solutions/providers/`.
2. **`GET /repos` on an empty repository reports `default_branch`**, equal to the organisation's setting. Cycle step 3.
3. **What `PUT .../contents` with a non-default `branch` does on an empty repository.** Cycle step 9. Decides whether
   decision (c)'s refusal can later be lifted.
4. **Whether a Contents API commit made with a PAT is signed.** Cycle step 6. If unsigned, an organisation rule
   requiring signatures refuses a new repository's first commit (risk below).
5. **`%2B` and `%40` in the contents path decode to `+` and `@`.** Cycle step 6 (`%2B`); `%40` by mock only.
6. **The root file's blob sha equals `git_blob_sha(content)`**, so the root file lands in `already_equal`. Cycle steps 6
   and 8.
7. **Fine-grained PAT facts**: a token limited to selected repositories cannot cover one created after it; an
   "All repositories" token covers repositories created later; which of `administration` and
   `repository_creation` `POST /orgs/{org}/repos` needs (the permissions file lists both with
   `additional-permissions: true`). Settled before any real-org credential is chosen, not by this milestone.
8. **Whether an organisation ruleset's `creation` rule refuses the first commit of an empty repository.** Not probed
   (it would need an org ruleset in the sandbox). Task S4's diagnostic names it if it happens.
9. **A branch name containing `/` in `rules/branches/{branch}`.** The diagnostic is best-effort and swallows its own
   failure; a default branch rarely contains one.

## Gates

Per task, scoped, never the full workspace gate (the coordinator runs that). Before **each** cargo command, wait for 3
consecutive seconds in which both `pgrep -x cargo` and `pgrep -f cargo-sweep` print nothing, polled every second,
then start the command in the same shell. Use `-j 2` and `RUST_TEST_THREADS=2`. Run fmt, clippy and tests as separate
commands, in the background with a 600,000 ms timeout, and read the output file's body, never piped through `tail` or
`tee`. A linker "missing .rcgu.o" or `E0463` means the host's cargo-sweep ran: `cargo clean -p <crate>` and rebuild.
Never edit tracked files while cargo builds.

```
cargo fmt --all --check
cargo clippy -p <crate> [-p <crate>...] --all-targets -j 2 -- -D warnings
RUST_TEST_THREADS=2 cargo test -p <crate> [-p <crate>...] -j 2 --no-fail-fast
```

No task changes `willikins-types`, so `cargo check -p willikins-types` is not needed per task.

**After milestone 3k.** If 3k has landed (the portfolio order puts it first), every new or edited non-gated
integration test named here lives at `crates/<c>/tests/it/<stem>.rs` with a `mod <stem>;` line in `tests/it/main.rs`
(`scaffold_ensure_mock`, `scaffold_fake_agrees_with_live`, `new_repository_scaffold_document`), a `--test <stem>`
gate becomes `--test it <stem>::`, and the characterization snapshot is
`crates/willikins-dsl/tests/it/snapshots/it__acceptance__characterization_of_every_document.snap`. The gated live
target L1 stays top-level with its own `[[test]]` entry, as 3k decision (d3) keeps every `live-tests` target.

Commit as soon as a commit's scoped gates are green, with `git commit --only <paths>`. Local hooks refuse any commit
or message naming the operator's private setup: never `--no-verify`; rewrite with placeholders.

## Tasks

One lane at a time on `main`, in this order. Each task is test first, one behaviour per commit, green alone, and
commits by path with `git commit --only` (never `git add -A`, `commit -a`, stash, `checkout --` or `reset`), with the
implementer's own `Co-Authored-By` trailer. Nobody pushes.

| # | Task | Delegate to |
| --- | --- | --- |
| S1 | **Client calls** (SHARED VALUES client rows; trust boundaries 3, 4; acceptance 1, 2). One commit, `crates/willikins-providers-github/src/client.rs` plus one pattern in `src/tools/repo_ensure.rs`: `RepoBody.default_branch`, `has_any_branch`, `create_first_file` with its path encoder and body suppression, `pause`, and their unit tests. `repo_ensure.rs`'s `observe` destructures `RepoBody { visibility, topics, archived: _ }` exhaustively, so it gains `default_branch: _` (E0027 otherwise); its behaviour is unchanged, and `repo_get.rs` needs nothing (it does not destructure). Not yet called by any tool (`#[allow(dead_code)]` only if clippy demands it, removed in S2/S3). Scoped: `-p willikins-providers-github` | sonnet implements, opus attacks |
| S2 | **The read table** (decision (b), (c)'s read-time refusal; acceptance 3, 10). One commit: `scaffold_ensure.rs`'s `observe` gains `ScaffoldState::Empty { default_branch }` and `ScaffoldState::RepositoryAbsent`; `read` maps them per the table; `ensure` returns the repository-absent `NotFound` and, on `Empty`, a temporary `NotFound` naming this plan's task S3 (replaced there); the rustdoc (module doc, `observe`) states the new rows and the `github.repo.get` mitigation. `tests/scaffold_ensure_mock.rs`: one test per row; the one existing test gains its mocks. Scoped: `-p willikins-providers-github` | sonnet implements, opus attacks |
| S3 | **Ensure on an empty repository** (decision (a); acceptance 4–9). One commit: `scaffold_ensure.rs`'s `ensure` initialises per decision (a), with the waits through `GitHubClient::pause`; mock tests with a recording `Sleeper`. Scoped: `-p willikins-providers-github` | sonnet implements, opus attacks |
| S4 | **Refusals name the rules in force** (decision (e); acceptance 11). One commit: `branch_rule_types` in `client.rs`, the suffix on a refused commit with an unmoved head and on a refused first-file `PUT`, mock tests. Scoped: `-p willikins-providers-github` | sonnet implements |
| F1 | **The fake models existence, emptiness and branches** (acceptance 12, 13). Commit 1, `crates/willikins-providers-fake`: the record fields, `with_empty_repo`, the fake `github.repo.ensure` create, the fake scaffold's table, and its own unit tests (those that never seeded a repository now seed one with `with_repo`, which keeps `branches: None`). Commit 2, `crates/willikins-providers-github/tests/scaffold_fake_agrees_with_live.rs` rows. Scoped: `-p willikins-providers-fake -p willikins-providers-github`, then `-p willikins-cli` and `-p willikins-dsl` tests to prove no other fake consumer moved. Also run the gitignored `operator_*` test targets of `willikins-cli` (run, never committed); report any that fail rather than editing them. The fake create's `"branches": []` appears in `--fake-state-out` dumps (`apply_and_journal.rs`, `smoke_parity.rs`); those tests reload the dump and never snapshot it, so no snapshot moves | sonnet implements, opus attacks |
| D1 | **The tracked document** (acceptance 14–17). One commit: `workflows/github-new-repository-scaffold.yaml` with a header comment naming its test, `workflows/fixtures/state/new-repository-empty-trunk.json`, `crates/willikins-cli/tests/new_repository_scaffold_document.rs`, and the characterization snapshot's one new entry (diff-reviewed). The same commit adds the document to the server's exact-set pins: both lists in `crates/willikins-server/tests/acceptance_13_trusted_directory.rs` (file names and the sorted `list_workflows` names) and the set in `crates/willikins-server/tests/image_contents.rs` (whose message counts the documents). Placeholders only (`example-org`). Scoped: `-p willikins-cli -p willikins-dsl`, plus `-p willikins-server --test acceptance_13_trusted_directory --test image_contents` | sonnet implements |
| L1 | **The live new-repository cycle** (the section above), written and compiling under `--features live-tests`, never run by the implementer. One commit: the test file and its `[[test]]` entry in `crates/willikins-providers-github/Cargo.toml`. Scoped: `cargo clippy -p willikins-providers-github --features live-tests --all-targets -j 2 -- -D warnings` | sonnet writes, coordinator runs once |
| X1 | **Adversarial pass**, recorded under `docs/research/2026-10-0x-m3l-adversarial-pass.md`, placeholders only. Every bypass becomes a test. At least four mutations restored from saved copies (`cmp` for byte identity). Priority targets: a `PUT` carrying `sha` (an overwrite path); a `PUT` on a repository that is not empty; a decision taken from the `PUT`'s own body instead of a re-read; a body fragment in any error; the first-file write on a branch other than the default; `Absent` where the table says an error (a `301`, a `409` on a non-empty repository); a second root commit on resume; a happy-path request that was not issued before this milestone; the fake disagreeing with live on any modelled row | opus |

Then the coordinator:
- runs L1 once and records verify items 1–6 here as addenda and in `docs/solutions/providers/`;
- runs the full gate;
- opens `todos/2026-10-05-github-ruleset-ensure.md` (decision (e)) and updates `docs/HANDOFF.md`'s open item on the
  operator's question with this plan's answer;
- marks this plan Completed.

## Risks

- **An organisation rule refuses the first commit.** A `creation`, `pull_request` or `required_signatures` rule at
  organisation level applies to a repository from birth. The scaffold cannot know whether its token's user bypasses
  it, so it fails at apply, not plan, and S4's suffix names the rule. If Contents API commits are unsigned (verify item
  4), `required_signatures` always refuses them; the remedy is then the operator's (a bypass for the willikins
  identity, or a GitHub App, whose commits GitHub signs).
- **A typo'd existing repository plans `Create`** (decision (b)). Fails at apply with `NotFound`, before any write.
  Mitigated by `github.repo.get`.
- **A mismatched default branch on a fresh document** is found at apply, after `github.repo.ensure` created the
  repository. Nothing else is written; re-running with the right branch converges.
- **Two commits on a new repository**, the first possibly unsigned. Stated, not hidden.
- **GitHub's eventual consistency right after creation** (`409` "unavailable", ref lag). Bounded waits, then a clear
  "re-run" error. Re-running converges.
- **The fake's legacy `branches: None`** keeps every existing seed working but cannot express a missing branch. Only
  seeds that opt into `Some(list)` are checked against live behaviour.
- **Privacy.** Placeholders only in tracked files; the hooks refuse otherwise, and are never bypassed.
- **Host contention.** If no 3-second quiet window comes within 40 minutes, the task stops and reports it.

## Needs the operator

1. **Before a real-organisation new repository (not for this milestone):** which credential creates repositories in
   the real organisation. Options, with their blast radius (decision (f)): a GitHub App installed on the organisation
   (recommended), or an organisation-owned fine-grained PAT with `administration` and `contents` write on all
   repositories, held in Doppler and bound through the `token` port. Either can delete any repository in the
   organisation; that is the price of creating one.
