//! `doppler.config.inheritable.ensure`: marks a real Doppler config
//! inheritable, so other configs may inherit it. Port table and
//! behaviour identical to `willikins_providers_fake`'s tool of the same
//! name (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).
//!
//! Doppler's config-inheritance endpoints are a Team/Enterprise feature
//! (`docs/research/2026-09-12-m2-dependencies.md`, "Config Inheritance").
//! No response body this tool needs differs by plan, so nothing here
//! branches on plan tier; a workplace without the feature fails the
//! `POST` at apply time with whatever status Doppler answers, same as
//! any other provider failure.

use std::sync::Arc;

use willikins_core::tool::helpers::{exact, get, port, require_present, tool_name};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_types::DopplerConfig;

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// `doppler.config.inheritable.ensure`.
pub struct DopplerConfigInheritableEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerConfigInheritableEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.config.inheritable.ensure"),
                description: "Ensure a Doppler config is inheritable.".to_string(),
                inputs,
                outputs: indexmap::IndexMap::new(),
                key: vec![port("config")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    /// `GET` the config, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`.
    ///
    /// `inheritable: true` is `Present`; `false`, or the field's absence
    /// (a config Doppler has never been asked to mark inheritable — the
    /// common case), is `Absent`. A missing parent project or config
    /// reads `Absent` too, through the same
    /// [`looks_like_a_missing_project`] every other read in this crate
    /// shares: see its own doc for what that tolerates and what it does
    /// not.
    fn observe(&self, config: &DopplerConfig) -> Result<Observation, ToolError> {
        match self.client.get_config(config.project(), config.name()) {
            Ok(body) if body.inheritable == Some(true) => Ok(Observation::Present(Outputs::new())),
            Ok(_) => Ok(Observation::Absent {
                predicted: Outputs::new(),
            }),
            Err(err) if looks_like_a_missing_project(&err) => Ok(Observation::Absent {
                predicted: Outputs::new(),
            }),
            Err(err) => Err(err.into()),
        }
    }
}

impl Tool for DopplerConfigInheritableEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        self.observe(&config)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        match self.observe(&config)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Foreign | Observation::Mismatch { .. } => unreachable!(
                "doppler.config.inheritable.ensure's own observe never returns Foreign or \
                 Mismatch: it has no ownership check and no non-key input to mismatch on"
            ),
            Observation::Absent { .. } => {
                match self.client.set_config_inheritable(&config, true) {
                    Ok(()) => Ok(Ensured {
                        outputs: Outputs::new(),
                        changed: true,
                    }),
                    // Doppler documents no error-body schema at all, so a
                    // conflict after a possibly delivered write cannot be
                    // told apart from a genuine failure by its body:
                    // re-read either way, the same deferral every other
                    // `ensure` in this crate relies on.
                    Err(err) => match self.observe(&config)? {
                        Observation::Present(outputs) => Ok(Ensured {
                            outputs,
                            changed: false,
                        }),
                        Observation::Absent { .. }
                        | Observation::Foreign
                        | Observation::Mismatch { .. } => Err(err.into()),
                    },
                }
            }
        }
    }
}
