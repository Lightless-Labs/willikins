//! `appstore.certificate.get`'s refusals name the certificate *type* and
//! what is wrong, never the serial number they were asked about, nor any
//! certificate's id. Found by the milestone 3c task-3 adversarial pass
//! (`docs/research/2026-09-22-m3c-adversarial-pass.md`).
//!
//! The serial is a non-secret input, but it is the operator's: trust
//! boundary 8 keeps a certificate's serial out of every log, commit and
//! verdict, and a `ToolError`'s message travels further than an input
//! does -- into an agent's transcript, a panic in a live harness, a
//! journal's failure record. The document that asked already holds the
//! serial; the refusal adds nothing by repeating it.

use willikins_core::{PortName, Tool, ToolErrorKind, Value};
use willikins_providers_appstore::AppstoreCertificateGet;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_types::{
    AppleCertificateSerial, AppleCertificateType, AppleIssuerId, AppleKeyId, AppleSigningKey,
    DomainType,
};

/// The serial every fixture but the two below is keyed on.
const SERIAL: &str = "7B3F2A9C1D4E5F607182930A1B2C3D4E";
/// `certificate_list_expired.json`'s one row.
const EXPIRED_SERIAL: &str = "AA11BB22CC33DD44EE55FF6607182930A";
/// `certificate_list_deactivated.json`'s one row.
const DEACTIVATED_SERIAL: &str = "AA11BB22CC33DD44EE55FF6607182930B";

fn inputs(serial: &str) -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    let mut put = |name: &str, value: Value| {
        inputs.insert(PortName::parse(name).unwrap(), value);
    };
    put(
        "issuer_id",
        Value::known(AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap()),
    );
    put(
        "key_id",
        Value::known(AppleKeyId::parse("2X9R4HXF34").unwrap()),
    );
    put(
        "key",
        Value::known(AppleSigningKey::parse(AppleSigningKey::example()).unwrap()),
    );
    put(
        "certificate_type",
        Value::known(AppleCertificateType::parse("DISTRIBUTION").unwrap()),
    );
    put(
        "serial_number",
        Value::known(AppleCertificateSerial::parse(serial).unwrap()),
    );
    inputs
}

fn refusal_for(fixture: &str, serial: &str) -> willikins_core::ToolError {
    let mut provider = MockProvider::start();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    provider
        .mock("GET", "/v1/certificates")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(load_fixture(&dir, "appstore", fixture).to_string())
        .create();
    AppstoreCertificateGet::new(provider.url())
        .read(&inputs(serial))
        .expect_err("this fixture is a refusal")
}

#[test]
fn no_refusal_repeats_the_serial_or_names_a_certificate_id() {
    for (fixture, serial, kind) in [
        ("certificate_list_empty", SERIAL, ToolErrorKind::NotFound),
        (
            "certificate_list_substring_neighbor",
            SERIAL,
            ToolErrorKind::NotFound,
        ),
        ("certificate_list_two", SERIAL, ToolErrorKind::Conflict),
        (
            "certificate_list_expired",
            EXPIRED_SERIAL,
            ToolErrorKind::Conflict,
        ),
        (
            "certificate_list_deactivated",
            DEACTIVATED_SERIAL,
            ToolErrorKind::Conflict,
        ),
    ] {
        let err = refusal_for(fixture, serial);
        assert_eq!(err.kind, kind, "{fixture}: {}", err.message);
        assert!(
            !err.message.contains(serial),
            "{fixture}: the refusal repeats the serial: {}",
            err.message
        );
        assert!(
            !err.message.contains("C3RT1F1CATE"),
            "{fixture}: the refusal names a certificate id: {}",
            err.message
        );
        assert!(
            err.message.contains("DISTRIBUTION"),
            "{fixture}: the refusal should still name the type: {}",
            err.message
        );
    }
}
