//! Milestone 3m task P3: mock tests for `doppler.config.ensure`'s
//! inverse (decisions 4, 7; acceptance 12). Each test pins the exact
//! query the inverse issues and, where decision 7's own table refuses
//! before a later read, that the later read was never issued at all
//! (trust boundary 6: "all reads before any delete") -- the same
//! conventions `project_inverse_mock.rs` already established for
//! `doppler.project.ensure`'s own inverse.

use std::sync::Arc;

use willikins_core::rollback::{CreationWindow, Rider};
use willikins_core::{
    Created, Inputs, InstanceRef, NodeName, Outputs, PortName, Side, Standing, Timestamp, Tool,
    ToolErrorKind, Value, Within,
};
use willikins_providers_doppler::{DopplerClient, DopplerConfigEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, EnvironmentSlug, TeardownToken,
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

fn standing(tool: &DopplerConfigEnsure, created: &Created<'_>) -> Standing {
    Tool::inverse(tool)
        .expect("doppler.config.ensure declares Delete, so inverse() is Some")
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

fn root_entry(created_at: &str) -> serde_json::Value {
    serde_json::json!({"name": "prd", "root": true, "environment": "prd", "created_at": created_at})
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

/// A clean config at `name`: no `inherits`, no `inheritedBy`, no foreign
/// secret names, no foreign tokens.
fn mock_clean_config(provider: &mut MockProvider, name: &str) {
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query(name))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": true, "inherits": [], "inheritedBy": []}})
                .to_string(),
        )
        .create();
    mock_names(provider, name, &[]);
    mock_tokens(provider, name, &[]);
}

/// Just the root config, clean, at the window's own start -- the shape a
/// fresh environment with no branch configs has.
fn mock_fresh_environment(provider: &mut MockProvider) {
    mock_configs(provider, &[root_entry("2026-01-01T00:05:00Z")]);
    mock_clean_config(provider, "prd");
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
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
    configs.assert();
}

/// The project is ours and lists configs, but none named `prd` is a
/// root -- the environment itself was deleted by hand, or by an earlier
/// step of this same teardown (decision 6's resume case). `Gone`, never
/// `NotOurs`: an already-gone resource must never refuse the whole
/// teardown.
#[test]
fn standing_reports_gone_when_the_environment_is_not_listed() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[
            serde_json::json!({"name": "dev", "root": true, "environment": "dev", "created_at": "2026-01-01T00:05:00Z"}),
        ],
    );
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    configs.assert();
}

/// A response that omits the root entry's own `created_at` outright
/// fails closed: never "in the window", always `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_at_is_missing() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "prd", "root": true, "environment": "prd"})],
    );
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
    mock_configs(&mut provider, &[root_entry("2025-12-31T23:59:59Z")]);
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
    mock_configs(&mut provider, &[root_entry("2026-01-01T00:10:01Z")]);
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
fn standing_reports_as_created_for_a_fresh_environment_with_only_its_root() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_fresh_environment(&mut provider);
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
    mock_configs(&mut provider, &[root_entry("2026-01-01T00:10:00Z")]);
    mock_clean_config(&mut provider, "prd");
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

/// A config in `dev` is never consulted while rolling back `prd`'s own
/// root: the membership walk is scoped to this environment alone.
#[test]
fn standing_never_reads_a_config_in_another_environment() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[
            root_entry("2026-01-01T00:05:00Z"),
            serde_json::json!({"name": "dev", "root": true, "environment": "dev", "created_at": "2020-01-01T00:00:00Z"}),
        ],
    );
    mock_clean_config(&mut provider, "prd");
    let dev_config = provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("dev"))
        .expect(0)
        .create();
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
    dev_config.assert();
}

// ---------------------------------------------------------------------
// Drifted
// ---------------------------------------------------------------------

#[test]
fn standing_reports_drifted_on_a_foreign_branch_config_in_the_environment() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[
            root_entry("2026-01-01T00:05:00Z"),
            serde_json::json!({"name": "prd_hand_set", "root": false, "environment": "prd", "created_at": "2026-01-01T00:05:00Z"}),
        ],
    );
    // Membership is checked before contents (trust boundary 6): a config
    // this run did not create refuses before its own secret names,
    // inheritedBy or tokens are ever read.
    let config_get = provider
        .mock("GET", "/v3/configs/config")
        .expect(0)
        .create();
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
    config_get.assert();
}

