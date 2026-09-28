//! `appstore.app_group.gate`: mirrors
//! `willikins_providers_appstore::tools::AppstoreAppGroupGate` -- see
//! that crate's own module doc for the full reasoning. Not a leaf: it
//! passes `identifier` through as its own output, exactly like the live
//! tool.

use std::sync::{Arc, Mutex};

use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::AppleBundleIdentifier;

use crate::state::FakeState;
use crate::support::{exact, get, port, require_present, scalar, tool_name};

/// The one capability this gate ever checks -- duplicated from
/// `willikins_providers_appstore::tools::AppstoreAppGroupGate`, for the
/// same "a fake never depends on its live counterpart" reason
/// `FakeAppstoreBundleIdCapabilityEnsure`'s own doc gives.
const APP_GROUPS: &str = "APP_GROUPS";

static GATE: Gate = Gate {
    need: "APP_GROUPS enabled on this bundle identifier",
    how: "register group.<app identifier>, enable App Groups on the identifier in App Store \
          Connect (Configure), and assign the group to it (portal Configure, or Xcode)",
    subject: &["identifier"],
};

/// `appstore.app_group.gate`.
pub struct FakeAppstoreAppGroupGate {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeAppstoreAppGroupGate {
    const TOOL_NAME: &'static str = "appstore.app_group.gate";

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
                description: "A gate: whether APP_GROUPS is enabled on a bundle identifier."
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

    /// Mirrors the live tool's own `observe`: an unregistered parent
    /// identifier is `Absent`, not a refusal (the same precedent
    /// `FakeAppstoreBundleIdCapabilityEnsure::observe` already follows).
    fn observe_state(state: &FakeState, identifier: &AppleBundleIdentifier) -> Observation {
        if !state.apple_bundle_ids.contains_key(identifier.as_str()) {
            return Observation::Absent {
                predicted: Self::outputs_for(identifier),
            };
        }
        let enabled = state
            .apple_bundle_id_capabilities
            .get(identifier.as_str())
            .is_some_and(|set| set.contains(APP_GROUPS));
        if enabled {
            Observation::Present(Self::outputs_for(identifier))
        } else {
            Observation::Absent {
                predicted: Self::outputs_for(identifier),
            }
        }
    }

    fn observe(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, identifier.as_str());
        Ok(Self::observe_state(&state, &identifier))
    }
}

impl Tool for FakeAppstoreAppGroupGate {
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
    use willikins_types::{
        AppleBundleIdName, AppleBundleIdPlatform, AppleCapabilityType, DomainType,
    };

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
        let tool = FakeAppstoreAppGroupGate::new(Arc::new(Mutex::new(FakeState::new())));
        tool.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_the_parent_is_not_registered() {
        let tool = FakeAppstoreAppGroupGate::new(Arc::new(Mutex::new(FakeState::new())));
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_absent_when_registered_but_app_groups_is_not_enabled() {
        let state = Arc::new(Mutex::new(FakeState::new().with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("my-app").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )));
        let tool = FakeAppstoreAppGroupGate::new(state);
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_present_once_app_groups_is_enabled() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_apple_bundle_id(
                    &identifier(),
                    &AppleBundleIdName::parse("my-app").unwrap(),
                    &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
                )
                .with_apple_bundle_id_capability(
                    &identifier(),
                    &AppleCapabilityType::parse("APP_GROUPS").unwrap(),
                ),
        ));
        let tool = FakeAppstoreAppGroupGate::new(state);
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    fn output_identifier_passes_the_input_through() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_apple_bundle_id(
                    &identifier(),
                    &AppleBundleIdName::parse("my-app").unwrap(),
                    &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
                )
                .with_apple_bundle_id_capability(
                    &identifier(),
                    &AppleCapabilityType::parse("APP_GROUPS").unwrap(),
                ),
        ));
        let tool = FakeAppstoreAppGroupGate::new(state);
        let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
            panic!("expected Present");
        };
        let out = outputs
            .get(&PortName::parse("identifier").unwrap())
            .unwrap();
        assert_eq!(out.render().to_string(), "com.example.MyApp");
    }

    #[test]
    fn is_a_gate_over_identifier_only() {
        let tool = FakeAppstoreAppGroupGate::new(Arc::new(Mutex::new(FakeState::new())));
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["identifier"]);
    }
}
