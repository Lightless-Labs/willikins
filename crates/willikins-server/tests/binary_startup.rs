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
