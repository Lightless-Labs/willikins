//! The live GitHub **rollback** cycle: milestone 3m, task L1, acceptance
//! 16 of `docs/plans/2026-10-05-milestone-3m-safe-rollback.md` ("Live,
//! sandbox only (L1-L4)... from documents that say `teardown: delete`:
//! create, roll back, `GET` is `404`. Create, drift (a push, a hand-set
//! secret), and the teardown refuses with the drift named. The test then
//! deletes by hand."), verify items 1-2.
//!
//! `#[ignore]`, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_TESTS=1` -- exactly `tests/live_new_repository_cycle.rs`'s
//! own gate, read only past that check. Compiled only under this crate's
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
//!   --test live_rollback_cycle -j 2 -- --ignored --nocapture
//! ```
//!
//! This file calls `github.repo.ensure`'s tool and its inverse directly,
//! never through `willikins-core`'s `plan_teardown`/`apply_teardown` or
//! a document -- the same reading milestone 3l's own L1
//! (`live_new_repository_cycle.rs`) gives the forward-provisioning live
//! cycles: this crate's own scoped gate names only this crate, and the
//! document-shaped end-to-end cycle is a separate task entirely (L4,
//! "End to end through the CLI"). This task's own dated addendum in the
//! plan records that reading. It builds the `Created` an inverse is
//! handed by hand, the same way `tests/it/repo_inverse_mock.rs` does
//! against `mockito`, but against the real sandbox org. Three
//! independent sub-cycles, each creating and disposing of its own
//! throwaway repository:
//!
//! 1. **Clean rollback** (`-a`): `github.repo.ensure` creates it; the
//!    inverse's `standing` reports `AsCreated` with no losing names; the
//!    inverse's `delete` (minting this file's own `TeardownToken`, the
//!    sanctioned test opt-in -- see `teardown_token` below) deletes it; a
//!    raw `GET` afterwards is `404`.
//! 2. **Push drift** (`-b`): created, its root file pre-landed by hand
//!    (a raw Contents-API `PUT`), then this cycle sleeps past
//!    `settled_by` and calls `github.scaffold.ensure` -- whose *only*
//!    write at that point is GraphQL's `createCommitOnBranch`, the root
//!    file already there. Verify item 1's own forward requirement: if
//!    that mutation did not bump `pushed_at`, this step's own raw
//!    before/after comparison fails outright, and a push after a real
//!    run would be invisible to the drift rule. The inverse's `standing`
//!    then reports `Drifted`, naming the push; `delete` refuses as a
//!    `Conflict` for the same reason -- the teardown's own refusal path,
//!    exercised for real. Deleted by hand afterwards, never through the
//!    refused inverse.
//! 3. **Hand-set secret drift** (`-c`): created, then an Actions secret
//!    set on it through `github.actions_secret.ensure`, never recorded as
//!    this run's own rider (`Created::riders` stays empty) -- exactly
//!    "a hand-set secret" the plan's own acceptance text names. A raw
//!    read of `GET .../actions/secrets` confirms GitHub's response lists
//!    the name with no `value` field at all (verify item 1's other
//!    half). The inverse's `standing` then reports `Drifted`, naming the
//!    foreign secret; `delete` refuses the same way. Deleted by hand.
//!
//! Every `Created.inputs`' `repo` port is built from GitHub's own
//! reported `full_name` right after creation ([`canonical_repo`]), never
//! from this cycle's own `WILLIKINS_SANDBOX_GITHUB_ORG`-built identity:
//! the inverse's `standing` compares `full_name` byte-for-byte, and only
//! `sandbox_org_from`'s case-insensitive check guards what that env var
//! may hold.
//!
//! Verify item 2 (`DELETE /repos/{owner}/{repo}` answers `204`) is
//! exercised by every hand delete below (`-b`, `-c`, and the guard's own
//! cleanup path); its `403`-when-forbidden half is not re-tested here --
//! the plan's own verify-item text says milestone 3l's live write cycle
//! already met that body.
//!
//! # Why raw requests for verification, not the client's own typed calls
//!
//! `GitHubClient`'s read/write methods this milestone's P1 task added
//! (`delete_repo`, `list_actions_secret_names`, and the rest) stay
//! `pub(crate)`, exactly as `tests/live_new_repository_cycle.rs`'s own
//! doc section explains for 3l's methods. Every independent verification
//! this file makes is therefore a raw request against the shared
//! [`willikins_providers_http::Http`] client; `repo_path` below is this
//! file's own copy of the client's private algorithm of the same name,
//! kept independent on purpose.
//!
//! # Redaction
//!
//! Nothing this file prints, records, or formats may contain the real
//! token's bytes, anything credential-shaped
//! ([`CREDENTIAL_PREFIXES`]), a response body, the name of any repository
//! but the three this cycle creates, or any byte of the throwaway Actions
//! secret value this cycle hand-sets ([`HAND_SET_SECRET_VALUE`], never
//! itself pushed into `sweep`; only its presence is asserted).
//! [`sweep_for_secrets`] checks every string kept for that purpose at the
//! end.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::Value as Json;

