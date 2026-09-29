//! `github.repo.get`: reference an existing GitHub repository, against
//! seeded state. Mirrors
//! `willikins_providers_github::tools::GitHubRepoGet`'s `ToolSpec` exactly
//! (`tests/catalog_parity.rs`, in `willikins-providers-github`, pins the
//! two equal) and its three outcomes: absent, present, archived.
//!
//! Unlike `github.repo.ensure`'s `Foreign`, this tool never checks
//! ownership: a seeded repository is `Present` whether or not it is ours
//! (`ours: false` still resolves), matching the live tool's own "whoever
//! owns it (no topic check)" (milestone 3e plan, decision (g)).

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::GitHubRepo;

use crate::state::{FakeState, repo_key};
use crate::support::{conflict, exact, get, not_found, port, require_present, scalar, tool_name};

/// `github.repo.get`.
pub struct FakeGitHubRepoGet {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeGitHubRepoGet {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("token"), exact("GitHubToken", false));
        let mut outputs = IndexMap::new();
        outputs.insert(port("repo"), scalar("GitHubRepo"));
        Self {
            spec: ToolSpec {
                name: tool_name("github.repo.get"),
                description: "Reference an existing GitHub repository, checked against the \
                               live API."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
            state,
        }
    }

    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let state = self.state.lock().unwrap();
        match state.github_repos.get(&repo_key(&repo)) {
            None => Err(not_found(format!(
                "no GitHub repository `{repo}` (or the credential cannot see it)"
            ))),
            Some(record) if record.archived => Err(conflict(format!(
                "`{repo}` is archived; a pipeline could never build on it"
            ))),
            Some(_) => {
                let mut outputs = Outputs::new();
                outputs.insert(port("repo"), Value::known(repo));
                Ok(outputs)
            }
        }
    }
}

impl Tool for FakeGitHubRepoGet {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.lookup(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.lookup(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::DomainType;

    fn repo() -> GitHubRepo {
        GitHubRepo::parse("example-org/monorepo").unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        inputs
    }

    fn tool(state: FakeState) -> FakeGitHubRepoGet {
        FakeGitHubRepoGet::new(Arc::new(Mutex::new(state)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool(FakeState::new())
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn spec_has_no_key_and_is_pure() {
        let tool = tool(FakeState::new());
        assert!(tool.spec().key.is_empty());
        assert!(tool.spec().pure);
    }

    #[test]
    fn read_reports_not_found_when_absent() {
        let err = tool(FakeState::new()).read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::NotFound);
    }

    #[test]
    fn read_reports_present_when_seeded_and_ours() {
        let state =
            FakeState::new().with_repo(&repo(), willikins_types::RepoVisibility::Private, true);
        let observation = tool(state).read(&inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let out_repo = outputs.get(&PortName::parse("repo").unwrap()).unwrap();
        assert_eq!(out_repo.render().to_string(), "example-org/monorepo");
    }

    /// Ownership is not checked: a repository seeded as foreign still
    /// resolves as `Present`, unlike `github.repo.ensure`'s `Foreign`.
    #[test]
    fn read_reports_present_when_seeded_and_foreign() {
        let state =
            FakeState::new().with_repo(&repo(), willikins_types::RepoVisibility::Private, false);
        let observation = tool(state).read(&inputs()).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_reports_conflict_when_archived() {
        let state = FakeState::new().with_archived_repo(&repo());
        let err = tool(state).read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(err.message.contains("archived"), "{}", err.message);
    }
}
