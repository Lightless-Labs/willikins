//! `github.repo.ensure`: creates a real GitHub repository. Port table and
//! behaviour identical to `willikins_providers_fake`'s tool of the same
//! name (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{GitHubRepo, RepoVisibility};

use crate::client::{GitHubClient, RepoBody, to_tool_error};

/// `github.repo.ensure`.
pub struct GitHubRepoEnsure {
    spec: ToolSpec,
    client: Arc<GitHubClient>,
}

impl GitHubRepoEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<GitHubClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("visibility"), exact("RepoVisibility", true));
        let mut outputs = indexmap::IndexMap::new();
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
            client,
        }
    }

    /// The outputs a `Present` or newly created repository reports.
    fn outputs_for(repo: &GitHubRepo) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("repo"), Value::known(repo.clone()));
        outputs.insert(port("url"), Value::known(repo.url()));
        outputs
    }

    /// The requested `visibility` input, when bound and
    /// [`Value::is_known`]. See the fake tool's identical helper: a not-
    /// yet-known `visibility` at plan time is "no mismatch determinable
    /// from here", not an error.
    fn requested_visibility(inputs: &Inputs) -> Option<RepoVisibility> {
        inputs
            .get(&port("visibility"))
            .filter(|value| value.is_known())
            .and_then(|value| value.downcast::<RepoVisibility>())
            .copied()
    }

    /// The `Conflict` a foreign (not-ours) repository at this key
    /// produces, shared between the direct `Foreign` observation and the
    /// "re-read after an ambiguous create" path.
    fn foreign_conflict(repo: &GitHubRepo) -> ToolError {
        conflict(format!("`{repo}` already exists and is not ours"))
    }

    /// The `Conflict` an owned-but-visibility-mismatched repository at
    /// this key produces, shared the same way as [`Self::foreign_conflict`].
    fn mismatch_conflict(repo: &GitHubRepo) -> ToolError {
        conflict(format!(
            "`{repo}` is ours, but its visibility does not match what was requested and \
             this tool will not change it; change it by hand, or pass its current value \
             instead"
        ))
    }

    /// `GET /repos/{owner}/{name}`, mapped to an [`Observation`]. Shared
    /// by `read` and `ensure`.
    fn observe(&self, repo: &GitHubRepo, inputs: &Inputs) -> Result<Observation, ToolError> {
        match self.client.get_repo(repo) {
            Ok(RepoBody { visibility, topics }) => {
                if !topics.iter().any(|topic| topic == crate::MANAGED_TOPIC) {
                    return Ok(Observation::Foreign);
                }
                match Self::requested_visibility(inputs) {
                    Some(requested) if requested != visibility => Ok(Observation::Mismatch {
                        port: port("visibility"),
                    }),
                    _ => Ok(Observation::Present(Self::outputs_for(repo))),
                }
            }
            // GitHub answers 404 both for a truly missing repository and
            // for one this credential cannot see — the same conflation
            // `willikins_providers_fake`'s tool models.
            Err(err) if err.status == Some(404) => Ok(Observation::Absent {
                predicted: Self::outputs_for(repo),
            }),
            // A renamed repository's old owner/name 301s rather than
            // 404ing; treated as Foreign, same as a repository that
            // exists but lost its ownership topic.
            Err(err) if err.status == Some(301) => Ok(Observation::Foreign),
            Err(err) => Err(to_tool_error(err)),
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
        self.observe(&repo, inputs)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let visibility: RepoVisibility = get(inputs, "visibility")?;
        match self.observe(&repo, inputs)? {
            Observation::Foreign => Err(Self::foreign_conflict(&repo)),
            Observation::Mismatch { .. } => Err(Self::mismatch_conflict(&repo)),
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { .. } => match self.client.create_repo(&repo, visibility) {
                Ok(()) => {
                    self.client
                        .put_managed_topic(&repo)
                        .map_err(to_tool_error)?;
                    Ok(Ensured {
                        outputs: Self::outputs_for(&repo),
                        changed: true,
                    })
                }
                // The create may have landed despite the error (a retried
                // ambiguous failure, or simply a name already taken):
                // re-read rather than assume either way.
                Err(err) if err.already_exists => match self.observe(&repo, inputs)? {
                    Observation::Present(outputs) => Ok(Ensured {
                        outputs,
                        changed: false,
                    }),
                    Observation::Mismatch { .. } => Err(Self::mismatch_conflict(&repo)),
                    _ => Err(Self::foreign_conflict(&repo)),
                },
                Err(err) => Err(to_tool_error(err)),
            },
        }
    }
}
