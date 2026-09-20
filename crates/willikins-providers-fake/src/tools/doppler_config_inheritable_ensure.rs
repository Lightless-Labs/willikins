//! `doppler.config.inheritable.ensure`: marks (in memory) a Doppler
//! config inheritable.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_types::DopplerConfig;

use crate::state::{FakeState, doppler_config_key};
use crate::support::{exact, get, port, require_present, tool_name};

/// `doppler.config.inheritable.ensure`.
pub struct DopplerConfigInheritableEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerConfigInheritableEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.config.inheritable.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a Doppler config is inheritable.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: vec![port("config")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<DopplerConfig, ToolError> {
        require_present(&self.spec, inputs)?;
        get(inputs, "config")
    }
}

impl Tool for DopplerConfigInheritableEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let config = self.key_ports(inputs)?;
        let key = doppler_config_key(&config);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        if state.doppler_config_inheritable.contains(&key) {
            Ok(Observation::Present(Outputs::new()))
        } else {
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let config = self.key_ports(inputs)?;
        let key = doppler_config_key(&config);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        let changed = state.doppler_config_inheritable.insert(key);
        Ok(Ensured {
            outputs: Outputs::new(),
            changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, Value};
    use willikins_types::DomainType;

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs
    }

    fn tool() -> DopplerConfigInheritableEnsure {
        DopplerConfigInheritableEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_empty() {
        let observation = tool().read(&full_inputs()).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    fn read_reports_present_when_seeded() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_config_inheritable(&config()),
        ));
        let observation = DopplerConfigInheritableEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = tool().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("config"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_on_first_call_and_false_on_a_second() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed);
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(!second.changed);
    }
}
