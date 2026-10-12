//! `github.repo.ensure`: creates (in memory) a GitHub repository.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::rollback::{Created, Deleted, Inverse, Reversal, Standing};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{GitHubRepo, RepoVisibility, TeardownToken};

use crate::state::{FakeState, GitHubRepoRecord, repo_key};
use crate::support::{conflict, exact, get, port, require_present, scalar, tool_name};

/// `github.repo.ensure`.
pub struct GitHubRepoEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
    inverse: GitHubRepoInverse,
}

impl GitHubRepoEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "github.repo.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("visibility"), exact("RepoVisibility", true));
        inputs.insert(port("token"), exact("GitHubToken", false));
        let mut outputs = IndexMap::new();
        outputs.insert(port("repo"), scalar("GitHubRepo"));
        outputs.insert(port("url"), scalar("HttpsUrl"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a GitHub repository exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("repo")],
                class: Class::Reversible,
                pure: false,
            },
            inverse: GitHubRepoInverse {
                state: state.clone(),
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
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &repo_key(&repo));
        Ok(Self::observe(&state, &repo, inputs))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let visibility: RepoVisibility = get(inputs, "visibility")?;
        let mut state = self.state.lock().unwrap();
        let key = repo_key(&repo);
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
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
                        archived: false,
                        // Milestone 3l, SHARED VALUES "Fake create (F1)":
                        // a freshly created repository has no branches
                        // yet (GitHub's own definition of empty), and no
                        // default-branch key exists on the live create
                        // body (decision (c)), so this never names one.
                        branches: Some(Vec::new()),
                        default_branch: None,
                    },
                );
                Ok(Ensured {
                    outputs: Self::outputs_for(&repo),
                    changed: true,
                })
            }
        }
    }

    /// Milestone 3m task P6: equal to the live tool's own declaration
    /// (decisions 4, 5), field for field --
    /// `tests/catalog_parity.rs` (in `willikins-providers-github`) pins
    /// the two `Reversal`s equal.
    fn reversal(&self) -> Reversal {
        Reversal::Delete {
            needs: &["repo", "token"],
            within: &[],
            losses: &[],
        }
    }

    fn inverse(&self) -> Option<&dyn Inverse> {
        Some(&self.inverse)
    }
}

/// `github.repo.ensure`'s fake inverse (milestone 3m task P6). Never a
/// catalog tool in its own right, never reachable except from
/// `apply_teardown` (trust boundaries 1, 2) -- the same rule the live
/// tool's own `GitHubRepoInverse` follows.
///
/// This fake carries no timestamp anywhere, so unlike the live inverse it
/// never consults `created.window`/`settled_by` at all: every
/// `NotOurs`/`Drifted` row below is this crate's own model of decision
/// 7's *ownership* and *contents* rules only, never its timing rule. The
/// mock tests in `willikins-providers-github`'s own `*_inverse_mock.rs`
/// are what pin the live tool's window behaviour; this fake's own unit
/// tests below pin everything else.
struct GitHubRepoInverse {
    state: Arc<Mutex<FakeState>>,
}

/// [`Standing::NotOurs`]: no repository record exists at this key, or
/// one exists but its own `ours` flag is `false` -- this fake's single
/// collapsed row for every live "not ours" case (renamed, transferred,
/// lost the topic): it has no `full_name`/topics field to tell them
/// apart, only the one `ours` bit.
const NOT_OURS: &str = "the repository is not ours, or has been renamed or transferred away";

/// [`Standing::Drifted`]: the repository carries an Actions secret name
/// this run's own riders did not create.
const DRIFTED_FOREIGN_ACTIONS_SECRET: &str =
    "the repository carries an Actions secret this run's own riders did not create";

/// [`crate::support::conflict`]'s message when [`Inverse::delete`] is
/// called on anything but [`Standing::Gone`] or a fully accepted
/// [`Standing::AsCreated`] -- mirrors the live inverse's own constant of
/// the same shape.
const DELETE_REFUSED_NOT_AS_CREATED: &str = "the repository has changed since this run created it, or is no longer ours; the teardown \
     will not delete it";

