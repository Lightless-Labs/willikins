//! Acceptance test 4's share for this crate: a credential marker and a
//! secret marker appear in no fixture, no recorded request except the
//! `Authorization` header, and no error. The plaintext-never-leaks half
//! (`ensure` never places the sealed secret's bytes in a `ToolError`) is
//! `actions_secret_ensure_mock.rs`'s
//! `ensure_never_leaks_the_marker_in_an_error`; the ciphertext-never-
//! contains-the-plaintext half is `seal.rs`'s own tests. This file covers
//! the two remaining claims: no authored fixture carries either marker,
//! and a credential built from a distinctive marker never reaches a
//! recorded request anywhere but the `Authorization` header it belongs
//! in.

use std::path::Path;
use std::sync::{Arc, Mutex};

use willikins_core::{PortName, Tool, Value};
use willikins_providers_github::{GitHubClient, GitHubRepoEnsure, GitHubRepoGet};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, GitHubRepo, GitHubToken, RepoVisibility};

/// Stands in for a real GitHub token: shaped like one (so nothing in the
/// pipeline could reject it before the point this test cares about) but
/// distinctive enough that its presence anywhere but the `Authorization`
/// header is unambiguously a bug. `concat!`-joined so this file holds no
/// literal spelling the whole thing contiguously.
const CREDENTIAL_MARKER: &str = concat!("ghp_", "wlknCredentialMarker00000000000000000");

/// The bound-`token`-port sibling of [`CREDENTIAL_MARKER`]: milestone 3e
/// task R2's optional `token` port takes an entirely separate code path
/// (`ScopedClient::Bound`, never the default client's own [`Credential`]),
/// so it earns its own marker rather than reusing one that only ever
/// exercised the default path. `concat!`-joined for the same reason.
const PORT_TOKEN_MARKER: &str = concat!("ghp_", "wlknPortTokenMarker00000000000000000");

/// Stands in for a real Doppler service token this crate would seal and
/// send to GitHub's Actions secret endpoint. Same reason as
/// [`CREDENTIAL_MARKER`] for the `concat!` split.
const SECRET_MARKER: &str = concat!(
    "dp.st.prd.",
    "wlknSecretMarker00000000000000000000000000wlkn"
);

#[test]
fn no_authored_fixture_carries_either_marker() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/github");
    for entry in std::fs::read_dir(&fixtures_dir).expect("fixtures/github/ is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() || path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        assert!(
            !text.contains(CREDENTIAL_MARKER),
            "{} carries the credential marker",
            path.display()
        );
        assert!(
            !text.contains(SECRET_MARKER),
            "{} carries the secret marker",
            path.display()
        );
        assert!(
            !text.contains(PORT_TOKEN_MARKER),
            "{} carries the bound-token-port marker",
            path.display()
        );
    }
}

#[test]
fn a_marker_credential_reaches_no_header_but_authorization() {
    let mut provider = MockProvider::start();
    let captured_headers = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let capture = captured_headers.clone();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body_from_request(move |request| {
            let headers = request
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        value.to_str().unwrap_or("<non-utf8>").to_string(),
                    )
                })
                .collect();
            *capture.lock().expect("not poisoned") = headers;
            br#"{"visibility":"private","topics":["managed-by-willikins"]}"#.to_vec()
        })
        .create();

    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", CREDENTIAL_MARKER);
    let http = Http::new(
        provider.url(),
        willikins_providers_github::default_headers(),
        credential,
    );
    let client = Arc::new(GitHubClient::new(http));
    let tool = GitHubRepoEnsure::new(client);

    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("repo").unwrap(),
        Value::known(GitHubRepo::parse("acme/widget").unwrap()),
    );
    inputs.insert(
        PortName::parse("visibility").unwrap(),
        Value::known(RepoVisibility::Private),
    );
    tool.read(&inputs).expect("the mock answers 200");

    let headers = captured_headers.lock().expect("not poisoned").clone();
    assert!(
        !headers.is_empty(),
        "the mock handler ran and captured headers"
    );
    let mut saw_authorization_with_marker = false;
    for (name, value) in &headers {
        if name.eq_ignore_ascii_case("authorization") {
            saw_authorization_with_marker |= value.contains(CREDENTIAL_MARKER);
        } else {
            assert!(
                !value.contains(CREDENTIAL_MARKER),
                "header `{name}` leaked the credential marker: {value}"
            );
        }
    }
    assert!(
        saw_authorization_with_marker,
        "the Authorization header should carry the marker"
    );
}

