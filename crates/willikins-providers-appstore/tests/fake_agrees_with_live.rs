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
use willikins_providers_appstore::{
    AppstoreBundleIdCapabilityEnsure, AppstoreBundleIdEnsure, AppstoreCertificateGet,
    AppstoreProfileEnsure,
};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::tools::{
    FakeAppstoreBundleIdCapabilityEnsure, FakeAppstoreBundleIdEnsure, FakeAppstoreCertificateGet,
    FakeAppstoreProfileEnsure,
};
use willikins_providers_http::testing::MockProvider;
use willikins_types::{
    AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, AppleCapabilityType,
    AppleCertificateId, AppleCertificateSerial, AppleCertificateType, AppleIssuerId, AppleKeyId,
    AppleProfileName, AppleProfileType, AppleSigningKey, DomainType,
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

// ---------------------------------------------------------------------
// `appstore.certificate.get`
// ---------------------------------------------------------------------

fn certificate_type() -> AppleCertificateType {
    AppleCertificateType::parse("DISTRIBUTION").unwrap()
}

fn serial_number() -> AppleCertificateSerial {
    AppleCertificateSerial::parse("7B3F2A9C1D4E5F607182930A1B2C3D4E").unwrap()
}

fn certificate_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("issuer_id"), Value::known(issuer_id()));
    inputs.insert(port("key_id"), Value::known(key_id()));
    inputs.insert(port("key"), Value::known(key()));
    inputs.insert(port("certificate_type"), Value::known(certificate_type()));
    inputs.insert(port("serial_number"), Value::known(serial_number()));
    inputs
}

fn certificate_list_body(id: &str, activated: Option<bool>, expired: bool) -> String {
    let mut attributes = serde_json::json!({
        "certificateType": "DISTRIBUTION",
        "serialNumber": "7B3F2A9C1D4E5F607182930A1B2C3D4E",
        "expirationDate": if expired { "2020-01-01T00:00:00.000+00:00" } else { "2099-01-01T00:00:00.000+00:00" },
    });
    if let Some(activated) = activated {
        attributes["activated"] = serde_json::json!(activated);
    }
    serde_json::json!({
        "data": [{
            "id": id,
            "attributes": attributes,
        }]
    })
    .to_string()
}

/// Seeds and serves *the same real world* for both sides, exactly the
/// method [`assert_bundle_id_agree`] uses. `id` is fixed (`"CERT1"`)
/// rather than derived, since certificates carry no equivalent of
/// [`fake_apple_bundle_id_id`] -- this fake's `appstore.certificate.get`
/// never creates a record, only a seed file does (this crate's own
/// certificate-write guard rules out a create path entirely).
fn assert_certificate_agree(case: &str, state: FakeState, served: Option<String>) {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", "/v1/certificates")
        .match_query(mockito::Matcher::Any);
    let _mock = match served {
        Some(body) => mock.with_status(200).with_body(body).create(),
        None => mock
            .with_status(200)
            .with_body(serde_json::json!({"data": []}).to_string())
            .create(),
    };

    let live = AppstoreCertificateGet::new(provider.url())
        .read(&certificate_inputs())
        .unwrap_or_else(|err| panic!("{case}: the live tool failed: {err}"));
    let fake = FakeAppstoreCertificateGet::new(Arc::new(Mutex::new(state)))
        .read(&certificate_inputs())
        .unwrap_or_else(|err| panic!("{case}: the fake tool failed: {err}"));

    assert_eq!(
        shape(&live),
        shape(&fake),
        "{case}: live says {live:?}, fake says {fake:?}"
    );
    assert_eq!(rendered(&live), rendered(&fake), "{case}: outputs differ");
}

