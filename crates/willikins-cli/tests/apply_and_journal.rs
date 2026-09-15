//! Task 11: process-level tests for `apply`, `approve`, `reject`, `runs`,
//! and `run`, driving the built `willikins` binary exactly the way
//! `tests/cli.rs` drives `validate`/`describe`/`plan`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn workflow(rel: &str) -> PathBuf {
    workspace_root().join(rel)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
        .env_clear()
        .output()
        .expect("failed to run the willikins binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("process was not signalled")
}

/// Every top-level JSON value concatenated in `text`, in order: `apply`
/// prints the `PlanResponse` and then the `RunRecord` (or a refusal) as
/// two separate pretty-printed documents on stdout, not one envelope.
fn json_documents(text: &str) -> Vec<serde_json::Value> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<serde_json::Value>()
        .map(|result| result.unwrap_or_else(|err| panic!("invalid JSON in {text:?}: {err}")))
        .collect()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "willikins-cli-apply-test-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------
// apply <file>: the positive fixture
// ---------------------------------------------------------------------

#[test]
fn apply_the_positive_fixture_creates_every_new_resource() {
    let output = run(&[
        "apply",
        workflow("workflows/new-rust-service.yaml")
            .to_str()
            .unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
    ]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("state: succeeded"), "text: {text}");
    assert!(text.contains("Created"), "text: {text}");
    // Doppler's fake auto-creates the three default root configs with the
    // project, so the planned `Create` for each finds them already
    // present -- `Unchanged`, not `Created` (acceptance test 5).
    assert!(text.contains("Unchanged"), "text: {text}");
    // The freshly minted service token is a secret: only its redaction
    // marker may ever appear.
    assert!(
        text.contains("[REDACTED DopplerServiceToken]"),
        "text: {text}"
    );
}

#[test]
fn apply_the_positive_fixture_json_prints_a_plan_response_then_a_run_record() {
    let output = run(&[
        "--json",
        "apply",
        workflow("workflows/new-rust-service.yaml")
            .to_str()
            .unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
    ]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let docs = json_documents(&stdout(&output));
    assert_eq!(
        docs.len(),
        2,
        "expected a PlanResponse then a RunRecord: {docs:?}"
    );
    assert!(docs[0]["plan_id"].is_string(), "{docs:?}");
    assert!(docs[0]["plan"]["nodes"].is_array(), "{docs:?}");
    assert_eq!(docs[1]["state"], "succeeded", "{docs:?}");
    assert_eq!(docs[1]["plan_id"], docs[0]["plan_id"], "{docs:?}");
    // No secret byte anywhere in the JSON text, only the marker.
    let raw = serde_json::to_string(&docs).unwrap();
    assert!(raw.contains("REDACTED"), "raw: {raw}");
}

/// `--fake-state-out` dumps the (redacted) ending fake state; reloading it
/// as a second, independent invocation's `--fake-state` sees every
/// non-secret resource already present -- `Unchanged`/`Converged`, not
/// `Created` -- since fake state otherwise never persists across two CLI
/// processes.
#[test]
fn a_second_apply_from_a_fake_state_out_dump_converges() {
    let dir = TempDir::new("fake-state-roundtrip");
    let dump = dir.join("state.json");

    let first = run(&[
        "apply",
        workflow("workflows/new-rust-service.yaml")
            .to_str()
            .unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
        "--fake-state-out",
        dump.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&first), 0, "stderr: {}", stderr(&first));
    assert!(dump.is_file(), "expected {dump:?} to be written");

    let second = run(&[
        "apply",
        workflow("workflows/new-rust-service.yaml")
            .to_str()
            .unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
        "--fake-state",
        dump.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&second), 0, "stderr: {}", stderr(&second));
    let text = stdout(&second);
    assert!(text.contains("state: succeeded"), "text: {text}");
    assert!(text.contains("Unchanged"), "text: {text}");
    assert!(text.contains("Converged"), "text: {text}");
    assert!(
        !text.contains(": Created"),
        "text should have nothing left to create: {text}"
    );
}

// ---------------------------------------------------------------------
// the approval gate
// ---------------------------------------------------------------------

fn irreversible_args(path: &str) -> Vec<&str> {
    vec![
        "apply",
        path,
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
    ]
}

#[test]
fn irreversible_without_approve_refuses_with_approval_required() {
    let path = workflow("workflows/fixtures/irreversible.yaml");
    let path = path.to_str().unwrap();

    let text_output = run(&irreversible_args(path));
    assert_eq!(
        exit_code(&text_output),
        1,
        "stderr: {}",
        stderr(&text_output)
    );
    let text = stdout(&text_output);
    assert!(text.contains("requires approval"), "text: {text}");
    assert!(text.contains("needs a human decision"), "text: {text}");

    let mut json_args = vec!["--json"];
    json_args.extend(irreversible_args(path));
    let json_output = run(&json_args);
    assert_eq!(
        exit_code(&json_output),
        1,
        "stderr: {}",
        stderr(&json_output)
    );
    let docs = json_documents(&stdout(&json_output));
    assert_eq!(docs.len(), 2, "{docs:?}");
    assert_eq!(docs[1]["kind"], "ApprovalRequired", "{docs:?}");
    assert!(docs[1]["message"].is_string(), "{docs:?}");
}

#[test]
fn irreversible_with_approve_runs() {
    let path = workflow("workflows/fixtures/irreversible.yaml");
    let mut args = irreversible_args(path.to_str().unwrap());
    args.push("--approve");
    let output = run(&args);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("state: succeeded"), "text: {text}");
    assert!(text.contains("approval: pending"), "text: {text}");
}

