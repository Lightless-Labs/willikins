//! `hash-token`: reads one token from stdin, prints its SHA-256 as 64
//! lower-case hex and nothing else. Task 12, step A -- the README's
//! `hash-token` procedure so a real token never touches a command line
//! or an argument list, only stdin.
//!
//! Process-level, following `tests/binary_startup.rs`'s own convention
//! (`Command::new(env!("CARGO_BIN_EXE_willikins-server"))`), but piping
//! bytes to the child's stdin rather than only inspecting its exit code.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run_hash_token(stdin_bytes: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_willikins-server"))
        .arg("hash-token")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn willikins-server hash-token");
    child
        .stdin
        .take()
        .expect("child stdin was piped")
        .write_all(stdin_bytes)
        .expect("failed to write to child stdin");
    child
        .wait_with_output()
        .expect("failed to wait for willikins-server hash-token")
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

/// A known SHA-256 test vector (`sha256("abc")`), independent of
/// `TokenHash::of`'s own implementation, so this test would catch a bug
/// shared by both. Exactly 64 lower-case hex characters.
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn hashes_a_token_with_a_trailing_newline_to_the_known_vector() {
    assert_eq!(SHA256_ABC.len(), 64);
    let output = run_hash_token(b"abc\n");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output).trim_end(), SHA256_ABC);
}

#[test]
fn hashes_a_token_with_no_trailing_newline_the_same_way() {
    let output = run_hash_token(b"abc");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output).trim_end(), SHA256_ABC);
}

#[test]
fn prints_exactly_64_lower_case_hex_characters_and_a_trailing_newline() {
    let output = run_hash_token(b"some-real-looking-token-value\n");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert_eq!(out, format!("{}\n", out.trim()));
    let digest = out.trim();
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')),
        "not lower-case hex: {digest}"
    );
}

#[test]
fn the_printed_hash_matches_token_hash_of_the_same_token() {
    // The claim the README makes: `hash-token`'s output is exactly what
    // the server compares a presented bearer token against.
    let output = run_hash_token(b"round-trip-token\n");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let printed = stdout(&output).trim().to_string();
    let parsed = willikins_server::TokenHash::parse(&printed)
        .expect("hash-token's output must parse as a TokenHash");
    assert_eq!(parsed, willikins_server::TokenHash::of("round-trip-token"));
}

#[test]
fn refuses_an_empty_token() {
    let output = run_hash_token(b"");
    assert_eq!(exit_code(&output), 2);
    assert!(stdout(&output).is_empty(), "stdout: {}", stdout(&output));
    assert!(
        stderr(&output).contains("empty"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn refuses_a_token_that_is_only_a_newline() {
    let output = run_hash_token(b"\n");
    assert_eq!(exit_code(&output), 2);
    assert!(
        stderr(&output).contains("empty"),
        "stderr: {}",
        stderr(&output)
    );
}

/// Two lines piped in must not silently hash to "the first line" or "the
/// whole blob including the embedded newline" -- either would compute a
/// digest the operator did not intend and the server would never
/// present. Refused instead (fail closed).
#[test]
fn refuses_a_token_with_an_embedded_newline() {
    let output = run_hash_token(b"first-line\nsecond-line\n");
    assert_eq!(exit_code(&output), 2);
    assert!(stdout(&output).is_empty(), "stdout: {}", stdout(&output));
}

/// `hash-token` never takes the token as a command-line argument -- only
/// `stdin` reaches it. Extra positional or unknown arguments are clap's
/// own refusal (exit code 2), proving no argument is accepted as the
/// token.
#[test]
fn takes_no_command_line_argument() {
    let output = Command::new(env!("CARGO_BIN_EXE_willikins-server"))
        .args(["hash-token", "a-token-passed-as-an-argument"])
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .expect("failed to run willikins-server hash-token with an argument");
    assert_ne!(
        exit_code(&output),
        0,
        "a positional argument must not be accepted as the token"
    );
}

// ---------------------------------------------------------------------
// Task 12 verification: the bytes `hash-token` may and may not accept.
// ---------------------------------------------------------------------

/// A token with one `\r\n` line ending hashes exactly as the same token
/// with one `\n` does: an operator on a machine that writes CRLF must
/// not configure the server with a different hash from everyone else.
#[test]
fn a_crlf_line_ending_hashes_to_the_same_vector() {
    let output = run_hash_token(b"abc\r\n");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output).trim_end(), SHA256_ABC);
}

/// A NUL byte can never appear in an `Authorization` header value, so a
/// hash of a token that holds one is a hash no presented token can ever
/// match. That is the same failure the embedded-newline refusal exists
/// to stop: a digest the operator did not intend and the server will
/// never see. Refused, naming no byte of the input.
#[test]
fn refuses_a_token_containing_a_nul_byte() {
    let output = run_hash_token(b"token-with-a\0nul\n");
    assert_eq!(
        exit_code(&output),
        2,
        "stdout: {}, stderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stdout(&output).is_empty(), "stdout: {}", stdout(&output));
}

/// The same rule for a carriage return in the middle of a token: only
/// one trailing `\r\n` is a line ending; an embedded `\r` is a control
/// character no header value carries.
#[test]
fn refuses_a_token_containing_an_embedded_carriage_return() {
    let output = run_hash_token(b"token-with-a\rcarriage-return\n");
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    assert!(stdout(&output).is_empty(), "stdout: {}", stdout(&output));
}

/// No refusal echoes any part of the token -- the whole point of the
/// subcommand is that a real token's bytes reach the digest and nothing
/// else.
#[test]
fn a_refusal_never_echoes_the_token() {
    let output = run_hash_token(b"MARKER-8f2c-secret-looking\nMARKER-8f2c-second-line\n");
    assert_eq!(exit_code(&output), 2);
    assert!(
        !stderr(&output).contains("MARKER-8f2c"),
        "stderr echoed the token: {}",
        stderr(&output)
    );
    assert!(!stdout(&output).contains("MARKER-8f2c"));
}

/// A 1 MiB token is still one line of printable text, so it hashes
/// rather than refusing -- `hash-token` bounds what a token may contain,
/// never how long it is.
#[test]
fn hashes_a_one_mebibyte_token() {
    let token = "a".repeat(1024 * 1024);
    let mut input = token.clone().into_bytes();
    input.push(b'\n');
    let output = run_hash_token(&input);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let printed = stdout(&output).trim().to_string();
    assert_eq!(printed.len(), 64);
    let parsed = willikins_server::TokenHash::parse(&printed).expect("must parse as a TokenHash");
    assert_eq!(parsed, willikins_server::TokenHash::of(&token));
}
