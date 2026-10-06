//! The live GitHub **new-repository** cycle: milestone 3l, task L1,
//! acceptance 18 of `docs/plans/2026-10-05-milestone-3l-new-repositories.md`
//! ("The live new-repository cycle" section, written by L1, run once by the
//! coordinator).
//!
//! `#[ignore]`, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_TESTS=1` -- exactly `tests/live_scaffold_cycle.rs`'s own
//! gate, read only past that check. Compiled only under this crate's
//! `live-tests` feature (its own `[[test]]` entry in `Cargo.toml`), so a
//! plain `cargo test --workspace` never builds this file at all.
//!
//! **This task writes the harness; it does not run it.** Per the plan's
//! own task table, sonnet writes L1, the coordinator runs it once. The
//! run command, credentials sourced only in the command that runs it:
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
//!   cargo test -p willikins-providers-github --features live-tests \
//!   --test live_new_repository_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! Eleven steps, each named in the plan's own "The live new-repository
//! cycle" section (see each `step_*` function's own doc comment for the
//! exact wording):
//!
//! 1. Refuse any org but `Willikins-Test`; count its repositories
//!    read-only; refuse to start if a `willikins-newrepo-` repository
//!    already exists; arm one guard that deletes `-a`, `-b` and `-c` on
//!    every exit path, before any write.
//! 2. The scaffold's `read` on `-a` (which does not exist): `Absent`.
//! 3. `github.repo.ensure` creates `-a`: `read` `Absent`, `ensure`
//!    `changed: true`; raw reads record `default_branch` (verify item 2)
//!    and that `branches?per_page=1` is `[]`.
//! 4. A raw `GET git/ref/heads/{default}` on `-a`: print the status
//!    (verify item 1).
//! 5. The scaffold's `read` on `-a` with branch `not-the-default`:
//!    `Conflict` naming both branches; `branches` still `[]`.
//! 6. The scaffold on `-a` with the live seed files: `read` `Absent`,
//!    `ensure` `changed: true`; independent raw reads confirm the shape of
//!    both landed commits (verify items 4, 5, 6).
//! 7. Converged: `read` `Present`, `ensure` `changed: false`, head
//!    unchanged; `github.repo.ensure` again `changed: false`.
//! 8. Resume: create `-b`, a raw `PUT` of the root file (a run that
//!    stopped after its first write), the scaffold's `ensure`:
//!    `changed: true`, exactly one new commit.
//! 9. Probe (verify item 3): create `-c`, a raw `PUT .../contents/probe.txt`
//!    with `branch: willikins-probe`. Printed only; no assertion beyond
//!    "nothing outside `-c` changed".
//! 10. One raw `GET .../rules/branches/{default}` on `-a`: parses as a
//!     list; print its length only.
//! 11. Delete `-a`, `-b`, `-c`; each re-reads `404`; the repository count
//!     returns to step 1's; disarm the guard.
//!
//! # Why raw requests, not the client's own typed calls
//!
//! `GitHubClient`'s five-plus read/write methods this milestone added
//! (`has_any_branch`, `create_first_file`, `pause`, `branch_rule_types`,
//! `encode_contents_path`) stay `pub(crate)`, exactly as milestone 3g's own
//! `live_scaffold_cycle.rs` left its five. Every independent verification
//! this file makes is therefore a raw, hand-built request against the
//! shared [`willikins_providers_http::Http`] client -- `repo_path`,
//! `contents_path`, `blob_sha` and `expected_marker_content` below are this
//! file's own copies of the client's and `scaffold_ensure`'s private
//! algorithms, kept independent on purpose: this harness's whole point is
//! an *independent* confirmation of what the tool itself believes, never a
//! call back into it.
//!
//! # Redaction
//!
//! Nothing this file prints, records, or formats may contain the real
//! token's bytes, anything credential-shaped
//! ([`CREDENTIAL_PREFIXES`]), a response body, or the name of any
//! repository but the three this cycle creates. [`sweep_for_secrets`]
//! checks every string kept for that purpose at the end.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value as Json;
use sha1::{Digest, Sha1};

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_github::{
    GitHubClient, GitHubRepoEnsure, GitHubScaffoldEnsure, credential_from_env, http_client,
};
use willikins_providers_http::Http;
use willikins_types::{
    CommitHeadline, DomainType, GitBranchName, GitHubOrg, GitHubRepo, ProjectSlug, RepoFile,
    RepoPath, RepoVisibility,
};

/// The token prefixes `willikins_providers_github::CREDENTIAL_PATTERN`
/// accepts. Nothing this file prints, records, or formats may contain
/// either.
const CREDENTIAL_PREFIXES: &[&str] = &["ghp_", "github_pat_"];

/// Every throwaway repository this cycle creates starts with this prefix
/// -- step 1 refuses to proceed if one already exists, and step 11
/// deletes exactly the three this run created.
const REPO_PREFIX: &str = "willikins-newrepo-";

/// The page size step 1 and step 11 list the org's repositories with. See
/// `live_scaffold_cycle.rs`'s own constant of the same name for why a
/// full page refuses rather than silently miscounting.
const REPO_LIST_PAGE_SIZE: usize = 100;

/// The scaffold's marker path for every repository this cycle scaffolds.
const MARKER: &str = ".willikins-scaffold";

/// The one org this cycle may create, write in, or delete from -- trust
/// boundary 2 of the milestone's plan ("Live GitHub writes only in the
/// sandbox org `Willikins-Test`").
const SANDBOX_ORG: &str = "Willikins-Test";

/// `value` as a [`GitHubOrg`], refused unless it is [`SANDBOX_ORG`]
/// (GitHub org names are case-insensitive). Copied from
/// `live_scaffold_cycle.rs`'s own `sandbox_org_from`.
fn sandbox_org_from(value: &str) -> Result<GitHubOrg, String> {
    if !value.eq_ignore_ascii_case(SANDBOX_ORG) {
        return Err(format!(
            "`{value}` is not the sandbox org `{SANDBOX_ORG}`; this cycle writes nowhere else"
        ));
    }
    GitHubOrg::parse(value).map_err(|_| format!("`{value}` is not a valid GitHub org slug"))
}

