//! Mock-server tests for `appstore.profile.ensure`: every arm this
//! crate's own module doc names (acceptance tests 5, 6, and 7,
//! `docs/plans/2026-09-22-milestone-3c-app-store-signing.md`).

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolError, ToolErrorKind, Value};
use willikins_providers_appstore::AppstoreProfileEnsure;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_types::{
    AppleBundleIdentifier, AppleCertificateId, AppleIssuerId, AppleKeyId, AppleProfileContent,
    AppleProfileName, AppleProfileType, AppleSigningKey, DomainType,
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

fn name() -> AppleProfileName {
    AppleProfileName::parse("willikins-example-profile").unwrap()
}

fn profile_type() -> AppleProfileType {
    AppleProfileType::parse("IOS_APP_STORE").unwrap()
}

fn certificate() -> AppleCertificateId {
    AppleCertificateId::parse("C3RT1F1CATE1").unwrap()
}

fn inputs() -> willikins_core::Inputs {
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
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs.insert(
        PortName::parse("profile_type").unwrap(),
        Value::known(profile_type()),
    );
    inputs.insert(
        PortName::parse("certificate").unwrap(),
        Value::known(certificate()),
    );
    inputs
}

fn mock_bundle_id_list(provider: &mut MockProvider, body: &serde_json::Value) -> mockito::Mock {
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(body.to_string())
        .create()
}

fn mock_profile_list(provider: &mut MockProvider, body: &serde_json::Value) -> mockito::Mock {
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(body.to_string())
        .create()
}

fn mock_profile_get(
    provider: &mut MockProvider,
    status: usize,
    body: &serde_json::Value,
) -> mockito::Mock {
    provider
        .mock("GET", "/v1/profiles/PR0F1LE1D0001")
        .match_query(mockito::Matcher::Any)
        .with_status(status)
        .with_body(body.to_string())
        .create()
}

fn mock_profile_post(
    provider: &mut MockProvider,
    status: usize,
    body: &serde_json::Value,
) -> mockito::Mock {
    provider
        .mock("POST", "/v1/profiles")
        .with_status(status)
        .with_body(body.to_string())
        .create()
}

fn content_of(outputs: &willikins_core::Outputs) -> AppleProfileContent {
    outputs
        .get(&PortName::parse("content").unwrap())
        .unwrap()
        .downcast::<AppleProfileContent>()
        .unwrap()
        .clone()
}

fn profile_id_of(outputs: &willikins_core::Outputs) -> String {
    outputs
        .get(&PortName::parse("profile").unwrap())
        .unwrap()
        .render()
        .to_string()
}

// ---------------------------------------------------------------------
// `read`: identifier resolution
// ---------------------------------------------------------------------

#[test]
fn read_reports_absent_when_the_identifier_is_not_registered() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_empty"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

// ---------------------------------------------------------------------
// `read`: name resolution against the bundle id's relationship
// ---------------------------------------------------------------------

#[test]
fn read_reports_absent_when_no_profile_matches_the_name() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_empty"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// `filter[name]` is never used (decision (e)), so a name that is merely
/// a substring neighbor of the requested one must not count as a match
/// -- exactly the load-bearing byte-exact compare
/// `appstore.bundle_id.ensure` and `appstore.certificate.get` already
/// prove for their own keys.
#[test]
fn read_reports_absent_when_only_a_substring_neighbor_name_matches() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_substring_neighbor"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn read_reports_conflict_naming_the_count_when_two_profiles_share_the_name() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_two"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains('2'), "{}", err.message);
    assert!(!err.message.contains("PR0F1LE1D0001"), "{}", err.message);
    assert!(!err.message.contains("PR0F1LE1D0002"), "{}", err.message);
}

#[test]
fn read_finds_an_exact_name_match_on_page_two() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));

    let mut page_one = fixture("profile_list_neighbor_only_page_one");
    page_one["links"] = serde_json::json!({
        "self": format!("{}/v1/bundleIds/T6G4XCV345/profiles", provider.url()),
        "next": format!("{}/v1/bundleIds/T6G4XCV345/profiles?cursor=PAGE2&limit=200", provider.url()),
    });
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(page_one.to_string())
        .create();
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::UrlEncoded(
            "cursor".into(),
            "PAGE2".into(),
        ))
        .with_status(200)
        .with_body(fixture("profile_list_one").to_string())
        .create();
    mock_profile_get(&mut provider, 200, &fixture("profile_get_present_healthy"));

    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

// ---------------------------------------------------------------------
// `read`: the single-instance checks, in decision (d)/(e)'s order
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn read_reports_present_with_profile_and_secret_content_when_healthy() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_one"));
    mock_profile_get(&mut provider, 200, &fixture("profile_get_present_healthy"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
        panic!("expected Present");
    };
    assert_eq!(profile_id_of(&outputs), "PR0F1LE1D0001");
    let content = content_of(&outputs);
    let exposed = content.expose(&SinkToken::new()).to_string();
    assert_eq!(
        exposed,
        "d2lsbGlraW5zLWV4YW1wbGUtcHJvZmlsZS1jb250ZW50LWV4YW1wbGU="
    );
}

