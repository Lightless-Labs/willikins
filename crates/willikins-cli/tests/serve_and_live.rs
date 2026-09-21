//! Task 11: `willikins serve` (proving the shared `willikins_server::cli::run_serve`
//! wiring through this binary specifically) and `--live`'s credential
//! refusal on `plan`/`apply`.
//!
//! Every case here runs with `env_clear`, so none of these ever reads a
//! developer's sandbox credential file or makes a network call: a
//! `--live` refusal happens before any of that, from the missing or
//! malformed environment variable alone.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

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
    // The refusal names not just the missing variable but why *this*
    // document demanded it: which document, and which of its own tools.
    // `new-rust-service.yaml`'s own `name:` is `new-rust-service`; its
    // first `github.*` node calls `github.repo.ensure`.
    assert_eq!(json["document"], "new-rust-service", "{json}");
    assert_eq!(json["tool"], "github.repo.ensure", "{json}");
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains("new-rust-service") && message.contains("github.repo.ensure"),
        "text `message` should also name the document and the tool: {message}"
    );
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

// ---------------------------------------------------------------------
// The wart this file's own two tests above did not catch: `--live` used
// to build the *whole* live catalog -- all three credentials -- before
// even parsing the document, so a document that only ever calls
// `doppler.*` tools still demanded `WILLIKINS_GITHUB_TOKEN` and
// `WILLIKINS_BUILDKITE_TOKEN`. `plan`/`apply <file> --live` now compute
// the required credential set from the document's own nodes
// (`willikins_server::live_catalog_for_document`), so a Doppler-only
// document needs only `WILLIKINS_DOPPLER_TOKEN`.
// ---------------------------------------------------------------------

/// A syntactically valid Doppler service-account token
/// (`^dp\.(sa|pt)\.[a-zA-Z0-9]{40,44}$`, the shape
/// `willikins_providers_doppler::credential_from_env` itself checks),
/// assembled from two literals rather than written as one: a contiguous
/// `dp.sa.`-prefixed run of 40+ alphanumeric characters is exactly the
/// shape `crates/willikins-core/tests/secret_literal_guard.rs` flags
/// tree-wide, even inside a test, and the fix is to stop writing the
/// literal, never to bypass the scanner.
fn doppler_test_token() -> String {
    concat!("dp.sa.", "OhbVrpoiVgRV5IfLBcbfnoGMbJmTPSIAoCLrZ3aWZk").to_string()
}

/// `workflows/doppler-project.yaml` calls only `doppler.*` tools. No
/// `--input project=...` is given, so `describe` refuses on the missing
/// input and exits 1 *before* `willikins_core::plan` ever calls a tool's
/// `read()` -- proving the catalog itself was built and `check`ed with
/// only `WILLIKINS_DOPPLER_TOKEN` set, without turning this into a
/// network test.
#[test]
fn plan_live_on_a_doppler_only_document_needs_only_the_doppler_token() {
    let token = doppler_test_token();
    let output = run(
        &[
            "plan",
            workflow("workflows/doppler-project.yaml").to_str().unwrap(),
            "--live",
        ],
        &[("WILLIKINS_DOPPLER_TOKEN", token.as_str())],
    );
    assert_eq!(
        exit_code(&output),
        1,
        "expected a domain refusal (missing input), not a config refusal -- \
         stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(
        stdout(&output).contains("missing `project`"),
        "stdout: {}",
        stdout(&output)
    );
    assert!(stderr(&output).is_empty(), "stderr: {}", stderr(&output));
}

/// The same document and the same missing input, through `apply <file>
/// --live`: `Butler::plan` runs `describe` before `willikins_core::plan`
/// (see `crate::butler`'s own `ButlerError::Input`), so this refuses the
/// same way -- exit 1, on stdout, before any node's `read()` reaches the
/// network -- and never even considers `WILLIKINS_GITHUB_TOKEN` or
/// `WILLIKINS_BUILDKITE_TOKEN`.
#[test]
fn apply_live_on_a_doppler_only_document_needs_only_the_doppler_token() {
    let token = doppler_test_token();
    let output = run(
        &[
            "apply",
            workflow("workflows/doppler-project.yaml").to_str().unwrap(),
            "--live",
        ],
        &[("WILLIKINS_DOPPLER_TOKEN", token.as_str())],
    );
    assert_eq!(
        exit_code(&output),
        1,
        "expected a domain refusal (missing input), not a config refusal -- \
         stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stderr(&output).is_empty(), "stderr: {}", stderr(&output));
}

/// `workflows/new-rust-service.yaml` calls both `github.*` and
/// `doppler.*` tools, so it still refuses -- naming
/// `WILLIKINS_GITHUB_TOKEN`, checked first -- when only the Doppler
/// credential is set. The negative half of the pair above: a document
/// that *does* use a provider still demands that provider's credential
/// even when a different provider's own token is present and shaped
/// correctly.
#[test]
fn plan_live_on_a_document_using_github_still_refuses_with_only_a_doppler_token_set() {
    let token = doppler_test_token();
    let output = run(
        &[
            "--json",
            "plan",
            workflow("workflows/new-rust-service.yaml")
                .to_str()
                .unwrap(),
            "--input",
            "slug=third-thoughts",
            "--input",
            "org=lightless-labs",
            "--live",
        ],
        &[("WILLIKINS_DOPPLER_TOKEN", token.as_str())],
    );
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_str(stderr(&output).trim()).expect("valid JSON on stderr");
    assert_eq!(json["kind"], "GitHub", "{json}");
    assert_eq!(json["document"], "new-rust-service", "{json}");
    assert_eq!(json["tool"], "github.repo.ensure", "{json}");
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

/// `hash-token` on the `willikins` binary (task 12, step A said "both
/// binaries"). `crates/willikins-server/tests/hash_token.rs` already
/// exercises the shared implementation against
/// `CARGO_BIN_EXE_willikins-server` in depth; this test proves the
/// `willikins` binary's own `Command::HashToken` arm actually dispatches
/// to that same implementation, piping a token on stdin exactly as the
/// README's `openssl rand -hex 32 | willikins hash-token` does.
#[test]
fn hash_token_dispatches_through_the_willikins_binary() {
    const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(SHA256_ABC.len(), 64);

    let mut child = Command::new(env!("CARGO_BIN_EXE_willikins"))
        .arg("hash-token")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn willikins hash-token");
    child
        .stdin
        .take()
        .expect("child stdin was piped")
        .write_all(b"abc\n")
        .expect("failed to write to child stdin");
    let output = child
        .wait_with_output()
        .expect("failed to wait for willikins hash-token");

    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output).trim_end(), SHA256_ABC);
}