/// The sandbox org this cycle runs against, from `WILLIKINS_SANDBOX_GITHUB_ORG`,
/// refused unless it names [`SANDBOX_ORG`].
fn sandbox_org() -> GitHubOrg {
    let value =
        std::env::var("WILLIKINS_SANDBOX_GITHUB_ORG").expect("WILLIKINS_SANDBOX_GITHUB_ORG is set");
    sandbox_org_from(&value).unwrap_or_else(|reason| panic!("{reason}"))
}

/// The test's own sink token (see `live_scaffold_cycle.rs`'s own
/// `sink_token` for why this is the sanctioned narrow opt-in).
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn sink_token() -> SinkToken {
    SinkToken::new()
}

/// The current Unix time in whole seconds, for this cycle's own throwaway
/// repository names.
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock reads a time after 1970")
        .as_secs()
}

/// `/repos/{owner}/{name}` -- this file's own copy of `client.rs`'s
/// private `repo_path`, `pub(crate)` there and unreachable from this
/// separate integration-test crate.
fn repo_path(repo: &GitHubRepo) -> String {
    format!("/repos/{}/{}", repo.owner(), repo.name())
}

/// `/repos/{owner}/{name}/contents/{encoded path}` -- this file's own
/// copy of `client.rs`'s private `encode_contents_path`/
/// `encode_contents_path_segment`, applied to one [`RepoPath`].
fn contents_path(repo: &GitHubRepo, path: &RepoPath) -> String {
    use std::fmt::Write as _;
    let encoded = path
        .segments()
        .map(|segment| {
            let mut out = String::with_capacity(segment.len());
            for byte in segment.bytes() {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
                    out.push(byte as char);
                } else {
                    let _ = write!(out, "%{byte:02X}");
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("{}/contents/{encoded}", repo_path(repo))
}

/// The same algorithm `client::git_blob_sha` computes (`pub(crate)`, so
/// this integration-test crate cannot call it directly): SHA-1 over
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

/// This file's own copy of `scaffold_ensure::marker_content` (private to
/// that module): `"managed-by: willikins"`, then one
/// `<40-hex blob sha> <path>` line per seeded file, sorted by path,
/// trailing newline.
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
    let mut content = String::from("managed-by: willikins");
    content.push('\n');
    for (path, sha) in lines {
        content.push_str(&sha);
        content.push(' ');
        content.push_str(&path);
        content.push('\n');
    }
    content
}

/// The three live seed files (SHARED VALUES "Live seed files"):
/// `.tool+versions` is the byte-order-smallest path (it starts with `.`,
/// before `R` and `d`), so it is the root file the first write seeds --
/// and its `+` exercises the Contents API path encoder's `%2B` (verify
/// item 5).
fn seed_files() -> Vec<RepoFile> {
    vec![
        RepoFile::new(
            RepoPath::parse(".tool+versions").expect("a valid RepoPath"),
            "rust 1.0\n",
        )
        .expect("valid RepoFile content"),
        RepoFile::new(
            RepoPath::parse("README.md").expect("a valid RepoPath"),
            "# probe\n",
        )
        .expect("valid RepoFile content"),
        RepoFile::new(
            RepoPath::parse("docs/guide.md").expect("a valid RepoPath"),
            "guide\n",
        )
        .expect("valid RepoFile content"),
    ]
}

/// The root file: the byte-order-smallest path in [`seed_files`],
/// `.tool+versions`.
fn root_file() -> RepoFile {
    seed_files()
        .into_iter()
        .min_by(|a, b| a.path().as_str().cmp(b.path().as_str()))
        .expect("seed_files is non-empty")
}

/// A fresh repository identity: `willikins-newrepo-<unix-seconds>-{suffix}`
/// -- at most 30 characters, well under `ProjectSlug::MAX_LEN` (32).
fn build_repo_identity(org: &GitHubOrg, ts: u64, suffix: &str) -> GitHubRepo {
    let name = format!("{REPO_PREFIX}{ts}-{suffix}");
    let slug = ProjectSlug::parse(&name)
        .unwrap_or_else(|err| panic!("`{name}` must be a valid ProjectSlug: {}", err.reason));
    GitHubRepo::new(org.clone(), slug)
}

/// Every repository name in `org`, one page only -- see
/// `live_scaffold_cycle.rs`'s own `list_repo_names` for why a full page
/// refuses rather than silently miscounting. Names themselves are never
/// printed (privacy: no repository but this cycle's own three may be
/// named in any output).
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

/// `GET /repos/{owner}/{name}`, raw.
fn get_repo_raw(raw: &Http, repo: &GitHubRepo) -> Json {
    raw.get(&repo_path(repo))
        .unwrap_or_else(|err| panic!("GET of `{repo}` failed (status {:?})", err.status))
}

/// `repo`'s reported `default_branch`, raw (verify item 2).
fn default_branch_of(raw: &Http, repo: &GitHubRepo) -> GitBranchName {
    let body = get_repo_raw(raw, repo);
    let name = body
        .get("default_branch")
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("`{repo}`'s GET carried no `default_branch`"));
    GitBranchName::parse(name)
        .unwrap_or_else(|err| panic!("`{name}` is not a valid GitBranchName: {}", err.reason))
}

/// `GET /repos/{owner}/{name}/branches?per_page=1`, raw: `true` iff
/// non-empty.
fn has_any_branch_raw(raw: &Http, repo: &GitHubRepo) -> bool {
    let branches: Vec<Json> = raw
        .get(&format!("{}/branches?per_page=1", repo_path(repo)))
        .unwrap_or_else(|err| {
            panic!(
                "GET of `{repo}`'s branches failed (status {:?})",
                err.status
            )
        });
    !branches.is_empty()
}

/// `branch`'s commits on `repo`, newest first, raw (`GET
/// .../commits?sha={branch}`), at most 10 -- every repository this cycle
/// scaffolds holds at most two.
fn list_commits_raw(raw: &Http, repo: &GitHubRepo, branch: &GitBranchName) -> Vec<Json> {
    raw.get(&format!(
        "{}/commits?sha={branch}&per_page=10",
        repo_path(repo)
    ))
    .unwrap_or_else(|err| {
        panic!(
            "GET of `{repo}`@`{branch}`'s commits failed (status {:?})",
            err.status
        )
    })
}

/// `branch`'s current head commit sha on `repo`, raw: the newest entry of
/// [`list_commits_raw`].
fn branch_head(raw: &Http, repo: &GitHubRepo, branch: &GitBranchName) -> String {
    list_commits_raw(raw, repo, branch)
        .first()
        .and_then(|commit| commit.get("sha"))
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("`{repo}`@`{branch}` carries no commits"))
        .to_string()
}

