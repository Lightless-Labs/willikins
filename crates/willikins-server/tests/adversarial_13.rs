//! Adversarial pass 2 (`docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`),
//! acceptance test 19's second half: the finished milestone 2 attacked
//! end to end **over TCP, against the real binary**, not through
//! `router()` in process the way `adversarial_10b.rs` does.
//!
//! Every test here starts `willikins-server serve --http --bind
//! 127.0.0.1:0` as a child process with `env_clear()` and
//! `WILLIKINS_FAKE_CATALOG=1`, learns the real port by parsing the
//! `willikins-server listening` line out of the child's JSON `tracing`
//! output on stderr, and speaks HTTP by hand on a `std::net::TcpStream`
//! -- which is the only way to send the requests an HTTP client library
//! will not: `HTTP/1.0`, a repeated `Authorization` header, a body
//! whose bytes never arrive, a `Content-Length` that lies.
//!
//! No test here reaches a real provider: the fake catalog is the only
//! catalog, so neither provider credential is ever read, and
//! `env_clear()` means a developer's own environment cannot leak one in
//! either.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The agent bearer token every test presents, and the approver
/// password. Literals, not secrets: they exist only inside one test
/// process's own child.
const AGENT_TOKEN: &str = "agent-token-for-pass-2";
const SECOND_AGENT_TOKEN: &str = "second-agent-token-for-pass-2";
const APPROVER_PASSWORD: &str = "approver-password-for-pass-2";

fn sha256_hex(input: &str) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(input.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn basic_credential(user: &str, password: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// A running `willikins-server` child, its bound address, and every byte
/// it has written to stderr so far.
///
/// stderr is drained on its own thread: a child whose stderr pipe fills
/// blocks forever on its next `tracing` line, and the tracing sweep
/// (goal 4) needs every byte anyway.
struct Server {
    child: Child,
    addr: SocketAddr,
    stderr: Arc<Mutex<String>>,
    stdout: Arc<Mutex<String>>,
    _dir: Option<tempfile::TempDir>,
    journal_path: PathBuf,
    workflows_dir: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How the child is configured, beyond the fixed defaults.
struct ServerOptions {
    /// Extra or overriding environment variables.
    vars: Vec<(String, String)>,
    /// Documents to copy into the trusted directory, as
    /// `(workflows-relative source, on-disk filename)`.
    documents: Vec<(String, String)>,
}

impl ServerOptions {
    fn new() -> Self {
        Self {
            vars: Vec::new(),
            documents: vec![(
                "new-rust-service.yaml".to_string(),
                "new-rust-service.yaml".to_string(),
            )],
        }
    }

    fn var(mut self, name: &str, value: &str) -> Self {
        self.vars.push((name.to_string(), value.to_string()));
        self
    }

    fn document(mut self, relative: &str, filename: &str) -> Self {
        self.documents
            .push((relative.to_string(), filename.to_string()));
        self
    }
}

fn copy_fixture_as(dir: &Path, relative: &str, filename: &str) {
    let root = workspace_root();
    let candidates = [
        root.join("workflows").join(relative),
        root.join("workflows").join("fixtures").join(relative),
    ];
    let source = candidates
        .iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("fixture `{relative}` not found"));
    std::fs::write(dir.join(filename), std::fs::read(source).unwrap()).unwrap();
}

/// A `UUIDv7` with the current millisecond in its timestamp field, built
/// by hand rather than pulled in as a dependency: the point of the
/// attack is that an id which is *indistinguishable in shape* from one
/// this server minted carries no authority at all.
#[allow(clippy::cast_possible_truncation)] // a millisecond counter and a loop index, both far inside their casts
fn forged_uuid_v7_now() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_millis() as u64;
    let mut bytes = [0u8; 16];
    bytes[..6].copy_from_slice(&millis.to_be_bytes()[2..]);
    // Version 7 in the high nibble of byte 6, variant 0b10 in byte 8.
    bytes[6] = 0x70 | 0x0a;
    bytes[7] = 0xbc;
    bytes[8] = 0x80 | 0x0d;
    for (index, byte) in bytes.iter_mut().enumerate().skip(9) {
        *byte = (index as u8).wrapping_mul(17);
    }
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Start a server over an existing workflows directory and journal path
/// -- the seam a restart test needs, since the directory and the journal
/// must outlive the first child.
fn start_server_at(workflows_dir: &Path, journal_path: &Path) -> Server {
    spawn_server(None, workflows_dir, journal_path, &[])
}

fn start_server(options: &ServerOptions) -> Server {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workflows_dir = dir.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    for (relative, filename) in &options.documents {
        copy_fixture_as(&workflows_dir, relative, filename);
    }
    let journal_path = dir.path().join("journal.jsonl");
    spawn_server(Some(dir), &workflows_dir, &journal_path, &options.vars)
}

fn spawn_server(
    dir: Option<tempfile::TempDir>,
    workflows_dir: &Path,
    journal_path: &Path,
    vars: &[(String, String)],
) -> Server {
    let mut command = Command::new(env!("CARGO_BIN_EXE_willikins-server"));
    command
        .args(["serve", "--http", "--bind", "127.0.0.1:0"])
        .env_clear()
        .env("WILLIKINS_WORKFLOWS_DIR", workflows_dir)
        .env("WILLIKINS_JOURNAL_PATH", journal_path)
        .env(
            "WILLIKINS_AGENT_TOKEN_HASHES",
            format!(
                "{},{}",
                sha256_hex(AGENT_TOKEN),
                sha256_hex(SECOND_AGENT_TOKEN)
            ),
        )
        .env(
            "WILLIKINS_APPROVER_TOKEN_HASH",
            sha256_hex(APPROVER_PASSWORD),
        )
        .env("WILLIKINS_ALLOWED_HOSTS", "127.0.0.1,localhost")
        // The fake catalog is the only catalog: no provider credential is
        // read, and no request can reach a real API.
        .env("WILLIKINS_FAKE_CATALOG", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in vars {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("the willikins-server binary runs");

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let stderr = drain(child.stderr.take().expect("stderr is piped"), Some(tx));
    let stdout = drain(child.stdout.take().expect("stdout is piped"), None);

    let addr = wait_for_listening_line(&rx).unwrap_or_else(|| {
        let text = stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        panic!("the server never announced a bound address; stderr was:\n{text}")
    });

    Server {
        child,
        addr,
        stderr,
        stdout,
        _dir: dir,
        journal_path: journal_path.to_path_buf(),
        workflows_dir: workflows_dir.to_path_buf(),
    }
}

/// Drain `pipe` on its own thread into a shared buffer -- a child whose
/// stderr pipe fills blocks forever on its next `tracing` line, and the
/// sweep needs every byte anyway. `tx`, when given, also forwards each
/// line so the caller can wait for the listening announcement.
fn drain<R: Read + Send + 'static>(
    pipe: R,
    tx: Option<std::sync::mpsc::Sender<String>>,
) -> Arc<Mutex<String>> {
    let buffer = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&buffer);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    sink.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push_str(&line);
                    if let Some(tx) = &tx {
                        let _ = tx.send(line.clone());
                    }
                }
            }
        }
    });
    buffer
}

/// Parse the one startup line `serve_http` emits
/// (`tracing::info!(bind = %bind, "willikins-server listening")`, JSON to
/// stderr) for the address actually bound -- never a guessed port.
fn wait_for_listening_line(rx: &std::sync::mpsc::Receiver<String>) -> Option<SocketAddr> {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(line) = rx.recv_timeout(remaining) else {
            return None;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let fields = json.get("fields")?;
        if fields.get("message").and_then(serde_json::Value::as_str)
            == Some("willikins-server listening")
            && let Some(bind) = fields.get("bind").and_then(serde_json::Value::as_str)
        {
            return bind.parse().ok();
        }
    }
    None
}

