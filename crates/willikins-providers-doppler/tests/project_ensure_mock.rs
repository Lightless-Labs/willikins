//! Acceptance test 2's share for `doppler.project.ensure`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerProjectEnsure};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{DomainType, DopplerProject};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "doppler", name)
}

/// A [`Sleeper`] that records every requested duration and never actually
/// waits, so retry tests run instantly.
#[derive(Default)]
struct RecordingSleeper {
    durations: Mutex<Vec<Duration>>,
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.durations.lock().expect("not poisoned").push(duration);
    }
}

fn client_against(url: String) -> (Arc<DopplerClient>, Arc<RecordingSleeper>) {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let sleeper = Arc::new(RecordingSleeper::default());
    let http = Http::new(url, Vec::new(), credential).with_sleeper(sleeper.clone());
    (Arc::new(DopplerClient::new(http)), sleeper)
}

fn project() -> DopplerProject {
    DopplerProject::parse("third-thoughts").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
    inputs
}

#[test]
fn read_reports_absent_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    let value = predicted
        .get(&PortName::parse("project").unwrap())
        .unwrap()
        .render()
        .to_string();
    assert_eq!(value, "third-thoughts");
}

/// **2026-09-20 defect.** Against a quiescent workplace, or one where a
/// project was recently deleted, the same absent project name answers
/// `400` "This token does not have access to requested project" instead
/// of `404` — and before this fix that `400` refused planning outright,
/// which is what a live rehearsal hit: `doppler.project.ensure` could
/// not plan a brand-new project whenever the workplace happened not to
/// have just created one. See
/// `docs/solutions/providers/doppler-400s-a-missing-project-when-quiescent.md`.
#[test]
fn read_reports_absent_on_a_400_naming_no_access() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    let value = predicted
        .get(&PortName::parse("project").unwrap())
        .unwrap()
        .render()
        .to_string();
    assert_eq!(value, "third-thoughts");
}

/// The exact body the 2026-09-20 rehearsal saw, for the exact project
/// name it named: planning `harbor-relay` against a quiescent workplace
/// answered `400` with `"This token does not have access to requested
/// project 'harbor-relay'"`, and was refused outright before this fix.
#[test]
fn read_reports_absent_on_the_exact_body_the_rehearsal_saw() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=harbor-relay")
        .with_status(400)
        .with_body(
            serde_json::json!({
                "success": false,
                "messages": ["This token does not have access to requested project 'harbor-relay'"],
            })
            .to_string(),
        )
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let harbor_relay = DopplerProject::parse("harbor-relay").unwrap();
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("project").unwrap(),
        Value::known(harbor_relay),
    );
    let observation = tool.read(&inputs).unwrap();
    assert!(
        matches!(observation, Observation::Absent { .. }),
        "got {observation:?}"
    );
}

/// A `400` naming anything other than "no access" — Doppler's *other*
/// documented `400`, "Could not find requested project." with a full
/// stop rather than the "does not have access" phrasing — still fails
/// rather than being read as absent: the tolerance is one message, not
/// every `400`.
#[test]
fn read_still_propagates_a_400_with_an_unrelated_message() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(400)
        .with_body(r#"{"success": false, "messages": ["Could not find requested project."]}"#)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn read_reports_present_when_owned() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(200)
        .with_body(fixture("project_get_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

#[test]
fn read_reports_foreign_when_the_marker_is_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(200)
        .with_body(fixture("project_get_foreign").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Foreign));
}

/// A `200` whose `description` is absent, or explicitly `null`, is
/// `Foreign`, not a parse failure — the exact bug class the sibling
/// GitHub crate's `RepoBody.topics` had (task 7 verifier finding):
/// `Option<String>` on its own only covers an absent key, so this pins
/// both shapes in one loop.
#[test]
fn read_reports_foreign_when_description_is_missing_or_null() {
    for body in [
        serde_json::json!({"project": {"name": "third-thoughts"}}),
        serde_json::json!({"project": {"name": "third-thoughts", "description": null}}),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/v3/projects/project?project=third-thoughts")
            .with_status(200)
            .with_body(body.to_string())
            .create();
        let (client, _sleeper) = client_against(provider.url());
        let tool = DopplerProjectEnsure::new(client);
        let observation = tool
            .read(&inputs())
            .unwrap_or_else(|err| panic!("body {body} must read, got {err:?}"));
        assert!(
            matches!(observation, Observation::Foreign),
            "body {body} must read as Foreign, got {observation:?}"
        );
    }
}

/// The ownership check is exact equality, not a substring match: a
/// description that merely *contains* the marker alongside other text
/// reads as `Foreign`, not `Present`.
#[test]
fn read_reports_foreign_when_description_contains_the_marker_plus_more() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "project": {
                    "name": "third-thoughts",
                    "description": "managed-by: willikins (and also by someone else)",
                },
            })
            .to_string(),
        )
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Foreign));
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(503)
        .with_body(format!(r#"{{"messages":["{long}"]}}"#))
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}

#[test]
fn read_is_retried_on_a_429_with_retry_after_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(200)
        .with_body(fixture("project_get_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    first.assert();
    second.assert();
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    // Nothing listens on this port: every attempt is a connection
    // failure, a transport error rather than a status code.
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
    let client = Arc::new(DopplerClient::new(http));
    let tool = DopplerProjectEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_the_project_and_asserts_the_request_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(404)
        .create();
    let create = provider
        .mock("POST", "/v3/projects")
        .match_body(json_body(serde_json::json!({
            "name": "third-thoughts",
            "description": "managed-by: willikins",
        })))
        .with_status(201)
        .with_body(fixture("project_post_created").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(404)
        .create();
    let create = provider
        .mock("POST", "/v3/projects")
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_error_after_create_re_reads_and_reports_unchanged_when_ours() {
    let mut provider = MockProvider::start();
    let first_read = provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(404)
        .expect(1)
        .create();
    let create = provider
        .mock("POST", "/v3/projects")
        .with_status(500)
        .with_body("{}")
        .expect(1)
        .create();
    let second_read = provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(200)
        .with_body(fixture("project_get_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    first_read.assert();
    create.assert();
    second_read.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_error_after_create_that_still_reads_absent_reports_the_original_error() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(404)
        .create();
    provider
        .mock("POST", "/v3/projects")
        .with_status(500)
        .with_body(r#"{"messages":["boom"]}"#)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

/// The create call is the arbiter the 2026-09-20 fix relies on: a `GET`
/// that reads `400` "no access" (so `Absent`) followed by a `POST` that
/// answers `400` "Project name already exists in this workplace." (a
/// genuine duplicate, the other Doppler `400`) must not be swallowed as
/// though the project were still absent. The re-read after the failed
/// create sees the identical `400` "no access" body again (the project
/// really was created by someone else a moment before, and this token
/// still cannot see it), so `ensure` reports the create's own error —
/// naming the duplicate, not "no access" — rather than looping forever
/// or silently succeeding.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_duplicate_create_is_still_distinguished_from_an_absent_project() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    provider
        .mock("POST", "/v3/projects")
        .with_status(400)
        .with_body(fixture("error_400_already_exists").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.contains("already exists"), "{}", err.message);
}

/// `ensure` on a `Foreign` project writes nothing at all.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_foreign_project_conflicts_and_writes_nothing() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(200)
        .with_body(fixture("project_get_foreign").to_string())
        .create();
    let post = provider.mock("POST", "/v3/projects").expect(0).create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerProjectEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("not ours"), "{}", err.message);
    post.assert();
}
