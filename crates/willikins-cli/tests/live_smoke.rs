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
//! removed when the test ends: `willikins rollback <file> --run
//! <run-id>` reads the created resources' names out of a run record,
//! and a run record that no longer exists cannot be torn down.
//!
//! 1. **Pre-flight, before anything is created.** `WILLIKINS_GITHUB_TOKEN`,
//!    `WILLIKINS_DOPPLER_TOKEN`, `WILLIKINS_BUILDKITE_TOKEN`,
//!    `WILLIKINS_SIGNOZ_API_KEY` and `WILLIKINS_SIGNOZ_HOST` must all be
//!    set (presence only -- no value is ever read for anything but the
//!    leak sweep). The first two are what this run's own document
//!    calls; the other three are needed only at step 6c, because
//!    `rollback --plan-id --live` builds the full live catalog the same
//!    unconditional way `apply --plan-id --live` does, so a missing
//!    Buildkite or `SigNoz` credential would refuse there -- after both
//!    resources already exist. The repository must answer `404` and the
//!    sandbox must not list the Doppler project (Doppler answers a read
//!    of a missing project with 400): a leftover from an earlier run is a
//!    stop, not something to reuse.
//! 2. **First apply** of a scratch copy of `workflows/new-rust-service.yaml`
//!    with `teardown: delete` added to `repo`, `doppler`, `configs` and
//!    `token` (decision 3a's default is `retain`; the tracked document
//!    keeps it, so this copy is what actually admits a teardown), with
//!    `slug=`[`common::SLUG`] and `org=`[`common::ORG`].
//! 3. **Second, identical apply**: the convergence claim.
//! 4. **The rotation without `--approve`**: refused `ApprovalRequired`,
//!    with no run started.
//! 5. **The rotation with `--approve`**.
//! 6. **Teardown through `willikins rollback`**: `rollback <file> --run
//!    <run-id> --live --journal <j>` plans it (6a), `approve <plan_id>
//!    --journal <j>` approves it (6b), `rollback --plan-id <plan_id>
//!    --live --journal <j> --workflows-dir <dir>` applies it (6c),
//!    followed by a leftover check that does not go through willikins at
//!    all (6d).
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
//! Every expectation table and JSON reader the apply/rotation steps use
//! lives in `tests/common/mod.rs` and is exercised on every workspace
//! gate by `tests/it/smoke_parity.rs`, which makes the same four
//! invocations against the fake catalog. `willikins rollback`'s own
//! JSON shapes are pinned offline by `tests/it/rollback_round_trip.rs`.
//! The live run is therefore the first time only the *providers* differ.

mod common;

use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use common::{
    FIRST_APPLY, Invocation, ORG, PRINCIPAL, PROJECT, REPO, ROTATION, SECOND_APPLY, SLUG,
    assert_no_credential_bytes, assert_nodes, assert_repo_url, assert_token_unknown,
    positive_inputs, recorded_run_count, rotation_inputs, willikins, workspace_path,
    workspace_root,
};

/// What to tell the operator when a step after the first apply fails:
/// the run is real, its resources exist, and `willikins rollback` is the
/// way to remove them.
///
/// Armed as soon as the first run's id is known and disarmed only after
/// the leftover check has passed, so a panic anywhere in between still
/// prints the line. The same guard shape both provider write cycles use.
struct TeardownHint {
    run_id: Option<String>,
    journal: PathBuf,
    document: PathBuf,
    workflows_dir: PathBuf,
    armed: bool,
}

impl TeardownHint {
    fn new(journal: &Path, document: &Path, workflows_dir: &Path) -> Self {
        Self {
            run_id: None,
            journal: journal.to_path_buf(),
            document: document.to_path_buf(),
            workflows_dir: workflows_dir.to_path_buf(),
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
            Some(run_id) => println!(
                "rollback: willikins rollback {} --run {run_id} --live --journal {journal}, \
                 then `approve <plan_id> --journal {journal}`, then `rollback --plan-id \
                 <plan_id> --live --journal {journal} --workflows-dir {}`",
                self.document.display(),
                self.workflows_dir.display()
            ),
            None => println!(
                "teardown: no run id was recorded; the journal is {journal} \
                 (check both accounts by hand)"
            ),
        }
    }
}

