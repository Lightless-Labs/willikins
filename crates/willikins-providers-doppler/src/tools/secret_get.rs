//! `doppler.secret.get`: reads a real Doppler secret's value. Pure and
//! read-only. Never reads or returns `value.raw` — only `value.computed`
//! (references resolved), the same rule
//! `willikins_providers_fake`'s tool of the same name follows.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    exact, get, not_found, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolErrorKind,
    ToolSpec, Value,
};
use willikins_types::{DopplerConfig, SecretName};

use crate::client::DopplerClient;

/// `doppler.secret.get`.
pub struct DopplerSecretGet {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerSecretGet {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("SecretName", true));
        let mut outputs = indexmap::IndexMap::new();
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
            client,
        }
    }

    /// Look the secret up and build its output, or a
    /// [`ToolError::NotFound`](willikins_core::ToolErrorKind::NotFound)
    /// naming the key it looked for — never the value, since there is
    /// none to name in that case, and Doppler's own `404` never carries
    /// one either.
    ///
    /// A `2xx` whose body did not parse (a missing, `null`, empty, or
    /// non-string `value.computed`) is named the same way. Without that,
    /// the only thing an operator would see is
    /// `willikins_providers_http::Http::finish`'s deliberately
    /// content-free "could not parse the response body as the expected
    /// shape (line N, column M)" — true, redacted, and unactionable,
    /// since nothing in it says *which* secret's response was malformed.
    /// The key is `DopplerConfig` and `SecretName`, both non-secret
    /// domain types, so naming it adds no response text: the provider's
    /// own bytes stay discarded.
    ///
    /// A provider *status* error is left exactly as it is. Its message
    /// already carries the provider's own words, bounded to
    /// `MAX_MESSAGE_CHARS`, and prefixing those would push the whole
    /// string past the bound that bounding exists to guarantee.
    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let name: SecretName = get(inputs, "name")?;
        let value = self
            .client
            .get_secret(config.project(), config.name(), &name)
            .map_err(|err| match err.status {
                Some(404) => not_found(format!("no secret at `{config}#{name}`")),
                Some(status) if (200..300).contains(&status) => ToolError {
                    kind: ToolErrorKind::Provider,
                    message: format!("reading `{config}#{name}`: {}", err.message),
                },
                _ => ToolError::from(err),
            })?;
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