#[test]
fn read_reports_mismatch_profile_type_when_the_profile_type_differs() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_one"));
    mock_profile_get(&mut provider, 200, &fixture("profile_get_wrong_type"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(
        &observation,
        Observation::Mismatch { port } if *port == PortName::parse("profile_type").unwrap()
    ));
}

#[test]
fn read_reports_mismatch_certificate_when_the_certificate_differs() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_one"));
    mock_profile_get(
        &mut provider,
        200,
        &fixture("profile_get_wrong_certificate"),
    );
    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(
        &observation,
        Observation::Mismatch { port } if *port == PortName::parse("certificate").unwrap()
    ));
}

#[test]
fn read_reports_mismatch_certificate_when_two_certificates_are_related() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_one"));
    mock_profile_get(&mut provider, 200, &fixture("profile_get_two_certificates"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(
        &observation,
        Observation::Mismatch { port } if *port == PortName::parse("certificate").unwrap()
    ));
}

#[test]
fn read_reports_conflict_when_the_profile_state_is_invalid() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_one"));
    mock_profile_get(&mut provider, 200, &fixture("profile_get_invalid"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
}

#[test]
fn read_reports_conflict_when_the_profile_is_expired() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_one"));
    mock_profile_get(&mut provider, 200, &fixture("profile_get_expired"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
}

// ---------------------------------------------------------------------
// The create body (acceptance test 6)
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_with_exactly_the_pinned_body_and_no_devices_key() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_empty"));
    // An *exact* JSON match (`json_body`, not the partial matcher), so a
    // stray `devices` key -- Apple's schema names it as a third possible
    // relationship -- would fail this mock rather than pass unnoticed.
    let create_mock = provider
        .mock("POST", "/v1/profiles")
        .match_body(willikins_providers_http::testing::json_body(
            serde_json::json!({
                "data": {
                    "type": "profiles",
                    "attributes": {
                        "name": "willikins-example-profile",
                        "profileType": "IOS_APP_STORE"
                    },
                    "relationships": {
                        "bundleId": {"data": {"type": "bundleIds", "id": "T6G4XCV345"}},
                        "certificates": {"data": [{"type": "certificates", "id": "C3RT1F1CATE1"}]}
                    }
                }
            }),
        ))
        .with_status(201)
        .with_body(fixture("profile_post_created").to_string())
        .expect(1)
        .create();

    let tool = AppstoreProfileEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    assert_eq!(profile_id_of(&ensured.outputs), "PR0F1LE1D0001");
    create_mock.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_follows_up_with_a_get_when_the_201_omits_profile_content() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_empty"));
    mock_profile_post(
        &mut provider,
        201,
        &fixture("profile_post_created_no_content"),
    );
    let get_mock = mock_profile_get(&mut provider, 200, &fixture("profile_get_present_healthy"));

    let tool = AppstoreProfileEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    let content = content_of(&ensured.outputs);
    let exposed = content.expose(&token).to_string();
    assert_eq!(
        exposed,
        "d2lsbGlraW5zLWV4YW1wbGUtcHJvZmlsZS1jb250ZW50LWV4YW1wbGU="
    );
    get_mock.assert();
}

// ---------------------------------------------------------------------
// Ambiguous create (acceptance test 7)
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_converges_when_a_create_500_is_followed_by_present_on_reread() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    // First read (inside `ensure`, before the create attempt): absent.
    let list_mock = provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("profile_list_empty").to_string())
        .expect(1)
        .create();
    mock_profile_post(&mut provider, 500, &fixture("error_5xx"));
    // The create's own re-read finds it now present.
    let reread_mock = provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("profile_list_one").to_string())
        .expect(1)
        .create();
    mock_profile_get(&mut provider, 200, &fixture("profile_get_present_healthy"));

    let tool = AppstoreProfileEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    list_mock.assert();
    reread_mock.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_propagates_the_original_error_when_a_create_500_is_still_absent_on_reread() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_empty"));
    mock_profile_post(&mut provider, 500, &fixture("error_5xx"));

    let tool = AppstoreProfileEnsure::new(provider.url());
    let token = SinkToken::new();
    let err: ToolError = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_reports_conflict_when_a_create_500_is_a_conflict_on_reread() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let list_mock = provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("profile_list_empty").to_string())
        .expect(1)
        .create();
    mock_profile_post(&mut provider, 500, &fixture("error_5xx"));
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("profile_list_two").to_string())
        .expect(1)
        .create();

    let tool = AppstoreProfileEnsure::new(provider.url());
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    list_mock.assert();
}

// ---------------------------------------------------------------------
// `ensure` refuses when the identifier is not yet registered
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_to_create_when_the_identifier_is_not_registered() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_empty"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

// ---------------------------------------------------------------------
// 401/403
// ---------------------------------------------------------------------

#[test]
fn read_maps_a_401_on_the_bundle_id_list_to_unauthenticated() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(fixture("error_403").to_string())
        .create();
    let tool = AppstoreProfileEnsure::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(err.message, willikins_providers_http::UNAUTHENTICATED);
}

#[test]
fn read_maps_a_403_on_the_profile_get_to_missing_permission() {
    let mut provider = MockProvider::start();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    mock_profile_list(&mut provider, &fixture("profile_list_one"));
    mock_profile_get(&mut provider, 403, &fixture("error_403"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(err.message, willikins_providers_http::MISSING_PERMISSION);
}