/// Step 1: every credential the full live catalog needs, even though
/// this run's own document calls only the first two. Presence only --
/// the CLI's own `--live` path parses and validates them, and a second
/// read here would make a plaintext copy for nothing.
fn require_credentials_present() {
    for name in [
        "WILLIKINS_GITHUB_TOKEN",
        "WILLIKINS_DOPPLER_TOKEN",
        "WILLIKINS_BUILDKITE_TOKEN",
        "WILLIKINS_SIGNOZ_API_KEY",
        "WILLIKINS_SIGNOZ_HOST",
    ] {
        assert!(
            std::env::var_os(name).is_some_and(|value| !value.is_empty()),
            "pre-flight: {name} is not set. Source \
             ~/.config/willikins/sandbox.env in the same command as this test."
        );
        println!("1a. {name}: set");
    }
}

/// A read-only GitHub client built from the sandbox PAT.
fn github() -> willikins_providers_http::Http {
    let credential = willikins_providers_github::credential_from_env()
        .expect("a valid sandbox GitHub token (source ~/.config/willikins/sandbox.env)");
    willikins_providers_github::http_client(credential)
}

/// `GET /repos/<owner>/<name>`: `Some(status)` for a failure, `None`
/// when the repository reads back.
fn github_repo_status(http: &willikins_providers_http::Http) -> Option<u16> {
    match http.get::<Json>(&format!("/repos/{REPO}")) {
        Ok(_) => None,
        Err(err) => Some(err.status.unwrap_or(0)),
    }
}

/// Step 1: the repository must not exist yet.
fn require_repository_absent(http: &willikins_providers_http::Http) {
    let status = github_repo_status(http);
    assert!(
        status == Some(404),
        "pre-flight: `repos/{REPO}` answered {status:?}, expected 404. \
         A leftover from an earlier run is the operator's to remove \
         (`willikins rollback <file> --run <run-id> --live --journal <journal>`, then \
         `approve`, then `rollback --plan-id`, or by hand); this test never reuses one. \
         Nothing has been created."
    );
    println!("1b. GET repos/{REPO}: 404, as a fresh run needs");
}

/// A read-only Doppler client built from the sandbox credential.
fn doppler() -> willikins_providers_http::Http {
    let credential = willikins_providers_doppler::credential_from_env()
        .expect("a valid sandbox Doppler token (source ~/.config/willikins/sandbox.env)");
    willikins_providers_doppler::http_client(credential)
}

/// Whether the sandbox lists a Doppler project named [`PROJECT`]: `GET
/// /v3/projects`, read by slug. Doppler answers a read of a project that
/// does not exist with 400 ("This token does not have access to requested
/// project"), not 404, so absence is decided from the listing instead
/// (milestone 3m's L2 and L4 saw the same answer).
fn doppler_project_listed(http: &willikins_providers_http::Http) -> bool {
    #[derive(serde::Deserialize)]
    struct Listing {
        projects: Vec<Slug>,
    }
    #[derive(serde::Deserialize)]
    struct Slug {
        slug: String,
    }
    let listing: Listing = http
        .get("/v3/projects?per_page=100")
        .unwrap_or_else(|err| panic!("listing Doppler projects failed (status {:?})", err.status));
    listing
        .projects
        .iter()
        .any(|project| project.slug == PROJECT)
}