impl Server {
    fn stderr_text(&self) -> String {
        self.stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn stdout_text(&self) -> String {
        self.stdout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Send `request` verbatim on a fresh connection and read the whole
    /// response until the server closes or `read_timeout` elapses.
    fn raw(&self, request: &str) -> String {
        self.raw_bytes(request.as_bytes(), Duration::from_secs(10))
    }

    fn raw_bytes(&self, request: &[u8], read_timeout: Duration) -> String {
        let mut stream = TcpStream::connect(self.addr).expect("connects");
        stream.set_read_timeout(Some(read_timeout)).unwrap();
        stream.write_all(request).expect("writes");
        stream.flush().unwrap();
        let mut response = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    response.extend_from_slice(&buffer[..n]);
                    // A keep-alive response does not close the socket, so
                    // stop once a complete head plus its declared body has
                    // arrived rather than waiting for the read timeout.
                    if response_is_complete(&response) {
                        break;
                    }
                }
            }
        }
        String::from_utf8_lossy(&response).into_owned()
    }

    /// Wait until the journal file holds at least `count` lines, then
    /// replay it. The server appends synchronously inside the request it
    /// answers, but the response can reach this process a moment before
    /// the append's `sync_data` returns.
    fn replay_journal(&self) -> willikins_journal::ReplayedJournal {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match willikins_journal::replay(&self.journal_path) {
                Ok(replayed) => return replayed,
                Err(error) if std::time::Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("the journal must replay: {error}"),
            }
        }
    }

    fn wait_for_journal_event(
        &self,
        predicate: impl Fn(&willikins_journal::Event) -> bool,
    ) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(replayed) = willikins_journal::replay(&self.journal_path)
                && replayed
                    .entries()
                    .iter()
                    .any(|entry| predicate(&entry.event))
            {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Whether `response` holds a complete HTTP head and, if it declares a
/// `Content-Length`, that many body bytes.
fn response_is_complete(response: &[u8]) -> bool {
    let Some(head_end) = find_subsequence(response, b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&response[..head_end]).to_ascii_lowercase();
    if let Some(index) = head.find("content-length:") {
        let rest = &head[index + "content-length:".len()..];
        let value: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(length) = value.parse::<usize>() {
            return response.len() >= head_end + 4 + length;
        }
    }
    // No Content-Length: a chunked or connection-closed body; keep
    // reading until the peer closes.
    false
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn status_line(response: &str) -> String {
    response.lines().next().unwrap_or_default().to_string()
}

/// A minimal JSON-RPC `tools/call` body for `body_len` padding purposes.
fn jsonrpc_call(tool: &str, arguments: &serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments.clone() },
    })
    .to_string()
}

fn post_mcp(server: &Server, token: Option<&str>, body: &str) -> String {
    let auth = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    server.raw(&format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
}

// =====================================================================
// Goal: bypass or confuse authentication, over TCP with real headers.
// =====================================================================

#[test]
fn a_request_with_no_authorization_header_is_401_over_tcp() {
    let server = start_server(&ServerOptions::new());
    let response = post_mcp(
        &server,
        None,
        &jsonrpc_call("list_tools", &serde_json::json!({})),
    );
    assert!(
        status_line(&response).contains("401"),
        "expected 401, got: {response}"
    );
    assert!(
        response
            .to_ascii_lowercase()
            .contains("www-authenticate: bearer"),
        "the 401 carries the bearer challenge: {response}"
    );
}

#[test]
fn the_approver_password_used_as_a_bearer_token_is_403_over_tcp() {
    let server = start_server(&ServerOptions::new());
    let response = post_mcp(
        &server,
        Some(APPROVER_PASSWORD),
        &jsonrpc_call("list_tools", &serde_json::json!({})),
    );
    assert!(
        status_line(&response).contains("403"),
        "a valid credential in the wrong role is 403: {response}"
    );
    assert!(
        server.wait_for_journal_event(|event| matches!(
            event,
            willikins_journal::Event::AuthFailed {
                reason: willikins_journal::AuthFailedReason::WrongRole,
                ..
            }
        )),
        "the refusal is journaled as WrongRole"
    );
}

/// `HeaderMap::get` returns the *first* value of a repeated header, so a
/// request carrying a valid token and then a forged one authenticates as
/// the first. Pinned rather than fixed: both orders are refused unless
/// the first value is itself a configured token, so a proxy that appends
/// its own header cannot smuggle one in ahead of the client's.
#[test]
fn a_repeated_authorization_header_is_decided_by_the_first_value() {
    let server = start_server(&ServerOptions::new());

    let valid_then_forged = server.raw(&format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\nAuthorization: Bearer {AGENT_TOKEN}\r\n\
Authorization: Bearer not-a-real-token\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        jsonrpc_call("list_tools", &serde_json::json!({})).len(),
        jsonrpc_call("list_tools", &serde_json::json!({}))
    ));
    assert!(
        !status_line(&valid_then_forged).contains("401"),
        "the first (valid) value decides: {valid_then_forged}"
    );

    let forged_then_valid = server.raw(&format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\nAuthorization: Bearer not-a-real-token\r\n\
Authorization: Bearer {AGENT_TOKEN}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        jsonrpc_call("list_tools", &serde_json::json!({})).len(),
        jsonrpc_call("list_tools", &serde_json::json!({}))
    ));
    assert!(
        status_line(&forged_then_valid).contains("401"),
        "a forged first value is refused even with a valid second: {forged_then_valid}"
    );
}

/// A token that differs from a configured one in case, or by *leading*
/// whitespace, is a different token: the comparison is over the SHA-256
/// of the exact bytes after `Bearer `.
///
/// **Trailing whitespace is the exception, and it is HTTP's doing, not
/// willikins'.** A header field value has its surrounding optional
/// whitespace stripped by the HTTP parser before any handler sees it
/// (RFC 9110's field-value grammar), so `Bearer <token> ` arrives as
/// `Bearer <token>` and authenticates. Measured here rather than
/// assumed, and left alone: refusing it would mean second-guessing the
/// transport, and the only way an operator gets bitten is by hashing a
/// token that carries trailing whitespace -- which `hash-token` already
/// refuses to do (it strips one trailing newline and rejects every other
/// control character).
#[test]
fn a_bearer_token_differing_in_case_or_leading_padding_is_refused() {
    let server = start_server(&ServerOptions::new());
    for candidate in [
        AGENT_TOKEN.to_ascii_uppercase(),
        format!(" {AGENT_TOKEN}"),
        format!("{AGENT_TOKEN}x"),
        AGENT_TOKEN.replace('-', "_"),
    ] {
        let response = post_mcp(
            &server,
            Some(&candidate),
            &jsonrpc_call("list_tools", &serde_json::json!({})),
        );
        assert!(
            status_line(&response).contains("401"),
            "`{candidate:?}` must not authenticate: {}",
            status_line(&response)
        );
    }

    // The measurement: trailing whitespace is stripped by the HTTP layer,
    // so this *is* the configured token by the time anything hashes it.
    let padded = post_mcp(
        &server,
        Some(&format!("{AGENT_TOKEN} ")),
        &jsonrpc_call("list_tools", &serde_json::json!({})),
    );
    assert!(
        status_line(&padded).contains("200"),
        "trailing header whitespace is stripped before the token is seen: {}",
        status_line(&padded)
    );
}