/// One commit's full body, raw (`GET .../commits/{sha}`), for its tree
/// sha, parents and `verification.verified`.
fn get_commit_raw(raw: &Http, repo: &GitHubRepo, sha: &str) -> Json {
    raw.get(&format!("{}/commits/{sha}", repo_path(repo)))
        .unwrap_or_else(|err| {
            panic!(
                "GET of commit `{sha}` on `{repo}` failed (status {:?})",
                err.status
            )
        })
}

/// `tree_sha`'s entries, recursive, raw -- never used to decide
/// correctness of this crate's own non-test code (it is `pub(crate)`
/// there and walked non-recursively); only this harness's own
/// independent verification reads a tree all at once.
fn get_tree_recursive_raw(raw: &Http, repo: &GitHubRepo, tree_sha: &str) -> Vec<Json> {
    let body: Json = raw
        .get(&format!(
            "{}/git/trees/{tree_sha}?recursive=1",
            repo_path(repo)
        ))
        .unwrap_or_else(|err| {
            panic!(
                "GET of tree `{tree_sha}` on `{repo}` failed (status {:?})",
                err.status
            )
        });
    body.get("tree")
        .and_then(Json::as_array)
        .unwrap_or_else(|| panic!("tree `{tree_sha}` on `{repo}` carried no `tree` array"))
        .clone()
}

/// The scaffold's own inputs for one `read`/`ensure` call.
fn scaffold_inputs(
    repo: &GitHubRepo,
    branch: &GitBranchName,
    files: &[RepoFile],
    message: &CommitHeadline,
) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(
        PortName::parse("repo").expect("`repo` is a valid port name"),
        Value::known(repo.clone()),
    );
    inputs.insert(
        PortName::parse("branch").expect("`branch` is a valid port name"),
        Value::known(branch.clone()),
    );
    inputs.insert(
        PortName::parse("marker").expect("`marker` is a valid port name"),
        Value::known(RepoPath::parse(MARKER).expect("a valid RepoPath")),
    );
    inputs.insert(
        PortName::parse("files").expect("`files` is a valid port name"),
        Value::known_list(files.to_vec()),
    );
    inputs.insert(
        PortName::parse("message").expect("`message` is a valid port name"),
        Value::known(message.clone()),
    );
    inputs
}

/// `github.repo.ensure`'s own inputs for one `read`/`ensure` call.
fn repo_ensure_inputs(repo: &GitHubRepo) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(
        PortName::parse("repo").expect("`repo` is a valid port name"),
        Value::known(repo.clone()),
    );
    inputs.insert(
        PortName::parse("visibility").expect("`visibility` is a valid port name"),
        Value::known(RepoVisibility::Private),
    );
    inputs
}

/// Deletes `-a`, `-b` and `-c` on every exit path -- a panic, a failed
/// assertion, or an early return -- unless the test body already deleted
/// all three and disarmed the guard. Armed from before step 2, the first
/// write, so a repository that was in fact created despite a later
/// failure is never left behind; `-b` and `-c` may not exist yet when the
/// guard is armed, and a `404` on their `DELETE` is treated exactly like
/// success.
struct DeleteGuard {
    http: Arc<Http>,
    repos: [GitHubRepo; 3],
    armed: bool,
}

impl DeleteGuard {
    fn new(http: Arc<Http>, repos: [GitHubRepo; 3]) -> Self {
        Self {
            http,
            repos,
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
        let mut leftover = false;
        for repo in &self.repos {
            println!("guard: deleting `{repo}`");
            match self.http.delete(&repo_path(repo)) {
                Ok(()) => println!("guard: deleted `{repo}`"),
                Err(err) if err.status == Some(404) => {
                    println!("guard: `{repo}` was already gone (404)");
                }
                Err(err) => {
                    leftover = true;
                    println!(
                        "guard: !!! LEFTOVER REPOSITORY `{repo}` !!! the guard's DELETE failed \
                         (status {:?}); it must be deleted by hand",
                        err.status
                    );
                }
            }
        }
        assert!(
            !leftover || already_panicking,
            "the guard could not delete every repository; see the LEFTOVER lines above -- they \
             must be deleted by hand"
        );
    }
}

/// Everything one run of this cycle shares: the raw client, the two
/// tools under test, and the final redaction sweep buffer.
struct Cycle {
    raw: Arc<Http>,
    scaffold: GitHubScaffoldEnsure,
    repo_ensure: GitHubRepoEnsure,
    message: CommitHeadline,
    sweep: Vec<String>,
}

impl Cycle {
    /// Print one line and keep it for the final sweep.
    fn say(&mut self, line: &str) {
        println!("{line}");
        self.sweep.push(line.to_string());
    }

