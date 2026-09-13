//! `doppler.config.ensure`: creates (in memory) a Doppler config, named
//! after its environment by `naming::v1::doppler_root_config`.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerProject, EnvironmentSlug, naming};

use crate::state::{FakeState, doppler_config_key};
use crate::support::{exact, get, port, require_present, scalar, tool_name};

/// `doppler.config.ensure`.
pub struct DopplerConfigEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerConfigEnsure {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        inputs.insert(port("environment"), exact("EnvironmentSlug", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.config.ensure"),
                description: "Ensure an environment's root Doppler config exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("environment")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerProject, EnvironmentSlug), ToolError> {
        require_present(&self.spec, inputs)?;
        let project = get(inputs, "project")?;
        let environment = get(inputs, "environment")?;
        Ok((project, environment))
    }

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }
}

impl Tool for DopplerConfigEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, environment) = self.key_ports(inputs)?;
        let config = naming::v1::doppler_root_config(&project, &environment);
        let state = self.state.lock().unwrap();
        if state.doppler_configs.contains(&doppler_config_key(&config)) {
            Ok(Observation::Present(Self::outputs_for(&config)))
        } else {
            Ok(Observation::Absent {
                predicted: Self::outputs_for(&config),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (project, environment) = self.key_ports(inputs)?;
        let config = naming::v1::doppler_root_config(&project, &environment);
        let mut state = self.state.lock().unwrap();
        // Read its own state first, so `changed` is truthful: a config
        // `doppler.project.ensure` already seeded (see its own module
        // doc) is not created a second time here.
        let changed = state.doppler_configs.insert(doppler_config_key(&config));
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
    use willikins_types::DomainType;

    fn project() -> DopplerProject {
        DopplerProject::parse("third-thoughts").unwrap()
    }

    fn environment() -> EnvironmentSlug {
        EnvironmentSlug::parse("prd").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::known(environment()),
        );
        inputs
    }

    fn tool() -> DopplerConfigEnsure {
        DopplerConfigEnsure::new(Arc::new(Mutex::new(FakeState::new())))
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
        assert_eq!(config.render().to_string(), "third-thoughts/prd");
    }

    #[test]
    fn read_reports_present_when_seeded() {
        let config = naming::v1::doppler_root_config(&project(), &environment());
        let state = Arc::new(Mutex::new(FakeState::new().with_doppler_config(&config)));
        let observation = DopplerConfigEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("EnvironmentSlug").unwrap())),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("environment"), "{}", err.message);
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

    /// Acceptance test 5's own claim: `doppler.project.ensure` seeds the
    /// `dev`/`stg`/`prd` root configs when it creates the project, so a
    /// following `doppler.config.ensure` for one of those finds it present
    /// (`changed: false`), while an environment outside that set (`qa`)
    /// is still created fresh (`changed: true`).
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn a_default_environment_is_unchanged_after_project_ensure_but_qa_is_created() {
        let state = Arc::new(Mutex::new(FakeState::new()));
        let project_tool = crate::tools::DopplerProjectEnsure::new(Arc::clone(&state));
        let token = SinkToken::new();
        let mut project_inputs = Inputs::new();
        project_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        project_tool.ensure(&project_inputs, &token).unwrap();

        let config_tool = DopplerConfigEnsure::new(Arc::clone(&state));
        let prd = config_tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            !prd.changed,
            "prd was already seeded by doppler.project.ensure"
        );

        let mut qa_inputs = Inputs::new();
        qa_inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
        qa_inputs.insert(
            PortName::parse("environment").unwrap(),
            Value::known(EnvironmentSlug::parse("qa").unwrap()),
        );
        let qa = config_tool.ensure(&qa_inputs, &token).unwrap();
        assert!(qa.changed, "qa is not one of the seeded defaults");
    }
}
