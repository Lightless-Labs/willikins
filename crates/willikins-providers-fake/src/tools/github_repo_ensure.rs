//! `github.repo.ensure`: creates (in memory) a GitHub repository.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
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

    /// The requested `visibility` input, when it is bound and
    /// [`Value::is_known`]. A tool whose static `plan_one` guarantee only
    /// covers key ports may see this port `Unknown`; treated as "no
    /// mismatch determinable from here" rather than an error, matching
    /// `github.actions_secret.ensure`'s own treatment of a not-yet-known
    /// `value` in `read`.
    fn requested_visibility(inputs: &Inputs) -> Option<RepoVisibility> {
        inputs
            .get(&port("visibility"))
            .filter(|value| value.is_known())
            .and_then(|value| value.downcast::<RepoVisibility>())
            .copied()
    }

    /// This tool's own read logic, without a token: the resource at
    /// `repo`'s key, observed against `state` and `inputs`' requested
    /// `visibility`. Shared by `read` (which locks `self.state` and calls
    /// this) and `ensure` (which locks once, calls this, then mutates) —
    /// `ensure` must never call `self.read()` directly, since that would
    /// try to lock `self.state` a second time and deadlock.
    fn observe(state: &FakeState, repo: &GitHubRepo, inputs: &Inputs) -> Observation {
        match state.github_repos.get(&repo_key(repo)) {
            None => Observation::Absent {
                predicted: Self::outputs_for(repo),
            },
            Some(record) if !record.ours => Observation::Foreign,
            Some(record) => match Self::requested_visibility(inputs) {
                Some(requested) if requested != record.visibility => Observation::Mismatch {
                    port: port("visibility"),
                },
                _ => Observation::Present(Self::outputs_for(repo)),
            },
        }
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
        Ok(Self::observe(&state, &repo, inputs))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let visibility: RepoVisibility = get(inputs, "visibility")?;
        let mut state = self.state.lock().unwrap();
        match Self::observe(&state, &repo, inputs) {
            Observation::Foreign => {
                Err(conflict(format!("`{repo}` already exists and is not ours")))
            }
            Observation::Mismatch { .. } => Err(conflict(format!(
                "`{repo}` is ours, but its visibility does not match what was requested and \
                 this tool will not change it; change it by hand, or pass its current value \
                 instead"
            ))),
            Observation::Present(_) => Ok(Ensured {
                outputs: Self::outputs_for(&repo),
                changed: false,
            }),
            Observation::Absent { .. } => {
                state.github_repos.insert(
                    repo_key(&repo),
                    GitHubRepoRecord {
                        visibility,
                        ours: true,
                    },
                );
                Ok(Ensured {
                    outputs: Self::outputs_for(&repo),
                    changed: true,
                })
            }
        }
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

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_on_creation_and_false_on_a_second_call() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool
            .ensure(&inputs(RepoVisibility::Public), &token)
            .unwrap();
        assert!(first.changed, "creating the repo must report changed");
        let second = tool
            .ensure(&inputs(RepoVisibility::Public), &token)
            .unwrap();
        assert!(
            !second.changed,
            "ensure on an already-matching repo must report changed: false"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_on_a_visibility_mismatch_conflicts_and_does_not_change_state() {
        let state = Arc::new(Mutex::new(FakeState::new().with_repo(
            &repo(),
            RepoVisibility::Public,
            true,
        )));
        let tool = GitHubRepoEnsure::new(state);
        let token = SinkToken::new();
        let err = tool
            .ensure(&inputs(RepoVisibility::Private), &token)
            .unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        // The state must be untouched: still `Public`, still ours.
        let observation = tool.read(&inputs(RepoVisibility::Public)).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    /// The refusal is symmetric: private-to-public is refused too, not
    /// just public-to-private.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_on_the_reverse_visibility_mismatch_also_conflicts() {
        let state = Arc::new(Mutex::new(FakeState::new().with_repo(
            &repo(),
            RepoVisibility::Private,
            true,
        )));
        let tool = GitHubRepoEnsure::new(state);
        let token = SinkToken::new();
        let err = tool
            .ensure(&inputs(RepoVisibility::Public), &token)
            .unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
    }

    #[test]
    fn read_reports_mismatch_when_ours_but_visibility_differs() {
        let state = Arc::new(Mutex::new(FakeState::new().with_repo(
            &repo(),
            RepoVisibility::Public,
            true,
        )));
        let tool = GitHubRepoEnsure::new(state);
        let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
        assert!(matches!(
            observation,
            Observation::Mismatch { port } if port == PortName::parse("visibility").unwrap()
        ));
    }

    /// A present-and-ours repository whose `visibility` input is bound but
    /// not yet [`Value::is_known`] cannot be statically compared, so
    /// `read` reports `Present` rather than guessing at a mismatch;
    /// `ensure` still enforces the real value once it is known.
    #[test]
    fn read_reports_present_when_ours_and_visibility_is_unknown() {
        let state = Arc::new(Mutex::new(FakeState::new().with_repo(
            &repo(),
            RepoVisibility::Public,
            true,
        )));
        let tool = GitHubRepoEnsure::new(state);
        let mut inputs = inputs(RepoVisibility::Public);
        inputs.insert(
            PortName::parse("visibility").unwrap(),
            Value::unknown(willikins_core::TypeRef::scalar(
                willikins_core::TypeName::parse("RepoVisibility").unwrap(),
            )),
        );
        let observation = tool.read(&inputs).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }
}
