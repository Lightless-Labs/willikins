//! Milestone 3m task P1: mock tests for `github.repo.ensure`'s inverse
//! (decisions 4, 7; acceptance 12). Each test pins the exact query the
//! inverse issues and, where decision 7's own table refuses before the
//! Actions-secrets read, that the secrets `GET` and the `DELETE` were
//! never issued at all (trust boundary 6: "all reads before any
//! delete").

use std::sync::Arc;

use willikins_core::rollback::{CreationWindow, Rider};
use willikins_core::{
    Created, Inputs, InstanceRef, NodeName, Outputs, PortName, Standing, Timestamp, Tool,
    ToolErrorKind, Value,
};
use willikins_providers_github::{GitHubClient, GitHubRepoEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{ActionsSecretName, DomainType, GitHubRepo, GitHubToken, TeardownToken};

fn client_against(url: String) -> Arc<GitHubClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    Arc::new(GitHubClient::new(Http::new(url, Vec::new(), credential)))
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("acme/widget").unwrap()
}

/// `[2026-01-01T00:00:00Z, 2026-01-01T00:10:00Z]`, inclusive both ends.
fn window() -> CreationWindow {
    CreationWindow {
        start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
        end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
    }
}

fn settled_by() -> Timestamp {
    Timestamp::parse("2026-01-01T00:10:00Z").unwrap()
}

fn created_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    inputs
}

/// A [`Created`] over `inputs`/`riders`, the fixed [`window`]/[`settled_by`],
/// and no accepted loss (`github.repo.ensure` declares none at all).
fn created<'a>(inputs: &'a Inputs, outputs: &'a Outputs, riders: &'a [Rider<'a>]) -> Created<'a> {
    Created {
        inputs,
        outputs,
        window: window(),
        settled_by: settled_by(),
        accept: &[],
        riders,
    }
}

/// A repository JSON body matching [`repo`], inside [`window`], not yet
/// pushed to, with no open issues or forks, and the ownership topic — the
/// `AsCreated` case before the Actions-secrets read.
fn as_created_body() -> serde_json::Value {
    serde_json::json!({
        "visibility": "private",
        "topics": ["managed-by-willikins"],
        "full_name": "acme/widget",
        "created_at": "2026-01-01T00:05:00Z",
        "pushed_at": "2026-01-01T00:05:00Z",
        "open_issues_count": 0,
        "forks_count": 0,
    })
}

fn empty_secrets_body() -> serde_json::Value {
    serde_json::json!({ "total_count": 0, "secrets": [] })
}

/// `GitHubRepoEnsure` itself implements nothing beyond `Tool`; its
/// `inverse()` is the one under test.
fn standing(tool: &GitHubRepoEnsure, created: &Created<'_>) -> Standing {
    Tool::inverse(tool)
        .expect("github.repo.ensure declares Delete, so inverse() is Some")
        .standing(created)
        .expect("standing never fails against a well-formed mock response")
}

#[test]
fn standing_reports_gone_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .with_body(serde_json::json!({"message": "Not Found"}).to_string())
        .create();
    let secrets = provider
        .mock(
            "GET",
            "/repos/acme/widget/actions/secrets?per_page=100&page=1",
        )
        .expect(0)
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
    secrets.assert();
}

/// Decision 5: a document-bound `token` port is re-resolved and used for
/// the teardown's own calls, exactly as `read`/`ensure` use it for a
/// forward run -- the reason `reversal()` names `token` in `needs` at
/// all. A `404` is enough here: this proves the credential swap
/// happened, not the drift table (that is every other test's job).
#[test]
fn standing_uses_a_bound_token_client() {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", "/repos/acme/widget")
        .match_header(
            "authorization",
            format!("Bearer {}", GitHubToken::example()).as_str(),
        )
        .with_status(404)
        .with_body(serde_json::json!({"message": "Not Found"}).to_string())
        .expect(1)
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let mut inputs = created_inputs();
    inputs.insert(
        PortName::parse("token").unwrap(),
        Value::known(GitHubToken::parse(GitHubToken::example()).unwrap()),
    );
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
    mock.assert();
}

/// Decision 7: "any 3xx (renamed or transferred: `Http` follows no
/// redirect)" — not only the `301` `observe` itself special-cases.
#[test]
fn standing_reports_not_ours_on_any_3xx() {
    let mut provider = MockProvider::start();
    let mock = provider
        .mock("GET", "/repos/acme/widget")
        .with_status(307)
        .expect(1)
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    mock.assert();
}

#[test]
fn standing_reports_not_ours_when_full_name_has_changed() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["full_name"] = serde_json::json!("acme/renamed");
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let secrets = provider
        .mock(
            "GET",
            "/repos/acme/widget/actions/secrets?per_page=100&page=1",
        )
        .expect(0)
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    secrets.assert();
}

#[test]
fn standing_reports_not_ours_when_the_topic_is_missing() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["topics"] = serde_json::json!([]);
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

/// Acceptance 12: "a `created_at` one second outside the window".
#[test]
fn standing_reports_not_ours_when_created_at_is_one_second_outside_the_window() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["created_at"] = serde_json::json!("2026-01-01T00:10:01Z");
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

