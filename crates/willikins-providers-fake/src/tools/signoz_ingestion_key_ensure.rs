//! `signoz.ingestion_key.ensure`: mints (in memory) a `SigNoz` ingestion
//! key. `read` (and `ensure` on an already-existing key) always report
//! the `key` output `Unknown`: an ingestion key's value cannot be
//! re-read once minted — see the live tool's own module docs
//! (`willikins_providers_signoz::tools::ingestion_key_ensure`) for the
//! full decision. `ensure`'s *create* path is the one exception, for the
//! same reason `doppler.service_token.ensure`'s fake gives: the executor
//! passes an `ensure` call's own outputs downstream in the same run, and
//! a plan that stores the minted key as a Doppler secret in the same run
//! it was minted needs the real bytes to do that with.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::SigNozIngestionKeyName;

use crate::state::{FakeState, signoz_ingestion_key_key};
use crate::support::{exact, get, port, require_present, scalar, tool_name};

/// `signoz.ingestion_key.ensure`.
pub struct SigNozIngestionKeyEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl SigNozIngestionKeyEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "signoz.ingestion_key.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("name"), exact("SigNozIngestionKeyName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("key"), scalar("SigNozIngestionKeyValue"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a SigNoz ingestion key exists. Its value can never be re-read once minted.".to_string(),
                inputs,
                outputs,
                key: vec![port("name")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn name_port(&self, inputs: &Inputs) -> Result<SigNozIngestionKeyName, ToolError> {
        require_present(&self.spec, inputs)?;
        get(inputs, "name")
    }

    /// The always-`Unknown` `key` output.
    fn unknown_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(
            port("key"),
            Value::unknown(scalar("SigNozIngestionKeyValue")),
        );
        outputs
    }
}

impl Tool for SigNozIngestionKeyEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let name = self.name_port(inputs)?;
        let key = signoz_ingestion_key_key(&name);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        if state.signoz_ingestion_keys.contains(&key) {
            Ok(Observation::Present(Self::unknown_outputs()))
        } else {
            Ok(Observation::Absent {
                predicted: Self::unknown_outputs(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let name = self.name_port(inputs)?;
        let key = signoz_ingestion_key_key(&name);
        let mut state = self.state.lock().unwrap();
        let call_count = state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        if state.signoz_ingestion_keys.contains(&key) {
            Ok(Ensured {
                outputs: Self::unknown_outputs(),
                changed: false,
            })
        } else {
            let minted = state.mint_signoz_key(Self::TOOL_NAME, &key, call_count);
            state.signoz_ingestion_keys.insert(key);
            let mut outputs = Outputs::new();
            outputs.insert(port("key"), Value::known(minted));
            Ok(Ensured {
                outputs,
                changed: true,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef};
    use willikins_types::{DomainType, SigNozIngestionKeyValue};

    fn name() -> SigNozIngestionKeyName {
        SigNozIngestionKeyName::parse("willikins-example-key").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs
    }

    fn tool() -> SigNozIngestionKeyEnsure {
        SigNozIngestionKeyEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    fn key_value(outputs: &Outputs) -> Value {
        outputs
            .get(&PortName::parse("key").unwrap())
            .unwrap()
            .clone()
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_with_an_unknown_key_when_empty() {
        let observation = tool().read(&full_inputs()).unwrap();
        let Observation::Absent { predicted } = observation else {
            panic!("expected Absent, got {observation:?}");
        };
        assert!(!key_value(&predicted).is_known());
    }

    #[test]
    fn read_reports_present_with_an_unknown_key_when_seeded() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_signoz_ingestion_key(&name()),
        ));
        let observation = SigNozIngestionKeyEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        assert!(!key_value(&outputs).is_known());
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("name").unwrap(),
            Value::unknown(TypeRef::scalar(
                TypeName::parse("SigNozIngestionKeyName").unwrap(),
            )),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("name"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own key
    fn ensure_then_read_gives_present_and_stays_unknown() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&full_inputs(), &token).unwrap();
        let observation = tool.read(&full_inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        assert!(!key_value(&outputs).is_known());
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own key
    fn ensure_reports_changed_true_on_creation_and_false_on_a_second_call() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed, "minting the key must report changed");
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            !second.changed,
            "ensure on an already-minted key must report changed: false"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own key
    fn ensure_on_creation_returns_the_minted_key_as_known() {
        let state = Arc::new(Mutex::new(FakeState::new().with_next_signoz_key(
            SigNozIngestionKeyValue::parse("marker-value-not-real").unwrap(),
        )));
        let tool = SigNozIngestionKeyEnsure::new(state);
        let token = SinkToken::new();
        let ensured = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(ensured.changed);
        let value = key_value(&ensured.outputs);
        assert!(value.is_known());
        assert_eq!(
            value.render().to_string(),
            "[REDACTED SigNozIngestionKeyValue]",
            "the redacted rendering, not a claim about which bytes"
        );
    }
}
