//! End-to-end proof for the capability/setting pairing refusal
//! (milestone 3e, task 1, acceptance test 2). The negative fixture
//! `workflows/fixtures/appstore-capability-setting-mismatch.yaml` checks
//! cleanly against the fake catalog -- both `capability` and `setting`
//! are individually well-typed literals, and the pairing is a fact about
//! two of one tool's own ports together, which `check` has no per-tool
//! hook to hold (`appstore.bundle_id_capability.ensure`'s own module
//! doc, decision (d)) -- but fails `plan` with `PlanError::Tool` wrapping
//! `ToolErrorKind::Invalid`, naming the capability and the setting key it
//! forbids, raised by `Tool::read` before any provider call is made.
//!
//! Also: the T3-blocking fix's own graph proof (milestone 3e's plan,
//! 2026-09-28 addendum). A document that both registers a bundle
//! identifier and enables a capability on it, in the same run, must plan
//! both nodes as `Create` -- not fail at `plan` with `NotFound` before
//! either node is ever ensured. This document is inlined with
//! `willikins_dsl::parse_document` rather than added under
//! `workflows/fixtures/`, since a file there is scanned by
//! `it__acceptance__characterization_of_every_document.snap` and this test
//! needs no entry in it.

use indexmap::IndexMap;

use willikins_core::{Action, InputName, PlanError, ToolErrorKind, TypeName, TypeRef, Value};
use willikins_providers_fake::FakeState;

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_document(name: &str) -> willikins_core::Workflow {
    let path = workspace_root()
        .join("workflows")
        .join("fixtures")
        .join(name);
    willikins_dsl::load_document(&path).unwrap_or_else(|err| panic!("{name} loads: {err}"))
}

fn scalar_input(type_name: &str, text: &str) -> Value {
    Value::parse(&TypeRef::scalar(TypeName::parse(type_name).unwrap()), text).unwrap()
}

/// Seeds exactly what the fixture's credential chain and its bundle id
/// parent need to resolve, so `plan` reaches the `healthkit` node itself
/// rather than failing earlier at credential resolution -- mirrors
/// `profile_documents.rs`'s own `seeded_state`, inlined here since this
/// state is used by nothing else.
fn seeded_state() -> std::sync::Arc<std::sync::Mutex<FakeState>> {
    let json = serde_json::json!({
        "doppler_values": {
            "app-store-connect/prd#APP_STORE_CONNECT_API_KEY_ISSUER_ID": "57246542-96fe-1a63-e053-0824d011072a",
            "app-store-connect/prd#APP_STORE_CONNECT_API_KEY_ID": "2X9R4HXF34",
        },
        "doppler_secrets": {
            "app-store-connect/prd#APP_STORE_CONNECT_API_KEY_BASE64": "VGhpcyBpcyBhbiBleGFtcGxlIGtleSBmb3IgdGVzdHMgb25seS4KLS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0tCk1JR0hBZ0VBTUJNR0J5cUdTTTQ5QWdFR0NDcUdTTTQ5QXdFSEJHMHdhd0lCQVFRZ3ZMNTJyZWtFcWdHcW9XbjkKK1lCa0lRdVFXRU9UaEtxcUlYYnZvbmVuY0FXaFJBTkNBQVRkdC9YZDRjL0NMT0thMmpvRDlHMXBCOTh1d0tOKwpMR0p2SzNoS1RyeFRXbkowR3lRaVAzUm1DdWJ6bCtHUVIvL2g5Y2lGYW1qeU5jSE1qVlUyY0tiQQotLS0tLUVORCBQUklWQVRFIEtFWS0tLS0tCg==",
        },
        "apple_bundle_ids": {
            "com.example.MyApp": {
                "id": "T6G4XCV345",
                "name": "my-app",
                "platform": "UNIVERSAL",
            },
        },
    })
    .to_string();
    let state = FakeState::from_json(&json).unwrap_or_else(|err| panic!("seeded state: {err}"));
    std::sync::Arc::new(std::sync::Mutex::new(state))
}

fn positive_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("config").unwrap(),
        scalar_input("DopplerConfig", "app-store-connect/prd"),
    );
    inputs.insert(
        InputName::parse("identifier").unwrap(),
        scalar_input("AppleBundleIdentifier", "com.example.MyApp"),
    );
    inputs
}

