//! Mock-server tests for `appstore.bundle_id_capability.ensure`.

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_appstore::AppstoreBundleIdCapabilityEnsure;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_types::{
    AppleBundleIdentifier, AppleCapabilitySetting, AppleCapabilityType, AppleIssuerId, AppleKeyId,
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

fn inputs_with_setting(capability: &str, setting: &str) -> willikins_core::Inputs {
    let mut inputs = inputs_for(capability);
    inputs.insert(
        PortName::parse("setting").unwrap(),
        Value::known(AppleCapabilitySetting::parse(setting).unwrap()),
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

/// Milestone 3e, T3-blocking fix: `plan` reads every node whose key ports
/// are known, and `appstore.bundle_id.ensure`'s own `Absent` predicts its
/// `identifier` output, so a capability node bound from
/// `steps.app_id.identifier` is read before the bundle id is ever
/// created. `read` must report `Absent` for a not-yet-registered parent
/// -- exactly the precedent `appstore.profile.ensure` already set for an
/// unregistered identifier -- so a document that registers an identifier
/// and enables a capability on it in the same run plans cleanly.
#[test]
fn read_reports_absent_when_the_parent_bundle_id_does_not_exist() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("bundle_id_list_empty").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// `ensure` is where the missing parent is still a hard `NotFound`: this
/// tool cannot create a bundle id, only `appstore.bundle_id.ensure` does,
/// so an apply-time attempt against a genuinely unregistered identifier
/// must refuse -- and must never `POST`, exactly as before this fix.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_when_the_parent_bundle_id_does_not_exist_and_never_posts() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("bundle_id_list_empty").to_string())
        .create();
    let create = provider
        .mock("POST", "/v1/bundleIdCapabilities")
        .expect(0)
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let token = SinkToken::new();
    let err = tool
        .ensure(&inputs_for("PUSH_NOTIFICATIONS"), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert!(err.message.contains("com.example.MyApp"), "{}", err.message);
    create.assert();
}

#[test]
fn read_reports_absent_when_the_capability_is_not_in_the_list() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
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
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_push").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// Milestone 3e made every capability read parse every row's
/// `settings`, where before it read `capabilityType` alone. A row this
/// read never asks about -- here an `ICLOUD` row whose settings omit
/// `enabled`, carry a `null` `options` and a keyless entry -- must not
/// turn a `HEALTHKIT` read into a parse failure.
#[test]
fn read_of_a_capability_is_unaffected_by_another_rows_unexpected_settings_shape() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_an_unparsed_setting_shape").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_for("HEALTHKIT"))
        .unwrap_or_else(|err| panic!("{:?}: {}", err.kind, err.message));
    assert!(matches!(observation, Observation::Present(_)));
}

// ---------------------------------------------------------------------
// `read`/`ensure`: the capability list paginates
// ---------------------------------------------------------------------

/// Apple's row order for `GET .../bundleIdCapabilities` was observed live
/// to change between reads (the milestone 3e capability read fixes'
/// independent review, 2026-09-28). Before this fix the client read a
/// single unpaginated page, so a capability sitting past page one would
/// read `Absent`. Page one here carries the `HEALTHKIT`/`ICLOUD` rows
/// (reusing `capabilities_list_with_an_unparsed_setting_shape`, which
/// already has no `PUSH_NOTIFICATIONS` row) plus a `links.next` cursor;
/// page two (`capabilities_list_with_push`) is where the requested
/// capability actually sits.
#[test]
fn read_finds_the_requested_capability_on_a_later_page() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    let mut page_one = fixture("capabilities_list_with_an_unparsed_setting_shape");
    page_one["links"] = serde_json::json!({
        "next": format!(
            "{}/v1/bundleIds/T6G4XCV345/bundleIdCapabilities?cursor=PAGE2&limit=200",
            provider.url()
        ),
    });
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Exact("limit=200".into()))
        .with_status(200)
        .with_body(page_one.to_string())
        .create();
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::UrlEncoded(
            "cursor".into(),
            "PAGE2".into(),
        ))
        .with_status(200)
        .with_body(fixture("capabilities_list_with_push").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// The same two-page list, exercised through `ensure`: a capability the
/// unstable row order put on page two must converge with no `POST` at
/// all, never a duplicate create.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_never_posts_when_the_requested_capability_is_on_a_later_page() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    let mut page_one = fixture("capabilities_list_with_an_unparsed_setting_shape");
    page_one["links"] = serde_json::json!({
        "next": format!(
            "{}/v1/bundleIds/T6G4XCV345/bundleIdCapabilities?cursor=PAGE2&limit=200",
            provider.url()
        ),
    });
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Exact("limit=200".into()))
        .with_status(200)
        .with_body(page_one.to_string())
        .create();
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::UrlEncoded(
            "cursor".into(),
            "PAGE2".into(),
        ))
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

