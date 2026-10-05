//! `doppler.secret_name.gate`'s own mock-server tests (milestone 3j, task
//! B2, acceptance 5 and 6): one test per decision (b2) branch, a source
//! guard that this tool (and the client call beneath it) never names a
//! value-reading endpoint, and the names-never-values trust boundary.
//! See `fixtures/doppler/README.md` for how the config fixtures used
//! here were verified.

use std::sync::Arc;

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerSecretNameGate};
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, DopplerConfig, SecretName};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "doppler", name)
}

fn client_against(url: String) -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new(url, Vec::new(), credential);
    Arc::new(DopplerClient::new(http))
}

fn tool_against(url: String) -> DopplerSecretNameGate {
    DopplerSecretNameGate::new(client_against(url))
}

/// The deployment branch config this file's rows all ask about.
fn config() -> DopplerConfig {
    DopplerConfig::parse("third-thoughts/prd").unwrap()
}

/// The milestone's placeholder secret name (SHARED VALUES).
fn name() -> SecretName {
    SecretName::parse("EXAMPLE_APNS_KEY").unwrap()
}

fn inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs
}

/// The exact `secrets/names` path [`willikins_providers_doppler::DopplerClient::secret_name_listed`]
/// sends for `project`/`config`, pinned literally (SHARED VALUES).
fn names_path(project: &str, config_name: &str) -> String {
    format!(
        "/v3/configs/config/secrets/names?project={project}&config={config_name}&include_dynamic_secrets=false&include_managed_secrets=false"
    )
}

fn config_path(project: &str, config_name: &str) -> String {
    format!("/v3/configs/config?project={project}&config={config_name}")
}

fn names_body(names: &[&str]) -> String {
    serde_json::json!({ "names": names }).to_string()
}

fn assert_config_output(outputs: &willikins_core::Outputs) {
    let out = outputs.get(&PortName::parse("config").unwrap()).unwrap();
    assert_eq!(out.render().to_string(), "third-thoughts/prd");
}

#[test]
fn listed_directly_reads_present_without_reading_the_config() {
    let mut provider = MockProvider::start();
    let names_mock = provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["A", "EXAMPLE_APNS_KEY"]))
        .expect(1)
        .create();
    // No `GET /v3/configs/config` mock at all: an unexpected request to
    // it would fail with a connection error, not silently succeed.
    let tool = tool_against(provider.url());
    let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
        panic!("expected Present");
    };
    assert_config_output(&outputs);
    names_mock.assert();
}

#[test]
fn unlisted_with_no_inherits_reads_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let Observation::Absent { predicted } = tool.read(&inputs()).unwrap() else {
        panic!("expected Absent");
    };
    assert_config_output(&predicted);
}

/// Acceptance 5's own row literally names `inherits: [shared_keys/prd]`
/// (SHARED VALUES' placeholder base), so this test walks that exact
/// base rather than a generic one — exercising the underscored-project
/// widening (milestone 3j part A) through this gate's own walk and
/// query string, the one place the milestone's two halves meet.
#[test]
fn unlisted_single_base_lists_it_reads_present() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_inherits_underscore").to_string())
        .expect(1)
        .create();
    let base_mock = provider
        .mock("GET", names_path("shared_keys", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["EXAMPLE_APNS_KEY"]))
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
        panic!("expected Present");
    };
    assert_config_output(&outputs);
    base_mock.assert();
}

#[test]
fn unlisted_two_bases_second_lists_it_reads_present_after_reading_both() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_inherits_extra").to_string())
        .expect(1)
        .create();
    let first_base = provider
        .mock("GET", names_path("shared-apple", "base").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    let second_base = provider
        .mock("GET", names_path("shared-apple", "other").as_str())
        .with_status(200)
        .with_body(names_body(&["EXAMPLE_APNS_KEY"]))
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
        panic!("expected Present");
    };
    assert_config_output(&outputs);
    first_base.assert();
    second_base.assert();
}

#[test]
fn unlisted_first_base_lists_it_second_base_is_never_read() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_inherits_extra").to_string())
        .expect(1)
        .create();
    let first_base = provider
        .mock("GET", names_path("shared-apple", "base").as_str())
        .with_status(200)
        .with_body(names_body(&["EXAMPLE_APNS_KEY"]))
        .expect(1)
        .create();
    // The second base must never be asked once the first already
    // answered `true`: `.expect(0)` fails the test if it is.
    let second_base = provider
        .mock("GET", names_path("shared-apple", "other").as_str())
        .with_status(200)
        .with_body(names_body(&["EXAMPLE_APNS_KEY"]))
        .expect(0)
        .create();
    let tool = tool_against(provider.url());
    let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
        panic!("expected Present");
    };
    assert_config_output(&outputs);
    first_base.assert();
    second_base.assert();
}