/// `Bearer` is matched exactly: neither `bearer` nor `BEARER` nor
/// `Bearer  ` (two spaces) is the prefix `extract_bearer` strips, so each
/// reads as no credential at all. Recorded rather than fixed -- RFC 6750
/// makes the scheme case-insensitive, so a compliant client sending
/// `bearer` gets a 401 it did not deserve. Interoperability defect, not a
/// security one: the failure is closed.
#[test]
fn a_lowercase_bearer_scheme_is_refused_as_a_missing_credential() {
    let server = start_server(&ServerOptions::new());
    let body = jsonrpc_call("list_tools", &serde_json::json!({}));
    let response = server.raw(&format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\nAuthorization: bearer {AGENT_TOKEN}\r\n\
Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    assert!(
        status_line(&response).contains("401"),
        "a lower-case scheme is refused: {response}"
    );
}

#[test]
fn http_1_0_reaches_the_same_authentication_refusal() {
    let server = start_server(&ServerOptions::new());
    let body = jsonrpc_call("list_tools", &serde_json::json!({}));
    let response = server.raw_bytes(
        format!(
            "POST /mcp HTTP/1.0\r\nHost: 127.0.0.1\r\nAccept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
        Duration::from_secs(10),
    );
    assert!(
        status_line(&response).contains("401"),
        "HTTP/1.0 is authenticated the same way: {response}"
    );
}

/// `/healthz` needs no credential and is answered with and without a
/// port in the `Host` header -- the shape Railway's private network
/// sends (task 12 pinned the allowed-hosts half of this in
/// `deploy_host_headers.rs`; this is the same rule over a real socket).
#[test]
fn healthz_answers_with_and_without_a_host_port_and_needs_no_credential() {
    let server = start_server(&ServerOptions::new());
    for host in [
        "127.0.0.1".to_string(),
        format!("127.0.0.1:{}", server.addr.port()),
        "localhost".to_string(),
    ] {
        let response = server.raw(&format!(
            "GET /healthz HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        ));
        assert!(
            status_line(&response).contains("200"),
            "Host `{host}` must be answered: {response}"
        );
    }
}

/// A `Host` header naming a host the deployment never allowed is refused
/// by rmcp's own DNS-rebinding defence -- but only on `/mcp`, and only
/// after authentication. Pinned so the ordering is on record: an
/// unauthenticated request with a foreign `Host` is a 401, not a 403,
/// which means the host check never becomes an oracle for whether a
/// token is valid.
#[test]
fn a_foreign_host_header_on_mcp_is_still_refused_at_authentication_first() {
    let server = start_server(&ServerOptions::new());
    let body = jsonrpc_call("list_tools", &serde_json::json!({}));
    let response = server.raw(&format!(
        "POST /mcp HTTP/1.1\r\nHost: evil.example.com\r\nAccept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    assert!(
        status_line(&response).contains("401"),
        "authentication is the outer gate: {response}"
    );

    let authenticated = server.raw(&format!(
        "POST /mcp HTTP/1.1\r\nHost: evil.example.com\r\nAccept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\nAuthorization: Bearer {AGENT_TOKEN}\r\nContent-Length: {}\r\n\
Connection: close\r\n\r\n{body}",
        body.len()
    ));
    assert!(
        !status_line(&authenticated).contains("200"),
        "a foreign Host is refused once past authentication: {authenticated}"
    );
}

/// A chunked request body is accepted by hyper and reaches the same
/// authentication decision: chunking is not a way around the bearer
/// check, and a chunked body past the cap is still refused.
#[test]
fn a_chunked_body_is_authenticated_like_any_other() {
    let server = start_server(&ServerOptions::new());
    let body = jsonrpc_call("list_tools", &serde_json::json!({}));
    let chunked = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
        body.len()
    );
    let response = server.raw(&chunked);
    assert!(
        status_line(&response).contains("401"),
        "a chunked body reaches the bearer check: {response}"
    );
}

// =====================================================================
// Goal: exceed a limit.
// =====================================================================

/// A body of exactly the configured cap is accepted; one byte more is
/// refused with 413 by rmcp's own streaming limit, never by running out
/// of memory. The padding is JSON whitespace, so the body stays a valid
/// JSON-RPC call right up to the boundary.
#[test]
fn a_body_at_exactly_the_cap_is_accepted_and_one_byte_more_is_413() {
    let server = start_server(&ServerOptions::new());
    let cap = 1024 * 1024;

    let base = jsonrpc_call("list_tools", &serde_json::json!({}));
    let at_cap = format!("{}{}", " ".repeat(cap - base.len()), base);
    assert_eq!(at_cap.len(), cap);
    let response = post_mcp(&server, Some(AGENT_TOKEN), &at_cap);
    assert!(
        status_line(&response).contains("200"),
        "exactly the cap is accepted: {}",
        status_line(&response)
    );

    let over_cap = format!(" {at_cap}");
    assert_eq!(over_cap.len(), cap + 1);
    let response = post_mcp(&server, Some(AGENT_TOKEN), &over_cap);
    assert!(
        status_line(&response).contains("413"),
        "one byte over the cap is 413: {}",
        status_line(&response)
    );
}

/// The same cap bounds the approvals form, which is axum's own
/// `DefaultBodyLimit` rather than rmcp's (task 10b's verify found it
/// falling back to axum's 2 MiB default). Over TCP, with the real
/// binary, against the configured value.
#[test]
fn an_oversized_approvals_form_is_refused_by_the_configured_cap() {
    let server = start_server(&ServerOptions::new());
    let cap = 1024 * 1024;
    let body = format!("decision=approve&nonce=x&reason={}", "a".repeat(cap));
    let credential = basic_credential("operator", APPROVER_PASSWORD);
    let response = server.raw(&format!(
        "POST /approvals/018f0000-0000-7000-8000-000000000000 HTTP/1.1\r\nHost: 127.0.0.1\r\n\
Authorization: Basic {credential}\r\nOrigin: https://127.0.0.1\r\n\
Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    assert!(
        status_line(&response).contains("413"),
        "an over-cap form is refused: {}",
        status_line(&response)
    );
}

/// A request that announces a body and never sends it is closed by the
/// server rather than held forever: the `tower-http` timeout is 30
/// seconds, so the connection must be answered or dropped well inside a
/// minute. Slow by construction (it waits out the real timeout), so it
/// is `#[ignore]`d by default and run by hand.
#[test]
#[ignore = "waits out the real 30 s request timeout"]
fn a_body_that_never_arrives_is_dropped_within_the_request_timeout() {
    let server = start_server(&ServerOptions::new());
    let started = std::time::Instant::now();
    let mut stream = TcpStream::connect(server.addr).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(90)))
        .unwrap();
    // Headers announcing 4,096 bytes, then nothing.
    stream
        .write_all(
            b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
Content-Length: 4096\r\n\r\n",
        )
        .unwrap();
    stream.flush().unwrap();
    let mut buffer = Vec::new();
    let _ = stream.read_to_end(&mut buffer);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(75),
        "the server held a stalled request for {elapsed:?}"
    );
    let text = String::from_utf8_lossy(&buffer);
    assert!(
        text.is_empty() || text.contains("408") || text.contains("401"),
        "a stalled request ends in a timeout, a refusal, or a closed socket: {text}"
    );
}

/// **A finding, pinned rather than fixed: headers that never terminate
/// are not bounded by anything.**
///
/// The 30-second `tower-http` timeout bounds the *service call*, and a
/// service call does not begin until hyper has read a complete request
/// head. A client that sends a request line, a `Host`, and then one more
/// header every couple of seconds forever therefore never starts the
/// clock -- this test holds a connection open well past the request
/// timeout and the server neither answers nor closes it. That is
/// slowloris, in its original form.
///
/// Not fixed here, and the reasoning is on the record. The fix is
/// hyper's own `http1::Builder::header_read_timeout`, which
/// `axum::serve` does not expose: reaching it means replacing
/// `axum::serve` with a hand-written accept loop over
/// `hyper_util::server::conn::auto::Builder`, plus its own graceful
/// shutdown and connection accounting -- a transport rewrite, not a
/// line. The cost of leaving it is bounded by the deployment this
/// milestone actually has: no public domain (the plan's own 2026-09-15
/// addendum), so the only reachable listener is the private network
/// behind Railway's proxy, and each held connection costs one socket and
/// a small buffer rather than a thread. Handed to milestone 3 with the
/// rest of the exposure work, where a public listener and this fix
/// belong together.
#[test]
#[ignore = "holds a connection for ~40 s to prove the header read is unbounded"]
fn headers_that_never_terminate_are_not_bounded_by_the_request_timeout() {
    let server = start_server(&ServerOptions::new());
    let started = std::time::Instant::now();
    let mut stream = TcpStream::connect(server.addr).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\n")
        .unwrap();
    stream.flush().unwrap();

    let mut answered = false;
    // Well past the 30-second request timeout.
    while started.elapsed() < Duration::from_secs(40) {
        if stream.write_all(b"X-Pad: pad\r\n").is_err() || stream.flush().is_err() {
            break;
        }
        let mut buffer = [0u8; 64];
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {
                answered = true;
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_secs(2)),
        }
    }
    assert!(
        !answered,
        "the server answered a request whose headers never terminated -- \
         if this starts passing, the header read has been bounded and this \
         test should become the assertion that it is"
    );
    assert!(
        started.elapsed() >= Duration::from_secs(40),
        "the connection was closed early, which would be the fix landing"
    );
}

// =====================================================================
// MCP helpers: plan, apply and read a run over the real transport.
// =====================================================================

/// The JSON-RPC result of one `tools/call`, or a panic naming the whole
/// HTTP response.
fn call_tool(
    server: &Server,
    token: &str,
    tool: &str,
    arguments: &serde_json::Value,
) -> serde_json::Value {
    let body = jsonrpc_call(tool, arguments);
    let response = post_mcp(server, Some(token), &body);
    let Some(index) = response.find("\r\n\r\n") else {
        panic!("no body in response: {response}");
    };
    let body = &response[index + 4..];
    serde_json::from_str(body).unwrap_or_else(|error| panic!("body is not JSON ({error}): {body}"))
}

/// `plan`'s recorded id, or the tool's own structured error.
fn plan_over_mcp(
    server: &Server,
    token: &str,
    workflow: &str,
    inputs: &serde_json::Value,
) -> Result<String, serde_json::Value> {
    let json = call_tool(
        server,
        token,
        "plan",
        &serde_json::json!({ "workflow": workflow, "inputs": inputs }),
    );
    let result = &json["result"];
    if result["isError"].as_bool() == Some(true) {
        return Err(result.clone());
    }
    result["structuredContent"]["plan_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| json.clone())
}

// =====================================================================
// Goal: replay or forge a plan_id.
// =====================================================================

/// A `plan_id` that is a perfectly well-formed `UUIDv7` minted a moment
/// ago -- the same shape this server itself mints -- is refused as
/// `UnknownPlan`: a plan is what the journal recorded, not what an id
/// looks like.
#[test]
fn a_freshly_minted_v7_plan_id_is_refused_as_unknown() {
    let server = start_server(&ServerOptions::new());
    let forged = forged_uuid_v7_now();
    let json = call_tool(
        &server,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": forged }),
    );
    let error = &json["result"]["structuredContent"];
    assert_eq!(error["kind"], "UnknownPlan", "{json}");
    assert!(
        server.wait_for_journal_event(|event| matches!(
            event,
            willikins_journal::Event::ApplyRefused {
                reason: willikins_journal::ApplyRefusedReason::UnknownPlan,
                ..
            }
        )),
        "the refusal is journaled"
    );
}

/// A `plan_id` copied out of a *different* server's journal is refused
/// the same way: nothing about an id carries authority across journals.
#[test]
fn a_plan_id_from_another_journal_is_refused_as_unknown() {
    let first = start_server(&ServerOptions::new());
    let plan_id = plan_over_mcp(
        &first,
        AGENT_TOKEN,
        "new-rust-service",
        &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
    )
    .expect("the positive fixture plans");

    let second = start_server(&ServerOptions::new());
    let json = call_tool(
        &second,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": plan_id }),
    );
    assert_eq!(json["result"]["structuredContent"]["kind"], "UnknownPlan");
}

/// Adversarial pass 2's decision, pinned end to end: a plan one agent
/// token recorded may be applied by a *second* agent token. The plan is
/// fixed at plan time and `apply` re-verifies all of it; the requester is
/// audit, not authorization. Both principals are on the record --
/// `plan_recorded.principal` names the first, `run_started.principal` the
/// second -- so an operator reading the journal sees the split.
///
/// See `Butler::apply`'s own doc for why refusing would invent a
/// principal class the trust boundaries do not have.
#[test]
fn a_second_agent_token_may_apply_the_first_agents_plan_and_both_are_journaled() {
    let server = start_server(&ServerOptions::new());
    let plan_id = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "new-rust-service",
        &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
    )
    .expect("the positive fixture plans");

    let json = call_tool(
        &server,
        SECOND_AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": plan_id }),
    );
    assert_ne!(
        json["result"]["isError"].as_bool(),
        Some(true),
        "a second agent token may apply: {json}"
    );

    let replayed = server.replay_journal();
    let requester = replayed
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            willikins_journal::Event::PlanRecorded { principal, .. } => principal.clone(),
            _ => None,
        })
        .expect("the plan records its requester");
    let applier = replayed
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            willikins_journal::Event::RunStarted { principal, .. } => Some(principal.clone()),
            _ => None,
        })
        .expect("the run records who started it");
    assert_ne!(
        requester.to_string(),
        applier.to_string(),
        "two different agent principals, both recorded"
    );
    assert!(requester.to_string().starts_with("agent-"));
    assert!(applier.to_string().starts_with("agent-"));
}

