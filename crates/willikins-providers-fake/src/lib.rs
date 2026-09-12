//! In-memory GitHub and Doppler tools for tests and the milestone 1 CLI.
//!
//! See `docs/plans/2026-09-11-milestone-1-core.md` for the crate contract.

pub mod state;
mod support;
pub mod tools;

use std::sync::{Arc, Mutex};

pub use state::FakeState;
use willikins_core::Catalog;

/// Register every fake tool against `state` into a fresh [`Catalog`].
///
/// # Panics
///
/// Panics if a tool's own spec fails [`willikins_core::ToolSpec::validate`]
/// against the global type registry, or if two fake tools share a name —
/// both would be a bug in this crate's own tool specs, never in caller
/// input.
#[must_use]
pub fn catalog(state: Arc<Mutex<FakeState>>) -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    macro_rules! insert {
        ($tool:expr) => {
            catalog
                .insert(Arc::new($tool))
                .unwrap_or_else(|err| unreachable!("fake tool spec is invalid: {err}"));
        };
    }
    insert!(tools::NamingV1::new());
    insert!(tools::GitHubRepoEnsure::new(state.clone()));
    insert!(tools::GitHubActionsSecretEnsure::new(state.clone()));
    insert!(tools::DopplerProjectEnsure::new(state.clone()));
    insert!(tools::DopplerConfigEnsure::new(state.clone()));
    insert!(tools::DopplerServiceTokenEnsure::new(state.clone()));
    insert!(tools::DopplerSecretGet::new(state.clone()));
    insert!(tools::FakeSecretList::new());
    // The last use of `state`: moved rather than cloned, so this
    // function's own `state` parameter is genuinely consumed.
    insert!(tools::FakeIrreversibleEnsure::new(state));
    insert!(tools::TemplateRender::new());
    catalog
}

/// A fresh, empty [`FakeState`] and a [`Catalog`] of every fake tool
/// registered against it, sharing that state.
#[must_use]
pub fn empty() -> (Arc<Mutex<FakeState>>, Catalog) {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = catalog(Arc::clone(&state));
    (state, catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::ToolName;

    #[test]
    fn catalog_registers_every_fake_tool() {
        let (_state, catalog) = empty();
        let names: Vec<&str> = catalog.specs().map(|spec| spec.name.as_str()).collect();
        for expected in [
            "naming.v1",
            "github.repo.ensure",
            "github.actions_secret.ensure",
            "doppler.project.ensure",
            "doppler.config.ensure",
            "doppler.service_token.ensure",
            "doppler.secret.get",
            "fake.secret_list",
            "fake.irreversible.ensure",
            "template.render",
        ] {
            assert!(names.contains(&expected), "missing tool `{expected}`");
        }
        assert_eq!(names.len(), 10);
    }

    #[test]
    fn every_tool_spec_validates_against_the_registry() {
        let (_state, catalog) = empty();
        for spec in catalog.specs() {
            spec.validate(catalog.registry()).unwrap_or_else(|err| {
                panic!("{}: {err}", spec.name);
            });
        }
    }

    #[test]
    fn catalog_get_finds_a_registered_tool_by_name() {
        let (_state, catalog) = empty();
        assert!(
            catalog
                .get(&ToolName::parse("naming.v1").unwrap())
                .is_some()
        );
    }

    #[test]
    fn catalog_json_snapshot() {
        let (_state, catalog) = empty();
        insta::assert_json_snapshot!(catalog.list_tools_json()["tools"]);
    }

    #[test]
    fn seeded_fixture_loads_and_reports_the_seeded_repo_as_present() {
        use willikins_core::{Inputs, Observation, PortName, Value};
        use willikins_types::{DomainType, GitHubRepo, RepoVisibility};

        let json =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/seeded.json"))
                .unwrap();
        let state = Arc::new(Mutex::new(FakeState::from_json(&json).unwrap()));
        let catalog = catalog(state);
        let tool = catalog
            .get(&ToolName::parse("github.repo.ensure").unwrap())
            .unwrap();
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("repo").unwrap(),
            Value::known(GitHubRepo::parse("lightless-labs/third-thoughts").unwrap()),
        );
        inputs.insert(
            PortName::parse("visibility").unwrap(),
            Value::known(RepoVisibility::Private),
        );
        let observation = tool.read(&inputs).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }
}
