//! `willikins-server serve`'s startup refusals, pinned at the process
//! level (`Command::new(env!("CARGO_BIN_EXE_willikins-server"))`, the
//! same convention `willikins-cli`'s own adversarial tests use for the
//! `willikins` binary) rather than only through `main.rs`'s internal
//! `StartError`/`build_butler` types, which no test previously exercised
//! at all.
//!
//! Every case here refuses with exit code 2 and a distinct message on
//! stderr, per the plan's "distinct message" requirement -- and, since
//! `env_clear` is used throughout, none of these ever reads a
//! developer's sandbox credential file, or makes any network call: the
//! process never gets past its own argument/environment validation.

use std::process::{Command, Output};

/// Run the built `willikins-server` binary with `args` and an explicitly
/// controlled environment (`env_clear` first, so a variable already set
/// in this test process's own environment -- such as one left over from
/// a developer's shell -- can never leak in and change the outcome).
fn willikins_server(args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_willikins-server"));
    command.args(args).env_clear();
    for (name, value) in vars {
        command.env(name, value);
    }
    command
        .output()
        .expect("failed to run the willikins-server binary")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("process was not signalled")
}

#[test]
fn serve_without_stdio_refuses_with_exit_code_2() {
    let output = willikins_server(&["serve"], &[]);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("--stdio"),
        "stderr should name the missing --stdio flag, got: {}",
        stderr(&output)
    );
}

#[test]
fn serve_stdio_with_no_environment_refuses_naming_the_missing_variable() {
    let output = willikins_server(&["serve", "--stdio"], &[]);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_WORKFLOWS_DIR"),
        "stderr should name the first missing required variable, got: {}",
        stderr(&output)
    );
}

// ---------------------------------------------------------------------
// `serve --http`'s own startup refusals -- task 10b. `HttpConfig::build`
// itself is unit-tested directly (`crate::http::config`'s own tests);
// these pin that `main.rs`'s wiring actually surfaces each refusal
// through the real binary, at the process level, with the plan's own
// "distinct message" and exit code 2. A dummy `WILLIKINS_WORKFLOWS_DIR`/
// `WILLIKINS_JOURNAL_PATH` (neither needs to exist: `build_http_config`
// runs, and refuses, before `build_butler` ever touches the filesystem)
// and `--bind 127.0.0.1:0` keep every case here from reaching anything
// but the one rule under test.
// ---------------------------------------------------------------------

const HEX_64: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn hex_64(fill: char) -> String {
    std::iter::repeat_n(fill, 64).collect()
}

fn dummy_paths() -> Vec<(&'static str, &'static str)> {
    vec![
        ("WILLIKINS_WORKFLOWS_DIR", "/nonexistent/workflows"),
        ("WILLIKINS_JOURNAL_PATH", "/nonexistent/journal.jsonl"),
    ]
}

#[test]
fn serve_http_with_no_agent_hash_refuses() {
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_APPROVER_TOKEN_HASH", HEX_64));
    let output = willikins_server(&["serve", "--http", "--bind", "127.0.0.1:0"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("agent token hash"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn serve_http_with_the_approver_hash_among_the_agent_hashes_refuses() {
    let shared = hex_64('a');
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_AGENT_TOKEN_HASHES", &shared));
    vars.push(("WILLIKINS_APPROVER_TOKEN_HASH", &shared));
    let output = willikins_server(&["serve", "--http", "--bind", "127.0.0.1:0"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("approver token hash"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn serve_http_with_empty_allowed_hosts_refuses() {
    let approver = hex_64('a');
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_AGENT_TOKEN_HASHES", HEX_64));
    vars.push(("WILLIKINS_APPROVER_TOKEN_HASH", &approver));
    let output = willikins_server(&["serve", "--http", "--bind", "127.0.0.1:0"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("allowed host"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn serve_http_with_no_approver_hash_refuses_naming_the_variable() {
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_AGENT_TOKEN_HASHES", HEX_64));
    vars.push(("WILLIKINS_ALLOWED_HOSTS", "example.com"));
    let output = willikins_server(&["serve", "--http", "--bind", "127.0.0.1:0"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_APPROVER_TOKEN_HASH"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn serve_http_with_no_bind_and_no_port_refuses() {
    let vars = dummy_paths();
    let output = willikins_server(&["serve", "--http"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("--bind") && stderr(&output).contains("PORT"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn serve_http_and_serve_stdio_together_refuses() {
    let output = willikins_server(&["serve", "--stdio", "--http"], &[]);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("not both"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn serve_stdio_with_an_invalid_principal_refuses_before_touching_the_environment() {
    // No env vars set at all (not even WILLIKINS_WORKFLOWS_DIR): the
    // principal is parsed before the environment is ever read, so this
    // must refuse on the principal, not on a missing variable.
    let output = willikins_server(&["serve", "--stdio", "--principal", "not valid!"], &[]);
    assert_eq!(exit_code(&output), 2);
    let text = stderr(&output);
    assert!(
        text.contains("--principal"),
        "stderr should name --principal as the refusal's source, got: {text}"
    );
    assert!(
        !text.contains("WILLIKINS_WORKFLOWS_DIR"),
        "the principal must be validated before the environment is read, got: {text}"
    );
}

/// One malformed entry in `WILLIKINS_AGENT_TOKEN_HASHES` refuses the
/// whole variable, naming it -- it is never silently dropped, leaving the
/// well-formed entries in force. A typo in an operator's hash list must
/// be a refusal to start, not a server that quietly honours fewer tokens
/// (or, worse, admits an entry that parsed into something unintended).
#[test]
fn a_malformed_entry_in_the_agent_hash_list_refuses_naming_the_variable() {
    let good = hex_64('a');
    let list = format!("{good},not-a-hash");
    let approver = hex_64('b');
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_AGENT_TOKEN_HASHES", &list));
    vars.push(("WILLIKINS_APPROVER_TOKEN_HASH", &approver));
    vars.push(("WILLIKINS_ALLOWED_HOSTS", "example.com"));
    let output = willikins_server(&["serve", "--http", "--bind", "127.0.0.1:0"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_AGENT_TOKEN_HASHES"),
        "got: {}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).contains("not-a-hash"),
        "the refusal must not echo the rejected value: {}",
        stderr(&output)
    );
}
