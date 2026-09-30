//! The live GitHub **scaffold** cycle: milestone 3g, task L1, acceptance
//! test 13 of `docs/plans/2026-09-30-milestone-3g-file-writing.md`.
//!
//! `#[ignore]`, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_TESTS=1` -- exactly `tests/live_write_cycle.rs`'s own
//! gate, read only past that check. Compiled only under this crate's
//! `live-tests` feature (its own `[[test]]` entry in `Cargo.toml`), so a
//! plain `cargo test --workspace` never builds this file at all. The org
//! comes from `WILLIKINS_SANDBOX_GITHUB_ORG`; the repository is a fresh
//! `willikins-files-<unix-seconds>` this run creates and deletes, never a
//! fixed name (unlike `live_write_cycle.rs`'s own `REPO_NAME`), because two
//! runs of *this* cycle could otherwise collide on the same throwaway name.
//!
//! **This task writes the harness; it does not run it.** Per milestone
//! 3g's task table, sonnet writes L1, opus runs it once. The run command:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   cargo test -p willikins-providers-github --features live-tests \
//!   --test live_scaffold_cycle -- --ignored --nocapture
//! ```
//!
//! Nine steps, each named in the plan's own "The live scaffold cycle"
//! section:
//!
//! 1. Count the sandbox org's repositories and refuse to start if a
//!    `willikins-files-*` leftover from an aborted run already exists.
//! 2. Create `<org>/willikins-files-<unix-seconds>`, private, with
//!    `auto_init: true` -- a raw `POST` (`github.repo.ensure`'s own
//!    `create_repo` never sets `auto_init`, and an empty, uninitialized
//!    repository has no branch for `createCommitOnBranch` to land on).
//!    The delete guard is armed **before** this call, not after: a `POST`
//!    is never retried, so a transport failure here may still have
//!    created the repository.
//! 3. `github.scaffold.ensure` with three files (one nested) and marker
//!    `app/.willikins-scaffold`: `read` is `Absent`, `ensure` is
//!    `changed: true`, and an independent raw `GET` of the resulting
//!    commit and its tree (never through the tool) confirms exactly one
//!    new commit whose sole parent is the init commit, records
//!    `commit.verification.verified` (verify item 2), and confirms every
//!    file plus the marker landed as `100644` with the expected git blob
//!    sha.
//! 4. `read` is now `Present`; a second `ensure` is `changed: false`; the
//!    branch head is unchanged.
//! 5. A raw `PUT .../contents/{path}` edits one seeded file directly (a
//!    developer's edit, never through this crate): `read` stays `Present`
//!    (the marker alone decides -- decision (b) -- so a seed path is
//!    never re-read once the scaffold has landed), `ensure` is
//!    `changed: false`, and the edit survives untouched.
//! 6. A second scaffold (a different marker, `other/.willikins-scaffold`)
//!    whose `files` names the edited path with its *original* content:
//!    `read` refuses with `Conflict`, naming that path; the head is
//!    unchanged.
//! 7. A client-level `createCommitOnBranch` with a deliberately stale
//!    `expectedHeadOid` fails and the head is unchanged (verify item 4).
//! 8. One `read` through the bound `token` port, against an
//!    *uncredentialed* client -- so the read can only have succeeded
//!    through the port, not a second reading of `WILLIKINS_GITHUB_TOKEN`
//!    by the client itself.
//! 9. Guard: `DELETE` the repository, a following `GET` is `404`, and the
//!    org's repository count returns to step 1's.
//!
//! # Two design choices this task makes (docs/plans/2026-09-30-milestone-3g-file-writing.md's
//! own G1 addendum, note 3, left both to "whichever of G2/L1 lands first")
//!
//! **The five read/write methods on [`willikins_providers_github::GitHubClient`]
//! stay `pub(crate)`.** G2 already landed and did not widen them (it drives
//! everything through `github.scaffold.ensure`'s own `Tool` impl); this
//! harness follows the same answer rather than reopening it. Step 3's
//! tree/commit verification and step 7's stale-`expectedHeadOid` mutation
//! are both done as **raw, hand-built requests** against the shared
//! [`willikins_providers_http::Http`] client instead -- exactly the
//! established shape `tests/live_write_cycle.rs`'s own `repo_path` comment
//! and `tests/scaffold_ensure_mock.rs`'s own duplicated `blob_sha` already
//! use for a `pub(crate)` algorithm or path an external test crate cannot
//! reach. `blob_sha` and `expected_marker_content` below are this file's
//! own copies of `client::git_blob_sha` and `scaffold_ensure::marker_content`,
//! kept independent on purpose: step 3's whole point is an *independent*
//! confirmation, not a call back into what the tool itself already
//! believes. A second reason favours raw requests for step 7 specifically:
//! `create_commit_on_branch` deliberately collapses every GraphQL failure
//! to a fixed, body-free message (decision (b)'s "never echoes ... any
//! response body"), which is exactly right for the tool but would hide
//! the very shape verify item 4 asks this harness to record.
//!
//! **Step 8 reads `WILLIKINS_GITHUB_TOKEN` directly**, once, via
//! `std::env::var` -- the one thing `tests/live_write_cycle.rs`'s own doc
//! comment says its cycle never needs to do. This cycle does need to: a
//! bound `token` port requires an actual [`willikins_types::GitHubToken`],
//! and a [`willikins_providers_http::Credential`] has no sanctioned way to
//! hand its bytes to a caller outside that crate. The plan's own words
//! sanction exactly this ("the `token` port with the same sandbox PAT
//! resolved in-process"). The plaintext `String` is parsed into a
//! `GitHubToken` and dropped immediately; it is never pushed onto
//! [`Cycle::sweep`], printed, or formatted.
//!
//! # Redaction
//!
//! Nothing this file prints, records, or formats may contain the real
//! token's bytes or anything credential-shaped
//! ([`willikins_providers_github::CREDENTIAL_PATTERN`]'s two prefixes,
//! `ghp_`/`github_pat_`). Step 7's GraphQL response is recorded by key
//! names and the `errors[].type` field only, per decision (b)'s own
//! stricter rule for that endpoint -- never `errors[].message`, which can
//! carry a fragment of file content. [`sweep_for_secrets`] checks every
//! string this file kept for that purpose at the end, the same shape
//! `tests/live_write_cycle.rs`'s own `sweep_for_secrets` uses.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value as Json;
use sha1::{Digest, Sha1};

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_github::{
    CREDENTIAL_VAR, GitHubClient, GitHubScaffoldEnsure, credential_from_env, http_client,
    http_client_without_credential,
};
use willikins_providers_http::Http;
use willikins_types::{
    CommitHeadline, DomainType, GitBranchName, GitHubOrg, GitHubRepo, GitHubToken, ProjectSlug,
    RepoFile, RepoPath,
};

