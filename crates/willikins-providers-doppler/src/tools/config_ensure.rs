//! `doppler.config.ensure`: creates a real Doppler environment (and,
//! implicitly, its root config), named after its environment by
//! `naming::v1::doppler_root_config`. Port table and behaviour identical
//! to `willikins_providers_fake`'s tool of the same name
//! (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).
//!
//! A `200` at the derived name is only `Present` when the fetched config
//! is a *root* config; anything else sitting at that name is `Foreign`
//! and `ensure` refuses it. See [`DopplerClient::get_config`].

use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, port, require_present, scalar, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerProject, EnvironmentSlug, naming};

use crate::client::DopplerClient;

/// `doppler.config.ensure`.
pub struct DopplerConfigEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerConfigEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("project"), exact("DopplerProject", true));
        inputs.insert(port("environment"), exact("EnvironmentSlug", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.config.ensure"),
                description: "Ensure an environment's root Doppler config exists.".to_string(),
                inputs,
                outputs,
                key: vec![port("project"), port("environment")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerProject, EnvironmentSlug), ToolError> {
        require_present(&self.spec, inputs)?;
        let project = get(inputs, "project")?;
        let environment = get(inputs, "environment")?;
        Ok((project, environment))
    }

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }

    /// `GET` the config, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`.
    ///
    /// `Present` needs `200` **and** `root: true`, per the plan's port
    /// table. A `200` carrying `root: false` — or no usable `root` at all
    /// — is a config that merely occupies this name, not this
    /// environment's root config, and is reported `Foreign`: see
    /// [`DopplerClient::get_config`]'s own docs for the branch-config
    /// name collision that makes this reachable rather than theoretical.
    fn observe(
        &self,
        project: &DopplerProject,
        config: &DopplerConfig,
    ) -> Result<Observation, ToolError> {
        match self.client.get_config(project, config.name()) {
            Ok(body) if body.root == Some(true) => {
                Ok(Observation::Present(Self::outputs_for(config)))
            }
            Ok(_) => Ok(Observation::Foreign),
            Err(err) if err.status == Some(404) => Ok(Observation::Absent {
                predicted: Self::outputs_for(config),
            }),
            Err(err) => Err(err.into()),
        }
    }

    fn foreign_conflict(config: &DopplerConfig) -> ToolError {
        conflict(format!(
            "`{config}` already exists and is not the environment's root config"
        ))
    }
}

impl Tool for DopplerConfigEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (project, environment) = self.key_ports(inputs)?;
        let config = naming::v1::doppler_root_config(&project, &environment);
        self.observe(&project, &config)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (project, environment) = self.key_ports(inputs)?;
        let config = naming::v1::doppler_root_config(&project, &environment);
        match self.observe(&project, &config)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Foreign => Err(Self::foreign_conflict(&config)),
            Observation::Mismatch { .. } => unreachable!(
                "doppler.config.ensure's own observe never returns Mismatch: it has no \
                 non-key input to mismatch on"
            ),
            Observation::Absent { .. } => {
                match self.client.create_environment(&project, config.name()) {
                    Ok(()) => Ok(Ensured {
                        outputs: Self::outputs_for(&config),
                        changed: true,
                    }),
                    // Doppler documents no error-body schema at all, so a
                    // conflict after a possibly delivered create cannot
                    // be told apart from a genuine failure by its body:
                    // re-read either way.
                    Err(err) => match self.observe(&project, &config)? {
                        Observation::Present(outputs) => Ok(Ensured {
                            outputs,
                            changed: false,
                        }),
                        Observation::Foreign => Err(Self::foreign_conflict(&config)),
                        Observation::Absent { .. } | Observation::Mismatch { .. } => {
                            Err(err.into())
                        }
                    },
                }
            }
        }
    }
}
