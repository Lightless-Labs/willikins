//! `appstore.certificate.get`: resolves a certificate type + serial
//! number to its id, against seeded state. Mirrors
//! `willikins_providers_appstore::tools::AppstoreCertificateGet`'s
//! `ToolSpec` exactly (`tests/catalog_parity.rs`, in
//! `willikins-providers-appstore`, pins the two equal) and the same
//! outcomes: zero matches, one healthy match, one expired or deactivated
//! match, or two or more matches.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec, Value,
};
use willikins_types::{
    AppleCertificateId, AppleCertificateSerial, AppleCertificateType, DomainType,
};

use crate::state::{FakeState, apple_certificate_key};
use crate::support::{conflict, exact, get, not_found, port, require_present, scalar, tool_name};

/// `appstore.certificate.get`.
pub struct FakeAppstoreCertificateGet {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FakeAppstoreCertificateGet {
    /// Build the tool against `state`, constructing its spec.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("issuer_id"), exact("AppleIssuerId", true));
        inputs.insert(port("key_id"), exact("AppleKeyId", true));
        inputs.insert(port("key"), exact("AppleSigningKey", true));
        inputs.insert(
            port("certificate_type"),
            exact("AppleCertificateType", true),
        );
        inputs.insert(port("serial_number"), exact("AppleCertificateSerial", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("certificate"), scalar("AppleCertificateId"));
        Self {
            spec: ToolSpec {
                name: tool_name("appstore.certificate.get"),
                description: "Select an App Store Connect distribution certificate by type and \
                               serial number."
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

    fn lookup(&self, inputs: &Inputs) -> Result<Outputs, ToolError> {
        require_present(&self.spec, inputs)?;
        // `issuer_id`/`key_id`/`key` are this tool's real-provider
        // credential ports (a credential reaches one team), but this
        // fake's state is not itself partitioned by team -- exactly like
        // every other fake tool's single-workplace state.
        let _issuer_id: willikins_types::AppleIssuerId = get(inputs, "issuer_id")?;
        let _key_id: willikins_types::AppleKeyId = get(inputs, "key_id")?;
        let _key: willikins_types::AppleSigningKey = get(inputs, "key")?;
        let certificate_type: AppleCertificateType = get(inputs, "certificate_type")?;
        let serial_number: AppleCertificateSerial = get(inputs, "serial_number")?;
        let state = self.state.lock().unwrap();
        let records = state
            .apple_certificates
            .get(&apple_certificate_key(&certificate_type, &serial_number))
            .cloned()
            .unwrap_or_default();
        match records.len() {
            0 => Err(not_found(format!(
                "no {certificate_type} certificate has serial `{serial_number}`"
            ))),
            1 => {
                let record = &records[0];
                if record.expired {
                    return Err(conflict(format!(
                        "the {certificate_type} certificate with serial `{serial_number}` is \
                         expired"
                    )));
                }
                if record.activated == Some(false) {
                    return Err(conflict(format!(
                        "the {certificate_type} certificate with serial `{serial_number}` is \
                         deactivated"
                    )));
                }
                let certificate =
                    AppleCertificateId::parse(&record.id).map_err(|err| ToolError {
                        kind: willikins_core::ToolErrorKind::Provider,
                        message: format!("certificate `{serial_number}` has a malformed id: {err}"),
                    })?;
                let mut outputs = Outputs::new();
                outputs.insert(port("certificate"), Value::known(certificate));
                Ok(outputs)
            }
            count => Err(conflict(format!(
                "{count} {certificate_type} certificates have serial `{serial_number}`; this \
                 tool cannot disambiguate"
            ))),
        }
    }
}

impl Tool for FakeAppstoreCertificateGet {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.lookup(inputs).map(Observation::Present)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.lookup(inputs)?,
            changed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::{AppleIssuerId, AppleKeyId, AppleSigningKey};

    fn certificate_type() -> AppleCertificateType {
        AppleCertificateType::parse("DISTRIBUTION").unwrap()
    }

    fn serial_number() -> AppleCertificateSerial {
        AppleCertificateSerial::parse("7B3F2A9C1D4E5F607182930A1B2C3D4E").unwrap()
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
            PortName::parse("certificate_type").unwrap(),
            Value::known(certificate_type()),
        );
        inputs.insert(
            PortName::parse("serial_number").unwrap(),
            Value::known(serial_number()),
        );
        inputs
    }

    fn tool(state: FakeState) -> FakeAppstoreCertificateGet {
        FakeAppstoreCertificateGet::new(Arc::new(Mutex::new(state)))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool(FakeState::new())
            .spec()
            .validate(willikins_types::registry())
            .unwrap();
    }

    #[test]
    fn spec_has_no_key_and_is_pure() {
        let spec_tool = tool(FakeState::new());
        assert!(spec_tool.spec().key.is_empty());
        assert!(spec_tool.spec().pure);
    }

    #[test]
    fn read_reports_not_found_when_no_certificate_matches() {
        let err = tool(FakeState::new()).read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::NotFound);
    }

    #[test]
    fn read_reports_present_with_the_seeded_id() {
        let state = FakeState::new().with_apple_certificate(
            &certificate_type(),
            &serial_number(),
            "C3RT1F1CATE1",
            false,
            None,
        );
        let observation = tool(state).read(&inputs()).unwrap();
        let Observation::Present(outputs) = observation else {
            panic!("expected Present, got {observation:?}");
        };
        let certificate = outputs
            .get(&PortName::parse("certificate").unwrap())
            .unwrap();
        assert_eq!(certificate.render().to_string(), "C3RT1F1CATE1");
    }

    #[test]
    fn read_reports_conflict_when_expired() {
        let state = FakeState::new().with_apple_certificate(
            &certificate_type(),
            &serial_number(),
            "C3RT1F1CATE1",
            true,
            None,
        );
        let err = tool(state).read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
    }

    #[test]
    fn read_reports_conflict_when_deactivated() {
        let state = FakeState::new().with_apple_certificate(
            &certificate_type(),
            &serial_number(),
            "C3RT1F1CATE1",
            false,
            Some(false),
        );
        let err = tool(state).read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
    }

    #[test]
    fn read_reports_present_when_activated_is_explicitly_true() {
        let state = FakeState::new().with_apple_certificate(
            &certificate_type(),
            &serial_number(),
            "C3RT1F1CATE1",
            false,
            Some(true),
        );
        assert!(matches!(
            tool(state).read(&inputs()).unwrap(),
            Observation::Present(_)
        ));
    }

    #[test]
    fn read_reports_conflict_when_two_certificates_share_type_and_serial() {
        let state = FakeState::new()
            .with_apple_certificate(
                &certificate_type(),
                &serial_number(),
                "C3RT1F1CATE1",
                false,
                None,
            )
            .with_apple_certificate(
                &certificate_type(),
                &serial_number(),
                "C3RT1F1CATE2",
                false,
                None,
            );
        let err = tool(state).read(&inputs()).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(err.message.contains('2'));
        assert!(!err.message.contains("C3RT1F1CATE1"));
        assert!(!err.message.contains("C3RT1F1CATE2"));
    }

    #[test]
    fn ensure_is_the_identity_of_read() {
        let state = FakeState::new().with_apple_certificate(
            &certificate_type(),
            &serial_number(),
            "C3RT1F1CATE1",
            false,
            None,
        );
        #[allow(clippy::disallowed_methods)]
        let token = SinkToken::new();
        let ensured = tool(state).ensure(&inputs(), &token).unwrap();
        assert!(!ensured.changed);
        let certificate = ensured
            .outputs
            .get(&PortName::parse("certificate").unwrap())
            .unwrap();
        assert_eq!(certificate.render().to_string(), "C3RT1F1CATE1");
    }
}
