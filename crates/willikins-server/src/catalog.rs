//! [`Butler::live_catalog`] and [`Butler::fake_catalog`]: the two ways a
//! caller builds the [`willikins_core::Catalog`] a [`crate::ButlerConfig`]
//! needs.
//!
//! `live_catalog` assembles the nine-tool live catalog --
//! `willikins-tools`' two pure tools, `willikins-providers-github`'s two
//! live tools, `willikins-providers-doppler`'s five -- exactly as
//! `crates/willikins-providers-doppler/tests/live_catalog.rs` built it
//! before this task; that test now calls [`live_catalog_with`] (this
//! module's own assembly, taking `Http`s rather than `Credential`s so a
//! test can point them at a mock server or nowhere) instead of keeping a
//! second copy, so the assembly exists exactly once.

use std::sync::Arc;

use willikins_core::{Catalog, Tool};
use willikins_providers_doppler::{
    DopplerClient, DopplerConfigEnsure, DopplerProjectEnsure, DopplerSecretGet,
    DopplerServiceTokenEnsure, DopplerServiceTokenRotate,
};
use willikins_providers_github::{GitHubActionsSecretEnsure, GitHubClient, GitHubRepoEnsure};
use willikins_providers_http::{Credential, Http};

/// Every tool name [`live_catalog_with`] (and so [`Butler::live_catalog`])
/// inserts, in insertion order -- pinned by
/// `tests::the_live_catalog_has_exactly_these_nine_tools_and_no_fake_tool_fits`.
pub const LIVE_TOOL_NAMES: [&str; 9] = [
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

/// Assemble the live catalog from an already-built `Http` for each
/// provider. The lower-level half of [`Butler::live_catalog`]: split out
/// so a test (this crate's own, and
/// `willikins-providers-doppler/tests/live_catalog.rs`) can supply an
/// `Http` pointed at a mock server or nowhere without duplicating the
/// tool list, while production code goes through
/// [`Butler::live_catalog`], which builds these two `Http`s from
/// `Credential`s against each provider's real base URL and default
/// headers (`willikins_providers_github::http_client`,
/// `willikins_providers_doppler::http_client`).
#[must_use]
pub fn live_catalog_with(github_http: Http, doppler_http: Http) -> Catalog {
    let github = Arc::new(GitHubClient::new(github_http));
    let doppler = Arc::new(DopplerClient::new(doppler_http));

    let mut catalog = Catalog::new(willikins_types::registry());
    let mut insert = |tool: Arc<dyn Tool>| {
        let name = tool.spec().name.clone();
        catalog
            .insert(tool)
            .unwrap_or_else(|err| unreachable!("live catalog rejected `{name}`: {err}"));
    };
    insert(Arc::new(willikins_tools::NamingV1::new()));
    insert(Arc::new(willikins_tools::TemplateRender::new()));
    insert(Arc::new(GitHubRepoEnsure::new(Arc::clone(&github))));
    insert(Arc::new(GitHubActionsSecretEnsure::new(github)));
    insert(Arc::new(DopplerProjectEnsure::new(Arc::clone(&doppler))));
    insert(Arc::new(DopplerConfigEnsure::new(Arc::clone(&doppler))));
    insert(Arc::new(DopplerServiceTokenEnsure::new(Arc::clone(
        &doppler,
    ))));
    insert(Arc::new(DopplerServiceTokenRotate::new(Arc::clone(
        &doppler,
    ))));
    insert(Arc::new(DopplerSecretGet::new(doppler)));
    catalog
}

/// The live catalog, against each provider's real base URL, built from
/// `github` and `doppler` credentials. See [`live_catalog_with`].
#[must_use]
pub fn live_catalog(github: Credential, doppler: Credential) -> Catalog {
    live_catalog_with(
        willikins_providers_github::http_client(github),
        willikins_providers_doppler::http_client(doppler),
    )
}

impl crate::Butler {
    /// The live catalog: `willikins-tools`' two pure tools plus every
    /// live GitHub and Doppler tool, against each provider's real API.
    /// See this module's own docs.
    #[must_use]
    pub fn live_catalog(github: Credential, doppler: Credential) -> Catalog {
        live_catalog(github, doppler)
    }

    /// A fresh all-fake catalog and its seedable state handle -- for
    /// tests, and for the CLI's default (non-`--live`) mode. A thin
    /// wrapper over `willikins_providers_fake::empty`, kept on `Butler`
    /// so every caller builds a `Catalog` through this crate's own
    /// surface rather than reaching into `willikins-providers-fake`
    /// directly.
    #[must_use]
    pub fn fake_catalog() -> (
        std::sync::Arc<std::sync::Mutex<willikins_providers_fake::FakeState>>,
        Catalog,
    ) {
        willikins_providers_fake::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::Credential;

    /// A port nothing listens on: a `check` never calls a tool, and a
    /// test that accidentally did would fail as a transport error rather
    /// than reach the network.
    const NOWHERE: &str = "http://127.0.0.1:1";

    fn test_catalog() -> Catalog {
        let github = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        let doppler = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
        live_catalog_with(
            Http::new(
                NOWHERE,
                willikins_providers_github::default_headers(),
                github,
            ),
            Http::new(NOWHERE, Vec::new(), doppler),
        )
    }

    #[test]
    fn the_live_catalog_has_exactly_these_nine_tools_and_no_fake_tool_fits() {
        let catalog = test_catalog();
        for name in LIVE_TOOL_NAMES {
            let tool_name = willikins_core::ToolName::parse(name).unwrap();
            let tool = catalog
                .get(&tool_name)
                .unwrap_or_else(|| panic!("the live catalog holds `{name}`"));
            tool.spec()
                .validate(willikins_types::registry())
                .unwrap_or_else(|err| panic!("`{name}`'s spec validates: {err:?}"));
        }
        assert_eq!(catalog.specs().count(), LIVE_TOOL_NAMES.len());

        let (_fake_state, fake_catalog) = willikins_providers_fake::empty();
        let mut catalog = test_catalog();
        for name in LIVE_TOOL_NAMES {
            let tool_name = willikins_core::ToolName::parse(name).unwrap();
            let fake = fake_catalog
                .get(&tool_name)
                .unwrap_or_else(|| panic!("the fake catalog holds `{name}` too"));
            catalog
                .insert(fake.clone())
                .expect_err(&format!("`{name}` is already in the live catalog"));
        }
    }
}