    /// Keep a produced string for the final sweep without printing it.
    fn note(&mut self, text: String) {
        self.sweep.push(text);
    }
}

/// Step 1: list the org's repositories, refuse to start if a
/// `willikins-newrepo-` leftover from an aborted run already exists, and
/// report the count step 11 must return to.
fn step_1_count_and_refuse_leftovers(raw: &Http, org: &GitHubOrg) -> usize {
    let names = list_repo_names(raw, org);
    let leftovers = names
        .iter()
        .filter(|name| name.starts_with(REPO_PREFIX))
        .count();
    assert_eq!(
        leftovers, 0,
        "step 1: {leftovers} leftover `{REPO_PREFIX}*` repository/repositories from an aborted \
         run; delete them by hand before running this cycle"
    );
    println!(
        "step 1 (no `{REPO_PREFIX}*` leftover; `{org}` holds {} repositories): pass",
        names.len()
    );
    names.len()
}

/// Step 2: the scaffold's `read` on `-a`, which does not exist yet:
/// `Absent`.
fn step_2_scaffold_read_absent_on_nonexistent_repo(cycle: &mut Cycle, repo_a: &GitHubRepo) {
    let default_guess = GitBranchName::parse("main").expect("a valid GitBranchName");
    let inputs = scaffold_inputs(repo_a, &default_guess, &seed_files(), &cycle.message);
    let observation = cycle
        .scaffold
        .read(&inputs)
        .expect("step 2: read of the scaffold on a repository that does not exist yet");
    cycle.note(format!("step 2 read: {observation:?}"));
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 2: a repository that does not exist yet must read Absent"
    );
    cycle.say("step 2 (the scaffold's read on a nonexistent repository is Absent): pass");
}

/// Step 3: `github.repo.ensure` creates `-a`: `read` `Absent`, `ensure`
/// `changed: true`; raw reads record `default_branch` (verify item 2) and
/// that `branches?per_page=1` is `[]`. Returns `-a`'s default branch.
fn step_3_repo_ensure_creates(cycle: &mut Cycle, repo_a: &GitHubRepo) -> GitBranchName {
    let inputs = repo_ensure_inputs(repo_a);
    let observation = cycle
        .repo_ensure
        .read(&inputs)
        .expect("step 3: read of `-a` before it exists");
    cycle.note(format!("step 3 read: {observation:?}"));
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 3: `-a` must read Absent before github.repo.ensure creates it"
    );

    let ensured = cycle
        .repo_ensure
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 3: creating `-a` failed ({:?}): {}",
                err.kind, err.message
            )
        });
    cycle.note(format!("step 3 ensured: changed={}", ensured.changed));
    assert!(
        ensured.changed,
        "step 3: creating `-a` must report changed: true"
    );

    let default_branch = default_branch_of(&cycle.raw, repo_a);
    let has_branch = has_any_branch_raw(&cycle.raw, repo_a);
    assert!(
        !has_branch,
        "step 3: a freshly created repository must report no branches yet"
    );
    cycle.say(&format!(
        "step 3 (github.repo.ensure created `-a`; default branch `{default_branch}`; \
         branches?per_page=1 is []): pass"
    ));
    default_branch
}

/// Step 4: a raw `GET git/ref/heads/{default}` on `-a`: print the status
/// (verify item 1: `404` or `409`).
fn step_4_ref_head_status_on_empty_repo(
    cycle: &mut Cycle,
    repo_a: &GitHubRepo,
    default_branch: &GitBranchName,
) {
    let path = format!("{}/git/ref/heads/{default_branch}", repo_path(repo_a));
    match cycle.raw.get::<Json>(&path) {
        Ok(_) => panic!(
            "step 4: `git/ref/heads/{default_branch}` unexpectedly succeeded on an empty repository"
        ),
        Err(err) => {
            cycle.say(&format!(
                "step 4 (git/ref/heads/{{default}} on an empty repository answers status {:?}, verify item 1): pass",
                err.status
            ));
            assert!(
                matches!(err.status, Some(404 | 409)),
                "step 4: expected 404 or 409 on an empty repository's ref, got {:?}",
                err.status
            );
        }
    }
}

/// Step 5: the scaffold's `read` on `-a` with branch `not-the-default`:
/// `Conflict` naming both branches; `branches` still `[]`.
fn step_5_mismatched_branch_conflicts(
    cycle: &mut Cycle,
    repo_a: &GitHubRepo,
    default_branch: &GitBranchName,
) {
    let other = GitBranchName::parse("not-the-default").expect("a valid GitBranchName");
    let inputs = scaffold_inputs(repo_a, &other, &seed_files(), &cycle.message);
    let err = cycle
        .scaffold
        .read(&inputs)
        .expect_err("step 5: a mismatched default branch on an empty repository must refuse");
    cycle.note(format!("step 5 error kind: {:?}", err.kind));
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "step 5: the refusal must be a Conflict"
    );
    assert!(
        err.message.contains(default_branch.as_str()) && err.message.contains(other.as_str()),
        "step 5: the refusal must name both the default branch `{default_branch}` and the \
         requested branch `{other}`"
    );
    assert!(
        !has_any_branch_raw(&cycle.raw, repo_a),
        "step 5: a refused read must not create a branch"
    );
    cycle.say(
        "step 5 (a mismatched default branch on an empty repository conflicts, naming both \
         branches; branches is still []): pass",
    );
}

/// Step 6: the scaffold on `-a` with the live seed files: `read`
/// `Absent`, `ensure` `changed: true`. Independent raw reads confirm: the
/// branch is the default; exactly two commits; the root commit has no
/// parent and its tree holds exactly `.tool+versions` (verify item 5,
/// `%2B`); the second commit's parent is the root and its tree holds
/// every path as `100644` with the expected blob sha plus the marker;
/// prints both commits' `verification.verified` (verify item 4).
fn step_6_scaffold_initializes_empty_repo(
    cycle: &mut Cycle,
    repo_a: &GitHubRepo,
    default_branch: &GitBranchName,
) {
    let files = seed_files();
    let inputs = scaffold_inputs(repo_a, default_branch, &files, &cycle.message);
    let observation = cycle
        .scaffold
        .read(&inputs)
        .expect("step 6: read of the scaffold on an empty repository, matching the default branch");
    cycle.note(format!("step 6 read: {observation:?}"));
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 6: an empty repository matching the default branch must read Absent"
    );

    let ensured = cycle
        .scaffold
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 6: ensure failed ({:?}): {}", err.kind, err.message));
    cycle.note(format!("step 6 ensured: changed={}", ensured.changed));
    assert!(
        ensured.changed,
        "step 6: the first ensure must initialize and scaffold the repository"
    );

    verify_two_commits_after_initialization(cycle, repo_a, default_branch, &files);
}