fn assert_certificate_error_kinds_agree(case: &str, state: FakeState, served: Option<String>) {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", "/v1/certificates")
        .match_query(mockito::Matcher::Any);
    let _mock = match served {
        Some(body) => mock.with_status(200).with_body(body).create(),
        None => mock
            .with_status(200)
            .with_body(serde_json::json!({"data": []}).to_string())
            .create(),
    };

    let live = AppstoreCertificateGet::new(provider.url())
        .read(&certificate_inputs())
        .expect_err(&format!("{case}: the live tool should refuse"));
    let fake = FakeAppstoreCertificateGet::new(Arc::new(Mutex::new(state)))
        .read(&certificate_inputs())
        .expect_err(&format!("{case}: the fake tool should refuse"));
    assert_eq!(live.kind, fake.kind, "{case}");
}

#[test]
fn certificate_not_found_agrees() {
    assert_certificate_error_kinds_agree("not-found", FakeState::new(), None);
}

#[test]
fn certificate_present_agrees() {
    let state = FakeState::new().with_apple_certificate(
        &certificate_type(),
        &serial_number(),
        "CERT1",
        false,
        None,
    );
    assert_certificate_agree(
        "present",
        state,
        Some(certificate_list_body("CERT1", None, false)),
    );
}

#[test]
fn certificate_expired_agrees() {
    let state = FakeState::new().with_apple_certificate(
        &certificate_type(),
        &serial_number(),
        "CERT1",
        true,
        None,
    );
    assert_certificate_error_kinds_agree(
        "expired",
        state,
        Some(certificate_list_body("CERT1", None, true)),
    );
}

#[test]
fn certificate_deactivated_agrees() {
    let state = FakeState::new().with_apple_certificate(
        &certificate_type(),
        &serial_number(),
        "CERT1",
        false,
        Some(false),
    );
    assert_certificate_error_kinds_agree(
        "deactivated",
        state,
        Some(certificate_list_body("CERT1", Some(false), false)),
    );
}

#[test]
fn certificate_conflict_on_two_matches_agrees() {
    let state = FakeState::new()
        .with_apple_certificate(&certificate_type(), &serial_number(), "CERT1", false, None)
        .with_apple_certificate(&certificate_type(), &serial_number(), "CERT2", false, None);
    let served = serde_json::json!({
        "data": [
            {"id": "CERT1", "attributes": {"certificateType": "DISTRIBUTION", "serialNumber": "7B3F2A9C1D4E5F607182930A1B2C3D4E"}},
            {"id": "CERT2", "attributes": {"certificateType": "DISTRIBUTION", "serialNumber": "7B3F2A9C1D4E5F607182930A1B2C3D4E"}},
        ]
    })
    .to_string();
    assert_certificate_error_kinds_agree("conflict-two-matches", state, Some(served));
}

// ---------------------------------------------------------------------
// `appstore.profile.ensure`
// ---------------------------------------------------------------------

fn profile_name() -> AppleProfileName {
    AppleProfileName::parse("willikins-example-profile").unwrap()
}

fn profile_type() -> AppleProfileType {
    AppleProfileType::parse("IOS_APP_STORE").unwrap()
}

fn profile_certificate() -> AppleCertificateId {
    AppleCertificateId::parse("C3RT1F1CATE1").unwrap()
}

fn profile_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("issuer_id"), Value::known(issuer_id()));
    inputs.insert(port("key_id"), Value::known(key_id()));
    inputs.insert(port("key"), Value::known(key()));
    inputs.insert(port("identifier"), Value::known(identifier()));
    inputs.insert(port("name"), Value::known(profile_name()));
    inputs.insert(port("profile_type"), Value::known(profile_type()));
    inputs.insert(port("certificate"), Value::known(profile_certificate()));
    inputs
}

fn profile_list_body(profile_id: &str, name: &str, profile_type: &str) -> String {
    serde_json::json!({
        "data": [{
            "id": profile_id,
            "attributes": {
                "name": name,
                "profileType": profile_type,
                "profileState": "ACTIVE",
                "expirationDate": "2099-01-01T00:00:00.000+00:00",
            }
        }]
    })
    .to_string()
}

