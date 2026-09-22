//! `doppler.value.get`: reads a real Doppler variable's value as
//! non-secret [`willikins_types::Text`]. Pure and read-only, and
//! otherwise a byte-for-byte mirror of [`crate::tools::DopplerSecretGet`]
//! — same config/name ports, same endpoint, same missing-parent
//! tolerance (`404` and the Doppler `400` shape,
//! `docs/solutions/providers/`) — with the one difference load-bearing to
//! this whole tool's reason for existing: the value it reads back is
//! typed [`willikins_types::Text`], not
//! [`willikins_types::DopplerSecretValue`].
//!
//! # What this tool does not protect against
//!
//! Doppler's API carries no secret/non-secret distinction of its own —
//! every value in a config, whatever it is named or marked, is returned
//! by the identical `GET /v3/configs/config/secret` this tool and
//! `doppler.secret.get` both call. **Choosing this tool over
//! `doppler.secret.get` is the document author's own declaration that the
//! variable it names is not a secret.** Nothing here checks that
//! declaration against Doppler's own "masked"/"unmasked" visibility flag,
//! and nothing gates on it: the design addendum this tool implements
//! (`docs/plans/2026-09-11-willikins-design.md`, "Credentials are ports,
//! resolvers are nodes", 2026-09-21) is explicit that there is
//! deliberately no such gate, because the operator marks every secret
//! they store `masked` — an `unmasked` gate would refuse their entire
//! vault and catch nothing an honest declaration does not already catch.
//! An author who points this tool at a genuine secret — an API key, a
//! database password — has declassified it: the value flows out as plain
//! [`willikins_types::Text`], renders in a plan, and lands in the journal
//! like any other non-secret value. That is not a bug this tool can fix;
//! it is the tradeoff of trusting the author's tool choice as the
//! authorization, spelled out here rather than discovered the hard way.
//! The two id ports of the App Store Connect credential
//! (`willikins_types::AppleIssuerId`/`AppleKeyId`) are exactly the
//! intended use: real values, genuinely not secret, that the operator
//! happens to keep in the same vault as the key.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    exact, get, not_found, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolErrorKind,
    ToolSpec, Value,
};
use willikins_types::{DopplerConfig, SecretName};

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// `doppler.value.get`.
pub struct DopplerValueGet {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerValueGet {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("SecretName", true));
        let mut outputs = indexmap::IndexMap::new();
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
            client,
        }
    }

    /// Look the variable up and build its output, or a
    /// [`ToolError::NotFound`](willikins_core::ToolErrorKind::NotFound)
    /// naming the key it looked for. See
    /// [`crate::tools::DopplerSecretGet::lookup`] for the full reasoning
    /// behind every branch here — this mirrors it exactly, over
    /// [`DopplerClient::get_value`] instead of
    /// [`DopplerClient::get_secret`].
    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let name: SecretName = get(inputs, "name")?;
        let value = self
            .client
            .get_value(config.project(), config.name(), &name)
            .map_err(|err| {
                if looks_like_a_missing_project(&err) {
                    not_found(format!("no secret at `{config}#{name}`"))
                } else if err
                    .status
                    .is_some_and(|status| (200..300).contains(&status))
                {
                    ToolError {
                        kind: ToolErrorKind::Provider,
                        message: format!("reading `{config}#{name}`: {}", err.message),
                    }
                } else {
                    ToolError::from(err)
                }
            })?;
        let Some(value) = value else {
            return Err(not_found(format!("no secret at `{config}#{name}`")));
        };
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
    use willikins_providers_http::{Credential, Http};

    fn tool() -> DopplerValueGet {
        let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
        let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
        DopplerValueGet::new(Arc::new(DopplerClient::new(http)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn output_port_is_a_non_secret_text() {
        assert_eq!(
            willikins_types::registry()
                .is_secret(&willikins_types::TypeName::parse("Text").unwrap()),
            Some(false)
        );
    }

    #[test]
    fn spec_matches_secret_get_apart_from_the_output_type() {
        // The mirroring this tool's module doc claims, pinned: same
        // input ports, same pure/class/key shape as `doppler.secret.get`.
        let secret_get =
            crate::tools::DopplerSecretGet::new(Arc::new(DopplerClient::new(Http::new(
                "http://127.0.0.1:1",
                Vec::new(),
                Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken"),
            ))));
        let value_get = tool();
        assert_eq!(value_get.spec().inputs, secret_get.spec().inputs);
        assert_eq!(value_get.spec().class, secret_get.spec().class);
        assert_eq!(value_get.spec().pure, secret_get.spec().pure);
        assert_eq!(value_get.spec().key, secret_get.spec().key);
        assert_ne!(value_get.spec().outputs, secret_get.spec().outputs);
    }
}
