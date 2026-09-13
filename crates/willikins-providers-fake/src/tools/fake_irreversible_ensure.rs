//! `fake.irreversible.ensure`: a test tool with [`Class::Irreversible`],
//! used to exercise the plan's approval-class computation.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_types::ProjectSlug;

use crate::state::{FakeState, irreversible_key};
use crate::support::{exact, get, port, require_present, tool_name};

/// `fake.irreversible.ensure`.
pub struct FakeIrreversibleEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeIrreversibleEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "fake.irreversible.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("key"), exact("ProjectSlug", true));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Test tool: an irreversible resource, keyed by a project slug."
                    .to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: vec![port("key")],
                class: Class::Irreversible,
                pure: false,
            },
            state,
        }
    }

    fn key_port(&self, inputs: &Inputs) -> Result<ProjectSlug, ToolError> {
        require_present(&self.spec, inputs)?;
        get(inputs, "key")
    }
}

impl Tool for FakeIrreversibleEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let slug = self.key_port(inputs)?;
        let key = irreversible_key(&slug);
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &key);
        if state.irreversible.contains(&key) {
            Ok(Observation::Present(Outputs::new()))
        } else {
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let slug = self.key_port(inputs)?;
        let key = irreversible_key(&slug);
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        let changed = state.irreversible.insert(key);
        Ok(Ensured {
            outputs: Outputs::new(),
            changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::{PortName, TypeName, TypeRef, Value};
    use willikins_types::DomainType;

    fn slug() -> ProjectSlug {
        ProjectSlug::parse("third-thoughts").unwrap()
    }

    fn full_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("key").unwrap(), Value::known(slug()));
        inputs
    }

    fn tool() -> FakeIrreversibleEnsure {
        FakeIrreversibleEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn spec_class_is_irreversible() {
        assert_eq!(tool().spec().class, Class::Irreversible);
    }

    #[test]
    fn read_reports_absent_when_empty() {
        let observation = tool().read(&full_inputs()).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    fn read_reports_present_when_seeded() {
        let state = Arc::new(Mutex::new(FakeState::new().with_irreversible(&slug())));
        let observation = FakeIrreversibleEnsure::new(state)
            .read(&full_inputs())
            .unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    fn read_rejects_an_unknown_key_port() {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("key").unwrap(),
            Value::unknown(TypeRef::scalar(TypeName::parse("ProjectSlug").unwrap())),
        );
        let err = tool().read(&inputs).unwrap_err();
        assert!(err.message.contains("key"), "{}", err.message);
    }

    #[test]
    fn read_rejects_a_missing_port() {
        let err = tool().read(&Inputs::new()).unwrap_err();
        assert!(err.message.contains("key"), "{}", err.message);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_then_read_gives_present() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&full_inputs(), &token).unwrap();
        let observation = tool.read(&full_inputs()).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_changed_true_on_creation_and_false_on_a_second_call() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(first.changed);
        let second = tool.ensure(&full_inputs(), &token).unwrap();
        assert!(!second.changed);
    }
}
