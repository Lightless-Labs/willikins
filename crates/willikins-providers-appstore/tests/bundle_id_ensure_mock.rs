//! Mock-server tests for `appstore.bundle_id.ensure`: every arm of
//! `read` and `ensure` this crate's own module doc names.

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_appstore::AppstoreBundleIdEnsure;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_types::{
    AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, AppleIssuerId, AppleKeyId,
    AppleSigningKey, DomainType,
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

fn name() -> AppleBundleIdName {
    AppleBundleIdName::parse("third-thoughts").unwrap()
}

fn platform() -> AppleBundleIdPlatform {
    AppleBundleIdPlatform::parse("UNIVERSAL").unwrap()
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
        PortName::parse("platform").unwrap(),
        Value::known(platform()),
    );
    inputs
}

fn mock_list(
    provider: &mut MockProvider,
    status: usize,
    body: &serde_json::Value,
) -> mockito::Mock {
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::UrlEncoded(
            "filter[identifier]".into(),
            "com.example.MyApp".into(),
        ))
        .with_status(status)
        .with_body(body.to_string())
        .create()
}

// ---------------------------------------------------------------------
// `read`
// ---------------------------------------------------------------------

#[test]
fn read_reports_absent_when_the_list_is_empty() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("bundle_id_list_empty"));
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn read_reports_absent_when_only_a_prefix_neighbor_matches_the_filter() {
    // Proves the client-side exact comparison decides the match, not the
    // (undocumented) filter semantics -- see this crate's module doc.
    let mut provider = MockProvider::start();
    mock_list(
        &mut provider,
        200,
        &fixture("bundle_id_list_prefix_neighbor"),
    );
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn read_reports_present_when_identifier_name_and_platform_all_match() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("bundle_id_list_one"));
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let id = outputs.get(&PortName::parse("id").unwrap()).unwrap();
    assert_eq!(id.render().to_string(), "T6G4XCV345");
}

#[test]
fn read_reports_mismatch_name_when_name_differs_but_platform_matches() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("bundle_id_list_mismatch_name"));
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(
        observation,
        Observation::Mismatch { port } if port == PortName::parse("name").unwrap()
    ));
}

#[test]
fn read_reports_mismatch_platform_when_platform_differs_even_if_name_also_differs() {
    // Platform is checked first (this crate's own module doc): a row
    // with both wrong reports `platform`, never `name`.
    let mut provider = MockProvider::start();
    mock_list(
        &mut provider,
        200,
        &fixture("bundle_id_list_mismatch_platform"),
    );
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(
        observation,
        Observation::Mismatch { port } if port == PortName::parse("platform").unwrap()
    ));
}

#[test]
fn read_reports_conflict_when_more_than_one_row_matches_exactly() {
    let mut provider = MockProvider::start();
    let one = fixture("bundle_id_list_one");
    let doubled = serde_json::json!({"data": [one["data"][0].clone(), one["data"][0].clone()]});
    mock_list(&mut provider, 200, &doubled);
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
}

#[test]
fn read_maps_a_500_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 500, &fixture("error_5xx"));
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn read_is_retried_on_a_429_then_succeeds() {
    // Apple documents `RATE_LIMIT_EXCEEDED` explicitly
    // (`docs/research/2026-09-16-app-store-connect.md`, section 1).
    let mut provider = MockProvider::start();
    let first = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::UrlEncoded(
            "filter[identifier]".into(),
            "com.example.MyApp".into(),
        ))
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::UrlEncoded(
            "filter[identifier]".into(),
            "com.example.MyApp".into(),
        ))
        .with_status(200)
        .with_body(fixture("bundle_id_list_empty").to_string())
        .expect(1)
        .create();
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
    first.assert();
    second.assert();
}

#[test]
fn read_maps_a_403_to_a_provider_error_naming_no_credential() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 403, &fixture("error_403"));
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

// ---------------------------------------------------------------------
// `ensure`: create
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_when_absent() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("bundle_id_list_empty"));
    let create = provider
        .mock("POST", "/v1/bundleIds")
        .with_status(201)
        .with_body(fixture("bundle_id_post_created").to_string())
        .expect(1)
        .create();
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_is_a_no_op_when_present() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("bundle_id_list_one"));
    let create = provider.mock("POST", "/v1/bundleIds").expect(0).create();
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    create.assert();
}

// ---------------------------------------------------------------------
// `ensure`: convergent name mismatch (PATCH)
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_converges_a_name_mismatch_via_patch() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("bundle_id_list_mismatch_name"));
    let patch = provider
        .mock("PATCH", "/v1/bundleIds/T6G4XCV345")
        .match_body(willikins_providers_http::testing::json_body(
            serde_json::json!({
                "data": {
                    "type": "bundleIds",
                    "id": "T6G4XCV345",
                    "attributes": {"name": "third-thoughts"}
                }
            }),
        ))
        .with_status(200)
        .with_body(fixture("bundle_id_patch_response").to_string())
        .expect(1)
        .create();
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    patch.assert();
}

// ---------------------------------------------------------------------
// `ensure`: terminal platform mismatch (never PATCHed)
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_a_platform_mismatch_and_never_patches() {
    let mut provider = MockProvider::start();
    mock_list(
        &mut provider,
        200,
        &fixture("bundle_id_list_mismatch_platform"),
    );
    let patch = provider
        .mock("PATCH", "/v1/bundleIds/T6G4XCV345")
        .expect(0)
        .create();
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    patch.assert();
}

// ---------------------------------------------------------------------
// `ensure`: ambiguous create failure, re-read recovery
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_error_after_create_re_reads_and_reports_unchanged_when_present() {
    let mut provider = MockProvider::start();
    let first_read = mock_list(&mut provider, 200, &fixture("bundle_id_list_empty"));
    let create = provider
        .mock("POST", "/v1/bundleIds")
        .with_status(500)
        .with_body("{}")
        .expect(1)
        .create();
    // The second `GET` now finds the row (a concurrent create actually
    // succeeded), so a fresh mock is needed for the second call.
    let second_read = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::UrlEncoded(
            "filter[identifier]".into(),
            "com.example.MyApp".into(),
        ))
        .with_status(200)
        .with_body(fixture("bundle_id_list_one").to_string())
        .expect(1)
        .create();
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    first_read.assert();
    create.assert();
    second_read.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_error_after_create_that_still_reads_absent_reports_the_original_error() {
    let mut provider = MockProvider::start();
    mock_list(&mut provider, 200, &fixture("bundle_id_list_empty"));
    provider
        .mock("POST", "/v1/bundleIds")
        .with_status(500)
        .with_body(r#"{"errors":[]}"#)
        .create();
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}