/// The independent raw reads `step_6_scaffold_initializes_empty_repo`
/// makes (split out purely to stay under `clippy::too_many_lines` -- this
/// is still step 6's own verification, not a separately reusable
/// helper): exactly two commits; the root commit has no parent and its
/// tree holds exactly the root file (verify item 5, `%2B`); the second
/// commit's parent is the root and its tree holds every path as
/// `100644` with the expected blob sha plus the marker; prints both
/// commits' `verification.verified` (verify item 4).
fn verify_two_commits_after_initialization(
    cycle: &mut Cycle,
    repo_a: &GitHubRepo,
    default_branch: &GitBranchName,
    files: &[RepoFile],
) {
    let commits = list_commits_raw(&cycle.raw, repo_a, default_branch);
    assert_eq!(
        commits.len(),
        2,
        "step 6: exactly two commits must exist after the scaffold lands"
    );
    let second_sha = commits[0]
        .get("sha")
        .and_then(Json::as_str)
        .expect("step 6: the newest commit carries a sha")
        .to_string();
    let root_sha = commits[1]
        .get("sha")
        .and_then(Json::as_str)
        .expect("step 6: the root commit carries a sha")
        .to_string();

    let root_verified = verify_root_commit(&cycle.raw, repo_a, &root_sha);
    let second_verified = verify_second_commit(&cycle.raw, repo_a, &root_sha, &second_sha, files);

    cycle.say(&format!(
        "step 6 commit verification.verified: root={root_verified:?}, second={second_verified:?} \
         (verify item 4, recorded, not asserted)"
    ));
    cycle.say(
        "step 6 (the scaffold initializes the empty repository: root commit holds exactly the \
         root file with no parent, second commit adds every other file and the marker): pass",
    );
}

/// Half of `verify_two_commits_after_initialization`'s own raw
/// verification, split out purely to stay under
/// `clippy::too_many_lines`: the root commit has no parent and its tree
/// holds exactly the root file (verify item 5, `%2B`). Returns its
/// `commit.verification.verified` (verify item 4), recorded by the
/// caller, never asserted.
fn verify_root_commit(raw: &Http, repo_a: &GitHubRepo, root_sha: &str) -> Option<bool> {
    let root_commit = get_commit_raw(raw, repo_a, root_sha);
    let root_parents = root_commit
        .pointer("/parents")
        .and_then(Json::as_array)
        .expect("step 6: the root commit carries a parents array");
    assert!(
        root_parents.is_empty(),
        "step 6: the root commit must have no parent"
    );
    let root_tree_sha = root_commit
        .pointer("/commit/tree/sha")
        .and_then(Json::as_str)
        .expect("step 6: the root commit names its tree sha")
        .to_string();
    let root_tree = get_tree_recursive_raw(raw, repo_a, &root_tree_sha);
    let root_file_path = root_file().path().as_str().to_string();
    assert_eq!(
        root_tree.len(),
        1,
        "step 6: the root commit's tree must hold exactly one entry"
    );
    assert_eq!(
        root_tree[0].get("path").and_then(Json::as_str),
        Some(root_file_path.as_str()),
        "step 6: the root commit's tree must hold exactly `{root_file_path}` (verify item 5, %2B)"
    );
    assert_eq!(
        root_tree[0].get("sha").and_then(Json::as_str),
        Some(blob_sha(root_file().content().as_bytes()).as_str()),
        "step 6: the root file's blob sha must match its rendered content"
    );
    root_commit
        .pointer("/commit/verification/verified")
        .and_then(Json::as_bool)
}

/// The other half of `verify_two_commits_after_initialization`'s own raw
/// verification: the second commit's parent is the root and its tree
/// holds every path as `100644` with the expected blob sha, plus the
/// marker. Returns its `commit.verification.verified` (verify item 4),
/// recorded by the caller, never asserted.
fn verify_second_commit(
    raw: &Http,
    repo_a: &GitHubRepo,
    root_sha: &str,
    second_sha: &str,
    files: &[RepoFile],
) -> Option<bool> {
    let second_commit = get_commit_raw(raw, repo_a, second_sha);
    let second_parents = second_commit
        .pointer("/parents")
        .and_then(Json::as_array)
        .expect("step 6: the second commit carries a parents array");
    assert_eq!(
        second_parents.len(),
        1,
        "step 6: the second commit must have exactly one parent"
    );
    assert_eq!(
        second_parents[0].get("sha").and_then(Json::as_str),
        Some(root_sha),
        "step 6: the second commit's parent must be the root commit"
    );
    let second_tree_sha = second_commit
        .pointer("/commit/tree/sha")
        .and_then(Json::as_str)
        .expect("step 6: the second commit names its tree sha")
        .to_string();
    let second_tree = get_tree_recursive_raw(raw, repo_a, &second_tree_sha);
    let find = |entries: &[Json], path: &str| {
        entries
            .iter()
            .find(|entry| entry.get("path").and_then(Json::as_str) == Some(path))
            .cloned()
    };
    for file in files {
        let entry = find(&second_tree, file.path().as_str()).unwrap_or_else(|| {
            panic!(
                "step 6: `{}` is missing from the second commit's tree",
                file.path()
            )
        });
        assert_eq!(
            entry.get("mode").and_then(Json::as_str),
            Some("100644"),
            "step 6: `{}` must land as a regular file",
            file.path()
        );
        assert_eq!(
            entry.get("sha").and_then(Json::as_str),
            Some(blob_sha(file.content().as_bytes()).as_str()),
            "step 6: `{}`'s landed content does not match what this cycle asked for",
            file.path()
        );
    }
    let marker_entry = find(&second_tree, MARKER).unwrap_or_else(|| {
        panic!("step 6: the marker `{MARKER}` is missing from the second commit's tree")
    });
    assert_eq!(
        marker_entry.get("sha").and_then(Json::as_str),
        Some(blob_sha(expected_marker_content(files).as_bytes()).as_str()),
        "step 6: the marker's landed content does not match the expected format"
    );
    second_commit
        .pointer("/commit/verification/verified")
        .and_then(Json::as_bool)
}