use willikins_core::rollback::{CreationWindow, CREATION_SKEW};
use willikins_core::{
    Created, Inputs, Inverse, Outputs, PortName, Standing, Timestamp, Tool, ToolErrorKind, Value,
};
use willikins_providers_github::{
    credential_from_env, http_client, GitHubActionsSecretEnsure, GitHubClient, GitHubRepoEnsure,
    GitHubScaffoldEnsure,
};
use willikins_providers_http::Http;
use willikins_types::{
    ActionsSecretName, CommitHeadline, DomainType, DopplerServiceToken, GitBranchName, GitHubOrg,
    GitHubRepo, ProjectSlug, RepoFile, RepoPath, RepoVisibility, TeardownToken,
};

/// The token prefixes `willikins_providers_github::CREDENTIAL_PATTERN`
/// accepts. Nothing this file prints, records, or formats may contain
/// either.
const CREDENTIAL_PREFIXES: &[&str] = &["ghp_", "github_pat_"];

/// Every throwaway repository this cycle creates starts with this
/// prefix -- the plan's own SHARED VALUES row "Live probe prefix",
/// `willikins-probe-delete-me-`, owned by L1-L3. Step 1 refuses to
/// proceed if one already exists, and the final step deletes exactly
/// the three this run created. 26 characters, which leaves only 6 for
/// [`PROBE_SUFFIX_LEN`]'s own short, unique suffix under
/// `ProjectSlug::MAX_LEN` (32) -- too tight for a full Unix timestamp,
/// unlike `live_new_repository_cycle.rs`'s much shorter
/// `willikins-newrepo-`.
const REPO_PREFIX: &str = "willikins-probe-delete-me-";

/// How many characters [`probe_timestamp`] plus a one-letter tag produce: 5 digits (seconds mod
/// 100,000) plus the one-letter sub-cycle tag (`a`/`b`/`c`), so
/// `REPO_PREFIX.len() + PROBE_SUFFIX_LEN == 32 == ProjectSlug::MAX_LEN`
/// exactly.
const PROBE_SUFFIX_LEN: usize = 6;

/// The page size this cycle lists the org's repositories with. See
/// `live_new_repository_cycle.rs`'s own constant of the same name for
/// why a full page refuses rather than silently miscounting.
const REPO_LIST_PAGE_SIZE: usize = 100;

/// The one org this cycle may create, write in, or delete from -- trust
/// boundary 2 of the milestone 3m plan's forebear (milestone 3l's own
/// "Live GitHub writes only in the sandbox org `Willikins-Test`"),
/// unchanged here.
const SANDBOX_ORG: &str = "Willikins-Test";

/// The Actions secret name `-c`'s hand-set drift uses. Not
/// `GITHUB_`-prefixed (reserved), and distinct from any name a real
/// document would bind, so a foreign-secret false positive never
/// confuses this cycle's own repositories.
const SECRET_NAME: &str = "WILLIKINS_ROLLBACK_PROBE";

/// The throwaway value hand-set into [`SECRET_NAME`] (milestone 3m's
/// `secret_literal_guard.rs`: a Doppler-service-token-shaped literal
/// must never appear contiguous in source, so this is `concat!`-assembled
/// from parts, exactly as `tests/it/actions_secret_ensure_mock.rs`'s own
/// `MARKER` is). Never a real credential; GitHub Actions accepts any
/// string as a secret's value.
const HAND_SET_SECRET_VALUE: &str = concat!(
    "dp.st.prd.",
    "wlknrollback",
    "0000",
    "0000",
    "0000",
    "0000",
    "0000",
    "0000",
    "wlkn"
);

/// `value` as a [`GitHubOrg`], refused unless it is [`SANDBOX_ORG`]
/// (GitHub org names are case-insensitive). Copied from
/// `live_new_repository_cycle.rs`'s own `sandbox_org_from`.
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

/// The test's own sink token, for a real `ensure` call (see
/// `live_new_repository_cycle.rs`'s own `sink_token` for why this is the
/// sanctioned narrow opt-in).
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn sink_token() -> willikins_core::SinkToken {
    willikins_core::SinkToken::new()
}

/// This file's own teardown token, the one legitimate test-side mint of
/// the capability `willikins_core::rollback::apply_teardown` otherwise
/// holds exclusively (milestone 3m trust boundary 2, task R2). This file
/// calls an inverse's `delete` directly, outside `apply_teardown`
/// entirely, so it must opt into the same narrow exemption every mock
/// test in this crate's own `tests/it/repo_inverse_mock.rs` already uses.
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn teardown_token() -> TeardownToken {
    TeardownToken::new()
}

