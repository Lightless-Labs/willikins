//! The *whole* live catalog — the one task 10a will assemble and serve:
//! `willikins-tools`' two pure tools, `willikins-providers-github`'s two
//! live tools, and this crate's five live Doppler tools. Nine tools, no
//! fake among them.
//!
//! `catalog_parity.rs` pins each Doppler `ToolSpec` equal to the fake's
//! one spec at a time, and `catalog_check_parity.rs` pins that swapping
//! the live Doppler tools into an otherwise-fake catalog changes nothing
//! `check` can see. Neither of those, nor
//! `willikins-providers-github`'s mirror-image file (live GitHub, fake
//! Doppler), ever builds the catalog with *both* providers live — so
//! until this file, nothing proved the assembly task 10a needs is even
//! constructible: that the nine specs' names do not collide, that every
//! one validates against the shared type registry, and that the
//! milestone's two positive fixtures still `check` against it, resolving
//! the same types in the same order as against the all-fake catalog.
//!
//! No tool here is ever called. `check` is pure, so every client points
//! at a port nothing listens on, and every `Credential` is a
//! `for_testing` one.

use std::sync::Arc;

use willikins_core::{Catalog, Tool};
use willikins_providers_doppler::{
    DopplerClient, DopplerConfigEnsure, DopplerProjectEnsure, DopplerSecretGet,
    DopplerServiceTokenEnsure, DopplerServiceTokenRotate,
};
use willikins_providers_github::{GitHubActionsSecretEnsure, GitHubClient, GitHubRepoEnsure};
use willikins_providers_http::{Credential, Http};

/// The two positive fixtures the milestone's goal names.
const POSITIVE_FIXTURES: [&str; 2] = ["new-rust-service.yaml", "rotate-service-token.yaml"];

/// Every tool name the assembled live catalog must hold, exactly.
const LIVE_TOOL_NAMES: [&str; 9] = [
    "naming.v1",
    "template.render",
    "github.repo.ensure",
    "github.actions_secret.ensure",
    "doppler.project.ensure",
    "doppler.config.ensure",
    "doppler.service_token.ensure",
    "doppler.service_token.rotate",
    "doppler.secret.get",
];

fn workflows_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("workflows")
}

/// A port nothing listens on: a `check` never calls a tool, and a test
/// that accidentally did would fail as a transport error rather than
/// reach the network.
const NOWHERE: &str = "http://127.0.0.1:1";

fn doppler_client() -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    Arc::new(DopplerClient::new(Http::new(
        NOWHERE,
        Vec::new(),
        credential,
    )))
}

fn github_client() -> Arc<GitHubClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    Arc::new(GitHubClient::new(Http::new(
        NOWHERE,
        willikins_providers_github::default_headers(),
        credential,
    )))
}

/// The live catalog task 10a assembles.
fn live_catalog() -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    let mut insert = |tool: Arc<dyn Tool>| {
        let name = tool.spec().name.clone();
        catalog
            .insert(tool)
            .unwrap_or_else(|err| panic!("live catalog rejected `{name}`: {err}"));
    };
    insert(Arc::new(willikins_tools::NamingV1::new()));
    insert(Arc::new(willikins_tools::TemplateRender::new()));
    insert(Arc::new(GitHubRepoEnsure::new(github_client())));
    insert(Arc::new(GitHubActionsSecretEnsure::new(github_client())));
    insert(Arc::new(DopplerProjectEnsure::new(doppler_client())));
    insert(Arc::new(DopplerConfigEnsure::new(doppler_client())));
    insert(Arc::new(DopplerServiceTokenEnsure::new(doppler_client())));
    insert(Arc::new(DopplerServiceTokenRotate::new(doppler_client())));
    insert(Arc::new(DopplerSecretGet::new(doppler_client())));
    catalog
}

/// The assembly itself: nine tools, no name collision, every spec valid
/// against the shared type registry.
#[test]
fn the_live_catalog_assembles_and_every_spec_validates() {
    let catalog = live_catalog();
    for name in LIVE_TOOL_NAMES {
        let tool_name = willikins_core::ToolName::parse(name)
            .unwrap_or_else(|err| panic!("`{name}` is a tool name: {err}"));
        let tool = catalog
            .get(&tool_name)
            .unwrap_or_else(|| panic!("the live catalog holds `{name}`"));
        tool.spec()
            .validate(willikins_types::registry())
            .unwrap_or_else(|err| panic!("`{name}`'s spec validates: {err:?}"));
    }
}

/// The two positive fixtures `check` against the fully-live catalog, and
/// resolve exactly what they resolve against the all-fake one: the same
/// types, output types, order, and class.
#[test]
fn both_positive_fixtures_check_the_same_against_the_live_catalog() {
    let (_state, fake_catalog) = willikins_providers_fake::empty();
    let live = live_catalog();

    for fixture in POSITIVE_FIXTURES {
        let workflow = willikins_dsl::load_document(&workflows_dir().join(fixture))
            .unwrap_or_else(|err| panic!("{fixture} loads: {err}"));
        let against_fake =
            willikins_core::check(&workflow, &fake_catalog).unwrap_or_else(|errors| {
                panic!("{fixture} checks against the fake catalog: {errors:?}")
            });
        let against_live = willikins_core::check(&workflow, &live).unwrap_or_else(|errors| {
            panic!("{fixture} checks against the fully-live catalog: {errors:?}")
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

/// Nothing fake survives in it: inserting any of the nine fake tools of
/// the same names on top is refused as a duplicate, which is what makes
/// the two tests above statements about the live tools at all.
#[test]
fn no_fake_tool_fits_into_the_live_catalog() {
    let (_fake_state, fake_catalog) = willikins_providers_fake::empty();
    let mut catalog = live_catalog();
    for name in LIVE_TOOL_NAMES {
        let tool_name = willikins_core::ToolName::parse(name).expect("a tool name");
        let fake = fake_catalog
            .get(&tool_name)
            .unwrap_or_else(|| panic!("the fake catalog holds `{name}` too"));
        catalog
            .insert(fake.clone())
            .expect_err(&format!("`{name}` is already in the live catalog"));
    }
}