/// A capability list whose `links.next` never stops (each page pointing
/// at another page with the same shape) must not page forever: past a
/// fixed cap this refuses, exactly like `list_bundle_ids`'s own cap.
#[test]
fn read_refuses_past_the_page_cap_rather_than_spinning() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    let mut looping_page = fixture("capabilities_list_empty");
    looping_page["links"] = serde_json::json!({
        "next": format!(
            "{}/v1/bundleIds/T6G4XCV345/bundleIdCapabilities?cursor=AGAIN&limit=200",
            provider.url()
        ),
    });
    // `MAX_PAGES` is private to `client.rs`; pin its value here (50) rather
    // than merely "eventually refuses" -- `.expect(50)` fails the test if
    // the client stops short or keeps going past it.
    let looping = provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(looping_page.to_string())
        .expect(50)
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let err = tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.contains("pages"), "{}", err.message);
    looping.assert();
}

/// `links.next` is an absolute URL Apple sends; a response naming a
/// different bundle id's path (and, separately, a different host)
/// must never be followed as-is -- only its query string is reattached
/// to this client's own fixed capabilities path. Proven the same way
/// `bundle_id_ensure_mock.rs`'s own pagination test is proven: the second
/// page is only reachable through this client's own path, never the one
/// the fixture names.
#[test]
fn read_reattaches_only_the_query_string_never_the_host_or_path_links_next_names() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    let mut page_one = fixture("capabilities_list_with_an_unparsed_setting_shape");
    // A different host AND a different bundle id's path -- if this client
    // ever followed `next` verbatim it would either fail to connect (the
    // host does not exist) or hit another bundle id's capabilities list
    // (a path this test never mocks, so mockito would 501 it).
    page_one["links"] = serde_json::json!({
        "next": "https://evil.example.com/v1/bundleIds/OTHERID99/bundleIdCapabilities?cursor=PAGE2&limit=200",
    });
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Exact("limit=200".into()))
        .with_status(200)
        .with_body(page_one.to_string())
        .create();
    // Only reachable if the client re-attached the query string to ITS OWN
    // path (this bundle id, `T6G4XCV345`) rather than the evil host or the
    // `OTHERID99` path `next` names.
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::UrlEncoded(
            "cursor".into(),
            "PAGE2".into(),
        ))
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
        .match_query(mockito::Matcher::Any)
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
        .match_query(mockito::Matcher::Any)
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
            .match_query(mockito::Matcher::Any)
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
        .match_query(mockito::Matcher::Any)
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

// ---------------------------------------------------------------------
// `setting`: the pairing refusal, before any request
// ---------------------------------------------------------------------

#[test]
fn read_refuses_data_protection_with_no_setting_and_makes_no_request() {
    let mut provider = MockProvider::start();
    let bundle_id_lookup = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let err = tool.read(&inputs_for("DATA_PROTECTION")).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
    assert!(
        err.message.contains("DATA_PROTECTION_PERMISSION_LEVEL"),
        "{}",
        err.message
    );
    bundle_id_lookup.assert();
}

#[test]
fn read_refuses_apple_id_auth_with_a_data_protection_setting_and_makes_no_request() {
    let mut provider = MockProvider::start();
    let bundle_id_lookup = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let err = tool
        .read(&inputs_with_setting(
            "APPLE_ID_AUTH",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
    bundle_id_lookup.assert();
}

#[test]
fn read_refuses_healthkit_given_any_setting_and_makes_no_request() {
    let mut provider = MockProvider::start();
    let bundle_id_lookup = provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let err = tool
        .read(&inputs_with_setting(
            "HEALTHKIT",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
    assert!(err.message.contains("HEALTHKIT"), "{}", err.message);
    bundle_id_lookup.assert();
}

// ---------------------------------------------------------------------
// `read`: a settings-aware capability
// ---------------------------------------------------------------------

/// The observed shape (2026-09-28's second live capability cycle): a row
/// with no `enabled` field at all, listing exactly the selected option.
/// The listed option *is* the selection.
#[test]
fn read_reports_present_when_the_requested_option_is_the_only_one_listed() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_with_setting(
            "DATA_PROTECTION",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// Observed shape: exactly one option listed, but it is not the one
/// requested.
#[test]
fn read_reports_mismatch_when_a_different_option_is_listed() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection_mismatched").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_with_setting(
            "DATA_PROTECTION",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap();
    match observation {
        Observation::Mismatch { port } => {
            assert_eq!(port, PortName::parse("setting").unwrap());
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }
}

#[test]
fn read_reports_mismatch_when_no_option_is_listed_at_all() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection_no_settings").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_with_setting(
            "DATA_PROTECTION",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap();
    assert!(matches!(observation, Observation::Mismatch { .. }));
}

/// Observed shape: two options listed under the same key, neither
/// carrying `enabled` -- ambiguous, so `Mismatch`, never a guess at which
/// one is the real selection.
#[test]
fn read_reports_mismatch_when_two_options_are_listed_with_no_enabled_field() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection_two_listed").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_with_setting(
            "DATA_PROTECTION",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap();
    assert!(matches!(observation, Observation::Mismatch { .. }));
}

/// The same two-listed row, with the option listed *first* requested: the
/// test above requests the second one, so a rule that looked only at the
/// first listed option (`[only, ..]` in place of `[only]`) passed it --
/// the independent review of 2026-09-28 made exactly that mutation and it
/// survived. Apple's row order is not stable across reads, so option
/// order must not decide the answer either way.
#[test]
fn read_reports_mismatch_when_two_options_are_listed_and_the_first_is_requested() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection_two_listed").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_with_setting(
            "DATA_PROTECTION",
            "DATA_PROTECTION_PERMISSION_LEVEL=COMPLETE_PROTECTION",
        ))
        .unwrap();
    assert!(matches!(observation, Observation::Mismatch { .. }));
}