/// A branch config this run's own rider `doppler.branch_config.ensure`
/// created is tolerated.
#[test]
fn standing_reports_as_created_when_a_branch_config_is_a_tolerated_rider() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(
        &mut provider,
        &[
            root_entry("2026-01-01T00:05:00Z"),
            serde_json::json!({"name": "prd_deployment", "root": false, "environment": "prd", "created_at": "2026-01-01T00:05:00Z"}),
        ],
    );
    mock_clean_config(&mut provider, "prd");
    mock_clean_config(&mut provider, "prd_deployment");
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let rider_instance = InstanceRef {
        node: NodeName::parse("branch").unwrap(),
        instance: None,
    };
    let rider_inputs = Inputs::new();
    let mut rider_outputs = Outputs::new();
    rider_outputs.insert(
        PortName::parse("config").unwrap(),
        Value::known(DopplerConfig::new(
            project(),
            DopplerConfigName::parse("prd_deployment").unwrap(),
        )),
    );
    let riders = [Rider {
        tool: "doppler.branch_config.ensure",
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
fn standing_reports_drifted_on_a_hand_set_secret() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[root_entry("2026-01-01T00:05:00Z")]);
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd"))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": true, "inherits": [], "inheritedBy": []}})
                .to_string(),
        )
        .create();
    mock_names(&mut provider, "prd", &["HAND_SET"]);
    let tokens = provider
        .mock("GET", "/v3/configs/config/tokens")
        .expect(0)
        .create();
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
    tokens.assert();
}

#[test]
fn standing_reports_drifted_when_inherited_by_is_missing() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[root_entry("2026-01-01T00:05:00Z")]);
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd"))
        .with_status(200)
        .with_body(serde_json::json!({"config": {"root": true, "inherits": []}}).to_string())
        .create();
    mock_names(&mut provider, "prd", &[]);
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

#[test]
fn standing_reports_drifted_when_inherited_by_names_a_config_outside_this_project() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &managed_project_body());
    mock_configs(&mut provider, &[root_entry("2026-01-01T00:05:00Z")]);
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd"))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "config": {
                    "root": true,
                    "inherits": [],
                    "inheritedBy": [{"project": "someone-elses-project", "config": "prd"}],
                }
            })
            .to_string(),
        )
        .create();
    mock_names(&mut provider, "prd", &[]);
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
    mock_configs(&mut provider, &[root_entry("2026-01-01T00:05:00Z")]);
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd"))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": true, "inherits": [], "inheritedBy": []}})
                .to_string(),
        )
        .create();
    mock_names(&mut provider, "prd", &[]);
    mock_tokens(&mut provider, "prd", &["ci"]);
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
        .mock("DELETE", "/v3/environments/environment")
        .expect(0)
        .create();
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
    mock_fresh_environment(&mut provider);
    let delete = provider
        .mock("DELETE", "/v3/environments/environment")
        .match_query(mockito::Matcher::AllOf(vec![
            project_query(),
            mockito::Matcher::UrlEncoded("environment".into(), "prd".into()),
        ]))
        .with_status(200)
        .with_body(serde_json::json!({"success": true}).to_string())
        .expect(1)
        .create();
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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
            root_entry("2026-01-01T00:05:00Z"),
            serde_json::json!({"name": "prd_hand_set", "root": false, "environment": "prd", "created_at": "2026-01-01T00:05:00Z"}),
        ],
    );
    let delete = provider
        .mock("DELETE", "/v3/environments/environment")
        .expect(0)
        .create();
    let tool = DopplerConfigEnsure::new(client_against(provider.url()));
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

/// Decision 4's reversal declaration: `Delete`, riding only with
/// `doppler.project.ensure`'s own `project` input, no loss (acceptance
/// 1's own validation already proves this against `Catalog::insert`;
/// `catalog_check_parity.rs` does so for this crate).
#[test]
fn reversal_declares_delete_within_project_ensure_only() {
    let tool = DopplerConfigEnsure::new(client_against("http://127.0.0.1:1".to_string()));
    let willikins_core::Reversal::Delete {
        needs,
        within,
        losses,
    } = Tool::reversal(&tool)
    else {
        panic!("doppler.config.ensure must declare Reversal::Delete");
    };
    assert_eq!(needs.to_vec(), vec!["project", "environment"]);
    assert_eq!(
        within,
        &[Within {
            tool: "doppler.project.ensure",
            ports: &[("project", Side::Input, "project")],
        }]
    );
    assert!(losses.is_empty());
}
