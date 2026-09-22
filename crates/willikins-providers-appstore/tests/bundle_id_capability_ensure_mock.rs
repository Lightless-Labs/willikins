//! Mock-server tests for `appstore.bundle_id_capability.ensure`.

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_appstore::AppstoreBundleIdCapabilityEnsure;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_types::{
    AppleBundleIdentifier, AppleCapabilityType, AppleIssuerId, AppleKeyId, AppleSigningKey,
    DomainType,
};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "appstore", name)
}

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

fn inputs_for(capability: &str) -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("issuer_id").unwrap(),
        Value::known(issuer_id()),
    );
    inputs.insert(PortName::parse("key_id").unwrap(), Value::known(key_id()));
    inputs.insert(PortName::parse("key").unwrap(), Value::known(key()));
    inputs.insert(
        PortName::parse("identifier").unwrap(),
        Value::known(identifier()),
    );
    inputs.insert(
        PortName::parse("capability").unwrap(),
        Value::known(AppleCapabilityType::parse(capability).unwrap()),
    );
    inputs
}

fn mock_bundle_id_lookup(provider: &mut MockProvider) -> mockito::Mock {
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::UrlEncoded(
            "filter[identifier]".into(),
            "com.example.MyApp".into(),
        ))
        .with_status(200)
        .with_body(fixture("bundle_id_list_one").to_string())
        .create()
}

// ---------------------------------------------------------------------
// `read`
// ---------------------------------------------------------------------

#[test]
fn read_refuses_when_the_parent_bundle_id_does_not_exist() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("bundle_id_list_empty").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let err = tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert!(err.message.contains("com.example.MyApp"), "{}", err.message);
}

#[test]
fn read_reports_absent_when_the_capability_is_not_in_the_list() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .with_status(200)
        .with_body(fixture("capabilities_list_empty").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn read_reports_present_when_the_capability_is_already_in_the_list() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .with_status(200)
        .with_body(fixture("capabilities_list_with_push").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

// ---------------------------------------------------------------------
// `ensure`: an ordinary capability
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_an_ordinary_capability_when_absent() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .with_status(200)
        .with_body(fixture("capabilities_list_empty").to_string())
        .create();
    let create = provider
        .mock("POST", "/v1/bundleIdCapabilities")
        .match_body(willikins_providers_http::testing::json_body(
            serde_json::json!({
                "data": {
                    "type": "bundleIdCapabilities",
                    "attributes": {"capabilityType": "PUSH_NOTIFICATIONS"},
                    "relationships": {
                        "bundleId": {"data": {"type": "bundleIds", "id": "T6G4XCV345"}}
                    }
                }
            }),
        ))
        .with_status(201)
        .with_body(fixture("capability_post_created").to_string())
        .expect(1)
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool
        .ensure(&inputs_for("PUSH_NOTIFICATIONS"), &token)
        .unwrap();
    assert!(ensured.changed);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_is_a_no_op_when_the_ordinary_capability_is_already_present() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .with_status(200)
        .with_body(fixture("capabilities_list_with_push").to_string())
        .create();
    let create = provider
        .mock("POST", "/v1/bundleIdCapabilities")
        .expect(0)
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool
        .ensure(&inputs_for("PUSH_NOTIFICATIONS"), &token)
        .unwrap();
    assert!(!ensured.changed);
    create.assert();
}

// ---------------------------------------------------------------------
// `ensure`: the three portal-configuration-only capabilities
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_app_groups_apple_pay_and_icloud_when_absent_and_never_posts() {
    for capability in ["APP_GROUPS", "APPLE_PAY", "ICLOUD"] {
        let mut provider = MockProvider::start();
        mock_bundle_id_lookup(&mut provider);
        provider
            .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
            .with_status(200)
            .with_body(fixture("capabilities_list_empty").to_string())
            .create();
        let create = provider
            .mock("POST", "/v1/bundleIdCapabilities")
            .expect(0)
            .create();
        let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
        let token = SinkToken::new();
        let err = tool
            .ensure(&inputs_for(capability), &token)
            .expect_err(capability);
        assert_eq!(err.kind, ToolErrorKind::Invalid, "{capability}");
        assert!(err.message.contains(capability), "{}", err.message);
        create.assert();
    }
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_converges_an_already_present_portal_only_capability_with_no_refusal() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .with_status(200)
        .with_body(
            serde_json::json!({"data": [{"attributes": {"capabilityType": "APP_GROUPS"}}]})
                .to_string(),
        )
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs_for("APP_GROUPS"), &token).unwrap();
    assert!(!ensured.changed);
}
