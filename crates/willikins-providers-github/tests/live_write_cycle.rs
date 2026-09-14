//! The live GitHub **write** cycle: the one test in this workspace that
//! provisions something real and then removes it again.
//!
//! `#[ignore]`, and inert even under `--ignored` unless
//! `WILLIKINS_LIVE_TESTS=1` — the credential is read (through
//! [`willikins_providers_github::credential_from_env`], itself
//! `Credential::from_env`) only past that gate, exactly as
//! `tests/live_probe.rs` does. The org comes from
//! `WILLIKINS_SANDBOX_GITHUB_ORG`.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   cargo test -p willikins-providers-github --test live_write_cycle \
//!   -- --ignored --nocapture
//! ```
//!
//! It drives the two real tools — never a mock — against a repository
//! with the fixed, distinctive name `willikins-live-write-cycle` in the
//! sandbox org, in eight asserted steps:
//!
//! 1. the repository must be absent (`404`); a leftover from an aborted
//!    run makes the test refuse to proceed and name it, because a
//!    leftover is the operator's to remove by hand;
//! 2. a [`DeleteGuard`] is armed, so every exit path — a panic, a failed
//!    assertion, an early return — deletes the repository again;
//! 3. `github.repo.ensure` reads `Absent`, creates it private, and a raw
//!    `GET` shows the `managed-by-willikins` topic and `visibility:
//!    private`;
//! 4. `read` is now `Present`, a second `ensure` is `changed: false`, and
//!    a raw `GET` before and after shows it changed nothing;
//! 5. acceptance test 9, live: `read` with `visibility: public` requested
//!    is `Mismatch { visibility }`, `ensure` is `Conflict`, and the
//!    repository is still private;
//! 6. `github.actions_secret.ensure` on a synthetic token: `Absent`, then
//!    `changed: true`, then `Present`, then `changed: true` again,
//!    because a sink whose value cannot be read back always writes;
//! 7. the three fixtures the read-only probe could not reach
//!    (`repo_get_present`, `actions_secret_public_key`,
//!    `actions_secret_get_present`) are recorded, redacted, under
//!    `fixtures/github/live/` and compared by top-level key set;
//! 8. `DELETE /repos/{org}/{repo}` succeeds, a following `GET` is `404`,
//!    and the guard is disarmed.
//!
//! GitHub documents `DELETE /repos/{owner}/{repo}` as answering `204` on
//! success and `403` with `{"message": "Organization members cannot
//! delete repositories.", ...}` when an org owner has configured the org
//! to prevent members from deleting organization-owned repositories
//! (`OpenAPI` description, `.paths."/repos/{owner}/{repo}".delete`, fetched
//! 2026-09-14; it also declares `307`, `404` and `409`).
//! [`willikins_providers_http::Http::delete`] collapses every `2xx` into
//! `Ok(())` and hands back no status, so step 8 asserts the success and
//! the following `404` rather than the literal `204`.
//!
//! **Deletion stays in this test.** No tool and no client method of
//! `willikins-providers-github` gains a delete; the crate's tools never
//! delete anything. The guard reaches `Http::delete` directly, on its own
//! [`willikins_providers_http::Http`], which the tools do not share.
//!
//! **Redaction.** The synthetic token's bytes and anything credential-
//! shaped (`ghp_`, `github_pat_` — what
//! [`willikins_providers_github::CREDENTIAL_PATTERN`] pins) must appear in
//! no `ToolError`, no `Observation` or `Ensured` `Debug` or `render`, no
//! recorded live fixture, and no line this test prints. Every such string
//! is collected as it is produced and swept at the end. The real
//! credential's bytes are never read into this test: it never calls
//! `std::env::var` on `WILLIKINS_GITHUB_TOKEN`, because a second raw read
//! would create exactly the plaintext `Credential` exists to prevent — so
//! "the credential's marker" is checked as its documented *prefixes*, not
//! as its value.
//!
//! A second `#[ignore]` test in this file,
//! `the_cycles_repository_is_gone`, is the after-the-run confirmation: it
//! only `GET`s the fixed name and asserts `404`. It needs
//! `WILLIKINS_LIVE_LEFTOVER_CHECK=1` on top of `WILLIKINS_LIVE_TESTS=1`,
//! so the command above never runs it concurrently with the cycle that
//! deliberately creates that repository.

