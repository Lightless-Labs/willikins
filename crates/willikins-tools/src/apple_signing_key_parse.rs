//! `apple.signing_key.parse`: turns a decoded opaque secret into an
//! [`AppleSigningKey`]. Pure, and the second tool in this crate whose
//! input port is itself secret (`base64.decode` is the first) — see
//! `willikins_types::secret`'s module doc for the token-less accessor
//! this and `base64.decode` share, and `willikins_types::appstore`'s
//! module doc for exactly what this tool's real work is: deciding "is
//! this actually a P-256 key" lives in [`AppleSigningKey::parse`] itself,
//! not here, so this tool is a thin wrapper that reads its input, calls
//! that parse, and reports whatever it says. A wrong or corrupt key
//! fails *here*, at plan time, with a clear reason — never at Apple's
//! own door.
//!
//! Deliberately takes `OpaqueSecret` rather than `AnySecret`
//! (`willikins_core::tool::helpers::any_secret`): the token-less
//! `reveal_for_transform` this tool needs exists only on the concrete
//! `OpaqueSecret` type (see that type's module doc for why it is not
//! offered generically), so this tool's input is exactly what
//! `base64.decode` produces or what a resolver that needs no decoding
//! step emits directly.

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    exact, get, invalid, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{AppleSigningKey, DomainType, OpaqueSecret};

/// `apple.signing_key.parse`.
pub struct AppleSigningKeyParse {
    spec: ToolSpec,
}

impl AppleSigningKeyParse {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("value"), exact("OpaqueSecret", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("AppleSigningKey"));
        Self {
            spec: ToolSpec {
                name: tool_name("apple.signing_key.parse"),
                description: "Parse a decoded opaque secret as an EC P-256 (PKCS#8 or SEC1 \
                               PEM) App Store Connect signing key."
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
        let value: OpaqueSecret = get(inputs, "value")?;
        let key = value
            .reveal_for_transform(AppleSigningKey::parse)
            .map_err(|err| invalid(format!("value: {}", err.reason)))?;
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::known(key));
        Ok(outputs)
    }
}

impl Default for AppleSigningKeyParse {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for AppleSigningKeyParse {
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
            Value::known(OpaqueSecret::parse(value).unwrap()),
        );
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        AppleSigningKeyParse::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn parses_a_pkcs8_pem_key() {
        let inputs = inputs_with(AppleSigningKey::example());
        let Observation::Present(outputs) = AppleSigningKeyParse::new().read(&inputs).unwrap()
        else {
            panic!("expected Present");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        assert_eq!(value.render().to_string(), "[REDACTED AppleSigningKey]");
    }

    #[test]
    fn refuses_a_non_key_value_with_a_clear_error_and_no_content() {
        let marker = "wlkn-test-marker-not-a-key";
        let inputs = inputs_with(marker);
        let err = AppleSigningKeyParse::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("P-256"), "{}", err.message);
        assert!(!err.message.contains(marker), "{}", err.message);
    }

    #[test]
    fn ensure_agrees_with_read() {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let tool = AppleSigningKeyParse::new();
        let inputs = inputs_with(AppleSigningKey::example());
        let via_ensure = tool.ensure(&inputs, &token).unwrap();
        assert!(!via_ensure.changed);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = AppleSigningKeyParse::new()
            .read(&Inputs::new())
            .unwrap_err();
        assert!(err.message.contains("value"), "{}", err.message);
    }
}