// ---------------------------------------------------------------------
// apply <file> copies into an isolated directory, ignoring siblings
// ---------------------------------------------------------------------

/// `workflows/fixtures/` holds many documents that do not even parse.
/// `apply` must copy only the one named file into its own private
/// temporary directory (see `commands`'s module docs), never point a
/// `Butler` at the fixtures directory itself -- proven here by applying
/// `irreversible.yaml` (whose own internal name differs from its
/// filename) straight out of that directory and getting exactly the same
/// `ApprovalRequired` refusal the isolated case gets, not a startup
/// failure from one of its broken siblings.
#[test]
fn a_file_whose_siblings_are_hostile_fixtures_still_applies() {
    let path = workflow("workflows/fixtures/irreversible.yaml");
    let output = run(&irreversible_args(path.to_str().unwrap()));
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).contains("requires approval"),
        "{}",
        stdout(&output)
    );
}

// ---------------------------------------------------------------------
// the approve-by-id flow, over a real file journal, across processes
// ---------------------------------------------------------------------

fn extract_plan_id(json_stdout: &str) -> String {
    let docs = json_documents(json_stdout);
    docs[0]["plan_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no plan_id in {docs:?}"))
        .to_string()
}

#[test]
fn the_approve_by_id_flow_runs_across_three_processes() {
    let dir = TempDir::new("approve-by-id");
    let journal = dir.join("journal.jsonl");
    // `apply --plan-id --workflows-dir` reloads `<dir>/<name>.yaml` and
    // compares its hash to the one `plan` recorded, so the same bytes
    // must be at that path for every one of the three processes -- put
    // the fixture there once, up front.
    std::fs::copy(
        workflow("workflows/fixtures/irreversible.yaml"),
        dir.join("new-rust-service-irreversible.yaml"),
    )
    .unwrap();

    // Process 1: apply records the plan and refuses (no --approve).
    let first = run(&[
        "--json",
        "apply",
        dir.join("new-rust-service-irreversible.yaml")
            .to_str()
            .unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
        "--journal",
        journal.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&first), 1, "stderr: {}", stderr(&first));
    let plan_id = extract_plan_id(&stdout(&first));

    // Process 2: approve.
    let approved = run(&["approve", &plan_id, "--journal", journal.to_str().unwrap()]);
    assert_eq!(exit_code(&approved), 0, "stderr: {}", stderr(&approved));

    // Process 3: apply --plan-id runs.
    let applied = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        journal.to_str().unwrap(),
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&applied), 0, "stderr: {}", stderr(&applied));
    let applied_docs = json_documents(&stdout(&applied));
    assert_eq!(applied_docs[0]["state"], "succeeded", "{applied_docs:?}");
    let run_id = applied_docs[0]["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no run_id in {applied_docs:?}"))
        .to_string();

    // `runs` over the same journal sees the applied run.
    let runs = run(&["runs", "--journal", journal.to_str().unwrap()]);
    assert_eq!(exit_code(&runs), 0, "stderr: {}", stderr(&runs));
    assert!(
        stdout(&runs).contains("state: succeeded"),
        "{}",
        stdout(&runs)
    );

    // `run <run_id>` over the same journal matches the applied run
    // exactly (same run_id, same plan_id, same final state).
    let one_run = run(&[
        "--json",
        "run",
        &run_id,
        "--journal",
        journal.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&one_run), 0, "stderr: {}", stderr(&one_run));
    let one_run_docs = json_documents(&stdout(&one_run));
    assert_eq!(one_run_docs[0]["run_id"], run_id, "{one_run_docs:?}");
    assert_eq!(one_run_docs[0]["plan_id"], plan_id, "{one_run_docs:?}");
    assert_eq!(one_run_docs[0]["state"], "succeeded", "{one_run_docs:?}");
}

