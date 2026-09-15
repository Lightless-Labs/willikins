//! Task 11: `willikins serve` (proving the shared `willikins_server::cli::run_serve`
//! wiring through this binary specifically) and `--live`'s credential
//! refusal on `plan`/`apply`.
//!
//! Every case here runs with `env_clear`, so none of these ever reads a
//! developer's sandbox credential file or makes a network call: a
//! `--live` refusal happens before any of that, from the missing or
//! malformed environment variable alone.

use std::path::PathBuf;
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn workflow(rel: &str) -> PathBuf {
    workspace_root().join(rel)
}

fn run(args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_willikins"));
    command.args(args).env_clear();
    for (name, value) in vars {
        command.env(name, value);
    }
    command
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

/// One process-level refusal proving `willikins serve` really does share
/// `willikins_server::cli::run_serve` with the `willikins-server` binary
/// (see that function's own module docs): the same flag-combination
/// message, with no binary name hardcoded into it, from a different
/// binary entirely.
#[test]
fn serve_with_both_stdio_and_http_refuses() {
    let output = run(&["serve", "--stdio", "--http"], &[]);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("--stdio"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("not both"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn serve_with_neither_stdio_nor_http_refuses() {
    let output = run(&["serve"], &[]);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("--stdio"),
        "stderr: {}",
        stderr(&output)
    );
}

// ---------------------------------------------------------------------
// --live without credentials
// ---------------------------------------------------------------------

fn positive_fixture_args() -> [String; 5] {
    [
        "plan".to_string(),
        workflow("workflows/new-rust-service.yaml")
            .to_str()
            .unwrap()
            .to_string(),
        "--input".to_string(),
        "slug=third-thoughts".to_string(),
        "--live".to_string(),
    ]
}

#[test]
fn plan_live_without_credentials_refuses_with_a_kind_tagged_error_and_no_network_call() {
    let args = positive_fixture_args();
    let mut full_args: Vec<&str> = args.iter().map(String::as_str).collect();
    full_args.push("--input");
    full_args.push("org=lightless-labs");

    let text_output = run(&full_args, &[]);
    assert_eq!(
        exit_code(&text_output),
        2,
        "stderr: {}",
        stderr(&text_output)
    );
    assert!(
        stderr(&text_output).contains("WILLIKINS_GITHUB_TOKEN"),
        "stderr: {}",
        stderr(&text_output)
    );

    let mut json_args = vec!["--json"];
    json_args.extend(full_args.iter().copied());
    let json_output = run(&json_args, &[]);
    assert_eq!(
        exit_code(&json_output),
        2,
        "stderr: {}",
        stderr(&json_output)
    );
    let json: serde_json::Value =
        serde_json::from_str(stderr(&json_output).trim()).expect("valid JSON on stderr");
    assert_eq!(json["kind"], "GitHub", "{json}");
    assert!(json["message"].is_string(), "{json}");
    assert!(
        stdout(&json_output).is_empty(),
        "stdout: {}",
        stdout(&json_output)
    );
}

#[test]
fn apply_live_without_credentials_refuses_the_same_way() {
    let output = run(
        &[
            "apply",
            workflow("workflows/new-rust-service.yaml")
                .to_str()
                .unwrap(),
            "--input",
            "slug=third-thoughts",
            "--input",
            "org=lightless-labs",
            "--live",
        ],
        &[],
    );
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("WILLIKINS_GITHUB_TOKEN"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(stdout(&output).is_empty(), "stdout: {}", stdout(&output));
}

#[test]
fn live_and_fake_state_are_mutually_exclusive() {
    let output = run(
        &[
            "plan",
            workflow("workflows/new-rust-service.yaml")
                .to_str()
                .unwrap(),
            "--input",
            "slug=third-thoughts",
            "--input",
            "org=lightless-labs",
            "--live",
            "--fake-state",
            "/nonexistent.json",
        ],
        &[],
    );
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("mutually exclusive"),
        "stderr: {}",
        stderr(&output)
    );
}
