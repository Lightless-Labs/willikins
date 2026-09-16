//! Acceptance test 2's share for `github.actions_secret.ensure`, and
//! acceptance test 3's mock-server half (the sealed box unseals with the
//! test key pair, and its base64 form matches GitHub's documented
//! pattern).

use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use crypto_box::SecretKey;
use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_github::{GitHubActionsSecretEnsure, GitHubClient};
use willikins_providers_http::testing::{MockProvider, load_fixture, partial_json_body};
use willikins_providers_http::{Credential, Http};
use willikins_types::{ActionsSecretName, DomainType, DopplerServiceToken, GitHubRepo};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "github", name)
}

fn client_against(url: String) -> Arc<GitHubClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new(url, Vec::new(), credential);
    Arc::new(GitHubClient::new(http))
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("acme/widget").unwrap()
}

fn secret_name() -> ActionsSecretName {
    ActionsSecretName::parse("DOPPLER_TOKEN").unwrap()
}

/// The distinctive marker used across this file's tests: if it ever shows
/// up in a recorded request body or an error message, redaction broke.
const MARKER: &str = "dp.st.prd.wlkntestmarker0000000000000000000000wlkn";

fn secret_value() -> DopplerServiceToken {
    DopplerServiceToken::parse(MARKER).unwrap()
}

fn full_inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(secret_name()),
    );
    inputs.insert(
        PortName::parse("value").unwrap(),
        Value::known(secret_value()),
    );
    inputs
}