mod common;

use std::sync::Arc;

use rand_core::RngCore as _;
use serde_json::Value as Json;
use willikins_core::{Ensured, Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind};
use willikins_providers_github::{
    GitHubActionsSecretEnsure, GitHubClient, GitHubRepoEnsure, credential_from_env, http_client,
};
use willikins_providers_http::Http;
use willikins_types::{
    ActionsSecretName, DomainType, DopplerServiceToken, GitHubOrg, GitHubRepo, ProjectSlug,
    RepoVisibility,
};

use common::{live_dir, record_and_compare};

/// The cycle's fixed repository name. Distinctive on purpose: anything by
/// this name in the sandbox org is this test's, and a leftover one is
/// deleted by hand.
const REPO_NAME: &str = "willikins-live-write-cycle";

/// The Actions secret this cycle writes. Never carries a real value.
const SECRET_NAME: &str = "WILLIKINS_WRITE_CYCLE";

/// The token prefixes [`willikins_providers_github::CREDENTIAL_PATTERN`]
/// accepts. Nothing this test prints, records, or formats may contain
/// either.
const CREDENTIAL_PREFIXES: &[&str] = &["ghp_", "github_pat_"];

/// The test's own sink token. `SinkToken::new` is disallowed outside the
/// apply executor; an executor-level test opts in narrowly, which is the
/// convention every such test in this workspace uses.
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn sink_token() -> SinkToken {
    SinkToken::new()
}

/// A synthetic Doppler service token: `dp.st.dev.` plus 40 lower-case
/// alphanumerics generated here, matching
/// [`DopplerServiceToken`]'s pattern. Never a real credential, never a
/// constant, never printed — returned as both its plaintext (so the final
/// sweep can look for it) and its parsed domain type.
fn synthetic_token() -> (String, DopplerServiceToken) {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = [0_u8; 40];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let suffix: String = bytes
        .iter()
        .map(|byte| char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()]))
        .collect();
    let plaintext = format!("dp.st.dev.{suffix}");
    let token = DopplerServiceToken::parse(&plaintext)
        .expect("a `dp.st.dev.` prefix and 40 lower-case alphanumerics is a valid service token");
    (plaintext, token)
}

/// `/repos/{owner}/{name}`, the one path this file builds by hand (the
/// client builds its own from the same already-validated parts).
fn repo_path(repo: &GitHubRepo) -> String {
    format!("/repos/{}/{}", repo.owner(), repo.name())
}

/// Deletes the cycle's repository on every exit path — a panic, a failed
/// assertion, or an early return — unless the test body already deleted
/// it explicitly and disarmed the guard.
///
/// A `Drop` implementation must not panic while the thread is already
/// panicking (that aborts the process and hides the original failure), so
/// a failed delete during a panic prints the repository's full name
/// loudly and returns; it panics only when it is itself the first
/// failure.
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
            // A `404` means there is nothing left to delete: the body
            // removed it and failed afterwards, before disarming.
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
                    "the guard could not delete `{}` (status {status:?}); \
                     it must be deleted by hand",
                    self.repo
                );
            }
        }
    }
}

/// Everything one run of the cycle needs: a raw HTTP channel (shared with
/// the guard) for the `GET`s and the `DELETE` that no tool performs, the
/// two real tools, the identities, and the sweep buffer every produced
/// string lands in.
struct Cycle {
    raw: Arc<Http>,
    repo_tool: GitHubRepoEnsure,
    secret_tool: GitHubActionsSecretEnsure,
    repo: GitHubRepo,
    secret_name: ActionsSecretName,
    token_plaintext: String,
    sweep: Vec<String>,
}

impl Cycle {
    /// Print one line and keep it for the final redaction sweep: this
    /// test prints only step names and pass/fail, never a body.
    fn say(&mut self, line: &str) {
        println!("{line}");
        self.sweep.push(line.to_string());
    }

    /// Keep a produced string (a `Debug`, a `render`, a `ToolError`
    /// message) for the final sweep without printing it.
    fn note(&mut self, text: String) {
        self.sweep.push(text);
    }