/// The token prefixes `willikins_providers_github::CREDENTIAL_PATTERN`
/// accepts. Nothing this file prints, records, or formats may contain
/// either.
const CREDENTIAL_PREFIXES: &[&str] = &["ghp_", "github_pat_"];

/// Every throwaway repository this cycle creates starts with this prefix
/// -- step 1 refuses to proceed if one already exists, and step 9 deletes
/// exactly the one this run created.
const REPO_PREFIX: &str = "willikins-files-";

/// The page size step 1 and step 9 list the org's repositories with. If a
/// single page ever comes back this full, the org may hold more
/// repositories than one page shows, and this harness's before/after
/// count would silently miscount -- so it refuses to proceed instead
/// (see [`list_repo_names`]). This harness implements no pagination.
const REPO_LIST_PAGE_SIZE: usize = 100;

/// The marker file's required first line -- this file's own copy of
/// `scaffold_ensure::MARKER_HEADER` (private to that module).
const MARKER_HEADER: &str = "managed-by: willikins";

/// The fixed mutation text `GitHubClient::create_commit_on_branch` always
/// sends -- this file's own copy, kept byte-identical to `client.rs`'s
/// private `CREATE_COMMIT_ON_BRANCH_MUTATION` constant, since step 7's
/// whole point is a *client-level* call this harness makes itself rather
/// than one it asks the tool to make.
const CREATE_COMMIT_ON_BRANCH_MUTATION: &str = "mutation($input: CreateCommitOnBranchInput!) { \
     createCommitOnBranch(input: $input) { commit { oid } } }";

/// How many times [`current_branch_head`] retries a `404` before giving
/// up -- GitHub's git database can briefly lag a repository's own
/// creation (`auto_init`'s first commit) or a just-landed commit becoming
/// visible at `git/ref/heads/{branch}`.
const MAX_HEAD_POLL_ATTEMPTS: u32 = 10;

/// The wait between [`current_branch_head`]'s own poll attempts.
const HEAD_POLL_INTERVAL: Duration = Duration::from_millis(300);

/// The test's own sink token. `SinkToken::new` is disallowed outside the
/// apply executor; a test opts in narrowly, the convention every such
/// test in this workspace uses (see `tests/live_write_cycle.rs`'s own
/// `sink_token`).
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn sink_token() -> SinkToken {
    SinkToken::new()
}

/// The current Unix time in whole seconds, for this cycle's own throwaway
/// repository name.
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock reads a time after 1970")
        .as_secs()
}

/// The sandbox org this cycle runs against, from `WILLIKINS_SANDBOX_GITHUB_ORG`
/// -- the same variable `tests/live_write_cycle.rs`'s own `sandbox_repo`
/// reads.
fn sandbox_org() -> GitHubOrg {
    let value =
        std::env::var("WILLIKINS_SANDBOX_GITHUB_ORG").expect("WILLIKINS_SANDBOX_GITHUB_ORG is set");
    GitHubOrg::parse(&value).expect("a valid GitHub org slug")
}

/// `/repos/{owner}/{name}`, the one path every raw call in this file
/// builds from -- the same shape `tests/live_write_cycle.rs`'s own
/// `repo_path` uses, for the same reason: it is `pub(crate)` inside
/// `client.rs` and this is a separate integration-test crate.
fn repo_path(repo: &GitHubRepo) -> String {
    format!("/repos/{}/{}", repo.owner(), repo.name())
}

/// `/repos/{owner}/{name}/contents/{path}`, for step 5's raw developer
/// edit.
fn contents_path(repo: &GitHubRepo, path: &RepoPath) -> String {
    format!("{}/contents/{}", repo_path(repo), path.as_str())
}