#[test]
fn unlisted_base_answers_404_reads_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_inherits_present").to_string())
        .expect(1)
        .create();
    provider
        .mock("GET", names_path("shared-apple", "base").as_str())
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
}

#[test]
fn unlisted_base_answers_403_is_err() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_inherits_present").to_string())
        .expect(1)
        .create();
    provider
        .mock("GET", names_path("shared-apple", "base").as_str())
        .with_status(403)
        .with_body(serde_json::json!({"messages": ["Forbidden"]}).to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

/// (b2) step 2's own "`get_config` itself answering the missing-project
/// shape" bullet: the direct listing came back unlisted, so the walk
/// begins, and `get_config` is the one that answers `404`.
#[test]
fn get_config_answers_404_reads_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
}

/// Same shape, the `400` "no access" spelling of the missing-project
/// answer.
#[test]
fn get_config_answers_400_no_access_reads_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
}

/// (b2) step 1's own "no walk" bullet: the *direct* names call on
/// `config` itself answers the missing-project shape, so `get_config`
/// is never called at all — the path a fresh document hits on its first
/// run, before the config it is about to create exists.
/// `.expect(0)` plus `.assert()` on the config mock is what actually
/// pins "no walk": without asserting it, an unexpected extra call would
/// pass silently.
#[test]
fn names_on_config_answers_404_reads_absent_with_no_walk() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .expect(1)
        .create();
    let config_mock = provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_inherits_present").to_string())
        .expect(0)
        .create();
    let tool = tool_against(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
    config_mock.assert();
}

/// Same shape, the `400` "no access" spelling of the missing-project
/// answer, on the direct call.
#[test]
fn names_on_config_answers_400_no_access_reads_absent_with_no_walk() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .expect(1)
        .create();
    let config_mock = provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_inherits_present").to_string())
        .expect(0)
        .create();
    let tool = tool_against(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
    config_mock.assert();
}

#[test]
fn config_answers_401_is_err() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(401)
        .with_body(serde_json::json!({"messages": ["Unauthorized"]}).to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn config_answers_403_is_err() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(403)
        .with_body(serde_json::json!({"messages": ["Forbidden"]}).to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn config_answers_500_is_err() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(500)
        .with_body(fixture("error_5xx").to_string())
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn get_config_answers_500_is_err() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(500)
        .with_body(fixture("error_5xx").to_string())
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

/// A malformed names body is a [`ToolErrorKind::Provider`] naming
/// `config` and `name` only, never the response text (decision (b2)'s
/// last paragraph, mirroring `doppler.secret.get`'s own malformed-body
/// arm).
#[test]
fn malformed_names_body_is_a_provider_error_naming_config_and_name_only() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(serde_json::json!({"names": null}).to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(
        err.message.contains("third-thoughts/prd"),
        "{}",
        err.message
    );
    assert!(err.message.contains("EXAMPLE_APNS_KEY"), "{}", err.message);
    assert!(!err.message.contains("null"), "{}", err.message);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_absent_never_writes_and_reports_unchanged() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["OTHER"]))
        .expect(1)
        .create();
    provider
        .mock("GET", config_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    assert_config_output(&ensured.outputs);
    // Only the two `GET`s above were mocked; any write (a `POST`) would
    // fail against the mock server rather than silently succeed.
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_present_never_writes_and_reports_unchanged() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", names_path("third-thoughts", "prd").as_str())
        .with_status(200)
        .with_body(names_body(&["EXAMPLE_APNS_KEY"]))
        .expect(1)
        .create();
    let tool = tool_against(provider.url());
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    assert_config_output(&ensured.outputs);
}

/// Acceptance 6: this tool's own source never names a value-reading
/// endpoint or asks Doppler to issue a dynamic-secret lease. Greps the
/// tool's source verbatim, so a future edit that adds one of these
/// strings fails this test rather than only a code review.
#[test]
fn the_tool_never_names_a_value_reading_endpoint() {
    let tool_source = include_str!("../../src/tools/secret_name_gate.rs");
    for forbidden in [
        "get_secret",
        "get_value",
        "/secret?",
        "/secrets?",
        "/secrets/download",
        "include_dynamic_secrets=true",
    ] {
        assert!(
            !tool_source.contains(forbidden),
            "secret_name_gate.rs names `{forbidden}`"
        );
    }
}
