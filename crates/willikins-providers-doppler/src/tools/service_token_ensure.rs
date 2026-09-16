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
    ///
    /// **2026-09-16 defect, fixed here.** Doppler 404s the token-list
    /// endpoint when the `project` or `config` it was asked about does
    /// not exist yet — which, at plan time, is exactly the state before
    /// this workflow's own `doppler.project.ensure` and
    /// `doppler.config.ensure` nodes have run (the first live smoke run
    /// found this: planning the positive fixture against a fresh Doppler
    /// account failed outright at this node). "No parent yet" answers
    /// "is a token named `name` already listed?" the same way "an empty
    /// list" does, so a 404 here reads as `false`, exactly the way
    /// `doppler.config.ensure::observe` already reads a 404 as `Absent`
    /// two tools over — see `fixtures/doppler/README.md` for the full
    /// note and the recorded fixture. `ensure` shares this method with
    /// `read`, so the same tolerance applies to both; that is safe
    /// because a project or config still missing by *apply* time
    /// (ordering should already have created both) makes `ensure` fall
    /// through to the mint `POST` below, which then fails for real
    /// against the still-missing parent — nothing is silently swallowed,
    /// only the question this method answers changes from "does Doppler
    /// error" to "is a token already there".
    fn is_listed(
        &self,
        config: &DopplerConfig,
        name: &DopplerTokenName,
    ) -> Result<bool, ToolError> {
        match self
            .client
            .list_service_tokens(config.project(), config.name())
        {
            Ok(listed) => Ok(listed.iter().any(|entry| entry.name == name.as_str())),
            Err(err) if err.status == Some(404) => Ok(false),
            Err(err) => Err(err.into()),
        }
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
