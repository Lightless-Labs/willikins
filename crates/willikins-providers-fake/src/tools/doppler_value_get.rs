//! `doppler.value.get`: reads a seeded Doppler variable's value as
//! non-secret [`Text`]. Pure and read-only. The fake twin of
//! `willikins_providers_doppler::tools::DopplerValueGet` — see that
//! tool's module doc for the footgun this tool shares by construction (it
//! is a fake, so there is no live Doppler visibility to worry about, but
//! the same "choosing this tool over `doppler.secret.get` is the
//! declaration of non-secrecy" applies to whoever seeds a fixture).

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, SecretName};

use crate::state::{FakeState, doppler_secret_key};
use crate::support::{exact, get, not_found, port, require_present, scalar, tool_name};

/// `doppler.value.get`.
pub struct DopplerValueGet {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerValueGet {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("SecretName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("Text"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.value.get"),
                description: "Read a Doppler variable's value as non-secret text. Choosing \
                               this tool over `doppler.secret.get` is the document's own \
                               declaration that the value is not a secret."
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

    /// Look the variable up and build its output, or a
    /// [`ToolError::NotFound`](willikins_core::ToolErrorKind::NotFound)
    /// naming the key it looked for.
    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let name: SecretName = get(inputs, "name")?;
        let key = doppler_secret_key(&config, &name);
        let state = self.state.lock().unwrap();
        let value = state
            .doppler_values
            .get(&key)
            .ok_or_else(|| not_found(format!("no secret at `{key}`")))?
            .clone();
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::known(value));
        Ok(outputs)
    }
}

impl Tool for DopplerValueGet {
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
    use willikins_types::{DomainType, Text};

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn name() -> SecretName {
        SecretName::parse("ASC_API_KEY_ISSUER_ID").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs
    }

    fn seeded_tool() -> DopplerValueGet {
        let state = FakeState::new().with_doppler_value(
            &config(),
            &name(),
            Text::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap(),
        );
        DopplerValueGet::new(Arc::new(Mutex::new(state)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        DopplerValueGet::new(Arc::new(Mutex::new(FakeState::new())))
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn output_port_is_a_non_secret_text() {
        assert_eq!(
            willikins_types::registry().is_secret(&TypeName::parse("Text").unwrap()),
            Some(false)
        );
    }

    #[test]
    fn read_returns_the_seeded_value_as_known_and_plain() {
        let observation = seeded_tool().read(&full_inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        assert!(value.is_known());
        assert_eq!(
            value.render().to_string(),
            "57246542-96fe-1a63-e053-0824d011072a"
        );
    }

    #[test]
    fn read_of_an_unseeded_variable_is_not_found() {
        let tool = DopplerValueGet::new(Arc::new(Mutex::new(FakeState::new())));
        let err = tool.read(&full_inputs()).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::NotFound);
        assert!(err.message.contains("third-thoughts/prd"));
        assert!(err.message.contains("ASC_API_KEY_ISSUER_ID"));
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
}