/// Step 7: converged. `read` `Present`; `ensure` `changed: false`; head
/// unchanged; `github.repo.ensure` again `changed: false`.
fn step_7_converges(cycle: &mut Cycle, repo_a: &GitHubRepo, default_branch: &GitBranchName) {
    let files = seed_files();
    let inputs = scaffold_inputs(repo_a, default_branch, &files, &cycle.message);
    let observation = cycle
        .scaffold
        .read(&inputs)
        .expect("step 7: read of the landed scaffold");
    cycle.note(format!("step 7 read: {observation:?}"));
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 7: a landed scaffold must read Present"
    );

    let head_before = list_commits_raw(&cycle.raw, repo_a, default_branch)[0]
        .get("sha")
        .and_then(Json::as_str)
        .expect("step 7: the current head has a sha")
        .to_string();
    let ensured = cycle
        .scaffold
        .ensure(&inputs, &sink_token())
        .expect("step 7: a second ensure of a converged scaffold");
    cycle.note(format!(
        "step 7 scaffold ensured: changed={}",
        ensured.changed
    ));
    assert!(
        !ensured.changed,
        "step 7: a converged scaffold must report changed: false"
    );
    let head_after = list_commits_raw(&cycle.raw, repo_a, default_branch)[0]
        .get("sha")
        .and_then(Json::as_str)
        .expect("step 7: the head after has a sha")
        .to_string();
    assert_eq!(
        head_before, head_after,
        "step 7: a converged ensure must not move the branch head"
    );

    let repo_inputs = repo_ensure_inputs(repo_a);
    let repo_ensured = cycle
        .repo_ensure
        .ensure(&repo_inputs, &sink_token())
        .expect("step 7: a second github.repo.ensure on the same repository");
    cycle.note(format!(
        "step 7 repo ensured: changed={}",
        repo_ensured.changed
    ));
    assert!(
        !repo_ensured.changed,
        "step 7: a second github.repo.ensure on `-a` must report changed: false"
    );

    cycle.say(
        "step 7 (converged: read is Present, both ensures report changed: false, the head is \
         unchanged): pass",
    );
}

/// Step 8: resume. Create `-b` through `github.repo.ensure`; a raw `PUT
/// .../contents/.tool+versions` with the same content (a run that stopped
/// after its first write); the scaffold's `ensure`: `changed: true`,
/// exactly one new commit whose tree adds the other two files and the
/// marker and leaves `.tool+versions`' blob unchanged.
fn step_8_resume_after_first_write(cycle: &mut Cycle, repo_b: &GitHubRepo) {
    let inputs = repo_ensure_inputs(repo_b);
    let ensured = cycle
        .repo_ensure
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 8: creating `-b` failed ({:?}): {}",
                err.kind, err.message
            )
        });
    assert!(
        ensured.changed,
        "step 8: creating `-b` must report changed: true"
    );

    let default_branch = default_branch_of(&cycle.raw, repo_b);
    let root = root_file();
    let put_body = serde_json::json!({
        "message": "test: a half-finished scaffold's own first write (L1 step 8)",
        "content": STANDARD.encode(root.content().as_bytes()),
        "branch": default_branch.as_str(),
    });
    cycle
        .raw
        .put::<Json>(&contents_path(repo_b, root.path()), &put_body)
        .unwrap_or_else(|err| {
            panic!(
                "step 8: the raw PUT of the root file failed (status {:?})",
                err.status
            )
        });
    let root_blob_sha = blob_sha(root.content().as_bytes());

    let files = seed_files();
    let scaffold_inputs = scaffold_inputs(repo_b, &default_branch, &files, &cycle.message);
    let observation = cycle
        .scaffold
        .read(&scaffold_inputs)
        .expect("step 8: read of a half-finished scaffold");
    cycle.note(format!("step 8 read: {observation:?}"));
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 8: a repository holding only the byte-equal root file must still read Absent"
    );

    let ensured = cycle
        .scaffold
        .ensure(&scaffold_inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 8: ensure failed ({:?}): {}", err.kind, err.message));
    cycle.note(format!("step 8 ensured: changed={}", ensured.changed));
    assert!(
        ensured.changed,
        "step 8: resuming a half-finished scaffold must report changed: true"
    );

    verify_resume_adds_remaining_files(
        &cycle.raw,
        repo_b,
        &default_branch,
        &root,
        &root_blob_sha,
        &files,
    );

    cycle.say(
        "step 8 (resume: a raw PUT of the root file, then the scaffold's own ensure adds the \
         remaining files and the marker as one commit, leaving the root file's blob unchanged): \
         pass",
    );
}

/// `step_8_resume_after_first_write`'s own raw verification, split out
/// purely to stay under `clippy::too_many_lines`: exactly two commits
/// exist; the newest commit's tree adds every file but the root one,
/// plus the marker, and leaves the root file's own blob unchanged.
fn verify_resume_adds_remaining_files(
    raw: &Http,
    repo_b: &GitHubRepo,
    default_branch: &GitBranchName,
    root: &RepoFile,
    root_blob_sha: &str,
    files: &[RepoFile],
) {
    let commits = list_commits_raw(raw, repo_b, default_branch);
    assert_eq!(
        commits.len(),
        2,
        "step 8: exactly two commits must exist (the raw PUT's root commit, plus one more)"
    );
    let newest_sha = commits[0]
        .get("sha")
        .and_then(Json::as_str)
        .expect("step 8: the newest commit carries a sha")
        .to_string();
    let newest_commit = get_commit_raw(raw, repo_b, &newest_sha);
    let tree_sha = newest_commit
        .pointer("/commit/tree/sha")
        .and_then(Json::as_str)
        .expect("step 8: the newest commit names its tree sha")
        .to_string();
    let tree = get_tree_recursive_raw(raw, repo_b, &tree_sha);
    let find = |entries: &[Json], path: &str| {
        entries
            .iter()
            .find(|entry| entry.get("path").and_then(Json::as_str) == Some(path))
            .cloned()
    };
    let root_entry = find(&tree, root.path().as_str())
        .unwrap_or_else(|| panic!("step 8: `{}` is missing from the resumed tree", root.path()));
    assert_eq!(
        root_entry.get("sha").and_then(Json::as_str),
        Some(root_blob_sha),
        "step 8: the root file's blob must be unchanged by the resumed scaffold"
    );
    for file in files.iter().filter(|file| file.path() != root.path()) {
        let entry = find(&tree, file.path().as_str()).unwrap_or_else(|| {
            panic!("step 8: `{}` is missing from the resumed tree", file.path())
        });
        assert_eq!(
            entry.get("sha").and_then(Json::as_str),
            Some(blob_sha(file.content().as_bytes()).as_str()),
            "step 8: `{}`'s landed content does not match what this cycle asked for",
            file.path()
        );
    }
    assert!(
        find(&tree, MARKER).is_some(),
        "step 8: the marker is missing from the resumed tree"
    );
}

