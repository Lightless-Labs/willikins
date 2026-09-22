//! `appstore.bundle_id_capability.ensure`: switches on (in memory) one
//! capability of an already-registered bundle id. Mirrors
//! `willikins_providers_appstore::tools::AppstoreBundleIdCapabilityEnsure`'s
//! `ToolSpec` (`tests/catalog_parity.rs`, in `willikins-providers-appstore`,
//! pins the two equal) and its refusal of the three portal-only
//! capabilities (`APP_GROUPS`, `APPLE_PAY`, `ICLOUD`) on the `Absent` ->
//! create path -- see that crate's own module doc for the full reasoning.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{AppleBundleIdentifier, AppleCapabilityType};

use crate::state::FakeState;
use crate::support::{exact, get, invalid, not_found, port, require_present, scalar, tool_name};

/// The three capability types this fake, like the live tool, refuses to
/// create (never to read as `Present`) -- see
/// `willikins_providers_appstore::CAPABILITIES_NEEDING_PORTAL_CONFIGURATION`,
/// duplicated here rather than depended on: a fake tool never depends on
/// its live counterpart's crate (the same rule every other fake tool in
/// this crate follows). `tests/catalog_parity.rs` (in
/// `willikins-providers-appstore`) does not catch a drift in *this* list,
/// since it is not part of either `ToolSpec` -- `tests/fake_agrees_with_live.rs`
/// (also in that crate) is what pins the fake's refusal against the live
/// one's, behaviourally.
const CAPABILITIES_NEEDING_PORTAL_CONFIGURATION: [&str; 3] = ["APP_GROUPS", "APPLE_PAY", "ICLOUD"];

/// `appstore.bundle_id_capability.ensure`.
pub struct FakeAppstoreBundleIdCapabilityEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeAppstoreBundleIdCapabilityEnsure {
    const TOOL_NAME: &'static str = "appstore.bundle_id_capability.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(port("identifier"), exact("AppleBundleIdentifier", true));
        inputs.insert(port("capability"), exact("AppleCapabilityType", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("capability"), scalar("AppleCapabilityType"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description:
                    "Switch on one capability of an already-registered App Store Connect bundle id."
                        .to_string(),
                inputs,
                outputs,
                key: vec![port("identifier"), port("capability")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn outputs_for(capability: &AppleCapabilityType) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("capability"), Value::known(capability.clone()));
        outputs
    }

    fn observe(
        state: &FakeState,
        identifier: &AppleBundleIdentifier,
        capability: &AppleCapabilityType,
    ) -> Result<Observation, ToolError> {
        if !state.apple_bundle_ids.contains_key(identifier.as_str()) {
            return Err(not_found(format!(
                "no App Store Connect bundle id has identifier `{identifier}`; run \
                 appstore.bundle_id.ensure first"
            )));
        }
        let present = state
            .apple_bundle_id_capabilities
            .get(identifier.as_str())
            .is_some_and(|set| set.contains(capability.as_str()));
        Ok(if present {
            Observation::Present(Self::outputs_for(capability))
        } else {
            Observation::Absent {
                predicted: Self::outputs_for(capability),
            }
        })
    }
}

impl Tool for FakeAppstoreBundleIdCapabilityEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let capability: AppleCapabilityType = get(inputs, "capability")?;
        let mut state = self.state.lock().unwrap();
        let key = format!("{identifier}#{capability}");
        state.record_read_call(Self::TOOL_NAME, &key);
        Self::observe(&state, &identifier, &capability)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let capability: AppleCapabilityType = get(inputs, "capability")?;
        let mut state = self.state.lock().unwrap();
        let key = format!("{identifier}#{capability}");
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        match Self::observe(&state, &identifier, &capability)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { .. } => {
                if CAPABILITIES_NEEDING_PORTAL_CONFIGURATION.contains(&capability.as_str()) {
                    return Err(invalid(format!(
                        "`{capability}` needs an identifier association (an app group, a \
                         merchant id, or an iCloud container) that the App Store Connect API \
                         cannot express -- enabling it here would leave the bundle id \
                         half-configured. Enable and configure it by hand in App Store \
                         Connect's \"Certificates, Identifiers & Profiles\" > Configure flow \
                         instead."
                    )));
                }
                state
                    .apple_bundle_id_capabilities
                    .entry(identifier.as_str().to_string())
                    .or_default()
                    .insert(capability.to_string());
                Ok(Ensured {
                    outputs: Self::outputs_for(&capability),
                    changed: true,
                })
            }
            Observation::Foreign | Observation::Mismatch { .. } => {
                unreachable!("this fake's own observe never returns these")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::{
        AppleBundleIdName, AppleBundleIdPlatform, AppleIssuerId, AppleKeyId, AppleSigningKey,
        DomainType,
    };

    fn identifier() -> AppleBundleIdentifier {
        AppleBundleIdentifier::parse("com.example.MyApp").unwrap()
    }

    fn inputs_for(capability: &str) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(
            PortName::parse("issuer_id").unwrap(),
            Value::known(AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap()),
        );
        inputs.insert(
            PortName::parse("key_id").unwrap(),
            Value::known(AppleKeyId::parse("2X9R4HXF34").unwrap()),
        );
        inputs.insert(
            PortName::parse("key").unwrap(),
            Value::known(AppleSigningKey::parse(AppleSigningKey::example()).unwrap()),
        );
        inputs.insert(
            PortName::parse("identifier").unwrap(),
            Value::known(identifier()),
        );
        inputs.insert(
            PortName::parse("capability").unwrap(),
            Value::known(AppleCapabilityType::parse(capability).unwrap()),
        );
        inputs
    }

    fn seeded_parent() -> Arc<Mutex<FakeState>> {
        Arc::new(Mutex::new(FakeState::new().with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("third-thoughts").unwrap(),
            &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
        )))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        let tool = FakeAppstoreBundleIdCapabilityEnsure::new(seeded_parent());
        tool.spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_refuses_when_the_parent_does_not_exist() {
        let tool =
            FakeAppstoreBundleIdCapabilityEnsure::new(Arc::new(Mutex::new(FakeState::new())));
        let err = tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::NotFound);
    }

