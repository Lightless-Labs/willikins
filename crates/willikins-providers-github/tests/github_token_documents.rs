//! End-to-end proof for `workflows/github-repo-token-from-doppler.yaml`:
//! milestone 3e task R2's demonstration that `github.repo.get`'s new
//! optional `token` port is a genuinely free credential port, resolved
//! from Doppler through `github.token.parse`, exactly the way
//! `willikins-providers-appstore/tests/bundle_id_documents.rs` proves the
//! same shape for App Store Connect's three credential ports.
//!
//! This document is new -- an addition, never a change to an existing
//! one -- so `crates/willikins-dsl/tests/acceptance.rs`'s characterization
//! sweep picks it up on its own and gains exactly one new snapshot entry;
//! nothing about any pre-existing document's `check`/`plan` output moves.

use indexmap::IndexMap;

use willikins_core::{InputName, TypeName, TypeRef, Value};
use willikins_providers_fake::FakeState;

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn document() -> willikins_core::Workflow {
    let path = workspace_root()
        .join("workflows")
        .join("github-repo-token-from-doppler.yaml");
    willikins_dsl::load_document(&path)
        .unwrap_or_else(|err| panic!("github-repo-token-from-doppler.yaml loads: {err}"))
}

fn seeded_state() -> std::sync::Arc<std::sync::Mutex<FakeState>> {
    let path = workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join("github-token.json");
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    let state =
        FakeState::from_json(&json).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    std::sync::Arc::new(std::sync::Mutex::new(state))
}

fn scalar_input(type_name: &str, text: &str) -> Value {
    Value::parse(&TypeRef::scalar(TypeName::parse(type_name).unwrap()), text).unwrap()
}

/// The Doppler chain -- `doppler.secret.get` into `github.token.parse` --
/// resolves to a real `GitHubToken`, binds to `github.repo.get`'s `token`
/// port, and `plan` reports the referenced repository `Present`: proof
/// the whole chain resolves end to end, not merely that the types line
/// up.
#[test]
fn the_doppler_chain_plans_and_references_the_repository() {
    let workflow = document();
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state);
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));

    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("config").unwrap(),
        scalar_input("DopplerConfig", "github/prd"),
    );
    inputs.insert(
        InputName::parse("repo").unwrap(),
        scalar_input("GitHubRepo", "bande-a-bonnot/monorepo"),
    );

    let plan = willikins_core::plan(&checked, &inputs, &catalog)
        .unwrap_or_else(|err| panic!("plan must resolve every node: {err:?}"));

    // The task's own redaction claim, for "plan": the seeded token's raw
    // value never reaches the plan as a whole, serialized exactly as an
    // MCP `plan` response or a CLI `--json` render would.
    let plan_json = serde_json::to_string(&plan).expect("Plan serializes");
    assert!(
        !plan_json.contains("ghp_example"),
        "the seeded token's raw value leaked into the serialized plan"
    );

    let repo = plan
        .outputs
        .get(&willikins_core::OutputName::parse("repo").unwrap())
        .expect("repo output");
    assert_eq!(repo.render().to_string(), "bande-a-bonnot/monorepo");

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
    assert_eq!(token_value.render().to_string(), "[REDACTED GitHubToken]");
}

/// Every existing document using `github.repo.get`, `github.repo.ensure`
/// or `github.actions_secret.ensure` (`new-rust-service.yaml`,
/// `walter-ios-app.yaml`, and others) leaves the new `token` port
/// completely unbound, and keeps checking exactly as it did: proven here
/// directly against the tool's spec (`required` is `false`), and proven
/// for those documents themselves by
/// `crates/willikins-dsl/tests/acceptance.rs`'s characterization sweep,
/// whose snapshot gains only this file's own new document, byte-identical
/// otherwise.
#[test]
fn an_unbound_token_port_is_not_required_by_check() {
    let (_state, catalog) = willikins_providers_fake::empty();
    let tool = catalog
        .get(&willikins_core::ToolName::parse("github.repo.get").unwrap())
        .expect("the fake catalog carries github.repo.get");
    let token_port = tool
        .spec()
        .inputs
        .get(&willikins_core::PortName::parse("token").unwrap())
        .expect("a `token` port is declared");
    assert!(!token_port.required, "`token` must stay optional");
}
