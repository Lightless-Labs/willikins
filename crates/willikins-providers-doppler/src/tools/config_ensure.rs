//! `doppler.config.ensure`: creates a real Doppler environment (and,
//! implicitly, its root config), named after its environment by
//! `naming::v1::doppler_root_config`. Port table and behaviour identical
//! to `willikins_providers_fake`'s tool of the same name
//! (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).

use std::sync::Arc;

use willikins_core::tool::helpers::{exact, get, port, require_present, scalar, tool_name};
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
    /// and `ensure`. This tool's port table gives it no `Foreign` state
    /// (a config found at the name this crate itself derived as a root
    /// config's own identifier is ours by construction — see
    /// [`DopplerClient::get_config`]'s own docs), so this can only ever
    /// answer `Present` or `Absent`.
    fn observe(
        &self,
        project: &DopplerProject,
        config: &DopplerConfig,
    ) -> Result<Observation, ToolError> {
        match self.client.get_config(project, config.name()) {
            Ok(()) => Ok(Observation::Present(Self::outputs_for(config))),
            Err(err) if err.status == Some(404) => Ok(Observation::Absent {
                predicted: Self::outputs_for(config),
            }),
            Err(err) => Err(err.into()),
        }
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
            Observation::Foreign | Observation::Mismatch { .. } => unreachable!(
                "doppler.config.ensure's own observe never returns Foreign or Mismatch"
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
                        _ => Err(err.into()),
                    },
                }
            }
        }
    }
}
