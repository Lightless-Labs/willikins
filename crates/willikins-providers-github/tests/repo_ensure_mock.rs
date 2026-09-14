//! Acceptance test 2's share for `github.repo.ensure`, and acceptance
//! test 9 (`AttributeMismatch`/`Conflict` on a visibility mismatch).

use std::sync::{Arc, Mutex};

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_github::{GitHubClient, GitHubRepoEnsure};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{DomainType, GitHubRepo, RepoVisibility};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "github", name)
}

/// A [`Sleeper`] that records every requested duration and never actually
/// waits, so retry tests run instantly.
#[derive(Default)]
struct RecordingSleeper {
    durations: Mutex<Vec<std::time::Duration>>,
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: std::time::Duration) {
        self.durations.lock().expect("not poisoned").push(duration);
    }
}

fn client_against(url: String) -> (Arc<GitHubClient>, Arc<RecordingSleeper>) {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new(url, Vec::new(), credential);
    let sleeper = Arc::new(RecordingSleeper::default());
    let client = GitHubClient::new(http).with_sleeper(sleeper.clone());
    (Arc::new(client), sleeper)
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("acme/widget").unwrap()
}

fn inputs(visibility: RepoVisibility) -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    inputs.insert(
        PortName::parse("visibility").unwrap(),
        Value::known(visibility),
    );
    inputs
}

#[test]
fn read_reports_absent_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .with_body(fixture("error_basic_404").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    let url = predicted
        .get(&PortName::parse("url").unwrap())
        .unwrap()
        .render()
        .to_string();
    assert_eq!(url, "https://github.com/acme/widget");
}

#[test]
fn read_reports_present_when_owned_with_the_topic_and_matching_visibility() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

#[test]
fn read_reports_foreign_when_the_topic_is_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_foreign").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
    assert!(matches!(observation, Observation::Foreign));
}

#[test]
fn read_reports_foreign_on_a_301_renamed_repository() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(301)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
    assert!(matches!(observation, Observation::Foreign));
}

