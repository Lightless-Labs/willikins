//! `doppler.secret.set`: records (in memory) that a secret's value was
//! written into a config. Never inspects the write for equality against
//! anything already there, and `read` always reports
//! [`Observation::Absent`] — see the live tool's own module docs
//! (`willikins_providers_doppler::tools::secret_set`) for why a sink
//! whose value cannot be read back safely is not a thing `read` can call
//! `Present` on truthfully.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_types::{DopplerConfig, SecretName};

use crate::state::{FakeState, doppler_secret_key};
use crate::support::{
    any_secret, exact, exact_derived_only, get, invalid, port, require_present, tool_name,
};

/// `doppler.secret.set`.
pub struct DopplerSecretSet {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerSecretSet {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.secret.set";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("config"), exact_derived_only("DopplerConfig"));
        inputs.insert(port("name"), exact("SecretName", true));
        inputs.insert(port("value"), any_secret(true));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Write a secret into a Doppler config. `config` must be the output of an earlier node — never a literal or a workflow input.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: vec![port("config"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerConfig, SecretName), ToolError> {
        require_present(&self.spec, inputs)?;
        let config = get(inputs, "config")?;
        let name = get(inputs, "name")?;
        Ok((config, name))
    }
}

impl Tool for DopplerSecretSet {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        let key = doppler_secret_key(&config, &name);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        // Existence, not value equality -- matching the live tool's own
        // corrected shape (its module docs give the full reasoning: a
        // `value` fed by an unrereadable resolver like
        // `signoz.ingestion_key.ensure` goes `Unknown` on a converging
        // second apply, and only a `NoOp`-planned node may hold an
        // `Unknown` required input without `apply` failing outright).
        // Checked against both this tool's own write record and
        // `doppler.secret.get`'s seeded/written map, so a document that
        // seeds a secret directly (rather than through this tool) still
        // reads `Present` here, the same way the live provider would.
        if state.doppler_secret_writes.contains(&key) || state.doppler_secrets.get(&key).is_some() {
            Ok(Observation::Present(Outputs::new()))
        } else {
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        let value = inputs
            .get(&port("value"))
            .ok_or_else(|| invalid("port `value` is required"))?;
        if !value.is_known() {
            return Err(invalid("port `value` is unknown"));
        }
        let key = doppler_secret_key(&config, &name);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        // A sink whose write is never compared against anything already
        // there always reports `changed: true` when called, matching
        // `github.actions_secret.ensure`'s own reasoning. This fake
        // records only that a write happened at this key, never the
        // secret's own bytes -- `FakeState::doppler_secrets` (a distinct
        // map, populated by `doppler.secret.get`'s own seeding) is
        // untouched, so a document cannot read back what this tool wrote
        // through the fake provider either.
        state.doppler_secret_writes.insert(key);
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, Value};
    use willikins_types::{DomainType, DopplerSecretValue};

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn name() -> SecretName {
        SecretName::parse("MIRRORED_URL").unwrap()
    }

    fn value() -> DopplerSecretValue {
        DopplerSecretValue::parse("a-minted-value-not-real").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs.insert(PortName::parse("value").unwrap(), Value::known(value()));
        inputs
    }

    fn tool() -> DopplerSecretSet {
        DopplerSecretSet::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_config_port_is_derived_only() {
        assert!(
            tool()
                .spec()
                .inputs
                .get(&port("config"))
                .unwrap()
                .derived_only
        );
    }

    #[test]
    fn read_reports_absent_when_never_written() {
        assert!(matches!(
            tool().read(&full_inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    /// The corrected shape: existence, not value equality, so a document
    /// chaining an unrereadable resolver into this sink can converge on a
    /// second `apply` -- see this fake's own `read` doc comment and the
    /// live tool's module docs for the full trace.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_then_read_reports_present() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&full_inputs(), &token).unwrap();
        assert!(matches!(
            tool.read(&full_inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_on_every_call() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed);
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            second.changed,
            "an unreadable-back, uncompared sink always reports changed: true"
        );
    }
}
