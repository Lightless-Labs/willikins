//! Mock-server tests for `appstore.certificate.get`: every arm this
//! crate's own module doc names (acceptance test 4,
//! `docs/plans/2026-09-22-milestone-3c-app-store-signing.md`).

use willikins_core::{Observation, PortName, Tool, ToolErrorKind, Value};
use willikins_providers_appstore::AppstoreCertificateGet;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_providers_http::{MISSING_PERMISSION, UNAUTHENTICATED};
use willikins_types::{
    AppleCertificateSerial, AppleCertificateType, AppleIssuerId, AppleKeyId, AppleSigningKey,
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

fn certificate_type() -> AppleCertificateType {
    AppleCertificateType::parse("DISTRIBUTION").unwrap()
}

fn serial_number() -> AppleCertificateSerial {
    AppleCertificateSerial::parse("7B3F2A9C1D4E5F607182930A1B2C3D4E").unwrap()
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
        PortName::parse("certificate_type").unwrap(),
        Value::known(certificate_type()),
    );
    inputs.insert(
        PortName::parse("serial_number").unwrap(),
        Value::known(serial_number()),
    );
    inputs
}

/// Matches `GET /v1/certificates` regardless of its query string -- every
/// test here asks for the same `certificate_type`/`serial_number` pair,
/// so the path alone disambiguates from the pagination test's own second
/// mock (matched by its `cursor` query param instead). `Matcher::Any` is
/// load-bearing: a mockito mock with no `match_query` does not match a
/// request that carries one, and every request this tool sends does --
/// without it every test here read mockito's own `501` (the task-3
/// adversarial pass found this whole file failing as committed).
fn mock_list(
    provider: &mut MockProvider,
    status: usize,
    body: &serde_json::Value,
) -> mockito::Mock {
    provider
        .mock("GET", "/v1/certificates")
        .match_query(mockito::Matcher::Any)
        .with_status(status)
        .with_body(body.to_string())
        .create()
}

// ---------------------------------------------------------------------
// `read`
// ---------------------------------------------------------------------

#[test]
fn read_reports_present_when_exactly_one_certificate_matches() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("certificate_list_one"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let certificate = outputs
        .get(&PortName::parse("certificate").unwrap())
        .unwrap();
    assert_eq!(certificate.render().to_string(), "C3RT1F1CATE1");
}

