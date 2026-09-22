//! The fake App Store Connect tools agree with the live ones
//! *behaviourally*, not only on their `ToolSpec`s -- the same gap
//! `willikins-providers-buildkite/tests/fake_agrees_with_live.rs` closes
//! for Buildkite, for the same reason its own module doc gives.
//!
//! Method: for each observation `appstore.bundle_id.ensure` and
//! `appstore.bundle_id_capability.ensure` can report, seed the fake's
//! state and serve the live tool a mock body that stand for *the same
//! real world*, then assert both tools answer the same way.

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, Observation, PortName, Tool, Value};
use willikins_providers_appstore::{AppstoreBundleIdCapabilityEnsure, AppstoreBundleIdEnsure};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::tools::{
    FakeAppstoreBundleIdCapabilityEnsure, FakeAppstoreBundleIdEnsure,
};
use willikins_providers_http::testing::MockProvider;
use willikins_types::{
    AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, AppleCapabilityType,
    AppleIssuerId, AppleKeyId, AppleSigningKey, DomainType,
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

fn name() -> AppleBundleIdName {
    AppleBundleIdName::parse("third-thoughts").unwrap()
}

fn platform() -> AppleBundleIdPlatform {
    AppleBundleIdPlatform::parse("UNIVERSAL").unwrap()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn bundle_id_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("issuer_id"), Value::known(issuer_id()));
    inputs.insert(port("key_id"), Value::known(key_id()));
    inputs.insert(port("key"), Value::known(key()));
    inputs.insert(port("identifier"), Value::known(identifier()));
    inputs.insert(port("name"), Value::known(name()));
    inputs.insert(port("platform"), Value::known(platform()));
    inputs
}

fn capability_inputs(capability: &str) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("issuer_id"), Value::known(issuer_id()));
    inputs.insert(port("key_id"), Value::known(key_id()));
    inputs.insert(port("key"), Value::known(key()));
    inputs.insert(port("identifier"), Value::known(identifier()));
    inputs.insert(
        port("capability"),
        Value::known(AppleCapabilityType::parse(capability).unwrap()),
    );
    inputs
}

fn shape(observation: &Observation) -> String {
    match observation {
        Observation::Absent { .. } => "Absent".to_string(),
        Observation::Present(_) => "Present".to_string(),
        Observation::Foreign => "Foreign".to_string(),
        Observation::Mismatch { port } => format!("Mismatch({port})"),
    }
}