/// The same algorithm `client::git_blob_sha` computes (`pub(crate)`, so
/// this integration test crate cannot call it directly, exactly as
/// `tests/scaffold_ensure_mock.rs`'s own duplicate documents): SHA-1 over
/// `blob <byte length>\0<bytes>`.
fn blob_sha(content: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", content.len()));
    hasher.update(content);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(40);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// The marker file's expected content: this file's own copy of
/// `scaffold_ensure::marker_content` (private to that module), for the
/// same "independent confirmation" reason `blob_sha` above is copied
/// rather than called back into.
fn expected_marker_content(files: &[RepoFile]) -> String {
    let mut lines: Vec<(String, String)> = files
        .iter()
        .map(|file| {
            (
                file.path().as_str().to_string(),
                blob_sha(file.content().as_bytes()),
            )
        })
        .collect();
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    let mut content = String::from(MARKER_HEADER);
    content.push('\n');
    for (path, sha) in lines {
        content.push_str(&sha);
        content.push(' ');
        content.push_str(&path);
        content.push('\n');
    }
    content
}

/// The top-level keys of a JSON object, empty for anything else -- step
/// 7's own "recorded by key names only" rule.
fn top_level_keys(value: &Json) -> BTreeSet<String> {
    value
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// The three files step 3 seeds: two short, one nested one level deep
/// (`app/nested/child.txt`), and one padded toward the same order of
/// magnitude as Sample's own projected ~20 KB base64 payload -- an
/// observed-fine data point for verify item 5, not a claim about its
/// exact ceiling.
fn seed_files() -> Vec<RepoFile> {
    let filler = "x".repeat(15_000);
    vec![
        RepoFile::new(
            RepoPath::parse("app/root.txt").expect("a valid RepoPath"),
            "a root-level seed file\n",
        )
        .expect("valid RepoFile content"),
        RepoFile::new(
            RepoPath::parse("app/nested/child.txt").expect("a valid RepoPath"),
            "a nested seed file\n",
        )
        .expect("valid RepoFile content"),
        RepoFile::new(
            RepoPath::parse("app/filler.txt").expect("a valid RepoPath"),
            filler,
        )
        .expect("valid RepoFile content"),
    ]
}

/// A fresh repository identity in `org`: `willikins-files-<unix-seconds>`,
/// 26 characters (`ProjectSlug` allows 32).
fn build_repo_identity(org: GitHubOrg) -> GitHubRepo {
    let name = format!("{REPO_PREFIX}{}", unix_seconds());
    let slug = ProjectSlug::parse(&name)
        .expect("a `willikins-files-` prefix plus a decimal Unix timestamp is a valid ProjectSlug");
    GitHubRepo::new(org, slug)
}

/// Every repository name in `org`, one page only -- see
/// [`REPO_LIST_PAGE_SIZE`]'s own doc comment for why a full page refuses
/// rather than silently miscounting.
fn list_repo_names(raw: &Http, org: &GitHubOrg) -> Vec<String> {
    let body: Vec<Json> = raw
        .get(&format!(
            "/orgs/{org}/repos?per_page={REPO_LIST_PAGE_SIZE}&type=all"
        ))
        .unwrap_or_else(|err| {
            panic!(
                "listing `{org}`'s repositories failed (status {:?})",
                err.status
            )
        });
    assert!(
        body.len() < REPO_LIST_PAGE_SIZE,
        "`{org}` holds {REPO_LIST_PAGE_SIZE} or more repositories on one page; this harness's \
         before/after count needs pagination it does not implement, so it refuses to proceed \
         rather than risk silently miscounting"
    );
    body.into_iter()
        .filter_map(|repo| repo.get("name").and_then(Json::as_str).map(str::to_string))
        .collect()
}

/// `branch`'s current head commit sha on `repo`, polling past a `404`
/// ([`MAX_HEAD_POLL_ATTEMPTS`] times, [`HEAD_POLL_INTERVAL`] apart): a
/// repository's `auto_init` commit, and a commit this harness itself just
/// landed, can each take a moment to become visible at
/// `git/ref/heads/{branch}`.
fn current_branch_head(raw: &Http, repo: &GitHubRepo, branch: &GitBranchName) -> String {
    let path = format!("{}/git/ref/heads/{branch}", repo_path(repo));
    for attempt in 1..=MAX_HEAD_POLL_ATTEMPTS {
        match raw.get::<Json>(&path) {
            Ok(body) => {
                return body
                    .pointer("/object/sha")
                    .and_then(Json::as_str)
                    .unwrap_or_else(|| {
                        panic!(
                            "the ref response for `{branch}` on `{repo}` carried no `object.sha`"
                        )
                    })
                    .to_string();
            }
            Err(err) if err.status == Some(404) && attempt < MAX_HEAD_POLL_ATTEMPTS => {
                thread::sleep(HEAD_POLL_INTERVAL);
            }
            Err(err) => panic!(
                "GET of `{branch}`'s ref on `{repo}` failed (status {:?}) after {attempt} attempt(s)",
                err.status
            ),
        }
    }
    unreachable!("the loop above always returns or panics")
}

/// Deletes the cycle's repository on every exit path -- a panic, a failed
/// assertion, or an early return -- unless the test body already deleted
/// it explicitly and disarmed the guard. Byte-identical shape to
/// `tests/live_write_cycle.rs`'s own `DeleteGuard`, duplicated rather than
/// shared because the two files are separate integration-test crates.
struct DeleteGuard {
    http: Arc<Http>,
    repo: GitHubRepo,
    armed: bool,
}

impl DeleteGuard {
    fn new(http: Arc<Http>, repo: GitHubRepo) -> Self {
        Self {
            http,
            repo,
            armed: false,
        }
    }

    fn arm(&mut self) {
        self.armed = true;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for DeleteGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let already_panicking = std::thread::panicking();
        println!("guard: deleting `{}`", self.repo);
        match self.http.delete(&repo_path(&self.repo)) {
            Ok(()) => println!("guard: deleted `{}`", self.repo),
            Err(err) if err.status == Some(404) => {
                println!("guard: `{}` was already gone (404)", self.repo);
            }
            Err(err) => {
                let status = err.status;
                println!(
                    "guard: !!! LEFTOVER REPOSITORY `{}` !!! the guard's DELETE failed \
                     (status {status:?}); it must be deleted by hand",
                    self.repo
                );
                assert!(
                    already_panicking,
                    "the guard could not delete `{}` (status {status:?}); it must be deleted by hand",
                    self.repo
                );
            }
        }
    }
}

/// Everything one run of the cycle needs, past repository creation: a raw
/// channel for the independent `GET`s and the developer's own `PUT`, the
/// tool under test, the scaffold's own identity, and the sweep buffer
/// every produced string this file wants swept for secrets lands in.
struct Cycle {
    raw: Arc<Http>,
    tool: GitHubScaffoldEnsure,
    repo: GitHubRepo,
    branch: GitBranchName,
    marker: RepoPath,
    files: Vec<RepoFile>,
    message: CommitHeadline,
    /// The repository's init commit sha, captured once at creation --
    /// step 3's expected commit parent, step 7's own deliberately stale
    /// `expectedHeadOid`.
    init_head: String,
    sweep: Vec<String>,
}

impl Cycle {
    /// Print one line and keep it for the final redaction sweep: this
    /// file prints only step names and pass/fail, never a body.
    fn say(&mut self, line: &str) {
        println!("{line}");
        self.sweep.push(line.to_string());
    }

    /// Keep a produced string (an `Observation`'s `Debug`, a `ToolError`'s
    /// kind, a recorded key set) for the final sweep without printing it.
    fn note(&mut self, text: String) {
        self.sweep.push(text);
    }

    /// The scaffold's own inputs: `repo`, `branch`, `marker`, `files`,
    /// `message` -- everything but an optional bound `token`.
    fn scaffold_inputs(&self) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("repo").expect("`repo` is a valid port name"),
            Value::known(self.repo.clone()),
        );
        inputs.insert(
            PortName::parse("branch").expect("`branch` is a valid port name"),
            Value::known(self.branch.clone()),
        );
        inputs.insert(
            PortName::parse("marker").expect("`marker` is a valid port name"),
            Value::known(self.marker.clone()),
        );
        inputs.insert(
            PortName::parse("files").expect("`files` is a valid port name"),
            Value::known_list(self.files.clone()),
        );
        inputs.insert(
            PortName::parse("message").expect("`message` is a valid port name"),
            Value::known(self.message.clone()),
        );
        inputs
    }
}