/// The requester survives a restart: the second process replays the
/// journal, and `GET /approvals` still names who asked, rather than the
/// "unknown" task 10b's process-lifetime map produced.
#[test]
fn the_approvals_page_names_the_requester_after_a_restart() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workflows_dir = dir.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    copy_fixture_as(
        &workflows_dir,
        "hostile-description-pending.yaml",
        "hostile-description-pending.yaml",
    );
    let journal_path = dir.path().join("journal.jsonl");

    let plan_id = {
        let first = start_server_at(&workflows_dir, &journal_path);
        plan_over_mcp(
            &first,
            AGENT_TOKEN,
            "hostile-description-pending",
            &serde_json::json!({ "slug": "third-thoughts" }),
        )
        .expect("a pending plan")
        // `first` is dropped here: the child is killed and the journal's
        // exclusive lock released, which is the restart.
    };

    let second = start_server_at(&workflows_dir, &journal_path);
    let credential = basic_credential("operator", APPROVER_PASSWORD);
    let page = second.raw(&format!(
        "GET /approvals HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {credential}\r\n\
Connection: close\r\n\r\n"
    ));
    assert!(page.contains(&plan_id), "the plan is still pending: {page}");
    assert!(
        page.contains("requester: agent-"),
        "the requester survives the restart: {page}"
    );
    assert!(
        !page.contains("requester: unknown"),
        "the requester is not lost: {page}"
    );
}

// =====================================================================
// Goal: escape the trusted directory, over HTTP.
// =====================================================================

/// Every shape of "a name that is not a name" is refused at parameter
/// parsing, before the trusted directory is touched: percent-encoded
/// traversal, a NUL, unicode look-alikes for `.` and `/`, and an
/// upper-case spelling of a document that really is there.
///
/// `WorkflowName`'s grammar is `^[a-z0-9]([a-z0-9-]*[a-z0-9])?$` with no
/// upper case and no separator, so none of these can name a file. The
/// case one matters twice over: **this host's filesystem is
/// case-insensitive and Railway's is not**, so `New-Rust-Service.yaml`
/// would find `new-rust-service.yaml` here and not there -- and the
/// grammar is what makes that difference unobservable, because the name
/// never reaches the filesystem at all. Both readings are pinned below:
/// the request is refused here, and the refusal does not depend on the
/// filesystem's own answer.
#[test]
fn no_spelling_of_a_workflow_name_escapes_the_trusted_directory() {
    let server = start_server(&ServerOptions::new());
    for name in [
        "../new-rust-service",
        "..%2fnew-rust-service",
        "%2e%2e%2fnew-rust-service",
        "new-rust-service\u{0}",
        "new-rust-service\u{0}.yaml",
        // A look-alike full stop and solidus.
        "\u{ff0e}\u{ff0e}\u{ff0f}new-rust-service",
        "new\u{2010}rust\u{2010}service",
        "NEW-RUST-SERVICE",
        "New-Rust-Service",
        "/etc/passwd",
        "new-rust-service.yaml",
    ] {
        let outcome = plan_over_mcp(
            &server,
            AGENT_TOKEN,
            name,
            &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
        );
        let error = outcome.expect_err(&format!("`{name}` must not resolve to a document"));
        let text = error.to_string();
        assert!(
            !text.contains("plan_id"),
            "`{name}` must not have planned: {text}"
        );
    }

    // And the honest spelling still works, so the refusals above are the
    // grammar's doing, not a broken directory.
    plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "new-rust-service",
        &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
    )
    .expect("the real name still plans");
}

/// The case-insensitivity of *this* host's filesystem, measured rather
/// than assumed, so the note's claim about Railway is anchored to
/// something. Nothing in the server depends on the answer -- see the
/// test above -- but a future change that started resolving names
/// through the filesystem would inherit this difference, and the record
/// should say what it is.
#[test]
fn an_upper_case_name_cannot_reach_the_filesystem_whatever_this_host_folds() {
    use willikins_types::DomainType as _;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("case-probe.yaml"), "x").unwrap();
    let case_insensitive = dir.path().join("Case-Probe.yaml").is_file();
    // macOS (APFS, default) folds case; Linux (ext4/overlayfs, which is
    // what Railway runs) does not. Printed, not asserted: this is an
    // observation about the host, and asserting either answer would make
    // the test fail on the other one.
    println!("filesystem is case-insensitive: {case_insensitive}");

    // What *is* asserted is the invariant that makes the difference
    // unobservable, and it holds on both hosts: an upper-case spelling
    // is not a `WorkflowName` at all, so it is refused before any path
    // is built and the filesystem's own answer never gets a say.
    //
    // Rewritten by adversarial pass 2's completeness critic, 2026-09-15.
    // The pass shipped this as `this_hosts_filesystem_case_sensitivity_is_recorded`,
    // whose only assertion was that the file it had just written existed
    // -- it measured the fold, printed it into captured output a green
    // run discards, and pinned nothing. The note claimed "both readings
    // of the case question, pinned"; only one of them was.
    assert!(
        willikins_types::WorkflowName::parse("Case-Probe").is_err(),
        "an upper-case name must be refused by the grammar"
    );
    assert!(
        willikins_types::WorkflowName::parse("case-probe").is_ok(),
        "and the honest spelling must still be a name"
    );
    // The sibling test drives the same upper-case spelling through the
    // real binary and requires the same refusal; together they say the
    // refusal is the grammar's and not the directory's.
    assert!(dir.path().join("case-probe.yaml").is_file());
}

/// A document dropped into the trusted directory *after* startup, whose
/// own `name:` does not match its filename stem, breaks `list_workflows`
/// for every principal -- `scan_directory` refuses the whole scan at the
/// first bad entry rather than skipping it.
///
/// Recorded, not fixed. The trusted directory is a checkout of a trusted
/// ref that only the operator writes, so this is an operator-level
/// footgun rather than an attack surface; and "refuse the whole listing,
/// naming the file" is the same fail-closed rule startup itself follows,
/// which is the behaviour you want when the alternative is silently
/// listing a directory that is not what the operator thinks it is. The
/// cost is on the record: one bad file denies the listing to everyone
/// until it is removed, and `plan` of an *unrelated*, valid document
/// still works, which is what this pins.
#[test]
fn a_mismatched_document_added_after_startup_denies_list_workflows_but_not_plan() {
    let server = start_server(&ServerOptions::new());
    let listed = call_tool(
        &server,
        AGENT_TOKEN,
        "list_workflows",
        &serde_json::json!({}),
    );
    assert_ne!(
        listed["result"]["isError"].as_bool(),
        Some(true),
        "{listed}"
    );

    std::fs::write(
        server.workflows_dir.join("intruder.yaml"),
        "name: something-else\ndescription: x\nsteps: {}\n",
    )
    .unwrap();

    let listed = call_tool(
        &server,
        AGENT_TOKEN,
        "list_workflows",
        &serde_json::json!({}),
    );
    assert_eq!(
        listed["result"]["isError"].as_bool(),
        Some(true),
        "one mismatched file refuses the whole listing: {listed}"
    );
    let error = &listed["result"]["structuredContent"];
    assert_eq!(error["kind"], "Startup", "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("intruder"),
        "the refusal names the file: {error}"
    );

    // The valid document still plans: the refusal is the listing's, not
    // the directory's.
    plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "new-rust-service",
        &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
    )
    .expect("an unrelated valid document still plans");
}

