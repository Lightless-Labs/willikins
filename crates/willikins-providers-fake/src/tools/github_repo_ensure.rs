//! `github.repo.ensure`: creates (in memory) a GitHub repository.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{GitHubRepo, RepoVisibility};

use crate::state::{FakeState, GitHubRepoRecord, repo_key};
use crate::support::{conflict, exact, get, port, require_present, scalar, tool_name};

/// `github.repo.ensure`.
pub struct GitHubRepoEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl GitHubRepoEnsure {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("visibility"), exact("RepoVisibility", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("repo"), scalar("GitHubRepo"));
        outputs.insert(port("url"), scalar("HttpsUrl"));
        Self {
            spec: ToolSpec {
                name: tool_name("github.repo.ensure"),
                description: "Ensure a GitHub repository exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("repo")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    /// The outputs a `Present` or newly `ensure`d repository reports:
    /// its own identity and its `https://` URL.
    fn outputs_for(repo: &GitHubRepo) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("repo"), Value::known(repo.clone()));
        outputs.insert(port("url"), Value::known(repo.url()));
        outputs
    }
}

impl Tool for GitHubRepoEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let state = self.state.lock().unwrap();
        match state.github_repos.get(&repo_key(&repo)) {
            None => Ok(Observation::Absent {
                predicted: Self::outputs_for(&repo),
            }),
            Some(record) if record.ours => Ok(Observation::Present(Self::outputs_for(&repo))),
            Some(_) => Ok(Observation::Foreign),
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let visibility: RepoVisibility = get(inputs, "visibility")?;
        let mut state = self.state.lock().unwrap();
        if let Some(existing) = state.github_repos.get(&repo_key(&repo))
            && !existing.ours
        {
            return Err(conflict(format!("`{repo}` already exists and is not ours")));
        }
        state.github_repos.insert(
            repo_key(&repo),
            GitHubRepoRecord {
                visibility,
                ours: true,
            },
        );
        Ok(Self::outputs_for(&repo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::DomainType;

    fn repo() -> GitHubRepo {
        GitHubRepo::parse("lightless-labs/third-thoughts").unwrap()
    }

    fn inputs(visibility: RepoVisibility) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        inputs.insert(
            PortName::parse("visibility").unwrap(),
            Value::known(visibility),
        );
        inputs
    }

    fn tool() -> GitHubRepoEnsure {
        GitHubRepoEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_with_predicted_outputs_when_empty() {
        let observation = tool().read(&inputs(RepoVisibility::Private)).unwrap();
        let Observation::Absent { predicted } = observation else {
            panic!("expected Absent, got {observation:?}");
        };
        let url = predicted.get(&PortName::parse("url").unwrap()).unwrap();
        assert_eq!(
            url.render().to_string(),
            "https://github.com/lightless-labs/third-thoughts"
        );
    }

    #[test]
    fn read_reports_present_when_seeded_and_ours() {
        let state = Arc::new(Mutex::new(FakeState::new().with_repo(
            &repo(),
            RepoVisibility::Private,
            true,
        )));
        let tool = GitHubRepoEnsure::new(state);
        let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_reports_foreign_when_seeded_and_not_ours() {
        let state = Arc::new(Mutex::new(FakeState::new().with_repo(
            &repo(),
            RepoVisibility::Private,
            false,
        )));
        let tool = GitHubRepoEnsure::new(state);
        let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
        assert!(matches!(observation, Observation::Foreign));
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut bad_inputs = inputs(RepoVisibility::Private);
        bad_inputs.insert(
            PortName::parse("repo").unwrap(),
            Value::unknown(willikins_core::TypeRef::scalar(
                willikins_core::TypeName::parse("GitHubRepo").unwrap(),
            )),
        );
        let err = tool().read(&bad_inputs).unwrap_err();
        assert!(err.message.contains("repo"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let mut bad_inputs = Inputs::new();
        bad_inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        let err = tool().read(&bad_inputs).unwrap_err();
        assert!(err.message.contains("visibility"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_on_a_foreign_repo_conflicts_and_does_not_overwrite() {
        let state = Arc::new(Mutex::new(FakeState::new().with_repo(
            &repo(),
            RepoVisibility::Public,
            false,
        )));
        let tool = GitHubRepoEnsure::new(state);
        let token = SinkToken::new();
        let err = tool
            .ensure(&inputs(RepoVisibility::Private), &token)
            .unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(err.message.contains("lightless-labs/third-thoughts"));
        let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
        assert!(matches!(observation, Observation::Foreign));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_then_read_gives_present() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&inputs(RepoVisibility::Public), &token)
            .unwrap();
        let observation = tool.read(&inputs(RepoVisibility::Public)).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let url = outputs.get(&PortName::parse("url").unwrap()).unwrap();
        assert_eq!(
            url.render().to_string(),
            "https://github.com/lightless-labs/third-thoughts"
        );
    }
}
