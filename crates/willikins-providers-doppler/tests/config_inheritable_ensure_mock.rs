//! `doppler.config.inheritable.ensure`'s own mock-server tests: every
//! `read` arm and every documented error status. See
//! `fixtures/doppler/README.md` for how each fixture used here was
//! verified.

use std::sync::Arc;

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerConfigInheritableEnsure};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, DopplerConfig};

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

fn config() -> DopplerConfig {
    DopplerConfig::parse("third-thoughts/prd").unwrap()
}

fn inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs
}

#[test]
fn read_rejects_a_missing_port() {
    let tool = DopplerConfigInheritableEnsure::new(client_against(MockProvider::start().url()));
    let err = tool.read(&Inputs::new()).unwrap_err();
    assert!(err.message.contains("config"), "{}", err.message);
}

#[test]
fn read_reports_present_when_inheritable_is_true() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inheritable_true").to_string())
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// An explicit `inheritable: false` reads `Absent`. This needs its own
/// fixture, and not `config_get_present` (whose body omits the key
/// altogether): with only the omitted-key body in the suite, weakening
/// the read from `== Some(true)` to `.is_some()` left every test in this
/// file green -- checked by mutation on 2026-09-20, with the file
/// restored from a saved copy afterwards. The two bodies are different
/// facts about the same config and both are reachable: Doppler answers
/// `false` for a config that was marked inheritable and then unmarked.
#[test]
fn read_reports_absent_when_inheritable_is_explicitly_false() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inheritable_false").to_string())
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// The other half: a body that omits `inheritable` entirely -- the common
/// case, a config Doppler was never asked to mark inheritable.
#[test]
fn read_reports_absent_when_inheritable_is_absent_from_the_body() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn read_reports_absent_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// Same shared-predicate tolerance every other read in this crate has
/// since 2026-09-20: see `client::looks_like_a_missing_project`'s doc.
#[test]
fn read_reports_absent_on_a_400_naming_no_access() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn read_still_propagates_a_400_with_an_unrelated_message() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(400)
        .with_body(r#"{"success": false, "messages": ["Could not find requested project."]}"#)
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn read_is_retried_on_a_429_with_retry_after_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inheritable_true").to_string())
        .expect(1)
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    first.assert();
    second.assert();
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(503)
        .with_body(fixture("error_5xx").to_string())
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_config_makes_no_post() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inheritable_true").to_string())
        .create();
    let post = provider
        .mock("POST", "/v3/configs/config/inheritable")
        .expect(0)
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_marks_the_config_inheritable_and_asserts_the_request_body() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let post = provider
        .mock("POST", "/v3/configs/config/inheritable")
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts",
            "config": "prd",
            "inheritable": true,
        })))
        .with_status(200)
        .with_body(fixture("config_post_inheritable").to_string())
        .expect(1)
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_error_after_the_post_re_reads_and_reports_unchanged_when_present() {
    let mut provider = MockProvider::start();
    let first_read = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .expect(1)
        .create();
    let post = provider
        .mock("POST", "/v3/configs/config/inheritable")
        .with_status(500)
        .with_body("{}")
        .expect(1)
        .create();
    let second_read = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inheritable_true").to_string())
        .expect(1)
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    first_read.assert();
    post.assert();
    second_read.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let post = provider
        .mock("POST", "/v3/configs/config/inheritable")
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let tool = DopplerConfigInheritableEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    post.assert();
}
