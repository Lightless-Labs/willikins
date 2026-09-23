//! End-to-end proof for the App Store Connect signing-profile document:
//! `workflows/appstore-signing-profile-from-doppler.yaml` (the credential
//! chained out of Doppler, a bundle identifier, a certificate selection,
//! a profile, and the profile's content written into Doppler through
//! `doppler.secret.set`) and its three negative fixtures. Acceptance
//! test 11 (`docs/plans/2026-09-22-milestone-3c-app-store-signing.md`).

use indexmap::IndexMap;

use willikins_core::{InputName, TypeName, TypeRef, Value};
use willikins_providers_fake::FakeState;

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn document(name: &str) -> willikins_core::Workflow {
    let path = workspace_root().join("workflows").join(name);
    willikins_dsl::load_document(&path).unwrap_or_else(|err| panic!("{name} loads: {err}"))
}

fn fixture_document(name: &str) -> willikins_core::Workflow {
    let path = workspace_root()
        .join("workflows")
        .join("fixtures")
        .join(name);
    willikins_dsl::load_document(&path).unwrap_or_else(|err| panic!("{name} loads: {err}"))
}

fn seeded_state() -> std::sync::Arc<std::sync::Mutex<FakeState>> {
    let path = workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join("appstore-signing-profile.json");
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    let state =
        FakeState::from_json(&json).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    std::sync::Arc::new(std::sync::Mutex::new(state))
}

fn scalar_input(type_name: &str, text: &str) -> Value {
    Value::parse(&TypeRef::scalar(TypeName::parse(type_name).unwrap()), text).unwrap()
}

fn positive_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("config").unwrap(),
        scalar_input("DopplerConfig", "app-store-connect/prd"),
    );
    inputs.insert(
        InputName::parse("identifier").unwrap(),
        scalar_input("AppleBundleIdentifier", "com.example.willikins-demo"),
    );
    inputs.insert(
        InputName::parse("bundle_name").unwrap(),
        scalar_input("AppleBundleIdName", "willikins-demo"),
    );
    inputs.insert(
        InputName::parse("platform").unwrap(),
        scalar_input("AppleBundleIdPlatform", "UNIVERSAL"),
    );
    inputs.insert(
        InputName::parse("certificate_type").unwrap(),
        scalar_input("AppleCertificateType", "DISTRIBUTION"),
    );
    inputs.insert(
        InputName::parse("serial_number").unwrap(),
        scalar_input("AppleCertificateSerial", "7B3F2A9C1D4E5F607182930A1B2C3D4E"),
    );
    inputs.insert(
        InputName::parse("profile_name").unwrap(),
        scalar_input("AppleProfileName", "willikins-demo-profile"),
    );
    inputs.insert(
        InputName::parse("destination_project").unwrap(),
        scalar_input("DopplerProject", "third-thoughts"),
    );
    inputs.insert(
        InputName::parse("destination_environment").unwrap(),
        scalar_input("EnvironmentSlug", "prd"),
    );
    inputs.insert(
        InputName::parse("secret_name").unwrap(),
        scalar_input("SecretName", "APPSTORE_SIGNING_PROFILE"),
    );
    inputs
}

/// The positive document checks cleanly and plans end to end against the
/// seeded fake catalogue: the bundle id and certificate both resolve to
/// their seeded records, the profile resolves `Present` (already seeded,
/// matching every port), and `doppler.secret.set` accepts its
/// derived-only `config` from the freshly planned `destination` node.
/// The profile's `content` output renders redacted at every stage this
/// plan exposes it -- the node's own outputs and the plan overall.
#[test]
fn the_doppler_chain_plans_and_produces_a_redacted_profile_content() {
    let workflow = document("appstore-signing-profile-from-doppler.yaml");
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state);
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));

    let plan = willikins_core::plan(&checked, &positive_inputs(), &catalog)
        .unwrap_or_else(|err| panic!("plan must resolve every node: {err:?}"));

    let profile_output = plan
        .outputs
        .get(&willikins_core::OutputName::parse("profile").unwrap())
        .expect("profile output");
    assert_eq!(profile_output.render().to_string(), "PROFILE1");

    let profile_node = plan
        .nodes
        .iter()
        .find(|node| node.name.as_str() == "profile")
        .expect("the `profile` node planned");
    let content = profile_node
        .outputs
        .get(&willikins_core::PortName::parse("content").unwrap())
        .expect("profile node has a `content` output");
    assert_eq!(
        content.render().to_string(),
        "[REDACTED AppleProfileContent]"
    );

    // The `store` node's own `value` input (bound from `steps.profile.content`)
    // is a secret bound to a secret-accepting port -- `check` already
    // proved that above; nothing in a rendered plan ever exposes it.
    let store_node = plan
        .nodes
        .iter()
        .find(|node| node.name.as_str() == "store")
        .expect("the `store` node planned");
    for (_, value) in store_node.outputs.iter() {
        assert!(!value.render().to_string().contains("willikins-example"));
    }
}

#[test]
fn appstore_profile_development_type_is_rejected() {
    let workflow = fixture_document("appstore-profile-development-type.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("a development profile_type must fail check");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        matches!(&errors[0], willikins_core::CheckError::InvalidLiteral { node, port, .. }
            if node.as_str() == "profile" && port.as_str() == "profile_type"),
        "{:?}",
        errors[0]
    );
}

#[test]
fn appstore_profile_wrong_typed_certificate_is_rejected() {
    let workflow = fixture_document("appstore-profile-wrong-typed-certificate.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("a wrong-typed certificate binding must fail check");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        matches!(&errors[0], willikins_core::CheckError::TypeMismatch { node, port, .. }
            if node.as_str() == "profile" && port.as_str() == "certificate"),
        "{:?}",
        errors[0]
    );
}

#[test]
fn appstore_profile_content_into_template_is_rejected() {
    let workflow = fixture_document("appstore-profile-content-into-template.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("the secret content output bound to a non-secret sink must fail check");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        matches!(&errors[0], willikins_core::CheckError::SecretToNonSecretSink { from, .. }
            if from.0.as_str() == "profile" && from.1.as_str() == "content"),
        "{:?}",
        errors[0]
    );
}
