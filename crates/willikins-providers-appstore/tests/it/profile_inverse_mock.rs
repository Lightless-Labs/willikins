//! Milestone 3m task P5: mock tests for `appstore.profile.ensure`'s
//! inverse (decisions 4, 7; acceptance 20). Each test pins the exact
//! sparse query the standing read issues and, where decision 7's own
//! row refuses before any `DELETE`, that the delete was never issued at
//! all (trust boundary 6: "all reads before any delete").

use willikins_core::rollback::CreationWindow;
use willikins_core::{
    Created, Inputs, Loss, Outputs, PortName, Standing, Timestamp, Tool, ToolErrorKind, Value,
};
use willikins_providers_appstore::AppstoreProfileEnsure;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_types::{
    AppleBundleIdentifier, AppleIssuerId, AppleKeyId, AppleProfileId, AppleProfileName,
    AppleSigningKey, DomainType, TeardownToken,
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

fn profile_id() -> AppleProfileId {
    AppleProfileId::parse("PR0F1LE1D0001").unwrap()
}

/// `[2026-01-01T00:00:00Z, 2026-01-01T00:10:00Z]`, inclusive both ends.
fn window() -> CreationWindow {
    CreationWindow {
        start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
        end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
    }
}

fn created_inputs() -> Inputs {
    let mut inputs = Inputs::new();
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
    inputs
}

fn created_outputs() -> Outputs {
    let mut outputs = Outputs::new();
    outputs.insert(
        PortName::parse("profile").unwrap(),
        Value::known(profile_id()),
    );
    outputs
}

/// A [`Created`] over `inputs`/`outputs`/`accept`, the fixed [`window`],
/// and no riders (this tool's own standing rule reads none, decision 7).
fn created<'a>(inputs: &'a Inputs, outputs: &'a Outputs, accept: &'a [Loss]) -> Created<'a> {
    Created {
        inputs,
        outputs,
        window: window(),
        settled_by: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
        accept,
        riders: &[],
    }
}

fn standing_path() -> &'static str {
    "/v1/profiles/PR0F1LE1D0001?fields[profiles]=name,createdDate,bundleId"
}

fn delete_path() -> &'static str {
    "/v1/profiles/PR0F1LE1D0001"
}

fn mock_bundle_id_list(provider: &mut MockProvider, body: &serde_json::Value) -> mockito::Mock {
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(body.to_string())
        .create()
}

/// A standing-read body matching [`identifier`]/[`name`], inside
/// [`window`], with its `bundleId` relationship pointing at
/// `bundle_id_list_one`'s own row (`T6G4XCV345`) -- the `AsCreated` case.
/// `createdDate`'s own shape (`.000+00:00`, not a bare `Z`) matches
/// `profile_get_present_healthy.json`'s own `expirationDate`, Apple's
/// actual wire shape for a `date-time` attribute on this resource.
fn as_created_body() -> serde_json::Value {
    serde_json::json!({
        "data": {
            "type": "profiles",
            "id": "PR0F1LE1D0001",
            "attributes": {
                "name": "willikins-example-profile",
                "createdDate": "2026-01-01T00:05:00.000+00:00",
            },
            "relationships": {
                "bundleId": {"data": {"type": "bundleIds", "id": "T6G4XCV345"}}
            }
        }
    })
}

/// `AppstoreProfileEnsure` itself implements nothing beyond `Tool`; its
/// `inverse()` is the one under test.
fn standing(tool: &AppstoreProfileEnsure, created: &Created<'_>) -> Standing {
    Tool::inverse(tool)
        .expect("appstore.profile.ensure declares Delete, so inverse() is Some")
        .standing(created)
        .expect("standing never fails against a well-formed mock response")
}

#[test]
fn standing_pins_the_exact_sparse_query_and_reports_as_created() {
    let mut provider = MockProvider::start();
    let standing_mock = provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(as_created_body().to_string())
        .expect(1)
        .create();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
    standing_mock.assert();
}

#[test]
fn standing_reports_gone_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", standing_path())
        .with_status(404)
        .with_body(serde_json::json!({"errors": []}).to_string())
        .create();
    let bundle_id_lookup = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
    bundle_id_lookup.assert();
}

#[test]
fn standing_reports_not_ours_when_the_name_differs() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["data"]["attributes"]["name"] = serde_json::json!("some-other-profile");
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let bundle_id_lookup = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    bundle_id_lookup.assert();
}

#[test]
fn standing_reports_not_ours_when_the_bundle_id_relationship_differs() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["data"]["relationships"]["bundleId"]["data"]["id"] = serde_json::json!("SOMEOTHERID");
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_the_bundle_id_relationship_is_missing_outright() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["data"]
        .as_object_mut()
        .unwrap()
        .remove("relationships");
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_created_date_is_one_second_outside_the_window() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["data"]["attributes"]["createdDate"] = serde_json::json!("2026-01-01T00:10:01.000+00:00");
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

/// The lower edge, mirroring the upper-edge test above: one second before
/// the window's own start is `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_date_is_one_second_before_the_window() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["data"]["attributes"]["createdDate"] = serde_json::json!("2025-12-31T23:59:59.000+00:00");
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

/// The same instant as the window's own end is still inside it (an
/// inclusive window): `AsCreated`, never `NotOurs`.
#[test]
fn standing_reports_as_created_when_created_date_is_exactly_the_windows_end() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["data"]["attributes"]["createdDate"] = serde_json::json!("2026-01-01T00:10:00.000+00:00");
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

/// A response that omits `createdDate` outright fails closed: never "in
/// the window", always `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_date_is_missing() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["data"]["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("createdDate");
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_is_a_noop_when_already_gone() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", standing_path())
        .with_status(404)
        .with_body(serde_json::json!({"errors": []}).to_string())
        .create();
    let delete = provider.mock("DELETE", delete_path()).expect(0).create();
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap();
    assert!(deleted.already_gone);
    delete.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_pins_the_delete_profiles_request() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    mock_bundle_id_list(&mut provider, &fixture("bundle_id_list_one"));
    let delete = provider
        .mock("DELETE", delete_path())
        .with_status(204)
        .expect(1)
        .create();
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap();
    assert!(!deleted.already_gone);
    delete.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_refuses_as_a_conflict_when_not_ours_and_never_deletes() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["data"]["attributes"]["name"] = serde_json::json!("some-other-profile");
    provider
        .mock("GET", standing_path())
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let delete = provider.mock("DELETE", delete_path()).expect(0).create();
    let tool = AppstoreProfileEnsure::new(provider.url());
    let inputs = created_inputs();
    let outputs = created_outputs();
    let token = TeardownToken::new();
    let err = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    delete.assert();
}

/// Decision 4's own reversal shape (milestone 3m task P5): `Delete`, no
/// container, no declared loss.
#[test]
fn reversal_declares_delete_with_no_within_and_no_losses() {
    let tool = AppstoreProfileEnsure::new("http://127.0.0.1:1");
    let willikins_core::Reversal::Delete {
        needs,
        within,
        losses,
    } = Tool::reversal(&tool)
    else {
        panic!("appstore.profile.ensure must declare Reversal::Delete");
    };
    assert_eq!(needs, ["issuer_id", "key_id", "key", "identifier", "name"]);
    assert!(within.is_empty());
    assert!(losses.is_empty());
}