/// The lower edge, mirroring the upper-edge test above: one second
/// before the window's own start is `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_at_is_one_second_before_the_window() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["created_at"] = serde_json::json!("2025-12-31T23:59:59Z");
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

/// The same instant as the window's own end is still inside it (an
/// inclusive window), so this reaches the Actions-secrets read and comes
/// back `AsCreated`.
#[test]
fn standing_reports_as_created_when_created_at_is_exactly_the_windows_end() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["created_at"] = serde_json::json!("2026-01-01T00:10:00Z");
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    provider
        .mock(
            "GET",
            "/repos/acme/widget/actions/secrets?per_page=100&page=1",
        )
        .with_status(200)
        .with_body(empty_secrets_body().to_string())
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

/// A response that omits `created_at` outright fails closed: never "in
/// the window", always `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_at_is_missing() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body.as_object_mut().unwrap().remove("created_at");
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_drifted_on_a_push_after_settled_by() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["pushed_at"] = serde_json::json!("2026-01-01T00:10:01Z");
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let secrets = provider
        .mock(
            "GET",
            "/repos/acme/widget/actions/secrets?per_page=100&page=1",
        )
        .expect(0)
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
    secrets.assert();
}

/// A response that omits `pushed_at` outright fails closed: never "no
/// push since", always `Drifted`.
#[test]
fn standing_reports_drifted_when_pushed_at_is_missing() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body.as_object_mut().unwrap().remove("pushed_at");
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

#[test]
fn standing_reports_drifted_on_an_open_issue() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["open_issues_count"] = serde_json::json!(1);
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

#[test]
fn standing_reports_drifted_on_a_fork() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["forks_count"] = serde_json::json!(1);
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

#[test]
fn standing_reports_drifted_on_a_foreign_actions_secret() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets?per_page=100&page=1")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "total_count": 1,
                "secrets": [{"name": "HAND_SET", "created_at": "2026-01-01T00:05:00Z", "updated_at": "2026-01-01T00:05:00Z"}],
            })
            .to_string(),
        )
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

/// An Actions secret this run's own rider created is tolerated: the
/// container reports `AsCreated`, not `Drifted`.
#[test]
fn standing_reports_as_created_when_every_secret_is_a_tolerated_rider() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    provider
        .mock("GET", "/repos/acme/widget/actions/secrets?per_page=100&page=1")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "total_count": 1,
                "secrets": [{"name": "DOPPLER_TOKEN", "created_at": "2026-01-01T00:05:00Z", "updated_at": "2026-01-01T00:05:00Z"}],
            })
            .to_string(),
        )
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let rider_instance = InstanceRef {
        node: NodeName::parse("secret").unwrap(),
        instance: None,
    };
    let mut rider_inputs = Inputs::new();
    rider_inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(ActionsSecretName::parse("DOPPLER_TOKEN").unwrap()),
    );
    let rider_outputs = Outputs::new();
    let riders = [Rider {
        tool: "github.actions_secret.ensure",
        instance: &rider_instance,
        inputs: &rider_inputs,
        outputs: &rider_outputs,
    }];
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &riders)),
        Standing::AsCreated { losing: Vec::new() }
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_is_a_noop_when_already_gone() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(404)
        .with_body(serde_json::json!({"message": "Not Found"}).to_string())
        .create();
    let delete = provider
        .mock("DELETE", "/repos/acme/widget")
        .expect(0)
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap();
    assert!(deleted.already_gone);
    delete.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_deletes_when_as_created() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(as_created_body().to_string())
        .create();
    provider
        .mock(
            "GET",
            "/repos/acme/widget/actions/secrets?per_page=100&page=1",
        )
        .with_status(200)
        .with_body(empty_secrets_body().to_string())
        .create();
    let delete = provider
        .mock("DELETE", "/repos/acme/widget")
        .with_status(204)
        .expect(1)
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap();
    assert!(!deleted.already_gone);
    delete.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_refuses_as_a_conflict_when_drifted() {
    let mut provider = MockProvider::start();
    let mut body = as_created_body();
    body["forks_count"] = serde_json::json!(1);
    provider
        .mock("GET", "/repos/acme/widget")
        .with_status(200)
        .with_body(body.to_string())
        .create();
    let delete = provider
        .mock("DELETE", "/repos/acme/widget")
        .expect(0)
        .create();
    let tool = GitHubRepoEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let err = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs, &[]), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    delete.assert();
}

/// Decision 4's reversal declaration itself: `Delete`, no container, no
/// loss (acceptance 1's own validation already proves this against
/// `Catalog::insert`; `catalog_check_parity.rs` does so for this crate).
#[test]
fn reversal_declares_delete_with_no_within_and_no_loss() {
    let tool = GitHubRepoEnsure::new(client_against("http://127.0.0.1:1".to_string()));
    let willikins_core::Reversal::Delete {
        needs,
        within,
        losses,
    } = Tool::reversal(&tool)
    else {
        panic!("github.repo.ensure must declare Reversal::Delete");
    };
    assert_eq!(needs.to_vec(), vec!["repo", "token"]);
    assert!(within.is_empty());
    assert!(losses.is_empty());
}
