//! `apple.signing_key.parse`: turns a decoded secret into an
//! [`AppleSigningKey`]. Pure, and the second tool in this crate whose
//! input port is itself secret (`base64.decode` is the first) — see
//! `willikins_types::secret`'s module doc for the token-less dispatch
//! this and `base64.decode` share, and `willikins_types::appstore`'s
//! module doc for exactly what this tool's real work is: deciding "is
//! this actually a P-256 key" lives in [`AppleSigningKey::parse`] itself,
//! not here, so this tool is a thin wrapper that reads its input, calls
//! that parse, and reports whatever it says. A wrong or corrupt key
//! fails *here*, at plan time, with a clear reason — never at Apple's
//! own door.
//!
//! **Input port: `AnySecret`, not `exact("OpaqueSecret", ...)`.** The
//! operator's own key habit wraps a base64 layer around the `.p8`
//! (`base64.decode`'s output is `OpaqueSecret`), but not every document
//! needs that layer — a resolver whose secret is already the raw PEM can
//! bind straight here. Either way this tool reads its input through
//! `willikins_types::secret::reveal_transform_input`, the same dispatch
//! `base64.decode` uses; see `willikins_types::secret`'s module doc,
//! "The Doppler bridge — 'wall one'".

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    any_secret, invalid, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{AppleSigningKey, DomainType};

/// `apple.signing_key.parse`.
pub struct AppleSigningKeyParse {
    spec: ToolSpec,
}

impl AppleSigningKeyParse {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("value"), any_secret(true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("AppleSigningKey"));
        Self {
            spec: ToolSpec {
                name: tool_name("apple.signing_key.parse"),
                description: "Parse a decoded secret as an EC P-256 (PKCS#8 or SEC1 PEM) App \
                               Store Connect signing key."
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
        let value = inputs
            .get(&port("value"))
            .ok_or_else(|| invalid("port `value` is required"))?;
        if !value.is_known() {
            return Err(invalid("port `value` is unknown"));
        }
        let object = value
            .as_scalar()
            .ok_or_else(|| invalid("port `value` must be a scalar secret"))?;
        let key =
            willikins_types::secret::reveal_transform_input(object, AppleSigningKey::parse, || {
                willikins_types::ParseError::new(
                    "AppleSigningKey",
                    "value is a secret type this parse cannot read",
                )
            })
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
    use willikins_types::OpaqueSecret;

    fn inputs_with(value: &str) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("value").unwrap(),
            Value::known(OpaqueSecret::parse(value).unwrap()),
        );
        inputs
    }

    fn inputs_with_doppler_secret(value: &str) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("value").unwrap(),
            Value::known(willikins_types::DopplerSecretValue::parse(value).unwrap()),
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

    #[test]
    fn spec_value_port_accepts_any_secret() {
        assert_eq!(
            AppleSigningKeyParse::new()
                .spec()
                .inputs
                .get(&port("value"))
                .unwrap()
                .ty,
            willikins_core::PortType::AnySecret
        );
    }

    #[test]
    fn parses_a_key_delivered_as_a_doppler_secret_value_directly() {
        // The plain-.p8-on-disk document from the two-documents pair:
        // a resolver whose output is a `DopplerSecretValue` and needs no
        // base64 layer at all can still bind straight here.
        let inputs = inputs_with_doppler_secret(AppleSigningKey::example());
        let Observation::Present(outputs) = AppleSigningKeyParse::new().read(&inputs).unwrap()
        else {
            panic!("expected Present");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        assert_eq!(value.render().to_string(), "[REDACTED AppleSigningKey]");
    }
}
