//! Acceptance test 18, the live smoke run, as one command.
//!
//! Compiled only with this crate's `live-tests` feature (its `[[test]]`
//! entry in `Cargo.toml` carries `required-features`), so a plain
//! `cargo test --workspace` never builds it. `#[ignore]` on top of that,
//! and inert even under `--ignored` unless `WILLIKINS_LIVE_TESTS=1`: it
//! prints a skip line and returns, having touched nothing.
//!
//! ```text
//! source ~/.config/willikins/sandbox.env && WILLIKINS_LIVE_TESTS=1 \
//!   RUST_TEST_THREADS=2 cargo test -p willikins-cli --features live-tests \
//!   --test live_smoke -j 2 -- --ignored --nocapture
//! ```
//!
//! `--nocapture` matters: every step prints a numbered line, and a
//! reader following a live run against two real accounts wants them.
//! Source the sandbox file **in the same command**; never print it.
//!
//! # What it does, in order
//!
//! Every invocation drives the real `willikins` binary
//! (`CARGO_BIN_EXE_willikins`) with `--live`, `--json`, `--principal
//! smoke-operator`, and the same `--journal` at a persistent path under
//! the system temporary directory, printed on stdout as `journal:
//! <path>` before anything else. The journal is deliberately **not**
//! removed when the test ends: `deploy/teardown.sh` reads the created
//! resources' names out of a run record, and a run record that no longer
//! exists cannot be torn down.
//!
//! 1. **Pre-flight, before anything is created.** `gh api -i user` must
//!    answer an `x-oauth-scopes` header containing `delete_repo`,
//!    because the teardown script deletes the repository through `gh
//!    api` with `gh`'s own credential, and a dry run cannot reveal that
//!    scope's absence. Both `WILLIKINS_GITHUB_TOKEN` and
//!    `WILLIKINS_DOPPLER_TOKEN` must be set (presence only -- neither
//!    value is ever read for anything but the leak sweep). The
//!    repository and the Doppler project must both answer `404`: a
//!    leftover from an earlier run is a stop, not something to reuse.
//!    `jq`, which the teardown script reads every document with, must be
//!    on `PATH`.
//! 2. **First apply** of `workflows/new-rust-service.yaml` with `slug=`
//!    [`common::SLUG`] and `org=`[`common::ORG`].
//! 3. **Second, identical apply**: the convergence claim.
//! 4. **The rotation without `--approve`**: refused `ApprovalRequired`,
//!    with no run started.
//! 5. **The rotation with `--approve`**.
//! 6. **Teardown through `deploy/teardown.sh`**, as a dry run and then
//!    with `--yes`, followed by a leftover check that does not go
//!    through the script at all.
//!
//! # Credentials
//!
//! The child inherits this process's environment; nothing calls
//! `env_clear()`, and nothing passes a token on a command line. The one
//! place a credential's bytes are read is
//! `common::assert_no_credential_bytes`' own narrow helper, which
//! answers a `bool` and drops the value -- see its doc for why that
//! deliberate departure from the provider write cycles' never-read rule
//! exists.
//!
//! # Offline proof
//!
//! Every expectation table and JSON reader this file uses lives in
//! `tests/common/mod.rs` and is exercised on every workspace gate by
//! `tests/smoke_parity.rs`, which makes the same four invocations
//! against the fake catalog. The live run is therefore the first time
//! only the *providers* differ.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value as Json;

use common::{
    FIRST_APPLY, Invocation, ORG, PRINCIPAL, PROJECT, REPO, ROTATION, SECOND_APPLY, SLUG,
    assert_no_credential_bytes, assert_nodes, assert_repo_url, assert_token_unknown,
    positive_inputs, recorded_run_count, rotation_inputs, willikins, workspace_path,
    workspace_root,
};

/// The GitHub scope `deploy/teardown.sh` needs on `gh`'s own credential.
const DELETE_REPO_SCOPE: &str = "delete_repo";

