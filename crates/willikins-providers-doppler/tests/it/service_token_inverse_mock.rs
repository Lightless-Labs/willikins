//! Milestone 3m task P3: mock tests for `doppler.service_token.ensure`'s
//! inverse (decisions 4, 7; acceptance 12).

use std::sync::Arc;

use willikins_core::rollback::CreationWindow;
use willikins_core::{
    Created, Inputs, Outputs, PortName, Side, Standing, Timestamp, Tool, ToolErrorKind, Value,
    Within,
};
use willikins_providers_doppler::{DopplerClient, DopplerServiceTokenEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, DopplerConfig, DopplerTokenName, TeardownToken};

fn client_against(url: String) -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    Arc::new(DopplerClient::new(Http::new(url, Vec::new(), credential)))
}

fn config() -> DopplerConfig {
    DopplerConfig::parse("shared_keys/prd").unwrap()
}

fn name() -> DopplerTokenName {
    DopplerTokenName::parse("ci").unwrap()
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
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs
}

fn created<'a>(inputs: &'a Inputs, outputs: &'a Outputs) -> Created<'a> {
    Created {
        inputs,
        outputs,
        window: window(),
        settled_by: settled_by(),
        accept: &[],
        riders: &[],
    }
}

fn standing(tool: &DopplerServiceTokenEnsure, created: &Created<'_>) -> Standing {
    Tool::inverse(tool)
        .expect("doppler.service_token.ensure declares Delete, so inverse() is Some")
        .standing(created)
        .expect("standing never fails against a well-formed mock response")
}

fn project_query() -> mockito::Matcher {
    mockito::Matcher::UrlEncoded("project".into(), "shared_keys".into())
}

fn mock_project(provider: &mut MockProvider, status: usize, body: &serde_json::Value) {
    provider
        .mock("GET", "/v3/projects/project")
        .match_query(project_query())
        .with_status(status)
        .with_body(body.to_string())
        .create();
}

fn managed_project_body() -> serde_json::Value {
    serde_json::json!({"project": {"description": "managed-by: willikins"}})
}

fn tokens_query() -> mockito::Matcher {
    mockito::Matcher::AllOf(vec![
        project_query(),
        mockito::Matcher::UrlEncoded("config".into(), "prd".into()),
    ])
}

fn mock_tokens(provider: &mut MockProvider, entries: &[serde_json::Value]) {
    provider
        .mock("GET", "/v3/configs/config/tokens")
        .match_query(tokens_query())
        .with_status(200)
        .with_body(serde_json::json!({"tokens": entries}).to_string())
        .create();
}

fn token_entry(name: &str, slug: &str, created_at: &str) -> serde_json::Value {
    serde_json::json!({"name": name, "slug": slug, "created_at": created_at})
}

// ---------------------------------------------------------------------
// Gone
// ---------------------------------------------------------------------

#[test]
fn standing_reports_gone_on_a_missing_project() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        404,
        &serde_json::json!({"messages": ["Could not find requested project"]}),
    );
    let tokens = provider
        .mock("GET", "/v3/configs/config/tokens")
        .expect(0)
        .create();
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(standing(&tool, &created(&inputs, &outputs)), Standing::Gone);
    tokens.assert();
}

/// No token is listed at this name at all -- hand-deleted, or an earlier
/// step of this same teardown already deleted it (decision 6's resume
/// case). `Gone`, never `NotOurs`.
#[test]
fn standing_reports_gone_when_no_token_is_listed() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(&mut provider, &[]);
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(standing(&tool, &created(&inputs, &outputs)), Standing::Gone);
}

/// A `400`/`404` listing the tokens themselves (the parent config is
/// gone) also reads `Gone`, mirroring `is_listed`'s own tolerance.
#[test]
fn standing_reports_gone_when_the_token_listing_itself_404s() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    provider
        .mock("GET", "/v3/configs/config/tokens")
        .match_query(tokens_query())
        .with_status(404)
        .with_body(serde_json::json!({"messages": ["Could not find requested config"]}).to_string())
        .create();
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(standing(&tool, &created(&inputs, &outputs)), Standing::Gone);
}

// ---------------------------------------------------------------------
// NotOurs
// ---------------------------------------------------------------------

#[test]
fn standing_reports_not_ours_when_the_marker_is_gone() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        200,
        &serde_json::json!({"project": {"description": "some other note"}}),
    );
    let tokens = provider
        .mock("GET", "/v3/configs/config/tokens")
        .expect(0)
        .create();
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs)),
        Standing::NotOurs { .. }
    ));
    tokens.assert();
}