impl GitHubRepoInverse {
    /// The `repo` key port, re-resolved from the journal (decision 2):
    /// exactly `Created::inputs`, never read a second time from anywhere
    /// else -- mirrors the live inverse's own `repo_of`.
    fn repo_of(created: &Created<'_>) -> Result<GitHubRepo, ToolError> {
        get(created.inputs, "repo")
    }

    /// Decision 7's ownership and contents rows, past the point
    /// `standing` has already confirmed a record exists and is ours.
    fn standing_from_record(
        state: &FakeState,
        repo: &GitHubRepo,
        created: &Created<'_>,
    ) -> Standing {
        let tolerated: HashSet<String> = created
            .riders
            .iter()
            .filter(|rider| rider.tool == "github.actions_secret.ensure")
            .filter_map(|rider| {
                get::<willikins_types::ActionsSecretName>(rider.inputs, "name").ok()
            })
            .map(|name| name.as_str().to_string())
            .collect();
        let prefix = format!("{repo}#");
        let foreign = state
            .github_actions_secrets
            .iter()
            .filter_map(|key| key.strip_prefix(&prefix))
            .any(|name| !tolerated.contains(name));
        if foreign {
            return Standing::Drifted {
                what: DRIFTED_FOREIGN_ACTIONS_SECRET,
            };
        }
        Standing::AsCreated { losing: Vec::new() }
    }
}

impl Inverse for GitHubRepoInverse {
    fn standing(&self, created: &Created<'_>) -> Result<Standing, ToolError> {
        let repo = Self::repo_of(created)?;
        let state = self.state.lock().unwrap();
        match state.github_repos.get(&repo_key(&repo)) {
            None => Ok(Standing::Gone),
            Some(record) if !record.ours => Ok(Standing::NotOurs { why: NOT_OURS }),
            Some(_) => Ok(Self::standing_from_record(&state, &repo, created)),
        }
    }