/// The credential marker reaches no part of a recorded request other than
/// the `Authorization` header — not the path, not the query string, not
/// the body — and no `Observation`, `Ensured` or `ToolError` the two
/// tools produce carries it either, across a whole create-then-topic
/// `ensure` and a failing one.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_marker_credential_reaches_no_request_line_observation_ensured_or_error() {
    let mut provider = MockProvider::start();
    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));

    for (method, path, status, body) in [
        ("GET", "/repos/acme/widget", 404, String::new()),
        (
            "POST",
            "/orgs/acme/repos",
            201,
            r#"{"visibility":"private","topics":[]}"#.to_string(),
        ),
        (
            "PUT",
            "/repos/acme/widget/topics",
            200,
            r#"{"names":["managed-by-willikins"]}"#.to_string(),
        ),
    ] {
        let capture = recorded.clone();
        provider
            .mock(method, path)
            .with_status(status)
            .with_body_from_request(move |request| {
                let mut seen = capture.lock().expect("not poisoned");
                seen.push(request.path_and_query().to_string());
                seen.push(
                    String::from_utf8_lossy(&request.body().cloned().unwrap_or_default())
                        .into_owned(),
                );
                body.clone().into_bytes()
            })
            .create();
    }

    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", CREDENTIAL_MARKER);
    let http = Http::new(
        provider.url(),
        willikins_providers_github::default_headers(),
        credential,
    );
    let tool = GitHubRepoEnsure::new(Arc::new(GitHubClient::new(http)));

    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("repo").unwrap(),
        Value::known(GitHubRepo::parse("acme/widget").unwrap()),
    );
    inputs.insert(
        PortName::parse("visibility").unwrap(),
        Value::known(RepoVisibility::Private),
    );

    let observation = tool.read(&inputs).expect("reads");
    let token = willikins_core::SinkToken::new();
    let ensured = tool.ensure(&inputs, &token).expect("ensures");

    // And one failing call, so a `ToolError` is in the sweep too.
    let mut failing = MockProvider::start();
    failing
        .mock("GET", "/repos/acme/widget")
        .with_status(500)
        .with_body(r#"{"message":"boom"}"#)
        .create();
    let failing_http = Http::new(
        failing.url(),
        willikins_providers_github::default_headers(),
        Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN_2", CREDENTIAL_MARKER),
    );
    let err = GitHubRepoEnsure::new(Arc::new(GitHubClient::new(failing_http)))
        .read(&inputs)
        .expect_err("500");

    let recorded = recorded.lock().expect("not poisoned").clone();
    assert!(!recorded.is_empty(), "the handlers ran");
    for text in recorded.iter().cloned().chain([
        format!("{observation:?}"),
        format!("{ensured:?}"),
        format!("{err:?}"),
        err.message.clone(),
    ]) {
        assert!(
            !text.contains(CREDENTIAL_MARKER),
            "the credential marker leaked into: {text}"
        );
    }
}

/// Inputs for `github.repo.ensure` naming `acme/widget`, private, with
/// the `token` port bound to [`PORT_TOKEN_MARKER`] -- shared by both
/// bound-token-port redaction tests below.
fn marker_token_inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("repo").unwrap(),
        Value::known(GitHubRepo::parse("acme/widget").unwrap()),
    );
    inputs.insert(
        PortName::parse("visibility").unwrap(),
        Value::known(RepoVisibility::Private),
    );
    inputs.insert(
        PortName::parse("token").unwrap(),
        Value::known(GitHubToken::parse(PORT_TOKEN_MARKER).unwrap()),
    );
    inputs
}

