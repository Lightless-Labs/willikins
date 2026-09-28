//! `appstore.app.get`: mirrors
//! `willikins_providers_appstore::tools::AppstoreAppGet` -- see that
//! crate's own module doc for the full reasoning (a leaf gate, "an App
//! Store Connect app record exists for this bundle identifier").

use std::sync::{Arc, Mutex};

use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::AppleBundleIdentifier;

use crate::state::FakeState;
use crate::support::{exact, get, port, require_present, scalar, tool_name};

static GATE: Gate = Gate {
    need: "an App Store Connect app record exists for this bundle identifier",
    how: "create it in App Store Connect (Apps -> + -> New App); the Account Holder must have \
          signed the latest agreement first",
    subject: &["identifier"],
};

/// `appstore.app.get`.
pub struct FakeAppstoreAppGet {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeAppstoreAppGet {
    const TOOL_NAME: &'static str = "appstore.app.get";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(port("identifier"), exact("AppleBundleIdentifier", true));
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("identifier"), scalar("AppleBundleIdentifier"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "A gate: whether an App Store Connect app record exists for a \
                              bundle identifier."
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

    fn outputs_for(identifier: &AppleBundleIdentifier) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("identifier"), Value::known(identifier.clone()));
        outputs
    }

    fn observe(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, identifier.as_str());
        if state.apple_apps.contains(identifier.as_str()) {
            Ok(Observation::Present(Self::outputs_for(&identifier)))
        } else {
            Ok(Observation::Absent {
                predicted: Self::outputs_for(&identifier),
            })
        }
    }
}

impl Tool for FakeAppstoreAppGet {
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
    use willikins_types::DomainType;

    fn identifier() -> AppleBundleIdentifier {
        AppleBundleIdentifier::parse("com.example.MyApp").unwrap()
    }

    fn inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("issuer_id").unwrap(),
            Value::known(
                willikins_types::AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a")
                    .unwrap(),
            ),
        );
        inputs.insert(
            PortName::parse("key_id").unwrap(),
            Value::known(willikins_types::AppleKeyId::parse("2X9R4HXF34").unwrap()),
        );
        inputs.insert(
            PortName::parse("key").unwrap(),
            Value::known(
                willikins_types::AppleSigningKey::parse(willikins_types::AppleSigningKey::example())
                    .unwrap(),
            ),
        );
        inputs.insert(
            PortName::parse("identifier").unwrap(),
            Value::known(identifier()),
        );
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let tool = FakeAppstoreAppGet::new(Arc::new(Mutex::new(FakeState::new())));
        tool.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_no_app_record_is_seeded() {
        let tool = FakeAppstoreAppGet::new(Arc::new(Mutex::new(FakeState::new())));
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_present_once_seeded() {
        let state = Arc::new(Mutex::new(FakeState::new().with_apple_app(&identifier())));
        let tool = FakeAppstoreAppGet::new(state);
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    fn is_a_gate_over_identifier_only() {
        let tool = FakeAppstoreAppGet::new(Arc::new(Mutex::new(FakeState::new())));
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["identifier"]);
    }
}
