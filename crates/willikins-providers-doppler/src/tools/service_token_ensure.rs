//! `doppler.service_token.ensure`: mints a real Doppler service token.
//! `read` (and `ensure` on an already-existing token) always report the
//! `token` output `Unknown`: a service token's value cannot be re-read
//! once issued — the list endpoint omits `key` entirely. `ensure`'s
//! *create* path is the one exception, matching
//! `willikins_providers_fake`'s tool of the same name.

use std::sync::Arc;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerTokenName};

use crate::client::DopplerClient;

/// `doppler.service_token.ensure`.
pub struct DopplerServiceTokenEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerServiceTokenEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("DopplerTokenName", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("token"), scalar("DopplerServiceToken"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.service_token.ensure"),
                description: "Ensure a Doppler service token exists. Its value can never be re-read once issued.".to_string(),
                inputs,
                outputs,
                key: vec![port("config"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerConfig, DopplerTokenName), ToolError> {
        require_present(&self.spec, inputs)?;
        let config = get(inputs, "config")?;
        let name = get(inputs, "name")?;
        Ok((config, name))
    }

    fn unknown_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("token"), Value::unknown(scalar("DopplerServiceToken")));
        outputs
    }

    /// Whether a token by `name` is already listed for `config`.
    fn is_listed(
        &self,
        config: &DopplerConfig,
        name: &DopplerTokenName,
    ) -> Result<bool, ToolError> {
        let listed = self
            .client
            .list_service_tokens(config.project(), config.name())?;
        Ok(listed.iter().any(|entry| entry.name == name.as_str()))
    }
}

impl Tool for DopplerServiceTokenEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        if self.is_listed(&config, &name)? {
            Ok(Observation::Present(Self::unknown_outputs()))
        } else {
            Ok(Observation::Absent {
                predicted: Self::unknown_outputs(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        if self.is_listed(&config, &name)? {
            return Ok(Ensured {
                outputs: Self::unknown_outputs(),
                changed: false,
            });
        }
        let created = self
            .client
            .create_service_token(config.project(), config.name(), &name)?;
        let mut outputs = Outputs::new();
        outputs.insert(port("token"), Value::known(created.key));
        Ok(Ensured {
            outputs,
            changed: true,
        })
    }
}