/// Step 1: list the org's repositories and refuse to start if a
/// `willikins-files-*` leftover from an aborted run already exists --
/// that leftover is the operator's to delete by hand, not this run's to
/// sweep away.
fn step_1_refuse_if_leftover(raw: &Http, org: &GitHubOrg) -> Vec<String> {
    let names = list_repo_names(raw, org);
    let leftovers: Vec<&String> = names
        .iter()
        .filter(|name| name.starts_with(REPO_PREFIX))
        .collect();
    assert!(
        leftovers.is_empty(),
        "step 1: leftover repositories from an aborted run: {leftovers:?}; delete them by hand \
         before running this cycle"
    );
    println!(
        "step 1 (no `{REPO_PREFIX}*` leftover; `{org}` holds {} repositories): pass",
        names.len()
    );
    names
}

/// Step 2: `POST /orgs/{org}/repos` with `auto_init: true`, raw (never
/// through `github.repo.ensure`'s own `create_repo`, which never sets
/// `auto_init` and so never gives `createCommitOnBranch` a branch to
/// land on). Returns the repository's default branch and its init
/// commit's sha.
fn step_2_create(raw: &Http, repo: &GitHubRepo) -> (GitBranchName, String) {
    let body = serde_json::json!({
        "name": repo.name().to_string(),
        "visibility": "private",
        "auto_init": true,
    });
    let response: Json = raw
        .post(&format!("/orgs/{}/repos", repo.owner()), &body)
        .unwrap_or_else(|err| panic!("step 2: creating `{repo}` failed (status {:?})", err.status));
    let branch_name = response
        .get("default_branch")
        .and_then(Json::as_str)
        .unwrap_or_else(|| {
            panic!("step 2: the create response for `{repo}` carried no `default_branch`")
        });
    let branch = GitBranchName::parse(branch_name)
        .expect("GitHub's own default branch name is a valid GitBranchName");
    let init_head = current_branch_head(raw, repo, &branch);
    println!(
        "step 2 (created `{repo}` private with auto_init; default branch `{branch}`, init commit \
         `{init_head}`): pass"
    );
    (branch, init_head)
}

