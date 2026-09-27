//! Acceptance test 6 (milestone 3e): `github.repo.get` against a mock —
//! `200` (owned or not) → `Present`; `404` → `NotFound`; archived → `Conflict`;
//! `401` → the fixed `UNAUTHENTICATED` message; `403` → the fixed
//! `MISSING_PERMISSION` message; only `GET` is ever recorded; the tool is
//! pure.

use std::sync::{Arc, Mutex};

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_github::{GitHubClient, GitHubRepoGet};
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_providers_http::{Credential, Http, MISSING_PERMISSION, Sleeper, UNAUTHENTICATED};
use willikins_types::{DomainType, GitHubRepo};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "github", name)
}

/// A [`Sleeper`] that records every requested duration and never actually
/// waits, so a secondary-rate-limit retry (were one ever triggered) runs
/// instantly rather than hanging this test.
#[derive(Default)]
struct RecordingSleeper {
    durations: Mutex<Vec<std::time::Duration>>,
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: std::time::Duration) {
        self.durations.lock().expect("not poisoned").push(duration);
    }
}

fn tool_against(url: String) -> GitHubRepoGet {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new(url, Vec::new(), credential);
    let client = GitHubClient::new(http).with_sleeper(Arc::new(RecordingSleeper::default()));
    GitHubRepoGet::new(Arc::new(client))
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("bande-a-bonnot/monorepo").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    inputs
}

#[test]
fn read_reports_present_with_repo_on_200() {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", "/repos/bande-a-bonnot/monorepo")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string())
        .create();
    let tool = tool_against(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let out_repo = outputs.get(&PortName::parse("repo").unwrap()).unwrap();
    assert_eq!(out_repo.render().to_string(), "bande-a-bonnot/monorepo");
    mock.assert();
}

/// Ownership is not checked: a repository with no `managed-by-willikins`
/// topic (the "foreign" fixture from `github.repo.ensure`'s own tests)
/// still resolves as `Present` here.
#[test]
fn read_reports_present_regardless_of_the_ownership_topic() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/bande-a-bonnot/monorepo")
        .with_status(200)
        .with_body(fixture("repo_get_foreign").to_string())
        .create();
    let tool = tool_against(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

#[test]
fn read_reports_not_found_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/bande-a-bonnot/monorepo")
        .with_status(404)
        .with_body(fixture("error_basic_404").to_string())
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

#[test]
fn read_reports_conflict_when_archived() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/bande-a-bonnot/monorepo")
        .with_status(200)
        .with_body(fixture("repo_get_archived").to_string())
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("archived"), "{}", err.message);
}

#[test]
fn read_reports_unauthenticated_on_401_never_the_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/bande-a-bonnot/monorepo")
        .with_status(401)
        .with_body("Bad credentials: ghp_SECRETVALUE")
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(!err.message.contains("SECRETVALUE"));
    assert_eq!(err.message, UNAUTHENTICATED);
}

#[test]
fn read_reports_missing_permission_on_403_never_the_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/bande-a-bonnot/monorepo")
        .with_status(403)
        .with_body("Bad credentials: ghp_SECRETVALUE")
        .create();
    let tool = tool_against(provider.url());
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(!err.message.contains("SECRETVALUE"));
    assert_eq!(err.message, MISSING_PERMISSION);
}

/// `ensure` is the identity of `read`: same observation, `changed: false`,
/// and — since this tool is pure and read-only — only `GET` is ever
/// recorded against the mock, never a write method.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_agrees_with_read_and_only_get_is_ever_recorded() {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", "/repos/bande-a-bonnot/monorepo")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string())
        .expect(2)
        .create();
    let tool = tool_against(provider.url());
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    mock.assert();
}

#[test]
fn spec_is_pure_with_no_key() {
    let tool = tool_against("http://127.0.0.1:1".to_string());
    assert!(tool.spec().pure);
    assert!(tool.spec().key.is_empty());
}