fn profile_get_body(
    profile_id: &str,
    name: &str,
    profile_type: &str,
    profile_state: &str,
    expired: bool,
    certificate_id: &str,
    content: &str,
) -> String {
    serde_json::json!({
        "data": {
            "id": profile_id,
            "attributes": {
                "name": name,
                "profileType": profile_type,
                "profileState": profile_state,
                "expirationDate": if expired { "2020-01-01T00:00:00.000+00:00" } else { "2099-01-01T00:00:00.000+00:00" },
                "profileContent": content,
            },
            "relationships": {
                "certificates": {
                    "data": [{"type": "certificates", "id": certificate_id}]
                }
            }
        }
    })
    .to_string()
}

/// Seeds and serves *the same real world* for both sides. Unlike
/// [`assert_certificate_agree`], this tool's read is a two-step chain
/// (resolve the bundle id, then the profile), so both the bundle id's
/// derived id (mirroring [`present_agrees`]'s own reasoning) and the
/// fake's freshly-created profile id must line up with what the live
/// side is served -- `id` and `certificate_id` are therefore parameters,
/// not fixed strings.
/// Serves the three requests `appstore.profile.ensure`'s read can make
/// (bundle id list, profile relationship list, and -- only when a name
/// matched -- the single-instance profile `GET`) against `provider`.
/// `profile_id` names the literal path the instance `GET` is mocked at;
/// `None` when the case never reaches that call (identifier or name
/// absent, or an ambiguous name match).
fn mock_profile_reads(
    provider: &mut MockProvider,
    bundle_id: &str,
    served_profile_list: Option<String>,
    served_profile_get: Option<(&str, String)>,
) {
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(bundle_id_list_body(bundle_id, "example", "UNIVERSAL"))
        .create();
    let list_mock = provider.mock(
        "GET",
        format!("/v1/bundleIds/{bundle_id}/profiles").as_str(),
    );
    let list_body =
        served_profile_list.unwrap_or_else(|| serde_json::json!({"data": []}).to_string());
    list_mock
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(list_body)
        .create();
    if let Some((profile_id, body)) = served_profile_get {
        provider
            .mock("GET", format!("/v1/profiles/{profile_id}").as_str())
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(body)
            .create();
    }
}

/// Seeds and serves *the same real world* for both sides. Unlike
/// [`assert_certificate_agree`], this tool's read is a two-step chain
/// (resolve the bundle id, then the profile), so both the bundle id's
/// derived id (mirroring [`present_agrees`]'s own reasoning) and the
/// profile id the instance `GET` is mocked at must line up with what the
/// fake side was seeded with.
fn assert_profile_agree(
    case: &str,
    state: FakeState,
    bundle_id: &str,
    served_profile_list: Option<String>,
    served_profile_get: Option<(&str, String)>,
) {
    let mut provider = MockProvider::start();
    mock_profile_reads(
        &mut provider,
        bundle_id,
        served_profile_list,
        served_profile_get,
    );

    let live = AppstoreProfileEnsure::new(provider.url())
        .read(&profile_inputs())
        .unwrap_or_else(|err| panic!("{case}: the live tool failed: {err}"));
    let fake = FakeAppstoreProfileEnsure::new(Arc::new(Mutex::new(state)))
        .read(&profile_inputs())
        .unwrap_or_else(|err| panic!("{case}: the fake tool failed: {err}"));

    assert_eq!(
        shape(&live),
        shape(&fake),
        "{case}: live says {live:?}, fake says {fake:?}"
    );
    assert_eq!(rendered(&live), rendered(&fake), "{case}: outputs differ");
}