/// Also this tool's answer for "an Actions secret in a missing
/// repository" (the same audit that found the 2026-09-16
/// `doppler.service_token.ensure`/`.rotate` defect: a parent this same
/// plan is about to create must not fail a `read` at plan time). GitHub
/// answers this endpoint `404` whether the secret's repository does not
/// exist yet or the repository exists but the secret does not — the same
/// opaque conflation `github.repo.ensure`'s own docs note for `get_repo`
/// — and this tool already reads either shape as `Absent`, matching
/// `github.repo.ensure`'s ordering guarantee (by the time this tool's
/// `ensure` runs, `github.repo.ensure` has already created the
/// repository). No fix needed here; this test is what pins it.
#[test]
fn read_reports_absent_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .with_status(404)
        .create();
    let tool = GitHubActionsSecretEnsure::new(client_against(provider.url()));
    let observation = tool.read(&full_inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn read_reports_present_on_200_and_never_carries_a_value() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .with_status(200)
        .with_body(fixture("actions_secret_get_present").to_string())
        .create();
    let tool = GitHubActionsSecretEnsure::new(client_against(provider.url()));
    let observation = tool.read(&full_inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    assert!(outputs.is_empty());
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .with_status(503)
        .with_body("{}")
        .create();
    let tool = GitHubActionsSecretEnsure::new(client_against(provider.url()));
    let err = tool.read(&full_inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn read_is_retried_on_a_429_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock("GET", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock("GET", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .with_status(200)
        .with_body(fixture("actions_secret_get_present").to_string())
        .expect(1)
        .create();
    let tool = GitHubActionsSecretEnsure::new(client_against(provider.url()));
    let observation = tool.read(&full_inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    first.assert();
    second.assert();
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
    let tool = GitHubActionsSecretEnsure::new(Arc::new(GitHubClient::new(http)));
    let err = tool.read(&full_inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

/// Acceptance tests 2 and 3, end to end through the tool (not just
/// `seal.rs`'s own unit tests): the sealed PUT body's `encrypted_value` is
/// neither the plaintext nor its base64, and — captured off the real
/// request mockito received, via `with_body_from_request`'s side channel,
/// since [`Http::put_empty`] deliberately never returns a response body
/// to the caller — unseals with the test key pair to exactly the
/// plaintext. `key_id` is asserted with the http crate's JSON matcher;
/// `changed: true` on the first call.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_seals_and_stores_the_secret_and_it_unseals_to_the_plaintext() {
    let secret_key = SecretKey::generate(&mut rand_core::OsRng);
    let public_key_base64 = STANDARD.encode(secret_key.public_key().as_bytes());

    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets/public-key")
        .with_status(200)
        .with_body(
            serde_json::json!({"key_id": "test-key-id", "key": public_key_base64}).to_string(),
        )
        .create();

    let captured_request_body = Arc::new(Mutex::new(None::<Vec<u8>>));
    let capture = captured_request_body.clone();
    let put = provider
        .mock("PUT", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .match_body(partial_json_body(
            serde_json::json!({"key_id": "test-key-id"}),
        ))
        .with_status(204)
        .with_body_from_request(move |request| {
            *capture.lock().expect("not poisoned") = request.body().ok().cloned();
            Vec::new()
        })
        .expect(1)
        .create();

    let tool = GitHubActionsSecretEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&full_inputs(), &token).unwrap();
    assert!(ensured.changed);
    put.assert();

    let raw = captured_request_body
        .lock()
        .expect("not poisoned")
        .clone()
        .expect("the PUT handler ran and captured a body");
    let body: serde_json::Value = serde_json::from_slice(&raw).expect("valid JSON");
    let encrypted_value = body["encrypted_value"]
        .as_str()
        .expect("encrypted_value is a string");
    assert_ne!(encrypted_value, MARKER);
    assert_ne!(encrypted_value, STANDARD.encode(MARKER));

    let ciphertext = STANDARD
        .decode(encrypted_value)
        .expect("encrypted_value is valid base64, matching GitHub's documented pattern");
    let opened = secret_key.unseal(&ciphertext).expect("unseals");
    assert_eq!(opened, MARKER.as_bytes());
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_reports_changed_true_on_both_the_first_and_a_second_call() {
    let secret_key = SecretKey::generate(&mut rand_core::OsRng);
    let public_key_base64 = STANDARD.encode(secret_key.public_key().as_bytes());
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets/public-key")
        .with_status(200)
        .with_body(
            serde_json::json!({"key_id": "test-key-id", "key": public_key_base64}).to_string(),
        )
        .create();
    provider
        .mock("PUT", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .with_status(201)
        .create();
    let tool = GitHubActionsSecretEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let first = tool.ensure(&full_inputs(), &token).unwrap();
    assert!(first.changed);
    let second = tool.ensure(&full_inputs(), &token).unwrap();
    assert!(second.changed, "an unreadable-back sink always writes");
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_never_leaks_the_marker_in_an_error() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets/public-key")
        .with_status(503)
        .with_body("{}")
        .create();
    let tool = GitHubActionsSecretEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&full_inputs(), &token).unwrap_err();
    assert!(!err.message.contains(MARKER), "{}", err.message);
}

/// A client whose [`Http`] carries the three headers GitHub requires, so
/// a mock can assert on them (production builds this shape through
/// [`willikins_providers_github::http_client`], which pins the real base
/// URL and cannot be aimed at a mock server).
fn client_with_default_headers(url: String) -> Arc<GitHubClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new(
        url,
        willikins_providers_github::default_headers(),
        credential,
    );
    Arc::new(GitHubClient::new(http))
}

fn requiring_github_headers(mock: mockito::Mock) -> mockito::Mock {
    mock.match_header("Accept", "application/vnd.github+json")
        .match_header("X-GitHub-Api-Version", "2022-11-28")
        .match_header("User-Agent", mockito::Matcher::Regex("^willikins/".into()))
}

/// Every request `github.actions_secret.ensure` makes — the `read`'s
/// `GET`, the public-key `GET` and the secret `PUT` — carries the three
/// required headers.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn every_request_carries_the_three_required_headers() {
    let secret_key = SecretKey::generate(&mut rand_core::OsRng);
    let public_key_base64 = STANDARD.encode(secret_key.public_key().as_bytes());
    let mut provider = MockProvider::start();
    let read = requiring_github_headers(
        provider.mock("GET", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN"),
    )
    .with_status(200)
    .with_body(fixture("actions_secret_get_present").to_string())
    .expect(1)
    .create();
    let public_key = requiring_github_headers(
        provider.mock("GET", "/repos/acme/widget/actions/secrets/public-key"),
    )
    .with_status(200)
    .with_body(serde_json::json!({"key_id": "test-key-id", "key": public_key_base64}).to_string())
    .expect(1)
    .create();
    let put = requiring_github_headers(
        provider.mock("PUT", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN"),
    )
    .with_status(204)
    .expect(1)
    .create();

    let tool = GitHubActionsSecretEnsure::new(client_with_default_headers(provider.url()));
    tool.read(&full_inputs())
        .expect("the read's GET matched the required headers");
    let token = SinkToken::new();
    tool.ensure(&full_inputs(), &token)
        .expect("both of ensure's requests matched the required headers");
    read.assert();
    public_key.assert();
    put.assert();
}

/// Lowercase hex of `bytes`, so a test can assert the sealed value is not
/// the plaintext in that encoding either.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// The whole recorded `PUT` — every byte of its body, and its path —
/// carries the secret in none of the three encodings a careless
/// implementation could have leaked it in (raw, base64, hex), and neither
/// the `Observation` nor the `Ensured` the tool hands back carries it in
/// its `Debug` form.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn no_recorded_request_observation_or_ensured_carries_the_secret_marker() {
    let secret_key = SecretKey::generate(&mut rand_core::OsRng);
    let public_key_base64 = STANDARD.encode(secret_key.public_key().as_bytes());
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .with_status(200)
        .with_body(fixture("actions_secret_get_present").to_string())
        .create();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets/public-key")
        .with_status(200)
        .with_body(
            serde_json::json!({"key_id": "test-key-id", "key": public_key_base64}).to_string(),
        )
        .create();
    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = recorded.clone();
    provider
        .mock("PUT", "/repos/acme/widget/actions/secrets/DOPPLER_TOKEN")
        .with_status(204)
        .with_body_from_request(move |request| {
            let body = request.body().ok().cloned().unwrap_or_default();
            let mut seen = capture.lock().expect("not poisoned");
            seen.push(String::from_utf8_lossy(&body).into_owned());
            seen.push(request.path().to_string());
            Vec::new()
        })
        .create();

    let tool = GitHubActionsSecretEnsure::new(client_against(provider.url()));
    let observation = tool.read(&full_inputs()).expect("reads");
    let token = SinkToken::new();
    let ensured = tool.ensure(&full_inputs(), &token).expect("ensures");

    let encodings = [
        MARKER.to_string(),
        STANDARD.encode(MARKER),
        hex(MARKER.as_bytes()),
    ];
    let recorded = recorded.lock().expect("not poisoned").clone();
    assert!(!recorded.is_empty(), "the PUT handler ran");
    for text in recorded
        .iter()
        .cloned()
        .chain([format!("{observation:?}"), format!("{ensured:?}")])
    {
        for encoding in &encodings {
            assert!(
                !text.contains(encoding.as_str()),
                "the secret marker leaked (as {} bytes) into: {text}",
                encoding.len()
            );
        }
    }
}