/// A symlinked document placed after startup is refused, not followed:
/// `plan` collapses it to `UnknownWorkflow`, exactly as if the directory
/// held nothing by that name.
#[cfg(unix)]
#[test]
fn a_symlinked_document_placed_after_startup_is_not_followed() {
    let server = start_server(&ServerOptions::new());
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("elsewhere.yaml");
    std::fs::write(
        &target,
        "name: linked\ndescription: x\nsteps:\n  names:\n    tool: naming.v1\n    with:\n      org: lightless-labs\n      slug: third-thoughts\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&target, server.workflows_dir.join("linked.yaml")).unwrap();

    let error = plan_over_mcp(&server, AGENT_TOKEN, "linked", &serde_json::json!({}))
        .expect_err("a symlinked document must not resolve");
    assert_eq!(
        error["structuredContent"]["kind"], "UnknownWorkflow",
        "{error}"
    );
}

// =====================================================================
// Goal: the tracing quarter adversarial pass 1 could not test.
// =====================================================================

/// **The binary does not read `RUST_LOG`, and INFO is the most verbose
/// level it will emit.** `cmd_serve_http` builds the subscriber as
/// `tracing_subscriber::fmt().json().with_writer(stderr).try_init()` --
/// the *builder's* `try_init`, which (unlike the module-level
/// `fmt::try_init`) installs no `EnvFilter`, and the workspace does not
/// enable tracing-subscriber's `env-filter` feature at all
/// (`Cargo.toml`: `features = ["json"]`). So the level is the `fmt`
/// default, INFO, whatever the environment says.
///
/// Pinned because it is load-bearing in both directions: an operator
/// cannot raise verbosity in production without a code change (which is
/// a real limitation, recorded), and the sweep below is therefore a
/// sweep of INFO only -- weaker evidence than it looks, which the
/// research note says out loud.
#[test]
fn the_binary_ignores_rust_log_and_emits_nothing_below_info() {
    let server = start_server(&ServerOptions::new().var("RUST_LOG", "trace"));
    // Drive some traffic so there is something to log at all.
    let _ = server.raw("GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    let _ = post_mcp(
        &server,
        None,
        &jsonrpc_call("list_tools", &serde_json::json!({})),
    );
    let _ = call_tool(
        &server,
        AGENT_TOKEN,
        "list_workflows",
        &serde_json::json!({}),
    );
    std::thread::sleep(Duration::from_millis(200));

    let text = server.stderr_text();
    assert!(
        text.contains("willikins-server listening"),
        "the startup line is INFO and is emitted: {text}"
    );
    for level in ["\"DEBUG\"", "\"TRACE\""] {
        assert!(
            !text.contains(level),
            "RUST_LOG=trace must change nothing; found {level} in:\n{text}"
        );
    }
}

/// The whole-session sweep: a plan, an apply, a run status, the
/// approvals page with a nonce round trip, and a 401 -- then every byte
/// the process wrote to stderr and stdout is searched for the agent
/// token, the approver password, any `Authorization` header value, the
/// nonce, and the `dp.st.` prefix every fake-minted Doppler service
/// token carries.
///
/// The nonce matters as much as the token: a nonce in a log line is a
/// replay for anyone who can read logs, which on a hosted deployment is
/// everyone with access to the project.
#[test]
#[allow(clippy::too_many_lines)] // one session, driven end to end; splitting it would hide what the sweep covers
fn no_secret_token_password_or_nonce_reaches_stderr_or_stdout() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workflows_dir = dir.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    copy_fixture_as(
        &workflows_dir,
        "new-rust-service.yaml",
        "new-rust-service.yaml",
    );
    copy_fixture_as(
        &workflows_dir,
        "hostile-description-pending.yaml",
        "hostile-description-pending.yaml",
    );
    let journal_path = dir.path().join("journal.jsonl");
    let server = start_server_at(&workflows_dir, &journal_path);

    // initialize, then a plan that mints a fake Doppler service token
    // when applied.
    let _ = post_mcp(
        &server,
        Some(AGENT_TOKEN),
        &serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "pass-2", "version": "0.1.0"},
            },
        })
        .to_string(),
    );
    let plan_id = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "new-rust-service",
        &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
    )
    .expect("the positive fixture plans");
    let applied = call_tool(
        &server,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": plan_id }),
    );
    let run_id = applied["result"]["structuredContent"]["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("apply must start a run: {applied}"))
        .to_string();
    // Poll the run to a final state, so the whole apply (token mint
    // included) has actually happened before the sweep.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status = call_tool(
            &server,
            AGENT_TOKEN,
            "run_status",
            &serde_json::json!({ "run_id": run_id }),
        );
        let state = status["result"]["structuredContent"]["state"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if state != "running" || std::time::Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // A pending plan, the approvals page, and a nonce round trip.
    let pending = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "hostile-description-pending",
        &serde_json::json!({ "slug": "third-thoughts" }),
    )
    .expect("a pending plan");
    let credential = basic_credential("operator", APPROVER_PASSWORD);
    let page = server.raw(&format!(
        "GET /approvals HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {credential}\r\n\
Connection: close\r\n\r\n"
    ));
    let nonce = extract_nonce(&page, &pending);
    let form = format!("decision=approve&nonce={nonce}");
    let decided = server.raw(&format!(
        "POST /approvals/{pending} HTTP/1.1\r\nHost: 127.0.0.1\r\n\
Authorization: Basic {credential}\r\nOrigin: https://127.0.0.1\r\n\
Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{form}",
        form.len()
    ));
    assert!(
        status_line(&decided).contains("303"),
        "the nonce round trip decides: {decided}"
    );

    // And a 401.
    let _ = post_mcp(
        &server,
        None,
        &jsonrpc_call("list_tools", &serde_json::json!({})),
    );
    std::thread::sleep(Duration::from_millis(300));

    let stderr = server.stderr_text();
    let stdout = server.stdout_text();
    // The sweep is only evidence if there is something to sweep: a
    // capture that came back empty would pass every assertion below
    // without looking at a single line the binary wrote. Added by
    // adversarial pass 2's completeness critic, 2026-09-15.
    assert!(
        stderr.contains("willikins-server listening"),
        "the stderr capture must be live for this sweep to mean anything: {stderr:?}"
    );
    for (what, needle) in [
        ("the agent bearer token", AGENT_TOKEN),
        ("the approver password", APPROVER_PASSWORD),
        ("the Basic credential", credential.as_str()),
        ("the approvals nonce", nonce.as_str()),
        // Every fake-minted Doppler service token starts with this.
        ("a Doppler service token", "dp.st."),
        ("an Authorization header value", "Bearer "),
    ] {
        assert!(!stderr.contains(needle), "{what} reached stderr:\n{stderr}");
        assert!(!stdout.contains(needle), "{what} reached stdout:\n{stdout}");
    }
    // The journal is the audit source of truth and must not carry them
    // either -- pass 1 proved that for the executor's own events; this
    // is the same sweep over a journal written by the real binary.
    let journal = std::fs::read_to_string(&journal_path).unwrap();
    for needle in [
        AGENT_TOKEN,
        APPROVER_PASSWORD,
        credential.as_str(),
        nonce.as_str(),
        "dp.st.",
    ] {
        assert!(
            !journal.contains(needle),
            "`{needle}` reached the journal file"
        );
    }
}

// =====================================================================
// The two handed-over items adversarial pass 2 did not attack.
// Added by its completeness critic, 2026-09-15.
// =====================================================================

/// Append a `RunStarted` for `plan_id` to an existing journal file, with
/// the next sequence number and the last line's own timestamp: exactly
/// what a process that died between `RunStarted` and `RunFinished`
/// leaves on the volume. Written by hand rather than by racing a
/// `SIGKILL`, so the test is deterministic.
fn append_orphan_run_started(journal_path: &Path, run_id: &str, plan_id: &str) {
    let text = std::fs::read_to_string(journal_path).expect("the journal exists");
    let last = text
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a non-empty journal");
    let parsed: serde_json::Value = serde_json::from_str(last).expect("the last line is JSON");
    let seq = parsed["seq"].as_u64().expect("a seq") + 1;
    let at = parsed["at"].as_str().expect("a timestamp").to_string();
    let line = serde_json::json!({
        "seq": seq,
        "at": at,
        "event": {
            "kind": "run_started",
            "run_id": run_id,
            "plan_id": plan_id,
            "principal": "agent-0123456789ab",
        },
    });
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(journal_path)
        .expect("the journal opens for append");
    writeln!(file, "{line}").expect("the orphan RunStarted appends");
}