    #[test]
    fn read_reports_absent_when_not_seeded() {
        let tool = FakeAppstoreBundleIdCapabilityEnsure::new(seeded_parent());
        assert!(matches!(
            tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_then_read_gives_present_and_changed_then_unchanged() {
        let tool = FakeAppstoreBundleIdCapabilityEnsure::new(seeded_parent());
        let token = SinkToken::new();
        let first = tool
            .ensure(&inputs_for("PUSH_NOTIFICATIONS"), &token)
            .unwrap();
        assert!(first.changed);
        let second = tool
            .ensure(&inputs_for("PUSH_NOTIFICATIONS"), &token)
            .unwrap();
        assert!(!second.changed);
        assert!(matches!(
            tool.read(&inputs_for("PUSH_NOTIFICATIONS")).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_refuses_app_groups_apple_pay_and_icloud_when_absent() {
        for capability in ["APP_GROUPS", "APPLE_PAY", "ICLOUD"] {
            let tool = FakeAppstoreBundleIdCapabilityEnsure::new(seeded_parent());
            let token = SinkToken::new();
            let err = tool
                .ensure(&inputs_for(capability), &token)
                .expect_err(capability);
            assert_eq!(
                err.kind,
                willikins_core::ToolErrorKind::Invalid,
                "{capability}"
            );
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_converges_an_already_present_portal_only_capability() {
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_apple_bundle_id(
                    &identifier(),
                    &AppleBundleIdName::parse("third-thoughts").unwrap(),
                    &AppleBundleIdPlatform::parse("UNIVERSAL").unwrap(),
                )
                .with_apple_bundle_id_capability(
                    &identifier(),
                    &AppleCapabilityType::parse("APP_GROUPS").unwrap(),
                ),
        ));
        let tool = FakeAppstoreBundleIdCapabilityEnsure::new(state);
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs_for("APP_GROUPS"), &token).unwrap();
        assert!(!ensured.changed);
    }
}
