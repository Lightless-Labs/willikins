//! `buildkite.pipeline.bootstrap.gate`: mirrors
//! `willikins_providers_buildkite::tools::BuildkitePipelineBootstrapGate`
//! -- see that crate's own module doc for the full reasoning (milestone
//! 3g task B1). Not a leaf: it passes `slug` through as its own output,
//! exactly like the live tool.
//!
//! Compares the seeded pipeline's own
//! [`BuildkitePipelineRecord::configuration`](crate::state::BuildkitePipelineRecord::configuration)
//! against `expected`'s content structurally (parsed as YAML), the same
//! way the live tool compares Buildkite's own stored `configuration` --
//! [`structurally_equal`] is its own local copy of the live crate's
//! function, the same duplication [`crate::tools::buildkite_pipeline_ensure`]'s
//! own `ssh_repository_url` copy makes, for the same reason (a fake tool
//! never depends on its live counterpart's crate).

use std::sync::{Arc, Mutex};

use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{BuildkiteOrg, BuildkitePipelineSlug, RepoFile};

use crate::state::{FakeState, buildkite_pipeline_key};
use crate::support::{exact, get, port, require_present, scalar, tool_name};

static GATE: Gate = Gate {
    need: "this pipeline's stored configuration equals the bootstrap this document renders",
    how: "in the pipeline's Settings, Steps page, replace the YAML with the rendered bootstrap \
          file this document committed, save, then re-run this document",
    subject: &["org", "slug"],
};

/// Parse `a` and `b` as YAML into a JSON value and compare for
/// structural equality -- the fake's own copy of
/// `willikins_providers_buildkite::tools::pipeline_bootstrap_gate::structurally_equal`.
fn structurally_equal(a: &str, b: &str) -> bool {
    let parsed_a = serde_yaml_ng::from_str::<serde_json::Value>(a);
    let parsed_b = serde_yaml_ng::from_str::<serde_json::Value>(b);
    matches!((parsed_a, parsed_b), (Ok(a), Ok(b)) if a == b)
}

/// `buildkite.pipeline.bootstrap.gate`.
pub struct FakeBuildkitePipelineBootstrapGate {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeBuildkitePipelineBootstrapGate {
    const TOOL_NAME: &'static str = "buildkite.pipeline.bootstrap.gate";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("org"), exact("BuildkiteOrg", true));
        inputs.insert(port("slug"), exact("BuildkitePipelineSlug", true));
        inputs.insert(port("expected"), exact("RepoFile", true));
        inputs.insert(port("token"), exact("BuildkiteToken", false));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("slug"), scalar("BuildkitePipelineSlug"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "A gate: whether a Buildkite pipeline's stored configuration \
                              equals the bootstrap a document renders."
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

    fn outputs_for(slug: &BuildkitePipelineSlug) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("slug"), Value::known(slug.clone()));
        outputs
    }

    /// Mirrors the live tool's own `observe`: no seeded pipeline, or one
    /// whose stored `configuration` is not structurally equal to
    /// `expected`'s content, both read `Absent`.
    fn observe_state(
        state: &FakeState,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        expected: &RepoFile,
    ) -> Observation {
        let equal = state
            .buildkite_pipelines
            .get(&buildkite_pipeline_key(org, slug))
            .is_some_and(|record| structurally_equal(&record.configuration, expected.content()));
        if equal {
            Observation::Present(Self::outputs_for(slug))
        } else {
            Observation::Absent {
                predicted: Self::outputs_for(slug),
            }
        }
    }

    fn observe(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let org: BuildkiteOrg = get(inputs, "org")?;
        let slug: BuildkitePipelineSlug = get(inputs, "slug")?;
        let expected: RepoFile = get(inputs, "expected")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &buildkite_pipeline_key(&org, &slug));
        Ok(Self::observe_state(&state, &org, &slug, &expected))
    }
}

impl Tool for FakeBuildkitePipelineBootstrapGate {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.observe(inputs)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        match self.observe(inputs)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { predicted } => Ok(Ensured {
                outputs: predicted,
                changed: false,
            }),
            other => unreachable!("this gate's own observe never returns {other:?}"),
        }
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&GATE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::{DomainType, RepoPath};

    use crate::state::BuildkitePipelineRecord;

    fn org() -> BuildkiteOrg {
        BuildkiteOrg::parse("willikins-test").unwrap()
    }

    fn slug() -> BuildkitePipelineSlug {
        BuildkitePipelineSlug::parse("third-thoughts").unwrap()
    }

    fn expected() -> RepoFile {
        RepoFile::new(
            RepoPath::parse("apps/sample/.buildkite/bootstrap.yml").unwrap(),
            "steps:\n  - command: \"echo hi\"\n",
        )
        .unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(port("org"), Value::known(org()));
        inputs.insert(port("slug"), Value::known(slug()));
        inputs.insert(port("expected"), Value::known(expected()));
        inputs
    }

    fn tool(state: Arc<Mutex<FakeState>>) -> FakeBuildkitePipelineBootstrapGate {
        FakeBuildkitePipelineBootstrapGate::new(state)
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        t.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_has_no_key_and_is_pure_and_reversible() {
        let spec = tool(Arc::new(Mutex::new(FakeState::new()))).spec;
        assert!(spec.key.is_empty());
        assert!(spec.pure);
        assert_eq!(spec.class, Class::Reversible);
    }

    #[test]
    fn declares_a_gate_over_org_and_slug() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        let gate = t.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["org", "slug"]);
    }

    #[test]
    fn read_reports_absent_when_no_pipeline_is_seeded() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_absent_when_the_seeded_configuration_differs() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            BuildkitePipelineRecord {
                repository: "git@github.com:lightless-labs/other.git".to_string(),
                cluster_id: "cluster".to_string(),
                ours: true,
                configuration:
                    "steps:\n  - command: \"buildkite-agent pipeline upload\"\n".to_string(),
            },
        )));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_present_when_the_seeded_configuration_matches_structurally() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            BuildkitePipelineRecord {
                repository: "git@github.com:lightless-labs/other.git".to_string(),
                cluster_id: "cluster".to_string(),
                ours: true,
                // Re-quoted, still structurally equal to `expected()`'s
                // content.
                configuration: "steps:\n  - command: 'echo hi'\n".to_string(),
            },
        )));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    fn present_passes_slug_through_as_its_own_output() {
        let state = Arc::new(Mutex::new(FakeState::new().with_buildkite_pipeline(
            &org(),
            &slug(),
            BuildkitePipelineRecord {
                repository: "git@github.com:lightless-labs/other.git".to_string(),
                cluster_id: "cluster".to_string(),
                ours: true,
                configuration: "steps:\n  - command: \"echo hi\"\n".to_string(),
            },
        )));
        let t = tool(state);
        let Observation::Present(outputs) = t.read(&inputs()).unwrap() else {
            panic!("expected Present");
        };
        let out = outputs.get(&PortName::parse("slug").unwrap()).unwrap();
        assert_eq!(out.render().to_string(), "third-thoughts");
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_never_reports_changed() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        let token = SinkToken::new();
        let ensured = t.ensure(&inputs(), &token).unwrap();
        assert!(!ensured.changed);
    }
}