#[test]
fn read_reports_not_found_when_the_list_is_empty() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("certificate_list_empty"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

/// The filter is proven substring (milestone 3c pre-flight): a row whose
/// `serialNumber` merely *contains* the requested serial as a substring
/// must not count as a match. Proves this crate's client-side exact
/// compare decides, not the (undocumented-semantics) filter.
#[test]
fn read_reports_not_found_when_only_a_substring_neighbor_matches() {
    let mut provider = MockProvider::start();
    mock_list(
        &mut provider,
        200,
        &fixture("certificate_list_substring_neighbor"),
    );
    let tool = AppstoreCertificateGet::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

/// The byte-exact compare checks *both* fields: a row whose serial
/// matches exactly but whose `certificateType` does not must not count.
#[test]
fn read_reports_not_found_when_the_serial_matches_but_the_type_does_not() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("certificate_list_wrong_type"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

#[test]
fn read_reports_conflict_naming_the_count_when_two_certificates_match() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("certificate_list_two"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains('2'), "{}", err.message);
    // Never a certificate's own id.
    assert!(!err.message.contains("C3RT1F1CATE1"), "{}", err.message);
    assert!(!err.message.contains("C3RT1F1CATE3"), "{}", err.message);
}

#[test]
fn read_reports_conflict_when_the_match_is_expired() {
    let mut provider = MockProvider::start();
    let mut inputs = inputs();
    inputs.insert(
        PortName::parse("serial_number").unwrap(),
        Value::known(AppleCertificateSerial::parse("AA11BB22CC33DD44EE55FF6607182930A").unwrap()),
    );
    mock_list(&mut provider, 200, &fixture("certificate_list_expired"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let err = tool.read(&inputs).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
}

#[test]
fn read_reports_conflict_when_the_match_is_deactivated() {
    let mut provider = MockProvider::start();
    let mut inputs = inputs();
    inputs.insert(
        PortName::parse("serial_number").unwrap(),
        Value::known(AppleCertificateSerial::parse("AA11BB22CC33DD44EE55FF6607182930B").unwrap()),
    );
    mock_list(&mut provider, 200, &fixture("certificate_list_deactivated"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let err = tool.read(&inputs).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
}

/// `activated` absent (the pre-flight's own observation on all 5 real
/// certificates) must read the same as `activated: true` -- both
/// `Present` -- never treated as deactivated.
#[test]
fn read_reports_present_when_activated_is_absent() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("certificate_list_one"));
    let tool = AppstoreCertificateGet::new(provider.url());
    assert!(matches!(
        tool.read(&inputs()).unwrap(),
        Observation::Present(_)
    ));
}

#[test]
fn read_reports_present_when_activated_is_explicitly_true() {
    let mut provider = MockProvider::start();
    let mut inputs = inputs();
    inputs.insert(
        PortName::parse("serial_number").unwrap(),
        Value::known(AppleCertificateSerial::parse("AA11BB22CC33DD44EE55FF6607182930C").unwrap()),
    );
    mock_list(
        &mut provider,
        200,
        &fixture("certificate_list_activated_true"),
    );
    let tool = AppstoreCertificateGet::new(provider.url());
    assert!(matches!(
        tool.read(&inputs).unwrap(),
        Observation::Present(_)
    ));
}

/// The substring filter can put the exact match on a later page (mirrors
/// `AppstoreBundleIdEnsure`'s own
/// `read_finds_an_exact_match_that_the_substring_filter_put_on_a_later_page`).
#[test]
fn read_finds_an_exact_match_on_page_two() {
    let mut provider = MockProvider::start();

    let mut page_one = fixture("certificate_list_neighbor_only_page_one");
    page_one["links"] = serde_json::json!({
        "self": format!("{}/v1/certificates", provider.url()),
        "next": format!("{}/v1/certificates?cursor=PAGE2&limit=200", provider.url()),
    });
    provider
        .mock("GET", "/v1/certificates")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(page_one.to_string())
        .create();

    provider
        .mock("GET", "/v1/certificates")
        .match_query(mockito::Matcher::UrlEncoded(
            "cursor".into(),
            "PAGE2".into(),
        ))
        .with_status(200)
        .with_body(fixture("certificate_list_one").to_string())
        .create();

    let tool = AppstoreCertificateGet::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present from page two, got {observation:?}");
    };
    let certificate = outputs
        .get(&PortName::parse("certificate").unwrap())
        .unwrap();
    assert_eq!(certificate.render().to_string(), "C3RT1F1CATE1");
}

#[test]
fn read_maps_a_401_to_unauthenticated() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 401, &fixture("error_403"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(err.message, UNAUTHENTICATED);
}

#[test]
fn read_maps_a_403_to_missing_permission() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 403, &fixture("error_403"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(err.message, MISSING_PERMISSION);
}

/// Every request this tool ever sends is a `GET` -- pinned here for the
/// same reason `tests/no_certificate_writes_guard.rs` pins it for the
/// crate's source: nothing about a *test* proves the guard, but nothing
/// stops a future edit from calling `.post`/`.patch`/`.delete` either.
/// The never-written mock's path is `"/"`, not `/v1/certificates` --
/// still a `"POST"` statement, but naming no certificates path, so it
/// does not trip this crate's own certificate-write guard either.
#[test]
fn every_request_this_tool_sends_is_a_get() {
    let mut provider = MockProvider::start();
    let get_mock = provider
        .mock("GET", "/v1/certificates")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("certificate_list_one").to_string())
        .expect_at_least(1)
        .create();
    let never_written = provider
        .mock("POST", "/")
        .with_status(500)
        .expect(0)
        .create();

    let tool = AppstoreCertificateGet::new(provider.url());
    tool.read(&inputs()).unwrap();

    get_mock.assert();
    never_written.assert();
}

// ---------------------------------------------------------------------
// `ensure` is the identity of `read`
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_is_the_identity_of_read_and_never_reports_changed() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("certificate_list_one"));
    let tool = AppstoreCertificateGet::new(provider.url());
    let token = willikins_core::SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    let certificate = ensured
        .outputs
        .get(&PortName::parse("certificate").unwrap())
        .unwrap();
    assert_eq!(certificate.render().to_string(), "C3RT1F1CATE1");
}
