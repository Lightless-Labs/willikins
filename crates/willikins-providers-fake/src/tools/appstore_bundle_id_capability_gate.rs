//! `appstore.bundle_id_capability.gate`: mirrors
//! `willikins_providers_appstore::tools::AppstoreBundleIdCapabilityGate`
//! -- see that crate's own module doc for the full reasoning. Not a leaf:
//! it passes `identifier` through as its own output, exactly like the
//! live tool, for any [`AppleObservableCapabilityType`] named by
//! `capability`.

use std::sync::{Arc, Mutex};

use willikins_core::{
    Class, Ensured, Gate, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{AppleBundleIdentifier, AppleObservableCapabilityType};

use crate::state::FakeState;
use crate::support::{exact, get, port, require_present, scalar, tool_name};

static GATE: Gate = Gate {
    need: "the named capability enabled on this bundle identifier",
    how: "willikins cannot enable this: the operator enables the named capability for the named \
          identifier in App Store Connect (Configure), or from Xcode's Signing & Capabilities.",
    subject: &["identifier", "capability"],
};

/// `appstore.bundle_id_capability.gate`.
pub struct FakeAppstoreBundleIdCapabilityGate {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeAppstoreBundleIdCapabilityGate {
    const TOOL_NAME: &'static str = "appstore.bundle_id_capability.gate";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = indexmap::IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(port("identifier"), exact("AppleBundleIdentifier", true));
        inputs.insert(
            port("capability"),
            exact("AppleObservableCapabilityType", true),
        );
        let mut outputs = indexmap::IndexMap::new();
        outputs.insert(port("identifier"), scalar("AppleBundleIdentifier"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "A gate: whether a named capability is enabled on a bundle \
                              identifier."
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
    /// `FakeAppstoreAppGroupGate::observe_state` already follows).
    fn observe_state(
        state: &FakeState,
        identifier: &AppleBundleIdentifier,
        capability: &AppleObservableCapabilityType,
    ) -> Observation {
        if !state.apple_bundle_ids.contains_key(identifier.as_str()) {
            return Observation::Absent {
                predicted: Self::outputs_for(identifier),
            };
        }
        let enabled = state
            .apple_bundle_id_capabilities
            .get(identifier.as_str())
            .is_some_and(|set| set.contains(capability.as_str()));
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
        let capability: AppleObservableCapabilityType = get(inputs, "capability")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, identifier.as_str());
        Ok(Self::observe_state(&state, &identifier, &capability))
    }
}

impl Tool for FakeAppstoreBundleIdCapabilityGate {
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
    use willikins_types::{AppleBundleIdName, AppleBundleIdPlatform, DomainType};

    fn identifier() -> AppleBundleIdentifier {
        AppleBundleIdentifier::parse("com.example.MyApp").unwrap()
    }

    fn capability() -> AppleObservableCapabilityType {
        AppleObservableCapabilityType::parse("APP_ATTEST").unwrap()
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
        inputs.insert(
            PortName::parse("capability").unwrap(),
            Value::known(capability()),
        );
        inputs
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let tool = FakeAppstoreBundleIdCapabilityGate::new(Arc::new(Mutex::new(FakeState::new())));
        tool.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_the_parent_is_not_registered() {
        let tool = FakeAppstoreBundleIdCapabilityGate::new(Arc::new(Mutex::new(FakeState::new())));
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_absent_when_registered_but_the_capability_is_not_enabled() {
        let state = Arc::new(Mutex::new(FakeState::new().with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("my-app").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )));
        let tool = FakeAppstoreBundleIdCapabilityGate::new(state);
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    fn read_reports_present_once_the_read_only_capability_is_enabled() {
        let mut state = FakeState::new().with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("my-app").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        );
        // `AppleCapabilityType` cannot express `APP_ATTEST` (it is
        // read-only, deliberately excluded from that type's writable
        // grammar) -- inserted directly into the fake's own capability
        // set, the same shape a real `APP_ATTEST` row would leave there.
        state
            .apple_bundle_id_capabilities
            .entry(identifier().as_str().to_string())
            .or_default()
            .insert("APP_ATTEST".to_string());
        let tool = FakeAppstoreBundleIdCapabilityGate::new(Arc::new(Mutex::new(state)));
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    fn output_identifier_passes_the_input_through() {
        let mut state = FakeState::new().with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("my-app").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        );
        state
            .apple_bundle_id_capabilities
            .entry(identifier().as_str().to_string())
            .or_default()
            .insert("APP_ATTEST".to_string());
        let tool = FakeAppstoreBundleIdCapabilityGate::new(Arc::new(Mutex::new(state)));
        let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
            panic!("expected Present");
        };
        let out = outputs
            .get(&PortName::parse("identifier").unwrap())
            .unwrap();
        assert_eq!(out.render().to_string(), "com.example.MyApp");
    }

    #[test]
    fn is_a_gate_over_identifier_and_capability() {
        let tool = FakeAppstoreBundleIdCapabilityGate::new(Arc::new(Mutex::new(FakeState::new())));
        let gate = tool.gate().expect("this tool declares a gate");
        assert_eq!(gate.subject, &["identifier", "capability"]);
    }

    /// A writable capability (`APP_GROUPS`), seeded through
    /// `with_apple_bundle_id_capability`, is observable through this
    /// gate too -- it is a strict superset, not a disjoint type.
    #[test]
    fn a_writable_capability_is_also_observable() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_apple_bundle_id(
                    &identifier(),
                    &AppleBundleIdName::parse("my-app").unwrap(),
                    &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
                )
                .with_apple_bundle_id_capability(
                    &identifier(),
                    &willikins_types::AppleCapabilityType::parse("APP_GROUPS").unwrap(),
                ),
        ));
        let mut app_groups_inputs = inputs();
        app_groups_inputs.insert(
            PortName::parse("capability").unwrap(),
            Value::known(AppleObservableCapabilityType::parse("APP_GROUPS").unwrap()),
        );
        let tool = FakeAppstoreBundleIdCapabilityGate::new(state);
        assert!(matches!(
            tool.read(&app_groups_inputs).unwrap(),
            Observation::Present(_)
        ));
    }
}