/// Step 9 (verify item 3): create `-c`; a raw `PUT .../contents/probe.txt`
/// with `branch: willikins-probe`. Prints one line: the status, whether
/// `willikins-probe` exists afterwards, and the repository's
/// `default_branch`. No assertion beyond "nothing outside `-c` changed",
/// checked against `repo_a` and `repo_b`'s own branch heads, captured
/// before this step runs.
fn step_9_probe_non_default_branch_on_empty_repo(
    cycle: &mut Cycle,
    repo_c: &GitHubRepo,
    repo_a: &GitHubRepo,
    default_branch_a: &GitBranchName,
    repo_b: &GitHubRepo,
    default_branch_b: &GitBranchName,
) {
    let heads_before = (
        branch_head(&cycle.raw, repo_a, default_branch_a),
        branch_head(&cycle.raw, repo_b, default_branch_b),
    );

    let inputs = repo_ensure_inputs(repo_c);
    let ensured = cycle
        .repo_ensure
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 9: creating `-c` failed ({:?}): {}",
                err.kind, err.message
            )
        });
    assert!(
        ensured.changed,
        "step 9: creating `-c` must report changed: true"
    );

    let probe_branch = "willikins-probe";
    let probe_path = RepoPath::parse("probe.txt").expect("a valid RepoPath");
    let put_body = serde_json::json!({
        "message": "test: probe a non-default branch on an empty repository (L1 step 9)",
        "content": STANDARD.encode(b"probe\n"),
        "branch": probe_branch,
    });
    let put_status = match cycle
        .raw
        .put::<Json>(&contents_path(repo_c, &probe_path), &put_body)
    {
        Ok(_) => "2xx".to_string(),
        Err(err) => format!("{:?}", err.status),
    };

    let probe_branch_exists = {
        let path = format!("{}/git/ref/heads/{probe_branch}", repo_path(repo_c));
        cycle.raw.get::<Json>(&path).is_ok()
    };
    let repo_c_default = default_branch_of(&cycle.raw, repo_c);

    cycle.say(&format!(
        "step 9 (verify item 3 -- PUT .../contents/probe.txt with branch: {probe_branch} on an \
         empty repository): PUT status {put_status}; `{probe_branch}` exists afterwards: \
         {probe_branch_exists}; `-c`'s default_branch: {repo_c_default}"
    ));

    let heads_after = (
        branch_head(&cycle.raw, repo_a, default_branch_a),
        branch_head(&cycle.raw, repo_b, default_branch_b),
    );
    assert_eq!(
        heads_before, heads_after,
        "step 9: the probe on `-c` must not change `-a` or `-b`"
    );

    cycle.say("step 9 (nothing outside `-c` changed): pass");
}

/// Step 10: one raw `GET .../rules/branches/{default}` on `-a`: parses
/// as a list; print its length only.
fn step_10_rules_length_only(
    cycle: &mut Cycle,
    repo_a: &GitHubRepo,
    default_branch: &GitBranchName,
) {
    let path = format!("{}/rules/branches/{default_branch}", repo_path(repo_a));
    // A private repository in an organisation on GitHub's free plan cannot
    // read rules at all: GitHub answers 403 ("Upgrade to GitHub Pro or make
    // this repository public"). Seen live on 2026-10-06; recorded, not a
    // failure, since the scaffold reads rules only to explain a refused write.
    match cycle.raw.get::<Vec<Json>>(&path) {
        Ok(rules) => cycle.say(&format!(
            "step 10 (rules in force on `-a`@`{default_branch}`): {} rule(s)",
            rules.len()
        )),
        Err(err) if err.status == Some(403) => cycle.say(
            "step 10 (rules on `-a` are not readable on this plan: 403; recorded, not asserted)",
        ),
        Err(err) => panic!(
            "step 10: GET of `-a`'s branch rules failed (status {:?})",
            err.status
        ),
    }
}

/// Step 11: delete `-a`, `-b`, `-c`; each re-reads `404`; the repository
/// count equals step 1's; disarm the guard.
fn step_11_delete_and_recount(
    cycle: &mut Cycle,
    guard: &mut DeleteGuard,
    repos: &[GitHubRepo; 3],
    org: &GitHubOrg,
    before_count: usize,
) {
    for repo in repos {
        cycle.raw.delete(&repo_path(repo)).unwrap_or_else(|err| {
            panic!(
                "step 11: DELETE of a repository failed (status {:?})",
                err.status
            )
        });
        match cycle.raw.get::<Json>(&repo_path(repo)) {
            Err(err) if err.status == Some(404) => {}
            Ok(_) => panic!("step 11: a repository still exists after its DELETE"),
            Err(err) => panic!(
                "step 11: the GET after a DELETE answered an unexpected status {:?}",
                err.status
            ),
        }
    }
    guard.disarm();

    let after_count = list_repo_names(&cycle.raw, org).len();
    assert_eq!(
        after_count, before_count,
        "step 11: the org's repository count must return to what step 1 saw"
    );

    cycle.say(
        "step 11 (all three repositories deleted, each re-read 404, repository count restored, \
         guard disarmed): pass",
    );
}

/// The final redaction sweep: nothing this file kept for it may contain
/// anything credential-shaped, and the only repository names it ever
/// collected are its own three (never printed by name, only referred to
/// as `-a`/`-b`/`-c`).
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
#[ignore = "opt-in live new-repository cycle against a real GitHub org; creates and deletes \
            three repositories. Run with WILLIKINS_LIVE_TESTS=1, the `live-tests` feature, and \
            sandbox credentials sourced in the same command: `source \
            ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
            cargo test -p willikins-providers-github --features live-tests --test \
            live_new_repository_cycle -j 2 -- --ignored --nocapture`"]