/// The current Unix time in whole seconds, for this cycle's own
/// throwaway repository names.
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

/// The short, unique suffix every repository identity below shares:
/// `unix_seconds() % 100_000`, zero-padded to 5 digits -- the most
/// [`REPO_PREFIX`] leaves room for, not a full Unix timestamp (see
/// `REPO_PREFIX`'s own doc).
fn probe_timestamp(ts: u64) -> String {
    format!("{:05}", ts % 100_000)
}

/// A fresh repository identity: `willikins-probe-delete-me-<5 digits><tag>`
/// -- exactly [`ProjectSlug::MAX_LEN`] (32) characters.
fn build_repo_identity(org: &GitHubOrg, timestamp: &str, tag: &str) -> GitHubRepo {
    let name = format!("{REPO_PREFIX}{timestamp}{tag}");
    let slug = ProjectSlug::parse(&name)
        .unwrap_or_else(|err| panic!("`{name}` must be a valid ProjectSlug: {}", err.reason));
    GitHubRepo::new(org.clone(), slug)
}

/// Every repository name in `org`, one page only -- see
/// `live_new_repository_cycle.rs`'s own `list_repo_names` for why a full
/// page refuses rather than silently miscounting. Names themselves are
/// never printed (privacy: no repository but this cycle's own three may
/// be named in any output).
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

/// Verify item 1's own field list: `GET /repos/{owner}/{repo}` must carry
/// `created_at`, `pushed_at`, `open_issues_count`, `forks_count`,
/// `full_name` and `topics`, every one non-null. Asserted once, raw,
/// independent of `RepoBody`'s own (`pub(crate)`, unreachable here)
/// deserialization.
fn assert_decision_7_fields_present(body: &Json) {
    for field in [
        "created_at",
        "pushed_at",
        "open_issues_count",
        "forks_count",
        "full_name",
        "topics",
    ] {
        assert!(
            body.get(field).is_some_and(|value| !value.is_null()),
            "verify item 1: `GET /repos/{{owner}}/{{repo}}` must carry a non-null `{field}`, \
             got: {:?}",
            body.get(field)
        );
    }
}

/// `repo`'s raw `pushed_at` field, as a string, or `None` if absent or
/// null -- this cycle's own before/after comparison for verify item 1's
/// `createCommitOnBranch` forward requirement.
fn pushed_at_raw(raw: &Http, repo: &GitHubRepo) -> Option<String> {
    get_repo_raw(raw, repo)
        .get("pushed_at")
        .and_then(Json::as_str)
        .map(str::to_string)
}

/// `repo`'s reported `default_branch`, raw -- this file's own copy of
/// `live_new_repository_cycle.rs`'s own `default_branch_of`.
fn default_branch_of(raw: &Http, repo: &GitHubRepo) -> GitBranchName {
    let body = get_repo_raw(raw, repo);
    let name = body
        .get("default_branch")
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("`{repo}`'s GET carried no `default_branch`"));
    GitBranchName::parse(name)
        .unwrap_or_else(|err| panic!("`{name}` is not a valid GitBranchName: {}", err.reason))
}

/// `repo`'s own identity exactly as GitHub itself reports it (its raw
/// `full_name`, byte for byte). `GitHubRepoInverse::standing_from_body`
/// compares `full_name` against `Created.inputs`'s `repo` port
/// byte-for-byte to detect a rename, so this cycle never trusts its own
/// `WILLIKINS_SANDBOX_GITHUB_ORG`-built identity to carry GitHub's own
/// canonical casing (`sandbox_org_from` only checks it case-insensitively,
/// the org's real casing on GitHub's own servers may differ from the
/// sandbox env var's); it asks GitHub right after creation instead, and
/// every later `Created.inputs` in this file is built from this, never
/// from the identity used to issue the create call.
fn canonical_repo(raw: &Http, repo: &GitHubRepo) -> GitHubRepo {
    let body = get_repo_raw(raw, repo);
    let full_name = body
        .get("full_name")
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("`{repo}`'s GET carried no `full_name`"));
    GitHubRepo::parse(full_name)
        .unwrap_or_else(|err| panic!("`{full_name}` is not a valid GitHubRepo: {}", err.reason))
}

/// `/repos/{owner}/{name}/contents/{encoded path}` -- this file's own
/// copy of `client.rs`'s private `encode_contents_path`, applied to one
/// [`RepoPath`] (copied from `live_new_repository_cycle.rs`'s own
/// `contents_path`).
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

/// `at` shifted by `seconds` (positive or negative) -- this file's own
/// copy of `willikins_core::rollback`'s private `shift`, unreachable from
/// this separate integration-test crate.
fn shift(at: Timestamp, seconds: i64) -> Timestamp {
    Timestamp::from_datetime(*at.as_datetime() + chrono::Duration::seconds(seconds))
}