/// Step 3: `github.scaffold.ensure` creates the scaffold, and an
/// independent raw `GET` (never through the tool) confirms the landed
/// commit and tree match what this cycle asked for.
fn step_3_scaffold_creates(cycle: &mut Cycle) {
    let inputs = cycle.scaffold_inputs();
    let observation = cycle
        .tool
        .read(&inputs)
        .expect("step 3: read of a fresh scaffold");
    cycle.note(format!("step 3 read: {observation:?}"));
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 3: a fresh scaffold must read Absent"
    );

    let ensured = cycle
        .tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 3: ensure failed ({:?}): {}", err.kind, err.message));
    cycle.note(format!("step 3 ensured: changed={}", ensured.changed));
    assert!(
        ensured.changed,
        "step 3: the first ensure must create the scaffold"
    );

    let head_after = current_branch_head(&cycle.raw, &cycle.repo, &cycle.branch);
    assert_ne!(
        head_after, cycle.init_head,
        "step 3: the scaffold commit must move the branch head"
    );

    verify_landed_scaffold(cycle, &head_after);
}

/// The independent raw `GET`s `step_3_scaffold_creates` makes (never
/// through the tool) of the commit `head_after` names and its tree: split
/// out from that function on its own, purely to stay under
/// `clippy::too_many_lines` -- this is still step 3's own verification,
/// not a separately reusable helper.
fn verify_landed_scaffold(cycle: &mut Cycle, head_after: &str) {
    let commit: Json = cycle
        .raw
        .get(&format!("{}/commits/{head_after}", repo_path(&cycle.repo)))
        .unwrap_or_else(|err| {
            panic!(
                "step 3: GET of the scaffold commit failed (status {:?})",
                err.status
            )
        });
    let parents = commit
        .get("parents")
        .and_then(Json::as_array)
        .expect("step 3: the commit response carries a `parents` array");
    assert_eq!(
        parents.len(),
        1,
        "step 3: the scaffold commit must have exactly one parent"
    );
    assert_eq!(
        parents[0].get("sha").and_then(Json::as_str),
        Some(cycle.init_head.as_str()),
        "step 3: the scaffold commit's parent must be the init commit"
    );
    let verified = commit
        .pointer("/commit/verification/verified")
        .and_then(Json::as_bool);
    cycle.note(format!(
        "step 3 commit verification.verified = {verified:?} (verify item 2, recorded, not asserted)"
    ));

    let tree_sha = commit
        .pointer("/commit/tree/sha")
        .and_then(Json::as_str)
        .expect("step 3: the commit response names its tree sha")
        .to_string();
    let tree: Json = cycle
        .raw
        .get(&format!(
            "{}/git/trees/{tree_sha}?recursive=1",
            repo_path(&cycle.repo)
        ))
        .unwrap_or_else(|err| {
            panic!(
                "step 3: GET of the scaffold tree failed (status {:?})",
                err.status
            )
        });
    let entries = tree
        .get("tree")
        .and_then(Json::as_array)
        .expect("step 3: the tree response carries a `tree` array");
    let find = |path: &str| {
        entries
            .iter()
            .find(|entry| entry.get("path").and_then(Json::as_str) == Some(path))
    };

    for file in &cycle.files {
        let entry = find(file.path().as_str())
            .unwrap_or_else(|| panic!("step 3: `{}` is missing from the landed tree", file.path()));
        assert_eq!(
            entry.get("mode").and_then(Json::as_str),
            Some("100644"),
            "step 3: `{}` must land as a regular file",
            file.path()
        );
        assert_eq!(
            entry.get("sha").and_then(Json::as_str),
            Some(blob_sha(file.content().as_bytes()).as_str()),
            "step 3: `{}`'s landed content does not match what this cycle asked for",
            file.path()
        );
    }
    let marker_entry = find(cycle.marker.as_str()).unwrap_or_else(|| {
        panic!(
            "step 3: the marker `{}` is missing from the landed tree",
            cycle.marker
        )
    });
    assert_eq!(
        marker_entry.get("mode").and_then(Json::as_str),
        Some("100644"),
        "step 3: the marker must land as a regular file"
    );
    assert_eq!(
        marker_entry.get("sha").and_then(Json::as_str),
        Some(blob_sha(expected_marker_content(&cycle.files).as_bytes()).as_str()),
        "step 3: the marker's landed content does not match the expected format"
    );

    cycle.say("step 3 (scaffold creates one commit on the init commit, landing every file and the marker): pass");
}