#[test]
fn the_setting_mismatch_fixture_checks_cleanly_but_fails_plan_naming_healthkit() {
    let workflow = fixture_document("appstore-capability-setting-mismatch.yaml");
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state.clone());

    let checked = willikins_core::check(&workflow, &catalog).unwrap_or_else(|errors| {
        panic!(
            "the fixture must check cleanly -- both ports are individually well-typed: {errors:?}"
        )
    });

    let err = willikins_core::plan(&checked, &positive_inputs(), &catalog)
        .expect_err("plan must fail on the mismatched pairing");

    match err {
        PlanError::Tool { node, error } => {
            assert_eq!(node.as_str(), "healthkit");
            assert_eq!(error.kind, ToolErrorKind::Invalid);
            assert!(error.message.contains("HEALTHKIT"), "{}", error.message);
        }
        other => panic!("expected PlanError::Tool naming `healthkit`, got {other:?}"),
    }

    // No state was ever consulted, matching "no request" on the live
    // side: the fake's own `check_setting_pairing` runs before its state
    // lock is even taken, so nothing was ever written.
    let state = state.lock().unwrap();
    assert!(
        state
            .apple_bundle_id_capabilities
            .get("com.example.MyApp")
            .is_none_or(std::collections::HashSet::is_empty),
        "the refused node must never have written a capability"
    );
}

// ---------------------------------------------------------------------
// The T3-blocking fix: a fresh bundle id and a capability on it, in the
// same plan
// ---------------------------------------------------------------------

/// Credential resolution, then `appstore.bundle_id.ensure` on
/// `inputs.identifier`, then `appstore.bundle_id_capability.ensure`
/// bound from `steps.bundle_id.identifier` (decision (c) of the
/// milestone 3e plan -- never from the input directly, since the data
/// edge from the registration node is what orders the capability node
/// after it). `HEALTHKIT` takes no setting, so the pairing refusal never
/// fires here.
const NEW_BUNDLE_ID_WITH_CAPABILITY: &str = r"
name: appstore-new-bundle-id-with-capability
description: test-only, inlined -- a fresh bundle id and a capability on it plan as create in the same run.
inputs:
  config: { type: DopplerConfig }
  identifier: { type: AppleBundleIdentifier }
  name: { type: AppleBundleIdName }
  platform: { type: AppleBundleIdPlatform }
steps:
  issuer_id_text:
    tool: doppler.value.get
    with:
      config: ${{ inputs.config }}
      name: APP_STORE_CONNECT_API_KEY_ISSUER_ID
  issuer_id:
    tool: apple.issuer_id.parse
    with:
      value: ${{ steps.issuer_id_text.value }}
  key_id_text:
    tool: doppler.value.get
    with:
      config: ${{ inputs.config }}
      name: APP_STORE_CONNECT_API_KEY_ID
  key_id:
    tool: apple.key_id.parse
    with:
      value: ${{ steps.key_id_text.value }}
  key_base64:
    tool: doppler.secret.get
    with:
      config: ${{ inputs.config }}
      name: APP_STORE_CONNECT_API_KEY_BASE64
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
      name: ${{ inputs.name }}
      platform: ${{ inputs.platform }}
  healthkit:
    tool: appstore.bundle_id_capability.ensure
    with:
      issuer_id: ${{ steps.issuer_id.value }}
      key_id: ${{ steps.key_id.value }}
      key: ${{ steps.key.value }}
      identifier: ${{ steps.bundle_id.identifier }}
      capability: HEALTHKIT
outputs:
  bundle_id: ${{ steps.bundle_id.id }}
";