/// This cycle's own creation window (milestone 3m decision 2):
/// `[before - CREATION_SKEW, after + CREATION_SKEW]`, around a real
/// `ensure` call's own `before`/`after` wall-clock instants.
fn creation_window(before: Timestamp, after: Timestamp) -> CreationWindow {
    CreationWindow {
        start: shift(before, -CREATION_SKEW),
        end: shift(after, CREATION_SKEW),
    }
}

/// This cycle's own `settled_by` (decision 2): the point after which a
/// later change to the resource is drift, not this run's own doing --
/// `after + CREATION_SKEW`, the same instant `creation_window`'s own
/// `end` computes for a one-node run.
fn settled_by_from(after: Timestamp) -> Timestamp {
    shift(after, CREATION_SKEW)
}

/// `github.repo.ensure`'s own inputs for a real `ensure` call.
fn ensure_inputs(repo: &GitHubRepo) -> Inputs {
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

/// Exactly `github.repo.ensure`'s declared `Reversal::Delete::needs`:
/// the `repo` key port alone (this cycle never binds a document `token`
/// port, so the inverse falls back to the tool's own held credential,
/// the same default a bare `read`/`ensure` call uses).
fn created_inputs(repo: &GitHubRepo) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(
        PortName::parse("repo").expect("`repo` is a valid port name"),
        Value::known(repo.clone()),
    );
    inputs
}

/// A [`Created`] over `inputs`/`outputs`, `window`/`settled_by`, no
/// accepted loss (`github.repo.ensure` declares none) and no rider
/// (neither sub-cycle below ever records one -- `-c`'s whole point is a
/// secret this run's own record does *not* carry).
fn created<'a>(
    inputs: &'a Inputs,
    outputs: &'a Outputs,
    window: CreationWindow,
    settled_by: Timestamp,
) -> Created<'a> {
    Created {
        inputs,
        outputs,
        window,
        settled_by,
        accept: &[],
        riders: &[],
    }
}

/// `github.actions_secret.ensure`'s own inputs for a real `ensure` call,
/// hand-setting [`SECRET_NAME`] to [`HAND_SET_SECRET_VALUE`].
fn hand_set_secret_inputs(repo: &GitHubRepo) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(
        PortName::parse("repo").expect("`repo` is a valid port name"),
        Value::known(repo.clone()),
    );
    inputs.insert(
        PortName::parse("name").expect("`name` is a valid port name"),
        Value::known(
            ActionsSecretName::parse(SECRET_NAME)
                .expect("SECRET_NAME is a valid ActionsSecretName"),
        ),
    );
    inputs.insert(
        PortName::parse("value").expect("`value` is a valid port name"),
        Value::known(
            DopplerServiceToken::parse(HAND_SET_SECRET_VALUE)
                .expect("HAND_SET_SECRET_VALUE is a valid DopplerServiceToken"),
        ),
    );
    inputs
}

/// `github.scaffold.ensure`'s own inputs for one `ensure` call.
fn scaffold_inputs(repo: &GitHubRepo, branch: &GitBranchName, files: &[RepoFile]) -> Inputs {
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
        Value::known(RepoPath::parse(".willikins-scaffold").expect("a valid RepoPath")),
    );
    inputs.insert(
        PortName::parse("files").expect("`files` is a valid port name"),
        Value::known_list(files.to_vec()),
    );
    inputs.insert(
        PortName::parse("message").expect("`message` is a valid port name"),
        Value::known(
            CommitHeadline::parse("test: live rollback cycle push drift (L1)")
                .expect("a valid CommitHeadline"),
        ),
    );
    inputs
}

/// The two seed files [`step_3_push_drift`] uses: `root` is pre-landed
/// by hand (a raw Contents-API `PUT`, isolating the scaffold's own later
/// `createCommitOnBranch` write), `second` is left for the scaffold's
/// `ensure` to land through that mutation alone.
fn push_drift_files() -> (RepoFile, RepoFile) {
    let root = RepoFile::new(
        RepoPath::parse(".probe-root").expect("a valid RepoPath"),
        "test: live rollback cycle push drift root (L1)\n",
    )
    .expect("valid RepoFile content");
    let second = RepoFile::new(
        RepoPath::parse("PROBE.md").expect("a valid RepoPath"),
        "test: live rollback cycle push drift (L1)\n",
    )
    .expect("valid RepoFile content");
    (root, second)
}

/// `github.repo.ensure`'s inverse, over `tool`.
fn inverse(tool: &GitHubRepoEnsure) -> &dyn Inverse {
    Tool::inverse(tool).expect("github.repo.ensure declares Delete, so inverse() is Some")
}

