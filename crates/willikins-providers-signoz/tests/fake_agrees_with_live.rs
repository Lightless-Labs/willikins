//! The fake `signoz.ingestion_key.ensure` agrees with the live tool
//! *behaviourally*, not only on its `ToolSpec` (`catalog_parity.rs`'s
//! own share). Mirrors
//! `willikins-providers-buildkite/tests/fake_agrees_with_live.rs` and
//! `willikins-providers-doppler/tests/fake_agrees_with_live.rs`'s own
//! method: for each observation shape, seed the fake's state and serve
//! the live tool a mock body standing for the same real world, then
//! assert both tools answer the same way (by serialized JSON equality,
//! since neither `Observation` nor `Ensured` implements `PartialEq`).

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_fake::FakeState;
use willikins_providers_http::Credential;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_providers_signoz::{SigNozClient, SigNozIngestionKeyEnsure};
use willikins_types::{DomainType, SigNozIngestionKeyName};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "signoz", name)
}

fn name() -> SigNozIngestionKeyName {
    SigNozIngestionKeyName::parse("willikins-example-key").unwrap()
}

fn inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs
}

fn live_tool(url: String) -> SigNozIngestionKeyEnsure {
    let credential = Credential::for_testing(
        "WILLIKINS_TEST_SIGNOZ_API_KEY",
        "test-signoz-api-key-000000",
    );
    let http = willikins_providers_signoz::http_client(url, credential);
    SigNozIngestionKeyEnsure::new(Arc::new(SigNozClient::new(http)))
}

fn fake_tool(
    state: Arc<Mutex<FakeState>>,
) -> willikins_providers_fake::tools::SigNozIngestionKeyEnsure {
    willikins_providers_fake::tools::SigNozIngestionKeyEnsure::new(state)
}

/// `key`'s value is never compared: both sides always report `Unknown`
/// for any observation but a fresh mint, so a JSON comparison of the
/// whole `Observation`/`Ensured` already proves they agree on that
/// without either side's real bytes ever existing to leak.
fn observation_json(observation: &Observation) -> serde_json::Value {
    serde_json::to_value(observation).expect("Observation serializes")
}

#[test]
fn read_agrees_on_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/api/v2/gateway/ingestion_keys")
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_absent").to_string())
        .create();
    let live = live_tool(provider.url()).read(&inputs()).unwrap();

    let state = Arc::new(Mutex::new(FakeState::new()));
    let fake = fake_tool(state).read(&inputs()).unwrap();

    assert!(matches!(live, Observation::Absent { .. }), "live: {live:?}");
    assert!(matches!(fake, Observation::Absent { .. }), "fake: {fake:?}");
    assert_eq!(observation_json(&live), observation_json(&fake));
}

#[test]
fn read_agrees_on_present_with_key_unknown() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/api/v2/gateway/ingestion_keys")
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_present").to_string())
        .create();
    let live = live_tool(provider.url()).read(&inputs()).unwrap();

    let state = Arc::new(Mutex::new(
        FakeState::new().with_signoz_ingestion_key(&name()),
    ));
    let fake = fake_tool(state).read(&inputs()).unwrap();

    assert!(matches!(live, Observation::Present(_)), "live: {live:?}");
    assert!(matches!(fake, Observation::Present(_)), "fake: {fake:?}");
    assert_eq!(observation_json(&live), observation_json(&fake));
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own key
fn ensure_agrees_on_create_changed_true_key_known() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/api/v2/gateway/ingestion_keys")
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_absent").to_string())
        .create();
    provider
        .mock("POST", "/api/v2/gateway/ingestion_keys")
        .with_status(201)
        .with_body(fixture("ingestion_key_post_created").to_string())
        .create();
    let sink = SinkToken::new();
    let live_ensured = live_tool(provider.url()).ensure(&inputs(), &sink).unwrap();

    let state = Arc::new(Mutex::new(FakeState::new()));
    let fake_ensured = fake_tool(state).ensure(&inputs(), &sink).unwrap();

    assert!(live_ensured.changed);
    assert!(fake_ensured.changed);
    let key_port = PortName::parse("key").unwrap();
    assert!(live_ensured.outputs.get(&key_port).unwrap().is_known());
    assert!(fake_ensured.outputs.get(&key_port).unwrap().is_known());
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own key
fn ensure_agrees_on_existing_changed_false_key_unknown() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/api/v2/gateway/ingestion_keys")
        .with_status(200)
        .with_body(fixture("ingestion_keys_list_present").to_string())
        .create();
    let sink = SinkToken::new();
    let live_ensured = live_tool(provider.url()).ensure(&inputs(), &sink).unwrap();

    let state = Arc::new(Mutex::new(
        FakeState::new().with_signoz_ingestion_key(&name()),
    ));
    let fake_ensured = fake_tool(state).ensure(&inputs(), &sink).unwrap();

    assert!(!live_ensured.changed);
    assert!(!fake_ensured.changed);
    let key_port = PortName::parse("key").unwrap();
    assert!(!live_ensured.outputs.get(&key_port).unwrap().is_known());
    assert!(!fake_ensured.outputs.get(&key_port).unwrap().is_known());
}
