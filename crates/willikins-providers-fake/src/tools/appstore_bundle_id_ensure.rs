//! `appstore.bundle_id.ensure`: creates (in memory) an App Store Connect
//! bundle id. Mirrors
//! `willikins_providers_appstore::tools::AppstoreBundleIdEnsure`'s
//! `ToolSpec` exactly (`tests/catalog_parity.rs`, in
//! `willikins-providers-appstore`, pins the two equal) and the same four
//! observations: `Absent`, `Present`, and both `Mismatch` arms (`name`,
//! convergent; `platform`, terminal) -- no `Foreign`, see that crate's
//! own module doc for why.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    AppleBundleIdId, AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, DomainType,
};

use crate::state::{AppleBundleIdRecord, FakeState, fake_apple_bundle_id_id};
use crate::support::{conflict, exact, get, port, require_present, scalar, tool_name};

/// `appstore.bundle_id.ensure`.
pub struct FakeAppstoreBundleIdEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeAppstoreBundleIdEnsure {
    const TOOL_NAME: &'static str = "appstore.bundle_id.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(port("identifier"), exact("AppleBundleIdentifier", true));
        inputs.insert(port("name"), exact("AppleBundleIdName", true));
        inputs.insert(port("platform"), exact("AppleBundleIdPlatform", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("id"), scalar("AppleBundleIdId"));
        outputs.insert(port("identifier"), scalar("AppleBundleIdentifier"));
        outputs.insert(port("name"), scalar("AppleBundleIdName"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Register or converge an App Store Connect bundle identifier."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("identifier")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn outputs_for(
        id: &str,
        identifier: &AppleBundleIdentifier,
        name: &AppleBundleIdName,
    ) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(
            port("id"),
            Value::known(AppleBundleIdId::parse(id).expect("fake ids always parse")),
        );
        outputs.insert(port("identifier"), Value::known(identifier.clone()));
        outputs.insert(port("name"), Value::known(name.clone()));
        outputs
    }

    fn predicted_outputs(identifier: &AppleBundleIdentifier, name: &AppleBundleIdName) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(
            port("id"),
            Value::unknown(willikins_core::TypeRef::scalar(
                willikins_core::TypeName::parse("AppleBundleIdId").unwrap(),
            )),
        );
        outputs.insert(port("identifier"), Value::known(identifier.clone()));
        outputs.insert(port("name"), Value::known(name.clone()));
        outputs
    }

    fn observe(
        state: &FakeState,
        identifier: &AppleBundleIdentifier,
        name: &AppleBundleIdName,
        platform: &AppleBundleIdPlatform,
    ) -> Observation {
        match state.apple_bundle_ids.get(identifier.as_str()) {
            None => Observation::Absent {
                predicted: Self::predicted_outputs(identifier, name),
            },
            Some(record) if record.platform != platform.to_string() => Observation::Mismatch {
                port: port("platform"),
            },
            Some(record) if record.name != name.as_str() => {
                Observation::Mismatch { port: port("name") }
            }
            Some(record) => Observation::Present(Self::outputs_for(&record.id, identifier, name)),
        }
    }
}

impl Tool for FakeAppstoreBundleIdEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let name: AppleBundleIdName = get(inputs, "name")?;
        let platform: AppleBundleIdPlatform = get(inputs, "platform")?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, identifier.as_str());
        Ok(Self::observe(&state, &identifier, &name, &platform))
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let identifier: AppleBundleIdentifier = get(inputs, "identifier")?;
        let name: AppleBundleIdName = get(inputs, "name")?;
        let platform: AppleBundleIdPlatform = get(inputs, "platform")?;
        let mut state = self.state.lock().unwrap();
        state.record_ensure_call(Self::TOOL_NAME, identifier.as_str());
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, identifier.as_str()) {
            return Err(err);
        }
        match Self::observe(&state, &identifier, &name, &platform) {
            Observation::Mismatch { port: mismatched } if mismatched == port("platform") => {
                Err(conflict(format!(
                    "`{identifier}` already exists, but its `platform` does not match what was \
                     requested and Apple's API cannot change a bundle id's platform once \
                     created; change it by hand in App Store Connect, or pass its current value \
                     instead"
                )))
            }
            Observation::Mismatch { .. } => {
                let id = state
                    .apple_bundle_ids
                    .get(identifier.as_str())
                    .expect("a Mismatch observation always carries a matched record")
                    .id
                    .clone();
                state.apple_bundle_ids.insert(
                    identifier.as_str().to_string(),
                    AppleBundleIdRecord {
                        id: id.clone(),
                        name: name.as_str().to_string(),
                        platform: platform.to_string(),
                    },
                );
                Ok(Ensured {
                    outputs: Self::outputs_for(&id, &identifier, &name),
                    changed: true,
                })
            }
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Absent { .. } => {
                let id = fake_apple_bundle_id_id(identifier.as_str());
                state.apple_bundle_ids.insert(
                    identifier.as_str().to_string(),
                    AppleBundleIdRecord {
                        id: id.clone(),
                        name: name.as_str().to_string(),
                        platform: platform.to_string(),
                    },
                );
                Ok(Ensured {
                    outputs: Self::outputs_for(&id, &identifier, &name),
                    changed: true,
                })
            }
            Observation::Foreign => unreachable!("this fake's own observe never returns Foreign"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;

    fn identifier() -> AppleBundleIdentifier {
        AppleBundleIdentifier::parse("com.example.MyApp").unwrap()
    }

    fn name() -> AppleBundleIdName {
        AppleBundleIdName::parse("third-thoughts").unwrap()
    }

    fn platform() -> AppleBundleIdPlatform {
        AppleBundleIdPlatform::parse("UNIVERSAL").unwrap()
    }

    fn inputs() -> Inputs {
        use willikins_types::{AppleIssuerId, AppleKeyId, AppleSigningKey};
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
        inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
        inputs.insert(
            PortName::parse("platform").unwrap(),
            Value::known(platform()),
        );
        inputs
    }

    fn tool() -> FakeAppstoreBundleIdEnsure {
        FakeAppstoreBundleIdEnsure::new(Arc::new(Mutex::new(FakeState::new())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_empty() {
        assert!(matches!(
            tool().read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_then_read_gives_present_and_changed_then_unchanged() {
        let tool = tool();
        let token = SinkToken::new();
        let first = tool.ensure(&inputs(), &token).unwrap();
        assert!(first.changed);
        let second = tool.ensure(&inputs(), &token).unwrap();
        assert!(!second.changed);
        assert!(matches!(
            tool.read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_converges_a_name_mismatch() {
        let state = Arc::new(Mutex::new(FakeState::new().with_apple_bundle_id(
            &identifier(),
            &AppleBundleIdName::parse("old-name").unwrap(),
            &platform(),
        )));
        let tool = FakeAppstoreBundleIdEnsure::new(state);
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(), &token).unwrap();
        assert!(ensured.changed);
        let observation = tool.read(&inputs()).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_refuses_a_platform_mismatch() {
        let state = Arc::new(Mutex::new(FakeState::new().with_apple_bundle_id(
            &identifier(),
            &name(),
            &AppleBundleIdPlatform::parse("IOS").unwrap(),
        )));
        let tool = FakeAppstoreBundleIdEnsure::new(state);
        let token = SinkToken::new();
        let err = tool.ensure(&inputs(), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
    }
}
