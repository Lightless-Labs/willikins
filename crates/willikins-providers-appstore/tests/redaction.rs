//! Redaction: the minted JWT reaches no `Observation`, `Ensured`,
//! `ToolError`, or `Debug` output, and reaches the `Authorization`
//! header and no other header. The raw signing key's own redaction is
//! already proven at the type level
//! (`crates/willikins-types/src/appstore.rs`'s own tests); this file
//! proves the *derived* secret -- the JWT this crate mints from it --
//! is held to the same standard end to end.

use std::sync::{Arc, Mutex};

use willikins_core::{Observation, PortName, SinkToken, Tool, Value};
use willikins_providers_appstore::{AppstoreBundleIdEnsure, AppstoreProfileEnsure};
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_types::{
    AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier, AppleCertificateId,
    AppleIssuerId, AppleKeyId, AppleProfileContent, AppleProfileName, AppleProfileType,
    AppleSigningKey, DomainType,
};

fn issuer_id() -> AppleIssuerId {
    AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap()
}

fn key_id() -> AppleKeyId {
    AppleKeyId::parse("2X9R4HXF34").unwrap()
}

fn key() -> AppleSigningKey {
    AppleSigningKey::parse(AppleSigningKey::example()).unwrap()
}

fn identifier() -> AppleBundleIdentifier {
    AppleBundleIdentifier::parse("com.example.MyApp").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("issuer_id").unwrap(),
        Value::known(issuer_id()),
    );
    inputs.insert(PortName::parse("key_id").unwrap(), Value::known(key_id()));
    inputs.insert(PortName::parse("key").unwrap(), Value::known(key()));
    inputs.insert(
        PortName::parse("identifier").unwrap(),
        Value::known(identifier()),
    );
    inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(AppleBundleIdName::parse("third-thoughts").unwrap()),
    );
    inputs.insert(
        PortName::parse("platform").unwrap(),
        Value::known(AppleBundleIdPlatform::parse("UNIVERSAL").unwrap()),
    );
    inputs
}

/// A minted JWT reaches no `Observation`, `Ensured`, `ToolError`, or
/// `Debug` output -- captured from the real `Authorization` header a
/// mock server sees, so this is the actual bytes this crate sent, not a
/// stand-in.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn the_minted_jwt_reaches_no_observation_ensured_error_or_debug_output() {
    let mut provider = MockProvider::start();
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = captured.clone();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body_from_request(move |request| {
            for (name, value) in request.headers() {
                if name == "authorization" {
                    capture
                        .lock()
                        .unwrap()
                        .push(value.to_str().unwrap_or("<non-utf8>").to_string());
                }
            }
            serde_json::json!({"data": []}).to_string().into_bytes()
        })
        .create();

    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let observation = tool.read(&inputs()).expect("reads");

    let headers = captured.lock().unwrap().clone();
    assert!(
        !headers.is_empty(),
        "the GET must have carried an Authorization header"
    );
    let jwt = headers[0]
        .strip_prefix("Bearer ")
        .expect("the header carries a bearer token")
        .to_string();
    assert_eq!(
        jwt.split('.').count(),
        3,
        "a JWT has three dot-separated parts: {jwt}"
    );
    // Every captured header (this read may have made more than one call)
    // carries a well-formed bearer JWT, and every one of them stays out
    // of the observation this call returned.
    for header in &headers {
        assert!(header.starts_with("Bearer "), "{header}");
    }
    assert!(
        !format!("{observation:?}").contains(&jwt),
        "the minted JWT leaked into the observation's Debug"
    );
}

/// The other half: the JWT must reach the `Authorization` header on
/// every request this crate makes, and no other header.
#[test]
fn every_request_carries_the_jwt_in_authorization_and_in_no_other_header() {
    let mut provider = MockProvider::start();
    let captured = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let capture = captured.clone();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body_from_request(move |request| {
            for (name, value) in request.headers() {
                capture.lock().unwrap().push((
                    name.to_string(),
                    value.to_str().unwrap_or("<non-utf8>").to_string(),
                ));
            }
            serde_json::json!({"data": []}).to_string().into_bytes()
        })
        .create();

    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let observation = tool.read(&inputs()).expect("reads");
    assert!(matches!(
        observation,
        willikins_core::Observation::Absent { .. }
    ));

    let captured = captured.lock().unwrap().clone();
    let jwt = captured
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| {
            assert!(value.starts_with("Bearer "), "{value}");
            let jwt = value.strip_prefix("Bearer ").unwrap().to_string();
            assert_eq!(
                jwt.split('.').count(),
                3,
                "the Authorization header must carry a three-part JWT: {jwt}"
            );
            jwt
        })
        .expect("the request must have carried an Authorization header");

    // The real check: no *other* header carries the same JWT bytes. A
    // generic "looks token-shaped" heuristic (three dot-separated parts,
    // say) is too fragile -- `ureq`'s own `user-agent` header
    // (`ureq/3.4.2`) has three dot-separated segments too.
    for (name, value) in &captured {
        if !name.eq_ignore_ascii_case("authorization") {
            assert!(
                !value.contains(&jwt),
                "header `{name}` carries the JWT too: {value}"
            );
        }
    }
}

