//! End-to-end proof for the App Store Connect credential correction: the
//! operator's own document (`workflows/apple-signing-credential-from-doppler.yaml`)
//! actually `plan`s against seeded fake state, not merely `check`s. Unit
//! tests already prove each node in isolation (`doppler.value.get`'s own
//! mock tests, `base64.decode`'s and `apple.signing_key.parse`'s own unit
//! tests, `secret.rs`'s `reveal_transform_input` tests); this proves the
//! whole chain, wired exactly as the document wires it, seeded from
//! `workflows/fixtures/state/apple-signing-credential.json` under the
//! operator's own three secret names.

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
        .join("apple-signing-credential.json");
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    let state =
        FakeState::from_json(&json).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    std::sync::Arc::new(std::sync::Mutex::new(state))
}

/// The Doppler chain: `doppler.value.get` for the two ids,
/// `doppler.secret.get` into `base64.decode` into `apple.signing_key.parse`
/// for the key. `plan` resolves every node against the seeded fake state,
/// the `issuer_id`/`key_id` outputs render the seeded plaintext (proving
/// `doppler.value.get` really is non-secret end to end), and the `key`
/// node's output renders redacted (proving the widened `AnySecret` ports
/// did not launder anything along the way).
#[test]
fn the_doppler_chain_plans_and_resolves_every_part() {
    let workflow = document("apple-signing-credential-from-doppler.yaml");
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state);
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));

    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("config").unwrap(),
        Value::parse(
            &TypeRef::scalar(TypeName::parse("DopplerConfig").unwrap()),
            "app-store-connect/prd",
        )
        .unwrap(),
    );

    let plan = willikins_core::plan(&checked, &inputs, &catalog)
        .unwrap_or_else(|err| panic!("plan must resolve every node: {err:?}"));

    let issuer_id = plan
        .outputs
        .get(&willikins_core::OutputName::parse("issuer_id").unwrap())
        .expect("issuer_id output");
    assert_eq!(
        issuer_id.render().to_string(),
        "57246542-96fe-1a63-e053-0824d011072a"
    );
    let key_id = plan
        .outputs
        .get(&willikins_core::OutputName::parse("key_id").unwrap())
        .expect("key_id output");
    assert_eq!(key_id.render().to_string(), "2X9R4HXF34");

    // The key itself is not a document output (no Apple tool exists yet
    // to hand it to), but its node still ran: find it in the plan and
    // confirm its rendered value is redacted, never the decoded PEM.
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

/// The plain-inputs document: the two ids arrive as workflow inputs,
/// which `check` accepts only because [`willikins_types::AppleIssuerId`]
/// and [`willikins_types::AppleKeyId`] are not secret -- this is the
/// static proof that the second document can be expressed at all, which
/// is the whole point of shipping it (see the document's own header
/// comment). `plan` is deliberately not exercised here: `env.get` reads
/// the real process environment, and mutating it with `std::env::set_var`
/// is `unsafe` under edition 2024, which this workspace's
/// `unsafe_code = "forbid"` lint blocks outright --
/// `willikins-tools/src/env_get.rs`'s own unit tests hit exactly this
/// wall and split their tool's logic below the port layer to test it
/// without mutating process state; nothing here can do that from outside
/// the tool's own crate. `env.get`'s "present"/"absent"/"not UTF-8"
/// behaviour is already proven by that crate's own tests, and this
/// document's Doppler-fed sibling
/// (`the_doppler_chain_plans_and_resolves_every_part`, above) already
/// proves a real `plan` reaching `apple.signing_key.parse` end to end.
#[test]
fn the_plain_inputs_document_checks_with_the_two_ids_as_workflow_inputs() {
    let workflow = document("apple-signing-credential-from-inputs.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));
}
