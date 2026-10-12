//! Milestone 3m task P2: mock tests for `doppler.project.ensure`'s
//! inverse (decisions 4, 7; acceptance 12). Each test pins the exact
//! query the inverse issues and, where decision 7's own table refuses
//! before a later read, that the later read was never issued at all
//! (trust boundary 6: "all reads before any delete").

use std::sync::Arc;

use willikins_core::rollback::{CreationWindow, Rider};
use willikins_core::{
    Created, Inputs, InstanceRef, NodeName, Outputs, PortName, Standing, Timestamp, Tool,
    ToolErrorKind, Value,
};
use willikins_providers_doppler::{DopplerClient, DopplerProjectEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{
    DomainType, DopplerConfig, DopplerConfigName, DopplerProject, DopplerTokenName, SecretName,
    TeardownToken,
};

fn client_against(url: String) -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    Arc::new(DopplerClient::new(Http::new(url, Vec::new(), credential)))
}

fn project() -> DopplerProject {
    DopplerProject::parse("shared_keys").unwrap()
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
    inputs
}

/// A [`Created`] over `inputs`/`riders`, the fixed [`window`]/[`settled_by`],
/// and no accepted loss (`doppler.project.ensure` declares none at all).
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

fn standing(tool: &DopplerProjectEnsure, created: &Created<'_>) -> Standing {
    Tool::inverse(tool)
        .expect("doppler.project.ensure declares Delete, so inverse() is Some")
        .standing(created)
        .expect("standing never fails against a well-formed mock response")
}

/// A project body at `created_at`, carrying the ownership marker — the
/// `AsCreated` case before any of the three listing reads.
fn project_body(created_at: &str) -> serde_json::Value {
    serde_json::json!({
        "project": {
            "description": "managed-by: willikins",
            "created_at": created_at,
        }
    })
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

fn mock_environments(provider: &mut MockProvider, ids: &[&str]) {
    let environments: Vec<_> = ids.iter().map(|id| serde_json::json!({"id": id})).collect();
    provider
        .mock("GET", "/v3/environments")
        .match_query(mockito::Matcher::AllOf(vec![
            project_query(),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
        ]))
        .with_status(200)
        .with_body(serde_json::json!({"environments": environments, "page": 1}).to_string())
        .create();
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
/// secret names, no foreign tokens — the shape every default root config
/// has on a fresh project.
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

fn mock_default_environments_and_roots(provider: &mut MockProvider) {
    mock_environments(provider, &["dev", "stg", "prd"]);
    mock_configs(
        provider,
        &[
            serde_json::json!({"name": "dev", "root": true, "environment": "dev"}),
            serde_json::json!({"name": "stg", "root": true, "environment": "stg"}),
            serde_json::json!({"name": "prd", "root": true, "environment": "prd"}),
        ],
    );
    mock_clean_config(provider, "dev");
    mock_clean_config(provider, "stg");
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
    let environments = provider.mock("GET", "/v3/environments").expect(0).create();
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
    environments.assert();
}

/// Every run after the first gets a `400` naming "no access" for a
/// genuinely missing project (`looks_like_a_missing_project`'s own doc):
/// read as `Gone` here too, the same under-deletion-safe reading every
/// other Doppler read in this crate already gives it.
#[test]
fn standing_reports_gone_on_a_400_naming_no_access() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        400,
        &serde_json::json!({"messages": ["This token does not have access to requested project 'shared_keys'"]}),
    );
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Gone
    );
}

// ---------------------------------------------------------------------
// NotOurs (decision 7's project row)
// ---------------------------------------------------------------------

#[test]
fn standing_reports_not_ours_when_the_marker_is_gone() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        200,
        &serde_json::json!({"project": {"description": "some other note", "created_at": "2026-01-01T00:05:00Z"}}),
    );
    let environments = provider.mock("GET", "/v3/environments").expect(0).create();
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
    environments.assert();
}

#[test]
fn standing_reports_not_ours_when_the_description_is_null() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        200,
        &serde_json::json!({"project": {"description": null, "created_at": "2026-01-01T00:05:00Z"}}),
    );
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

/// A response that omits `created_at` outright fails closed: never "in
/// the window", always `NotOurs`.
#[test]
fn standing_reports_not_ours_when_created_at_is_missing() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        200,
        &serde_json::json!({"project": {"description": "managed-by: willikins"}}),
    );
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
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
    mock_project(&mut provider, 200, &project_body("2025-12-31T23:59:59Z"));
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
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
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:10:01Z"));
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::NotOurs { .. }
    ));
}

// ---------------------------------------------------------------------
// AsCreated: the fresh-project happy path, and the inclusive window edge
// ---------------------------------------------------------------------

/// Doppler's own millisecond shape (`2019-11-19T07:19:00.476Z`-style
/// fractional seconds) parses the same as a whole-second one (verify
/// item 6's mock-level answer).
#[test]
fn standing_reports_as_created_for_a_fresh_project_with_only_defaults() {
    let mut provider = MockProvider::start();
    mock_project(
        &mut provider,
        200,
        &project_body("2026-01-01T00:05:00.476Z"),
    );
    mock_default_environments_and_roots(&mut provider);
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

/// The same instant as the window's own end is still inside it (an
/// inclusive window).
#[test]
fn standing_reports_as_created_when_created_at_is_exactly_the_windows_end() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:10:00Z"));
    mock_default_environments_and_roots(&mut provider);
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::AsCreated { losing: Vec::new() }
    );
}

