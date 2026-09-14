//! What a Doppler `429` actually costs an apply.
//!
//! Doppler answers a rate limit with `retry-after` in **seconds**
//! (research note section 3, "Doppler rate limits, errors, and platform
//! limits"), and the plan says the retry policy honours it. Every mock
//! test in this crate builds a `RecordingSleeper` and then drops it into
//! `_sleeper` without ever reading it back, so until this file nothing
//! checked that the header reached the sleep at all — a client that
//! ignored `retry-after` entirely and simply backed off exponentially
//! would have passed every one of them.
//!
//! `willikins-providers-http`'s own `tests/adversarial_retry.rs` pins the
//! cap on a bare `Http`. What this file pins is the same guarantee
//! reached *through a Doppler tool*, on the exact endpoints the five
//! tools call.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, Tool, Value};
use willikins_providers_doppler::{DopplerClient, DopplerProjectEnsure, DopplerServiceTokenEnsure};
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_providers_http::{Credential, Http, MAX_RETRY_AFTER, Sleeper};
use willikins_types::{DomainType, DopplerConfig, DopplerProject, DopplerTokenName};

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

impl RecordingSleeper {
    fn recorded(&self) -> Vec<Duration> {
        self.durations.lock().expect("not poisoned").clone()
    }
}

fn client_against(url: String) -> (Arc<DopplerClient>, Arc<RecordingSleeper>) {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let sleeper = Arc::new(RecordingSleeper::default());
    let http = Http::new(url, Vec::new(), credential).with_sleeper(sleeper.clone());
    (Arc::new(DopplerClient::new(http)), sleeper)
}

const PROJECT_PATH: &str = "/v3/projects/project?project=third-thoughts";
const LIST_PATH: &str = "/v3/configs/config/tokens?project=third-thoughts&config=prd";

fn project_inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("project").unwrap(),
        Value::known(DopplerProject::parse("third-thoughts").unwrap()),
    );
    inputs
}

fn token_inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("config").unwrap(),
        Value::known(DopplerConfig::parse("third-thoughts/prd").unwrap()),
    );
    inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(DopplerTokenName::parse("ci").unwrap()),
    );
    inputs
}

/// A plain-seconds `retry-after` on a `429` is the wait — exactly, not a
/// backoff that happens to be in the same order of magnitude.
#[test]
fn a_retry_after_in_seconds_is_the_wait_exactly() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", PROJECT_PATH)
        .with_status(429)
        .with_header("Retry-After", "7")
        .with_body("{}")
        .expect(1)
        .create();
    provider
        .mock("GET", PROJECT_PATH)
        .with_status(200)
        .with_body(fixture("project_get_present").to_string())
        .expect(1)
        .create();
    let (client, sleeper) = client_against(provider.url());
    let observation = DopplerProjectEnsure::new(client)
        .read(&project_inputs())
        .unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    assert_eq!(sleeper.recorded(), vec![Duration::from_secs(7)]);
}

/// A `retry-after` Doppler could answer but willikins will not obey: a
/// day. The wait is the cap, not the header, so an apply never blocks on
/// a number the provider alone chooses.
#[test]
fn an_enormous_retry_after_waits_the_cap_instead() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(429)
        .with_header("Retry-After", "86400")
        .with_body("{}")
        .expect(1)
        .create();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .expect(1)
        .create();
    let (client, sleeper) = client_against(provider.url());
    let observation = DopplerServiceTokenEnsure::new(client)
        .read(&token_inputs())
        .unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    assert_eq!(sleeper.recorded(), vec![MAX_RETRY_AFTER]);
    assert_eq!(MAX_RETRY_AFTER, Duration::from_secs(60));
}

/// A `429` that never lets up costs three waits and four attempts, each
/// wait the honoured header — the whole budget, bounded.
#[test]
fn a_persistent_429_costs_three_honoured_waits_and_then_fails() {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", PROJECT_PATH)
        .with_status(429)
        .with_header("Retry-After", "2")
        .with_body(r#"{"messages":["Too many requests."]}"#)
        .expect(4)
        .create();
    let (client, sleeper) = client_against(provider.url());
    let err = DopplerProjectEnsure::new(client)
        .read(&project_inputs())
        .unwrap_err();
    assert_eq!(err.kind, willikins_core::ToolErrorKind::Provider);
    assert_eq!(
        sleeper.recorded(),
        vec![Duration::from_secs(2); 3],
        "three waits, each the honoured header"
    );
    mock.assert();
}
