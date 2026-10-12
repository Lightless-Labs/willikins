//! Milestone 3m task P3: mock tests for `doppler.branch_config.ensure`'s
//! inverse (decisions 4, 7; acceptance 12), mirroring
//! `config_inverse_mock.rs`'s own conventions scoped to one config
//! instead of a whole environment or project.

use std::sync::Arc;

use willikins_core::rollback::{CreationWindow, Rider};
use willikins_core::{
    Created, Inputs, InstanceRef, NodeName, Outputs, PortName, Side, Standing, Timestamp, Tool,
    ToolErrorKind, Value, Within,
};
use willikins_providers_doppler::{DopplerBranchConfigEnsure, DopplerClient};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, EnvironmentSlug, SecretName,
    TeardownToken,
};

fn client_against(url: String) -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    Arc::new(DopplerClient::new(Http::new(url, Vec::new(), credential)))
}

fn project() -> DopplerProject {
    DopplerProject::parse("shared_keys").unwrap()
}

fn environment() -> EnvironmentSlug {
    EnvironmentSlug::parse("prd").unwrap()
}

fn branch() -> DopplerConfigName {
    DopplerConfigName::parse("deployment").unwrap()
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
    inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
    inputs.insert(
        PortName::parse("environment").unwrap(),
        Value::known(environment()),
    );
    inputs.insert(PortName::parse("branch").unwrap(), Value::known(branch()));
    inputs
}

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

fn standing(tool: &DopplerBranchConfigEnsure, created: &Created<'_>) -> Standing {
    Tool::inverse(tool)
        .expect("doppler.branch_config.ensure declares Delete, so inverse() is Some")
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

fn mock_configs(provider: &mut MockProvider, configs: &[serde_json::Value]) {
    provider
        .mock("GET", "/v3/configs")
        .match_query(mockito::Matcher::AllOf(vec![
            project_query(),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
        ]))
        .with_status(200)
        .with_body(serde_json::json!({"configs": configs, "page": 1}).to_string())
        .create();
}

fn branch_entry(created_at: &str) -> serde_json::Value {
    serde_json::json!({"name": "prd_deployment", "root": false, "environment": "prd", "created_at": created_at})
}

fn config_query(name: &str) -> mockito::Matcher {
    mockito::Matcher::AllOf(vec![
        project_query(),
        mockito::Matcher::UrlEncoded("config".into(), name.into()),
    ])
}

fn mock_names(provider: &mut MockProvider, name: &str, names: &[&str]) {
    provider
        .mock("GET", "/v3/configs/config/secrets/names")
        .match_query(config_query(name))
        .with_status(200)
        .with_body(serde_json::json!({"names": names}).to_string())
        .create();
}

fn mock_tokens(provider: &mut MockProvider, name: &str, tokens: &[&str]) {
    let entries: Vec<_> = tokens
        .iter()
        .map(|name| serde_json::json!({"name": name, "slug": "11111111-1111-1111-1111-111111111111"}))
        .collect();
    provider
        .mock("GET", "/v3/configs/config/tokens")
        .match_query(config_query(name))
        .with_status(200)
        .with_body(serde_json::json!({"tokens": entries}).to_string())
        .create();
}

fn mock_clean_config(provider: &mut MockProvider, name: &str) {
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query(name))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": false, "environment": "prd", "inherits": [], "inheritedBy": []}})
                .to_string(),
        )
        .create();
    mock_names(provider, name, &[]);
    mock_tokens(provider, name, &[]);
}

fn mock_fresh_branch(provider: &mut MockProvider) {
    mock_configs(provider, &[branch_entry("2026-01-01T00:05:00Z")]);
    mock_clean_config(provider, "prd_deployment");
}

// ---------------------------------------------------------------------
// Gone
// ---------------------------------------------------------------------

#[test]
fn standing_reports_gone_on_404() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        404,
        &serde_json::json!({"messages": ["Could not find requested project"]}),
    );
    let configs = provider.mock("GET", "/v3/configs").expect(0).create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
    configs.assert();
}

#[test]
fn standing_reports_gone_when_the_config_is_not_listed() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[
            serde_json::json!({"name": "prd", "root": true, "environment": "prd", "created_at": "2026-01-01T00:05:00Z"}),
        ],
    );
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
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
    let configs = provider.mock("GET", "/v3/configs").expect(0).create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    configs.assert();
}

/// The config at this name has become a root config -- the identity has
/// drifted, never "not ours" by marker.
#[test]
fn standing_reports_not_ours_when_root_is_true() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[
            serde_json::json!({"name": "prd_deployment", "root": true, "environment": "prd", "created_at": "2026-01-01T00:05:00Z"}),
        ],
    );
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_the_environment_differs() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[
            serde_json::json!({"name": "prd_deployment", "root": false, "environment": "stg", "created_at": "2026-01-01T00:05:00Z"}),
        ],
    );
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_created_at_is_missing() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "prd_deployment", "root": false, "environment": "prd"})],
    );
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_created_at_is_one_second_before_the_window() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[branch_entry("2025-12-31T23:59:59Z")]);
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