    fn delete(&self, created: &Created<'_>, _token: &TeardownToken) -> Result<Deleted, ToolError> {
        match self.standing(created)? {
            Standing::Gone => Ok(Deleted { already_gone: true }),
            Standing::AsCreated { losing }
                if losing.iter().all(|loss| {
                    created
                        .accept
                        .iter()
                        .any(|accepted| accepted.as_str() == *loss)
                }) =>
            {
                let repo = Self::repo_of(created)?;
                let mut state = self.state.lock().unwrap();
                state.github_repos.remove(&repo_key(&repo));
                let prefix = format!("{repo}#");
                state
                    .github_actions_secrets
                    .retain(|key| !key.starts_with(&prefix));
                state.scaffolds.retain(|key, _| !key.starts_with(&prefix));
                Ok(Deleted {
                    already_gone: false,
                })
            }
            _ => Err(conflict(DELETE_REFUSED_NOT_AS_CREATED)),
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

    /// Milestone 3l, task F1, acceptance 12: creating a repository records
    /// it as empty (`branches: Some(vec![])`), never the legacy
    /// `branches: None`, and names no default branch.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_on_creation_records_an_empty_repo() {
        let state = Arc::new(Mutex::new(FakeState::new()));
        let tool = GitHubRepoEnsure::new(state.clone());
        let token = SinkToken::new();
        tool.ensure(&inputs(RepoVisibility::Public), &token)
            .unwrap();
        let record = state
            .lock()
            .unwrap()
            .github_repos
            .get(&repo_key(&repo()))
            .unwrap()
            .clone();
        assert_eq!(record.branches, Some(Vec::new()));
        assert_eq!(record.default_branch, None);
    }

    /// Milestone 3m task P6: the fake inverse's own unit tests, parallel
    /// to the live `GitHubRepoInverse`'s mock tests but against
    /// `FakeState` directly, and never consulting a window (this crate
    /// has none).
    mod inverse {
        use super::*;
        use willikins_core::rollback::{CreationWindow, Rider, Standing};
        use willikins_core::{InstanceRef, NodeName, Timestamp};
        use willikins_types::ActionsSecretName;

        fn window() -> CreationWindow {
            CreationWindow {
                start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
                end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
            }
        }

        fn created<'a>(
            inputs: &'a Inputs,
            outputs: &'a Outputs,
            riders: &'a [Rider<'a>],
        ) -> Created<'a> {
            Created {
                inputs,
                outputs,
                window: window(),
                settled_by: window().end,
                accept: &[],
                riders,
            }
        }

        #[test]
        fn declares_delete_with_no_within_and_no_loss() {
            assert_eq!(
                tool().reversal(),
                Reversal::Delete {
                    needs: &["repo", "token"],
                    within: &[],
                    losses: &[],
                }
            );
        }

        #[test]
        fn standing_reports_gone_when_absent() {
            let tool = tool();
            let inverse = Tool::inverse(&tool).unwrap();
            let inputs = Inputs::new();
            let mut inputs = inputs;
            inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
            let outputs = Outputs::new();
            assert_eq!(
                inverse.standing(&created(&inputs, &outputs, &[])).unwrap(),
                Standing::Gone
            );
        }

        #[test]
        fn standing_reports_not_ours_when_foreign() {
            let state = Arc::new(Mutex::new(FakeState::new().with_repo(
                &repo(),
                RepoVisibility::Private,
                false,
            )));
            let tool = GitHubRepoEnsure::new(state);
            let inverse = Tool::inverse(&tool).unwrap();
            let mut inputs = Inputs::new();
            inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
            let outputs = Outputs::new();
            assert_eq!(
                inverse.standing(&created(&inputs, &outputs, &[])).unwrap(),
                Standing::NotOurs { why: NOT_OURS }
            );
        }

        #[test]
        fn standing_reports_as_created_when_ours_and_no_foreign_secret() {
            let state = Arc::new(Mutex::new(FakeState::new().with_repo(
                &repo(),
                RepoVisibility::Private,
                true,
            )));
            let tool = GitHubRepoEnsure::new(state);
            let inverse = Tool::inverse(&tool).unwrap();
            let mut inputs = Inputs::new();
            inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
            let outputs = Outputs::new();
            assert_eq!(
                inverse.standing(&created(&inputs, &outputs, &[])).unwrap(),
                Standing::AsCreated { losing: Vec::new() }
            );
        }

        #[test]
        fn standing_reports_drifted_on_an_untolerated_actions_secret() {
            let state = Arc::new(Mutex::new(
                FakeState::new()
                    .with_repo(&repo(), RepoVisibility::Private, true)
                    .with_actions_secret(&repo(), &ActionsSecretName::parse("TOKEN").unwrap()),
            ));
            let tool = GitHubRepoEnsure::new(state);
            let inverse = Tool::inverse(&tool).unwrap();
            let mut inputs = Inputs::new();
            inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
            let outputs = Outputs::new();
            assert_eq!(
                inverse.standing(&created(&inputs, &outputs, &[])).unwrap(),
                Standing::Drifted {
                    what: DRIFTED_FOREIGN_ACTIONS_SECRET
                }
            );
        }

        #[test]
        fn standing_tolerates_an_actions_secret_a_rider_created() {
            let state = Arc::new(Mutex::new(
                FakeState::new()
                    .with_repo(&repo(), RepoVisibility::Private, true)
                    .with_actions_secret(&repo(), &ActionsSecretName::parse("TOKEN").unwrap()),
            ));
            let tool = GitHubRepoEnsure::new(state);
            let inverse = Tool::inverse(&tool).unwrap();
            let mut inputs = Inputs::new();
            inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
            let outputs = Outputs::new();
            let rider_instance = InstanceRef {
                node: NodeName::parse("secret").unwrap(),
                instance: None,
            };
            let mut rider_inputs = Inputs::new();
            rider_inputs.insert(
                PortName::parse("name").unwrap(),
                Value::known(ActionsSecretName::parse("TOKEN").unwrap()),
            );
            let rider_outputs = Outputs::new();
            let riders = [Rider {
                tool: "github.actions_secret.ensure",
                instance: &rider_instance,
                inputs: &rider_inputs,
                outputs: &rider_outputs,
            }];
            assert_eq!(
                inverse
                    .standing(&created(&inputs, &outputs, &riders))
                    .unwrap(),
                Standing::AsCreated { losing: Vec::new() }
            );
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn delete_removes_the_repo_and_its_actions_secrets_and_scaffolds() {
            let state = Arc::new(Mutex::new(
                FakeState::new()
                    .with_repo(&repo(), RepoVisibility::Private, true)
                    .with_actions_secret(&repo(), &ActionsSecretName::parse("TOKEN").unwrap())
                    .with_scaffold_files(
                        &repo(),
                        &willikins_types::GitBranchName::parse("main").unwrap(),
                        &[("README.md", "hi")],
                    ),
            ));
            let tool = GitHubRepoEnsure::new(state.clone());
            let inverse = Tool::inverse(&tool).unwrap();
            let mut inputs = Inputs::new();
            inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
            let outputs = Outputs::new();
            // The one Actions secret is a rider this run itself created
            // (decision 3's "riding is not tolerance"), so `standing`
            // tolerates it rather than refusing as drift.
            let rider_instance = InstanceRef {
                node: NodeName::parse("secret").unwrap(),
                instance: None,
            };
            let mut rider_inputs = Inputs::new();
            rider_inputs.insert(
                PortName::parse("name").unwrap(),
                Value::known(ActionsSecretName::parse("TOKEN").unwrap()),
            );
            let rider_outputs = Outputs::new();
            let riders = [Rider {
                tool: "github.actions_secret.ensure",
                instance: &rider_instance,
                inputs: &rider_inputs,
                outputs: &rider_outputs,
            }];
            let token = willikins_types::TeardownToken::new();
            let deleted = inverse
                .delete(&created(&inputs, &outputs, &riders), &token)
                .unwrap();
            assert!(!deleted.already_gone);
            let locked = state.lock().unwrap();
            assert!(!locked.github_repos.contains_key(&repo_key(&repo())));
            assert!(locked.github_actions_secrets.is_empty());
            assert!(locked.scaffolds.is_empty());
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn delete_on_an_already_gone_repo_reports_already_gone() {
            let tool = tool();
            let inverse = Tool::inverse(&tool).unwrap();
            let mut inputs = Inputs::new();
            inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
            let outputs = Outputs::new();
            let token = willikins_types::TeardownToken::new();
            let deleted = inverse
                .delete(&created(&inputs, &outputs, &[]), &token)
                .unwrap();
            assert!(deleted.already_gone);
        }

        #[test]
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        fn delete_refuses_a_drifted_repo_and_changes_nothing() {
            let state = Arc::new(Mutex::new(
                FakeState::new()
                    .with_repo(&repo(), RepoVisibility::Private, true)
                    .with_actions_secret(&repo(), &ActionsSecretName::parse("TOKEN").unwrap()),
            ));
            let tool = GitHubRepoEnsure::new(state.clone());
            let inverse = Tool::inverse(&tool).unwrap();
            let mut inputs = Inputs::new();
            inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
            let outputs = Outputs::new();
            let token = willikins_types::TeardownToken::new();
            let err = inverse
                .delete(&created(&inputs, &outputs, &[]), &token)
                .unwrap_err();
            assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
            let locked = state.lock().unwrap();
            assert!(locked.github_repos.contains_key(&repo_key(&repo())));
        }
    }
}
