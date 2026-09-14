//! Acceptance test 2's share for `doppler.service_token.rotate`: `read`
//! always reports `Absent` (see the crate's own module docs), and
//! `ensure`'s `DELETE` bodies are asserted one per listed token.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerServiceTokenRotate};
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
const DELETE_PATH: &str = "/v3/configs/config/tokens/token";

fn token_value(outputs: &willikins_core::Outputs) -> Value {
    outputs
        .get(&PortName::parse("token").unwrap())
        .unwrap()
        .clone()
}

/// `read` always reports `Absent`, even when a token by that name is
/// already listed — see the crate's own module docs for why.
#[test]
fn read_always_reports_absent_even_when_a_token_is_listed() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// The listing `GET` still runs during `read`, so a bad credential or
/// config fails at plan time rather than being silently swallowed.
#[test]
fn read_still_propagates_a_listing_failure() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(503)
        .with_body("{}")
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
    let client = Arc::new(DopplerClient::new(http));
    let tool = DopplerServiceTokenRotate::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

/// `ensure` on no existing token: no `DELETE` at all, straight to mint.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_with_no_existing_token_makes_no_delete_and_mints() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let delete = provider.mock("DELETE", DELETE_PATH).expect(0).create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(fixture("service_token_post_created").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    assert!(token_value(&ensured.outputs).is_known());
    delete.assert();
    create.assert();
}

/// `ensure` on one existing token: exactly one `DELETE`, its body
/// asserted, then the mint.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_deletes_the_one_listed_token_by_slug_then_mints() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let delete = provider
        .mock("DELETE", DELETE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts",
            "config": "prd",
            "slug": "56c69f96-3045-11ea-978f-2e728ce88125",
        })))
        .with_status(200)
        .with_body(fixture("service_token_delete").to_string())
        .expect(1)
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(fixture("service_token_post_created").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    delete.assert();
    create.assert();
}

/// One `DELETE` per listed token sharing the rotated name — never a
/// single call for however many are listed.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_deletes_every_listed_token_sharing_the_name() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(
            serde_json::json!({"tokens": [
                {"name": "ci", "slug": "slug-one"},
                {"name": "ci", "slug": "slug-two"},
                {"name": "other", "slug": "slug-three"},
            ]})
            .to_string(),
        )
        .create();
    let delete_one = provider
        .mock("DELETE", DELETE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts", "config": "prd", "slug": "slug-one",
        })))
        .with_status(200)
        .with_body(r#"{"success":true}"#)
        .expect(1)
        .create();
    let delete_two = provider
        .mock("DELETE", DELETE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts", "config": "prd", "slug": "slug-two",
        })))
        .with_status(200)
        .with_body(r#"{"success":true}"#)
        .expect(1)
        .create();
    let delete_other = provider
        .mock("DELETE", DELETE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts", "config": "prd", "slug": "slug-three",
        })))
        .expect(0)
        .create();
    provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(fixture("service_token_post_created").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    tool.ensure(&inputs(), &token).unwrap();
    delete_one.assert();
    delete_two.assert();
    delete_other.assert();
}

/// If the `DELETE` fails, nothing is minted: a rotation that fails to
/// revoke the old token but still hands out a new one is the opposite of
/// a rotation.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_failed_delete_mints_nothing() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    provider
        .mock("DELETE", DELETE_PATH)
        .with_status(500)
        .with_body("{}")
        .create();
    let create = provider.mock("POST", CREATE_PATH).expect(0).create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
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
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

#[test]
fn spec_class_is_destructive() {
    let (client, _sleeper) = client_against("http://127.0.0.1:0".to_string());
    let tool = DopplerServiceTokenRotate::new(client);
    assert_eq!(tool.spec().class, willikins_core::Class::Destructive);
}