/// Step 4: `read` is now `Present`; a second `ensure` is `changed: false`
/// and does not move the branch head.
fn step_4_second_ensure_converges(cycle: &mut Cycle) {
    let inputs = cycle.scaffold_inputs();
    let observation = cycle
        .tool
        .read(&inputs)
        .expect("step 4: read of the scaffold just created");
    cycle.note(format!("step 4 read: {observation:?}"));
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 4: a landed scaffold must read Present"
    );

    let head_before = current_branch_head(&cycle.raw, &cycle.repo, &cycle.branch);
    let ensured = cycle
        .tool
        .ensure(&inputs, &sink_token())
        .expect("step 4: a second ensure of a converged scaffold");
    cycle.note(format!("step 4 ensured: changed={}", ensured.changed));
    assert!(
        !ensured.changed,
        "step 4: a second ensure must report changed: false"
    );
    let head_after = current_branch_head(&cycle.raw, &cycle.repo, &cycle.branch);
    assert_eq!(
        head_before, head_after,
        "step 4: a second ensure must not move the branch head"
    );

    cycle.say(
        "step 4 (read is Present, a second ensure is changed: false, the head is unchanged): pass",
    );
}

/// Step 5: a raw `PUT .../contents/{path}` edits one seeded file directly
/// (a developer's edit, never through this crate). `read` stays
/// `Present`, `ensure` is `changed: false`, and the edit survives.
/// Returns the edited path and its new blob sha, for step 6.
fn step_5_developer_edit(cycle: &mut Cycle) -> (RepoPath, String) {
    let edited = cycle.files.first().expect("at least one seed file").clone();
    let current_sha = blob_sha(edited.content().as_bytes());
    let new_content = "a developer's direct edit replaces the original content\n".to_string();
    let put_body = serde_json::json!({
        "message": "test: a developer edits a seeded file directly",
        "content": STANDARD.encode(new_content.as_bytes()),
        "sha": current_sha,
        "branch": cycle.branch.as_str(),
    });
    let response: Json = cycle
        .raw
        .put(&contents_path(&cycle.repo, edited.path()), &put_body)
        .unwrap_or_else(|err| panic!("step 5: the raw PUT failed (status {:?})", err.status));
    let new_sha = response
        .pointer("/content/sha")
        .and_then(Json::as_str)
        .expect("step 5: the PUT response names the new blob sha")
        .to_string();
    assert_eq!(
        new_sha,
        blob_sha(new_content.as_bytes()),
        "step 5: the PUT response's blob sha must match the edited content"
    );

    let inputs = cycle.scaffold_inputs();
    let observation = cycle
        .tool
        .read(&inputs)
        .expect("step 5: read after a developer's direct edit");
    cycle.note(format!("step 5 read: {observation:?}"));
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 5: the scaffold must still read Present (the marker alone decides, never a seed path)"
    );
    let ensured = cycle
        .tool
        .ensure(&inputs, &sink_token())
        .expect("step 5: ensure after a developer's direct edit");
    cycle.note(format!("step 5 ensured: changed={}", ensured.changed));
    assert!(
        !ensured.changed,
        "step 5: ensure must not touch a landed scaffold's own seed files"
    );

    let head = current_branch_head(&cycle.raw, &cycle.repo, &cycle.branch);
    let commit: Json = cycle
        .raw
        .get(&format!("{}/commits/{head}", repo_path(&cycle.repo)))
        .unwrap_or_else(|err| {
            panic!(
                "step 5: GET of the current commit failed (status {:?})",
                err.status
            )
        });
    let tree_sha = commit
        .pointer("/commit/tree/sha")
        .and_then(Json::as_str)
        .expect("step 5: the commit response names its tree sha")
        .to_string();
    let tree: Json = cycle
        .raw
        .get(&format!(
            "{}/git/trees/{tree_sha}?recursive=1",
            repo_path(&cycle.repo)
        ))
        .unwrap_or_else(|err| {
            panic!(
                "step 5: GET of the current tree failed (status {:?})",
                err.status
            )
        });
    let entries = tree
        .get("tree")
        .and_then(Json::as_array)
        .expect("step 5: the tree response carries a `tree` array");
    let survives = entries
        .iter()
        .find(|entry| entry.get("path").and_then(Json::as_str) == Some(edited.path().as_str()))
        .and_then(|entry| entry.get("sha"))
        .and_then(Json::as_str);
    assert_eq!(
        survives,
        Some(new_sha.as_str()),
        "step 5: the developer's edit must survive ensure untouched"
    );

    cycle.say("step 5 (a developer's direct edit survives read/ensure unchanged): pass");
    (edited.path().clone(), new_sha)
}

/// Step 6: a second scaffold, a different marker, whose `files` names the
/// edited path with its *original* content -- `read` must refuse with
/// `Conflict`, naming that path, and the head must not move.
fn step_6_second_scaffold_conflicts(cycle: &mut Cycle, edited_path: &RepoPath) {
    let original = cycle
        .files
        .iter()
        .find(|file| file.path() == edited_path)
        .expect("the edited path is one of this cycle's own seed files")
        .clone();
    let other_marker = RepoPath::parse("other/.willikins-scaffold").expect("a valid RepoPath");

    let mut inputs = Inputs::new();
    inputs.insert(
        PortName::parse("repo").expect("`repo` is a valid port name"),
        Value::known(cycle.repo.clone()),
    );
    inputs.insert(
        PortName::parse("branch").expect("`branch` is a valid port name"),
        Value::known(cycle.branch.clone()),
    );
    inputs.insert(
        PortName::parse("marker").expect("`marker` is a valid port name"),
        Value::known(other_marker),
    );
    inputs.insert(
        PortName::parse("files").expect("`files` is a valid port name"),
        Value::known_list(vec![original]),
    );
    inputs.insert(
        PortName::parse("message").expect("`message` is a valid port name"),
        Value::known(cycle.message.clone()),
    );

    let head_before = current_branch_head(&cycle.raw, &cycle.repo, &cycle.branch);
    let err = cycle
        .tool
        .read(&inputs)
        .expect_err("step 6: a second scaffold naming an edited path must refuse");
    cycle.note(format!("step 6 error kind: {:?}", err.kind));
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "step 6: the refusal must be a Conflict"
    );
    assert!(
        err.message.contains(edited_path.as_str()),
        "step 6: the refusal must name the differing path `{edited_path}`"
    );
    let head_after = current_branch_head(&cycle.raw, &cycle.repo, &cycle.branch);
    assert_eq!(
        head_before, head_after,
        "step 6: a refused read must not move the branch head"
    );

    cycle.say("step 6 (a second scaffold naming the edited path with its original content conflicts): pass");
}