/// What to tell the operator when a step after the first apply fails:
/// the run is real, its resources exist, and the teardown script is the
/// way to remove them.
///
/// Armed as soon as the first run's id is known and disarmed only after
/// the leftover check has passed, so a panic anywhere in between still
/// prints the line. The same guard shape both provider write cycles use.
struct TeardownHint {
    run_id: Option<String>,
    journal: PathBuf,
    armed: bool,
}

impl TeardownHint {
    fn new(journal: &Path) -> Self {
        Self {
            run_id: None,
            journal: journal.to_path_buf(),
            armed: true,
        }
    }

    fn arm(&mut self, run_id: &str) {
        self.run_id = Some(run_id.to_string());
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TeardownHint {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let journal = self.journal.display();
        match &self.run_id {
            Some(run_id) => println!("teardown: run {run_id} --journal {journal}"),
            None => println!(
                "teardown: no run id was recorded; the journal is {journal} \
                 (check both accounts by hand)"
            ),
        }
    }
}

/// Run `gh` with `args` and hand back the whole [`Output`]; `gh` exits
/// non-zero on an HTTP error, which several callers here expect.
fn gh(args: &[&str]) -> Output {
    Command::new("gh")
        .args(args)
        .output()
        .expect("failed to run `gh`; install the GitHub CLI and authenticate it")
}

/// The HTTP status of a `gh api -i` response.
///
/// Read from the status line `-i` prints on stdout; a plain non-zero
/// exit is not enough, because it cannot tell `404` (what a fresh run
/// needs) from `401` (a dead `gh` credential). `gh` also names the
/// status in its stderr message (`gh: Not Found (HTTP 404)`), which is
/// the fallback when the status line is missing.
fn gh_status(output: &Output) -> Option<u16> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(line) = stdout.lines().next()
        && line.starts_with("HTTP/")
        && let Some(code) = line.split_whitespace().nth(1)
        && let Ok(code) = code.parse::<u16>()
    {
        return Some(code);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let start = stderr.find("(HTTP ")? + "(HTTP ".len();
    stderr[start..].split(')').next()?.trim().parse().ok()
}

/// One response header of a `gh api -i` response, by lower-case name.
fn gh_header(output: &Output, name: &str) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.trim().is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':')
            && key.trim().eq_ignore_ascii_case(name)
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Step 1: `gh`'s own credential must carry `delete_repo`.
///
/// A fine-grained personal access token used for `gh auth login` carries
/// no `X-OAuth-Scopes` header at all, so a missing header and an empty
/// one are the same failure, and the message says so: the fix is to
/// authenticate `gh` with a token that has the scope.
fn require_delete_repo_scope() {
    let output = gh(&["api", "-i", "user"]);
    let status = gh_status(&output);
    assert!(
        status == Some(200),
        "pre-flight: `gh api -i user` answered {status:?}; `gh` is not authenticated. \
         Run `gh auth login`, then \
         `gh auth refresh -h github.com -s {DELETE_REPO_SCOPE}`."
    );
    let scopes = gh_header(&output, "x-oauth-scopes").unwrap_or_default();
    assert!(
        scopes
            .split(',')
            .any(|scope| scope.trim() == DELETE_REPO_SCOPE),
        "pre-flight: `gh`'s credential does not carry the `{DELETE_REPO_SCOPE}` scope, \
         so `deploy/teardown.sh` could not delete the repository it is about to create. \
         `gh api -i user` reported the scopes `{scopes}` (a fine-grained personal access \
         token reports none at all). Fix it with \
         `gh auth refresh -h github.com -s {DELETE_REPO_SCOPE}`, then run this test again. \
         Nothing has been created."
    );
    println!("1a. gh scope pre-flight: `{DELETE_REPO_SCOPE}` present");
}

/// Step 1: both provider credentials must be set. Presence only -- the
/// CLI's own `--live` path parses and validates them, and a second read
/// here would make a plaintext copy for nothing.
fn require_credentials_present() {
    for name in ["WILLIKINS_GITHUB_TOKEN", "WILLIKINS_DOPPLER_TOKEN"] {
        assert!(
            std::env::var_os(name).is_some_and(|value| !value.is_empty()),
            "pre-flight: {name} is not set. Source \
             ~/.config/willikins/sandbox.env in the same command as this test."
        );
        println!("1b. {name}: set");
    }
}