/// Deletes `-a`, `-b` and `-c` on every exit path -- a panic, a failed
/// assertion, or an early return -- unless the test body already deleted
/// all three and disarmed the guard. Armed from before step 1's own
/// first write, so a repository that was in fact created despite a later
/// failure is never left behind; a `404` on any of their `DELETE` is
/// treated exactly like success (either already deleted through the
/// inverse, by hand, or never created at all). Copied from
/// `live_new_repository_cycle.rs`'s own `DeleteGuard`.
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

/// Everything one run of this cycle shares: the raw client, the three
/// tools under test, and the final redaction sweep buffer.
struct Cycle {
    raw: Arc<Http>,
    repo_ensure: GitHubRepoEnsure,
    scaffold: GitHubScaffoldEnsure,
    secret_ensure: GitHubActionsSecretEnsure,
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
/// `willikins-probe-delete-me-` leftover from an aborted run already exists, and
/// report the count the final step must return to.
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

/// Sub-cycle 1 (`-a`): clean rollback. `github.repo.ensure` creates it;
/// a raw `GET` confirms verify item 1's own field list; the inverse's
/// `standing` reports `AsCreated` with no losing names; `delete` (this
/// file's own `TeardownToken`) deletes it; a raw `GET` afterwards is
/// `404`.
fn step_2_clean_rollback(cycle: &mut Cycle, repo_a: &GitHubRepo) {
    let before = Timestamp::now();
    let ensured = cycle
        .repo_ensure
        .ensure(&ensure_inputs(repo_a), &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 2: creating `-a` failed ({:?}): {}",
                err.kind, err.message
            )
        });
    let after = Timestamp::now();
    assert!(
        ensured.changed,
        "step 2: creating `-a` must report changed: true"
    );

    let raw_body = get_repo_raw(&cycle.raw, repo_a);
    assert_decision_7_fields_present(&raw_body);
    cycle.say("step 2 (`-a` created; its raw GET carries verify item 1's six fields): pass");

    // `canonical_repo` below (not `repo_a`): `GitHubRepoInverse::standing`
    // compares `full_name` byte-for-byte against `Created.inputs`'s own
    // `repo` port, so this cycle's `repo` value must carry GitHub's own
    // canonical casing, never assume its `WILLIKINS_SANDBOX_GITHUB_ORG`
    // input already does.
    let repo_a = canonical_repo(&cycle.raw, repo_a);
    let window = creation_window(before, after);
    let settled_by = settled_by_from(after);
    let inputs = created_inputs(&repo_a);
    let outputs = Outputs::new();
    let standing = inverse(&cycle.repo_ensure)
        .standing(&created(&inputs, &outputs, window, settled_by))
        .unwrap_or_else(|err| panic!("step 2: standing on `-a` failed: {}", err.message));
    cycle.note(format!("step 2 standing: {standing:?}"));
    assert_eq!(
        standing,
        Standing::AsCreated { losing: Vec::new() },
        "step 2: a freshly created, untouched repository must report AsCreated with no losing \
         names"
    );

    let token = teardown_token();
    let deleted = inverse(&cycle.repo_ensure)
        .delete(&created(&inputs, &outputs, window, settled_by), &token)
        .unwrap_or_else(|err| panic!("step 2: delete of `-a` failed: {}", err.message));
    assert!(
        !deleted.already_gone,
        "step 2: deleting a freshly created repository must not report already_gone"
    );

    match cycle.raw.get::<Json>(&repo_path(&repo_a)) {
        Err(err) if err.status == Some(404) => {}
        Ok(_) => panic!("step 2: `-a` still exists after the inverse's own delete"),
        Err(err) => panic!(
            "step 2: the GET after the inverse's own delete answered an unexpected status {:?}",
            err.status
        ),
    }
    cycle.say(
        "step 2 (clean rollback: standing is AsCreated, delete reports already_gone: false, a \
         raw GET afterwards is 404): pass",
    );
}

