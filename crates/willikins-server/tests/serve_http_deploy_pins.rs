//! Task 12, step A: three pins on the real `willikins-server` binary,
//! spawned as a live child process (never `.output()`, which would
//! block forever on an HTTP server that only stops on a signal):
//!
//! 1. `serve --http` with no `--bind` binds `0.0.0.0:$PORT`.
//! 2. `serve --http` creates the journal file, in a fresh empty
//!    directory, owned by the process's own user (no privilege drop or
//!    escalation).
//! 3. `WILLIKINS_FAKE_CATALOG=1` behaves exactly like `--fake`: the
//!    fake catalog is announced in `initialize`'s `instructions`, with
//!    no `--fake` flag on the command line at all.
//!
//! The child's stderr is redirected to a file (never a pipe this test
//! does not drain -- `tracing`'s JSON lines could otherwise fill the
//! pipe buffer and stall the server); a drop guard kills the child so a
//! failed assertion never leaks a listening process; `try_wait` inside
//! the poll loop surfaces a startup refusal immediately, with its
//! stderr, instead of spinning until the poll bound.

use std::io::Read as _;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use willikins_server::TokenHash;

/// Kills the wrapped child on drop, so a panicking assertion in a test
/// below never leaves a `willikins-server serve --http` listening on a
/// port for the rest of the test run.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// An unbound ephemeral port from the OS, released immediately so
/// `willikins-server` can bind it. Small race (another process could
/// grab it first) accepted, as every "free port" test convention does.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("failed to reserve an ephemeral port")
        .local_addr()
        .expect("bound listener has a local address")
        .port()
}

/// Spawn `willikins-server` with `args`/`vars`, stderr redirected to
/// `stderr_path` (inside `dir`, so it is cleaned up with everything
/// else). `env_clear` first, exactly as every other process-level test
/// in this crate.
fn spawn(dir: &std::path::Path, args: &[&str], vars: &[(&str, &str)]) -> KillOnDrop {
    let stderr_file =
        std::fs::File::create(dir.join("stderr.log")).expect("failed to create stderr.log");
    let mut command = Command::new(env!("CARGO_BIN_EXE_willikins-server"));
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_file));
    for (name, value) in vars {
        command.env(name, value);
    }
    let child = command.spawn().expect("failed to spawn willikins-server");
    KillOnDrop(child)
}

fn read_stderr(dir: &std::path::Path) -> String {
    let mut text = String::new();
    if let Ok(mut file) = std::fs::File::open(dir.join("stderr.log")) {
        let _ = file.read_to_string(&mut text);
    }
    text
}

/// Poll `url` until it answers or `guard`'s child exits on its own
/// (a startup refusal) or the bound is reached. Panics with the child's
/// stderr on either failure, so a broken case fails fast and legibly
/// rather than timing out silently.
fn wait_until_healthy(guard: &mut KillOnDrop, dir: &std::path::Path, url: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Some(status)) = guard.0.try_wait() {
            panic!(
                "willikins-server exited early ({status}) before becoming healthy; stderr:\n{}",
                read_stderr(dir)
            );
        }
        if let Ok(response) = ureq::get(url).call()
            && response.status().as_u16() == 200
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "willikins-server never became healthy; stderr:\n{}",
            read_stderr(dir)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

const AGENT_TOKEN: &str = "deploy-pin-agent-token";
const APPROVER_TOKEN: &str = "deploy-pin-approver-token";
const ALLOWED_HOST: &str = "willikins.example";

/// The six variables every case below needs beyond `PORT`: the two
/// paths, two token hashes, one allowed host, and `WILLIKINS_FAKE_
/// CATALOG=1` so none of these ever needs a live GitHub/Doppler
/// credential just to answer `/healthz` -- shared so each test only
/// states what it's actually pinning. Pin 3 below additionally proves
/// this variable's own effect (the `initialize` announcement); pins 1
/// and 2 use it only to avoid needing real credentials at all.
fn base_vars(
    workflows_dir: &std::path::Path,
    journal_path: &std::path::Path,
) -> Vec<(String, String)> {
    vec![
        (
            "WILLIKINS_WORKFLOWS_DIR".to_string(),
            workflows_dir.display().to_string(),
        ),
        (
            "WILLIKINS_JOURNAL_PATH".to_string(),
            journal_path.display().to_string(),
        ),
        (
            "WILLIKINS_AGENT_TOKEN_HASHES".to_string(),
            hex(TokenHash::of(AGENT_TOKEN).as_bytes()),
        ),
        (
            "WILLIKINS_APPROVER_TOKEN_HASH".to_string(),
            hex(TokenHash::of(APPROVER_TOKEN).as_bytes()),
        ),
        (
            "WILLIKINS_ALLOWED_HOSTS".to_string(),
            ALLOWED_HOST.to_string(),
        ),
        ("WILLIKINS_FAKE_CATALOG".to_string(), "1".to_string()),
    ]
}