/// **Crash recovery.** Task 12 handed over "the crash-recovery detour
/// (volume detach and reattach) is untested" and adversarial pass 2 did
/// not attack it: every restart it drives is a *clean* one, a child
/// killed between two complete records. The shape a detached volume
/// actually leaves behind is a journal whose last record is a
/// `RunStarted` with no `RunFinished`, and Railway is not needed to
/// make one.
///
/// Four things must hold, and do:
///
/// - the second server **starts**. An unfinished run is a valid journal,
///   not a truncated one, so the fail-closed replay has nothing to
///   refuse -- the refusal task 12 asked about (finding 17) is about a
///   half-written *line*, which is a different fault.
/// - the run reads `running` for ever. That is the honest answer, and
///   it is finding 13's state reached by the other road: willikins does
///   not know how that run ended.
/// - the crashed plan is **`AlreadyApplied`**. `PlanRecord::applied` is
///   folded from `RunStarted`, not from a finished run, so a crash
///   cannot buy a second apply of a plan that may already have written
///   to a provider. This is the one that would matter: the opposite
///   answer would make "kill the process mid-run" a way to run an
///   irreversible plan twice.
/// - and the single-apply slot is **`Idle`**, so an unrelated plan still
///   applies. A crash must not wedge the server until someone edits the
///   journal.
#[test]
fn a_run_that_never_finished_recovers_without_wedging_the_server() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workflows_dir = dir.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    copy_fixture_as(
        &workflows_dir,
        "new-rust-service.yaml",
        "new-rust-service.yaml",
    );
    let journal_path = dir.path().join("journal.jsonl");

    let (crashed_plan, unrelated_plan) = {
        let first = start_server_at(&workflows_dir, &journal_path);
        let crashed = plan_over_mcp(
            &first,
            AGENT_TOKEN,
            "new-rust-service",
            &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
        )
        .expect("the positive fixture plans");
        let unrelated = plan_over_mcp(
            &first,
            AGENT_TOKEN,
            "new-rust-service",
            &serde_json::json!({ "slug": "fourth-thoughts", "org": "lightless-labs" }),
        )
        .expect("and plans a second time");
        (crashed, unrelated)
        // `first` is dropped here: the child is killed and the journal's
        // exclusive lock released.
    };

    // The crash itself.
    let run_id = forged_uuid_v7_now();
    append_orphan_run_started(&journal_path, &run_id, &crashed_plan);

    let second = start_server_at(&workflows_dir, &journal_path);

    let status = call_tool(
        &second,
        AGENT_TOKEN,
        "run_status",
        &serde_json::json!({ "run_id": run_id }),
    );
    assert_eq!(
        status["result"]["structuredContent"]["state"], "running",
        "a run with no RunFinished reads running: {status}"
    );

    let refused = call_tool(
        &second,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": crashed_plan }),
    );
    assert_eq!(
        refused["result"]["structuredContent"]["kind"], "AlreadyApplied",
        "a crash must not buy a second apply: {refused}"
    );

    let applied = call_tool(
        &second,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": unrelated_plan }),
    );
    assert!(
        applied["result"]["structuredContent"]["run_id"].is_string(),
        "a crashed run must not hold the single-apply slot: {applied}"
    );
}

/// **What an agent is actually handed when an apply is refused.**
///
/// Task 11 handed over "rmcp round-trips for the four remaining apply
/// refusal kinds are pinned by type identity only" -- a `matches!` on
/// the Rust enum, which says nothing about the JSON that crosses the
/// transport. Adversarial pass 2 did not pick that item up, and then
/// added two more kinds to the same list. These are the refusals an
/// agent can provoke from outside the process, each driven through the
/// real binary and read back as the `kind` string an agent branches on.
///
/// Not reachable from outside, and named rather than quietly skipped:
/// `PlanExpired` needs the clock, `Drift` and `PlanFailed` need a
/// provider that changes its answer between plan and apply, and
/// `ApplyPreparing` and `RunInProgress` need two applies in flight --
/// all five are pinned in process (`adversarial_10a.rs`,
/// `adversarial_10b.rs`, `blocking_pool_13.rs`). `UnknownPlan` and
/// `AlreadyApplied` have their own tests in this file.
#[test]
fn the_apply_refusals_an_agent_can_provoke_name_their_kind_over_the_wire() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workflows_dir = dir.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    copy_fixture_as(
        &workflows_dir,
        "new-rust-service.yaml",
        "new-rust-service.yaml",
    );
    copy_fixture_as(
        &workflows_dir,
        "hostile-description-pending.yaml",
        "hostile-description-pending.yaml",
    );
    let journal_path = dir.path().join("journal.jsonl");
    let server = start_server_at(&workflows_dir, &journal_path);

    // ApprovalRequired: a plan above the threshold, applied with no
    // decision recorded for it.
    let pending = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "hostile-description-pending",
        &serde_json::json!({ "slug": "third-thoughts" }),
    )
    .expect("a pending plan");
    let refused = call_tool(
        &server,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": pending }),
    );
    assert_eq!(
        refused["result"]["structuredContent"]["kind"], "ApprovalRequired",
        "{refused}"
    );

    // DocumentChanged: the document the plan was made against is not the
    // document on disk any more. A trailing comment is enough -- the
    // check is the SHA-256, not the meaning.
    let planned = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "new-rust-service",
        &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
    )
    .expect("the positive fixture plans");
    let document = workflows_dir.join("new-rust-service.yaml");
    let before = std::fs::read_to_string(&document).unwrap();
    std::fs::write(&document, format!("{before}\n# changed after the plan\n")).unwrap();
    let changed = call_tool(
        &server,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": planned }),
    );
    assert_eq!(
        changed["result"]["structuredContent"]["kind"], "DocumentChanged",
        "{changed}"
    );
}

/// `RecordedInputUnreadable` over the wire, which needs a restart: the
/// refusal exists because a plan's resolved inputs are rebuilt from the
/// journal, and that only happens in a process that did not record them.
///
/// `adversarial_10b.rs` pins the `ButlerError`'s own serialization; this
/// pins what an agent is handed, through the real transport, and that
/// the refusal names the input rather than inventing a planning error
/// (finding 3's whole point).
///
/// Added by adversarial pass 2's completeness critic, 2026-09-15.
#[test]
fn a_recorded_input_that_no_longer_parses_names_the_input_over_the_wire() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workflows_dir = dir.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    copy_fixture_as(
        &workflows_dir,
        "new-rust-service.yaml",
        "new-rust-service.yaml",
    );
    let journal_path = dir.path().join("journal.jsonl");

    let plan_id = {
        let first = start_server_at(&workflows_dir, &journal_path);
        plan_over_mcp(
            &first,
            AGENT_TOKEN,
            "new-rust-service",
            &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
        )
        .expect("the positive fixture plans")
    };

    // Hand-edit the recorded value into something `ProjectSlug` refuses.
    let text = std::fs::read_to_string(&journal_path).unwrap();
    let edited = text.replace("third-thoughts", "NOT A SLUG");
    assert_ne!(edited, text, "the recorded value must really be in there");
    std::fs::write(&journal_path, edited).unwrap();

    let second = start_server_at(&workflows_dir, &journal_path);
    let refused = call_tool(
        &second,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": plan_id }),
    );
    let error = &refused["result"]["structuredContent"];
    assert_eq!(error["kind"], "RecordedInputUnreadable", "{refused}");
    assert_eq!(
        error["input"], "slug",
        "the refusal names the input, not a planning-error kind: {refused}"
    );
}

/// The nonce in the page's own form, for `plan_id`.
fn extract_nonce(html: &str, plan_id: &str) -> String {
    let section = html
        .split("<section class=\"plan\">")
        .find(|section| section.contains(plan_id))
        .unwrap_or_else(|| panic!("no section for {plan_id} in:\n{html}"));
    let marker = "name=\"nonce\" value=\"";
    let start = section
        .find(marker)
        .unwrap_or_else(|| panic!("no nonce in:\n{section}"))
        + marker.len();
    let rest = &section[start..];
    let end = rest.find('"').expect("the nonce value is quoted");
    rest[..end].to_string()
}

// =====================================================================
// Goal: confuse the approval gate, over TCP.
// =====================================================================

