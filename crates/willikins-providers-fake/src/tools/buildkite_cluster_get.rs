//! `buildkite.cluster.get`: resolves a human-written Buildkite cluster
//! name to its id, against seeded state. Mirrors
//! `willikins_providers_buildkite::tools::BuildkiteClusterGet`'s
//! `ToolSpec` exactly (`tests/catalog_parity.rs`, in
//! `willikins-providers-buildkite`, pins the two equal) and the same
//! three outcomes: zero matches, one match, or two or more.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{BuildkiteClusterId, BuildkiteClusterName, BuildkiteOrg, DomainType};

use crate::state::FakeState;
use crate::support::{conflict, exact, get, not_found, port, require_present, scalar, tool_name};

/// `buildkite.cluster.get`.
pub struct FakeBuildkiteClusterGet {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeBuildkiteClusterGet {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("org"), exact("BuildkiteOrg", true));
        inputs.insert(port("name"), exact("BuildkiteClusterName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("cluster"), scalar("BuildkiteClusterId"));
        Self {
            spec: ToolSpec {
                name: tool_name("buildkite.cluster.get"),
                description: "Resolve a Buildkite cluster's human-written name to its id."
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
        // `org` is part of this tool's real-provider port shape (a
        // credential reaches one organisation), but this fake's state is
        // not itself partitioned by organisation -- exactly like every
        // other fake tool's single-workplace state.
        let _org: BuildkiteOrg = get(inputs, "org")?;
        let name: BuildkiteClusterName = get(inputs, "name")?;
        let state = self.state.lock().unwrap();
        let ids = state
            .buildkite_clusters
            .get(name.as_str())
            .cloned()
            .unwrap_or_default();
        match ids.len() {
            0 => Err(not_found(format!("no Buildkite cluster named `{name}`"))),
            1 => {
                let cluster = BuildkiteClusterId::parse(&ids[0]).map_err(|err| ToolError {
                    kind: willikins_core::ToolErrorKind::Provider,
                    message: format!("cluster `{name}` has a malformed id: {err}"),
                })?;
                let mut outputs = Outputs::new();
                outputs.insert(port("cluster"), Value::known(cluster));
                Ok(outputs)
            }
            count => Err(conflict(format!(
                "{count} Buildkite clusters are named `{name}`; this tool cannot disambiguate \
                 by name alone"
            ))),
        }
    }
}

impl Tool for FakeBuildkiteClusterGet {
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

    fn org() -> BuildkiteOrg {
        BuildkiteOrg::parse("willikins-test").unwrap()
    }

    fn name() -> BuildkiteClusterName {
        BuildkiteClusterName::parse("Default cluster").unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("org").unwrap(), Value::known(org()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let tool = FakeBuildkiteClusterGet::new(Arc::new(Mutex::new(FakeState::new())));
        tool.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_not_found_when_no_cluster_of_that_name() {
        let tool = FakeBuildkiteClusterGet::new(Arc::new(Mutex::new(FakeState::new())));
        let err = tool.read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::NotFound);
        assert!(err.message.contains("Default cluster"));
    }

    #[test]
    fn read_reports_present_with_the_seeded_id() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_buildkite_cluster(&name(), "018e5a22-d14c-7085-bb28-db0f83f43a1c"),
        ));
        let tool = FakeBuildkiteClusterGet::new(state);
        let observation = tool.read(&inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let cluster = outputs.get(&PortName::parse("cluster").unwrap()).unwrap();
        assert_eq!(
            cluster.render().to_string(),
            "018e5a22-d14c-7085-bb28-db0f83f43a1c"
        );
    }

    #[test]
    fn read_reports_conflict_when_two_clusters_share_a_name() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_buildkite_cluster(&name(), "018e5a22-d14c-7085-bb28-db0f83f43a1c")
                .with_buildkite_cluster(&name(), "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        ));
        let tool = FakeBuildkiteClusterGet::new(state);
        let err = tool.read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(err.message.contains('2'));
        assert!(err.message.contains("Default cluster"));
    }
}