/// Acceptance test 9: ours, public, requested private.
#[test]
fn read_reports_mismatch_when_owned_and_visibility_differs() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string()) // visibility: private
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let observation = tool.read(&inputs(RepoVisibility::Public)).unwrap();
    assert!(matches!(
        observation,
        Observation::Mismatch { port } if port == PortName::parse("visibility").unwrap()
    ));
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_mismatch_conflicts_and_the_mock_records_no_patch() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string()) // visibility: private
        .create();
    let patch = provider
        .mock("PATCH", "/repos/acme/widget")
        .expect(0)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let token = SinkToken::new();
    let err = tool
        .ensure(&inputs(RepoVisibility::Public), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    patch.assert();
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(503)
        .with_body(format!(r#"{{"message":"{long}"}}"#))
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let err = tool.read(&inputs(RepoVisibility::Private)).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}

#[test]
fn read_is_retried_on_a_429_with_retry_after_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    first.assert();
    second.assert();
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    // Nothing listens on this port: every attempt is a connection
    // failure, a transport error rather than a status code.
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
    let client = Arc::new(GitHubClient::new(http));
    let tool = GitHubRepoEnsure::new(client);
    let err = tool.read(&inputs(RepoVisibility::Private)).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_the_repo_then_sets_the_topic_and_asserts_both_bodies() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .create();
    let create = provider
        .mock("POST", "/orgs/acme/repos")
        .match_body(json_body(
            serde_json::json!({"name": "widget", "visibility": "private"}),
        ))
        .with_status(201)
        .with_body(fixture("repo_post_created").to_string())
        .expect(1)
        .create();
    let topics = provider
        .mock("PUT", "/repos/acme/widget/topics")
        .match_body(json_body(
            serde_json::json!({"names": ["managed-by-willikins"]}),
        ))
        .with_status(200)
        .with_body(fixture("repo_topics_put").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool
        .ensure(&inputs(RepoVisibility::Private), &token)
        .unwrap();
    assert!(ensured.changed);
    create.assert();
    topics.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .create();
    let create = provider
        .mock("POST", "/orgs/acme/repos")
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let token = SinkToken::new();
    let err = tool
        .ensure(&inputs(RepoVisibility::Private), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_422_already_exists_after_create_re_reads_and_reports_present_when_ours() {
    let mut provider = MockProvider::start();
    // First read (inside `ensure`, before the create attempt): absent.
    let first_read = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .expect(1)
        .create();
    let create = provider
        .mock("POST", "/orgs/acme/repos")
        .with_status(422)
        .with_body(fixture("error_already_exists_422").to_string())
        .expect(1)
        .create();
    // Re-read after the ambiguous create: now present and ours.
    let second_read = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool
        .ensure(&inputs(RepoVisibility::Private), &token)
        .unwrap();
    assert!(!ensured.changed);
    first_read.assert();
    create.assert();
    second_read.assert();
}

/// After an ambiguous create, the re-read can find the repository ours
/// but visibility-mismatched rather than foreign — that is still a
/// `Conflict`, but with the mismatch message, not the "already exists and
/// is not ours" one (a distinct bug this test pins: an early version of
/// this tool's re-read branch conflated the two).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_422_already_exists_after_create_re_reads_and_reports_mismatch_not_foreign() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .create();
    provider
        .mock("POST", "/orgs/acme/repos")
        .with_status(422)
        .with_body(fixture("error_already_exists_422").to_string())
        .create();
    // Re-read: ours, but public where private was requested.
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "visibility": "public",
                "topics": ["managed-by-willikins"],
            })
            .to_string(),
        )
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let token = SinkToken::new();
    let err = tool
        .ensure(&inputs(RepoVisibility::Private), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(
        err.message.contains("visibility"),
        "expected the mismatch message, got: {}",
        err.message
    );
    assert!(
        !err.message.contains("not ours"),
        "must not report the foreign-repository message for a mismatch: {}",
        err.message
    );
}

/// GitHub's other shape for "name already taken": `code: "custom"` on
/// `field: "name"` (research note section 2, a real captured example).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_422_custom_name_after_create_is_treated_the_same_as_already_exists() {
    let mut provider = MockProvider::start();
    let first_read = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .expect(1)
        .create();
    let create = provider
        .mock("POST", "/orgs/acme/repos")
        .with_status(422)
        .with_body(fixture("error_custom_name_422").to_string())
        .expect(1)
        .create();
    let second_read = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_foreign").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let token = SinkToken::new();
    let err = tool
        .ensure(&inputs(RepoVisibility::Private), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    first_read.assert();
    create.assert();
    second_read.assert();
}

/// GitHub's secondary rate limit: a `403` carrying `Retry-After` is
/// retried, then succeeds — never surfaced as "missing permission".
#[test]
fn a_secondary_rate_limit_403_is_retried_then_succeeds() {
    let mut provider = MockProvider::start();
    let limited = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(403)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let succeeding = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string())
        .expect(1)
        .create();
    let (client, sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let observation = tool.read(&inputs(RepoVisibility::Private)).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    limited.assert();
    succeeding.assert();
    assert_eq!(sleeper.durations.lock().unwrap().len(), 1);
}

/// A `403` that never clears is reported naming the rate limit and reset
/// time, never as a missing permission.
#[test]
fn a_persistent_secondary_rate_limit_403_names_the_rate_limit_and_reset_time() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(403)
        .with_header("x-ratelimit-remaining", "0")
        .with_header("x-ratelimit-reset", "1700000000")
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let err = tool.read(&inputs(RepoVisibility::Private)).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.contains("rate limit"), "{}", err.message);
    assert!(err.message.contains("1700000000"), "{}", err.message);
    assert!(
        !err.message.contains("permission"),
        "must not read as a missing permission: {}",
        err.message
    );
}

/// A bare `403` (no rate-limit headers) is still the ordinary missing-
/// permission message, and is never retried.
#[test]
fn a_bare_403_is_a_missing_permission_message_not_a_rate_limit() {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(403)
        .expect(1)
        .create();
    let (client, sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let err = tool.read(&inputs(RepoVisibility::Private)).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.contains("permission"), "{}", err.message);
    mock.assert();
    assert_eq!(sleeper.durations.lock().unwrap().len(), 0);
}