    fn repo_inputs(&self, visibility: RepoVisibility) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("repo").expect("`repo` is a valid port name"),
            willikins_core::Value::known(self.repo.clone()),
        );
        inputs.insert(
            PortName::parse("visibility").expect("`visibility` is a valid port name"),
            willikins_core::Value::known(visibility),
        );
        inputs
    }

    fn secret_inputs(&self) -> Inputs {
        let token = DopplerServiceToken::parse(&self.token_plaintext)
            .expect("the synthetic token parsed once already");
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("repo").expect("`repo` is a valid port name"),
            willikins_core::Value::known(self.repo.clone()),
        );
        inputs.insert(
            PortName::parse("name").expect("`name` is a valid port name"),
            willikins_core::Value::known(self.secret_name.clone()),
        );
        inputs.insert(
            PortName::parse("value").expect("`value` is a valid port name"),
            willikins_core::Value::known(token),
        );
        inputs
    }

    fn get_repo_body(&self, step: &str) -> Json {
        self.raw
            .get::<Json>(&repo_path(&self.repo))
            .unwrap_or_else(|err| {
                panic!(
                    "{step}: the raw GET of `{}` failed (status {:?})",
                    self.repo, err.status
                )
            })
    }

    /// Record an `Observation` and every value it carries, redaction and
    /// all, into the sweep.
    fn record_observation(&mut self, step: &str, observation: &Observation) {
        self.note(format!("{step} observation: {observation:?}"));
        if let Observation::Present(outputs) | Observation::Absent { predicted: outputs } =
            observation
        {
            let rendered: Vec<String> = outputs
                .iter()
                .map(|(port, value)| format!("{port}={}", value.render()))
                .collect();
            self.note(format!("{step} rendered: {rendered:?}"));
        }
    }

    fn record_ensured(&mut self, step: &str, ensured: &Ensured) {
        self.note(format!("{step} ensured: {ensured:?}"));
        let rendered: Vec<String> = ensured
            .outputs
            .iter()
            .map(|(port, value)| format!("{port}={}", value.render()))
            .collect();
        self.note(format!("{step} rendered: {rendered:?}"));
    }
}

/// Step 1: the repository must not exist. A leftover from an aborted run
/// is the operator's to remove by hand, so the test refuses to proceed
/// and names it rather than deleting something it did not create in this
/// run.
fn step_1_absent(raw: &Http, repo: &GitHubRepo) {
    match raw.get::<Json>(&repo_path(repo)) {
        Err(err) if err.status == Some(404) => {
            println!("step 1 (GET /repos/{{org}}/{{repo}} is 404): pass");
        }
        Ok(_) => panic!(
            "step 1: `{repo}` already exists, left behind by an aborted run; \
             a leftover is the operator's to delete by hand, so this test refuses to proceed"
        ),
        Err(err) => panic!(
            "step 1: GET of `{repo}` answered an unexpected status {:?}",
            err.status
        ),
    }
}

/// Step 3: `read` is `Absent`, `ensure` creates the repository private
/// and reports `changed: true` with the predicted `repo` and `url`, and a
/// raw `GET` shows the ownership topic and `visibility: private`.
fn step_3_create(cycle: &mut Cycle) {
    let inputs = cycle.repo_inputs(RepoVisibility::Private);
    let observation = cycle
        .repo_tool
        .read(&inputs)
        .expect("step 3: read of an absent repository");
    cycle.record_observation("step 3 read", &observation);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 3: read of an absent repository should be Absent"
    );

    let ensured = cycle
        .repo_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| {
            panic!(
                "step 3: ensure failed ({:?}): {}; if the create landed and the topic PUT \
                 failed, the guard deletes the repository",
                err.kind, err.message
            )
        });
    cycle.record_ensured("step 3 ensure", &ensured);
    assert!(ensured.changed, "step 3: a create reports changed: true");

    let repo_port = PortName::parse("repo").expect("`repo` is a valid port name");
    let url_port = PortName::parse("url").expect("`url` is a valid port name");
    assert_eq!(
        ensured
            .outputs
            .get(&repo_port)
            .and_then(willikins_core::Value::downcast::<GitHubRepo>),
        Some(&cycle.repo),
        "step 3: the `repo` output is the repository that was created"
    );
    assert_eq!(
        ensured
            .outputs
            .get(&url_port)
            .map(|v| v.render().to_string()),
        Some(cycle.repo.url().to_string()),
        "step 3: the `url` output is the repository's URL"
    );

    let body = cycle.get_repo_body("step 3");
    assert_eq!(
        body.get("visibility").and_then(Json::as_str),
        Some("private"),
        "step 3: the created repository is private"
    );
    let topics: Vec<&str> = body
        .get("topics")
        .and_then(Json::as_array)
        .map(|items| items.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default();
    assert!(
        topics.contains(&"managed-by-willikins"),
        "step 3: the topic PUT did not land; the repository carries topics {topics:?}"
    );
    cycle.say("step 3 (ensure creates it private and marks it ours): pass");
}

