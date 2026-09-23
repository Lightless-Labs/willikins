//! `appstore.profile.ensure`: creates (in memory) an App Store Connect
//! provisioning profile. Mirrors
//! `willikins_providers_appstore::tools::AppstoreProfileEnsure`'s
//! `ToolSpec` exactly (`tests/catalog_parity.rs`, in
//! `willikins-providers-appstore`, pins the two equal) and the same
//! observations and check order: `Absent`, `Mismatch { profile_type }`,
//! `Mismatch { certificate }`, `Conflict` (`INVALID` or expired),
//! `Present` -- no `Foreign`, for the identical reason the live tool's
//! own module doc gives.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    AppleBundleIdentifier, AppleCertificateId, AppleProfileContent, AppleProfileId,
    AppleProfileName, AppleProfileType, DomainType,
};

use crate::state::{AppleProfileRecord, FakeState, apple_profile_key};
use crate::support::{conflict, exact, get, not_found, port, require_present, scalar, tool_name};

/// `appstore.profile.ensure`.
pub struct FakeAppstoreProfileEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeAppstoreProfileEnsure {
    const TOOL_NAME: &'static str = "appstore.profile.ensure";

    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(port("identifier"), exact("AppleBundleIdentifier", true));
        inputs.insert(port("name"), exact("AppleProfileName", true));
        inputs.insert(port("profile_type"), exact("AppleProfileType", true));
        inputs.insert(port("certificate"), exact("AppleCertificateId", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("profile"), scalar("AppleProfileId"));
        outputs.insert(port("content"), scalar("AppleProfileContent"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Produce (or report) an App Store Connect IOS_APP_STORE \
                               provisioning profile relating a bundle identifier to a \
                               distribution certificate."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("identifier"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn predicted_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(
            port("profile"),
            Value::unknown(willikins_core::TypeRef::scalar(
                willikins_core::TypeName::parse("AppleProfileId").unwrap(),
            )),
        );
        outputs.insert(
            port("content"),
            Value::unknown(willikins_core::TypeRef::scalar(
                willikins_core::TypeName::parse("AppleProfileContent").unwrap(),
            )),
        );
        outputs
    }

    fn outputs_for(record: &AppleProfileRecord) -> Result<Outputs, ToolError> {
        let profile = AppleProfileId::parse(&record.id).map_err(|err| ToolError {
            kind: willikins_core::ToolErrorKind::Provider,
            message: format!("fake profile record has a malformed id: {err}"),
        })?;
        let content = AppleProfileContent::parse(&record.content).map_err(|err| ToolError {
            kind: willikins_core::ToolErrorKind::Provider,
            message: format!("fake profile record has malformed content: {err}"),
        })?;
        let mut outputs = Outputs::new();
        outputs.insert(port("profile"), Value::known(profile));
        outputs.insert(port("content"), Value::known(content));
        Ok(outputs)
    }

    /// The full read, mirroring the live tool's `observe`: identifier
    /// resolution, then the record lookup, then the checks in decision
    /// (d)/(e)'s order.
    fn observe(
        state: &FakeState,
        identifier: &AppleBundleIdentifier,
        name: &AppleProfileName,
        profile_type: &AppleProfileType,
        certificate: &AppleCertificateId,
    ) -> Result<Observation, ToolError> {
        if !state.apple_bundle_ids.contains_key(identifier.as_str()) {
            return Ok(Observation::Absent {
                predicted: Self::predicted_outputs(),
            });
        }
        let records = state
            .apple_profiles
            .get(&apple_profile_key(identifier, name))
            .cloned()
            .unwrap_or_default();
        match records.len() {
            0 => Ok(Observation::Absent {
                predicted: Self::predicted_outputs(),
            }),
            1 => {
                let record = &records[0];
                if record.profile_type != profile_type.as_str() {
                    return Ok(Observation::Mismatch {
                        port: port("profile_type"),
                    });
                }
                if record.certificate_id != certificate.as_str() {
                    return Ok(Observation::Mismatch {
                        port: port("certificate"),
                    });
                }
                if record.profile_state == "INVALID" {
                    return Err(conflict(
                        "the profile exists but Apple reports it INVALID; willikins cannot \
                         repair a profile, replace it instead"
                            .to_string(),
                    ));
                }
                if record.expired {
                    return Err(conflict(
                        "the profile is expired; willikins cannot repair a profile, replace \
                         it instead"
                            .to_string(),
                    ));
                }
                Ok(Observation::Present(Self::outputs_for(record)?))
            }
            count => Err(conflict(format!(
                "{count} profiles named `{name}` already exist on this bundle id; this tool \
                 cannot disambiguate"
            ))),
        }
    }
}

fn inputs_of(
    inputs: &Inputs,
) -> Result<
    (
        AppleBundleIdentifier,
        AppleProfileName,
        AppleProfileType,
        AppleCertificateId,
    ),
    ToolError,
> {
    Ok((
        get(inputs, "identifier")?,
        get(inputs, "name")?,
        get(inputs, "profile_type")?,
        get(inputs, "certificate")?,
    ))
}

impl Tool for FakeAppstoreProfileEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let (identifier, name, profile_type, certificate) = inputs_of(inputs)?;
        let mut state = self.state.lock().unwrap();
        state.record_read_call(Self::TOOL_NAME, &apple_profile_key(&identifier, &name));
        Self::observe(&state, &identifier, &name, &profile_type, &certificate)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let (identifier, name, profile_type, certificate) = inputs_of(inputs)?;
        let mut state = self.state.lock().unwrap();
        let key = apple_profile_key(&identifier, &name);
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        match Self::observe(&state, &identifier, &name, &profile_type, &certificate)? {
            Observation::Present(outputs) => Ok(Ensured {
                outputs,
                changed: false,
            }),
            Observation::Mismatch { .. } => Err(conflict(
                "the existing profile does not match what was requested, and cannot be \
                 converged (there is no update operation for a profile); replace it instead"
                    .to_string(),
            )),
            Observation::Foreign => unreachable!("this fake's own observe never returns Foreign"),
            Observation::Absent { .. } => {
                if !state.apple_bundle_ids.contains_key(identifier.as_str()) {
                    return Err(not_found(format!(
                        "bundle identifier `{identifier}` is not registered; register it \
                         (for example via appstore.bundle_id.ensure) before creating a \
                         profile for it"
                    )));
                }
                let id = format!("FAKEPR0F{key:0>8}", key = records_seen(&state) + 1);
                let content = format!("fakeprofilecontent{id}==");
                let record = AppleProfileRecord {
                    id: id.clone(),
                    certificate_id: certificate.as_str().to_string(),
                    profile_type: profile_type.as_str().to_string(),
                    profile_state: "ACTIVE".to_string(),
                    expired: false,
                    content,
                };
                let outputs = Self::outputs_for(&record)?;
                state.apple_profiles.entry(key).or_default().push(record);
                Ok(Ensured {
                    outputs,
                    changed: true,
                })
            }
        }
    }
}