/// A response that omits the token's own `created_at` outright fails
/// closed: never "in the window", always `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_at_is_missing() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[serde_json::json!({"name": "ci", "slug": "11111111-1111-1111-1111-111111111111"})],
    );
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs)),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_created_at_is_one_second_before_the_window() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[token_entry(
            "ci",
            "11111111-1111-1111-1111-111111111111",
            "2025-12-31T23:59:59Z",
        )],
    );
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs)),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_created_at_is_one_second_after_the_window() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[token_entry(
            "ci",
            "11111111-1111-1111-1111-111111111111",
            "2026-01-01T00:10:01Z",
        )],
    );
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs)),
        Standing::NotOurs { .. }
    ));
}

/// A listed token under a different name is never consulted: only an
/// entry whose own `name` matches is read at all.
#[test]
fn standing_reports_gone_when_only_a_differently_named_token_is_listed() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[token_entry(
            "deploy",
            "22222222-2222-2222-2222-222222222222",
            "2026-01-01T00:05:00Z",
        )],
    );
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(standing(&tool, &created(&inputs, &outputs)), Standing::Gone);
}

// ---------------------------------------------------------------------
// Drifted
// ---------------------------------------------------------------------

#[test]
fn standing_reports_drifted_when_more_than_one_token_matches() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[
            token_entry(
                "ci",
                "11111111-1111-1111-1111-111111111111",
                "2026-01-01T00:05:00Z",
            ),
            token_entry(
                "ci",
                "22222222-2222-2222-2222-222222222222",
                "2026-01-01T00:06:00Z",
            ),
        ],
    );
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs)),
        Standing::Drifted { .. }
    ));
}

// ---------------------------------------------------------------------
// AsCreated
// ---------------------------------------------------------------------

#[test]
fn standing_reports_as_created_for_the_one_matching_token() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[token_entry(
            "ci",
            "11111111-1111-1111-1111-111111111111",
            "2026-01-01T00:05:00Z",
        )],
    );
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs)),
        Standing::AsCreated { losing: Vec::new() }
    );
}

#[test]
fn standing_reports_as_created_when_created_at_is_exactly_the_windows_end() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[token_entry(
            "ci",
            "11111111-1111-1111-1111-111111111111",
            "2026-01-01T00:10:00Z",
        )],
    );
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs)),
        Standing::AsCreated { losing: Vec::new() }
    );
}

// ---------------------------------------------------------------------
// delete
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_is_a_noop_when_already_gone() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(&mut provider, &[]);
    let delete = provider
        .mock("DELETE", "/v3/configs/config/tokens/token")
        .expect(0)
        .create();
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs), &token)
        .unwrap();
    assert!(deleted.already_gone);
    delete.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_deletes_the_matching_slug_when_as_created() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[token_entry(
            "ci",
            "11111111-1111-1111-1111-111111111111",
            "2026-01-01T00:05:00Z",
        )],
    );
    let delete = provider
        .mock("DELETE", "/v3/configs/config/tokens/token")
        .match_body(willikins_providers_http::testing::json_body(
            serde_json::json!({
                "project": "shared_keys",
                "config": "prd",
                "slug": "11111111-1111-1111-1111-111111111111",
            }),
        ))
        .with_status(200)
        .with_body(serde_json::json!({"success": true}).to_string())
        .expect(1)
        .create();
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let deleted = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs), &token)
        .unwrap();
    assert!(!deleted.already_gone);
    delete.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_refuses_as_a_conflict_when_drifted() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_tokens(
        &mut provider,
        &[
            token_entry(
                "ci",
                "11111111-1111-1111-1111-111111111111",
                "2026-01-01T00:05:00Z",
            ),
            token_entry(
                "ci",
                "22222222-2222-2222-2222-222222222222",
                "2026-01-01T00:06:00Z",
            ),
        ],
    );
    let delete = provider
        .mock("DELETE", "/v3/configs/config/tokens/token")
        .expect(0)
        .create();
    let tool = DopplerServiceTokenEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let token = TeardownToken::new();
    let err = Tool::inverse(&tool)
        .unwrap()
        .delete(&created(&inputs, &outputs), &token)
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    delete.assert();
}

/// Decision 4's reversal declaration: `Delete`, riding with either
/// `doppler.config.ensure` or `doppler.branch_config.ensure`, no loss.
#[test]
fn reversal_declares_delete_within_either_config_tool() {
    let tool = DopplerServiceTokenEnsure::new(client_against("http://127.0.0.1:1".to_string()));
    let willikins_core::Reversal::Delete {
        needs,
        within,
        losses,
    } = Tool::reversal(&tool)
    else {
        panic!("doppler.service_token.ensure must declare Reversal::Delete");
    };
    assert_eq!(needs.to_vec(), vec!["config", "name"]);
    assert_eq!(
        within,
        &[
            Within {
                tool: "doppler.config.ensure",
                ports: &[("config", Side::Output, "config")],
            },
            Within {
                tool: "doppler.branch_config.ensure",
                ports: &[("config", Side::Output, "config")],
            },
        ]
    );
    assert!(losses.is_empty());
}