/// Sub-cycle 2 (`-b`): push drift. Created; its root file is pre-landed
/// by hand (a raw Contents-API `PUT`, the same route a scaffold's own
/// first write on an empty repository uses), so that the *later*
/// `github.scaffold.ensure` call's only write is `createCommitOnBranch`
/// -- isolating verify item 1's own forward requirement (does that
/// mutation alone bump `pushed_at`?) from the unrelated REST write that
/// would otherwise also land in the same `ensure` call on a truly empty
/// repository. This cycle then sleeps past `settled_by` (`CREATION_SKEW`
/// plus slack for clock drift and `pushed_at`'s own second granularity)
/// before that push, so the inverse's `standing` reports `Drifted`,
/// naming the push, rather than `AsCreated`; `delete` refuses as a
/// `Conflict` for the same reason.
fn step_3_push_drift(cycle: &mut Cycle, repo_b: &GitHubRepo) {
    let before = Timestamp::now();
    let ensured = cycle
        .repo_ensure
        .ensure(&ensure_inputs(repo_b), &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 3: creating `-b` failed ({:?}): {}",
                err.kind, err.message
            )
        });
    let after = Timestamp::now();
    assert!(
        ensured.changed,
        "step 3: creating `-b` must report changed: true"
    );
    let repo_b = canonical_repo(&cycle.raw, repo_b);
    let window = creation_window(before, after);
    let settled_by = settled_by_from(after);

    let (root, second) = push_drift_files();
    let default_branch = default_branch_of(&cycle.raw, &repo_b);
    let pre_land_body = serde_json::json!({
        "message": "test: live rollback cycle push drift pre-land (L1)",
        "content": STANDARD.encode(root.content().as_bytes()),
        "branch": default_branch.as_str(),
    });
    cycle
        .raw
        .put::<Json>(&contents_path(&repo_b, root.path()), &pre_land_body)
        .unwrap_or_else(|err| {
            panic!(
                "step 3: the raw PUT pre-landing `-b`'s root file failed (status {:?})",
                err.status
            )
        });
    let pushed_at_before = pushed_at_raw(&cycle.raw, &repo_b);

    // Decision 2's own skew, plus margin for host/GitHub clock drift and
    // `pushed_at`'s second granularity: without clearing `settled_by`,
    // the push below would land inside it and `standing` would read
    // `AsCreated`, not `Drifted`.
    std::thread::sleep(Duration::from_secs((CREATION_SKEW + 10) as u64));

    let scaffold_ensured = cycle
        .scaffold
        .ensure(
            &scaffold_inputs(&repo_b, &default_branch, &[root, second]),
            &sink_token(),
        )
        .unwrap_or_else(|err| {
            panic!(
                "step 3: scaffolding `-b` failed ({:?}): {}",
                err.kind, err.message
            )
        });
    assert!(
        scaffold_ensured.changed,
        "step 3: the scaffold's ensure (the root file already pre-landed) must still report \
         changed: true for the remaining file and the marker"
    );
    let pushed_at_after = pushed_at_raw(&cycle.raw, &repo_b);
    assert_ne!(
        pushed_at_before, pushed_at_after,
        "verify item 1: createCommitOnBranch alone (the root file already pre-landed by a \
         separate raw PUT) must bump pushed_at -- without this, a push after a run is \
         invisible to the drift rule"
    );
    cycle.say(
        "step 3 (verify item 1: github.scaffold.ensure's own createCommitOnBranch, isolated \
         from the pre-landed root file's raw PUT, bumped `-b`'s pushed_at): pass",
    );

    let inputs = created_inputs(&repo_b);
    let outputs = Outputs::new();
    let standing = inverse(&cycle.repo_ensure)
        .standing(&created(&inputs, &outputs, window, settled_by))
        .unwrap_or_else(|err| panic!("step 3: standing on `-b` failed: {}", err.message));
    cycle.note(format!("step 3 standing: {standing:?}"));
    let what = match &standing {
        Standing::Drifted { what } => *what,
        other => panic!(
            "step 3: a repository pushed to after settled_by must report Drifted, got {other:?}"
        ),
    };
    assert!(
        what.contains("pushed"),
        "step 3: the drift reason must name the push, got: {what:?}"
    );
    cycle.say(&format!(
        "step 3 (the teardown refuses with the drift named -- standing is Drifted {{ what: \
         {what:?} }}): pass"
    ));

    let token = teardown_token();
    let err = inverse(&cycle.repo_ensure)
        .delete(&created(&inputs, &outputs, window, settled_by), &token)
        .expect_err("step 3: delete on a drifted repository must refuse");
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "step 3: the refusal must be a Conflict"
    );
    cycle.say("step 3 (delete refuses the drifted repository as a Conflict): pass");
}

