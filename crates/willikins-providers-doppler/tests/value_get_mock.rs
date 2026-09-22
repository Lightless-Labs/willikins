//! `doppler.value.get`'s mock-server share, mirroring
//! `secret_get_mock.rs` almost line for line -- same endpoint, same
//! missing-parent tolerance, same fixtures (Doppler's wire response
//! carries no secret/non-secret distinction at all; only the Rust type
//! this client parses `value.computed` into differs, which is the whole
//! point this tool's module doc makes). What differs is spelled out at
//! each test that differs.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerValueGet};
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{DomainType, DopplerConfig, SecretName};

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

fn name() -> SecretName {
    SecretName::parse("DATABASE").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs
}

const SECRET_PATH: &str =
    "/v3/configs/config/secret?project=third-thoughts&config=prd&name=DATABASE";

#[test]
fn read_returns_computed_as_plain_known_text() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerValueGet::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
    assert!(value.is_known());
    // Not redacted: unlike `doppler.secret.get`'s identical fixture, this
    // tool's whole reason for existing is that the value renders plainly.
    let computed = fixture("secret_get")["value"]["computed"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(value.render().to_string(), computed);
}

#[test]
fn read_of_a_missing_variable_is_not_found_and_never_names_a_value() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerValueGet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert!(err.message.contains("third-thoughts/prd"));
    assert!(err.message.contains("DATABASE"));
}

/// See `secret_get_mock.rs`'s twin test for the full missing-parent
/// reasoning: identical here, over `Text` instead of `DopplerSecretValue`.
#[test]
fn read_of_a_variable_whose_project_answers_400_naming_no_access_is_also_not_found() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerValueGet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert!(err.message.contains("third-thoughts/prd"));
    assert!(err.message.contains("DATABASE"));
    assert!(
        !err.message.contains("access") && !err.message.contains("token"),
        "the message must echo nothing from the provider: {}",
        err.message
    );
}

#[test]
fn a_200_with_a_null_computed_is_not_found() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get_absent").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerValueGet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert!(err.message.contains("third-thoughts/prd"));
    assert!(err.message.contains("DATABASE"));
}

/// The one genuine divergence from `doppler.secret.get`'s own behaviour,
/// and the reason it belongs in this crate's own test file rather than
/// only in `secret_get_mock.rs`'s shadow: `DopplerSecretValue::parse`
/// refuses an empty string (`min_len = 1`), so `doppler.secret.get`
/// treats `computed: ""` as a malformed response (`Provider`, see
/// `secret_get_mock.rs`'s `a_computed_that_is_missing_null_empty_or_not_a_string_names_the_key_and_echoes_nothing`).
/// `Text` carries no such floor, so `doppler.value.get` reports the
/// identical body `Present` with an empty value -- a real Doppler
/// variable set to the empty string is not the same fact as one that was
/// never set, and this tool is allowed to tell them apart precisely
/// because it does not have to reject empty as unparseable the way a
/// secret type does.
#[test]
fn a_computed_empty_string_is_present_with_empty_text_not_not_found() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(serde_json::json!({"name": "DATABASE", "value": {"computed": ""}}).to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerValueGet::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
    assert_eq!(value.render().to_string(), "");
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock("GET", SECRET_PATH)
        .with_status(503)
        .with_body(format!(r#"{{"messages":["{long}"]}}"#))
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerValueGet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}

#[test]
fn read_is_retried_on_a_429_with_retry_after_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock("GET", SECRET_PATH)
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerValueGet::new(client);
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
    let tool = DopplerValueGet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_is_identity_and_never_changed() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerValueGet::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
}
