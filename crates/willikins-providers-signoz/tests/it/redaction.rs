//! A credential marker reaches no error, no `Debug`, no
//! `Observation`/`Ensured` rendering, and no request header but
//! `SigNoz-Api-Key` — never `Authorization`, which is the one fact that
//! makes this crate's redaction test structurally different from every
//! sibling provider crate's. No authored fixture carries the marker
//! either. A minted key's own value (never a credential, but a secret
//! this tool must equally never leak into an error or a log) gets the
//! same treatment in `ingestion_key_ensure_mock.rs`'s
//! `read_reports_present_with_an_unknown_key_when_listed`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use willikins_core::{PortName, SinkToken, Tool, Value};
use willikins_providers_http::Credential;
use willikins_providers_http::testing::MockProvider;
use willikins_providers_signoz::{SigNozClient, SigNozIngestionKeyEnsure};
use willikins_types::{DomainType, SigNozIngestionKeyName};

/// Stands in for a real `SigNoz` API key: shaped to satisfy
/// `CREDENTIAL_PATTERN` so nothing rejects it before the point this test
/// cares about, distinctive enough that its presence anywhere but the
/// `SigNoz-Api-Key` header is unambiguously a bug.
const CREDENTIAL_MARKER: &str = "wlknCredentialMarker00000000000000000000";

#[test]
fn no_authored_fixture_carries_a_credential_shaped_literal() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/signoz");
    for entry in std::fs::read_dir(&fixtures_dir).expect("fixtures/signoz/ is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() || path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        assert!(
            !text.contains(CREDENTIAL_MARKER),
            "{} carries the credential marker",
            path.display()
        );
    }
}

fn client_against(url: String) -> Arc<SigNozClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_SIGNOZ_API_KEY", CREDENTIAL_MARKER);
    Arc::new(SigNozClient::new(willikins_providers_signoz::http_client(
        url, credential,
    )))
}

fn name() -> SigNozIngestionKeyName {
    SigNozIngestionKeyName::parse("willikins-example-key").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs
}

/// The credential marker reaches no part of a recorded observation,
/// ensured result, or error across a full read-then-ensure and a failing
/// read.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own credential
fn a_marker_credential_reaches_no_observation_ensured_or_error() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/api/v2/gateway/ingestion_keys")
        .with_status(200)
        .with_body(r#"{"status":"success","data":{"keys":[],"_pagination":{}}}"#)
        .create();
    let client = client_against(provider.url());
    let observation = SigNozIngestionKeyEnsure::new(client.clone())
        .read(&inputs())
        .expect("reads");

    let mut failing = MockProvider::start();
    failing
        .mock("GET", "/api/v2/gateway/ingestion_keys")
        .with_status(500)
        .with_body(r#"{"error":{"message":"boom"}}"#)
        .create();
    let failing_client = client_against(failing.url());
    let err = SigNozIngestionKeyEnsure::new(failing_client)
        .read(&inputs())
        .expect_err("500");

    for text in [
        format!("{observation:?}"),
        format!("{err:?}"),
        err.message.clone(),
    ] {
        assert!(
            !text.contains(CREDENTIAL_MARKER),
            "the credential marker leaked into: {text}"
        );
    }
}

/// The marker must reach the `SigNoz-Api-Key` header on every request
/// this crate makes, and — critically, the one way this differs from
/// every sibling provider crate — never `Authorization`, and no other
/// header either.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own credential
fn every_request_carries_the_marker_in_signoz_api_key_and_never_in_authorization() {
    let mut provider = MockProvider::start();
    let captured = Arc::new(Mutex::new(Vec::<(String, String)>::new()));

    provider
        .mock("GET", "/api/v2/gateway/ingestion_keys")
        .with_status(200)
        .with_body_from_request({
            let capture = captured.clone();
            move |request| {
                let mut seen = capture.lock().expect("not poisoned");
                for (header_name, value) in request.headers() {
                    seen.push((
                        header_name.to_string(),
                        value.to_str().unwrap_or("<non-utf8>").to_string(),
                    ));
                }
                r#"{"status":"success","data":{"keys":[],"_pagination":{}}}"#
                    .as_bytes()
                    .to_vec()
            }
        })
        .create();

    let client = client_against(provider.url());
    let observation = SigNozIngestionKeyEnsure::new(client)
        .read(&inputs())
        .expect("reads");
    assert!(matches!(
        observation,
        willikins_core::Observation::Absent { .. }
    ));

    let captured = captured.lock().expect("not poisoned").clone();
    let mut carried_the_right_header = false;
    let mut saw_authorization = false;
    for (header_name, value) in &captured {
        if header_name.eq_ignore_ascii_case("signoz-api-key") {
            assert!(
                value.contains(CREDENTIAL_MARKER),
                "the SigNoz-Api-Key header must carry the credential: {value}"
            );
            carried_the_right_header = true;
        } else {
            if header_name.eq_ignore_ascii_case("authorization") {
                saw_authorization = true;
            }
            assert!(
                !value.contains(CREDENTIAL_MARKER),
                "header `{header_name}` leaked the credential marker: {value}"
            );
        }
    }
    assert!(
        carried_the_right_header,
        "the request must have carried a SigNoz-Api-Key header"
    );
    assert!(
        !saw_authorization,
        "SigNoz's own scheme is SigNoz-Api-Key; an Authorization header on this request \
         would mean the generalised header method was not used"
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_never_makes_or_needs_a_sink_token_to_read_the_value_back() {
    // A structural check, not a live one: `read`'s and the "already
    // present" arm of `ensure`'s outputs are always `Unknown`, which the
    // type system refuses to let anything call `.expose()` on -- there is
    // no `SinkToken` anywhere in `read`'s signature at all, and `ensure`'s
    // token is never read from the outputs it returns on that arm.
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/api/v2/gateway/ingestion_keys")
        .with_status(200)
        .with_body(
            r#"{"status":"success","data":{"keys":[{"id":"018e5a22-0000-7000-a000-000000000001","name":"willikins-example-key"}],"_pagination":{}}}"#,
        )
        .create();
    let client = client_against(provider.url());
    let tool = SigNozIngestionKeyEnsure::new(client);
    let sink = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &sink).expect("ensures");
    assert!(!ensured.changed);
    let key = ensured
        .outputs
        .get(&PortName::parse("key").unwrap())
        .unwrap();
    assert!(!key.is_known(), "an already-present key must stay Unknown");
}
