//! Mock-server tests for `signoz.ingestion_key.ensure`: every arm the
//! research names, plus the `409` belt-and-braces path and the decision
//! that the list response's `value` field never reaches an output.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Sleeper};
use willikins_providers_signoz::{SigNozClient, SigNozIngestionKeyEnsure};
use willikins_types::{DomainType, SigNozIngestionKeyName};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "signoz", name)
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

fn client_against(url: String) -> (Arc<SigNozClient>, Arc<RecordingSleeper>) {
    let credential = Credential::for_testing(
        "WILLIKINS_TEST_SIGNOZ_API_KEY",
        "test-signoz-api-key-000000",
    );
    let sleeper = Arc::new(RecordingSleeper::default());
    let http =
        willikins_providers_signoz::http_client(url, credential).with_sleeper(sleeper.clone());
    (Arc::new(SigNozClient::new(http)), sleeper)
}

fn name() -> SigNozIngestionKeyName {
    SigNozIngestionKeyName::parse("willikins-example-key").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs
}

const LIST_PATH: &str = "/api/v2/gateway/ingestion_keys";
const CREATE_PATH: &str = "/api/v2/gateway/ingestion_keys";

fn key_value(outputs: &willikins_core::Outputs) -> Value {
    outputs
        .get(&PortName::parse("key").unwrap())
        .unwrap()
        .clone()
}

#[test]
fn read_reports_absent_with_an_unknown_key_when_empty() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_absent").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    assert!(!key_value(&predicted).is_known());
}

#[test]
fn read_reports_present_with_an_unknown_key_when_listed() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    // The decision this tool exists to make concrete: the list response
    // carries `value` (a real, minted secret for an existing key), and
    // this tool never surfaces it. `ingestion_keys_list_present.json`'s
    // `value` is a distinctive marker precisely so this assertion is
    // meaningful, not vacuous.
    assert!(!key_value(&outputs).is_known());
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock("GET", LIST_PATH)
        .with_status(503)
        .with_body(format!(r#"{{"error":{{"message":"{long}"}}}}"#))
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
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
        .with_body(fixture("ingestion_keys_list_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    first.assert();
    second.assert();
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    let credential = Credential::for_testing(
        "WILLIKINS_TEST_SIGNOZ_API_KEY",
        "test-signoz-api-key-000000",
    );
    let http = willikins_providers_signoz::http_client("http://127.0.0.1:1", credential);
    let client = Arc::new(SigNozClient::new(http));
    let tool = SigNozIngestionKeyEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_key_makes_no_post_and_stays_unknown() {
    let mut provider = MockProvider::start();
    let list = provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_present").to_string())
        .expect(1)
        .create();
    let post = provider.mock("POST", CREATE_PATH).expect(0).create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    assert!(!key_value(&ensured.outputs).is_known());
    list.assert();
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_an_absent_key_creates_it_and_asserts_the_request_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_absent").to_string())
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .match_body(json_body(serde_json::json!({
            "name": "willikins-example-key",
        })))
        .with_status(201)
        .with_body(fixture("ingestion_key_post_created").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    assert!(key_value(&ensured.outputs).is_known());
    create.assert();
}

/// The belt-and-braces path: this call's own listing found nothing, the
/// create raced against a concurrent mint and lost (`409`), and a
/// re-list now finds `name` — so this call converges rather than
/// failing.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_409_re_lists_and_converges_when_now_present() {
    let mut provider = MockProvider::start();
    let first_list = provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_absent").to_string())
        .expect(1)
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(409)
        .with_body(fixture("error_409_already_exists").to_string())
        .expect(1)
        .create();
    let second_list = provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    assert!(!key_value(&ensured.outputs).is_known());
    first_list.assert();
    create.assert();
    second_list.assert();
}

/// Where the belt-and-braces path stops: a `409` whose re-list still does
/// not show `name` is not explained by a race, so it propagates as the
/// `Conflict` `willikins_providers_http::error::ProviderError`'s generic
/// `409` mapping already gives it.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_409_that_a_re_list_does_not_explain_still_fails() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_absent").to_string())
        .create();
    provider
        .mock("POST", CREATE_PATH)
        .with_status(409)
        .with_body(fixture("error_409_already_exists").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_absent").to_string())
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(503)
        .with_body(fixture("error_5xx").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

#[test]
fn a_403_is_the_fixed_missing_permission_message_and_echoes_no_scope_text() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(403)
        .with_body(fixture("error_403_missing_scope").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(
        !err.message.contains("ingestion-key:create"),
        "{}",
        err.message
    );
}

/// A `value` that fails [`willikins_types::SigNozIngestionKeyValue`]'s
/// parse (empty, over length) is a `ToolError` that echoes nothing: it
/// fails inside `Http::finish`'s body-parse step, which names only a
/// line/column position, never response text.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_malformed_value_in_the_response_is_a_provider_error_that_echoes_nothing() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_absent").to_string())
        .create();
    let marker = "wlkn-empty-value-marker-9gk3";
    provider
        .mock("POST", CREATE_PATH)
        .with_status(201)
        .with_body(
            serde_json::json!({"status": "success", "data": {"id": marker, "value": ""}})
                .to_string(),
        )
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(!err.message.contains(marker), "{}", err.message);
}
