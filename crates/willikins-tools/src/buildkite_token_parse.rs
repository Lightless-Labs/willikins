//! `buildkite.token.parse`: turns any resolved secret into a
//! [`BuildkiteToken`]. Pure, and follows `github.token.parse` exactly
//! (see that tool's own module doc, and `apple.signing_key.parse` before
//! it) -- the design addendum "Credentials are ports, resolvers are
//! nodes" (`docs/plans/2026-09-11-willikins-design.md`, 2026-09-21)
//! applied to Buildkite: a document may chain `doppler.secret.get` (or
//! `env.get`, optionally through `base64.decode`) into this tool, then
//! bind its output to `buildkite.cluster.get`'s or
//! `buildkite.pipeline.ensure`'s optional `token` port, rather than
//! relying solely on `WILLIKINS_BUILDKITE_TOKEN` in the process
//! environment.
//!
//! **Input port: `AnySecret`, not `exact("OpaqueSecret", ...)`.** Same
//! reasoning as `github.token.parse`: a resolver whose secret is already
//! the raw token (the common case -- a Buildkite API access token stored
//! plainly in Doppler) can bind straight here, and this tool reads its
//! input through `willikins_types::secret::reveal_transform_input`, the
//! same dispatch every transform/parse tool in this crate uses.

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    any_secret, invalid, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{BuildkiteToken, DomainType};

/// `buildkite.token.parse`.
pub struct BuildkiteTokenParse {
    spec: ToolSpec,
}

impl BuildkiteTokenParse {
    /// Build the tool, constructing its spec.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("value"), any_secret(true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), scalar("BuildkiteToken"));
        Self {
            spec: ToolSpec {
                name: tool_name("buildkite.token.parse"),
                description: "Parse a resolved secret as a Buildkite API access token.".to_string(),
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
        let token =
            willikins_types::secret::reveal_transform_input(object, BuildkiteToken::parse, || {
                willikins_types::ParseError::new(
                    "BuildkiteToken",
                    "value is a secret type this parse cannot read",
                )
            })
            .map_err(|err| invalid(format!("value: {}", err.reason)))?;
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::known(token));
        Ok(outputs)
    }
}

impl Default for BuildkiteTokenParse {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for BuildkiteTokenParse {
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

    /// A valid [`BuildkiteToken`], assembled the same way as the type's
    /// own `#[domain(example = ...)]` value: any single literal spelling
    /// this contiguously is exactly what `secret_literal_guard.rs`'s
    /// `BUILDKITE_TOKEN` pattern looks for.
    const EXAMPLE_TOKEN: &str = concat!("bkua_", "exampleexampleexample");

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
        BuildkiteTokenParse::new()
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn parses_a_token() {
        let inputs = inputs_with(EXAMPLE_TOKEN);
        let Observation::Present(outputs) = BuildkiteTokenParse::new().read(&inputs).unwrap()
        else {
            panic!("expected Present");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        assert_eq!(value.render().to_string(), "[REDACTED BuildkiteToken]");
    }

    #[test]
    fn refuses_a_non_token_value_with_a_clear_error_and_no_content() {
        let marker = "wlkn-test-marker-not-a-token";
        let inputs = inputs_with(marker);
        let err = BuildkiteTokenParse::new().read(&inputs).unwrap_err();
        assert!(err.message.contains("pattern"), "{}", err.message);
        assert!(!err.message.contains(marker), "{}", err.message);
    }

    #[test]
    fn ensure_agrees_with_read() {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        let token = SinkToken::new();
        let tool = BuildkiteTokenParse::new();
        let inputs = inputs_with(EXAMPLE_TOKEN);
        let via_ensure = tool.ensure(&inputs, &token).unwrap();
        assert!(!via_ensure.changed);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = BuildkiteTokenParse::new().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("value"), "{}", err.message);
    }

    #[test]
    fn spec_value_port_accepts_any_secret() {
        assert_eq!(
            BuildkiteTokenParse::new()
                .spec()
                .inputs
                .get(&port("value"))
                .unwrap()
                .ty,
            willikins_core::PortType::AnySecret
        );
    }

    #[test]
    fn parses_a_token_delivered_as_a_doppler_secret_value_directly() {
        // The common Buildkite habit: a plain API access token stored
        // directly in Doppler, no base64 layer at all.
        let inputs = inputs_with_doppler_secret(EXAMPLE_TOKEN);
        let Observation::Present(outputs) = BuildkiteTokenParse::new().read(&inputs).unwrap()
        else {
            panic!("expected Present");
        };
        let value = outputs.get(&PortName::parse("value").unwrap()).unwrap();
        assert_eq!(value.render().to_string(), "[REDACTED BuildkiteToken]");
    }
}
