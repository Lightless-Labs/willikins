//! `doppler.config.inherits.ensure`'s own mock-server tests: every `read`
//! arm (`Present`, `Absent`, `Mismatch`, and the missing-parent
//! tolerance) and every documented error status. See
//! `fixtures/doppler/README.md` for how each fixture used here was
//! verified.

use std::sync::Arc;

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerConfigInheritsEnsure};
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

fn base() -> DopplerConfig {
    DopplerConfig::parse("shared-apple/base").unwrap()
}

fn inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs.insert(
        PortName::parse("inherits").unwrap(),
        Value::known_list(vec![base()]),
    );
    inputs
}

/// `inputs()` with an arbitrary `inherits` list, for the underscored-base
/// rows below (milestone 3j decision (a1)).
fn inputs_with(inherits: Vec<DopplerConfig>) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs.insert(
        PortName::parse("inherits").unwrap(),
        Value::known_list(inherits),
    );
    inputs
}

/// The placeholder base config milestone 3j's plan uses throughout:
/// `shared_keys/prd`. Only parseable once `DopplerProject` admits
/// underscores (task A1).
fn shared_keys_prd() -> DopplerConfig {
    DopplerConfig::parse("shared_keys/prd").unwrap()
}

/// An unrelated second base, used to pin the "missing one entry" case
/// below: `other/prd` is never actually inherited by the fixture, so
/// asking for it alongside `shared_keys/prd` must read `Absent`, not
/// `Present`.
fn other_prd() -> DopplerConfig {
    DopplerConfig::parse("other/prd").unwrap()
}

#[test]
fn read_reports_present_when_the_set_matches_exactly() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inherits_present").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// Milestone 3j, task A1 (acceptance 2): a config whose `GET` reports it
/// inheriting an underscored base (`config_get_inherits_underscore.json`)
/// reads `Present` when asked for exactly that base. Before task A1
/// widened `DopplerProject`, the fixture's own `ConfigRefBody.project`
/// failed to deserialize at all, so this test could not even reach an
/// observation.
#[test]
fn read_reports_present_for_an_underscored_base_the_config_actually_inherits() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inherits_underscore").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs_with(vec![shared_keys_prd()])).unwrap();
    assert!(
        matches!(observation, Observation::Present(_)),
        "{observation:?}"
    );
}

/// Same fixture, asked for `[shared_keys/prd, other/prd]`: the config
/// only actually inherits the first, so this is the ordinary "missing
/// entry, nothing extra" case, `Absent` rather than `Mismatch` (the
/// module doc's own distinction).
#[test]
fn read_reports_absent_when_asked_for_an_underscored_base_plus_one_more() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inherits_underscore").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let observation = tool
        .read(&inputs_with(vec![shared_keys_prd(), other_prd()]))
        .unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "{observation:?}"
    );
}

/// Milestone 3j, task C1 (acceptance 9): `Present` carries `config` as
/// its own output, the pass-through decision (c2) adds.
#[test]
fn read_reports_config_known_in_present() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inherits_present").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let Observation::Present(outputs) = tool.read(&inputs()).unwrap() else {
        panic!("expected Present");
    };
    let out = outputs.get(&PortName::parse("config").unwrap()).unwrap();
    assert_eq!(out.render().to_string(), "third-thoughts/prd");
}

#[test]
fn read_reports_absent_when_nothing_is_inherited_yet() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// Milestone 3j, task C1 (acceptance 9): `Absent`'s own `predicted`
/// carries `config` known too, not only `Present`'s outputs -- the port
/// a downstream gate reads at plan time on every fresh run, where this
/// tool itself plans `Create`.
#[test]
fn read_reports_config_known_in_absent_predicted() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let Observation::Absent { predicted } = tool.read(&inputs()).unwrap() else {
        panic!("expected Absent");
    };
    let out = predicted.get(&PortName::parse("config").unwrap()).unwrap();
    assert_eq!(out.render().to_string(), "third-thoughts/prd");
}

/// Same claim, for the missing-project tolerance, since that is a
/// separate branch of `observe` that also returns `Absent`.
#[test]
fn read_reports_config_known_in_absent_predicted_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let Observation::Absent { predicted } = tool.read(&inputs()).unwrap() else {
        panic!("expected Absent");
    };
    let out = predicted.get(&PortName::parse("config").unwrap()).unwrap();
    assert_eq!(out.render().to_string(), "third-thoughts/prd");
}

/// The module doc's own claim: an extra the caller did not ask for is
/// `Mismatch`, never silently dropped or silently accepted.
#[test]
fn read_reports_mismatch_when_an_extra_is_already_inherited() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inherits_extra").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Mismatch { port } = observation else {
        panic!("expected Mismatch, got {observation:?}");
    };
    assert_eq!(port.as_str(), "inherits");
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
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
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
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
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
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
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
        .with_body(fixture("config_get_inherits_present").to_string())
        .expect(1)
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
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
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
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
        .with_body(fixture("config_get_inherits_present").to_string())
        .create();
    let post = provider
        .mock("POST", "/v3/configs/config/inherits")
        .expect(0)
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    // Milestone 3j, task C1 (acceptance 9): `config` is known after a
    // converged read too, the branch that never reaches the `POST`.
    let out = ensured
        .outputs
        .get(&PortName::parse("config").unwrap())
        .unwrap();
    assert_eq!(out.render().to_string(), "third-thoughts/prd");
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_an_extra_with_a_conflict_and_makes_no_post() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inherits_extra").to_string())
        .create();
    let post = provider
        .mock("POST", "/v3/configs/config/inherits")
        .expect(0)
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_sets_the_requested_inherits_and_asserts_the_request_body() {
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
        .mock("POST", "/v3/configs/config/inherits")
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts",
            "config": "prd",
            "inherits": [{"project": "shared-apple", "config": "base"}],
        })))
        .with_status(200)
        .with_body(fixture("config_post_inherits").to_string())
        .expect(1)
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    post.assert();
}

/// Milestone 3j, task C1 (acceptance 9): `config` is known in the
/// `Ensured` returned right after a successful write, not only after a
/// later `read`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_reports_config_known_after_a_write() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    provider
        .mock("POST", "/v3/configs/config/inherits")
        .with_status(200)
        .with_body(fixture("config_post_inherits").to_string())
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    let out = ensured
        .outputs
        .get(&PortName::parse("config").unwrap())
        .unwrap();
    assert_eq!(out.render().to_string(), "third-thoughts/prd");
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
        .mock("POST", "/v3/configs/config/inherits")
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
        .with_body(fixture("config_get_inherits_present").to_string())
        .expect(1)
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    // Milestone 3j, task C1 (acceptance 9): `config` is known even after
    // a failed write, as long as the re-read it falls back to lands on
    // `Present`.
    let out = ensured
        .outputs
        .get(&PortName::parse("config").unwrap())
        .unwrap();
    assert_eq!(out.render().to_string(), "third-thoughts/prd");
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
        .mock("POST", "/v3/configs/config/inherits")
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let tool = DopplerConfigInheritsEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    post.assert();
}
