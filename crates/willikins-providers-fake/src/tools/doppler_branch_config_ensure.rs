//! `doppler.branch_config.ensure`: creates (in memory) a Doppler branch
//! config at `<environment's snake join>_<branch>`, mirroring the live
//! tool's own name assembly (`willikins_providers_doppler::tools::DopplerBranchConfigEnsure`'s
//! module doc, "The name: a suffix port, not a full-name port"). Like
//! this crate's own `doppler.config.ensure` twin, membership in
//! [`crate::state::FakeState::doppler_configs`] is the only fact
//! recorded: this fake has no concept of `root` or of one config's
//! `environment`, so it cannot model the live tool's `Foreign` collision
//! case (a branch config's literal name squatted on by something else) —
//! the same limit that crate's `fake_agrees_with_live.rs` already
//! records for `doppler.config.inheritable.ensure`'s own `Foreign`-adjacent
//! case.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, EnvironmentSlug,
};

use crate::state::{FakeState, doppler_config_key};
use crate::support::{exact, get, invalid, port, require_present, scalar, tool_name};

/// `doppler.branch_config.ensure`.
pub struct DopplerBranchConfigEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerBranchConfigEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.branch_config.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        inputs.insert(port("environment"), exact("EnvironmentSlug", true));
        inputs.insert(port("branch"), exact("DopplerConfigName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description:
                    "Ensure a named branch config exists under an environment's root config."
                        .to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("environment"), port("branch")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn key_ports(
        &self,
        inputs: &Inputs,
    ) -> Result<(DopplerProject, EnvironmentSlug, DopplerConfigName), ToolError> {
        require_present(&self.spec, inputs)?;
        let project = get(inputs, "project")?;
        let environment = get(inputs, "environment")?;
        let branch = get(inputs, "branch")?;
        Ok((project, environment, branch))
    }

    /// See the live tool's own `full_name`: the same snake-join assembly,
    /// so the two agree on which config a given `(environment, branch)`
    /// pair names.
    fn full_name(
        environment: &EnvironmentSlug,
        branch: &DopplerConfigName,
    ) -> Result<DopplerConfigName, ToolError> {
        let candidate = format!("{}_{branch}", environment.words().snake());
        DopplerConfigName::parse(&candidate).map_err(|err| {
            invalid(format!(
                "`{candidate}` (environment `{environment}` plus branch `{branch}`) is not a \
                 valid Doppler config name: {err}"
            ))
        })
    }

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }
}

impl Tool for DopplerBranchConfigEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, environment, branch) = self.key_ports(inputs)?;
        let name = Self::full_name(&environment, &branch)?;
        let config = DopplerConfig::new(project, name);
        let key = doppler_config_key(&config);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        if state.doppler_configs.contains(&key) {
            Ok(Observation::Present(Self::outputs_for(&config)))
        } else {
            Ok(Observation::Absent {
                predicted: Self::outputs_for(&config),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (project, environment, branch) = self.key_ports(inputs)?;
        let name = Self::full_name(&environment, &branch)?;
        let config = DopplerConfig::new(project, name);
        let key = doppler_config_key(&config);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        let changed = state.doppler_configs.insert(key);
        Ok(Ensured {
            outputs: Self::outputs_for(&config),
            changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef};

    fn project() -> DopplerProject {
        DopplerProject::parse("sample").unwrap()
    }

    fn environment() -> EnvironmentSlug {
        EnvironmentSlug::parse("prd").unwrap()
    }

    fn branch() -> DopplerConfigName {
        DopplerConfigName::parse("deployment_ios").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::known(environment()),
        );
        inputs.insert(PortName::parse("branch").unwrap(), Value::known(branch()));
        inputs
    }

    fn tool() -> DopplerBranchConfigEnsure {
        DopplerBranchConfigEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_with_the_predicted_config_when_empty() {
        let observation = tool().read(&full_inputs()).unwrap();
        let Observation::Absent { predicted } = observation else {
            panic!("expected Absent, got {observation:?}");
        };
        let config = predicted.get(&PortName::parse("config").unwrap()).unwrap();
        assert_eq!(config.render().to_string(), "sample/prd_deployment_ios");
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("branch").unwrap(),
            Value::unknown(TypeRef::scalar(
                TypeName::parse("DopplerConfigName").unwrap(),
            )),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("branch"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = tool().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("project"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_then_read_gives_present() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&full_inputs(), &token).unwrap();
        let observation = tool.read(&full_inputs()).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_on_creation_and_false_on_a_second_call() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed, "creating the config must report changed");
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            !second.changed,
            "ensure on an already-present config must report changed: false"
        );
    }

    /// A branch config and its environment's root config never collide:
    /// `doppler.config.ensure` seeds `dev`/`stg`/`prd` for a fresh
    /// project, and this tool's own assembled name always carries an
    /// extra `_<branch>` suffix, so the two tools' keys never coincide.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn a_branch_config_is_independent_of_its_environment_s_root_config() {
        let state = Arc::new(Mutex::new(FakeState::new()));
        let project_tool = crate::tools::DopplerProjectEnsure::new(Arc::clone(&state));
        let token = SinkToken::new();
        let mut project_inputs = Inputs::new();
        project_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        project_tool.ensure(&project_inputs, &token).unwrap();

        let branch_tool = DopplerBranchConfigEnsure::new(Arc::clone(&state));
        let branch = branch_tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            branch.changed,
            "the branch config is not one of the seeded root configs"
        );

        let root_tool = crate::tools::DopplerConfigEnsure::new(state);
        let mut root_inputs = Inputs::new();
        root_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        root_inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::known(environment()),
        );
        let root = root_tool.ensure(&root_inputs, &token).unwrap();
        assert!(
            !root.changed,
            "prd's root config was already seeded by doppler.project.ensure, untouched by the \
             branch config's own creation"
        );
    }
}
