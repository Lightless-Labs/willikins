//! `doppler.secret.get`: reads a seeded secret's value. Pure and
//! read-only, and the redaction proof path: its output is a known secret
//! value, so its `Outputs` must still render redacted everywhere.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, SecretName};

use crate::state::{FakeState, doppler_secret_key};
use crate::support::{exact, get, not_found, port, require_present, scalar, tool_name};

/// `doppler.secret.get`.
pub struct DopplerSecretGet {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerSecretGet {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("SecretName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("DopplerSecretValue"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.secret.get"),
                description: "Read a Doppler secret's value.".to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
            state,
        }
    }

    /// Look the secret up and build its output, or a [`ToolError::NotFound`](willikins_core::ToolErrorKind::NotFound)
    /// naming the key it looked for — never the value, since there is
    /// none to name in that case.
    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let name: SecretName = get(inputs, "name")?;
        let key = doppler_secret_key(&config, &name);
        let state = self.state.lock().unwrap();
        let value = state
            .doppler_secrets
            .get(&key)
            .ok_or_else(|| not_found(format!("no secret at `{key}`")))?
            .clone();
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::known(value));
        Ok(outputs)
    }
}

impl Tool for DopplerSecretGet {
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
    use willikins_core::{PortName, ToolErrorKind, TypeName, TypeRef};
    use willikins_types::{DomainType, DopplerSecretValue};

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn name() -> SecretName {
        SecretName::parse("DATABASE_URL").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs
    }

    fn seeded_tool() -> DopplerSecretGet {
        let state = FakeState::new().with_doppler_secret(
            &config(),
            &name(),
            DopplerSecretValue::parse("s3cr3t-value").unwrap(),
        );
        DopplerSecretGet::new(Arc::new(Mutex::new(state)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        DopplerSecretGet::new(Arc::new(Mutex::new(FakeState::new())))
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn read_returns_the_seeded_value_as_known() {
        let observation = seeded_tool().read(&full_inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        assert!(value.is_known());
        assert_eq!(value.render().to_string(), "[REDACTED DopplerSecretValue]");
    }

    #[test]
    fn read_of_an_unseeded_secret_is_not_found_and_never_names_a_value() {
        let tool = DopplerSecretGet::new(Arc::new(Mutex::new(FakeState::new())));
        let err = tool.read(&full_inputs()).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::NotFound);
        assert!(err.message.contains("third-thoughts/prd"));
        assert!(err.message.contains("DATABASE_URL"));
        assert!(!err.message.contains("s3cr3t"));
    }

    #[test]
    fn read_rejects_an_unknown_input() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("config").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("DopplerConfig").unwrap())),
        );
        let err = seeded_tool().read(&inputs).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Invalid);
        assert!(err.message.contains("config"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = seeded_tool().read(&Inputs::new()).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Invalid);
    }

    #[test]
    fn known_secret_renders_redacted_in_outputs_debug_and_json() {
        let observation = seeded_tool().read(&full_inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let debug = format!("{outputs:?}");
        assert!(debug.contains("REDACTED"), "{debug}");
        assert!(!debug.contains("s3cr3t"), "{debug}");
        let json = serde_json::to_string(&outputs).unwrap();
        assert!(json.contains("REDACTED"), "{json}");
        assert!(!json.contains("s3cr3t"), "{json}");
    }
}