/// Step 1: `deploy/teardown.sh` pipes every document it reads through
/// `jq`. A missing `jq` would fail step 6, after both resources exist --
/// so it is a pre-flight question, not a teardown-time one.
fn require_jq() {
    let found = Command::new("jq")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    assert!(
        found,
        "pre-flight: `jq` is not on PATH, and `deploy/teardown.sh` needs it to read \
         the run record. Install it before running this test. Nothing has been created."
    );
    println!("1c. jq: present");
}

/// Step 1: the repository must not exist yet.
fn require_repository_absent() {
    let output = gh(&["api", "-i", &format!("repos/{REPO}")]);
    let status = gh_status(&output);
    assert!(
        status == Some(404),
        "pre-flight: `repos/{REPO}` answered {status:?}, expected 404. \
         A leftover from an earlier run is the operator's to remove \
         (`deploy/teardown.sh <run-id> <journal> --yes`, or by hand); \
         this test never reuses one. Nothing has been created."
    );
    println!("1d. GET repos/{REPO}: 404, as a fresh run needs");
}

/// A read-only Doppler client built from the sandbox credential, for the
/// two checks `gh` cannot make.
fn doppler() -> willikins_providers_http::Http {
    let credential = willikins_providers_doppler::credential_from_env()
        .expect("a valid sandbox Doppler token (source ~/.config/willikins/sandbox.env)");
    willikins_providers_doppler::http_client(credential)
}

/// `GET /v3/projects/project?project=<project>`: `Some(status)` for a
/// failure, `None` when the project reads back.
fn doppler_project_status(http: &willikins_providers_http::Http) -> Option<u16> {
    match http.get::<Json>(&format!("/v3/projects/project?project={PROJECT}")) {
        Ok(_) => None,
        Err(err) => Some(err.status.unwrap_or(0)),
    }
}

/// Step 1: the Doppler project must not exist yet, for the same reason
/// the repository must not.
fn require_project_absent(http: &willikins_providers_http::Http) {
    let status = doppler_project_status(http);
    assert!(
        status == Some(404),
        "pre-flight: the Doppler project `{PROJECT}` answered {status:?}, expected a 404. \
         A leftover is the operator's to remove; this test never reuses one. \
         Nothing has been created."
    );
    println!("1e. GET /v3/projects/project ({PROJECT}): 404, as a fresh run needs");
}

/// `willikins --json apply <document> --live --journal <j> --principal
/// smoke-operator <inputs> [extra]`.
fn apply(document: &str, journal: &str, inputs: &[String], extra: &[&str]) -> Invocation {
    let document = workspace_path(document);
    let mut args: Vec<&str> = vec![
        "--json",
        "apply",
        document.as_str(),
        "--live",
        "--journal",
        journal,
        "--principal",
        PRINCIPAL,
    ];
    args.extend(inputs.iter().map(String::as_str));
    args.extend(extra.iter().copied());
    willikins(&args, &[])
}

/// Run `deploy/teardown.sh <run-id> <journal> [--yes]` with
/// `WILLIKINS_BIN` pointing at the binary this test drives, so the
/// script reads the same run record this test wrote.
fn teardown(run_id: &str, journal: &str, yes: bool) -> Output {
    let script = workspace_root().join("deploy").join("teardown.sh");
    let mut command = Command::new("bash");
    command
        .arg(&script)
        .arg(run_id)
        .arg(journal)
        .env("WILLIKINS_BIN", env!("CARGO_BIN_EXE_willikins"))
        .current_dir(workspace_root());
    if yes {
        command.arg("--yes");
    }
    command.output().expect("failed to run deploy/teardown.sh")
}

#[test]
#[ignore = "opt-in live smoke run against the sandbox GitHub org and Doppler workplace; \
            creates and deletes a repository and a project. Run with WILLIKINS_LIVE_TESTS=1, \
            --features live-tests, and sandbox credentials sourced in the same command"]
