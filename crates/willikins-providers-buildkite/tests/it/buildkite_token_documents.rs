//! End-to-end proof for `workflows/buildkite-cluster-token-from-doppler.yaml`:
//! milestone 3e task K1's demonstration that `buildkite.cluster.get`'s new
//! optional `token` port is a genuinely free credential port, resolved
//! from Doppler through `buildkite.token.parse`, exactly the way
//! `willikins-providers-github/tests/it/github_token_documents.rs` proves the
//! same shape for GitHub (task R2).
//!
//! This document is new -- an addition, never a change to an existing
//! one -- so `crates/willikins-dsl/tests/it/acceptance.rs`'s characterization
//! sweep picks it up on its own and gains exactly one new snapshot entry;
//! nothing about any pre-existing document's `check`/`plan` output moves.

use indexmap::IndexMap;

use willikins_core::{InputName, TypeName, TypeRef, Value};
use willikins_providers_fake::FakeState;

/// A valid [`willikins_types::BuildkiteToken`], `concat!`-assembled so no
/// single literal in this file (or in the seed fixture on disk, which
/// carries only a placeholder) spells a real-shaped Buildkite token
/// contiguously.
const SEEDED_TOKEN: &str = concat!("bkua_", "wlknFixtureTokenNotARealCredential00");

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn document() -> willikins_core::Workflow {
    let path = workspace_root()
        .join("workflows")
        .join("buildkite-cluster-token-from-doppler.yaml");
    willikins_dsl::load_document(&path)
        .unwrap_or_else(|err| panic!("buildkite-cluster-token-from-doppler.yaml loads: {err}"))
}

/// `buildkite-token.json`'s seeded secret is the placeholder
/// `BUILDKITE_TOKEN_PLACEHOLDER`, not a token-shaped literal (the same
/// technique `willikins-providers-doppler/fixtures/doppler/README.md`
/// documents for `service_token_post_created.json`'s `key`): this
/// substitutes in [`SEEDED_TOKEN`] before parsing the fixture as
/// [`FakeState`], so the file on disk never carries a real-shaped
/// Buildkite token.
fn seeded_state() -> std::sync::Arc<std::sync::Mutex<FakeState>> {
    let path = workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join("buildkite-token.json");
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    let json = json.replace("BUILDKITE_TOKEN_PLACEHOLDER", SEEDED_TOKEN);
    let state =
        FakeState::from_json(&json).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    std::sync::Arc::new(std::sync::Mutex::new(state))
}

fn scalar_input(type_name: &str, text: &str) -> Value {
    Value::parse(&TypeRef::scalar(TypeName::parse(type_name).unwrap()), text).unwrap()
}

/// The Doppler chain -- `doppler.secret.get` into `buildkite.token.parse`
/// -- resolves to a real `BuildkiteToken`, binds to
/// `buildkite.cluster.get`'s `token` port, and `plan` reports the
/// referenced cluster `Present`: proof the whole chain resolves end to
/// end, not merely that the types line up. Also the task's own redaction
/// claim for "plan": the seeded token's raw value never reaches the
/// serialized `Plan`, exactly as an MCP `plan` response or a CLI
/// `--json` render would carry it.
#[test]
fn the_doppler_chain_plans_and_references_the_cluster() {
    let workflow = document();
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state);
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));

    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("config").unwrap(),
        scalar_input("DopplerConfig", "buildkite/prd"),
    );
    inputs.insert(
        InputName::parse("org").unwrap(),
        scalar_input("BuildkiteOrg", "willikins-test"),
    );
    inputs.insert(
        InputName::parse("name").unwrap(),
        scalar_input("BuildkiteClusterName", "Default cluster"),
    );

    let plan = willikins_core::plan(&checked, &inputs, &catalog)
        .unwrap_or_else(|err| panic!("plan must resolve every node: {err:?}"));

    let plan_json = serde_json::to_string(&plan).expect("Plan serializes");
    assert!(
        !plan_json.contains(SEEDED_TOKEN),
        "the seeded token's raw value leaked into the serialized plan"
    );

    let cluster = plan
        .outputs
        .get(&willikins_core::OutputName::parse("cluster").unwrap())
        .expect("cluster output");
    assert_eq!(
        cluster.render().to_string(),
        "018e5a22-d14c-7085-bb28-db0f83f43a1c"
    );

    // The token node itself still renders redacted -- the chain reaching
    // a real optional credential port changes nothing about that.
    let token_node = plan
        .nodes
        .iter()
        .find(|node| node.name.as_str() == "token")
        .expect("the `token` node planned");
    let token_value = token_node
        .outputs
        .get(&willikins_core::PortName::parse("value").unwrap())
        .expect("token node has a `value` output");
    assert_eq!(
        token_value.render().to_string(),
        "[REDACTED BuildkiteToken]"
    );
}

/// Every existing document using `buildkite.cluster.get` or
/// `buildkite.pipeline.ensure` (`new-rust-service-buildkite.yaml`) leaves
/// the new `token` port completely unbound, and keeps checking exactly
/// as it did: proven here directly against both tools' specs (`required`
/// is `false` on each), and proven for those documents themselves by
/// `crates/willikins-dsl/tests/it/acceptance.rs`'s characterization sweep,
/// whose snapshot gains only this file's own new document, byte-identical
/// otherwise.
#[test]
fn an_unbound_token_port_is_not_required_by_check() {
    let (_state, catalog) = willikins_providers_fake::empty();
    for tool_name in ["buildkite.cluster.get", "buildkite.pipeline.ensure"] {
        let tool = catalog
            .get(&willikins_core::ToolName::parse(tool_name).unwrap())
            .unwrap_or_else(|| panic!("the fake catalog carries {tool_name}"));
        let token_port = tool
            .spec()
            .inputs
            .get(&willikins_core::PortName::parse("token").unwrap())
            .unwrap_or_else(|| panic!("{tool_name}: a `token` port is declared"));
        assert!(
            !token_port.required,
            "{tool_name}: `token` must stay optional"
        );
    }
}