#[test]
fn standing_reports_not_ours_when_created_at_is_one_second_after_the_window() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[branch_entry("2026-01-01T00:10:01Z")]);
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

// ---------------------------------------------------------------------
// AsCreated
// ---------------------------------------------------------------------

#[test]
fn standing_reports_as_created_for_a_clean_branch_config() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_fresh_branch(&mut provider);
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

#[test]
fn standing_reports_as_created_when_created_at_is_exactly_the_windows_end() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[branch_entry("2026-01-01T00:10:00Z")]);
    mock_clean_config(&mut provider, "prd_deployment");
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

// ---------------------------------------------------------------------
// Drifted
// ---------------------------------------------------------------------

#[test]
fn standing_reports_drifted_on_a_hand_set_secret() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[branch_entry("2026-01-01T00:05:00Z")]);
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd_deployment"))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": false, "environment": "prd", "inherits": [], "inheritedBy": []}})
                .to_string(),
        )
        .create();
    mock_names(&mut provider, "prd_deployment", &["HAND_SET"]);
    let tokens = provider
        .mock("GET", "/v3/configs/config/tokens")
        .expect(0)
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
    tokens.assert();
}

/// A secret this run's own rider `doppler.secret.set` wrote is
/// tolerated.
#[test]
fn standing_reports_as_created_when_the_secret_is_a_tolerated_rider() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[branch_entry("2026-01-01T00:05:00Z")]);
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd_deployment"))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": false, "environment": "prd", "inherits": [], "inheritedBy": []}})
                .to_string(),
        )
        .create();
    mock_names(&mut provider, "prd_deployment", &["DOPPLER_TOKEN"]);
    mock_tokens(&mut provider, "prd_deployment", &[]);
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let rider_instance = InstanceRef {
        node: NodeName::parse("secret").unwrap(),
        instance: None,
    };
    let mut rider_inputs = Inputs::new();
    rider_inputs.insert(
        PortName::parse("config").unwrap(),
        Value::known(DopplerConfig::new(
            project(),
            DopplerConfigName::parse("prd_deployment").unwrap(),
        )),
    );
    rider_inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(SecretName::parse("DOPPLER_TOKEN").unwrap()),
    );
    let rider_outputs = Outputs::new();
    let riders = [Rider {
        tool: "doppler.secret.set",
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
fn standing_reports_drifted_when_inherited_by_is_missing() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[branch_entry("2026-01-01T00:05:00Z")]);
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd_deployment"))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": false, "environment": "prd", "inherits": []}})
                .to_string(),
        )
        .create();
    mock_names(&mut provider, "prd_deployment", &[]);
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

#[test]
fn standing_reports_drifted_on_a_foreign_service_token() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[branch_entry("2026-01-01T00:05:00Z")]);
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd_deployment"))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": false, "environment": "prd", "inherits": [], "inheritedBy": []}})
                .to_string(),
        )
        .create();
    mock_names(&mut provider, "prd_deployment", &[]);
    mock_tokens(&mut provider, "prd_deployment", &["ci"]);
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

// ---------------------------------------------------------------------
// delete
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn delete_is_a_noop_when_already_gone() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        404,
        &serde_json::json!({"messages": ["Could not find requested project"]}),
    );
    let delete = provider
        .mock("DELETE", "/v3/configs/config")
        .expect(0)
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
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
    mock_project(&mut provider, 200, &managed_project_body());
    mock_fresh_branch(&mut provider);
    let delete = provider
        .mock("DELETE", "/v3/configs/config")
        .match_query(mockito::Matcher::AllOf(vec![
            project_query(),
            mockito::Matcher::UrlEncoded("config".into(), "prd_deployment".into()),
        ]))
        .with_status(200)
        .with_body(serde_json::json!({"success": true}).to_string())
        .expect(1)
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
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
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[
            serde_json::json!({"name": "prd_deployment", "root": true, "environment": "prd", "created_at": "2026-01-01T00:05:00Z"}),
        ],
    );
    let delete = provider
        .mock("DELETE", "/v3/configs/config")
        .expect(0)
        .create();
    let tool = DopplerBranchConfigEnsure::new(client_against(provider.url()));
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

/// Decision 4's reversal declaration: `Delete`, riding with either
/// `doppler.project.ensure` or `doppler.config.ensure`, no loss.
#[test]
fn reversal_declares_delete_within_project_or_config_ensure() {
    let tool = DopplerBranchConfigEnsure::new(client_against("http://127.0.0.1:1".to_string()));
    let willikins_core::Reversal::Delete {
        needs,
        within,
        losses,
    } = Tool::reversal(&tool)
    else {
        panic!("doppler.branch_config.ensure must declare Reversal::Delete");
    };
    assert_eq!(needs.to_vec(), vec!["project", "environment", "branch"]);
    assert_eq!(
        within,
        &[
            Within {
                tool: "doppler.project.ensure",
                ports: &[("project", Side::Input, "project")],
            },
            Within {
                tool: "doppler.config.ensure",
                ports: &[
                    ("project", Side::Input, "project"),
                    ("environment", Side::Input, "environment"),
                ],
            },
        ]
    );
    assert!(losses.is_empty());
}
