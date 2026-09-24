//! End-to-end proof for the App Store Connect signing-profile document:
//! `workflows/appstore-signing-profile-from-doppler.yaml` (the credential
//! chained out of Doppler, a bundle identifier, a certificate selection,
//! a profile, and the profile's content written into Doppler through
//! `doppler.secret.set`) and its negative fixtures. Acceptance
//! test 11 (`docs/plans/2026-09-22-milestone-3c-app-store-signing.md`),
//! and milestone 3d's acceptance tests 5 (the real half) and 11 and its
//! equivalence item 3 (`docs/plans/2026-09-23-milestone-3d-conversions.md`):
//! the document now names the profile after its bundle identifier through
//! the one registered conversion, and plans and applies exactly as its
//! two-input predecessor, kept verbatim below as [`PREDECESSOR`], did.

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

/// `workflows/appstore-signing-profile-from-doppler.yaml` as it stood
/// before milestone 3d, verbatim: the profile's name was a second input,
/// `profile_name`, that had to repeat the identifier. Kept here, not under
/// `workflows/fixtures/` (which holds one document per negative case), for
/// equivalence item 3 and the fact-not-convention test.
const PREDECESSOR: &str = r#"# The operator's own chain, extended one step past
# appstore-bundle-id-from-doppler.yaml: the same App Store Connect
# credential, resolved out of Doppler the identical way; a bundle
# identifier; a distribution certificate selected by
# `appstore.certificate.get` (never pasted as a literal id -- milestone
# 3c's task-2 Addendum records why that is no longer *enforced* by
# `check`, only conventional, and this document is the convention);
# `appstore.profile.ensure` relating the two; and the profile's content
# written into a Doppler secret through `doppler.secret.set`, whose
# `config` port only accepts a derived-only binding -- hence the
# `doppler.config.ensure` node, even though `destination_project`'s
# config may already exist.
#
# `docs/plans/2026-09-22-milestone-3c-app-store-signing.md`, "The
# positive document".
name: appstore-signing-profile-from-doppler
description: Resolve the App Store Connect credential from Doppler, produce an IOS_APP_STORE profile, and store its content in Doppler.
inputs:
  config: { type: DopplerConfig, description: "Doppler config holding the App Store Connect credential, e.g. app-store-connect/prd" }
  identifier: { type: AppleBundleIdentifier, description: "The bundle identifier to sign, e.g. com.example.MyApp" }
  bundle_name: { type: AppleBundleIdName, description: "The bundle id's human-written name" }
  platform: { type: AppleBundleIdPlatform, description: "IOS, MAC_OS, or UNIVERSAL" }
  certificate_type: { type: AppleCertificateType, description: "DISTRIBUTION or IOS_DISTRIBUTION" }
  serial_number: { type: AppleCertificateSerial, description: "The distribution certificate's serial number" }
  profile_name: { type: AppleProfileName, description: "The profile's human-written name" }
  destination_project: { type: DopplerProject, description: "The Doppler project the profile's content is written into" }
  destination_environment: { type: EnvironmentSlug, description: "The Doppler config's environment, e.g. prd" }
  secret_name: { type: SecretName, description: "The Doppler secret name the profile's content is stored under" }
steps:
  issuer_id_text:
    tool: doppler.value.get
    with:
      config: ${{ inputs.config }}
      name: ASC_API_KEY_ISSUER_ID
  issuer_id:
    tool: apple.issuer_id.parse
    with:
      value: ${{ steps.issuer_id_text.value }}
  key_id_text:
    tool: doppler.value.get
    with:
      config: ${{ inputs.config }}
      name: ASC_API_KEY_ID
  key_id:
    tool: apple.key_id.parse
    with:
      value: ${{ steps.key_id_text.value }}
  key_base64:
    tool: doppler.secret.get
    with:
      config: ${{ inputs.config }}
      name: ASC_API_KEY_BASE64
  key_decoded:
    tool: base64.decode
    with:
      value: ${{ steps.key_base64.value }}
  key:
    tool: apple.signing_key.parse
    with:
      value: ${{ steps.key_decoded.value }}
  bundle_id:
    tool: appstore.bundle_id.ensure
    with:
      issuer_id: ${{ steps.issuer_id.value }}
      key_id: ${{ steps.key_id.value }}
      key: ${{ steps.key.value }}
      identifier: ${{ inputs.identifier }}
      name: ${{ inputs.bundle_name }}
      platform: ${{ inputs.platform }}
  certificate:
    tool: appstore.certificate.get
    with:
      issuer_id: ${{ steps.issuer_id.value }}
      key_id: ${{ steps.key_id.value }}
      key: ${{ steps.key.value }}
      certificate_type: ${{ inputs.certificate_type }}
      serial_number: ${{ inputs.serial_number }}
  profile:
    tool: appstore.profile.ensure
    with:
      issuer_id: ${{ steps.issuer_id.value }}
      key_id: ${{ steps.key_id.value }}
      key: ${{ steps.key.value }}
      identifier: ${{ steps.bundle_id.identifier }}
      name: ${{ inputs.profile_name }}
      profile_type: IOS_APP_STORE
      certificate: ${{ steps.certificate.certificate }}
  destination:
    tool: doppler.config.ensure
    with:
      project: ${{ inputs.destination_project }}
      environment: ${{ inputs.destination_environment }}
  store:
    tool: doppler.secret.set
    with:
      config: ${{ steps.destination.config }}
      name: ${{ inputs.secret_name }}
      value: ${{ steps.profile.content }}
outputs:
  profile: ${{ steps.profile.profile }}
"#;