/// Step 4: `read` is now `Present`, a second `ensure` is `changed: false`,
/// and a raw `GET` before and after shows it changed nothing.
fn step_4_converged(cycle: &mut Cycle) {
    let inputs = cycle.repo_inputs(RepoVisibility::Private);
    let observation = cycle
        .repo_tool
        .read(&inputs)
        .expect("step 4: read of the repository just created");
    cycle.record_observation("step 4 read", &observation);
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 4: read of our own repository should be Present"
    );

    let before = cycle.get_repo_body("step 4 (before)");
    let ensured = cycle
        .repo_tool
        .ensure(&inputs, &sink_token())
        .expect("step 4: a second ensure of a converged repository");
    cycle.record_ensured("step 4 ensure", &ensured);
    assert!(
        !ensured.changed,
        "step 4: a second ensure reports changed: false"
    );
    let after = cycle.get_repo_body("step 4 (after)");

    let differing = differing_keys(&before, &after);
    assert!(
        differing.is_empty(),
        "step 4: the second ensure changed these fields: {differing:?}"
    );
    cycle.say("step 4 (a second ensure is changed: false and changes nothing): pass");
}

/// Step 5: acceptance test 9, live. Requesting `public` on a repository
/// that is ours and private reads as `Mismatch { visibility }` and
/// `ensure`s as `Conflict`, and the repository is still private
/// afterwards — the tool changes visibility neither way.
fn step_5_visibility_mismatch(cycle: &mut Cycle) {
    let inputs = cycle.repo_inputs(RepoVisibility::Public);
    let observation = cycle
        .repo_tool
        .read(&inputs)
        .expect("step 5: read with a mismatched visibility");
    cycle.record_observation("step 5 read", &observation);
    let visibility_port = PortName::parse("visibility").expect("`visibility` is a valid port name");
    match &observation {
        Observation::Mismatch { port } => assert_eq!(
            port, &visibility_port,
            "step 5: the mismatch is at the `visibility` port"
        ),
        other => panic!("step 5: expected Mismatch, observed {other:?}"),
    }

    let err = cycle
        .repo_tool
        .ensure(&inputs, &sink_token())
        .expect_err("step 5: ensure on a mismatch must conflict");
    cycle.note(format!("step 5 error: {err:?}"));
    cycle.note(err.message.clone());
    assert_eq!(
        err.kind,
        ToolErrorKind::Conflict,
        "step 5: a visibility mismatch is a Conflict"
    );

    let body = cycle.get_repo_body("step 5");
    assert_eq!(
        body.get("visibility").and_then(Json::as_str),
        Some("private"),
        "step 5: the refused ensure left the repository private"
    );
    cycle.say("step 5 (public requested on a private repository conflicts): pass");
}

