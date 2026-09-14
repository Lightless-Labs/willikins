//! `doppler.secret.get`: reads a real Doppler secret's value. Pure and
//! read-only. Never reads or returns `value.raw` — only `value.computed`
//! (references resolved), the same rule
//! `willikins_providers_fake`'s tool of the same name follows.

use std::sync::Arc;

use willikins_core::tool::helpers::{
    exact, get, not_found, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
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
    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let name: SecretName = get(inputs, "name")?;
        let value = self
            .client
            .get_secret(config.project(), config.name(), &name)
            .map_err(|err| {
                if err.status == Some(404) {
                    not_found(format!("no secret at `{config}#{name}`"))
                } else {
                    ToolError::from(err)
                }
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