fn vars_as_str(vars: &[(String, String)]) -> Vec<(&str, &str)> {
    vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

/// Pin 1: `serve --http` with no `--bind` binds `0.0.0.0:$PORT` --
/// connecting a client to `127.0.0.1:$PORT` cannot by itself distinguish
/// a `0.0.0.0` bind from a `127.0.0.1` one (loopback reaches either), so
/// this reads the address `serve_http` itself logs.
#[test]
fn serve_http_with_no_bind_binds_0_0_0_0_port() {
    let temp = tempfile::tempdir().unwrap();
    let workflows_dir = temp.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    let journal_path = temp.path().join("journal.jsonl");
    let port = free_port();

    let mut vars = base_vars(&workflows_dir, &journal_path);
    vars.push(("PORT".to_string(), port.to_string()));

    let mut guard = spawn(temp.path(), &["serve", "--http"], &vars_as_str(&vars));
    wait_until_healthy(
        &mut guard,
        temp.path(),
        &format!("http://127.0.0.1:{port}/healthz"),
    );

    let stderr = read_stderr(temp.path());
    assert!(
        stderr.contains(&format!("0.0.0.0:{port}")),
        "expected the logged bind address to be 0.0.0.0:{port}, stderr:\n{stderr}"
    );
}

/// Pin 2: the journal file is created inside a fresh, empty directory,
/// owned by whichever user actually ran the process -- no chown, no
/// privilege drop that would brick a deployment nobody can shell into.
#[test]
#[cfg(unix)]
fn serve_http_creates_the_journal_file_as_the_running_user() {
    use std::os::unix::fs::MetadataExt as _;

    let temp = tempfile::tempdir().unwrap();
    let workflows_dir = temp.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    let journal_dir = temp.path().join("fresh-empty-journal-dir");
    std::fs::create_dir_all(&journal_dir).unwrap();
    assert_eq!(
        std::fs::read_dir(&journal_dir).unwrap().count(),
        0,
        "journal_dir must start empty"
    );
    let journal_path = journal_dir.join("journal.jsonl");
    let port = free_port();

    let mut vars = base_vars(&workflows_dir, &journal_path);
    vars.push(("PORT".to_string(), port.to_string()));

    let mut guard = spawn(
        temp.path(),
        &["serve", "--http", "--bind", &format!("127.0.0.1:{port}")],
        &vars_as_str(&vars),
    );
    wait_until_healthy(
        &mut guard,
        temp.path(),
        &format!("http://127.0.0.1:{port}/healthz"),
    );

    assert!(journal_path.is_file(), "journal file was not created");
    let journal_owner = std::fs::metadata(&journal_path).unwrap().uid();
    let dir_owner = std::fs::metadata(&journal_dir).unwrap().uid();
    assert_eq!(
        journal_owner, dir_owner,
        "the journal file must be owned by the same user that owns the directory \
         it was created in -- this test process's own user, since it created that \
         directory itself"
    );
}

/// Pin 3: `WILLIKINS_FAKE_CATALOG=1`, with no `--fake` flag at all,
/// announces the fake catalog in `initialize`'s `instructions` exactly
/// as `--fake` does (`mcp.rs`'s own sentence).
#[test]
fn fake_catalog_env_var_announces_itself_over_http_with_no_fake_flag() {
    let temp = tempfile::tempdir().unwrap();
    let workflows_dir = temp.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    let journal_path = temp.path().join("journal.jsonl");
    let port = free_port();

    // `base_vars` already includes `WILLIKINS_FAKE_CATALOG=1` (see its
    // own doc) -- restated here only as documentation of what this
    // specific test relies on; no `--fake` flag appears anywhere in the
    // command below.
    let vars = base_vars(&workflows_dir, &journal_path);

    let mut guard = spawn(
        temp.path(),
        &["serve", "--http", "--bind", &format!("127.0.0.1:{port}")],
        &vars_as_str(&vars),
    );
    wait_until_healthy(
        &mut guard,
        temp.path(),
        &format!("http://127.0.0.1:{port}/healthz"),
    );

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "deploy-pin-test", "version": "0.1.0" },
        },
    });
    let mut response = ureq::post(format!("http://127.0.0.1:{port}/mcp"))
        .header("host", ALLOWED_HOST)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {AGENT_TOKEN}"))
        .send_json(&body)
        .expect("initialize request failed");
    let status = response.status().as_u16();
    assert_eq!(status, 200, "unexpected status {status}");
    let text = response
        .body_mut()
        .read_to_string()
        .expect("initialize response body was not readable");
    let json: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{error}: {text}"));
    let instructions = json["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("no instructions string in {json}"));
    assert!(
        instructions.contains("fake in-memory catalog"),
        "instructions did not announce the fake catalog: {instructions}"
    );
}
