//! Acceptance test 2's share for `doppler.secret.get`: pure, `ensure` is
//! identity, `raw` and `computed` deliberately differ in the fixture and
//! the tool returns `computed`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerSecretGet};
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
fn read_returns_computed_not_raw() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretGet::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
    assert!(value.is_known());
    // Redacted by construction: the point is `is_known()`, and that the
    // fixture's raw value never leaks (checked in redaction.rs).
    assert_eq!(value.render().to_string(), "[REDACTED DopplerSecretValue]");
}

#[test]
fn read_of_a_missing_secret_is_not_found_and_never_names_a_value() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretGet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert!(err.message.contains("third-thoughts/prd"));
    assert!(err.message.contains("DATABASE"));
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
    let tool = DopplerSecretGet::new(client);
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
    let tool = DopplerSecretGet::new(client);
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
    let tool = DopplerSecretGet::new(client);
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
    let tool = DopplerSecretGet::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
}

#[test]
fn known_secret_renders_redacted_in_outputs_debug_and_json() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretGet::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let debug = format!("{outputs:?}");
    assert!(debug.contains("REDACTED"), "{debug}");
    let raw = fixture("secret_get")["value"]["raw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!debug.contains(&raw), "{debug}");
    let json = serde_json::to_string(&outputs).unwrap();
    assert!(json.contains("REDACTED"), "{json}");
    assert!(!json.contains(&raw), "{json}");
}
