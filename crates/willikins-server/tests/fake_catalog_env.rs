//! `WILLIKINS_FAKE_CATALOG`: the environment equivalent of `--fake` for
//! `serve`, so the deployed image's command stays `serve --http` and the
//! environment decides. Task 12, step A.
//!
//! The refusal case is process-level and fast (`.output()`, no server
//! ever starts listening). The "it actually behaves like `--fake`"
//! cases need a running server and live in
//! `tests/serve_http_deploy_pins.rs`, alongside the other two pins this
//! task adds (the journal file's owner, and the default bind address).

use std::process::{Command, Output};

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

fn dummy_paths() -> Vec<(&'static str, &'static str)> {
    vec![
        ("WILLIKINS_WORKFLOWS_DIR", "/nonexistent/workflows"),
        ("WILLIKINS_JOURNAL_PATH", "/nonexistent/journal.jsonl"),
    ]
}

/// Any value other than `1` (and not unset) is a refusal naming the
/// variable -- the same "distinct message, never echo the value" rule
/// every other `WILLIKINS_*` refusal in this crate follows.
#[test]
fn a_fake_catalog_value_other_than_1_refuses_naming_the_variable() {
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_FAKE_CATALOG", "true"));
    let output = willikins_server(&["serve", "--stdio"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_FAKE_CATALOG"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_fake_catalog_value_refuses() {
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_FAKE_CATALOG", ""));
    let output = willikins_server(&["serve", "--stdio"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_FAKE_CATALOG"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn a_fake_catalog_value_of_0_refuses() {
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_FAKE_CATALOG", "0"));
    let output = willikins_server(&["serve", "--stdio"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_FAKE_CATALOG"),
        "got: {}",
        stderr(&output)
    );
}

/// The refusal is checked before the environment's required variables --
/// mirroring `serve_stdio_with_an_invalid_principal_refuses_before_touching_the_environment`
/// in `tests/binary_startup.rs`, this pins that a malformed
/// `WILLIKINS_FAKE_CATALOG` is caught even when the workflow directory
/// and journal path are entirely unset.
#[test]
fn a_malformed_fake_catalog_value_refuses_even_with_no_other_variable_set() {
    let output = willikins_server(&["serve", "--stdio"], &[("WILLIKINS_FAKE_CATALOG", "yes")]);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_FAKE_CATALOG"),
        "got: {}",
        stderr(&output)
    );
}

/// Unset behaves exactly as before this task: the missing
/// `WILLIKINS_WORKFLOWS_DIR` refusal still fires, proving the new check
/// does not swallow or reorder the existing one.
#[test]
fn an_unset_fake_catalog_variable_does_not_change_the_existing_refusal() {
    let output = willikins_server(&["serve", "--stdio"], &[]);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_WORKFLOWS_DIR"),
        "got: {}",
        stderr(&output)
    );
}

/// A value that would be `1` but for one stray character refuses like
/// any other. A trailing space is what a dashboard variable field
/// collects from a paste; `resolve_fake_catalog` never trims, so the
/// refusal is what an operator sees instead of a silently live server.
#[test]
fn a_fake_catalog_value_of_1_with_a_trailing_space_refuses() {
    let mut vars = dummy_paths();
    vars.push(("WILLIKINS_FAKE_CATALOG", "1 "));
    let output = willikins_server(&["serve", "--stdio"], &vars);
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_FAKE_CATALOG"),
        "got: {}",
        stderr(&output)
    );
}

/// The same for a leading space, `01`, and `true`-like spellings: only
/// the one-character value `1` is accepted.
#[test]
fn every_near_miss_of_1_refuses() {
    for value in [" 1", "01", "1\n", "1,1", "yes", "on", "TRUE", "-1"] {
        let mut vars = dummy_paths();
        vars.push(("WILLIKINS_FAKE_CATALOG", value));
        let output = willikins_server(&["serve", "--stdio"], &vars);
        assert_eq!(
            exit_code(&output),
            2,
            "{value:?} was not refused; stderr: {}",
            stderr(&output)
        );
        assert!(
            stderr(&output).contains("WILLIKINS_FAKE_CATALOG"),
            "{value:?}: {}",
            stderr(&output)
        );
    }
}

/// `--fake` and `WILLIKINS_FAKE_CATALOG=1` together are not a conflict:
/// the two mean the same thing, so the server starts. (That it announces
/// the fake catalog exactly once, not twice, is
/// `tests/serve_http_deploy_pins.rs`'s own pin -- it needs a running
/// server.) Here: no refusal, so the process does not exit 2 before it
/// reaches its own missing-variable refusal for the workflow directory.
#[test]
fn the_fake_flag_and_the_fake_variable_together_are_not_a_conflict() {
    let output = willikins_server(
        &["serve", "--stdio", "--fake"],
        &[("WILLIKINS_FAKE_CATALOG", "1")],
    );
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("WILLIKINS_WORKFLOWS_DIR"),
        "the refusal should be the missing workflow directory, not the variable: {}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).contains("WILLIKINS_FAKE_CATALOG"),
        "got: {}",
        stderr(&output)
    );
}