/// Step 6: the Actions secret. `Absent`, then `changed: true`, then
/// `Present`, then `changed: true` again — a sink whose value cannot be
/// read back always writes.
fn step_6_secret(cycle: &mut Cycle) {
    let inputs = cycle.secret_inputs();
    let observation = cycle
        .secret_tool
        .read(&inputs)
        .expect("step 6: read of an absent secret");
    cycle.record_observation("step 6 read (absent)", &observation);
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "step 6: an unwritten secret reads Absent"
    );

    let first = cycle
        .secret_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 6: the first ensure failed ({:?})", err.kind));
    cycle.record_ensured("step 6 first ensure", &first);
    assert!(first.changed, "step 6: the first ensure writes");

    let observation = cycle
        .secret_tool
        .read(&inputs)
        .expect("step 6: read of the secret just written");
    cycle.record_observation("step 6 read (present)", &observation);
    assert!(
        matches!(observation, Observation::Present(_)),
        "step 6: a written secret reads Present"
    );

    let second = cycle
        .secret_tool
        .ensure(&inputs, &sink_token())
        .unwrap_or_else(|err| panic!("step 6: the second ensure failed ({:?})", err.kind));
    cycle.record_ensured("step 6 second ensure", &second);
    assert!(
        second.changed,
        "step 6: a sink whose value cannot be read back always writes"
    );
    cycle.say("step 6 (the Actions secret is written, read back, and rewritten): pass");
}

