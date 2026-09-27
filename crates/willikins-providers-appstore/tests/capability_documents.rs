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

use indexmap::IndexMap;

use willikins_core::{InputName, PlanError, ToolErrorKind, TypeName, TypeRef, Value};
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
            "app-store-connect/prd#ASC_API_KEY_ISSUER_ID": "57246542-96fe-1a63-e053-0824d011072a",
            "app-store-connect/prd#ASC_API_KEY_ID": "2X9R4HXF34",
        },
        "doppler_secrets": {
            "app-store-connect/prd#ASC_API_KEY_BASE64": "VGhpcyBpcyBhbiBleGFtcGxlIGtleSBmb3IgdGVzdHMgb25seS4KLS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0tCk1JR0hBZ0VBTUJNR0J5cUdTTTQ5QWdFR0NDcUdTTTQ5QXdFSEJHMHdhd0lCQVFRZ3ZMNTJyZWtFcWdHcW9XbjkKK1lCa0lRdVFXRU9UaEtxcUlYYnZvbmVuY0FXaFJBTkNBQVRkdC9YZDRjL0NMT0thMmpvRDlHMXBCOTh1d0tOKwpMR0p2SzNoS1RyeFRXbkowR3lRaVAzUm1DdWJ6bCtHUVIvL2g5Y2lGYW1qeU5jSE1qVlUyY0tiQQotLS0tLUVORCBQUklWQVRFIEtFWS0tLS0tCg==",
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
