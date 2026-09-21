//! Mock-server tests for `doppler.secret.set`, the first tool in this
//! crate that writes a secret. `read`'s existence-by-name shape (not
//! `doppler.service_token.rotate`'s "always `Absent`") is this tool's own
//! module docs' central point; the tests here pin it directly.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerSecretSet};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{DomainType, DopplerConfig, DopplerSecretValue, SecretName};

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
    SecretName::parse("MIRRORED_URL").unwrap()
}

/// A distinctive marker for the value this crate writes: if it ever
/// shows up outside the request body this test itself asserts on, a
/// redaction rule broke.
const VALUE_MARKER: &str = "wlkn-secret-set-marker-4qz9";

fn value() -> DopplerSecretValue {
    DopplerSecretValue::parse(VALUE_MARKER).unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs.insert(PortName::parse("value").unwrap(), Value::known(value()));
    inputs
}

const SECRET_PATH: &str =
    "/v3/configs/config/secret?project=third-thoughts&config=prd&name=MIRRORED_URL";
const SECRETS_PATH: &str = "/v3/configs/config/secrets";

#[test]
fn read_reports_absent_when_the_secret_does_not_exist() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get_absent").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);
    assert!(matches!(
        tool.read(&inputs()).unwrap(),
        Observation::Absent { .. }
    ));
}

#[test]
fn read_reports_present_when_the_secret_already_exists() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// The provenance rule forces `config` to come from a `doppler.config.ensure`
/// node in the same graph, so the common case at plan time is a config
/// that does not exist *yet* -- tolerated the same way every other read
/// in this crate tolerates a missing parent.
#[test]
fn read_reports_absent_when_the_parent_project_or_config_does_not_exist_yet() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);
    assert!(matches!(
        tool.read(&inputs()).unwrap(),
        Observation::Absent { .. }
    ));
}

#[test]
fn read_reports_absent_when_the_parent_answers_400_naming_no_access() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);
    assert!(matches!(
        tool.read(&inputs()).unwrap(),
        Observation::Absent { .. }
    ));
}

#[test]
fn read_still_propagates_a_400_with_an_unrelated_message() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(400)
        .with_body(r#"{"success": false, "messages": ["Could not find requested project."]}"#)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
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
    let tool = DopplerSecretSet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
    let client = Arc::new(DopplerClient::new(http));
    let tool = DopplerSecretSet::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_posts_the_flat_secrets_map_and_reports_changed() {
    let mut provider = MockProvider::start();
    let post = provider
        .mock("POST", SECRETS_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts",
            "config": "prd",
            "secrets": { "MIRRORED_URL": VALUE_MARKER },
        })))
        .with_status(200)
        .with_body(serde_json::json!({"secrets": {}}).to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_reports_changed_true_even_when_the_secret_already_existed() {
    let mut provider = MockProvider::start();
    provider
        .mock("POST", SECRETS_PATH)
        .with_status(200)
        .with_body(serde_json::json!({"secrets": {}}).to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(
        ensured.changed,
        "a write this tool never compares against the existing value always reports changed"
    );
}

/// This crate's one exception to `tests/redaction.rs`'s "no recorded
/// request line carries the marker" rule, by design (see that file's own
/// updated module doc): the value reaches the `POST` body, since sending
/// it is this tool's whole job, but nowhere else -- not the `read` path,
/// not an error, not `Debug` on the `Ensured` result.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn the_value_reaches_only_the_post_body_never_an_error_or_debug_rendering() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", SECRET_PATH)
        .with_status(200)
        .with_body(fixture("secret_get_absent").to_string())
        .create();
    provider
        .mock("POST", SECRETS_PATH)
        .with_status(200)
        .with_body(serde_json::json!({"secrets": {}}).to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);

    let observation = tool.read(&inputs()).unwrap();
    assert!(
        !format!("{observation:?}").contains(VALUE_MARKER),
        "read's own Observation must never carry the value"
    );

    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(
        !format!("{ensured:?}").contains(VALUE_MARKER),
        "ensure's own Ensured (empty outputs) must never carry the value"
    );

    let mut failing = MockProvider::start();
    failing
        .mock("POST", SECRETS_PATH)
        .with_status(500)
        .with_body(r#"{"messages":["boom"]}"#)
        .create();
    let (failing_client, _sleeper2) = client_against(failing.url());
    let err = DopplerSecretSet::new(failing_client)
        .ensure(&inputs(), &token)
        .unwrap_err();
    assert!(
        !err.message.contains(VALUE_MARKER),
        "a failed write must never echo the value: {}",
        err.message
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    let post = provider
        .mock("POST", SECRETS_PATH)
        .with_status(503)
        .with_body(r#"{"messages":["boom"]}"#)
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerSecretSet::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    post.assert();
}