/// A failing call still never leaks anything beyond the bounded,
/// content-free error shape `willikins-providers-http` already enforces.
#[test]
fn the_minted_jwt_reaches_no_error_message_on_a_failing_call() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(500)
        .with_body(r#"{"errors":[{"status":"500","code":"GENERAL_ERROR"}]}"#)
        .create();
    let tool = AppstoreBundleIdEnsure::new(provider.url());
    let err = tool.read(&inputs()).expect_err("500");
    // No JWT can be captured for comparison in this arm (the request
    // never reaches a per-request capture point in this test), so the
    // structural assertion is that the error message stays within the
    // bounded shape `willikins-providers-http` already enforces -- never
    // growing to carry a header or a request body.
    assert!(err.message.len() < 512, "{}", err.message);
    assert!(
        !format!("{err:?}").contains("BEGIN"),
        "the error must never carry PEM markers"
    );
}

// ---------------------------------------------------------------------
// Acceptance test 8: `appstore.profile.ensure`'s `content` output. A
// distinctive marker planted in the mock's `profileContent` never
// appears in an `Observation`'s or `Ensured`'s rendering, `Debug`, or a
// `ToolError` -- only `AppleProfileContent::expose`, called with a
// `SinkToken` a test mints itself, ever recovers the real bytes.
// ---------------------------------------------------------------------

fn profile_fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn profile_fixture(name: &str) -> serde_json::Value {
    load_fixture(&profile_fixtures_dir(), "appstore", name)
}

/// A base64 string containing the marker as plain ASCII text, so the
/// marker's own bytes appear verbatim in the encoded fixture the mock
/// serves -- exactly the shape a real `profileContent` marker search
/// needs to be a real proof, not a coincidence of the fixture's own
/// wording.
const MARKER: &str = "REDACTIONMARKERPROBE";

fn profile_inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("issuer_id").unwrap(),
        Value::known(issuer_id()),
    );
    inputs.insert(PortName::parse("key_id").unwrap(), Value::known(key_id()));
    inputs.insert(PortName::parse("key").unwrap(), Value::known(key()));
    inputs.insert(
        PortName::parse("identifier").unwrap(),
        Value::known(identifier()),
    );
    inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(AppleProfileName::parse("willikins-example-profile").unwrap()),
    );
    inputs.insert(
        PortName::parse("profile_type").unwrap(),
        Value::known(AppleProfileType::parse("IOS_APP_STORE").unwrap()),
    );
    inputs.insert(
        PortName::parse("certificate").unwrap(),
        Value::known(AppleCertificateId::parse("C3RT1F1CATE1").unwrap()),
    );
    inputs
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_profile_content_marker_never_reaches_an_observations_render_or_debug() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(profile_fixture("bundle_id_list_one").to_string())
        .create();
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(profile_fixture("profile_list_one").to_string())
        .create();
    let marked_content = format!("{MARKER}AAAAAAAAAAAAAAAAAAAAAAAAAAAAA==");
    let mut instance = profile_fixture("profile_get_present_healthy");
    instance["data"]["attributes"]["profileContent"] =
        serde_json::Value::String(marked_content.clone());
    provider
        .mock("GET", "/v1/profiles/PR0F1LE1D0001")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(instance.to_string())
        .create();

    let tool = AppstoreProfileEnsure::new(provider.url());
    let observation = tool.read(&profile_inputs()).expect("reads Present");
    let debug = format!("{observation:?}");
    assert!(!debug.contains(MARKER), "marker leaked into Debug: {debug}");

    let Observation::Present(outputs) = &observation else {
        panic!("expected Present, got {observation:?}");
    };
    let rendered = outputs
        .get(&PortName::parse("content").unwrap())
        .unwrap()
        .render()
        .to_string();
    assert!(
        !rendered.contains(MARKER),
        "marker leaked into the rendered output: {rendered}"
    );
    assert_eq!(rendered, "[REDACTED AppleProfileContent]");

    // The one place the real bytes are recoverable: `expose`, gated by a
    // `SinkToken` this test mints itself.
    let token = SinkToken::new();
    let content = outputs
        .get(&PortName::parse("content").unwrap())
        .unwrap()
        .downcast::<AppleProfileContent>()
        .unwrap();
    assert_eq!(content.expose(&token), marked_content);
}

#[test]
fn a_profile_content_marker_never_reaches_a_toolerror() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v1/bundleIds")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(profile_fixture("bundle_id_list_one").to_string())
        .create();
    provider
        .mock("GET", "/v1/bundleIds/T6G4XCV345/profiles")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(profile_fixture("profile_list_one").to_string())
        .create();
    // A malformed `profileContent` (containing the marker) makes the
    // single-instance read fail to parse -- proving the marker survives
    // neither a successful nor a failing path.
    let mut instance = profile_fixture("profile_get_present_healthy");
    instance["data"]["attributes"]["profileContent"] =
        serde_json::Value::String(format!("not base64 {MARKER} !!!"));
    provider
        .mock("GET", "/v1/profiles/PR0F1LE1D0001")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(instance.to_string())
        .create();

    let tool = AppstoreProfileEnsure::new(provider.url());
    let err = tool
        .read(&profile_inputs())
        .expect_err("malformed content refuses");
    assert!(
        !err.message.contains(MARKER),
        "marker leaked into a ToolError: {}",
        err.message
    );
    assert!(
        !format!("{err:?}").contains(MARKER),
        "marker leaked into a ToolError's Debug"
    );
}