fn assert_profile_error_kinds_agree(
    case: &str,
    state: FakeState,
    bundle_id: &str,
    served_profile_list: Option<String>,
    served_profile_get: Option<(&str, String)>,
) {
    let mut provider = MockProvider::start();
    mock_profile_reads(
        &mut provider,
        bundle_id,
        served_profile_list,
        served_profile_get,
    );

    let live = AppstoreProfileEnsure::new(provider.url())
        .read(&profile_inputs())
        .expect_err(&format!("{case}: the live tool should refuse"));
    let fake = FakeAppstoreProfileEnsure::new(Arc::new(Mutex::new(state)))
        .read(&profile_inputs())
        .expect_err(&format!("{case}: the fake tool should refuse"));
    assert_eq!(live.kind, fake.kind, "{case}");
}

#[test]
fn profile_absent_when_identifier_not_registered_agrees() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"data": []}).to_string())
        .create();
    let live = AppstoreProfileEnsure::new(provider.url())
        .read(&profile_inputs())
        .expect("the live tool reads");
    let fake = FakeAppstoreProfileEnsure::new(Arc::new(Mutex::new(FakeState::new())))
        .read(&profile_inputs())
        .expect("the fake tool reads");
    assert_eq!(shape(&live), shape(&fake));
}

#[test]
fn profile_absent_when_name_not_found_agrees() {
    let bundle_id = willikins_providers_fake::state::fake_apple_bundle_id_id(identifier().as_str());
    let state = FakeState::new().with_apple_bundle_id(
        &identifier(),
        &AppleBundleIdName::parse("example").unwrap(),
        &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
    );
    assert_profile_agree("absent-name", state, &bundle_id, None, None);
}

#[test]
fn profile_present_agrees() {
    let bundle_id = willikins_providers_fake::state::fake_apple_bundle_id_id(identifier().as_str());
    let state = FakeState::new()
        .with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("example").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )
        .with_apple_profile(
            &identifier(),
            &profile_name(),
            "PROFILE1",
            profile_certificate().as_str(),
            "IOS_APP_STORE",
            "ACTIVE",
            false,
            "ZmFrZWNvbnRlbnQ=",
        );
    assert_profile_agree(
        "present",
        state,
        &bundle_id,
        Some(profile_list_body(
            "PROFILE1",
            "willikins-example-profile",
            "IOS_APP_STORE",
        )),
        Some((
            "PROFILE1",
            profile_get_body(
                "PROFILE1",
                "willikins-example-profile",
                "IOS_APP_STORE",
                "ACTIVE",
                false,
                profile_certificate().as_str(),
                "ZmFrZWNvbnRlbnQ=",
            ),
        )),
    );
}

#[test]
fn profile_mismatch_profile_type_agrees() {
    let bundle_id = willikins_providers_fake::state::fake_apple_bundle_id_id(identifier().as_str());
    let state = FakeState::new()
        .with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("example").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )
        .with_apple_profile(
            &identifier(),
            &profile_name(),
            "PROFILE1",
            profile_certificate().as_str(),
            "IOS_APP_ADHOC",
            "ACTIVE",
            false,
            "ZmFrZWNvbnRlbnQ=",
        );
    assert_profile_agree(
        "mismatch-profile-type",
        state,
        &bundle_id,
        Some(profile_list_body(
            "PROFILE1",
            "willikins-example-profile",
            "IOS_APP_ADHOC",
        )),
        Some((
            "PROFILE1",
            profile_get_body(
                "PROFILE1",
                "willikins-example-profile",
                "IOS_APP_ADHOC",
                "ACTIVE",
                false,
                profile_certificate().as_str(),
                "ZmFrZWNvbnRlbnQ=",
            ),
        )),
    );
}

#[test]
fn profile_invalid_state_agrees() {
    let bundle_id = willikins_providers_fake::state::fake_apple_bundle_id_id(identifier().as_str());
    let state = FakeState::new()
        .with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("example").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )
        .with_apple_profile(
            &identifier(),
            &profile_name(),
            "PROFILE1",
            profile_certificate().as_str(),
            "IOS_APP_STORE",
            "INVALID",
            false,
            "ZmFrZWNvbnRlbnQ=",
        );
    assert_profile_error_kinds_agree(
        "invalid",
        state,
        &bundle_id,
        Some(profile_list_body(
            "PROFILE1",
            "willikins-example-profile",
            "IOS_APP_STORE",
        )),
        Some((
            "PROFILE1",
            profile_get_body(
                "PROFILE1",
                "willikins-example-profile",
                "IOS_APP_STORE",
                "INVALID",
                false,
                profile_certificate().as_str(),
                "ZmFrZWNvbnRlbnQ=",
            ),
        )),
    );
}