/// Step 7: a client-level `createCommitOnBranch`, built by hand against
/// `expectedHeadOid: cycle.init_head` -- long stale by this point -- must
/// fail, and the head must not move. Records the response's top-level
/// keys and `errors[0]`'s key set plus `type`, never `message` (verify
/// item 4).
fn step_7_stale_expected_head_fails(cycle: &mut Cycle) {
    let head_before = current_branch_head(&cycle.raw, &cycle.repo, &cycle.branch);
    let body = serde_json::json!({
        "query": CREATE_COMMIT_ON_BRANCH_MUTATION,
        "variables": {
            "input": {
                "branch": {
                    "repositoryNameWithOwner": format!("{}/{}", cycle.repo.owner(), cycle.repo.name()),
                    "branchName": cycle.branch.as_str(),
                },
                "fileChanges": {
                    "additions": [{
                        "path": "other/stale-expected-head-test.txt",
                        "contents": STANDARD.encode(b"this commit must never land\n"),
                    }],
                },
                "message": {"headline": "test: a stale expectedHeadOid must fail"},
                "expectedHeadOid": cycle.init_head,
            },
        },
    });

    match cycle.raw.post::<Json>("/graphql", &body) {
        Ok(response) => {
            let keys = top_level_keys(&response);
            cycle.note(format!("step 7 response top-level keys: {keys:?}"));
            let errors = response.get("errors").and_then(Json::as_array);
            let has_errors = errors.is_some_and(|list| !list.is_empty());
            let data_is_null = response.get("data").is_none_or(Json::is_null);
            assert!(
                has_errors || data_is_null,
                "step 7: a stale expectedHeadOid unexpectedly succeeded"
            );
            if let Some(first) = errors.and_then(|list| list.first()) {
                let error_keys = top_level_keys(first);
                let error_type = first.get("type").and_then(Json::as_str);
                cycle.note(format!(
                    "step 7 errors[0] keys: {error_keys:?}, type: {error_type:?} (verify item 4, recorded, not pinned)"
                ));
            }
        }
        Err(err) => {
            cycle.note(format!(
                "step 7 request failed at the transport/status level: status {:?} (verify item 4)",
                err.status
            ));
        }
    }

    let head_after = current_branch_head(&cycle.raw, &cycle.repo, &cycle.branch);
    assert_eq!(
        head_before, head_after,
        "step 7: a stale expectedHeadOid must not move the branch head"
    );

    cycle.say("step 7 (a client-level createCommitOnBranch with a stale expectedHeadOid fails and the head is unchanged): pass");
}

/// Step 8: one `read` through the bound `token` port, against an
/// *uncredentialed* default client -- so the read can only have
/// succeeded through the port. Reads `WILLIKINS_GITHUB_TOKEN` directly,
/// once (this module's own doc comment explains why), and drops the
/// plaintext the moment it is parsed into a `GitHubToken`.
fn step_8_token_port_read(cycle: &mut Cycle) {
    let token_plaintext =
        std::env::var(CREDENTIAL_VAR).expect("step 8: WILLIKINS_GITHUB_TOKEN is set");
    let bound_token = GitHubToken::parse(&token_plaintext)
        .expect("step 8: the sandbox token matches GitHubToken's grammar");
    drop(token_plaintext);

    let uncredentialed = Arc::new(GitHubClient::new(http_client_without_credential()));
    let tool = GitHubScaffoldEnsure::new(uncredentialed);

    let mut inputs = cycle.scaffold_inputs();
    inputs.insert(
        PortName::parse("token").expect("`token` is a valid port name"),
        Value::known(bound_token),
    );

    let observation = tool
        .read(&inputs)
        .expect("step 8: read through the bound token port");
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 8: the landed scaffold must still read Present through the bound token port"
    );

    cycle.say("step 8 (one read through the bound token port, against an uncredentialed default client): pass");
}

