//! Milestone 3e task B1's mock coverage for `doppler.branch_config.ensure`.
//! `read`'s `Present` needs `root: false` **and** a matching `environment`
//! (this tool's own module doc, "Read: `root: false` and the right
//! environment"); `ensure`'s create path is `POST /v3/configs` with the
//! already-prefixed full name this tool assembles from `environment` and
//! `branch` (see `full_name`'s own doc and unit tests in
//! `src/tools/branch_config_ensure.rs`).
//!
//! **Verify item, recorded rather than assumed:** what Doppler answers
//! `GET /v3/configs/config` for a branch config that is absent from a
//! project that *does* exist (as opposed to the missing-*project* case
//! every other Doppler `read` in this crate already tolerates) is
//! unestablished by any primary source read for this task. `looks_like_a_missing_project`
//! already treats a bare `404` as `Absent` regardless of which of the two
//! it names, so this file pins that one shape (a `404`); whether Doppler
//! instead answers a `400` naming something other than "no access" for
//! this specific case — which `read_still_propagates_a_400_with_an_unrelated_message`
//! shows fails the plan today — is unverified and belongs on the
//! milestone's verify list, not assumed true here.

use std::sync::Arc;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerBranchConfigEnsure, DopplerClient};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, DopplerConfigName, DopplerProject, EnvironmentSlug};

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

fn project() -> DopplerProject {
    DopplerProject::parse("sample").unwrap()
}

fn environment() -> EnvironmentSlug {
    EnvironmentSlug::parse("prd").unwrap()
}

fn branch() -> DopplerConfigName {
    DopplerConfigName::parse("deployment_ios").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
    inputs.insert(
        PortName::parse("environment").unwrap(),
        Value::known(environment()),
    );
    inputs.insert(PortName::parse("branch").unwrap(), Value::known(branch()));
    inputs
}

const CONFIG_PATH: &str = "/v3/configs/config?project=sample&config=prd_deployment_ios";

#[test]
fn read_reports_absent_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    let value = predicted
        .get(&PortName::parse("config").unwrap())
        .unwrap()
        .render()
        .to_string();
    assert_eq!(value, "sample/prd_deployment_ios");
}

#[test]
fn read_reports_absent_on_a_400_naming_no_access() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// A `400` naming anything other than "no access" still fails -- the same
/// posture `doppler.config.ensure`'s own mock suite pins, and the reason
/// this file's own header comment records the missing-*config*-in-an-
/// existing-project shape as a verify item rather than an assumption.
#[test]
fn read_still_propagates_a_400_with_an_unrelated_message() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(400)
        .with_body(r#"{"success": false, "messages": ["Some other problem."]}"#)
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn read_reports_present_when_root_is_false_and_environment_matches() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(200)
        .with_body(fixture("branch_config_get_present").to_string())
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// The inverse of `doppler.config.ensure`'s own collision guard: a
/// literal branch name this tool derived could, in principle, already be
/// occupied by the environment's own *root* config -- `root: true` at
/// this name is `Foreign`, never adopted as this tool's own branch
/// config.
#[test]
fn read_reports_foreign_when_root_is_true() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"name": "prd_deployment_ios", "root": true,
                                          "environment": "prd"}})
            .to_string(),
        )
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(
        matches!(observation, Observation::Foreign),
        "got {observation:?}"
    );
}

/// A same-named branch config under a *different* environment is a
/// different resource entirely -- Doppler names configs uniquely within
/// a project, not scoped per environment, so nothing prevents the
/// literal string colliding.
#[test]
fn read_reports_foreign_when_the_environment_does_not_match() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"name": "prd_deployment_ios", "root": false,
                                          "environment": "stg"}})
            .to_string(),
        )
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(
        matches!(observation, Observation::Foreign),
        "got {observation:?}"
    );
}

/// A missing or null `root` -- like a missing or null `environment` -- is
/// `Foreign` too, never a parse failure and never `Present`: absence of
/// proof is not proof of presence, the same rule `doppler.config.ensure`'s
/// own mock suite pins for its `root: true` case.
#[test]
fn read_reports_foreign_when_root_or_environment_is_missing_or_null() {
    for body in [
        serde_json::json!({"config": {"name": "prd_deployment_ios"}}),
        serde_json::json!({"config": {"name": "prd_deployment_ios", "root": null, "environment": "prd"}}),
        serde_json::json!({"config": {"name": "prd_deployment_ios", "root": false, "environment": null}}),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", CONFIG_PATH)
            .with_status(200)
            .with_body(body.to_string())
            .create();
        let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
        let observation = tool
            .read(&inputs())
            .unwrap_or_else(|err| panic!("body {body} must read, got {err:?}"));
        assert!(
            matches!(observation, Observation::Foreign),
            "body {body} must read as Foreign, got {observation:?}"
        );
    }
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_the_branch_config_and_asserts_the_request_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let create = provider
        .mock("POST", "/v3/configs")
        .match_body(json_body(serde_json::json!({
            "project": "sample",
            "environment": "prd",
            "name": "prd_deployment_ios",
        })))
        .with_status(201)
        .with_body(fixture("branch_config_post_created").to_string())
        .expect(1)
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_config_makes_no_post() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(200)
        .with_body(fixture("branch_config_get_present").to_string())
        .create();
    let post = provider.mock("POST", "/v3/configs").expect(0).create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    post.assert();
}

/// A root config genuinely squatting on this tool's derived name refuses
/// -- `Conflict`, no `POST` -- rather than silently adopting it.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_a_foreign_config_with_no_post() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"name": "prd_deployment_ios", "root": true,
                                          "environment": "prd"}})
            .to_string(),
        )
        .create();
    let post = provider.mock("POST", "/v3/configs").expect(0).create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let create = provider
        .mock("POST", "/v3/configs")
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_error_after_create_re_reads_and_reports_unchanged_when_present() {
    let mut provider = MockProvider::start();
    let first_read = provider
        .mock("GET", CONFIG_PATH)
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .expect(1)
        .create();
    let create = provider
        .mock("POST", "/v3/configs")
        .with_status(500)
        .with_body("{}")
        .expect(1)
        .create();
    let second_read = provider
        .mock("GET", CONFIG_PATH)
        .with_status(200)
        .with_body(fixture("branch_config_get_present").to_string())
        .expect(1)
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    first_read.assert();
    create.assert();
    second_read.assert();
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock("GET", CONFIG_PATH)
        .with_status(503)
        .with_body(format!(r#"{{"messages":["{long}"]}}"#))
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}