/// Sub-cycle 3 (`-c`): hand-set secret drift. Created, then an Actions
/// secret hand-set on it through `github.actions_secret.ensure`, never
/// recorded as this run's own rider. A raw read of the secrets list
/// confirms verify item 1's other half (names, no value); the inverse's
/// `standing` then reports `Drifted`, naming the foreign secret;
/// `delete` refuses the same way.
fn step_4_secret_drift(cycle: &mut Cycle, repo_c: &GitHubRepo) {
    let before = Timestamp::now();
    let ensured = cycle
        .repo_ensure
        .ensure(&ensure_inputs(repo_c), &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 4: creating `-c` failed ({:?}): {}",
                err.kind, err.message
            )
        });
    let after = Timestamp::now();
    assert!(
        ensured.changed,
        "step 4: creating `-c` must report changed: true"
    );
    let repo_c = canonical_repo(&cycle.raw, repo_c);
    let window = creation_window(before, after);
    let settled_by = settled_by_from(after);

    cycle
        .secret_ensure
        .ensure(&hand_set_secret_inputs(&repo_c), &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 4: hand-setting the secret on `-c` failed ({:?}): {}",
                err.kind, err.message
            )
        });

    let secrets_body: Json = cycle
        .raw
        .get(&format!(
            "{}/actions/secrets?per_page=100&page=1",
            repo_path(&repo_c)
        ))
        .unwrap_or_else(|err| {
            panic!(
                "step 4: GET of `-c`'s Actions secrets failed (status {:?})",
                err.status
            )
        });
    let secrets = secrets_body
        .get("secrets")
        .and_then(Json::as_array)
        .expect("step 4: the secrets list carries a `secrets` array");
    assert_eq!(
        secrets.len(),
        1,
        "step 4: exactly one Actions secret must exist on `-c`"
    );
    assert_eq!(
        secrets[0].get("name").and_then(Json::as_str),
        Some(SECRET_NAME),
        "step 4: the one secret must be named SECRET_NAME"
    );
    assert!(
        secrets[0].get("value").is_none(),
        "verify item 1: the secrets list must carry names with no value field"
    );
    cycle.say(
        "step 4 (verify item 1: the Actions secrets list carries `-c`'s secret's name with no \
         value field): pass",
    );

    let inputs = created_inputs(&repo_c);
    let outputs = Outputs::new();
    let standing = inverse(&cycle.repo_ensure)
        .standing(&created(&inputs, &outputs, window, settled_by))
        .unwrap_or_else(|err| panic!("step 4: standing on `-c` failed: {}", err.message));
    cycle.note(format!("step 4 standing: {standing:?}"));
    let what = match &standing {
        Standing::Drifted { what } => *what,
        other => panic!(
            "step 4: a repository with a foreign Actions secret must report Drifted, got \
             {other:?}"
        ),
    };
    assert!(
        what.contains("secret"),
        "step 4: the drift reason must name the secret, got: {what:?}"
    );
    cycle.say(&format!(
        "step 4 (the teardown refuses with the drift named -- standing is Drifted {{ what: \
         {what:?} }}): pass"
    ));

    let token = teardown_token();
    let err = inverse(&cycle.repo_ensure)
        .delete(&created(&inputs, &outputs, window, settled_by), &token)
        .expect_err("step 4: delete on a repository with a foreign secret must refuse");
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "step 4: the refusal must be a Conflict"
    );
    cycle.say("step 4 (delete refuses the secret-drifted repository as a Conflict): pass");
}

/// Step 5: `-b` and `-c` (refused by their own inverse) are deleted by
/// hand, raw -- verify item 2 (`DELETE` answers `204`); each re-reads
/// `404`. `-a` is already gone (step 2's own inverse delete). The
/// repository count returns to step 1's; the guard is disarmed.
fn step_5_manual_cleanup_and_recount(
    cycle: &mut Cycle,
    guard: &mut DeleteGuard,
    repo_b: &GitHubRepo,
    repo_c: &GitHubRepo,
    org: &GitHubOrg,
    before_count: usize,
) {
    for repo in [repo_b, repo_c] {
        cycle.raw.delete(&repo_path(repo)).unwrap_or_else(|err| {
            panic!(
                "step 5: the hand DELETE of a drifted repository failed (status {:?})",
                err.status
            )
        });
        match cycle.raw.get::<Json>(&repo_path(repo)) {
            Err(err) if err.status == Some(404) => {}
            Ok(_) => panic!("step 5: a repository still exists after its hand DELETE"),
            Err(err) => panic!(
                "step 5: the GET after a hand DELETE answered an unexpected status {:?}",
                err.status
            ),
        }
    }
    guard.disarm();

    let after_count = list_repo_names(&cycle.raw, org).len();
    assert_eq!(
        after_count, before_count,
        "step 5: the org's repository count must return to what step 1 saw"
    );
    cycle.say(
        "step 5 (`-b` and `-c` deleted by hand, each re-read 404 -- verify item 2; repository \
         count restored; guard disarmed): pass",
    );
}

/// The final redaction sweep: nothing this file kept for it may contain
/// anything credential-shaped, the hand-set secret's own bytes, or any
/// repository name but implied by `-a`/`-b`/`-c`.
fn sweep_for_secrets(cycle: &Cycle) {
    for (index, text) in cycle.sweep.iter().enumerate() {
        for prefix in CREDENTIAL_PREFIXES {
            assert!(
                !text.contains(prefix),
                "produced string {index} looks credential-shaped (`{prefix}`): refusing to let \
                 it stand"
            );
        }
        assert!(
            !text.contains(HAND_SET_SECRET_VALUE),
            "produced string {index} carries the hand-set secret's own bytes: refusing to let \
             it stand"
        );
    }
    println!("redaction sweep over {} strings: pass", cycle.sweep.len());
}