/// The credential chain resolves (same values as [`seeded_state`]), but
/// `apple_bundle_ids` carries no entry at all for the identifier this
/// document registers -- the identifier is genuinely fresh.
fn seeded_state_without_a_bundle_id() -> std::sync::Arc<std::sync::Mutex<FakeState>> {
    let json = serde_json::json!({
        "doppler_values": {
            "app-store-connect/prd#APP_STORE_CONNECT_API_KEY_ISSUER_ID": "57246542-96fe-1a63-e053-0824d011072a",
            "app-store-connect/prd#APP_STORE_CONNECT_API_KEY_ID": "2X9R4HXF34",
        },
        "doppler_secrets": {
            "app-store-connect/prd#APP_STORE_CONNECT_API_KEY_BASE64": "VGhpcyBpcyBhbiBleGFtcGxlIGtleSBmb3IgdGVzdHMgb25seS4KLS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0tCk1JR0hBZ0VBTUJNR0J5cUdTTTQ5QWdFR0NDcUdTTTQ5QXdFSEJHMHdhd0lCQVFRZ3ZMNTJyZWtFcWdHcW9XbjkKK1lCa0lRdVFXRU9UaEtxcUlYYnZvbmVuY0FXaFJBTkNBQVRkdC9YZDRjL0NMT0thMmpvRDlHMXBCOTh1d0tOKwpMR0p2SzNoS1RyeFRXbkowR3lRaVAzUm1DdWJ6bCtHUVIvL2g5Y2lGYW1qeU5jSE1qVlUyY0tiQQotLS0tLUVORCBQUklWQVRFIEtFWS0tLS0tCg==",
        },
    })
    .to_string();
    let state = FakeState::from_json(&json).unwrap_or_else(|err| panic!("seeded state: {err}"));
    std::sync::Arc::new(std::sync::Mutex::new(state))
}

/// The inputs [`NEW_BUNDLE_ID_WITH_CAPABILITY`] runs with: a fresh
/// identifier, `com.example.Fresh`.
fn fresh_bundle_id_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("config").unwrap(),
        scalar_input("DopplerConfig", "app-store-connect/prd"),
    );
    inputs.insert(
        InputName::parse("identifier").unwrap(),
        scalar_input("AppleBundleIdentifier", "com.example.Fresh"),
    );
    inputs.insert(
        InputName::parse("name").unwrap(),
        scalar_input("AppleBundleIdName", "fresh-app"),
    );
    inputs.insert(
        InputName::parse("platform").unwrap(),
        scalar_input("AppleBundleIdPlatform", "UNIVERSAL"),
    );
    inputs
}

/// Acceptance test (T3-blocking fix, milestone 3e plan, 2026-09-28
/// addendum): `plan` must not fail at the `healthkit` node with
/// `NotFound` before `bundle_id` is ever ensured. Both nodes plan as
/// `Action::Create`.
#[test]
fn a_document_that_ensures_a_new_bundle_id_and_a_capability_on_it_plans_with_both_as_create() {
    let workflow = willikins_dsl::parse_document(NEW_BUNDLE_ID_WITH_CAPABILITY)
        .unwrap_or_else(|err| panic!("the inlined document parses: {err}"));
    let state = seeded_state_without_a_bundle_id();
    let catalog = willikins_providers_fake::catalog(state);

    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document must check cleanly: {errors:?}"));

    let inputs = fresh_bundle_id_inputs();

    let plan = willikins_core::plan(&checked, &inputs, &catalog).unwrap_or_else(|err| {
        panic!("plan must resolve both nodes as create, never fail on the fresh parent: {err:?}")
    });

    let action_of = |node: &str| {
        plan.nodes
            .iter()
            .find(|planned| planned.name.as_str() == node)
            .unwrap_or_else(|| panic!("the `{node}` node planned"))
            .action
    };
    assert_eq!(action_of("bundle_id"), Action::Create);
    assert_eq!(action_of("healthkit"), Action::Create);
}

