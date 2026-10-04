//! `doppler.secret_name.gate`: mirrors
//! `willikins_providers_doppler::tools::DopplerSecretNameGate` -- see
//! that crate's own module doc for the full reasoning (milestone 3j,
//! task B3). Not a leaf: it passes `config` through as its own output,
//! exactly like the live tool and like
//! [`crate::tools::DopplerConfigInheritableGate`], so a document that
//! wants to order a consumer after this gate can bind from it.
//!
//! # What it reads
//!
//! A config absent from [`FakeState::doppler_configs`] reads `Absent`
//! with no walk at all, mirroring the live tool's "a config that does
//! not exist yet, or one this token cannot see" deviation. Otherwise the
//! name is visible when `config#NAME` ([`doppler_secret_key`]) is a
//! member of [`FakeState::doppler_secrets`], [`FakeState::doppler_values`]
//! or [`FakeState::doppler_secret_writes`] -- membership only, never a
//! read or a clone of the value itself. If none of those three holds the
//! name directly, the gate walks [`FakeState::doppler_config_inherits`]
//! for `config` one level, applying the very same membership test to
//! each base in turn (a base absent from
//! [`FakeState::doppler_configs`] contributes nothing, because nothing
//! is ever seeded under a key for a config that does not exist). There
//! is no new seed field: a seed that wants a name visible seeds
//! [`FakeState::doppler_secrets`] exactly as every existing seed already
//! does.

use std::sync::{Arc, Mutex};

use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, SecretName};

use crate::state::{FakeState, doppler_config_key, doppler_secret_key};
use crate::support::{exact, get, port, require_present, scalar, tool_name};

/// This tool's gate: both ports are the subject, matching the live
/// tool's own [`Gate`] field for field.
static GATE: Gate = Gate {
    need: "this secret name is visible in this Doppler config, set there or inherited from a config it inherits",
    how: "store the secret in this config, or in an inheritable config this one inherits (doppler.config.inherits.ensure); if this run also changed what this config inherits, re-run: a gate observes before the run applies; a config that does not exist yet, or one this token cannot see (Doppler answers both the same way), also blocks here until it is created or this token is granted read access",
    subject: &["config", "name"],
};

/// `doppler.secret_name.gate`.
pub struct DopplerSecretNameGate {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerSecretNameGate {
    const TOOL_NAME: &'static str = "doppler.secret_name.gate";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("SecretName", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "A gate: whether a secret name is visible in a Doppler config, set there or inherited."
                    .to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
            state,
        }
    }

    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }

    /// Whether `name` is a member of any of the three maps a secret
    /// under `config_key` could be seeded into -- membership only,
    /// never a read of the value itself.
    fn listed(state: &FakeState, config_key: &str, name: &SecretName) -> bool {
        let secret_key = format!("{config_key}#{name}");
        state.doppler_secrets.contains_key(&secret_key)
            || state.doppler_values.contains_key(&secret_key)
            || state.doppler_secret_writes.contains(&secret_key)
    }

    /// Mirrors the live tool's own `observe` (decision (b2)): a config
    /// absent from [`FakeState::doppler_configs`] reads `Absent` with no
    /// walk; otherwise a direct listing wins, then one level of
    /// `inherits`.
    fn observe_state(state: &FakeState, config: &DopplerConfig, name: &SecretName) -> Observation {
        let key = doppler_config_key(config);
        if !state.doppler_configs.contains(&key) {
            return Observation::Absent {
                predicted: Self::outputs_for(config),
            };
        }
        if Self::listed(state, &key, name) {
            return Observation::Present(Self::outputs_for(config));
        }
        if let Some(bases) = state.doppler_config_inherits.get(&key) {
            for base in bases {
                if Self::listed(state, base, name) {
                    return Observation::Present(Self::outputs_for(config));
                }
            }
        }
        Observation::Absent {
            predicted: Self::outputs_for(config),
        }
    }

    fn observe(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let config: DopplerConfig = get(inputs, "config")?;
        let name: SecretName = get(inputs, "name")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &doppler_secret_key(&config, &name));
        Ok(Self::observe_state(&state, &config, &name))
    }
}

impl Tool for DopplerSecretNameGate {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.observe(inputs)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        match self.observe(inputs)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { predicted } => Ok(Ensured {
                outputs: predicted,
                changed: false,
            }),
            other => unreachable!("this gate's own observe never returns {other:?}"),
        }
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&GATE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::{DomainType, DopplerSecretValue};

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn base() -> DopplerConfig {
        DopplerConfig::parse("shared_keys/prd").unwrap()
    }

    fn name() -> SecretName {
        SecretName::parse("EXAMPLE_APNS_KEY").unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs
    }

    fn tool(state: Arc<Mutex<FakeState>>) -> DopplerSecretNameGate {
        DopplerSecretNameGate::new(state)
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        t.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn is_a_gate_over_config_and_name() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        let gate = t.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["config", "name"]);
    }

    #[test]
    fn read_reports_absent_when_the_config_does_not_exist() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_present_when_the_name_is_listed_directly() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_doppler_config(&config())
                .with_doppler_secret(
                    &config(),
                    &name(),
                    DopplerSecretValue::parse("willikins test secret value").unwrap(),
                ),
        ));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    fn read_reports_absent_when_the_config_exists_but_lists_nothing() {
        let state = Arc::new(Mutex::new(FakeState::new().with_doppler_config(&config())));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_present_when_an_inherited_base_lists_it() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_doppler_config(&config())
                .with_doppler_config(&base())
                .with_doppler_config_inherits(&config(), &[base()])
                .with_doppler_secret(
                    &base(),
                    &name(),
                    DopplerSecretValue::parse("willikins test secret value").unwrap(),
                ),
        ));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    fn read_reports_absent_when_the_inherited_base_does_not_list_it() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_doppler_config(&config())
                .with_doppler_config(&base())
                .with_doppler_config_inherits(&config(), &[base()]),
        ));
        let t = tool(state);
        assert!(matches!(
            t.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn present_passes_the_config_through_as_its_own_output() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_doppler_config(&config())
                .with_doppler_secret(
                    &config(),
                    &name(),
                    DopplerSecretValue::parse("willikins test secret value").unwrap(),
                ),
        ));
        let t = tool(state);
        let Observation::Present(outputs) = t.read(&inputs()).unwrap() else {
            panic!("expected Present");
        };
        let out = outputs.get(&PortName::parse("config").unwrap()).unwrap();
        assert_eq!(out.render().to_string(), "third-thoughts/prd");
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_never_reports_changed() {
        let t = tool(Arc::new(Mutex::new(FakeState::new())));
        let token = SinkToken::new();
        let ensured = t.ensure(&inputs(), &token).unwrap();
        assert!(!ensured.changed);
    }
}