/// A monotonically increasing count of every profile record seeded or
/// created so far, used only to make freshly created fake profile ids
/// distinct from one another within one process -- not a claim about the
/// live provider's own id generation (which this fake never claims to
/// predict; contrast `fake_apple_bundle_id_id`, which *is* deterministic
/// from the identifier alone because the live bundle id tool's own key
/// is the identifier, not an ambient count).
fn records_seen(state: &FakeState) -> usize {
    state.apple_profiles.values().map(Vec::len).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::{AppleIssuerId, AppleKeyId, AppleSigningKey};

    fn identifier() -> AppleBundleIdentifier {
        AppleBundleIdentifier::parse("com.example.MyApp").unwrap()
    }

    fn name() -> AppleProfileName {
        AppleProfileName::parse("willikins-example-profile").unwrap()
    }

    fn profile_type() -> AppleProfileType {
        AppleProfileType::parse("IOS_APP_STORE").unwrap()
    }

    fn certificate() -> AppleCertificateId {
        AppleCertificateId::parse("C3RT1F1CATE1").unwrap()
    }

    fn inputs() -> Inputs {
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
            PortName::parse("profile_type").unwrap(),
            Value::known(profile_type()),
        );
        inputs.insert(
            PortName::parse("certificate").unwrap(),
            Value::known(certificate()),
        );
        inputs
    }

    fn tool_with(state: FakeState) -> FakeAppstoreProfileEnsure {
        FakeAppstoreProfileEnsure::new(Arc::new(Mutex::new(state)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool_with(FakeState::new())
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn read_reports_absent_when_the_identifier_is_not_registered() {
        assert!(matches!(
            tool_with(FakeState::new()).read(&inputs()).unwrap(),
            Observation::Absent { .. }
        ));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_refuses_to_create_when_the_identifier_is_not_registered() {
        let tool = tool_with(FakeState::new());
        let token = SinkToken::new();
        let err = tool.ensure(&inputs(), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::NotFound);
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn ensure_then_read_gives_present_and_changed_then_unchanged() {
        let identifier_platform =
            willikins_types::AppleBundleIdPlatform::parse("UNIVERSAL").unwrap();
        let identifier_name = willikins_types::AppleBundleIdName::parse("example").unwrap();
        let state = FakeState::new().with_apple_bundle_id(
            &identifier(),
            &identifier_name,
            &identifier_platform,
        );
        let tool = tool_with(state);
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
}