#[test]
fn reject_then_apply_by_id_refuses_with_approval_required() {
    let dir = TempDir::new("reject-by-id");
    let journal = dir.join("journal.jsonl");
    std::fs::copy(
        workflow("workflows/fixtures/irreversible.yaml"),
        dir.join("new-rust-service-irreversible.yaml"),
    )
    .unwrap();

    let first = run(&[
        "--json",
        "apply",
        dir.join("new-rust-service-irreversible.yaml")
            .to_str()
            .unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
        "--journal",
        journal.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&first), 1, "stderr: {}", stderr(&first));
    let plan_id = extract_plan_id(&stdout(&first));

    let rejected = run(&[
        "reject",
        &plan_id,
        "--journal",
        journal.to_str().unwrap(),
        "--reason",
        "not today",
    ]);
    assert_eq!(exit_code(&rejected), 0, "stderr: {}", stderr(&rejected));

    let applied = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        journal.to_str().unwrap(),
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&applied), 1, "stderr: {}", stderr(&applied));
    let docs = json_documents(&stdout(&applied));
    assert_eq!(docs[0]["kind"], "ApprovalRequired", "{docs:?}");
}

// ---------------------------------------------------------------------
// `apply --plan-id --workflows-dir`: a real directory is validated whole
// ---------------------------------------------------------------------

