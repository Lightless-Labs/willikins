//! `doppler.config.inherits.ensure`: makes a real Doppler config inherit
//! a set of base configs. Port table and behaviour identical to
//! `willikins_providers_fake`'s tool of the same name
//! (`tests/catalog_parity.rs` pins the two `ToolSpec`s equal).
//!
//! # Extra entries are `Mismatch`, never silently dropped
//!
//! `POST /v3/configs/config/inherits` takes the *whole* `inherits` array
//! (`docs/research/2026-09-12-m2-dependencies.md`, "Config Inheritance"):
//! it replaces what `config` inherits, it does not add to it. So a
//! config that inherits something this tool's `inherits` input did not
//! ask for is ambiguous between two readings — a base a human wired up
//! by hand, or one an earlier, differently-configured run of this same
//! workflow left behind — and this tool cannot tell which from here.
//! Reading it `Absent` and letting `ensure` `POST` the requested set
//! would silently drop the extra, which is exactly the kind of surprise
//! a `Reversible` tool's own class promises it will not spring: nothing
//! about "reversible" says "will overwrite a human's edit without
//! saying so". Reading it `Present` would leave the drift unreported.
//! **`Mismatch { port: "inherits" }` is the answer instead**: `plan`
//! refuses rather than guesses, the same posture the type registry and
//! `check` already take everywhere else this crate can decide something
//! statically. A workflow author who means to *replace* the set drops
//! this node in favour of a fresh one, or clears the extra by hand
//! first; nothing here does it for them.
//!
//! Missing entries alone (`inherits` requested but not yet on the
//! config, with nothing extra) are the ordinary `Absent` case: the
//! `POST` would only ever *add* to what is already there, so it cannot
//! stomp anything.

use std::collections::BTreeSet;
use std::sync::Arc;

use willikins_core::tool::helpers::{
    conflict, exact, get, invalid, list, port, require_present, tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec,
};
use willikins_types::DopplerConfig;

use crate::client::{DopplerClient, looks_like_a_missing_project};

/// `doppler.config.inherits.ensure`.
pub struct DopplerConfigInheritsEnsure {
    spec: ToolSpec,
    client: Arc<DopplerClient>,
}

impl DopplerConfigInheritsEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<DopplerClient>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(
            port("inherits"),
            PortSpec {
                ty: PortType::Exact(list("DopplerConfig")),
                required: true,
                derived_only: false,
            },
        );
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.config.inherits.ensure"),
                description: "Ensure a Doppler config inherits a set of base configs.".to_string(),
                inputs,
                outputs: indexmap::IndexMap::new(),
                key: vec![port("config")],
                class: Class::Reversible,
                pure: false,
            },
            client,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerConfig, Vec<DopplerConfig>), ToolError> {
        require_present(&self.spec, inputs)?;
        let config = get(inputs, "config")?;
        let inherits = Self::get_config_list(inputs, "inherits")?;
        Ok((config, inherits))
    }

    /// Read a required `list<DopplerConfig>` input by its port name.
    /// `willikins_core::tool::helpers::get` only recovers a scalar (it
    /// downcasts a single [`willikins_core::Value`]); no equivalent
    /// exists yet for a list, so this tool reads its own.
    fn get_config_list(inputs: &Inputs, name: &str) -> Result<Vec<DopplerConfig>, ToolError> {
        let value = inputs
            .get(&port(name))
            .ok_or_else(|| invalid(format!("port `{name}` is required")))?;
        if !value.is_known() {
            return Err(invalid(format!("port `{name}` is unknown")));
        }
        let items = value
            .as_list()
            .ok_or_else(|| invalid(format!("port `{name}` is not a list")))?;
        items
            .iter()
            .map(|item| {
                willikins_types::downcast::<DopplerConfig>(item.as_ref())
                    .cloned()
                    .ok_or_else(|| invalid(format!("port `{name}` has an unexpected type")))
            })
            .collect()
    }

    /// `inherits`, as the canonical-string set `observe` compares
    /// against `config`'s actual `inherits` array. Canonical strings
    /// rather than `DopplerConfig` itself: the type has no `Hash`, and a
    /// set of at most a handful of entries makes string comparison exact
    /// (canonical strings round-trip, since `DopplerConfig`'s `Display`
    /// is its own parser's input) without needing one.
    fn wanted_set(inherits: &[DopplerConfig]) -> BTreeSet<String> {
        inherits.iter().map(ToString::to_string).collect()
    }

    /// `GET` the config, mapped to an [`Observation`]. Shared by `read`
    /// and `ensure`. See the module doc for the `Mismatch` reasoning.
    fn observe(
        &self,
        config: &DopplerConfig,
        wanted: &[DopplerConfig],
    ) -> Result<Observation, ToolError> {
        match self.client.get_config(config.project(), config.name()) {
            Ok(body) => {
                let actual: BTreeSet<String> = body
                    .inherits
                    .unwrap_or_default()
                    .into_iter()
                    .map(|entry| DopplerConfig::new(entry.project, entry.config).to_string())
                    .collect();
                let wanted = Self::wanted_set(wanted);
                if actual.difference(&wanted).next().is_some() {
                    Ok(Observation::Mismatch {
                        port: port("inherits"),
                    })
                } else if actual == wanted {
                    Ok(Observation::Present(Outputs::new()))
                } else {
                    Ok(Observation::Absent {
                        predicted: Outputs::new(),
                    })
                }
            }
            Err(err) if looks_like_a_missing_project(&err) => Ok(Observation::Absent {
                predicted: Outputs::new(),
            }),
            Err(err) => Err(err.into()),
        }
    }

    fn mismatch_conflict(config: &DopplerConfig) -> ToolError {
        conflict(format!(
            "`{config}` already inherits a config this tool was not asked for, and this tool \
             will not drop it; ask for the full set instead, or clear it by hand"
        ))
    }
}

impl Tool for DopplerConfigInheritsEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, wanted) = self.key_ports(inputs)?;
        self.observe(&config, &wanted)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, wanted) = self.key_ports(inputs)?;
        match self.observe(&config, &wanted)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Mismatch { .. } => Err(Self::mismatch_conflict(&config)),
            Observation::Foreign => unreachable!(
                "doppler.config.inherits.ensure's own observe never returns Foreign: it has no \
                 ownership check"
            ),
            Observation::Absent { .. } => {
                match self.client.set_config_inherits(&config, &wanted) {
                    Ok(()) => Ok(Ensured {
                        outputs: Outputs::new(),
                        changed: true,
                    }),
                    // Doppler documents no error-body schema at all, so a
                    // conflict after a possibly delivered write cannot be
                    // told apart from a genuine failure by its body:
                    // re-read either way, the same deferral every other
                    // `ensure` in this crate relies on.
                    Err(err) => match self.observe(&config, &wanted)? {
                        Observation::Present(outputs) => Ok(Ensured {
                            outputs,
                            changed: false,
                        }),
                        Observation::Mismatch { .. } => Err(Self::mismatch_conflict(&config)),
                        Observation::Absent { .. } | Observation::Foreign => Err(err.into()),
                    },
                }
            }
        }
    }
}
