//! `appstore.profile.ensure` against the response shapes Apple actually
//! sends, rather than the ones the task-2 fixtures assumed. Found by the
//! milestone 3c task-3 adversarial pass
//! (`docs/research/2026-09-22-m3c-adversarial-pass.md`).
//!
//! App Store Connect is a JSON:API server: without an `include`, a
//! resource's relationships carry `links` (and, for a to-many one,
//! `meta.paging`) but **no `data`**. A `POST /v1/profiles` takes no
//! `include`, so its `201` names no certificate ids at all. The task-2
//! client declared `relationships.certificates.data` required on every
//! `profiles` resource, so the real `201` failed to parse -- after Apple
//! had already created the profile. The tool then re-read, found the
//! profile `Present`, and reported `changed: false` for a create; and the
//! live cycle, which records a profile's id only from a successful
//! `ensure`, would have left a throwaway profile on the operator's account
//! that trust boundary 4 forbids it to find by name and delete.

use willikins_core::{Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_appstore::AppstoreProfileEnsure;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_types::{
    AppleBundleIdentifier, AppleCertificateId, AppleIssuerId, AppleKeyId, AppleProfileName,
    AppleProfileType, AppleSigningKey, DomainType,
};

fn fixture(name: &str) -> serde_json::Value {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    load_fixture(&dir, "appstore", name)
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    let mut put = |name: &str, value: Value| {
        inputs.insert(PortName::parse(name).unwrap(), value);
    };
    put(
        "issuer_id",
        Value::known(AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap()),
    );
    put(
        "key_id",
        Value::known(AppleKeyId::parse("2X9R4HXF34").unwrap()),
    );
    put(
        "key",
        Value::known(AppleSigningKey::parse(AppleSigningKey::example()).unwrap()),
    );
    put(
        "identifier",
        Value::known(AppleBundleIdentifier::parse("com.example.MyApp").unwrap()),
    );
    put(
        "name",
        Value::known(AppleProfileName::parse("willikins-example-profile").unwrap()),
    );
    put(
        "profile_type",
        Value::known(AppleProfileType::parse("IOS_APP_STORE").unwrap()),
    );
    put(
        "certificate",
        Value::known(AppleCertificateId::parse("C3RT1F1CATE1").unwrap()),
    );
    inputs
}

fn mock_get(provider: &mut MockProvider, path: &str, body: &serde_json::Value) -> mockito::Mock {
    provider
        .mock("GET", path)
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(body.to_string())
        .create()
}

fn profile_id_of(outputs: &willikins_core::Outputs) -> String {
    outputs
        .get(&PortName::parse("profile").unwrap())
        .unwrap()
        .render()
        .to_string()
}

/// The create's own `201`, links-only relationships and all, is a create:
/// `changed: true`, the id from the response, exactly one `POST`, and no
/// re-read standing in for the answer.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_201_whose_relationships_carry_links_but_no_data_is_a_create() {
    let mut provider = MockProvider::start();
    mock_get(
        &mut provider,
        "/v1/bundleIds",
        &fixture("bundle_id_list_one"),
    );
    mock_get(
        &mut provider,
        "/v1/bundleIds/T6G4XCV345/profiles",
        &fixture("profile_list_empty"),
    );
    let create = provider
        .mock("POST", "/v1/profiles")
        .with_status(201)
        .with_body(fixture("profile_post_created_links_only").to_string())
        .expect(1)
        .create();

    let tool = AppstoreProfileEnsure::new(provider.url());
    let ensured = tool
        .ensure(&inputs(), &SinkToken::new())
        .unwrap_or_else(|err| panic!("a real 201 must parse: {:?}: {}", err.kind, err.message));
    assert!(ensured.changed, "a profile Apple just created is a change");
    assert_eq!(profile_id_of(&ensured.outputs), "PR0F1LE1D0001");
    create.assert();
}

/// A list row that carries a links-only relationship (Apple returning a
/// relationship the `fields[profiles]` list did not name) still reads.
#[test]
fn a_list_row_whose_relationships_carry_links_but_no_data_still_reads() {
    let mut provider = MockProvider::start();
    mock_get(
        &mut provider,
        "/v1/bundleIds",
        &fixture("bundle_id_list_one"),
    );
    mock_get(
        &mut provider,
        "/v1/bundleIds/T6G4XCV345/profiles",
        &fixture("profile_list_one_links_only"),
    );
    mock_get(
        &mut provider,
        "/v1/profiles/PR0F1LE1D0001",
        &fixture("profile_get_present_healthy"),
    );

    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool
        .read(&inputs())
        .unwrap_or_else(|err| panic!("a links-only row must parse: {:?}", err.kind));
    assert!(
        matches!(observation, Observation::Present(_)),
        "{observation:?}"
    );
}

/// An instance read that somehow carries no certificate `data` (the
/// `include` ignored) is a provider error naming what is missing -- never
/// a `Mismatch { certificate }`, which would tell the operator their
/// profile names the wrong certificate when nothing was read at all.
#[test]
fn an_instance_read_with_no_certificate_data_is_a_provider_error_not_a_mismatch() {
    let mut provider = MockProvider::start();
    mock_get(
        &mut provider,
        "/v1/bundleIds",
        &fixture("bundle_id_list_one"),
    );
    mock_get(
        &mut provider,
        "/v1/bundleIds/T6G4XCV345/profiles",
        &fixture("profile_list_one"),
    );
    mock_get(
        &mut provider,
        "/v1/profiles/PR0F1LE1D0001",
        &fixture("profile_post_created_links_only"),
    );

    let tool = AppstoreProfileEnsure::new(provider.url());
    let err = tool
        .read(&inputs())
        .expect_err("no certificate data is not an answer");
    assert_eq!(err.kind, willikins_core::ToolErrorKind::Provider);
    assert!(err.message.contains("certificates"), "{}", err.message);
}
