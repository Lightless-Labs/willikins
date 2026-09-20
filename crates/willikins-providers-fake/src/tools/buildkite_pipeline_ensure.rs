//! `buildkite.pipeline.ensure`: creates (in memory) a Buildkite pipeline.
//! Mirrors `willikins_providers_buildkite::tools::BuildkitePipelineEnsure`'s
//! `ToolSpec` exactly (`tests/catalog_parity.rs`, in
//! `willikins-providers-buildkite`, pins the two equal) and the same four
//! observations: `Absent`, `Present`, `Foreign`, and both `Mismatch`
//! arms (`repo`, then `cluster`).

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    BuildkiteClusterId, BuildkiteOrg, BuildkitePipelineSlug, DomainType, GitHubRepo,
};

use crate::state::{BuildkitePipelineRecord, FakeState, buildkite_pipeline_key};
use crate::support::{conflict, exact, get, port, require_present, scalar, tool_name};

/// The SSH repository URL this fake pipeline points at, exactly the form
/// `willikins_providers_buildkite::ssh_repository_url` builds -- kept as
/// its own local copy rather than a dependency on that crate, the same
/// way every other fake tool in this crate never depends on its live
/// counterpart's crate.
fn ssh_repository_url(repo: &GitHubRepo) -> String {
    format!("git@github.com:{}/{}.git", repo.owner(), repo.name())
}

/// `buildkite.pipeline.ensure`.
pub struct FakeBuildkitePipelineEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeBuildkitePipelineEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "buildkite.pipeline.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("org"), exact("BuildkiteOrg", true));
        inputs.insert(port("slug"), exact("BuildkitePipelineSlug", true));
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("cluster"), exact("BuildkiteClusterId", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("slug"), scalar("BuildkitePipelineSlug"));
        outputs.insert(port("url"), scalar("HttpsUrl"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a Buildkite pipeline exists in a cluster, pointed at a GitHub repository.".to_string(),
                inputs,
                outputs,
                key: vec![port("org"), port("slug")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn outputs_for(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("slug"), Value::known(slug.clone()));
        outputs.insert(
            port("url"),
            Value::known(
                willikins_types::HttpsUrl::parse(&format!("https://buildkite.com/{org}/{slug}"))
                    .expect("org and slug always join into a valid HttpsUrl"),
            ),
        );
        outputs
    }

    fn observe(
        state: &FakeState,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        repo: &GitHubRepo,
        cluster: &BuildkiteClusterId,
    ) -> Observation {
        match state
            .buildkite_pipelines
            .get(&buildkite_pipeline_key(org, slug))
        {
            None => Observation::Absent {
                predicted: Self::outputs_for(org, slug),
            },
            Some(record) if !record.ours => Observation::Foreign,
            Some(record) if record.repository != ssh_repository_url(repo) => {
                Observation::Mismatch { port: port("repo") }
            }
            Some(record) if record.cluster_id != cluster.to_string() => Observation::Mismatch {
                port: port("cluster"),
            },
            Some(_) => Observation::Present(Self::outputs_for(org, slug)),
        }
    }
}

impl Tool for FakeBuildkitePipelineEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let cluster: BuildkiteClusterId = get(inputs, "cluster")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &buildkite_pipeline_key(&org, &slug));
        Ok(Self::observe(&state, &org, &slug, &repo, &cluster))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let cluster: BuildkiteClusterId = get(inputs, "cluster")?;
        let mut state = self.state.lock().unwrap();
        let key = buildkite_pipeline_key(&org, &slug);
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        match Self::observe(&state, &org, &slug, &repo, &cluster) {
            Observation::Foreign => Err(conflict(format!(
                "`{org}/{slug}` already exists and is not ours"
            ))),
            Observation::Mismatch { port } => Err(conflict(format!(
                "`{org}/{slug}` is ours, but its `{port}` does not match what was requested and \
                 this tool will not change it; change it by hand, or pass its current value \
                 instead"
            ))),
            Observation::Present(_) => Ok(Ensured {
                outputs: Self::outputs_for(&org, &slug),
                changed: false,
            }),
            Observation::Absent { .. } => {
                state.buildkite_pipelines.insert(
                    key,
                    BuildkitePipelineRecord {
                        repository: ssh_repository_url(&repo),
                        cluster_id: cluster.to_string(),
                        ours: true,
                    },
                );
                Ok(Ensured {
                    outputs: Self::outputs_for(&org, &slug),
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
    use willikins_types::{DomainType, GitHubOrg, ProjectSlug};

    fn org() -> BuildkiteOrg {
        BuildkiteOrg::parse("willikins-test").unwrap()
    }

    fn slug() -> BuildkitePipelineSlug {
        BuildkitePipelineSlug::parse("third-thoughts").unwrap()
    }

    fn repo() -> GitHubRepo {
        GitHubRepo::new(
            GitHubOrg::parse("lightless-labs").unwrap(),
            ProjectSlug::parse("third-thoughts").unwrap(),
        )
    }

    fn cluster() -> BuildkiteClusterId {
        BuildkiteClusterId::parse("018e5a22-d14c-7085-bb28-db0f83f43a1c").unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("org").unwrap(), Value::known(org()));
        inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug()));
        inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        inputs.insert(PortName::parse("cluster").unwrap(), Value::known(cluster()));
        inputs
    }

    fn tool() -> FakeBuildkitePipelineEnsure {
        FakeBuildkitePipelineEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_empty() {
        let observation = tool().read(&inputs()).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_then_read_gives_present_and_changed_then_unchanged() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&inputs(), &token).unwrap();
        assert!(first.changed);
        let second = tool.ensure(&inputs(), &token).unwrap();
        assert!(!second.changed);
        let observation = tool.read(&inputs()).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_reports_foreign_when_seeded_without_ours() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            BuildkitePipelineRecord {
                repository: ssh_repository_url(&repo()),
                cluster_id: cluster().to_string(),
                ours: false,
            },
        )));
        let tool = FakeBuildkitePipelineEnsure::new(state);
        let observation = tool.read(&inputs()).unwrap();
        assert!(matches!(observation, Observation::Foreign));
    }

    #[test]
    fn read_reports_mismatch_on_repository_before_cluster() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            BuildkitePipelineRecord {
                repository: "git@github.com:lightless-labs/other.git".to_string(),
                cluster_id: "different-cluster".to_string(),
                ours: true,
            },
        )));
        let tool = FakeBuildkitePipelineEnsure::new(state);
        let observation = tool.read(&inputs()).unwrap();
        assert!(matches!(
            observation,
            Observation::Mismatch { port } if port == PortName::parse("repo").unwrap()
        ));
    }

    #[test]
    fn read_reports_mismatch_on_cluster_when_repository_matches() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            BuildkitePipelineRecord {
                repository: ssh_repository_url(&repo()),
                cluster_id: "different-cluster".to_string(),
                ours: true,
            },
        )));
        let tool = FakeBuildkitePipelineEnsure::new(state);
        let observation = tool.read(&inputs()).unwrap();
        assert!(matches!(
            observation,
            Observation::Mismatch { port } if port == PortName::parse("cluster").unwrap()
        ));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_on_a_foreign_pipeline_conflicts_and_does_not_overwrite() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            BuildkitePipelineRecord {
                repository: ssh_repository_url(&repo()),
                cluster_id: cluster().to_string(),
                ours: false,
            },
        )));
        let tool = FakeBuildkitePipelineEnsure::new(state);
        let token = SinkToken::new();
        let err = tool.ensure(&inputs(), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        let observation = tool.read(&inputs()).unwrap();
        assert!(matches!(observation, Observation::Foreign));
    }
}