// ---------------------------------------------------------------------
// Drifted: foreign environment / config
// ---------------------------------------------------------------------

#[test]
fn standing_reports_drifted_on_a_foreign_environment() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd", "qa"]);
    let configs = provider.mock("GET", "/v3/configs").expect(0).create();
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
    configs.assert();
}

#[test]
fn standing_reports_drifted_on_a_foreign_non_root_config() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd"]);
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "dev_hand_set", "root": false, "environment": "dev"})],
    );
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

/// A config name Doppler's own grammar allows but this crate's own
/// [`DopplerConfigName`] refuses (a space) fails closed as `Drifted`,
/// never an opaque parse error.
#[test]
fn standing_reports_drifted_on_a_config_name_this_crate_cannot_parse() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd"]);
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "has a space", "root": true, "environment": "dev"})],
    );
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

// ---------------------------------------------------------------------
// Drifted: secrets, inheritedBy, tokens
// ---------------------------------------------------------------------

#[test]
fn standing_reports_drifted_on_a_hand_set_secret() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd"]);
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "prd", "root": true, "environment": "prd"})],
    );
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
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
    tokens.assert();
}

/// A secret this run's own rider `doppler.secret.set` wrote is tolerated.
#[test]
fn standing_reports_as_created_when_the_secret_is_a_tolerated_rider() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd"]);
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "prd", "root": true, "environment": "prd"})],
    );
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd"))
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"root": true, "inherits": [], "inheritedBy": []}})
                .to_string(),
        )
        .create();
    mock_names(&mut provider, "prd", &["DOPPLER_TOKEN"]);
    mock_tokens(&mut provider, "prd", &[]);
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
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
            DopplerConfigName::parse("prd").unwrap(),
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
fn standing_reports_drifted_when_inherited_by_names_a_config_outside_this_project() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd"]);
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "prd", "root": true, "environment": "prd"})],
    );
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
    let tokens = provider
        .mock("GET", "/v3/configs/config/tokens")
        .expect(0)
        .create();
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
    tokens.assert();
}

/// A response that omits `inheritedBy` outright fails closed: never "no
/// opinion", always `Drifted`.
#[test]
fn standing_reports_drifted_when_inherited_by_is_missing() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd"]);
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "prd", "root": true, "environment": "prd"})],
    );
    provider
        .mock("GET", "/v3/configs/config")
        .match_query(config_query("prd"))
        .with_status(200)
        .with_body(serde_json::json!({"config": {"root": true, "inherits": []}}).to_string())
        .create();
    mock_names(&mut provider, "prd", &[]);
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
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
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd"]);
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "prd", "root": true, "environment": "prd"})],
    );
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
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    assert!(matches!(
        standing(&tool, &created(&inputs, &outputs, &[])),
        Standing::Drifted { .. }
    ));
}

/// A token this run's own rider `doppler.service_token.ensure` minted is
/// tolerated.
#[test]
fn standing_reports_as_created_when_the_token_is_a_tolerated_rider() {
    let mut provider = MockProvider::start();
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd"]);
    mock_configs(
        &mut provider,
        &[serde_json::json!({"name": "prd", "root": true, "environment": "prd"})],
    );
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
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
    let inputs = created_inputs();
    let outputs = Outputs::new();
    let rider_instance = InstanceRef {
        node: NodeName::parse("token").unwrap(),
        instance: None,
    };
    let mut rider_inputs = Inputs::new();
    rider_inputs.insert(
        PortName::parse("config").unwrap(),
        Value::known(DopplerConfig::new(
            project(),
            DopplerConfigName::parse("prd").unwrap(),
        )),
    );
    rider_inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(DopplerTokenName::parse("ci").unwrap()),
    );
    let rider_outputs = Outputs::new();
    let riders = [Rider {
        tool: "doppler.service_token.ensure",
        instance: &rider_instance,
        inputs: &rider_inputs,
        outputs: &rider_outputs,
    }];
    assert_eq!(
        standing(&tool, &created(&inputs, &outputs, &riders)),
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
    mock_project(
        &mut provider,
        404,
        &serde_json::json!({"messages": ["Could not find requested project"]}),
    );
    let delete = provider
        .mock("DELETE", "/v3/projects/project")
        .expect(0)
        .create();
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
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
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_default_environments_and_roots(&mut provider);
    let delete = provider
        .mock("DELETE", "/v3/projects/project")
        .match_body(willikins_providers_http::testing::json_body(
            serde_json::json!({"project": "shared_keys"}),
        ))
        .with_status(200)
        .with_body(serde_json::json!({"success": true}).to_string())
        .expect(1)
        .create();
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
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
    mock_project(&mut provider, 200, &project_body("2026-01-01T00:05:00Z"));
    mock_environments(&mut provider, &["dev", "stg", "prd", "qa"]);
    let delete = provider
        .mock("DELETE", "/v3/projects/project")
        .expect(0)
        .create();
    let tool = DopplerProjectEnsure::new(client_against(provider.url()));
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
    let tool = DopplerProjectEnsure::new(client_against("http://127.0.0.1:1".to_string()));
    let willikins_core::Reversal::Delete {
        needs,
        within,
        losses,
    } = Tool::reversal(&tool)
    else {
        panic!("doppler.project.ensure must declare Reversal::Delete");
    };
    assert_eq!(needs.to_vec(), vec!["project"]);
    assert!(within.is_empty());
    assert!(losses.is_empty());
}
