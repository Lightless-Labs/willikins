//! Mock-server tests for `appstore.bundle_id_capability.gate`: parent
//! absent, present, and absent-but-registered -- the same three cases
//! `AppstoreAppGroupGate`'s own mock coverage in `fake_agrees_with_live.rs`
//! pins for `APP_GROUPS`, exercised here directly against the live tool
//! (not only through the fake-parity harness) and with a read-only
//! capability (`APP_ATTEST`) `AppleCapabilityType` itself could never
//! express.

use willikins_core::{Observation, PortName, Tool, Value};
use willikins_providers_appstore::AppstoreBundleIdCapabilityGate;
use willikins_providers_http::testing::MockProvider;
use willikins_types::{
    AppleBundleIdentifier, AppleIssuerId, AppleKeyId, AppleObservableCapabilityType,
    AppleSigningKey, DomainType,
};

fn issuer_id() -> AppleIssuerId {
    AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap()
}

fn key_id() -> AppleKeyId {
    AppleKeyId::parse("2X9R4HXF34").unwrap()
}

fn key() -> AppleSigningKey {
    AppleSigningKey::parse(AppleSigningKey::example()).unwrap()
}

fn identifier() -> AppleBundleIdentifier {
    AppleBundleIdentifier::parse("com.example.MyApp").unwrap()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn inputs(capability: &str) -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(port("issuer_id"), Value::known(issuer_id()));
    inputs.insert(port("key_id"), Value::known(key_id()));
    inputs.insert(port("key"), Value::known(key()));
    inputs.insert(port("identifier"), Value::known(identifier()));
    inputs.insert(
        port("capability"),
        Value::known(AppleObservableCapabilityType::parse(capability).unwrap()),
    );
    inputs
}

#[test]
fn absent_when_the_parent_identifier_is_not_registered() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"data": []}).to_string())
        .create();

    let tool = AppstoreBundleIdCapabilityGate::new(provider.url());
    let observation = tool.read(&inputs("APP_ATTEST")).expect("read succeeds");
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn absent_when_registered_but_the_capability_is_not_in_the_list() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({"data": [{
                "id": "BID1",
                "attributes": {"identifier": "com.example.MyApp", "name": "third-thoughts", "platform": "UNIVERSAL"}
            }]})
            .to_string(),
        )
        .create();
    provider
        .mock("GET", "/v1/bundleIds/BID1/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"data": []}).to_string())
        .create();

    let tool = AppstoreBundleIdCapabilityGate::new(provider.url());
    let observation = tool.read(&inputs("APP_ATTEST")).expect("read succeeds");
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn present_once_the_read_only_capability_is_listed() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({"data": [{
                "id": "BID1",
                "attributes": {"identifier": "com.example.MyApp", "name": "third-thoughts", "platform": "UNIVERSAL"}
            }]})
            .to_string(),
        )
        .create();
    provider
        .mock("GET", "/v1/bundleIds/BID1/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({"data": [{"attributes": {"capabilityType": "APP_ATTEST"}}]})
                .to_string(),
        )
        .create();

    let tool = AppstoreBundleIdCapabilityGate::new(provider.url());
    let observation = tool.read(&inputs("APP_ATTEST")).expect("read succeeds");
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let out = outputs.get(&port("identifier")).unwrap();
    assert_eq!(out.render().to_string(), "com.example.MyApp");
}

#[test]
fn ensure_agrees_with_read_and_never_reports_changed() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({"data": [{
                "id": "BID1",
                "attributes": {"identifier": "com.example.MyApp", "name": "third-thoughts", "platform": "UNIVERSAL"}
            }]})
            .to_string(),
        )
        .create();
    provider
        .mock("GET", "/v1/bundleIds/BID1/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({"data": [{"attributes": {"capabilityType": "APP_ATTEST"}}]})
                .to_string(),
        )
        .create();

    let tool = AppstoreBundleIdCapabilityGate::new(provider.url());
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    let token = willikins_core::SinkToken::new();
    let ensured = tool
        .ensure(&inputs("APP_ATTEST"), &token)
        .expect("ensure succeeds");
    assert!(!ensured.changed);
}

// ---------------------------------------------------------------------
// Adversarial pass 7: the gate never writes, even when its capability is
// missing. Structurally it cannot write a read-only capability at all
// (the client's only capability write takes `&AppleCapabilityType`, which
// this module never imports, and no conversion reaches it); these pin the
// executable half: `ensure` on an unmet gate sends no `POST` (the mock
// below must see zero calls) and any other unmocked write would have
// failed `ensure` outright with mockito's 501.
// ---------------------------------------------------------------------

#[test]
fn ensure_on_an_unregistered_parent_never_writes() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"data": []}).to_string())
        .create();
    let create_bundle_id = provider.mock("POST", "/v1/bundleIds").expect(0).create();
    let create_capability = provider
        .mock("POST", "/v1/bundleIdCapabilities")
        .expect(0)
        .create();

    let tool = AppstoreBundleIdCapabilityGate::new(provider.url());
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    let token = willikins_core::SinkToken::new();
    let ensured = tool
        .ensure(&inputs("APP_ATTEST"), &token)
        .expect("an unmet gate's ensure still succeeds, reporting nothing changed");
    assert!(!ensured.changed);
    create_bundle_id.assert();
    create_capability.assert();
}

#[test]
fn ensure_when_the_capability_is_not_listed_never_writes() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({"data": [{
                "id": "BID1",
                "attributes": {"identifier": "com.example.MyApp", "name": "third-thoughts", "platform": "UNIVERSAL"}
            }]})
            .to_string(),
        )
        .create();
    provider
        .mock("GET", "/v1/bundleIds/BID1/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"data": []}).to_string())
        .create();
    let create_capability = provider
        .mock("POST", "/v1/bundleIdCapabilities")
        .expect(0)
        .create();

    let tool = AppstoreBundleIdCapabilityGate::new(provider.url());
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    let token = willikins_core::SinkToken::new();
    let ensured = tool
        .ensure(&inputs("APP_ATTEST"), &token)
        .expect("an unmet gate's ensure still succeeds, reporting nothing changed");
    assert!(!ensured.changed);
    create_capability.assert();
}