fn rendered(observation: &Observation) -> Vec<(String, String)> {
    let outputs = match observation {
        Observation::Present(outputs) => Some(outputs),
        Observation::Absent { .. } | Observation::Foreign | Observation::Mismatch { .. } => None,
    };
    outputs
        .map(|outputs| {
            outputs
                .iter()
                .map(|(name, value)| (name.to_string(), value.render().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn bundle_id_list_body(id: &str, name: &str, platform: &str) -> String {
    serde_json::json!({
        "data": [{
            "id": id,
            "attributes": {
                "identifier": "com.example.MyApp",
                "name": name,
                "platform": platform,
            }
        }]
    })
    .to_string()
}

fn assert_bundle_id_agree(case: &str, state: FakeState, served: Option<String>) {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any);
    let _mock = match served {
        Some(body) => mock.with_status(200).with_body(body).create(),
        None => mock
            .with_status(200)
            .with_body(serde_json::json!({"data": []}).to_string())
            .create(),
    };

    let live = AppstoreBundleIdEnsure::new(provider.url())
        .read(&bundle_id_inputs())
        .unwrap_or_else(|err| panic!("{case}: the live tool failed: {err}"));
    let fake = FakeAppstoreBundleIdEnsure::new(Arc::new(Mutex::new(state)))
        .read(&bundle_id_inputs())
        .unwrap_or_else(|err| panic!("{case}: the fake tool failed: {err}"));

    assert_eq!(
        shape(&live),
        shape(&fake),
        "{case}: live says {live:?}, fake says {fake:?}"
    );
    assert_eq!(rendered(&live), rendered(&fake), "{case}: outputs differ");
}

#[test]
fn absent_agrees() {
    assert_bundle_id_agree("absent", FakeState::new(), None);
}

#[test]
fn present_agrees() {
    let state = FakeState::new().with_apple_bundle_id(&identifier(), &name(), &platform());
    // The fake derives its own id from `identifier` (`fake_apple_bundle_id_id`);
    // the live mock must answer with the same id so the two sides' outputs
    // actually compare equal, not merely agree on shape.
    let id = willikins_providers_fake::state::fake_apple_bundle_id_id(identifier().as_str());
    assert_bundle_id_agree(
        "present",
        state,
        Some(bundle_id_list_body(&id, "third-thoughts", "UNIVERSAL")),
    );
}

#[test]
fn mismatch_name_agrees() {
    let state = FakeState::new().with_apple_bundle_id(
        &identifier(),
        &AppleBundleIdName::parse("someone-elses-name").unwrap(),
        &platform(),
    );
    assert_bundle_id_agree(
        "mismatch-name",
        state,
        Some(bundle_id_list_body(
            "T6G4XCV345",
            "someone-elses-name",
            "UNIVERSAL",
        )),
    );
}

#[test]
fn mismatch_platform_agrees_and_is_checked_before_name() {
    let state = FakeState::new().with_apple_bundle_id(
        &identifier(),
        &AppleBundleIdName::parse("someone-elses-name").unwrap(),
        &AppleBundleIdPlatform::parse("IOS").unwrap(),
    );
    assert_bundle_id_agree(
        "mismatch-platform",
        state,
        Some(bundle_id_list_body(
            "T6G4XCV345",
            "someone-elses-name",
            "IOS",
        )),
    );
}

// ---------------------------------------------------------------------
// `appstore.bundle_id_capability.ensure`
// ---------------------------------------------------------------------

#[test]
fn capability_not_found_agrees_by_error_kind() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"data": []}).to_string())
        .create();
    let live = AppstoreBundleIdCapabilityEnsure::new(provider.url())
        .read(&capability_inputs("PUSH_NOTIFICATIONS"))
        .expect_err("the live tool refuses");
    let fake = FakeAppstoreBundleIdCapabilityEnsure::new(Arc::new(Mutex::new(FakeState::new())))
        .read(&capability_inputs("PUSH_NOTIFICATIONS"))
        .expect_err("the fake tool refuses");
    assert_eq!(live.kind, fake.kind);
}

#[test]
fn capability_absent_and_present_agree() {
    for (case, capabilities_body, seed_capability) in [
        ("absent", serde_json::json!({"data": []}), None),
        (
            "present",
            serde_json::json!({"data": [{"attributes": {"capabilityType": "PUSH_NOTIFICATIONS"}}]}),
            Some("PUSH_NOTIFICATIONS"),
        ),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/v1/bundleIds")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(bundle_id_list_body(
                "T6G4XCV345",
                "third-thoughts",
                "UNIVERSAL",
            ))
            .create();
        provider
            .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
            .with_status(200)
            .with_body(capabilities_body.to_string())
            .create();

        let mut state = FakeState::new().with_apple_bundle_id(&identifier(), &name(), &platform());
        if let Some(capability) = seed_capability {
            state = state.with_apple_bundle_id_capability(
                &identifier(),
                &AppleCapabilityType::parse(capability).unwrap(),
            );
        }

        let live = AppstoreBundleIdCapabilityEnsure::new(provider.url())
            .read(&capability_inputs("PUSH_NOTIFICATIONS"))
            .unwrap_or_else(|err| panic!("{case}: live failed: {err}"));
        let fake = FakeAppstoreBundleIdCapabilityEnsure::new(Arc::new(Mutex::new(state)))
            .read(&capability_inputs("PUSH_NOTIFICATIONS"))
            .unwrap_or_else(|err| panic!("{case}: fake failed: {err}"));

        assert_eq!(shape(&live), shape(&fake), "{case}");
    }
}