fn github_live_new_repository_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = credential_from_env().expect("a valid sandbox GitHub token");
    let org = sandbox_org();
    let raw = Arc::new(http_client(credential.clone()));

    let before_count = step_1_count_and_refuse_leftovers(&raw, &org);

    let ts = unix_seconds();
    let repo_a = build_repo_identity(&org, ts, "a");
    let repo_b = build_repo_identity(&org, ts, "b");
    let repo_c = build_repo_identity(&org, ts, "c");

    let mut guard = DeleteGuard::new(
        Arc::clone(&raw),
        [repo_a.clone(), repo_b.clone(), repo_c.clone()],
    );
    guard.arm();
    println!("the delete guard is armed before any write");

    let client = Arc::new(GitHubClient::new(http_client(credential)));
    let mut cycle = Cycle {
        raw: Arc::clone(&raw),
        scaffold: GitHubScaffoldEnsure::new(Arc::clone(&client)),
        repo_ensure: GitHubRepoEnsure::new(client),
        message: CommitHeadline::parse("test: live new-repository cycle (L1)")
            .expect("a valid CommitHeadline"),
        sweep: Vec::new(),
    };

    step_2_scaffold_read_absent_on_nonexistent_repo(&mut cycle, &repo_a);
    let default_branch_a = step_3_repo_ensure_creates(&mut cycle, &repo_a);
    step_4_ref_head_status_on_empty_repo(&mut cycle, &repo_a, &default_branch_a);
    step_5_mismatched_branch_conflicts(&mut cycle, &repo_a, &default_branch_a);
    step_6_scaffold_initializes_empty_repo(&mut cycle, &repo_a, &default_branch_a);
    step_7_converges(&mut cycle, &repo_a, &default_branch_a);
    step_8_resume_after_first_write(&mut cycle, &repo_b);
    let default_branch_b = default_branch_of(&raw, &repo_b);
    step_9_probe_non_default_branch_on_empty_repo(
        &mut cycle,
        &repo_c,
        &repo_a,
        &default_branch_a,
        &repo_b,
        &default_branch_b,
    );
    step_10_rules_length_only(&mut cycle, &repo_a, &default_branch_a);
    step_11_delete_and_recount(
        &mut cycle,
        &mut guard,
        &[repo_a, repo_b, repo_c],
        &org,
        before_count,
    );

    sweep_for_secrets(&cycle);
}

/// Pins this file's own repository-name construction against
/// `ProjectSlug`'s grammar, without any network call.
#[test]
fn repo_prefix_unix_seconds_and_suffix_produce_a_valid_project_slug() {
    for suffix in ["a", "b", "c"] {
        let candidate = format!("{REPO_PREFIX}{}-{suffix}", unix_seconds());
        assert!(
            candidate.len() <= 32,
            "the sandbox throwaway name must fit ProjectSlug's 32-character bound, was {} chars: \
             {candidate}",
            candidate.len()
        );
        ProjectSlug::parse(&candidate).expect(
            "a `willikins-newrepo-<unix-seconds>-<suffix>` name must be a valid ProjectSlug",
        );
    }
}

/// Pins this file's own `blob_sha` against git's well-known empty-blob
/// sha1 (`git hash-object` on an empty file), without any network call.
#[test]
fn blob_sha_matches_a_known_git_hash_object_vector() {
    assert_eq!(blob_sha(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
}

/// Pins this file's own `contents_path` against the Contents API's
/// `%2B`/`%40` encoding (SHARED VALUES "Path encoding (S1)"), without any
/// network call.
#[test]
fn contents_path_percent_encodes_plus_and_at() {
    let org = GitHubOrg::parse("example-org").expect("a valid GitHubOrg");
    let repo = GitHubRepo::new(
        org,
        ProjectSlug::parse("example-repo").expect("a valid slug"),
    );
    let path = RepoPath::parse(".tool+versions").expect("a valid RepoPath");
    assert_eq!(
        contents_path(&repo, &path),
        "/repos/example-org/example-repo/contents/.tool%2Bversions"
    );
}

/// Pins this file's own `root_file` against [`seed_files`]: the
/// byte-order-smallest path is `.tool+versions`.
#[test]
fn root_file_is_the_byte_order_smallest_seed_path() {
    assert_eq!(root_file().path().as_str(), ".tool+versions");
}

/// Pins this file's own `expected_marker_content` against SHARED VALUES'
/// exact marker format (header first, then `<sha> <path>` lines sorted by
/// path), without any network call.
#[test]
fn expected_marker_content_is_sorted_by_path_with_the_required_header() {
    let content = expected_marker_content(&seed_files());
    let mut lines = content.lines();
    assert_eq!(lines.next(), Some("managed-by: willikins"));
    let first = lines.next().expect("a line for .tool+versions");
    let second = lines.next().expect("a line for README.md");
    let third = lines.next().expect("a line for docs/guide.md");
    assert!(
        first.ends_with(" .tool+versions"),
        "expected `.tool+versions` first, got: {first}"
    );
    assert!(
        second.ends_with(" README.md"),
        "expected `README.md` second, got: {second}"
    );
    assert!(
        third.ends_with(" docs/guide.md"),
        "expected `docs/guide.md` third, got: {third}"
    );
}

/// Trust boundary 2 of the milestone's plan: live GitHub writes only in
/// the sandbox org `Willikins-Test`. GitHub org names are
/// case-insensitive.
#[test]
fn sandbox_org_from_refuses_every_org_but_the_sandbox() {
    for real in [
        "Example-Org",
        "Lightless-Labs",
        "willikins-test-other",
        "Willikins",
    ] {
        assert!(
            sandbox_org_from(real).is_err(),
            "`{real}` is not the sandbox org and must be refused"
        );
    }
    for sandbox in ["Willikins-Test", "willikins-test", "WILLIKINS-TEST"] {
        assert!(
            sandbox_org_from(sandbox).is_ok(),
            "`{sandbox}` is the sandbox org"
        );
    }
}
