//! End-to-end proof for the two App Store Connect bundle id documents:
//! `workflows/appstore-bundle-id-from-doppler.yaml` (the operator's own
//! Doppler chain, extended into a real tool this crate now provides) and
//! `workflows/appstore-bundle-id-from-inputs.yaml` (the two ids as plain
//! workflow inputs, only the key resolved). Both must `check` cleanly
//! against the fake catalog; the Doppler one is also `plan`ned against
//! seeded fake state, proving the whole chain -- Doppler through
//! `appstore.bundle_id.ensure` -- resolves end to end, not merely
//! type-checks.
//!
//! What the second document (`-from-inputs`) proves, on its own: the
//! credential's two non-secret ids are genuinely free ports. If they had
//! to come from wherever the key does, this document could not be
//! written at all -- `check` would refuse a secret-typed workflow input,
//! and there would be no way to express "the two ids are workflow
//! inputs, the key is a resolver output" as two different provenances
//! for the same tool's three credential ports. That it *does* check
//! cleanly, with `issuer_id`/`key_id` bound straight to `inputs.*` and
//! `key` bound to a resolver chain's output, is the proof the earlier
//! lane's coupled design (`AppleSigningCredential::new(issuer_id:
//! impl Into<String>, ...)`, reading the ids from wherever the key came
//! from) is genuinely gone.

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

fn seeded_state() -> std::sync::Arc<std::sync::Mutex<FakeState>> {
    let path = workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join("appstore-bundle-id.json");
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    let state =
        FakeState::from_json(&json).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    std::sync::Arc::new(std::sync::Mutex::new(state))
}

fn scalar_input(type_name: &str, text: &str) -> Value {
    Value::parse(&TypeRef::scalar(TypeName::parse(type_name).unwrap()), text).unwrap()
}

/// The Doppler chain: `doppler.value.get` for the two ids,
/// `doppler.secret.get` into `base64.decode` into `apple.signing_key.parse`
/// for the key, then `appstore.bundle_id.ensure` with the three resolved
/// parts and three literal-input bundle id attributes. `plan` resolves
/// every node against the seeded fake state (which already holds a
/// matching `apple_bundle_ids` entry), so the whole chain reports
/// `Present` -- proving the key really did decode and parse to a usable
/// credential, not only that the types line up.
#[test]
fn the_doppler_chain_plans_and_registers_the_bundle_id() {
    let workflow = document("appstore-bundle-id-from-doppler.yaml");
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state);
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));

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
        InputName::parse("name").unwrap(),
        scalar_input("AppleBundleIdName", "willikins-demo"),
    );
    inputs.insert(
        InputName::parse("platform").unwrap(),
        scalar_input("AppleBundleIdPlatform", "UNIVERSAL"),
    );

    let plan = willikins_core::plan(&checked, &inputs, &catalog)
        .unwrap_or_else(|err| panic!("plan must resolve every node: {err:?}"));

    let id = plan
        .outputs
        .get(&willikins_core::OutputName::parse("id").unwrap())
        .expect("id output");
    assert_eq!(id.render().to_string(), "FAKE00000001");
    let identifier = plan
        .outputs
        .get(&willikins_core::OutputName::parse("identifier").unwrap())
        .expect("identifier output");
    assert_eq!(
        identifier.render().to_string(),
        "com.example.willikins-demo"
    );

    // The key node itself still renders redacted -- the chain reaching a
    // real tool changes nothing about that.
    let key_node = plan
        .nodes
        .iter()
        .find(|node| node.name.as_str() == "key")
        .expect("the `key` node planned");
    let key_value = key_node
        .outputs
        .get(&willikins_core::PortName::parse("value").unwrap())
        .expect("key node has a `value` output");
    assert_eq!(key_value.render().to_string(), "[REDACTED AppleSigningKey]");
}

/// The discriminating test `appstore.bundle_id.ensure`'s own module doc
/// points to: a document reaching this tool with a *drifted* `name`
/// fails at `plan()` with `AttributeMismatch`, even though
/// `Tool::ensure` itself can converge that exact drift (proven directly
/// by `tests/bundle_id_ensure_mock.rs`'s
/// `ensure_converges_a_name_mismatch_via_patch`). This is
/// `willikins_core::plan`'s own pre-existing invariant -- every
/// `Observation::Mismatch` is terminal before any node's `ensure` runs --
/// not a bug in this tool, and this test is what makes that concrete
/// rather than left as an unverified claim in a doc comment.
#[test]
fn a_drifted_name_fails_plan_even_though_ensure_can_converge_it() {
    let workflow = document("appstore-bundle-id-from-doppler.yaml");
    // Seed a record whose `name` does not match what the document will
    // request, everything else held equal to the seeded state's own
    // matching record.
    let mut state = FakeState::from_json(
        &std::fs::read_to_string(
            workspace_root()
                .join("workflows")
                .join("fixtures")
                .join("state")
                .join("appstore-bundle-id.json"),
        )
        .unwrap(),
    )
    .unwrap();
    state.apple_bundle_ids.insert(
        "com.example.willikins-demo".to_string(),
        willikins_providers_fake::state::AppleBundleIdRecord {
            id: "FAKE00000001".to_string(),
            name: "a-drifted-name".to_string(),
            platform: "UNIVERSAL".to_string(),
        },
    );
    let catalog =
        willikins_providers_fake::catalog(std::sync::Arc::new(std::sync::Mutex::new(state)));
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));

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
        InputName::parse("name").unwrap(),
        scalar_input("AppleBundleIdName", "willikins-demo"),
    );
    inputs.insert(
        InputName::parse("platform").unwrap(),
        scalar_input("AppleBundleIdPlatform", "UNIVERSAL"),
    );

    let err = willikins_core::plan(&checked, &inputs, &catalog)
        .expect_err("a drifted name must fail plan, not silently converge");
    assert!(
        matches!(err, willikins_core::PlanError::AttributeMismatch { .. }),
        "{err:?}"
    );
}

/// The plain-inputs document: the two ids arrive as workflow inputs,
/// which `check` accepts only because `AppleIssuerId`/`AppleKeyId` are
/// not secret. `plan` is deliberately not exercised here, for the same
/// reason `apple_signing_credential_chain.rs`'s own sibling test gives:
/// `env.get` reads the real process environment, and mutating it is
/// `unsafe` under this workspace's `unsafe_code = "forbid"` lint. What
/// this document alone proves is stated in this file's own module doc.
#[test]
fn the_plain_inputs_document_checks_with_the_two_ids_as_workflow_inputs() {
    let workflow = document("appstore-bundle-id-from-inputs.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));
}
