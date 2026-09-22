//! `apple.key_id.parse`: turns a [`Text`] into an [`AppleKeyId`]. Pure,
//! non-secret -- [`crate::apple_issuer_id_parse`]'s own module doc
//! explains why this parse node exists at all (nominal typing: a `Text`
//! output cannot bind directly to `appstore.bundle_id.ensure`'s
//! `key_id` port, which is `exact("AppleKeyId", true)`).

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    exact, get, invalid, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{AppleKeyId, DomainType, Text};

/// `apple.key_id.parse`.
pub struct AppleKeyIdParse {
    spec: ToolSpec,
}

impl AppleKeyIdParse {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("value"), exact("Text", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("AppleKeyId"));
        Self {
            spec: ToolSpec {
                name: tool_name("apple.key_id.parse"),
                description: "Parse text as an App Store Connect API key's key id.".to_string(),
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
        let text: Text = get(inputs, "value")?;
        let key_id = AppleKeyId::parse(text.as_str())
            .map_err(|err| invalid(format!("value: {}", err.reason)))?;
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::known(key_id));
        Ok(outputs)
    }
}

impl Default for AppleKeyIdParse {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for AppleKeyIdParse {
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
            Value::known(Text::parse(value).unwrap()),
        );
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        AppleKeyIdParse::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn parses_a_valid_key_id() {
        let inputs = inputs_with("2X9R4HXF34");
        let Observation::Present(outputs) = AppleKeyIdParse::new().read(&inputs).unwrap() else {
            panic!("expected Present");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        assert_eq!(value.render().to_string(), "2X9R4HXF34");
    }

    #[test]
    fn refuses_a_lowercase_value() {
        let inputs = inputs_with("2x9r4hxf34");
        let err = AppleKeyIdParse::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("value"), "{}", err.message);
    }

    #[test]
    fn ensure_agrees_with_read() {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let tool = AppleKeyIdParse::new();
        let inputs = inputs_with("2X9R4HXF34");
        let via_ensure = tool.ensure(&inputs, &token).unwrap();
        assert!(!via_ensure.changed);
    }
}
