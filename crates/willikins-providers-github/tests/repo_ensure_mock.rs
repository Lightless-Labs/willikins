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

/// A `200` whose `topics` is absent, or explicitly `null`, is the same
/// thing as a `200` whose `topics` is `[]`: the ownership marker is not
/// there, so the repository is `Foreign`. GitHub's `full-repository`
/// schema marks no field required at all (research note section 2), so
/// neither shape may become a parse failure reported as `Provider`, and
/// neither may panic.
#[test]
fn read_reports_foreign_when_topics_is_missing_or_null() {
    for body in [
        serde_json::json!({"visibility": "private"}),
        serde_json::json!({"visibility": "private", "topics": null}),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/repos/acme/widget")
            .with_status(200)
            .with_body(body.to_string())
            .create();
        let (client, _sleeper) = client_against(provider.url());
        let tool = GitHubRepoEnsure::new(client);
        let observation = tool
            .read(&inputs(RepoVisibility::Private))
            .unwrap_or_else(|err| panic!("body {body} must read, got {err:?}"));
        assert!(
            matches!(observation, Observation::Foreign),
            "body {body} must read as Foreign, got {observation:?}"
        );
    }
}

/// A client whose [`Http`] carries exactly the three headers GitHub
/// requires — the shape [`willikins_providers_github::http_client`]
/// builds in production, which pins the real base URL and so cannot be
/// aimed at a mock server.
fn client_with_default_headers(url: String) -> Arc<GitHubClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new(
        url,
        willikins_providers_github::default_headers(),
        credential,
    );
    Arc::new(GitHubClient::new(http))
}

/// Add the three required-header matchers to a mock. A request missing
/// any of them fails to match, mockito answers `501`, and the tool call
/// fails — so every mock built this way is itself the assertion.
fn requiring_github_headers(mock: mockito::Mock) -> mockito::Mock {
    mock.match_header("Accept", "application/vnd.github+json")
        .match_header("X-GitHub-Api-Version", "2022-11-28")
        .match_header("User-Agent", mockito::Matcher::Regex("^willikins/".into()))
}

/// Every request `github.repo.ensure` makes — the existence `GET`, the
/// create `POST`, and the topics `PUT` — carries `Accept`,
/// `X-GitHub-Api-Version` and a `willikins/<version>` `User-Agent`
/// (GitHub rejects a request with no `User-Agent` at all).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn every_request_carries_the_three_required_headers() {
    let mut provider = MockProvider::start();
    let read = requiring_github_headers(provider.mock("GET", "/repos/acme/widget"))
        .with_status(404)
        .expect(1)
        .create();
    let create = requiring_github_headers(provider.mock("POST", "/orgs/acme/repos"))
        .with_status(201)
        .with_body(fixture("repo_post_created").to_string())
        .expect(1)
        .create();
    let topics = requiring_github_headers(provider.mock("PUT", "/repos/acme/widget/topics"))
        .with_status(200)
        .with_body(fixture("repo_topics_put").to_string())
        .expect(1)
        .create();

    let tool = GitHubRepoEnsure::new(client_with_default_headers(provider.url()));
    let token = SinkToken::new();
    let ensured = tool
        .ensure(&inputs(RepoVisibility::Private), &token)
        .expect("every request matched the required headers");
    assert!(ensured.changed);
    read.assert();
    create.assert();
    topics.assert();
}

/// `ensure` on a `Foreign` repository writes nothing at all: no create,
/// no topics `PUT`, no `PATCH`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_foreign_repository_conflicts_and_writes_nothing() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_foreign").to_string())
        .create();
    let post = provider.mock("POST", "/orgs/acme/repos").expect(0).create();
    let put = provider
        .mock("PUT", "/repos/acme/widget/topics")
        .expect(0)
        .create();
    let patch = provider
        .mock("PATCH", "/repos/acme/widget")
        .expect(0)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let token = SinkToken::new();
    let err = tool
        .ensure(&inputs(RepoVisibility::Private), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("not ours"), "{}", err.message);
    post.assert();
    put.assert();
    patch.assert();
}

/// `ensure` on a `Mismatch` writes nothing either — the `PATCH` that
/// would change the visibility, and equally the create and the topics
/// `PUT`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_mismatch_writes_neither_post_nor_put_nor_patch() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string()) // visibility: private
        .create();
    let post = provider.mock("POST", "/orgs/acme/repos").expect(0).create();
    let put = provider
        .mock("PUT", "/repos/acme/widget/topics")
        .expect(0)
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
    post.assert();
    put.assert();
    patch.assert();
}

/// A `422` whose `errors[].code` is neither `already_exists` nor a
/// `custom` on `field: name` is an ordinary provider failure: it is not
/// re-read, and it is `Provider`, never `Conflict`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_422_with_another_code_is_a_provider_error_and_is_not_re_read() {
    let mut provider = MockProvider::start();
    let read = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .expect(1)
        .create();
    let create = provider
        .mock("POST", "/orgs/acme/repos")
        .with_status(422)
        .with_body(
            serde_json::json!({
                "message": "Validation Failed",
                "documentation_url": "https://docs.github.com/rest",
                "errors": [{"resource": "Repository", "code": "unprocessable", "field": "name"}],
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let token = SinkToken::new();
    let err = tool
        .ensure(&inputs(RepoVisibility::Private), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    // Exactly one GET: the one `ensure` makes before the create. A
    // re-read would make it two.
    read.assert();
    create.assert();
}

/// Trust boundary 5's "a provider may impersonate willikins" case: a
/// provider message that itself looks like willikins' redaction marker is
/// still labelled as the provider's own words.
#[test]
fn a_provider_message_that_mimics_a_redaction_marker_is_labelled_provider_says() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(503)
        .with_body(r#"{"message":"[REDACTED DopplerServiceToken]"}"#)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = GitHubRepoEnsure::new(client);
    let err = tool.read(&inputs(RepoVisibility::Private)).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(
        err.message.starts_with("provider says: "),
        "{}",
        err.message
    );
}
