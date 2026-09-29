//! `doppler.config.inheritable.gate`: mirrors
//! `willikins_providers_doppler::tools::DopplerConfigInheritableGate` --
//! see that crate's own module doc for the full reasoning (task R3).
//! Not a leaf: it passes `config` through as its own output, exactly
//! like the live tool, so a document that wants to order a base config's
//! consumer after this gate can bind from it.

use std::sync::{Arc, Mutex};

use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::DopplerConfig;

use crate::state::{FakeState, doppler_config_key};
use crate::support::{exact, get, port, require_present, scalar, tool_name};

static GATE: Gate = Gate {
    need: "this Doppler config exists and is marked inheritable",
    how: "create it (`doppler.config.ensure` or `doppler.branch_config.ensure`) and mark it \
          inheritable (`doppler.config.inheritable.ensure`), or do both in the Doppler \
          dashboard; a `400` naming \"does not have access\" can also mean this token cannot \
          see the project it lives in",
    subject: &["config"],
};

/// `doppler.config.inheritable.gate`.
pub struct DopplerConfigInheritableGate {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerConfigInheritableGate {
    const TOOL_NAME: &'static str = "doppler.config.inheritable.gate";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "A gate: whether a Doppler config exists and is inheritable."
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

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }

    /// Mirrors the live tool's own `observe`: a config that does not
    /// exist yet and one that exists but is not marked inheritable both
    /// read `Absent` (the module doc's own reasoning).
    fn observe_state(state: &FakeState, config: &DopplerConfig) -> Observation {
        let key = doppler_config_key(config);
        let exists = state.doppler_configs.contains(&key);
        let inheritable = state.doppler_config_inheritable.contains(&key);
        if exists && inheritable {
            Observation::Present(Self::outputs_for(config))
        } else {
            Observation::Absent {
                predicted: Self::outputs_for(config),
            }
        }
    }

    fn observe(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &doppler_config_key(&config));
        Ok(Self::observe_state(&state, &config))
    }
}

impl Tool for DopplerConfigInheritableGate {
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
    use willikins_types::DomainType;

    fn config() -> DopplerConfig {
        DopplerConfig::parse("appstore-connect/deploy_ios").unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs
    }

    fn tool(state: Arc<Mutex<FakeState>>) -> DopplerConfigInheritableGate {
        DopplerConfigInheritableGate::new(state)
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        t.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_the_config_does_not_exist() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_absent_when_the_config_exists_but_is_not_inheritable() {
        let state = Arc::new(Mutex::new(FakeState::new().with_doppler_config(&config())));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_present_when_the_config_exists_and_is_inheritable() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_doppler_config(&config())
                .with_doppler_config_inheritable(&config()),
        ));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    fn present_passes_the_config_through_as_its_own_output() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_doppler_config(&config())
                .with_doppler_config_inheritable(&config()),
        ));
        let t = tool(state);
        let Observation::Present(outputs) = t.read(&inputs()).unwrap() else {
            panic!("expected Present");
        };
        let out = outputs.get(&PortName::parse("config").unwrap()).unwrap();
        assert_eq!(out.render().to_string(), "appstore-connect/deploy_ios");
    }

    #[test]
    fn is_a_gate_over_config_only() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        let gate = t.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["config"]);
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