/// The same document carried through `apply`, then planned again: the
/// capability node's `Absent`-for-a-missing-parent read must not let its
/// `ensure` run before the parent exists (the fake's `ensure`, like the
/// live one, refuses a missing parent with `NotFound`, so a wrong order
/// fails this test), the capability must land on the freshly registered
/// identifier, and a second plan must read every node `NoOp` --
/// acceptance 8's shape (milestone 3e plan), on the smallest document
/// that exercises the parent-then-capability edge.
#[test]
fn a_document_that_ensures_a_new_bundle_id_and_a_capability_on_it_applies_then_converges() {
    let workflow = willikins_dsl::parse_document(NEW_BUNDLE_ID_WITH_CAPABILITY)
        .unwrap_or_else(|err| panic!("the inlined document parses: {err}"));
    let state = seeded_state_without_a_bundle_id();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document must check cleanly: {errors:?}"));
    let inputs = fresh_bundle_id_inputs();

    let plan = willikins_core::plan(&checked, &inputs, &catalog)
        .unwrap_or_else(|err| panic!("must plan: {err:?}"));
    let mut observer = willikins_core::RecordingObserver::new();
    let approval = willikins_core::Approval::Human {
        approver: willikins_core::PrincipalId::parse("operator").unwrap(),
        at: willikins_core::Timestamp::now(),
    };
    willikins_core::apply(&checked, &inputs, &catalog, &plan, &approval, &mut observer)
        .unwrap_or_else(|err| {
            panic!("must apply: the capability node runs after its parent exists: {err:?}")
        });

    {
        let state = state.lock().unwrap();
        assert!(
            state.apple_bundle_ids.contains_key("com.example.Fresh"),
            "apply must have registered the fresh identifier"
        );
        assert!(
            state
                .apple_bundle_id_capabilities
                .get("com.example.Fresh")
                .is_some_and(|set| set.contains("HEALTHKIT")),
            "apply must have enabled HEALTHKIT on the freshly registered identifier"
        );
    }

    let second = willikins_core::plan(&checked, &inputs, &catalog)
        .unwrap_or_else(|err| panic!("must plan again: {err:?}"));
    for node in ["bundle_id", "healthkit"] {
        let action = second
            .nodes
            .iter()
            .find(|planned| planned.name.as_str() == node)
            .unwrap_or_else(|| panic!("the `{node}` node planned"))
            .action;
        assert_eq!(
            action,
            Action::NoOp,
            "`{node}` must converge on a second plan"
        );
    }
}

// ---------------------------------------------------------------------
// Adversarial pass 7 (the App Attest gate task): a capability App Store
// Connect only ever *reports* can never be handed to the capability
// *writer*, by literal or by typed binding
// ---------------------------------------------------------------------

/// `workflows/fixtures/appstore-capability-read-only-literal.yaml`:
/// `APP_ATTEST` parses as `AppleObservableCapabilityType` (the gate's
/// port) but not as `AppleCapabilityType` (the writer's), so `check`
/// refuses the literal before anything is planned -- the writer can never
/// be asked to `POST` a capability Apple answers `409` for.
#[test]
fn a_read_only_capability_literal_is_refused_by_check_on_capability_ensure() {
    let workflow = fixture_document("appstore-capability-read-only-literal.yaml");
    let catalog = willikins_providers_fake::catalog(seeded_state());
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("APP_ATTEST must never check on the capability writer");
    assert_eq!(errors.len(), 1, "exactly one error: {errors:?}");
    match &errors[0] {
        willikins_core::CheckError::InvalidLiteral { node, port, error } => {
            assert_eq!(node.as_str(), "app_attest");
            assert_eq!(port.as_str(), "capability");
            assert_eq!(error.type_name, "AppleCapabilityType");
        }
        other => panic!("expected InvalidLiteral on app_attest.capability, got {other:?}"),
    }
}

/// `workflows/fixtures/appstore-capability-observable-into-ensure.yaml`:
/// the same refusal through a typed binding -- an
/// `AppleObservableCapabilityType` input into the writer's
/// `AppleCapabilityType` port is a type mismatch, since no conversion
/// between the two is registered.
#[test]
fn an_observable_capability_input_is_refused_by_check_on_capability_ensure() {
    let workflow = fixture_document("appstore-capability-observable-into-ensure.yaml");
    let catalog = willikins_providers_fake::catalog(seeded_state());
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("an observable-typed capability must never check on the capability writer");
    assert_eq!(errors.len(), 1, "exactly one error: {errors:?}");
    match &errors[0] {
        willikins_core::CheckError::TypeMismatch {
            node,
            port,
            expected,
            found,
        } => {
            assert_eq!(node.as_str(), "capability");
            assert_eq!(port.as_str(), "capability");
            assert_eq!(
                *expected,
                willikins_core::PortType::Exact(TypeRef::scalar(
                    TypeName::parse("AppleCapabilityType").unwrap()
                ))
            );
            assert_eq!(
                *found,
                TypeRef::scalar(TypeName::parse("AppleObservableCapabilityType").unwrap())
            );
        }
        other => panic!("expected TypeMismatch on capability.capability, got {other:?}"),
    }
}
