//! `apple.issuer_id.parse`: turns a [`DopplerValue`] into an
//! [`AppleIssuerId`]. Pure, non-secret -- the parse node a
//! `doppler.value.get` output needs before it can bind to
//! `appstore.bundle_id.ensure`'s `issuer_id` port, which is
//! `exact("AppleIssuerId", true)`: nominal typing means a `DopplerValue`
//! can never bind there directly, even though both are non-secret
//! strings at the byte level (`willikins_types::appstore`'s own module
//! doc: "a type name at a port carries provider meaning").
//!
//! **Milestone 3i, task B8.** The input was [`willikins_types::Text`]
//! until this task; retyped to [`DopplerValue`] alongside
//! `doppler.value.get`'s own output so the issuer id this tool reads
//! stays identifier-typed (and masked on every output surface) across
//! the whole chain from Doppler to the parsed `AppleIssuerId`, instead of
//! printing in full for the one node in between.

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    exact, get, invalid, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{AppleIssuerId, DomainType, DopplerValue};

/// `apple.issuer_id.parse`.
pub struct AppleIssuerIdParse {
    spec: ToolSpec,
}

impl AppleIssuerIdParse {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("value"), exact("DopplerValue", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("AppleIssuerId"));
        Self {
            spec: ToolSpec {
                name: tool_name("apple.issuer_id.parse"),
                description: "Parse text as an App Store Connect API key's issuer id.".to_string(),
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
        let text: DopplerValue = get(inputs, "value")?;
        let issuer_id = AppleIssuerId::parse(text.as_str())
            .map_err(|err| invalid(format!("value: {}", err.reason)))?;
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::known(issuer_id));
        Ok(outputs)
    }
}

impl Default for AppleIssuerIdParse {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for AppleIssuerIdParse {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.compute(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.compute(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;

    fn inputs_with(value: &str) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("value").unwrap(),
            Value::known(DopplerValue::parse(value).unwrap()),
        );
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        AppleIssuerIdParse::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn parses_a_valid_issuer_id() {
        let inputs = inputs_with("57246542-96fe-1a63-e053-0824d011072a");
        let Observation::Present(outputs) = AppleIssuerIdParse::new().read(&inputs).unwrap() else {
            panic!("expected Present");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        assert_eq!(
            value.render().to_string(),
            "57246542-96fe-1a63-e053-0824d011072a"
        );
    }

    #[test]
    fn refuses_a_non_uuid_value() {
        let inputs = inputs_with("not-a-uuid");
        let err = AppleIssuerIdParse::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("value"), "{}", err.message);
    }

    #[test]
    fn ensure_agrees_with_read() {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let tool = AppleIssuerIdParse::new();
        let inputs = inputs_with("57246542-96fe-1a63-e053-0824d011072a");
        let via_ensure = tool.ensure(&inputs, &token).unwrap();
        assert!(!via_ensure.changed);
    }
}