/// A `--workflows-dir` whose one document's filename does not match its
/// own internal `name:` is refused naming both -- `Butler::start`'s own
/// `StartupError::NameMismatch`, reached because `--plan-id` mode points
/// a `Butler` at a real, named directory rather than a private, always-
/// consistent temporary one (unlike `apply <file>`; see the module docs).
#[test]
fn a_workflows_dir_with_a_mismatched_document_is_refused_naming_both() {
    let dir = TempDir::new("name-mismatch");
    std::fs::write(
        dir.join("foo.yaml"),
        "name: bar\ndescription: mismatched on purpose\nsteps: {}\n",
    )
    .unwrap();
    let journal = dir.join("journal.jsonl");

    let output = run(&[
        "--json",
        "apply",
        "--plan-id",
        "018e0000-0000-7000-8000-000000000000",
        "--journal",
        journal.to_str().unwrap(),
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    let err = stderr(&output);
    assert!(err.contains("NameMismatch"), "stderr: {err}");
    assert!(err.contains("foo"), "stderr: {err}");
    assert!(err.contains("bar"), "stderr: {err}");
}

// ---------------------------------------------------------------------
// a locked journal
// ---------------------------------------------------------------------

#[test]
fn approve_against_a_journal_a_file_journal_already_holds_refuses_plainly() {
    let dir = TempDir::new("locked-journal");
    let journal_path = dir.join("journal.jsonl");
    // Hold the lock for the duration of this test with a real FileJournal.
    let _held = willikins_journal::FileJournal::open(&journal_path).unwrap();

    let output = run(&[
        "approve",
        "018e0000-0000-7000-8000-000000000000",
        "--journal",
        journal_path.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("locked"),
        "stderr: {}",
        stderr(&output)
    );
}

// ---------------------------------------------------------------------
// no secret byte anywhere
// ---------------------------------------------------------------------

const SEEDED_SECRET: &str = "acceptance-test-8b-fake-secret-bytes-do-not-leak";

#[test]
fn a_seeded_secret_never_appears_in_apply_runs_or_run_output() {
    let dir = TempDir::new("secret-get");
    let journal = dir.join("journal.jsonl");

    for json in [false, true] {
        let mut args: Vec<String> = Vec::new();
        if json {
            args.push("--json".to_string());
        }
        args.extend(
            [
                "apply",
                workflow("workflows/fixtures/secret-get.yaml")
                    .to_str()
                    .unwrap(),
                "--input",
                "project=widgets",
                "--fake-state",
            ]
            .map(str::to_string),
        );
        args.push(
            workflow("workflows/fixtures/state/secret-seeded.json")
                .to_str()
                .unwrap()
                .to_string(),
        );
        args.push("--journal".to_string());
        args.push(journal.to_str().unwrap().to_string());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();

        let output = run(&args);
        assert!(
            !stdout(&output).contains(SEEDED_SECRET),
            "stdout leaked: {}",
            stdout(&output)
        );
        assert!(
            !stderr(&output).contains(SEEDED_SECRET),
            "stderr leaked: {}",
            stderr(&output)
        );
    }

    let runs = run(&["runs", "--journal", journal.to_str().unwrap()]);
    assert!(
        !stdout(&runs).contains(SEEDED_SECRET),
        "runs leaked: {}",
        stdout(&runs)
    );
    let runs_json = run(&["--json", "runs", "--journal", journal.to_str().unwrap()]);
    assert!(
        !stdout(&runs_json).contains(SEEDED_SECRET),
        "runs --json leaked: {}",
        stdout(&runs_json)
    );
}

// ---------------------------------------------------------------------
// --principal, and an unrecognised run id
// ---------------------------------------------------------------------

/// `--approve` self-approves as `--principal`, not always `local`: a
/// non-default approver name must reach the journal's own
/// `ApprovalGranted`, which the approve-by-id flow's own `approve`
/// command (not `apply --approve`, which self-approves in the same
/// breath) is what actually decides. Exercises `--principal` on
/// `approve` for the first time in this file's own tests.
#[test]
fn a_non_default_principal_approves_and_is_recorded() {
    let dir = TempDir::new("custom-principal");
    let journal = dir.join("journal.jsonl");
    std::fs::copy(
        workflow("workflows/fixtures/irreversible.yaml"),
        dir.join("new-rust-service-irreversible.yaml"),
    )
    .unwrap();

    let first = run(&[
        "--json",
        "apply",
        dir.join("new-rust-service-irreversible.yaml")
            .to_str()
            .unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
        "--journal",
        journal.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&first), 1, "stderr: {}", stderr(&first));
    let plan_id = extract_plan_id(&stdout(&first));

    let approved = run(&[
        "approve",
        &plan_id,
        "--journal",
        journal.to_str().unwrap(),
        "--principal",
        "release-manager",
    ]);
    assert_eq!(exit_code(&approved), 0, "stderr: {}", stderr(&approved));

    let applied = run(&[
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        journal.to_str().unwrap(),
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&applied), 0, "stderr: {}", stderr(&applied));
    assert!(
        stdout(&applied).contains("state: succeeded"),
        "{}",
        stdout(&applied)
    );
}

#[test]
fn run_with_an_unrecognised_id_is_a_kind_tagged_domain_refusal() {
    let dir = TempDir::new("unknown-run");
    let journal = dir.join("journal.jsonl");
    // A journal must exist (and hold at least one entry) for `replay` to
    // open it at all; an empty file is fine.
    std::fs::write(&journal, "").unwrap();

    let output = run(&[
        "--json",
        "run",
        "018e0000-0000-7000-8000-000000000000",
        "--journal",
        journal.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    let docs = json_documents(&stdout(&output));
    assert_eq!(docs[0]["kind"], "UnknownRun", "{docs:?}");
    assert!(docs[0]["message"].is_string(), "{docs:?}");
}
