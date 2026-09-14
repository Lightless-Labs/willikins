//! Acceptance test 2's share for `doppler.config.ensure`. This tool's
//! port table gives it no `Foreign` state (see the crate's own docs), so
//! there is no `Foreign` case here.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerConfigEnsure};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{DomainType, DopplerProject, EnvironmentSlug};

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

fn project() -> DopplerProject {
    DopplerProject::parse("third-thoughts").unwrap()
}

fn environment() -> EnvironmentSlug {
    EnvironmentSlug::parse("prd").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
    inputs.insert(
        PortName::parse("environment").unwrap(),
        Value::known(environment()),
    );
    inputs
}

#[test]
fn read_reports_absent_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    let value = predicted
        .get(&PortName::parse("config").unwrap())
        .unwrap()
        .render()
        .to_string();
    assert_eq!(value, "third-thoughts/prd");
}

#[test]
fn read_reports_present_on_200() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(503)
        .with_body(format!(r#"{{"messages":["{long}"]}}"#))
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}

#[test]
fn read_is_retried_on_a_429_with_retry_after_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
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
    let tool = DopplerConfigEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_the_environment_and_asserts_the_request_body() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .create();
    let create = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .match_body(json_body(serde_json::json!({
            "name": "prd",
            "slug": "prd",
        })))
        .with_status(201)
        .with_body(fixture("environment_post_created").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .create();
    let create = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_error_after_create_re_reads_and_reports_unchanged_when_present() {
    let mut provider = MockProvider::start();
    let first_read = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .expect(1)
        .create();
    let create = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .with_status(500)
        .with_body("{}")
        .expect(1)
        .create();
    let second_read = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    first_read.assert();
    create.assert();
    second_read.assert();
}

/// Acceptance test 5's own claim: `doppler.project.ensure` seeded
/// `dev`/`stg`/`prd` finds them `Unchanged` — i.e. `doppler.config.ensure`
/// on an already-present config makes no `POST` at all.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_config_makes_no_post() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let post = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .expect(0)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    post.assert();
}