#[allow(clippy::too_many_lines)]
fn live_smoke_run() {
    if std::env::var("WILLIKINS_LIVE_TESTS").as_deref() != Ok("1") {
        println!("skip: WILLIKINS_LIVE_TESTS is not 1");
        return;
    }

    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs();
    let dir = std::env::temp_dir().join(format!("willikins-smoke-{seconds}"));
    std::fs::create_dir_all(&dir).expect("a journal directory");
    let journal_path = dir.join("journal.jsonl");
    let journal = journal_path
        .to_str()
        .expect("a UTF-8 journal path")
        .to_string();
    // First thing on stdout, before any provider is touched: the run
    // record is the only thing `deploy/teardown.sh` can read names from,
    // so the operator needs this path even if the next line panics.
    println!("journal: {journal}");

    // --- 1. pre-flight ----------------------------------------------
    require_delete_repo_scope();
    require_credentials_present();
    require_jq();
    require_repository_absent();
    let http = doppler();
    require_project_absent(&http);

    let mut hint = TeardownHint::new(&journal_path);

    // --- 2. first apply ---------------------------------------------
    println!("2. apply new-rust-service (slug={SLUG}, org={ORG}) --live");
    let first = apply(
        "workflows/new-rust-service.yaml",
        &journal,
        &positive_inputs(),
        &[],
    );
    let first_run_id = first
        .docs
        .iter()
        .rev()
        .find_map(|doc| doc.get("run_id").and_then(Json::as_str))
        .map(str::to_string);
    if let Some(run_id) = &first_run_id {
        hint.arm(run_id);
    }
    // The sweep comes before every assertion whose message prints a
    // stream: a failed live apply is exactly the case where an
    // unexpected byte could be in one, and a panic message is printed.
    assert_no_credential_bytes(
        "first apply",
        &[("stdout", &first.stdout), ("stderr", &first.stderr)],
    );
    assert_eq!(
        first.code(),
        0,
        "first apply failed:\nstdout:\n{}\nstderr:\n{}",
        first.stdout,
        first.stderr
    );
    let record = first.run_record();
    assert_eq!(
        record["state"].as_str(),
        Some("succeeded"),
        "first apply: {record}"
    );
    assert_nodes(record, FIRST_APPLY, "first apply");
    assert_repo_url(record, "first apply");
    let first_run_id = first.run_id();
    println!("2. ok: run {first_run_id}");

    // --- 3. second apply, identical ---------------------------------
    println!("3. apply new-rust-service again (the convergence claim)");
    let second = apply(
        "workflows/new-rust-service.yaml",
        &journal,
        &positive_inputs(),
        &[],
    );
    assert_no_credential_bytes(
        "second apply",
        &[("stdout", &second.stdout), ("stderr", &second.stderr)],
    );
    assert_eq!(
        second.code(),
        0,
        "second apply failed:\nstdout:\n{}\nstderr:\n{}",
        second.stdout,
        second.stderr
    );
    let record = second.run_record();
    assert_eq!(
        record["state"].as_str(),
        Some("succeeded"),
        "second apply: {record}"
    );
    assert_nodes(record, SECOND_APPLY, "second apply");
    assert_token_unknown(record, "second apply");
    assert_repo_url(record, "second apply");
    println!("3. ok");

    // --- 4. the rotation, refused without approval ------------------
    println!("4. apply rotate-service-token without --approve (must be refused)");
    let before = recorded_run_count(&journal);
    let refused = apply(
        "workflows/rotate-service-token.yaml",
        &journal,
        &rotation_inputs(),
        &[],
    );
    assert_no_credential_bytes(
        "refused rotation",
        &[("stdout", &refused.stdout), ("stderr", &refused.stderr)],
    );
    assert_eq!(
        refused.code(),
        1,
        "the unapproved rotation was not refused:\nstdout:\n{}\nstderr:\n{}",
        refused.stdout,
        refused.stderr
    );
    let error = refused
        .docs
        .iter()
        .find(|doc| doc.get("kind").is_some())
        .unwrap_or_else(|| panic!("no error document: {}", refused.stdout));
    assert_eq!(error["kind"].as_str(), Some("ApprovalRequired"), "{error}");
    assert_eq!(error["class"].as_str(), Some("destructive"), "{error}");
    assert_eq!(
        recorded_run_count(&journal),
        before,
        "the refused rotation started a run"
    );
    println!("4. ok: ApprovalRequired, no run started");

    // --- 5. the rotation, applied with approval ---------------------
    println!("5. apply rotate-service-token with --approve");
    let rotated = apply(
        "workflows/rotate-service-token.yaml",
        &journal,
        &rotation_inputs(),
        &["--approve"],
    );
    assert_no_credential_bytes(
        "rotation",
        &[("stdout", &rotated.stdout), ("stderr", &rotated.stderr)],
    );
    assert_eq!(
        rotated.code(),
        0,
        "rotation failed:\nstdout:\n{}\nstderr:\n{}",
        rotated.stdout,
        rotated.stderr
    );
    let record = rotated.run_record();
    assert_eq!(
        record["state"].as_str(),
        Some("succeeded"),
        "rotation: {record}"
    );
    assert_nodes(record, ROTATION, "rotation");
    let rotation_run_id = rotated.run_id();
    println!("5. ok: run {rotation_run_id}");

    // --- 6. teardown, through the script the plan's acceptance names -
    println!("6a. deploy/teardown.sh {first_run_id} {journal} (dry run)");
    let dry = teardown(&first_run_id, &journal, false);
    let dry_out = String::from_utf8_lossy(&dry.stdout).into_owned();
    let dry_err = String::from_utf8_lossy(&dry.stderr).into_owned();
    assert_no_credential_bytes(
        "teardown dry run",
        &[("stdout", &dry_out), ("stderr", &dry_err)],
    );
    assert!(
        dry.status.success(),
        "the dry run failed:\nstdout:\n{dry_out}\nstderr:\n{dry_err}"
    );
    // Line by line, and each line has to name its own resource:
    // `PROJECT` is a substring of `REPO`, so a bare `contains` would
    // pass on the repository line alone.
    assert!(
        dry_out
            .lines()
            .any(|line| line.contains("GitHub repository") && line.contains(REPO)),
        "the dry run did not name the repository: {dry_out}"
    );
    assert!(
        dry_out
            .lines()
            .any(|line| line.contains("Doppler project") && line.trim_end().ends_with(PROJECT)),
        "the dry run did not name the Doppler project: {dry_out}"
    );
    println!("6a. ok: it names {REPO} and {PROJECT}");

    println!("6b. deploy/teardown.sh {first_run_id} {journal} --yes");
    let deleted = teardown(&first_run_id, &journal, true);
    let deleted_out = String::from_utf8_lossy(&deleted.stdout).into_owned();
    let deleted_err = String::from_utf8_lossy(&deleted.stderr).into_owned();
    assert_no_credential_bytes(
        "teardown",
        &[("stdout", &deleted_out), ("stderr", &deleted_err)],
    );
    assert!(
        deleted.status.success(),
        "the teardown failed:\nstdout:\n{deleted_out}\nstderr:\n{deleted_err}"
    );
    println!("6b. ok");

    // The leftover check does not go through the script: the script
    // reporting success and the two accounts agreeing are different
    // claims, and only the second one is the one that matters.
    println!("6c. leftover check, independent of the script");
    let repo_after = gh(&["api", "-i", &format!("repos/{REPO}")]);
    let repo_status = gh_status(&repo_after);
    assert!(
        repo_status == Some(404),
        "leftover: `repos/{REPO}` answered {repo_status:?} after the teardown; \
         delete it by hand"
    );
    let project_after = doppler_project_status(&http);
    assert!(
        project_after == Some(404),
        "leftover: the Doppler project `{PROJECT}` answered {project_after:?} after the \
         teardown; delete it by hand"
    );
    println!("6c. ok: both are gone");

    hint.disarm();

    // --- 7. the record the coordinator keeps ------------------------
    println!("first apply run id:  {first_run_id}");
    println!("rotation run id:     {rotation_run_id}");
    println!("journal:             {journal}");
}