#[test]
#[ignore = "opt-in live rollback cycle against a real GitHub org; creates, drifts, and deletes \
            three repositories plus one Actions secret. Run with WILLIKINS_LIVE_TESTS=1, the \
            `live-tests` feature, and sandbox credentials sourced in the same command: `source \
            ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 RUST_TEST_THREADS=2 \
            cargo test -p willikins-providers-github --features live-tests --test \
            live_rollback_cycle -j 2 -- --ignored --nocapture`"]
fn github_live_rollback_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = credential_from_env().expect("a valid sandbox GitHub token");
    let org = sandbox_org();
    let raw = Arc::new(http_client(credential.clone()));

    let before_count = step_1_count_and_refuse_leftovers(&raw, &org);

    let timestamp = probe_timestamp(unix_seconds());
    let repo_a = build_repo_identity(&org, &timestamp, "a");
    let repo_b = build_repo_identity(&org, &timestamp, "b");
    let repo_c = build_repo_identity(&org, &timestamp, "c");

    let mut guard = DeleteGuard::new(
        Arc::clone(&raw),
        [repo_a.clone(), repo_b.clone(), repo_c.clone()],
    );
    guard.arm();
    println!("the delete guard is armed before any write");

    let client = Arc::new(GitHubClient::new(http_client(credential)));
    let mut cycle = Cycle {
        raw: Arc::clone(&raw),
        repo_ensure: GitHubRepoEnsure::new(Arc::clone(&client)),
        scaffold: GitHubScaffoldEnsure::new(Arc::clone(&client)),
        secret_ensure: GitHubActionsSecretEnsure::new(client),
        sweep: Vec::new(),
    };

    step_2_clean_rollback(&mut cycle, &repo_a);
    step_3_push_drift(&mut cycle, &repo_b);
    step_4_secret_drift(&mut cycle, &repo_c);
    step_5_manual_cleanup_and_recount(&mut cycle, &mut guard, &repo_b, &repo_c, &org, before_count);

    sweep_for_secrets(&cycle);
}

/// Pins this file's own repository-name construction against
/// `ProjectSlug`'s grammar and its 32-character bound, without any
/// network call: `REPO_PREFIX` (the plan's own SHARED VALUES "Live
/// probe prefix") leaves exactly [`PROBE_SUFFIX_LEN`] characters, no
/// more.
#[test]
fn repo_prefix_and_probe_suffix_produce_a_valid_project_slug_at_exactly_the_bound() {
    assert_eq!(REPO_PREFIX.len() + PROBE_SUFFIX_LEN, ProjectSlug::MAX_LEN);
    for tag in ["a", "b", "c"] {
        let candidate = format!("{REPO_PREFIX}{}{tag}", probe_timestamp(unix_seconds()));
        assert_eq!(
            candidate.len(),
            ProjectSlug::MAX_LEN,
            "the probe repository name must land exactly at ProjectSlug's 32-character bound, \
             was {} chars: {candidate}",
            candidate.len()
        );
        ProjectSlug::parse(&candidate).expect(
            "a `willikins-probe-delete-me-<5 digits><tag>` name must be a valid ProjectSlug",
        );
    }
}

/// `probe_timestamp` always zero-pads to exactly 5 digits, even for a
/// small modulus result, without any network call.
#[test]
fn probe_timestamp_is_always_five_digits() {
    for ts in [0, 7, 99, 100_000, 100_007, u64::MAX] {
        assert_eq!(probe_timestamp(ts).len(), 5, "ts={ts}");
    }
}

/// Pins this file's own hand-set secret name and value against their
/// respective grammars, without any network call.
#[test]
fn secret_name_and_value_are_valid() {
    ActionsSecretName::parse(SECRET_NAME).expect("SECRET_NAME must be a valid ActionsSecretName");
    DopplerServiceToken::parse(HAND_SET_SECRET_VALUE)
        .expect("HAND_SET_SECRET_VALUE must be a valid DopplerServiceToken");
}

/// Trust boundary 2 (carried over from milestone 3l): live GitHub writes
/// only in the sandbox org `Willikins-Test`. GitHub org names are
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

/// Pins this file's own `shift`/`creation_window`/`settled_by_from`
/// arithmetic against `CREATION_SKEW`, without any network call.
#[test]
fn creation_window_and_settled_by_apply_the_skew_on_the_right_side() {
    let before = Timestamp::parse("2026-01-01T00:00:00Z").expect("a valid Timestamp");
    let after = Timestamp::parse("2026-01-01T00:00:05Z").expect("a valid Timestamp");
    let window = creation_window(before, after);
    assert_eq!(
        window.start,
        Timestamp::parse("2025-12-31T23:58:00Z").expect("a valid Timestamp")
    );
    assert_eq!(
        window.end,
        Timestamp::parse("2026-01-01T00:02:05Z").expect("a valid Timestamp")
    );
    assert_eq!(settled_by_from(after), window.end);
}