/// Milestone 3e, task R2's own redaction proof, header half: a marker
/// carried by a document-bound `token` port (never the tool's default
/// credential) reaches no part of a recorded request but the
/// `Authorization` header -- the same guarantee
/// [`a_marker_credential_reaches_no_header_but_authorization`] proves for
/// the default-credential path, proven again for the new one
/// `ScopedClient::default_for` adds.
#[test]
fn a_bound_token_port_marker_reaches_no_header_but_authorization() {
    let mut provider = MockProvider::start();
    let captured_headers = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let capture = captured_headers.clone();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .with_body_from_request(move |request| {
            let headers = request
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        value.to_str().unwrap_or("<non-utf8>").to_string(),
                    )
                })
                .collect();
            *capture.lock().expect("not poisoned") = headers;
            br#"{"message":"Not Found"}"#.to_vec()
        })
        .create();

    // The tool's own default credential is a plain, unrelated value --
    // if the bound `token` port were ever ignored in favour of it, the
    // marker below would never appear anywhere, which the final
    // assertion below would catch.
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_unrelated");
    let http = Http::new(
        provider.url(),
        willikins_providers_github::default_headers(),
        credential,
    );
    let tool = GitHubRepoEnsure::new(Arc::new(GitHubClient::new(http)));
    tool.read(&marker_token_inputs()).ok();

    let headers = captured_headers.lock().expect("not poisoned").clone();
    assert!(
        !headers.is_empty(),
        "the mock handler ran and captured headers"
    );
    let mut saw_authorization_with_marker = false;
    for (name, value) in &headers {
        if name.eq_ignore_ascii_case("authorization") {
            saw_authorization_with_marker |= value.contains(PORT_TOKEN_MARKER);
        } else {
            assert!(
                !value.contains(PORT_TOKEN_MARKER),
                "header `{name}` leaked the bound-token-port marker: {value}"
            );
        }
    }
    assert!(
        saw_authorization_with_marker,
        "the Authorization header should carry the bound token's marker, proving it (not the \
         default credential) authorized the request"
    );
}

/// The same proof's other half: neither a `404`'s `Observation` nor a
/// genuinely failing call's `ToolError` (`Debug` or `message`) ever
/// carries the bound-token-port marker -- the same two-source shape
/// [`a_marker_credential_reaches_no_request_line_observation_ensured_or_error`]
/// uses for the default-credential path.
#[test]
fn a_bound_token_port_marker_reaches_no_observation_or_error() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .with_body(r#"{"message":"Not Found"}"#)
        .create();
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_unrelated");
    let http = Http::new(
        provider.url(),
        willikins_providers_github::default_headers(),
        credential,
    );
    let observation = GitHubRepoEnsure::new(Arc::new(GitHubClient::new(http)))
        .read(&marker_token_inputs())
        .expect("a 404 is Absent, not an error");

    let mut failing = MockProvider::start();
    failing
        .mock("GET", "/repos/acme/widget")
        .with_status(500)
        .with_body(r#"{"message":"boom"}"#)
        .create();
    let failing_credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN_2", "ghp_other");
    let failing_http = Http::new(
        failing.url(),
        willikins_providers_github::default_headers(),
        failing_credential,
    );
    let err = GitHubRepoEnsure::new(Arc::new(GitHubClient::new(failing_http)))
        .read(&marker_token_inputs())
        .expect_err("500");

    for text in [
        format!("{observation:?}"),
        format!("{err:?}"),
        err.message.clone(),
    ] {
        assert!(
            !text.contains(PORT_TOKEN_MARKER),
            "the bound-token-port marker leaked into: {text}"
        );
    }
}

/// Adversarial pass 4 (2026-09-29): the two bound-token proofs above drive
/// `github.repo.ensure` only, but the one GitHub tool the Sample document
/// binds `token` on is `github.repo.get` -- a separate `lookup` with its
/// own error path. A mutation that wrote the bound token into that
/// path's `ToolError` message left every test in this crate green. Every
/// error `github.repo.get` can return for a request it authorized with a
/// bound token -- a `404` (`NotFound`), a `401` (`Provider`) and a failing
/// `500` (`Provider`) -- must carry the marker in neither its `Debug` nor
/// its `message`.
#[test]
fn a_bound_token_port_marker_reaches_no_repo_get_error() {
    for (status, body) in [
        (404, r#"{"message":"Not Found"}"#),
        (401, r#"{"message":"Bad credentials"}"#),
        (500, r#"{"message":"boom"}"#),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/repos/acme/widget")
            .with_status(status)
            .with_body(body)
            .create();
        let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_unrelated");
        let http = Http::new(
            provider.url(),
            willikins_providers_github::default_headers(),
            credential,
        );
        let mut inputs = willikins_core::Inputs::new();
        inputs.insert(
            PortName::parse("repo").unwrap(),
            Value::known(GitHubRepo::parse("acme/widget").unwrap()),
        );
        inputs.insert(
            PortName::parse("token").unwrap(),
            Value::known(GitHubToken::parse(PORT_TOKEN_MARKER).unwrap()),
        );
        let err = GitHubRepoGet::new(Arc::new(GitHubClient::new(http)))
            .read(&inputs)
            .expect_err("every one of these statuses is an error for github.repo.get");
        for text in [format!("{err:?}"), err.message.clone()] {
            assert!(
                !text.contains(PORT_TOKEN_MARKER),
                "a {status}: the bound-token-port marker leaked into: {text}"
            );
        }
    }
}
