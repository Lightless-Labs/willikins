//! `fake.secret_list`: a pure test tool returning a known list of secret
//! values, used to exercise `SecretForEachSource` in `willikins-core`'s
//! checker.

use indexmap::IndexMap;

use willikins_core::{
    Class, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DomainType, DopplerConfig, DopplerServiceToken};

use crate::support::{exact, get, list, port, require_present, tool_name};

/// The first constant token `fake.secret_list` always reports.
const TOKEN_ONE: &str = "dp.st.fakesecretlistoneaaaaaaaaaaaaaaaaaaaaaaaaa";
/// The second constant token `fake.secret_list` always reports.
const TOKEN_TWO: &str = "dp.st.fakesecretlisttwoaaaaaaaaaaaaaaaaaaaaaaaaa";

/// `fake.secret_list`.
pub struct FakeSecretList {
    spec: ToolSpec,
}

impl FakeSecretList {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("tokens"), list("DopplerServiceToken"));
        Self {
            spec: ToolSpec {
                name: tool_name("fake.secret_list"),
                description: "Test tool: always reports two constant, known service tokens."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
        }
    }

    fn compute(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let _config: DopplerConfig = get(inputs, "config")?;
        let tokens = vec![
            DopplerServiceToken::parse(TOKEN_ONE).unwrap_or_else(|err| {
                unreachable!("TOKEN_ONE is a valid DopplerServiceToken: {err}")
            }),
            DopplerServiceToken::parse(TOKEN_TWO).unwrap_or_else(|err| {
                unreachable!("TOKEN_TWO is a valid DopplerServiceToken: {err}")
            }),
        ];
        let mut outputs = Outputs::new();
        outputs.insert(port("tokens"), Value::known_list(tokens));
        Ok(outputs)
    }
}

impl Default for FakeSecretList {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for FakeSecretList {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.compute(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
        self.compute(inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef};

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        FakeSecretList::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn read_reports_two_known_secret_tokens() {
        let observation = FakeSecretList::new().read(&full_inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let tokens = outputs.get(&PortName::parse("tokens").unwrap()).unwrap();
        assert!(tokens.is_known());
        assert!(tokens.is_secret());
        assert_eq!(tokens.as_list().unwrap().len(), 2);
    }

    #[test]
    fn read_rejects_an_unknown_input() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("config").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("DopplerConfig").unwrap())),
        );
        let err = FakeSecretList::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("config"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = FakeSecretList::new().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("config"), "{}", err.message);
    }
}
