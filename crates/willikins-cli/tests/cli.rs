//! Integration tests for the `willikins` binary: drives the built binary
//! through [`std::process::Command`], exactly the way an agent or a human
//! at a terminal would. See `docs/plans/2026-09-11-milestone-1-core.md`'s
//! "Acceptance tests" section for what each of these pins.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The workspace root: two levels up from this crate's manifest directory
/// (`crates/willikins-cli` -> `crates` -> the workspace root), so fixtures
/// under `workflows/` resolve regardless of the test binary's own working
/// directory.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn workflow(rel: &str) -> PathBuf {
    workspace_root().join(rel)
}

fn cli_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
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

// ---------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------

#[test]
fn validate_the_positive_fixture_exits_0() {
    let path = workflow("workflows/new-rust-service.yaml");
    let output = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
}

/// Acceptance test 1: the taint fixture fails `check` with exactly one
/// `SecretToNonSecretSink`, naming `token.token` and `readme.value`, in
/// both text and JSON.
#[test]
fn validate_the_taint_fixture_exits_1_and_names_the_dotted_sites() {
    let path = workflow("workflows/fixtures/secret-into-template.yaml");

    let text_output = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(exit_code(&text_output), 1);
    let text = stdout(&text_output);
    assert!(text.contains("SecretToNonSecretSink"), "text: {text}");
    assert!(text.contains("token.token"), "text: {text}");
    assert!(text.contains("readme.value"), "text: {text}");

    let json_output = run(&["--json", "validate", path.to_str().unwrap()]);
    assert_eq!(exit_code(&json_output), 1);
    let json_text = stdout(&json_output);
    assert!(
        json_text.contains("SecretToNonSecretSink"),
        "json: {json_text}"
    );
    assert!(json_text.contains("token.token"), "json: {json_text}");
    assert!(json_text.contains("readme.value"), "json: {json_text}");
    let json: serde_json::Value = serde_json::from_str(&json_text).expect("valid JSON array");
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------------
// describe
// ---------------------------------------------------------------------

/// Acceptance test 5: with no inputs, `describe` lists `slug` and `org` as
/// missing (each with a type, prompt, and example) and neither
/// `visibility` nor `environments`, since both have defaults.
#[test]
fn describe_with_no_inputs_lists_slug_and_org_and_exits_1() {
    let path = workflow("workflows/new-rust-service.yaml");
    let output = run(&["describe", path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1);
    let text = stdout(&output);
    assert!(text.contains("slug"), "text: {text}");
    assert!(text.contains("org"), "text: {text}");
    assert!(!text.contains("missing `visibility`"), "text: {text}");
    assert!(!text.contains("missing `environments`"), "text: {text}");
}

#[test]
fn describe_with_both_inputs_resolves_everything_and_exits_0() {
    let path = workflow("workflows/new-rust-service.yaml");
    let output = run(&[
        "describe",
        path.to_str().unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
    ]);
    assert_eq!(exit_code(&output), 0, "stdout: {}", stdout(&output));
    let text = stdout(&output);
    assert!(text.contains("third-thoughts"), "text: {text}");
    assert!(text.contains("lightless-labs"), "text: {text}");
}

// ---------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------

/// Acceptance test 6: planning the positive fixture against empty state.
#[test]
fn plan_against_empty_state_exits_0_with_eight_planned_entries() {
    let path = workflow("workflows/new-rust-service.yaml");
    let output = run(&[
        "--json",
        "plan",
        path.to_str().unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
    ]);
    assert_eq!(exit_code(&output), 0, "stdout: {}", stdout(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    let nodes = json["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 8, "nodes: {nodes:#?}");

    let find = |name: &str, instance: Option<&str>| {
        nodes
            .iter()
            .find(|n| {
                n["name"] == name
                    && match instance {
                        Some(key) => n["instance"] == key,
                        None => n["instance"].is_null(),
                    }
            })
            .unwrap_or_else(|| panic!("no planned node `{name}` (instance {instance:?})"))
    };

    assert_eq!(find("names", None)["action"], "compute");
    assert_eq!(find("repo", None)["action"], "create");
    assert_eq!(find("doppler", None)["action"], "create");
    assert_eq!(find("token", None)["action"], "create");
    assert_eq!(find("ci_secret", None)["action"], "create");
    for key in ["dev", "stg", "prd"] {
        assert_eq!(find("configs", Some(key))["action"], "create");
    }

    assert_eq!(json["class"], "reversible");
    assert_eq!(json["requires_approval"], false);
    assert_eq!(
        json["outputs"]["repo_url"]["value"],
        "https://github.com/lightless-labs/third-thoughts"
    );
}

/// Acceptance test 7: the repo exists and is foreign, so `plan` returns
/// `NameTaken` rather than planning anything.
#[test]
fn plan_against_a_foreign_repo_exits_1_with_name_taken() {
    let path = workflow("workflows/new-rust-service.yaml");
    let fake_state = cli_fixture("foreign-repo.json");
    let output = run(&[
        "--json",
        "plan",
        path.to_str().unwrap(),
        "--input",
        "slug=third-thoughts",
        "--input",
        "org=lightless-labs",
        "--fake-state",
        fake_state.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&output), 1);
    let json_text = stdout(&output);
    assert!(json_text.contains("NameTaken"), "json: {json_text}");
    let json: serde_json::Value = serde_json::from_str(&json_text).expect("valid JSON");
    // `PlanError` is now internally tagged (`#[serde(tag = "kind")]`):
    // `{"kind": "NameTaken", "node": "repo", ...}`, not the old externally
    // tagged `{"NameTaken": {"node": "repo", ...}}`.
    assert_eq!(json["kind"], "NameTaken");
    assert_eq!(json["node"], "repo");
}

/// Acceptance test 8b: a `--fake-state` file seeds a Doppler secret; a
/// small inline fixture using `doppler.secret.get` is written to a temp
/// file so this test does not depend on any checked-in fixture, and its
/// plan must carry the value as `Known` without ever printing the seeded
/// bytes, in either text or JSON.
#[test]
fn plan_with_a_seeded_doppler_secret_never_leaks_the_seeded_bytes() {
    const SEEDED_BYTES: &str = "s3cr3t-fake-bytes-do-not-leak";
    let workflow_yaml = r"
name: get-secret
description: Read a seeded Doppler secret value for the redaction proof.
inputs:
  project: { type: DopplerProject }
steps:
  config:
    tool: doppler.config.ensure
    with:
      project: ${{ inputs.project }}
      environment: prd
  secret:
    tool: doppler.secret.get
    with:
      config: ${{ steps.config.config }}
      name: DATABASE_URL
outputs:
  value: ${{ steps.secret.value }}
";
    let dir = std::env::temp_dir().join(format!(
        "willikins-cli-test-{}-{}",
        std::process::id(),
        "get-secret"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let workflow_path = dir.join("get-secret.yaml");
    std::fs::write(&workflow_path, workflow_yaml).unwrap();

    let fake_state = cli_fixture("seeded-doppler-secret.json");
    let seeded_value = std::fs::read_to_string(&fake_state).unwrap();
    assert!(
        seeded_value.contains(SEEDED_BYTES),
        "fixture must actually seed the bytes this test checks for"
    );

    let text_output = run(&[
        "plan",
        workflow_path.to_str().unwrap(),
        "--input",
        "project=widgets",
        "--fake-state",
        fake_state.to_str().unwrap(),
    ]);
    assert_eq!(
        exit_code(&text_output),
        0,
        "stderr: {}",
        stderr(&text_output)
    );
    let text = stdout(&text_output);
    assert!(!text.contains(SEEDED_BYTES), "text leaked: {text}");
    assert!(
        text.contains("[REDACTED DopplerSecretValue]"),
        "text: {text}"
    );

    let json_output = run(&[
        "--json",
        "plan",
        workflow_path.to_str().unwrap(),
        "--input",
        "project=widgets",
        "--fake-state",
        fake_state.to_str().unwrap(),
    ]);
    assert_eq!(
        exit_code(&json_output),
        0,
        "stderr: {}",
        stderr(&json_output)
    );
    let json_text = stdout(&json_output);
    assert!(
        !json_text.contains(SEEDED_BYTES),
        "json leaked: {json_text}"
    );
    assert!(
        json_text.contains("[REDACTED DopplerSecretValue]"),
        "json: {json_text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------
// schema
// ---------------------------------------------------------------------

#[test]
fn schema_document_prints_valid_json() {
    let output = run(&["schema", "--document"]);
    assert_eq!(exit_code(&output), 0);
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(json["type"], "object");
}

#[test]
fn schema_catalog_prints_valid_json() {
    let output = run(&["schema", "--catalog"]);
    assert_eq!(exit_code(&output), 0);
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert!(json["tools"].is_array());
    assert!(json["types"].is_array());
}

// ---------------------------------------------------------------------
// propose-slug
// ---------------------------------------------------------------------

#[test]
fn propose_slug_prints_the_proposed_slug() {
    let output = run(&["propose-slug", "Third Thoughts"]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "third-thoughts");
}

// ---------------------------------------------------------------------
// fatal errors: exit 2
// ---------------------------------------------------------------------

#[test]
fn a_missing_file_exits_2() {
    let missing = Path::new("/nonexistent/definitely-not-a-real-path/workflow.yaml");
    let output = run(&["validate", missing.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 2);
    assert!(!stderr(&output).is_empty());
}

#[test]
fn a_yaml_syntax_error_exits_2_with_line_and_column_on_stderr() {
    let dir = std::env::temp_dir().join(format!(
        "willikins-cli-test-{}-{}",
        std::process::id(),
        "bad-yaml"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.yaml");
    // A genuine scanner-level failure: an opening quote with no matching
    // close. `serde_yaml_ng` fails here before any document shape is even
    // considered, and reports it with a location.
    std::fs::write(&path, "name: \"unterminated\n").unwrap();

    let output = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 2);
    let err_text = stdout(&output);
    assert!(
        err_text.is_empty(),
        "expected nothing on stdout: {err_text}"
    );
    let err = stderr(&output);
    assert!(!err.is_empty());
    let first_char = err.chars().next().expect("non-empty stderr");
    assert!(
        first_char.is_ascii_digit(),
        "expected a `line:column: ...` prefix, got: {err}"
    );
    assert!(err.contains(':'), "expected a line:column separator: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}
