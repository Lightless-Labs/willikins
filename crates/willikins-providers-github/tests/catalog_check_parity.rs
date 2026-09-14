//! Acceptance test 1's second sentence: `check` of both positive fixtures
//! against a catalog holding these two *live* tools in place of the fake
//! ones resolves exactly the same types, in the same order, as against
//! the fake catalog.
//!
//! `catalog_parity.rs` pins the two `ToolSpec`s equal on their own; this
//! file pins the consequence — that swapping the live tools in changes
//! nothing `check` can see. Neither catalog's tools are called here: a
//! `check` is pure, so the live tools' `Http` may point at a port nothing
//! listens on.

use std::sync::{Arc, Mutex};

use willikins_core::{Catalog, Tool};
use willikins_providers_fake::FakeState;
use willikins_providers_github::{GitHubActionsSecretEnsure, GitHubClient, GitHubRepoEnsure};
use willikins_providers_http::{Credential, Http};

/// The two positive fixtures the milestone's goal names.
const POSITIVE_FIXTURES: [&str; 2] = ["new-rust-service.yaml", "rotate-service-token.yaml"];

fn workflows_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("workflows")
}

fn live_client() -> Arc<GitHubClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    Arc::new(GitHubClient::new(Http::new(
        "http://127.0.0.1:1",
        Vec::new(),
        credential,
    )))
}

/// Every fake tool except the two GitHub ones, plus the two live GitHub
/// ones. Built by hand rather than by overlaying
/// [`willikins_providers_fake::catalog`], because [`Catalog::insert`]
/// refuses a duplicate name outright — which is itself the guarantee that
/// this catalog holds the live tools and not the fake ones.
fn live_catalog(state: &Arc<Mutex<FakeState>>) -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    let mut insert = |tool: Arc<dyn Tool>| {
        catalog
            .insert(tool)
            .unwrap_or_else(|err| panic!("live catalog: {err}"));
    };
    insert(Arc::new(willikins_tools::NamingV1::new()));
    insert(Arc::new(willikins_tools::TemplateRender::new()));
    insert(Arc::new(GitHubRepoEnsure::new(live_client())));
    insert(Arc::new(GitHubActionsSecretEnsure::new(live_client())));
    insert(Arc::new(
        willikins_providers_fake::tools::DopplerProjectEnsure::new(state.clone()),
    ));
    insert(Arc::new(
        willikins_providers_fake::tools::DopplerConfigEnsure::new(state.clone()),
    ));
    insert(Arc::new(
        willikins_providers_fake::tools::DopplerServiceTokenEnsure::new(state.clone()),
    ));
    insert(Arc::new(
        willikins_providers_fake::tools::DopplerServiceTokenRotate::new(state.clone()),
    ));
    insert(Arc::new(
        willikins_providers_fake::tools::DopplerSecretGet::new(state.clone()),
    ));
    catalog
}

#[test]
fn check_resolves_the_same_types_against_the_live_and_fake_catalogs() {
    let (fake_state, fake_catalog) = willikins_providers_fake::empty();
    let live = live_catalog(&fake_state);

    for fixture in POSITIVE_FIXTURES {
        let workflow = willikins_dsl::load_document(&workflows_dir().join(fixture))
            .unwrap_or_else(|err| panic!("{fixture} loads: {err}"));
        let against_fake =
            willikins_core::check(&workflow, &fake_catalog).unwrap_or_else(|errors| {
                panic!("{fixture} checks against the fake catalog: {errors:?}")
            });
        let against_live = willikins_core::check(&workflow, &live).unwrap_or_else(|errors| {
            panic!("{fixture} checks against the live catalog: {errors:?}")
        });
        assert_eq!(against_live.types, against_fake.types, "{fixture}: types");
        assert_eq!(
            against_live.output_types, against_fake.output_types,
            "{fixture}: output types"
        );
        assert_eq!(against_live.order, against_fake.order, "{fixture}: order");
        assert_eq!(against_live.class, against_fake.class, "{fixture}: class");
    }
}

/// The live catalog really does hold the live tools: inserting the fake
/// GitHub tools on top of it is refused as a duplicate name, which is
/// what makes the test above a statement about the live tools at all.
#[test]
fn the_live_catalog_holds_the_live_github_tools_not_the_fake_ones() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let mut catalog = live_catalog(&state);
    for tool in [
        Arc::new(willikins_providers_fake::tools::GitHubRepoEnsure::new(
            state.clone(),
        )) as Arc<dyn Tool>,
        Arc::new(willikins_providers_fake::tools::GitHubActionsSecretEnsure::new(state.clone())),
    ] {
        let name = tool.spec().name.clone();
        catalog
            .insert(tool)
            .expect_err(&format!("`{name}` is already in the live catalog"));
    }
}