/// Two pending plans, and plan A's nonce posted against plan B.
///
/// **Adversarial pass 2's decision, over the real transport.** Task 10b
/// left `NonceStore::consume` removing the entry whether or not the
/// presented value matched, which meant this request burned *B's* nonce
/// -- the approver's own pending decision stopped working because of a
/// request that never proved it knew anything. Compare-then-remove is
/// now the rule: the mismatch is refused, B's nonce survives, and A's is
/// untouched too.
#[test]
fn a_nonce_posted_against_another_plan_burns_neither() {
    let server = start_server(&ServerOptions::new().document(
        "hostile-description-pending.yaml",
        "hostile-description-pending.yaml",
    ));
    let plan_a = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "hostile-description-pending",
        &serde_json::json!({ "slug": "third-thoughts" }),
    )
    .expect("plan A");
    let plan_b = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "hostile-description-pending",
        &serde_json::json!({ "slug": "second-thoughts" }),
    )
    .expect("plan B");
    assert_ne!(plan_a, plan_b);

    let credential = basic_credential("operator", APPROVER_PASSWORD);
    let page = server.raw(&format!(
        "GET /approvals HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {credential}\r\n\
Connection: close\r\n\r\n"
    ));
    let nonce_a = extract_nonce(&page, &plan_a);
    let nonce_b = extract_nonce(&page, &plan_b);
    assert_ne!(nonce_a, nonce_b);

    // A's nonce against B: refused, and journaled as `invalid_nonce`
    // rather than `invalid_credential` (the approver's password was
    // right).
    let crossed = post_decision(
        &server,
        &credential,
        &plan_b,
        "approve",
        &nonce_a,
        "https://127.0.0.1",
    );
    assert!(
        status_line(&crossed).contains("403"),
        "a crossed nonce is refused: {crossed}"
    );
    assert!(
        server.wait_for_journal_event(|event| matches!(
            event,
            willikins_journal::Event::AuthFailed {
                reason: willikins_journal::AuthFailedReason::InvalidNonce,
                ..
            }
        )),
        "the refusal is journaled with its own reason"
    );

    // Neither plan's nonce was burned.
    let decided_b = post_decision(
        &server,
        &credential,
        &plan_b,
        "approve",
        &nonce_b,
        "https://127.0.0.1",
    );
    assert!(
        status_line(&decided_b).contains("303"),
        "B's own nonce still decides: {decided_b}"
    );
    let decided_a = post_decision(
        &server,
        &credential,
        &plan_a,
        "approve",
        &nonce_a,
        "https://127.0.0.1",
    );
    assert!(
        status_line(&decided_a).contains("303"),
        "A's own nonce still decides: {decided_a}"
    );
}

/// A forged cross-site POST -- the exact shape review resolution 1 is
/// against, a browser re-attaching cached Basic credentials to a form on
/// another site -- is refused on the `Origin` before the nonce is even
/// looked at, and journaled as `foreign_origin`.
#[test]
fn a_cross_site_post_is_refused_on_the_origin_and_journaled_as_such() {
    let server = start_server(&ServerOptions::new().document(
        "hostile-description-pending.yaml",
        "hostile-description-pending.yaml",
    ));
    let plan_id = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "hostile-description-pending",
        &serde_json::json!({ "slug": "third-thoughts" }),
    )
    .expect("a pending plan");
    let credential = basic_credential("operator", APPROVER_PASSWORD);
    let page = server.raw(&format!(
        "GET /approvals HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {credential}\r\n\
Connection: close\r\n\r\n"
    ));
    let nonce = extract_nonce(&page, &plan_id);

    let forged = post_decision(
        &server,
        &credential,
        &plan_id,
        "approve",
        &nonce,
        "https://evil.example",
    );
    assert!(status_line(&forged).contains("403"), "{forged}");
    assert!(server.wait_for_journal_event(|event| matches!(
        event,
        willikins_journal::Event::AuthFailed {
            reason: willikins_journal::AuthFailedReason::ForeignOrigin,
            ..
        }
    )));

    // The approver's own nonce is untouched by the forgery.
    let honest = post_decision(
        &server,
        &credential,
        &plan_id,
        "approve",
        &nonce,
        "https://127.0.0.1",
    );
    assert!(status_line(&honest).contains("303"), "{honest}");
}

/// The approver's password with a username that claims the `agent-`
/// namespace, or that is not a principal at all, is 403 and journaled as
/// `malformed_username` -- not `invalid_credential`, which would have an
/// operator rotating a password that was never wrong.
#[test]
fn an_approver_password_with_a_bad_username_is_journaled_as_a_bad_username() {
    let server = start_server(&ServerOptions::new());
    for username in ["agent-0123456789ab", "not a principal", ""] {
        let credential = basic_credential(username, APPROVER_PASSWORD);
        let response = server.raw(&format!(
            "GET /approvals HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {credential}\r\n\
Connection: close\r\n\r\n"
        ));
        assert!(
            status_line(&response).contains("403"),
            "`{username}` must not be an approver identity: {response}"
        );
    }
    assert!(server.wait_for_journal_event(|event| matches!(
        event,
        willikins_journal::Event::AuthFailed {
            reason: willikins_journal::AuthFailedReason::MalformedUsername,
            ..
        }
    )));
}

fn post_decision(
    server: &Server,
    credential: &str,
    plan_id: &str,
    decision: &str,
    nonce: &str,
    origin: &str,
) -> String {
    let form = format!("decision={decision}&nonce={nonce}");
    server.raw(&format!(
        "POST /approvals/{plan_id} HTTP/1.1\r\nHost: 127.0.0.1\r\n\
Authorization: Basic {credential}\r\nOrigin: {origin}\r\n\
Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{form}",
        form.len()
    ))
}

// =====================================================================
// Goal: every error surface answers with one key per name.
// =====================================================================

/// Assert that no JSON object anywhere in `text` carries the same key
/// twice.
///
/// `serde_json` folds a duplicate key silently -- the last one wins --
/// so a parsed value can never show this. The scan is over the raw
/// bytes, quote- and escape-aware, at every nesting level: task 11's own
/// helper looked at the top level only, and the collision this pass was
/// asked to hunt (`willikins_dsl::DocumentErrorKind`'s own `message`
/// against the one `Reported` adds) is one level down.
fn assert_no_duplicate_json_keys(text: &str, what: &str) {
    let mut stack: Vec<Vec<String>> = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut pending_key: Option<String> = None;
    while let Some((_, c)) = chars.next() {
        match c {
            '{' | '[' => stack.push(Vec::new()),
            '}' | ']' => {
                stack.pop();
            }
            '"' => {
                let mut value = String::new();
                let mut escaped = false;
                for (_, c) in chars.by_ref() {
                    if escaped {
                        value.push(c);
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        break;
                    } else {
                        value.push(c);
                    }
                }
                pending_key = Some(value);
            }
            ':' => {
                if let (Some(key), Some(keys)) = (pending_key.take(), stack.last_mut()) {
                    assert!(
                        !keys.contains(&key),
                        "{what}: duplicate key `{key}` in one JSON object:\n{text}"
                    );
                    keys.push(key);
                }
            }
            ',' => pending_key = None,
            _ => {}
        }
    }
}

/// Every error the MCP surface can be made to emit, swept for a
/// duplicate key at any nesting level.
///
/// `Reported`'s `serde(flatten)` writes *both* keys when the wrapped
/// type declares one of its own, and every JSON reader then folds them
/// back to one, silently, with no rule saying which it keeps -- so an
/// agent can be handed a `message` that is not the message willikins
/// meant. Task 11's verify found four of these and renamed the fields;
/// this is the sweep that says whether any are left.
#[test]
fn no_error_any_mcp_surface_emits_carries_a_duplicate_key() {
    let server = start_server(&ServerOptions::new());
    let provocations: Vec<(&str, &str, serde_json::Value)> = vec![
        (
            "malformed yaml",
            "validate",
            serde_json::json!({ "document": "name: [unclosed" }),
        ),
        (
            "a semantic document error",
            "validate",
            serde_json::json!({ "document": "name: Not A Slug\ndescription: x\nsteps: {}\n" }),
        ),
        (
            "an over-large document",
            "validate",
            serde_json::json!({ "document": "#".repeat(300 * 1024) }),
        ),
        (
            "an unknown workflow",
            "plan",
            serde_json::json!({ "workflow": "no-such-workflow", "inputs": {} }),
        ),
        (
            "a rejected input",
            "plan",
            serde_json::json!({ "workflow": "new-rust-service", "inputs": { "slug": "Not A Slug", "org": "lightless-labs" } }),
        ),
        (
            "a missing input",
            "plan",
            serde_json::json!({ "workflow": "new-rust-service", "inputs": {} }),
        ),
        (
            "an unknown plan",
            "apply",
            serde_json::json!({ "plan_id": "018f0000-0000-7000-8000-000000000000" }),
        ),
        (
            "an unknown run",
            "run_status",
            serde_json::json!({ "run_id": "018f0000-0000-7000-8000-000000000000" }),
        ),
        (
            "an invalid project name",
            "propose_slug",
            serde_json::json!({ "name": "" }),
        ),
        (
            "a document that fails check",
            "describe",
            serde_json::json!({ "document": "name: demo\ndescription: x\nsteps:\n  a:\n    tool: no.such.tool\n    with: {}\n", "inputs": {} }),
        ),
    ];

    for (what, tool, arguments) in provocations {
        let body = jsonrpc_call(tool, &arguments);
        let response = post_mcp(&server, Some(AGENT_TOKEN), &body);
        let index = response
            .find("\r\n\r\n")
            .unwrap_or_else(|| panic!("{what}: no body in {response}"));
        assert_no_duplicate_json_keys(&response[index + 4..], what);
    }
}

// =====================================================================
// Goal: inject through document text.
// =====================================================================