/// Step 7: record the three fixtures the read-only probe could not reach,
/// redacted, and compare each by top-level key set against its authored
/// fixture.
fn step_7_fixtures(cycle: &mut Cycle) {
    let base = repo_path(&cycle.repo);
    let endpoints = [
        (
            "repo_get_present",
            base.clone(),
            "/repos/{org}/{repo}".to_string(),
        ),
        (
            "actions_secret_public_key",
            format!("{base}/actions/secrets/public-key"),
            "/repos/{org}/{repo}/actions/secrets/public-key".to_string(),
        ),
        (
            "actions_secret_get_present",
            format!("{base}/actions/secrets/{}", cycle.secret_name),
            "/repos/{org}/{repo}/actions/secrets/{name}".to_string(),
        ),
    ];

    let mut failures = Vec::new();
    for (fixture, path, display) in endpoints {
        match cycle.raw.get::<Json>(&path) {
            Ok(body) => match record_and_compare(fixture, &body) {
                Ok(()) => cycle.say(&format!("step 7 (GET {display}): pass")),
                Err(diff) => {
                    cycle.say(&format!("step 7 (GET {display}): fail"));
                    failures.push(format!("{display}: {diff}"));
                }
            },
            Err(err) => {
                cycle.say(&format!("step 7 (GET {display}): fail"));
                failures.push(format!(
                    "{display}: request failed, status {:?}",
                    err.status
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "step 7: recorded-fixture drift: {failures:?}"
    );
}

/// Step 8: delete explicitly, confirm the following `GET` is `404`, and
/// disarm the guard.
fn step_8_delete(cycle: &mut Cycle, guard: &mut DeleteGuard) {
    cycle
        .raw
        .delete(&repo_path(&cycle.repo))
        .unwrap_or_else(|err| {
            panic!(
                "step 8: DELETE of `{}` failed (status {:?}); GitHub answers 403 when an \
                 org owner prevents members from deleting organization-owned repositories",
                cycle.repo, err.status
            )
        });
    match cycle.raw.get::<Json>(&repo_path(&cycle.repo)) {
        Err(err) if err.status == Some(404) => {
            guard.disarm();
            cycle.say("step 8 (DELETE succeeds and the following GET is 404): pass");
        }
        Ok(_) => panic!("step 8: `{}` still exists after the DELETE", cycle.repo),
        Err(err) => panic!(
            "step 8: the GET after the DELETE answered an unexpected status {:?}",
            err.status
        ),
    }
}

/// The final redaction sweep: neither the synthetic token's bytes nor
/// anything credential-shaped may appear in a produced string or in a
/// recorded live fixture.
fn sweep_for_secrets(cycle: &Cycle) {
    let mut haystacks: Vec<(String, String)> = cycle
        .sweep
        .iter()
        .enumerate()
        .map(|(index, text)| (format!("produced string {index}"), text.clone()))
        .collect();
    for fixture in [
        "repo_get_present",
        "actions_secret_public_key",
        "actions_secret_get_present",
    ] {
        let path = live_dir().join(format!("{fixture}.json"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading the recorded `{fixture}`: {err}"));
        haystacks.push((format!("the recorded `{fixture}` fixture"), text));
    }

    for (where_, text) in &haystacks {
        assert!(
            !text.contains(&cycle.token_plaintext),
            "the synthetic secret's bytes reached {where_}"
        );
        for prefix in CREDENTIAL_PREFIXES {
            assert!(
                !text.contains(prefix),
                "something credential-shaped (`{prefix}`) reached {where_}"
            );
        }
    }
    println!("redaction sweep over {} strings: pass", haystacks.len());
}

/// The top-level keys whose values differ between two JSON objects.
/// Names keys only, never values: a repository body is not secret, but a
/// failure message is not the place to print one.
fn differing_keys(before: &Json, after: &Json) -> Vec<String> {
    let mut keys: Vec<String> = common::top_level_keys(before)
        .union(&common::top_level_keys(after))
        .cloned()
        .collect();
    keys.retain(|key| before.get(key) != after.get(key));
    keys
}

/// Build the cycle's identities from the sandbox environment.
fn sandbox_repo() -> GitHubRepo {
    let org_value =
        std::env::var("WILLIKINS_SANDBOX_GITHUB_ORG").expect("WILLIKINS_SANDBOX_GITHUB_ORG is set");
    let org = GitHubOrg::parse(&org_value).expect("a valid GitHub org slug");
    let name = ProjectSlug::parse(REPO_NAME).expect("the cycle's fixed name is a valid slug");
    GitHubRepo::new(org, name)
}

#[test]
#[ignore = "opt-in live write cycle against a real GitHub org; creates and deletes a \
            repository. Run with WILLIKINS_LIVE_TESTS=1 and sandbox credentials sourced \
            in the same command"]
fn github_live_write_cycle() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let credential = credential_from_env().expect("a valid sandbox GitHub token");
    let repo = sandbox_repo();
    // Two clients: one the tools own (through `GitHubClient`, which
    // consumes it), one this test keeps for the raw `GET`s and the
    // `DELETE` that no tool in this crate performs.
    let raw = Arc::new(http_client(credential.clone()));
    let client = Arc::new(GitHubClient::new(http_client(credential)));

    step_1_absent(&raw, &repo);

    let mut guard = DeleteGuard::new(Arc::clone(&raw), repo.clone());
    guard.arm();
    println!("step 2 (the delete guard is armed): pass");

    let (token_plaintext, _) = synthetic_token();
    let mut cycle = Cycle {
        raw,
        repo_tool: GitHubRepoEnsure::new(Arc::clone(&client)),
        secret_tool: GitHubActionsSecretEnsure::new(client),
        repo,
        secret_name: ActionsSecretName::parse(SECRET_NAME)
            .expect("the cycle's secret name is valid"),
        token_plaintext,
        sweep: Vec::new(),
    };

    step_3_create(&mut cycle);
    step_4_converged(&mut cycle);
    step_5_visibility_mismatch(&mut cycle);
    step_6_secret(&mut cycle);
    step_7_fixtures(&mut cycle);
    step_8_delete(&mut cycle, &mut guard);
    sweep_for_secrets(&cycle);
}

/// The after-the-run confirmation, read-only: the cycle's repository is
/// gone from the sandbox org. Gated behind a second variable so the
/// cycle's own command never runs the two concurrently.
#[test]
#[ignore = "opt-in read-only check that the write cycle left nothing behind; run with \
            WILLIKINS_LIVE_TESTS=1 and WILLIKINS_LIVE_LEFTOVER_CHECK=1"]
fn the_cycles_repository_is_gone() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1")
        || std::env::var("WILLIKINS_LIVE_LEFTOVER_CHECK").as_deref() != Ok("1")
    {
        println!("skip: WILLIKINS_LIVE_TESTS and WILLIKINS_LIVE_LEFTOVER_CHECK are not both 1");
        return;
    }
    let credential = credential_from_env().expect("a valid sandbox GitHub token");
    let repo = sandbox_repo();
    let raw = http_client(credential);
    match raw.get::<Json>(&repo_path(&repo)) {
        Err(err) if err.status == Some(404) => println!("leftover check (`{repo}` is gone): pass"),
        Ok(_) => panic!("leftover check: `{repo}` still exists and must be deleted by hand"),
        Err(err) => panic!(
            "leftover check: GET of `{repo}` answered an unexpected status {:?}",
            err.status
        ),
    }
}