/// The one remaining fixture carrying `enabled` fields at all (superseded
/// by the observed key-only shape above, kept so the enabled-field branch
/// of the rule stays exercised): decision (d)'s "exactly one" makes a row
/// with two enabled options `Mismatch { setting }`, not `Present`, even
/// though the requested option is among the enabled ones.
#[test]
fn read_reports_mismatch_when_another_option_is_enabled_beside_the_requested_one() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection_two_enabled").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_with_setting(
            "DATA_PROTECTION",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap();
    match observation {
        Observation::Mismatch { port } => {
            assert_eq!(port, PortName::parse("setting").unwrap());
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }
}

/// The `enabled`-field branch's one `Present` case: every option carries
/// `enabled`, and exactly the requested one is `true`.
#[test]
fn read_reports_present_when_every_option_carries_enabled_and_only_the_requested_one_is_true() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection_one_enabled").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_with_setting(
            "DATA_PROTECTION",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// A row mixing the two read-back shapes: the requested option marked
/// `enabled: true` beside a second option listed by `key` alone. Under
/// the `enabled` rule the bare option is unselected (one selection);
/// under the observed key-only rule a listed option *is* selected (two
/// selections). The two rules disagree, so the row is ambiguous and must
/// read `Mismatch { setting }`, never `Present` -- a wrong `Present`
/// plans `NoOp` silently, a wrong `Mismatch` stops and asks. Mock-only:
/// the fake stores one option per (identifier, capability) and cannot
/// express this row (`fake_agrees_with_live.rs` states the same limit).
#[test]
fn read_reports_mismatch_when_an_enabled_option_sits_beside_a_bare_listed_one() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection_mixed_enabled").to_string())
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let observation = tool
        .read(&inputs_with_setting(
            "DATA_PROTECTION",
            "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
        ))
        .unwrap();
    match observation {
        Observation::Mismatch { port } => {
            assert_eq!(port, PortName::parse("setting").unwrap());
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// `ensure`: creating with a setting, and the terminal `Mismatch`
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_data_protection_with_the_requested_setting() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_empty").to_string())
        .create();
    let create = provider
        .mock("POST", "/v1/bundleIdCapabilities")
        .match_body(willikins_providers_http::testing::json_body(
            serde_json::json!({
                "data": {
                    "type": "bundleIdCapabilities",
                    "attributes": {
                        "capabilityType": "DATA_PROTECTION",
                        "settings": [{
                            "key": "DATA_PROTECTION_PERMISSION_LEVEL",
                            "options": [{"key": "PROTECTED_UNTIL_FIRST_USER_AUTH", "enabled": true}]
                        }]
                    },
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
        .ensure(
            &inputs_with_setting(
                "DATA_PROTECTION",
                "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
            ),
            &token,
        )
        .unwrap();
    assert!(ensured.changed);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_a_setting_mismatch_with_conflict_and_never_posts() {
    let mut provider = MockProvider::start();
    mock_bundle_id_lookup(&mut provider);
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/bundleIdCapabilities")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(fixture("capabilities_list_with_data_protection_mismatched").to_string())
        .create();
    let create = provider
        .mock("POST", "/v1/bundleIdCapabilities")
        .expect(0)
        .create();
    let tool = AppstoreBundleIdCapabilityEnsure::new(provider.url());
    let token = SinkToken::new();
    let err = tool
        .ensure(
            &inputs_with_setting(
                "DATA_PROTECTION",
                "DATA_PROTECTION_PERMISSION_LEVEL=PROTECTED_UNTIL_FIRST_USER_AUTH",
            ),
            &token,
        )
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    create.assert();
}