#[test]
fn appstore_profile_name_into_identifier_is_rejected() {
    let workflow = fixture_document("appstore-profile-name-into-identifier.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("an AppleProfileName bound to the identifier port must fail check");
    assert_eq!(
        errors,
        vec![willikins_core::CheckError::TypeMismatch {
            node: willikins_core::NodeName::parse("profile").unwrap(),
            port: willikins_core::PortName::parse("identifier").unwrap(),
            expected: willikins_core::PortType::Exact(TypeRef::scalar(
                TypeName::parse("AppleBundleIdentifier").unwrap()
            )),
            found: TypeRef::scalar(TypeName::parse("AppleProfileName").unwrap()),
        }]
    );
}

/// The profile's `name` port, bound to the bundle id node's `identifier`
/// output, is an edge carrying the one registered conversion; the
/// `identifier` port beside it, bound to the same output, is exact.
#[test]
fn the_profile_name_edge_carries_the_one_conversion() {
    let workflow = document("appstore-signing-profile-from-doppler.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));
    let profile = &checked.types[&willikins_core::NodeName::parse("profile").unwrap()];
    let name = &profile[&willikins_core::PortName::parse("name").unwrap()];
    assert_eq!(name.ty().to_string(), "AppleBundleIdentifier");
    let conversion = name.conversion().expect("the name edge converts");
    assert_eq!(conversion.from().as_str(), "AppleBundleIdentifier");
    assert_eq!(conversion.to().as_str(), "AppleProfileName");
    let identifier = &profile[&willikins_core::PortName::parse("identifier").unwrap()];
    assert!(identifier.conversion().is_none());
}

/// Fact, not convention: nothing in `appstore.profile.ensure` assumes a
/// profile is named after its identifier. The same document with the
/// profile's `name` bound to a separate input still checks and plans, to
/// a profile carrying that other name.
#[test]
fn a_profile_name_other_than_the_identifier_still_checks_and_plans() {
    let workflow = willikins_dsl::parse_document(PREDECESSOR).expect("the predecessor parses");
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state);
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("a different name must check: {errors:?}"));
    let name_edge = &checked.types[&willikins_core::NodeName::parse("profile").unwrap()]
        [&willikins_core::PortName::parse("name").unwrap()];
    assert!(name_edge.conversion().is_none());
    let mut inputs = positive_inputs();
    inputs.insert(
        InputName::parse("profile_name").unwrap(),
        scalar_input("AppleProfileName", "Any Name The Operator Likes"),
    );
    let plan = willikins_core::plan(&checked, &inputs, &catalog)
        .unwrap_or_else(|err| panic!("a different name must plan: {err:?}"));
    let profile = plan
        .nodes
        .iter()
        .find(|node| node.name.as_str() == "profile")
        .expect("the `profile` node planned");
    let name = profile
        .inputs
        .get(&willikins_core::PortName::parse("name").unwrap())
        .unwrap();
    assert_eq!(name.render().to_string(), "Any Name The Operator Likes");
}

/// Equivalence item 3: the one-input document plans and applies exactly
/// as its two-input predecessor did when both inputs carried the same
/// string -- the same plan JSON, the same fingerprint, the same event
/// stream. Twice: once for the seeded identifier, whose profile is
/// already `Present`, and once for the second seeded identifier, which
/// has no profile, so `apply` really creates one through the converted
/// edge.
#[test]
fn the_document_is_equivalent_to_its_two_input_predecessor() {
    let new_workflow = document("appstore-signing-profile-from-doppler.yaml");
    let old_workflow = willikins_dsl::parse_document(PREDECESSOR).expect("the predecessor parses");
    let now = willikins_core::Timestamp::now();
    let run = |workflow: &willikins_core::Workflow, inputs: &IndexMap<InputName, Value>| {
        let catalog = willikins_providers_fake::catalog(seeded_state());
        let checked = willikins_core::check(workflow, &catalog)
            .unwrap_or_else(|errors| panic!("must check: {errors:?}"));
        let plan = willikins_core::plan(&checked, inputs, &catalog)
            .unwrap_or_else(|err| panic!("must plan: {err:?}"));
        let plan_json = serde_json::to_value(&plan).unwrap();
        let fingerprint = serde_json::to_value(plan.fingerprint()).unwrap();
        let mut observer = willikins_core::RecordingObserver::new();
        let approval = willikins_core::Approval::Human {
            approver: willikins_core::PrincipalId::parse("operator").unwrap(),
            at: now,
        };
        willikins_core::apply(&checked, inputs, &catalog, &plan, &approval, &mut observer)
            .unwrap_or_else(|err| panic!("must apply: {err:?}"));
        let events = serde_json::to_value(&observer.events).unwrap();
        (plan_json, fingerprint, events)
    };

    for identifier in [
        "com.example.willikins-demo",
        "com.example.willikins-demo-two",
    ] {
        let mut new_inputs = positive_inputs();
        new_inputs.insert(
            InputName::parse("identifier").unwrap(),
            scalar_input("AppleBundleIdentifier", identifier),
        );
        let mut old_inputs = new_inputs.clone();
        old_inputs.insert(
            InputName::parse("profile_name").unwrap(),
            scalar_input("AppleProfileName", identifier),
        );
        let (new_plan, new_fingerprint, new_events) = run(&new_workflow, &new_inputs);
        let (old_plan, old_fingerprint, old_events) = run(&old_workflow, &old_inputs);
        assert_eq!(new_plan, old_plan, "{identifier}: plan");
        assert_eq!(
            new_fingerprint, old_fingerprint,
            "{identifier}: fingerprint"
        );
        assert_eq!(new_events, old_events, "{identifier}: events");
    }
}