/// Step 1: the Doppler project must not exist yet, for the same reason
/// the repository must not.
fn require_project_absent(http: &willikins_providers_http::Http) {
    assert!(
        !doppler_project_listed(http),
        "pre-flight: the sandbox lists a Doppler project `{PROJECT}`. \
         A leftover is the operator's to remove; this test never reuses one. \
         Nothing has been created."
    );
    println!("1c. GET /v3/projects: no `{PROJECT}`, as a fresh run needs");
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

/// This run's own `Delete`-capable nodes, by the tool that makes them
/// candidates at all (decision 3a). `ci_secret` (`github.actions_secret.ensure`,
/// `Contained`) is deliberately absent: `check` refuses `teardown:` on a
/// `Contained` node (`CheckError::TeardownNotAdmitted`), and it rides
/// with `repo` regardless of what it says.
const DELETABLE_TOOLS: [&str; 4] = [
    "github.repo.ensure",
    "doppler.project.ensure",
    "doppler.config.ensure",
    "doppler.service_token.ensure",
];

/// A copy of the tracked `document`'s own bytes with `teardown: delete`
/// inserted right after each of [`DELETABLE_TOOLS`]'s own `tool:` line,
/// at that line's own indentation.
///
/// Decision 3a: the default is `retain`, and a `Delete`-capable candidate
/// riding inside a container this run deletes (`configs` inside
/// `doppler`, `token` inside `configs[prd]`) must itself say `teardown:
/// delete` or the whole teardown refuses ("a node the document keeps is
/// inside a container it deletes"). So this run cannot roll anything
/// back from the tracked document as committed -- a scratch copy with
/// the policy added is what milestone 3m's own L4 runbook used
/// (`docs/plans/2026-10-05-milestone-3m-safe-rollback.md`, 2026-10-16
/// addendum) and what this test now automates.
///
/// Read from disk and transformed, rather than hardcoded, so this test
/// cannot silently drift from `common::FIRST_APPLY`/`SECOND_APPLY`
/// (which describe the *tracked* document's own shape) the way a second,
/// hand-maintained copy could.
fn teardown_delete_copy(document: &str) -> String {
    let mut out = String::with_capacity(document.len() + DELETABLE_TOOLS.len() * 24);
    let mut inserted = 0usize;
    for line in document.lines() {
        out.push_str(line);
        out.push('\n');
        let trimmed = line.trim_start();
        if let Some(tool) = trimmed.strip_prefix("tool: ")
            && DELETABLE_TOOLS.contains(&tool)
        {
            out.push_str(&line[..line.len() - trimmed.len()]);
            out.push_str("teardown: delete\n");
            inserted += 1;
        }
    }
    assert_eq!(
        inserted,
        DELETABLE_TOOLS.len(),
        "expected exactly {} `teardown: delete` insertions in:\n{document}",
        DELETABLE_TOOLS.len()
    );
    out
}

/// `willikins --json rollback <document> --run <run-id> --live --journal
/// <j> --principal smoke-operator`: plans a teardown of what `run-id`
/// created.
fn rollback_plan(document: &str, run_id: &str, journal: &str) -> Invocation {
    willikins(
        &[
            "--json",
            "rollback",
            document,
            "--run",
            run_id,
            "--live",
            "--journal",
            journal,
            "--principal",
            PRINCIPAL,
        ],
        &[],
    )
}

/// `willikins approve <plan-id> --journal <j> --principal
/// smoke-operator`.
fn approve_plan(plan_id: &str, journal: &str) -> Invocation {
    willikins(
        &[
            "approve",
            plan_id,
            "--journal",
            journal,
            "--principal",
            PRINCIPAL,
        ],
        &[],
    )
}

/// `willikins --json rollback --plan-id <plan-id> --live --journal <j>
/// --workflows-dir <dir> --principal smoke-operator`: applies an
/// approved teardown. Builds the **full** live catalog
/// (`willikins_server::live_catalog_from_env`, unconditional), exactly
/// as `apply --plan-id --live` does, so it needs every provider's
/// credential even though this document calls only two of them -- see
/// `require_credentials_present`.
fn rollback_apply(plan_id: &str, journal: &str, workflows_dir: &str) -> Invocation {
    willikins(
        &[
            "--json",
            "rollback",
            "--plan-id",
            plan_id,
            "--live",
            "--journal",
            journal,
            "--workflows-dir",
            workflows_dir,
            "--principal",
            PRINCIPAL,
        ],
        &[],
    )
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
    // record is the only thing `willikins rollback` can read names from,
    // so the operator needs this path even if the next line panics.
    println!("journal: {journal}");

    // A scratch copy of `workflows/new-rust-service.yaml` with
    // `teardown: delete` added, so this run's own rollback can delete
    // what it creates (decision 3a; see `teardown_delete_copy`'s doc).
    let tracked = std::fs::read_to_string(
        workspace_root()
            .join("workflows")
            .join("new-rust-service.yaml"),
    )
    .expect("the tracked new-rust-service.yaml document");
    let document_path = dir.join("new-rust-service.yaml");
    std::fs::write(&document_path, teardown_delete_copy(&tracked))
        .expect("a scratch copy of new-rust-service.yaml");
    let document = document_path
        .to_str()
        .expect("a UTF-8 scratch document path")
        .to_string();
    let workflows_dir = dir
        .to_str()
        .expect("a UTF-8 scratch workflows directory")
        .to_string();

    // --- 1. pre-flight ----------------------------------------------
    require_credentials_present();
    let github_http = github();
    require_repository_absent(&github_http);
    let http = doppler();
    require_project_absent(&http);

    let mut hint = TeardownHint::new(&journal_path, &document_path, &dir);

    // --- 2. first apply ---------------------------------------------
    println!("2. apply new-rust-service (slug={SLUG}, org={ORG}) --live");
    let first = apply(&document, &journal, &positive_inputs(), &[]);
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
    let second = apply(&document, &journal, &positive_inputs(), &[]);
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

    // --- 6. teardown, through `willikins rollback` -------------------
    println!("6a. rollback {document} --run {first_run_id} --live --journal {journal}");
    let planned = rollback_plan(&document, &first_run_id, &journal);
    assert_no_credential_bytes(
        "rollback plan",
        &[("stdout", &planned.stdout), ("stderr", &planned.stderr)],
    );
    assert_eq!(
        planned.code(),
        0,
        "rollback plan failed:\nstdout:\n{}\nstderr:\n{}",
        planned.stdout,
        planned.stderr
    );
    let teardown_plan = planned
        .docs
        .first()
        .unwrap_or_else(|| panic!("no teardown plan document: {}", planned.stdout));
    let plan_id = teardown_plan["plan_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no plan_id in {teardown_plan}"))
        .to_string();
    let steps = teardown_plan["teardown"]["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("no teardown.steps in {teardown_plan}"));
    let step_for = |node: &str| {
        steps
            .iter()
            .find(|step| step["instance"]["node"] == node)
            .unwrap_or_else(|| panic!("no {node} step in {steps:?}"))
    };
    let repo_step = step_for("repo");
    assert_eq!(repo_step["action"]["kind"], "delete", "{repo_step:?}");
    assert_eq!(
        repo_step["resource"]["repo"]["value"], REPO,
        "{repo_step:?}"
    );
    let doppler_step = step_for("doppler");
    assert_eq!(doppler_step["action"]["kind"], "delete", "{doppler_step:?}");
    assert_eq!(
        doppler_step["resource"]["project"]["value"], PROJECT,
        "{doppler_step:?}"
    );
    println!("6a. ok: plan {plan_id} deletes repo and doppler");

    println!("6b. approve {plan_id} --journal {journal}");
    let approved = approve_plan(&plan_id, &journal);
    assert_no_credential_bytes(
        "approve",
        &[("stdout", &approved.stdout), ("stderr", &approved.stderr)],
    );
    assert_eq!(
        approved.code(),
        0,
        "approve failed:\nstdout:\n{}\nstderr:\n{}",
        approved.stdout,
        approved.stderr
    );
    println!("6b. ok");

    println!(
        "6c. rollback --plan-id {plan_id} --live --journal {journal} --workflows-dir \
         {workflows_dir}"
    );
    let deleted = rollback_apply(&plan_id, &journal, &workflows_dir);
    assert_no_credential_bytes(
        "rollback apply",
        &[("stdout", &deleted.stdout), ("stderr", &deleted.stderr)],
    );
    assert_eq!(
        deleted.code(),
        0,
        "rollback apply failed:\nstdout:\n{}\nstderr:\n{}",
        deleted.stdout,
        deleted.stderr
    );
    let teardown_run = deleted
        .docs
        .first()
        .unwrap_or_else(|| panic!("no teardown run document: {}", deleted.stdout));
    assert_eq!(teardown_run["succeeded"], true, "{teardown_run:?}");
    let run_steps = teardown_run["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("no steps in {teardown_run:?}"));
    let repo_outcome = run_steps
        .iter()
        .find(|pair| pair[0]["node"] == "repo")
        .unwrap_or_else(|| panic!("no repo step in {teardown_run:?}"));
    assert_eq!(repo_outcome[1]["kind"], "deleted", "{repo_outcome:?}");
    let doppler_outcome = run_steps
        .iter()
        .find(|pair| pair[0]["node"] == "doppler")
        .unwrap_or_else(|| panic!("no doppler step in {teardown_run:?}"));
    assert_eq!(doppler_outcome[1]["kind"], "deleted", "{doppler_outcome:?}");
    println!("6c. ok");

    // The leftover check does not go through willikins: willikins
    // reporting success and the two accounts agreeing are different
    // claims, and only the second one is the one that matters.
    println!("6d. leftover check, independent of willikins");
    let repo_status = github_repo_status(&github_http);
    assert!(
        repo_status == Some(404),
        "leftover: `repos/{REPO}` answered {repo_status:?} after the teardown; \
         delete it by hand"
    );
    assert!(
        !doppler_project_listed(&http),
        "leftover: the sandbox still lists the Doppler project `{PROJECT}` after the \
         teardown; delete it by hand"
    );
    println!("6d. ok: both are gone");

    hint.disarm();

    // --- 7. the record the coordinator keeps ------------------------
    println!("first apply run id:  {first_run_id}");
    println!("rotation run id:     {rotation_run_id}");
    println!("journal:             {journal}");
}