/// Document text cannot forge a line in the server's own JSON `tracing`
/// output, and cannot paint the terminal of whoever is tailing it.
///
/// Two defences, and this measures both.
///
/// The first is the DSL's, and it is stronger than expected: a
/// `description` carrying *any* control character -- a newline, an ESC --
/// is refused at parse, naming the character. So the classic log-forging
/// payload (a newline plus a plausible-looking JSON object) and the
/// terminal-painting payload (an ANSI escape) never become a `Workflow`
/// at all, on either surface.
///
/// The second is that no document text reaches the log output anyway.
/// `TraceLayer::new_for_http()`'s defaults record the method, the path,
/// the status and the latency and nothing else, so a description made
/// entirely of JSON punctuation -- which the DSL does accept, since
/// quotes and braces are not control characters -- still never appears on
/// stderr, and every line stderr carries is still one well-formed JSON
/// object.
#[test]
fn document_text_cannot_forge_a_tracing_line_or_paint_a_terminal() {
    let server = start_server(&ServerOptions::new());

    // Part one: a control character in a description is refused at parse.
    // Written as YAML's own `\uXXXX` escape so this source file carries no
    // raw control character of its own.
    for (label, payload) in [
        ("a newline", r#""line one\nline two""#),
        ("an ANSI escape", r#""ESCAPE[31mRED""#),
    ] {
        let document = format!("name: hostile-tracing\ndescription: {payload}\nsteps: {{}}\n")
            .replace("ESCAPE", "\\u001b");
        let json = call_tool(
            &server,
            AGENT_TOKEN,
            "validate",
            &serde_json::json!({ "document": document }),
        );
        let error = &json["result"]["structuredContent"];
        assert_eq!(
            error["kind"], "Document",
            "{label} in a description must be refused at parse: {json}"
        );
        assert!(
            error["message"]
                .as_str()
                .unwrap_or_default()
                .contains("control character"),
            "{label}: the refusal names the control character: {error}"
        );
    }

    // Part two: what the DSL *does* accept still reaches no log line.
    let hostile = concat!(
        "name: hostile-tracing\n",
        r#"description: '","level":"ERROR","message":"forged PASS2MARKER'"#,
        "\n",
        "inputs:\n",
        r#"  slug: { type: ProjectSlug, description: '","level":"ERROR" PASS2MARKER' }"#,
        "\n",
        "steps:\n",
        "  names:\n",
        "    tool: naming.v1\n",
        "    with:\n",
        "      org: lightless-labs\n",
        "      slug: ${{ inputs.slug }}\n",
    );
    std::fs::write(server.workflows_dir.join("hostile-tracing.yaml"), hostile).unwrap();

    let described = call_tool(
        &server,
        AGENT_TOKEN,
        "describe",
        &serde_json::json!({ "workflow": "hostile-tracing", "inputs": {} }),
    )
    .to_string();
    let described: serde_json::Value = serde_json::from_str(&described).expect("a JSON result");
    let missing = &described["result"]["structuredContent"]["missing"][0];
    assert!(
        missing["document_description"]
            .as_str()
            .unwrap_or_default()
            .contains("PASS2MARKER"),
        "the description reaches `describe` under `document_description`: {described}"
    );
    assert!(
        !missing["prompt"]
            .as_str()
            .unwrap_or_default()
            .contains("PASS2MARKER"),
        "and never inside willikins' own prompt (trust boundary 4): {described}"
    );

    let _ = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "hostile-tracing",
        &serde_json::json!({ "slug": "third-thoughts" }),
    );
    let _ = call_tool(
        &server,
        AGENT_TOKEN,
        "list_workflows",
        &serde_json::json!({}),
    );
    std::thread::sleep(Duration::from_millis(300));

    let stderr = server.stderr_text();
    // Same liveness check as the secret sweep: an empty capture would
    // satisfy every assertion below. Added by adversarial pass 2's
    // completeness critic, 2026-09-15.
    assert!(
        stderr.contains("willikins-server listening"),
        "the stderr capture must be live for this sweep to mean anything: {stderr:?}"
    );
    assert!(
        !stderr.contains("PASS2MARKER"),
        "document text reached the server's own log output:\n{stderr}"
    );
    assert!(
        !stderr.contains(ESCAPE_CHARACTER),
        "an ANSI escape reached the server's own log output:\n{stderr:?}"
    );
    for line in stderr.lines().filter(|line| !line.trim().is_empty()) {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|error| panic!("a non-JSON tracing line ({error}): {line}"));
    }
}

/// ASCII ESC, named rather than written, so no source file in this
/// repository carries a raw control character.
const ESCAPE_CHARACTER: char = '\u{1b}';

/// An input value the *caller* supplies, rejected by its domain type, is
/// quoted back bounded and escaped -- never raw -- so a 10 KiB or
/// control-character-bearing value cannot ride an error message out to
/// the agent or into a log.
#[test]
fn a_rejected_input_value_is_quoted_back_bounded_and_escaped() {
    let server = start_server(&ServerOptions::new());
    let hostile = format!("{}\u{1b}[31m\n\"", "A".repeat(10_000));
    let error = plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "new-rust-service",
        &serde_json::json!({ "slug": hostile, "org": "lightless-labs" }),
    )
    .expect_err("a 10 KiB slug is not a ProjectSlug");
    let text = error.to_string();
    assert!(
        text.len() < 4_000,
        "the refusal must not echo the whole value: {} bytes",
        text.len()
    );
    assert!(
        !text.contains(&"A".repeat(200)),
        "the refusal must bound what it quotes"
    );
    assert!(
        !text.contains('\u{1b}'),
        "the refusal must not carry a raw escape"
    );
}

/// A `WILLIKINS_WORKFLOWS_DIR` that is *itself* a symlink is followed,
/// and the server starts and serves the documents behind it.
///
/// Recorded, not changed. The refusal that exists -- `scan_directory`'s
/// and `load_named_document`'s -- is about an *entry inside* the trusted
/// directory, which is content a workflow document's author could
/// introduce. The directory itself is named by an environment variable
/// only the operator sets, on the host the operator controls, alongside
/// the two provider credentials; refusing a symlinked path there would
/// break the ordinary deployment shapes (a mounted volume, a
/// `current -> release-N` layout) to defend against someone who already
/// chooses the value. Pinned so the asymmetry is deliberate and on the
/// record rather than discovered later.
#[cfg(unix)]
#[test]
fn a_symlinked_workflows_directory_is_followed_and_recorded_as_operator_level() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let real = dir.path().join("real-workflows");
    std::fs::create_dir_all(&real).unwrap();
    copy_fixture_as(&real, "new-rust-service.yaml", "new-rust-service.yaml");
    let link = dir.path().join("linked-workflows");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let server = start_server_at(&link, &dir.path().join("journal.jsonl"));
    let listed = call_tool(
        &server,
        AGENT_TOKEN,
        "list_workflows",
        &serde_json::json!({}),
    );
    assert_ne!(
        listed["result"]["isError"].as_bool(),
        Some(true),
        "a symlinked trusted directory is followed: {listed}"
    );
    plan_over_mcp(
        &server,
        AGENT_TOKEN,
        "new-rust-service",
        &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
    )
    .expect("and its documents plan");
}

/// A plan applied in one process cannot be applied again in the next.
///
/// `PlanRecord::applied` is folded from the journal's own `RunStarted`,
/// not held in memory, so the single-apply rule survives the process --
/// which is the shape that matters, since the whole point of a 24-hour
/// approval window is that the process will be replaced inside it. Over
/// TCP, through two child processes, against the same journal file.
#[test]
fn a_plan_applied_before_a_restart_cannot_be_applied_again_after_one() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workflows_dir = dir.path().join("workflows");
    std::fs::create_dir_all(&workflows_dir).unwrap();
    copy_fixture_as(
        &workflows_dir,
        "new-rust-service.yaml",
        "new-rust-service.yaml",
    );
    let journal_path = dir.path().join("journal.jsonl");

    let plan_id = {
        let first = start_server_at(&workflows_dir, &journal_path);
        let plan_id = plan_over_mcp(
            &first,
            AGENT_TOKEN,
            "new-rust-service",
            &serde_json::json!({ "slug": "third-thoughts", "org": "lightless-labs" }),
        )
        .expect("the positive fixture plans");
        let applied = call_tool(
            &first,
            AGENT_TOKEN,
            "apply",
            &serde_json::json!({ "plan_id": plan_id }),
        );
        let run_id = applied["result"]["structuredContent"]["run_id"]
            .as_str()
            .unwrap_or_else(|| panic!("apply must start a run: {applied}"))
            .to_string();
        // Let the run reach a final state before the process is killed,
        // so the journal is not merely mid-run.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let status = call_tool(
                &first,
                AGENT_TOKEN,
                "run_status",
                &serde_json::json!({ "run_id": run_id }),
            );
            let state = status["result"]["structuredContent"]["state"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if state != "running" || std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        plan_id
    };

    let second = start_server_at(&workflows_dir, &journal_path);
    let again = call_tool(
        &second,
        AGENT_TOKEN,
        "apply",
        &serde_json::json!({ "plan_id": plan_id }),
    );
    assert_eq!(
        again["result"]["structuredContent"]["kind"], "AlreadyApplied",
        "a plan applied before the restart must not apply again: {again}"
    );
    assert!(second.wait_for_journal_event(|event| matches!(
        event,
        willikins_journal::Event::ApplyRefused {
            reason: willikins_journal::ApplyRefusedReason::AlreadyApplied,
            ..
        }
    )));
}
