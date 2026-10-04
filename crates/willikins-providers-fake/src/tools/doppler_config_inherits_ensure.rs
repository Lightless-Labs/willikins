//! `doppler.config.inherits.ensure`: makes (in memory) a Doppler config
//! inherit a set of base configs.
//!
//! Mirrors the live tool's `Mismatch` reasoning (see its own module doc
//! for why): a config that already inherits something this call's
//! `inherits` input did not name is `Mismatch`, never silently dropped
//! or silently left alone.
//!
//! Also mirrors the live tool's `config` output: a pass-through of this
//! tool's own `config` input, known in `Present`, in `Absent`'s
//! `predicted`, and in every `Ensured`. See the live tool's module doc
//! for why (ordering a downstream gate after inheritance; `apply` reuses
//! a pure node's planned outputs rather than re-reading it).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec, Value,
};
use willikins_types::DopplerConfig;

use crate::state::{FakeState, doppler_config_key};
use crate::support::{
    conflict, exact, get, invalid, list, port, require_present, scalar, tool_name,
};

/// `doppler.config.inherits.ensure`.
pub struct DopplerConfigInheritsEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerConfigInheritsEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.config.inherits.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(
            port("inherits"),
            PortSpec {
                ty: PortType::Exact(list("DopplerConfig")),
                required: true,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("config"), scalar("DopplerConfig"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a Doppler config inherits a set of base configs.".to_string(),
                inputs,
                outputs,
                key: vec![port("config")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerConfig, Vec<DopplerConfig>), ToolError> {
        require_present(&self.spec, inputs)?;
        let config = get(inputs, "config")?;
        let inherits = Self::get_config_list(inputs, "inherits")?;
        Ok((config, inherits))
    }

    /// Read a required `list<DopplerConfig>` input by its port name. See
    /// the live tool's identical helper for why this is not shared
    /// through `willikins_core::tool::helpers::get`.
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

    fn wanted_set(inherits: &[DopplerConfig]) -> BTreeSet<String> {
        inherits.iter().map(doppler_config_key).collect()
    }

    /// The `config` output: this tool's own `config` input, passed
    /// through unchanged. See the module doc and the live tool's own
    /// module doc for why every branch carries it known.
    fn outputs_for(config: &DopplerConfig) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("config"), Value::known(config.clone()));
        outputs
    }

    fn observe(
        state: &FakeState,
        config: &DopplerConfig,
        key: &str,
        wanted: &BTreeSet<String>,
    ) -> Observation {
        let actual: BTreeSet<String> = state
            .doppler_config_inherits
            .get(key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        if actual.difference(wanted).next().is_some() {
            Observation::Mismatch {
                port: port("inherits"),
            }
        } else if &actual == wanted {
            Observation::Present(Self::outputs_for(config))
        } else {
            Observation::Absent {
                predicted: Self::outputs_for(config),
            }
        }
    }
}

impl Tool for DopplerConfigInheritsEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, wanted) = self.key_ports(inputs)?;
        let key = doppler_config_key(&config);
        let wanted = Self::wanted_set(&wanted);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        Ok(Self::observe(&state, &config, &key, &wanted))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, wanted) = self.key_ports(inputs)?;
        let key = doppler_config_key(&config);
        let wanted = Self::wanted_set(&wanted);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        match Self::observe(&state, &config, &key, &wanted) {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Mismatch { .. } => Err(conflict(format!(
                "`{config}` already inherits a config this tool was not asked for, and this \
                 tool will not drop it; ask for the full set instead, or clear it by hand"
            ))),
            Observation::Absent { .. } | Observation::Foreign => {
                state
                    .doppler_config_inherits
                    .insert(key, wanted.into_iter().collect());
                Ok(Ensured {
                    outputs: Self::outputs_for(&config),
                    changed: true,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, Value};
    use willikins_types::DomainType;

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn base() -> DopplerConfig {
        DopplerConfig::parse("shared-apple/base").unwrap()
    }

    fn other_base() -> DopplerConfig {
        DopplerConfig::parse("shared-apple/other").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs.insert(
            PortName::parse("inherits").unwrap(),
            Value::known_list(vec![base()]),
        );
        inputs
    }

    fn tool() -> DopplerConfigInheritsEnsure {
        DopplerConfigInheritsEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_empty() {
        let observation = tool().read(&full_inputs()).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    fn read_reports_present_when_seeded_exactly() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_config_inherits(&config(), &[base()]),
        ));
        let observation = DopplerConfigInheritsEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_reports_mismatch_when_an_extra_is_seeded() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_config_inherits(&config(), &[base(), other_base()]),
        ));
        let observation = DopplerConfigInheritsEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(
            matches!(observation, Observation::Mismatch { .. }),
            "got {observation:?}"
        );
    }

    /// Milestone 3j, task C1 (acceptance 9): `Absent`'s own `predicted`
    /// carries `config` known, not just `Present`'s outputs -- a
    /// downstream gate's `get(inputs, "config")` reads this on every
    /// fresh run, where `inherits.ensure` itself plans `Create`.
    #[test]
    fn read_reports_config_known_in_absent_predicted() {
        let Observation::Absent { predicted } = tool().read(&full_inputs()).unwrap() else {
            panic!("expected Absent");
        };
        let out = predicted.get(&PortName::parse("config").unwrap()).unwrap();
        assert_eq!(out.render().to_string(), "third-thoughts/prd");
    }

    #[test]
    fn read_reports_config_known_in_present() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_config_inherits(&config(), &[base()]),
        ));
        let Observation::Present(outputs) = DopplerConfigInheritsEnsure::new(state)
            .read(&full_inputs())
            .unwrap()
        else {
            panic!("expected Present");
        };
        let out = outputs.get(&PortName::parse("config").unwrap()).unwrap();
        assert_eq!(out.render().to_string(), "third-thoughts/prd");
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = tool().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("config"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_inherits_port() {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("inherits"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_on_first_call_and_false_on_a_second() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed);
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(!second.changed);
    }

    /// Milestone 3j, task C1 (acceptance 9): `config` is known after a
    /// write (first call, `changed: true`) and after a converged read
    /// (second call, `changed: false`).
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_config_known_after_a_write_and_after_a_converged_read() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        let out = first
            .outputs
            .get(&PortName::parse("config").unwrap())
            .unwrap();
        assert_eq!(out.render().to_string(), "third-thoughts/prd");
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        let out = second
            .outputs
            .get(&PortName::parse("config").unwrap())
            .unwrap();
        assert_eq!(out.render().to_string(), "third-thoughts/prd");
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_refuses_to_drop_an_extra_already_there() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_config_inherits(&config(), &[base(), other_base()]),
        ));
        let tool = DopplerConfigInheritsEnsure::new(state);
        let token = SinkToken::new();
        let err = tool.ensure(&full_inputs(), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
    }
}