#[test]
fn profile_conflict_on_two_names_agrees() {
    let bundle_id = willikins_providers_fake::state::fake_apple_bundle_id_id(identifier().as_str());
    let state = FakeState::new()
        .with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("example").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )
        .with_apple_profile(
            &identifier(),
            &profile_name(),
            "PROFILE1",
            profile_certificate().as_str(),
            "IOS_APP_STORE",
            "ACTIVE",
            false,
            "ZmFrZWNvbnRlbnQ=",
        )
        .with_apple_profile(
            &identifier(),
            &profile_name(),
            "PROFILE2",
            profile_certificate().as_str(),
            "IOS_APP_STORE",
            "ACTIVE",
            false,
            "ZmFrZWNvbnRlbnQ=",
        );
    let served = serde_json::json!({
        "data": [
            {"id": "PROFILE1", "attributes": {"name": "willikins-example-profile", "profileType": "IOS_APP_STORE", "profileState": "ACTIVE", "expirationDate": "2099-01-01T00:00:00.000+00:00"}},
            {"id": "PROFILE2", "attributes": {"name": "willikins-example-profile", "profileType": "IOS_APP_STORE", "profileState": "ACTIVE", "expirationDate": "2099-01-01T00:00:00.000+00:00"}},
        ]
    })
    .to_string();
    assert_profile_error_kinds_agree("conflict-two-names", state, &bundle_id, Some(served), None);
}

#[test]
fn profile_mismatch_certificate_agrees() {
    let bundle_id = willikins_providers_fake::state::fake_apple_bundle_id_id(identifier().as_str());
    let state = FakeState::new()
        .with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("example").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )
        .with_apple_profile(
            &identifier(),
            &profile_name(),
            "PROFILE1",
            "OTHERCERTID999",
            "IOS_APP_STORE",
            "ACTIVE",
            false,
            "ZmFrZWNvbnRlbnQ=",
        );
    assert_profile_agree(
        "mismatch-certificate",
        state,
        &bundle_id,
        Some(profile_list_body(
            "PROFILE1",
            "willikins-example-profile",
            "IOS_APP_STORE",
        )),
        Some((
            "PROFILE1",
            profile_get_body(
                "PROFILE1",
                "willikins-example-profile",
                "IOS_APP_STORE",
                "ACTIVE",
                false,
                "OTHERCERTID999",
                "ZmFrZWNvbnRlbnQ=",
            ),
        )),
    );
}

#[test]
fn profile_expired_agrees() {
    let bundle_id = willikins_providers_fake::state::fake_apple_bundle_id_id(identifier().as_str());
    let state = FakeState::new()
        .with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("example").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )
        .with_apple_profile(
            &identifier(),
            &profile_name(),
            "PROFILE1",
            profile_certificate().as_str(),
            "IOS_APP_STORE",
            "ACTIVE",
            true,
            "ZmFrZWNvbnRlbnQ=",
        );
    assert_profile_error_kinds_agree(
        "expired",
        state,
        &bundle_id,
        Some(profile_list_body(
            "PROFILE1",
            "willikins-example-profile",
            "IOS_APP_STORE",
        )),
        Some((
            "PROFILE1",
            profile_get_body(
                "PROFILE1",
                "willikins-example-profile",
                "IOS_APP_STORE",
                "ACTIVE",
                true,
                profile_certificate().as_str(),
                "ZmFrZWNvbnRlbnQ=",
            ),
        )),
    );
}
