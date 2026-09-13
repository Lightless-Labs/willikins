//! `doppler.service_token.rotate`: revokes and re-mints (in memory) a
//! Doppler service token. [`Class::Destructive`] — the highest class,
//! since revoking a token that is still in use elsewhere breaks whatever
//! depended on the old bytes.
//!
//! Unlike `doppler.service_token.ensure`, this tool always acts: `read`
//! reports [`Observation::Absent`] unconditionally, so a plan always shows
//! [`willikins_core::Action::Create`] for it — never a `NoOp` that would
//! misrepresent what `ensure` is about to do. This is deliberate: a plan
//! is what a human approves, and a plan that said "nothing will happen"
//! for a step that is about to revoke a live token would be exactly the
//! dishonest plan `crate::state`'s own "every `ensure` reads first"
//! invariant exists to prevent elsewhere. `ensure` always reports
//! `changed: true` to match.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{DopplerConfig, DopplerTokenName};

use crate::state::{FakeState, doppler_service_token_key};
use crate::support::{exact, get, port, require_present, scalar, tool_name};

/// `doppler.service_token.rotate`.
pub struct DopplerServiceTokenRotate {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl DopplerServiceTokenRotate {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "doppler.service_token.rotate";

    /// Build the tool against `state`, constructing its spec. Same ports
    /// as `doppler.service_token.ensure`: `config`, `name` in, `token`
    /// out.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("config"), exact("DopplerConfig", true));
        inputs.insert(port("name"), exact("DopplerTokenName", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("token"), scalar("DopplerServiceToken"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Revoke and re-mint a Doppler service token.".to_string(),
                inputs,
                outputs,
                key: vec![port("config"), port("name")],
                class: Class::Destructive,
                pure: false,
            },
            state,
        }
    }

    fn key_ports(&self, inputs: &Inputs) -> Result<(DopplerConfig, DopplerTokenName), ToolError> {
        require_present(&self.spec, inputs)?;
        let config = get(inputs, "config")?;
        let name = get(inputs, "name")?;
        Ok((config, name))
    }

    /// The always-`Unknown` predicted `token` output `read` reports: a
    /// rotated token's real value is only known to the `ensure` call that
    /// minted it, the same as `doppler.service_token.ensure`'s own
    /// predicted output.
    fn unknown_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("token"), Value::unknown(scalar("DopplerServiceToken")));
        outputs
    }
}

impl Tool for DopplerServiceTokenRotate {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        let key = doppler_service_token_key(&config, &name);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        // Always `Absent`: see this module's own doc for why a rotate
        // step must never plan as `NoOp`.
        Ok(Observation::Absent {
            predicted: Self::unknown_outputs(),
        })
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, name) = self.key_ports(inputs)?;
        let key = doppler_service_token_key(&config, &name);
        let mut state = self.state.lock().unwrap();
        let call_count = state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        // Revoke (existence alone is what the fake tracks, so "revoke" is
        // a no-op on a token that never existed) and re-mint.
        let minted = state.mint_token(Self::TOOL_NAME, &key, call_count);
        state.doppler_service_tokens.insert(key);
        let mut outputs = Outputs::new();
        outputs.insert(port("token"), Value::known(minted));
        Ok(Ensured {
            outputs,
            changed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef};
    use willikins_types::{DomainType, DopplerServiceToken};

    fn config() -> DopplerConfig {
        DopplerConfig::parse("third-thoughts/prd").unwrap()
    }

    fn name() -> DopplerTokenName {
        DopplerTokenName::parse("ci").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs
    }

    fn tool() -> DopplerServiceTokenRotate {
        DopplerServiceTokenRotate::new(Arc::new(Mutex::new(FakeState::new())))
    }

    fn token_value(outputs: &Outputs) -> Value {
        outputs
            .get(&PortName::parse("token").unwrap())
            .unwrap()
            .clone()
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_class_is_destructive() {
        assert_eq!(tool().spec().class, Class::Destructive);
    }

    #[test]
    fn read_always_reports_absent_even_when_the_token_already_exists() {
        let state = Arc::new(Mutex::new(
            FakeState::new().with_doppler_service_token(&config(), &name()),
        ));
        let observation = DopplerServiceTokenRotate::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = full_inputs();
        inputs.insert(
            PortName::parse("config").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("DopplerConfig").unwrap())),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("config"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_always_reports_changed_true_and_a_known_token() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed, "a rotate always changes something");
        assert!(token_value(&first.outputs).is_known());
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(
            second.changed,
            "a second rotate must also report changed: true"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_mints_a_different_token_each_call() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        let first_value = first
            .outputs
            .get(&PortName::parse("token").unwrap())
            .unwrap()
            .downcast::<DopplerServiceToken>()
            .cloned();
        let second_value = second
            .outputs
            .get(&PortName::parse("token").unwrap())
            .unwrap()
            .downcast::<DopplerServiceToken>()
            .cloned();
        assert_ne!(
            first_value, second_value,
            "a rotation must actually mint a fresh value each time"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_uses_the_seeded_next_token_once() {
        let seeded = DopplerServiceToken::parse(&format!("dp.st.prd.{}", "MARKER".repeat(7)))
            .expect("a valid token literal");
        let state = Arc::new(Mutex::new(FakeState::new().with_next_token(seeded.clone())));
        let tool = DopplerServiceTokenRotate::new(state);
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        let minted = first
            .outputs
            .get(&PortName::parse("token").unwrap())
            .unwrap()
            .downcast::<DopplerServiceToken>()
            .cloned()
            .unwrap();
        assert_eq!(minted, seeded);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_records_a_call_per_attempt_and_an_injected_failure_fires_once() {
        let state = Arc::new(Mutex::new(FakeState::new().with_fail_ensure_once(
            DopplerServiceTokenRotate::TOOL_NAME,
            &doppler_service_token_key(&config(), &name()),
        )));
        let tool = DopplerServiceTokenRotate::new(Arc::clone(&state));
        let token = SinkToken::new();

        let err = tool.ensure(&full_inputs(), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Provider);

        let ensured = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(ensured.changed);

        let key = doppler_service_token_key(&config(), &name());
        let locked = state.lock().unwrap();
        assert_eq!(
            locked.ensure_calls
                [&crate::state::call_key(DopplerServiceTokenRotate::TOOL_NAME, &key)],
            2
        );
    }
}