/// Step 9: `DELETE` the repository, confirm the following `GET` is
/// `404`, disarm the guard, and confirm the org's repository count
/// returns to step 1's.
fn step_9_delete_and_recount(cycle: &mut Cycle, guard: &mut DeleteGuard, before_count: usize) {
    cycle
        .raw
        .delete(&repo_path(&cycle.repo))
        .unwrap_or_else(|err| {
            panic!(
                "step 9: DELETE of `{}` failed (status {:?})",
                cycle.repo, err.status
            )
        });
    match cycle.raw.get::<Json>(&repo_path(&cycle.repo)) {
        Err(err) if err.status == Some(404) => guard.disarm(),
        Ok(_) => panic!("step 9: `{}` still exists after the DELETE", cycle.repo),
        Err(err) => panic!(
            "step 9: the GET after the DELETE answered an unexpected status {:?}",
            err.status
        ),
    }

    let after_names = list_repo_names(&cycle.raw, cycle.repo.owner());
    assert_eq!(
        after_names.len(),
        before_count,
        "step 9: the org's repository count must return to what step 1 saw"
    );

    cycle.say("step 9 (DELETE succeeds, the following GET is 404, and the repository count returns to step 1's): pass");
}

/// The final redaction sweep: nothing this file kept for it may contain
/// anything credential-shaped.
fn sweep_for_secrets(cycle: &Cycle) {
    for (index, text) in cycle.sweep.iter().enumerate() {
        for prefix in CREDENTIAL_PREFIXES {
            assert!(
                !text.contains(prefix),
                "produced string {index} looks credential-shaped (`{prefix}`): refusing to let it stand"
            );
        }
    }
    println!("redaction sweep over {} strings: pass", cycle.sweep.len());
}

#[test]
#[ignore = "opt-in live scaffold cycle against a real GitHub org; creates and deletes a \
            repository. Run with WILLIKINS_LIVE_TESTS=1, the `live-tests` feature, and \
            sandbox credentials sourced in the same command: `source \
            ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 cargo test -p \
            willikins-providers-github --features live-tests --test live_scaffold_cycle -- \
            --ignored --nocapture`"]
fn github_live_scaffold_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = credential_from_env().expect("a valid sandbox GitHub token");
    let org = sandbox_org();
    let raw = Arc::new(http_client(credential.clone()));

    let before_names = step_1_refuse_if_leftover(&raw, &org);
    let before_count = before_names.len();

    let repo = build_repo_identity(org);
    let mut guard = DeleteGuard::new(Arc::clone(&raw), repo.clone());
    guard.arm();
    println!("step 2 (the delete guard is armed before the create POST): pass");

    let (branch, init_head) = step_2_create(&raw, &repo);

    let client = Arc::new(GitHubClient::new(http_client(credential)));
    let mut cycle = Cycle {
        raw: Arc::clone(&raw),
        tool: GitHubScaffoldEnsure::new(client),
        repo,
        branch,
        marker: RepoPath::parse("app/.willikins-scaffold").expect("a valid RepoPath"),
        files: seed_files(),
        message: CommitHeadline::parse("test: live scaffold cycle (L1)")
            .expect("a valid CommitHeadline"),
        init_head,
        sweep: Vec::new(),
    };

    step_3_scaffold_creates(&mut cycle);
    step_4_second_ensure_converges(&mut cycle);
    let (edited_path, _edited_sha) = step_5_developer_edit(&mut cycle);
    step_6_second_scaffold_conflicts(&mut cycle, &edited_path);
    step_7_stale_expected_head_fails(&mut cycle);
    step_8_token_port_read(&mut cycle);
    step_9_delete_and_recount(&mut cycle, &mut guard, before_count);

    sweep_for_secrets(&cycle);
}

/// Pins this file's own repository-name construction against `ProjectSlug`'s
/// grammar, without any network call -- runs under `--features live-tests`
/// like every other test in this file, but needs neither `#[ignore]` nor
/// `WILLIKINS_LIVE_TESTS`.
#[test]
fn repo_prefix_and_unix_seconds_produce_a_valid_project_slug() {
    let candidate = format!("{REPO_PREFIX}{}", unix_seconds());
    assert!(
        candidate.len() <= 32,
        "the sandbox throwaway name must fit ProjectSlug's 32-character bound, was {} chars: {candidate}",
        candidate.len()
    );
    ProjectSlug::parse(&candidate)
        .expect("a unix-seconds-suffixed name must be a valid ProjectSlug");
}

/// Pins this file's own `blob_sha` against git's well-known empty-blob
/// sha1 (`git hash-object` on an empty file), without any network call.
#[test]
fn blob_sha_matches_a_known_git_hash_object_vector() {
    assert_eq!(blob_sha(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
}

/// Pins this file's own `expected_marker_content` against SHARED VALUES'
/// exact marker format (header first, then `<sha> <path>` lines sorted by
/// path), without any network call.
#[test]
fn expected_marker_content_is_sorted_by_path_with_the_required_header() {
    let files = vec![
        RepoFile::new(RepoPath::parse("b.txt").unwrap(), "b\n").unwrap(),
        RepoFile::new(RepoPath::parse("a.txt").unwrap(), "a\n").unwrap(),
    ];
    let content = expected_marker_content(&files);
    let mut lines = content.lines();
    assert_eq!(lines.next(), Some(MARKER_HEADER));
    let second = lines.next().expect("a line for a.txt");
    let third = lines.next().expect("a line for b.txt");
    assert!(
        second.ends_with(" a.txt"),
        "expected `a.txt` sorted before `b.txt`, got: {second}"
    );
    assert!(
        third.ends_with(" b.txt"),
        "expected `b.txt` second, got: {third}"
    );
}
