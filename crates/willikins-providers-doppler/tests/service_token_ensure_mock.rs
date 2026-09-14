//! Acceptance test 2's share for `doppler.service_token.ensure`. This
//! tool's port table gives it no `Foreign` state: any listed token by
//! that name is ours (Doppler has no per-token ownership marker; the
//! plan's own ownership rule says tokens under an owned config are ours).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerServiceTokenEnsure};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{DomainType, DopplerConfig, DopplerTokenName};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "doppler", name)
}

#[derive(Default)]
struct RecordingSleeper {
    durations: Mutex<Vec<Duration>>,
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.durations.lock().expect("not poisoned").push(duration);
    }
}

fn client_against(url: String) -> (Arc<DopplerClient>, Arc<RecordingSleeper>) {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let sleeper = Arc::new(RecordingSleeper::default());
    let http = Http::new(url, Vec::new(), credential).with_sleeper(sleeper.clone());
    (Arc::new(DopplerClient::new(http)), sleeper)
}

fn config() -> DopplerConfig {
    DopplerConfig::parse("third-thoughts/prd").unwrap()
}

fn name() -> DopplerTokenName {
    DopplerTokenName::parse("ci").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs
}

const LIST_PATH: &str = "/v3/configs/config/tokens?project=third-thoughts&config=prd";
const CREATE_PATH: &str = "/v3/configs/config/tokens";

fn token_value(outputs: &willikins_core::Outputs) -> Value {
    outputs
        .get(&PortName::parse("token").unwrap())
        .unwrap()
        .clone()
}

#[test]
fn read_reports_absent_with_an_unknown_token_when_empty() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    assert!(!token_value(&predicted).is_known());
}

#[test]
fn read_reports_present_with_an_unknown_token_when_listed() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    assert!(!token_value(&outputs).is_known());
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock("GET", LIST_PATH)
        .with_status(503)
        .with_body(format!(r#"{{"messages":["{long}"]}}"#))
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}

#[test]
fn read_is_retried_on_a_429_with_retry_after_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock("GET", LIST_PATH)
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    first.assert();
    second.assert();
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
    let client = Arc::new(DopplerClient::new(http));
    let tool = DopplerServiceTokenEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_token_makes_no_post_and_stays_unknown() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let post = provider.mock("POST", CREATE_PATH).expect(0).create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    assert!(!token_value(&ensured.outputs).is_known());
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_an_absent_token_creates_it_and_asserts_the_request_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts",
            "config": "prd",
            "name": "ci",
            "access": "read",
        })))
        .with_status(200)
        .with_body(fixture("service_token_post_created").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    assert!(token_value(&ensured.outputs).is_known());
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

/// A `key` that fails [`willikins_types::DopplerServiceToken`]'s pattern
/// is a `ToolError` that echoes nothing: it fails inside `Http::finish`'s
/// body-parse step, which names only a line/column position, never
/// response text.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_malformed_key_in_the_response_is_a_provider_error_that_echoes_nothing() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let malformed_key = "dp.st.tooshort";
    provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(
            serde_json::json!({"token": {"name": "ci", "slug": "abc", "key": malformed_key}})
                .to_string(),
        )
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(
        !err.message.contains(malformed_key) && !err.message.contains("dp.st"),
        "{}",
        err.message
    );
}

/// Every other way a create response can fail to yield a token: the
/// `key` missing entirely, empty, `null`, or not a string at all. All
/// four are `Provider` errors that echo nothing — the numeric one
/// matters most, because `serde_json`'s own `Display` for a type
/// mismatch quotes the offending value verbatim, and `Http::finish`
/// throws that text away in favour of a line/column position.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_key_that_is_missing_empty_null_or_not_a_string_errors_and_echoes_nothing() {
    for token in [
        serde_json::json!({"name": "ci", "slug": "abc"}),
        serde_json::json!({"name": "ci", "slug": "abc", "key": ""}),
        serde_json::json!({"name": "ci", "slug": "abc", "key": null}),
        serde_json::json!({"name": "ci", "slug": "abc", "key": 123}),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", LIST_PATH)
            .with_status(200)
            .with_body(fixture("service_tokens_list_absent").to_string())
            .create();
        provider
            .mock("POST", CREATE_PATH)
            .with_status(200)
            .with_body(serde_json::json!({"token": token}).to_string())
            .create();
        let (client, _sleeper) = client_against(provider.url());
        let tool = DopplerServiceTokenEnsure::new(client);
        let sink = SinkToken::new();
        let err = tool
            .ensure(&inputs(), &sink)
            .err()
            .unwrap_or_else(|| panic!("token {token} must not yield a minted token"));
        assert_eq!(err.kind, ToolErrorKind::Provider, "token {token}");
        assert!(
            !err.message.contains("123") && !err.message.contains("key"),
            "token {token}: the error echoed the response: {}",
            err.message
        );
    }
}

/// `Present` means *no call at all* beyond the listing `GET`: not the
/// mint, and not a stray `DELETE` either (the sibling `rotate` tool's
/// `DELETE` path must never be reachable from `ensure`).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_token_makes_no_call_but_the_listing_get() {
    let mut provider = MockProvider::start();
    let list = provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .expect(1)
        .create();
    let post = provider.mock("POST", CREATE_PATH).expect(0).create();
    let delete = provider
        .mock("DELETE", "/v3/configs/config/tokens/token")
        .expect(0)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let sink = SinkToken::new();
    tool.ensure(&inputs(), &sink).unwrap();
    list.assert();
    post.assert();
    delete.assert();
}
