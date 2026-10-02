//! `doppler.project_member.ensure`'s own mock-server tests: one test per
//! row of the milestone 3h plan's decision (a) table, plus the extra
//! cases acceptance 4 names (zero/two name matches, a missing project,
//! shape refusals before any request, the `PATCH`'s sorted union, a
//! failed write whose re-read is `Present`, and that outputs never carry
//! a slug).
//!
//! Every write mock pins the exact path and body (the 3e lesson); every
//! list mock matches any query, since this tool's own paging is already
//! pinned by `project_member_client_mock.rs` and is not this file's
//! concern.

use std::sync::Arc;

use willikins_core::{Inputs, Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerProjectMemberEnsure};
use willikins_providers_http::testing::{MockProvider, json_body};
use willikins_providers_http::{Credential, Http};
use willikins_types::{
    DomainType, DopplerProject, DopplerProjectRole, DopplerServiceAccountName, EnvironmentSlug,
};

const SLUG: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

fn client_against(url: String) -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new(url, Vec::new(), credential);
    Arc::new(DopplerClient::new(http))
}

fn project() -> DopplerProject {
    DopplerProject::parse("walter").unwrap()
}

fn service_account() -> DopplerServiceAccountName {
    DopplerServiceAccountName::parse("buildkite-ci").unwrap()
}

fn viewer() -> DopplerProjectRole {
    DopplerProjectRole::parse("viewer").unwrap()
}

fn collaborator() -> DopplerProjectRole {
    DopplerProjectRole::parse("collaborator").unwrap()
}

fn prd() -> EnvironmentSlug {
    EnvironmentSlug::parse("prd").unwrap()
}

fn stg() -> EnvironmentSlug {
    EnvironmentSlug::parse("stg").unwrap()
}

fn dev() -> EnvironmentSlug {
    EnvironmentSlug::parse("dev").unwrap()
}

fn inputs(role: &DopplerProjectRole, environments: &[EnvironmentSlug]) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
    inputs.insert(
        PortName::parse("service_account").unwrap(),
        Value::known(service_account()),
    );
    inputs.insert(PortName::parse("role").unwrap(), Value::known(role.clone()));
    inputs.insert(
        PortName::parse("environments").unwrap(),
        Value::known_list(environments.to_vec()),
    );
    inputs
}

fn service_account_json(name: &str, slug: &str) -> serde_json::Value {
    serde_json::json!({"name": name, "slug": slug})
}

fn member_json(
    member_type: &str,
    slug: &str,
    role: &str,
    access_all_environments: bool,
    environments: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "type": member_type,
        "slug": slug,
        "role": {"identifier": role},
        "access_all_environments": access_all_environments,
        "environments": environments,
    })
}

fn mock_accounts(provider: &mut MockProvider, accounts: &[serde_json::Value]) {
    provider
        .mock("GET", "/v3/workplace/service_accounts")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"service_accounts": accounts}).to_string())
        .create();
}

fn mock_members(provider: &mut MockProvider, members: &[serde_json::Value]) {
    provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"members": members}).to_string())
        .create();
}

fn tool(url: String) -> DopplerProjectMemberEnsure {
    DopplerProjectMemberEnsure::new(client_against(url))
}

#[test]
fn spec_validates_against_the_registry() {
    tool("http://127.0.0.1:1".to_string())
        .spec()
        .validate(willikins_types::registry())
        .unwrap();
}

// ---------------------------------------------------------------------
// Shape, before any request
// ---------------------------------------------------------------------

#[test]
fn empty_environments_is_invalid_before_any_request() {
    // No mock at all: a request here would fail the test with "no mock
    // matched" before the assertion even runs, which already proves
    // nothing was sent.
    let err = tool("http://127.0.0.1:1".to_string())
        .read(&inputs(&viewer(), &[]))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
}

#[test]
fn duplicate_environments_is_invalid_before_any_request() {
    let err = tool("http://127.0.0.1:1".to_string())
        .read(&inputs(&viewer(), &[prd(), prd()]))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
}

#[test]
fn seventeen_environments_is_invalid_before_any_request() {
    let many: Vec<EnvironmentSlug> = (0..17)
        .map(|i| EnvironmentSlug::parse(&format!("env{i}")).unwrap())
        .collect();
    let err = tool("http://127.0.0.1:1".to_string())
        .read(&inputs(&viewer(), &many))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Invalid);
}

// ---------------------------------------------------------------------
// Name resolution
// ---------------------------------------------------------------------

#[test]
fn zero_name_matches_is_not_found() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[]);
    let err = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

#[test]
fn two_name_matches_is_conflict() {
    let mut provider = MockProvider::start();
    mock_accounts(
        &mut provider,
        &[
            service_account_json("buildkite-ci", "11111111-1111-1111-1111-111111111111"),
            service_account_json("buildkite-ci", "22222222-2222-2222-2222-222222222222"),
        ],
    );
    let err = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
}

#[test]
fn only_a_byte_for_byte_name_match_resolves() {
    // Near misses -- another case, a suffix, a prefix -- never resolve:
    // listed alone they leave the name `NotFound`, and listed beside the
    // exact name they never make it ambiguous.
    let near_misses = [
        service_account_json("Buildkite-CI", "11111111-1111-1111-1111-111111111111"),
        service_account_json("buildkite-ci-old", "22222222-2222-2222-2222-222222222222"),
        service_account_json("old-buildkite-ci", "33333333-3333-3333-3333-333333333333"),
    ];
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &near_misses);
    let err = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);

    let mut provider = MockProvider::start();
    let mut accounts = near_misses.to_vec();
    accounts.push(service_account_json("buildkite-ci", SLUG));
    mock_accounts(&mut provider, &accounts);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["prd"],
        )],
    );
    let observation = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

#[test]
fn a_member_of_another_type_with_the_same_slug_is_not_this_service_account() {
    // Only a `service_account` entry is this member: a workplace user or
    // group listed under the same slug (here as an admin, which would read
    // `Mismatch`) is someone else, so the service account reads `Absent`.
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json("workplace_user", SLUG, "admin", true, &[])],
    );
    let t = tool(provider.url());
    assert!(matches!(
        t.read(&inputs(&viewer(), &[prd()])).unwrap(),
        Observation::Absent { .. }
    ));
    assert!(!t.updates(&inputs(&viewer(), &[prd()])).unwrap());
}

// ---------------------------------------------------------------------
// decision (a)'s table
// ---------------------------------------------------------------------

#[test]
fn a_missing_project_is_absent() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(404)
        .with_body(r#"{"messages": ["Could not find requested project"]}"#)
        .create();
    let observation = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

#[test]
fn absent_member_is_absent_and_updates_is_false() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(&mut provider, &[]);
    let t = tool(provider.url());
    assert!(matches!(
        t.read(&inputs(&viewer(), &[prd()])).unwrap(),
        Observation::Absent { .. }
    ));
    assert!(!t.updates(&inputs(&viewer(), &[prd()])).unwrap());
}

#[test]
fn role_matches_and_covers_requested_environments_is_present() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["prd"],
        )],
    );
    let observation = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

#[test]
fn access_all_environments_is_present_regardless_of_the_requested_list() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json("service_account", SLUG, "viewer", true, &[])],
    );
    let observation = tool(provider.url())
        .read(&inputs(&viewer(), &[prd(), stg()]))
        .unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

#[test]
fn role_matches_but_an_environment_is_missing_needs_update() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["prd"],
        )],
    );
    let t = tool(provider.url());
    assert!(matches!(
        t.read(&inputs(&viewer(), &[prd(), stg()])).unwrap(),
        Observation::Absent { .. }
    ));
    assert!(t.updates(&inputs(&viewer(), &[prd(), stg()])).unwrap());
}

#[test]
fn role_ranks_below_requested_needs_update() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["prd"],
        )],
    );
    let t = tool(provider.url());
    assert!(matches!(
        t.read(&inputs(&collaborator(), &[prd()])).unwrap(),
        Observation::Absent { .. }
    ));
    assert!(t.updates(&inputs(&collaborator(), &[prd()])).unwrap());
}

#[test]
fn role_ranks_above_requested_is_mismatch() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "collaborator",
            false,
            &["prd"],
        )],
    );
    let observation = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap();
    let Observation::Mismatch { port } = observation else {
        panic!("expected Mismatch, got {observation:?}");
    };
    assert_eq!(port.as_str(), "role");
}

#[test]
fn an_unrankable_role_is_mismatch() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "admin",
            false,
            &["prd"],
        )],
    );
    let observation = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap();
    assert!(matches!(observation, Observation::Mismatch { .. }));
}

/// Extra environments the member already has are never a mismatch.
#[test]
fn extra_environments_already_held_are_never_a_mismatch() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["dev", "prd", "stg"],
        )],
    );
    let observation = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

// ---------------------------------------------------------------------
// ensure
// ---------------------------------------------------------------------

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_absent_posts_and_reports_changed() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(&mut provider, &[]);
    let post = provider
        .mock("POST", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .match_body(json_body(serde_json::json!({
            "type": "service_account",
            "slug": SLUG,
            "role": "viewer",
            "environments": ["prd"],
        })))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();
    let ensured = tool(provider.url())
        .ensure(&inputs(&viewer(), &[prd()]), &SinkToken::new())
        .unwrap();
    assert!(ensured.changed);
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_present_reports_unchanged_and_makes_no_write() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["prd"],
        )],
    );
    let post = provider
        .mock("POST", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let patch = provider
        .mock(
            "PATCH",
            "/v3/projects/project/members/member/service_account/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        )
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let ensured = tool(provider.url())
        .ensure(&inputs(&viewer(), &[prd()]), &SinkToken::new())
        .unwrap();
    assert!(!ensured.changed);
    post.assert();
    patch.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_needs_update_patches_the_sorted_union_and_never_deletes() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["dev", "prd"],
        )],
    );
    let patch = provider
        .mock(
            "PATCH",
            "/v3/projects/project/members/member/service_account/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        )
        .match_query(mockito::Matcher::Any)
        .match_body(json_body(serde_json::json!({
            "role": "viewer",
            "environments": ["dev", "prd", "stg"],
        })))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();
    let ensured = tool(provider.url())
        .ensure(
            &inputs(&viewer(), &[prd(), stg(), dev()]),
            &SinkToken::new(),
        )
        .unwrap();
    assert!(ensured.changed);
    patch.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_keeps_an_environment_the_request_never_named() {
    // The member holds `dev`; the request names only `prd`. The `PATCH`
    // must still carry `dev` -- a request-only list would narrow access.
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["dev"],
        )],
    );
    let patch = provider
        .mock(
            "PATCH",
            "/v3/projects/project/members/member/service_account/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        )
        .match_query(mockito::Matcher::Any)
        .match_body(json_body(serde_json::json!({
            "role": "viewer",
            "environments": ["dev", "prd"],
        })))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();
    let ensured = tool(provider.url())
        .ensure(&inputs(&viewer(), &[prd()]), &SinkToken::new())
        .unwrap();
    assert!(ensured.changed);
    patch.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_needs_update_from_a_lower_role_omits_environments_when_access_all_is_set() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json("service_account", SLUG, "viewer", true, &[])],
    );
    let patch = provider
        .mock(
            "PATCH",
            "/v3/projects/project/members/member/service_account/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        )
        .match_query(mockito::Matcher::Any)
        .match_body(json_body(serde_json::json!({"role": "collaborator"})))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();
    let ensured = tool(provider.url())
        .ensure(&inputs(&collaborator(), &[prd()]), &SinkToken::new())
        .unwrap();
    assert!(ensured.changed);
    patch.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_mismatch_is_conflict_and_makes_no_write() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "collaborator",
            false,
            &["prd"],
        )],
    );
    let post = provider
        .mock("POST", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let err = tool(provider.url())
        .ensure(&inputs(&viewer(), &[prd()]), &SinkToken::new())
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    post.assert();
}

/// No `DELETE` is ever recorded: nothing in this tool's `ensure` ever
/// calls a delete endpoint, over every row, including `Mismatch`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_never_issues_a_delete() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "collaborator",
            false,
            &["prd"],
        )],
    );
    let delete = provider
        .mock(
            "DELETE",
            "/v3/projects/project/members/member/service_account/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        )
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let err = tool(provider.url())
        .ensure(&inputs(&viewer(), &[prd()]), &SinkToken::new())
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    delete.assert();
}

/// An existing environment this crate cannot re-express as an
/// `EnvironmentSlug` (too long for its 16-character limit) is never
/// silently dropped from the `PATCH`'s union: `ensure` refuses instead
/// (decision (a) addendum, D3).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_rather_than_drops_an_unparseable_existing_environment() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["a-branch-config-nobody-asked-about"],
        )],
    );
    let patch = provider
        .mock(
            "PATCH",
            "/v3/projects/project/members/member/service_account/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        )
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create();
    let err = tool(provider.url())
        .ensure(&inputs(&viewer(), &[prd()]), &SinkToken::new())
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    patch.assert();
}

/// A failed write whose re-read finds the member already `Present`
/// reports `changed: false`, the original error never surfacing.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_failed_post_whose_reread_is_present_reports_changed_false() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    let first_members = provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"members": []}).to_string())
        .expect(1)
        .create();
    let post = provider
        .mock("POST", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(500)
        .with_body("{}")
        .expect(1)
        .create();
    let second_members = provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({
                "members": [member_json("service_account", SLUG, "viewer", false, &["prd"])],
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let ensured = tool(provider.url())
        .ensure(&inputs(&viewer(), &[prd()]), &SinkToken::new())
        .unwrap();
    assert!(!ensured.changed);
    first_members.assert();
    post.assert();
    second_members.assert();
}

#[test]
fn outputs_never_contain_a_slug() {
    let mut provider = MockProvider::start();
    mock_accounts(&mut provider, &[service_account_json("buildkite-ci", SLUG)]);
    mock_members(
        &mut provider,
        &[member_json(
            "service_account",
            SLUG,
            "viewer",
            false,
            &["prd"],
        )],
    );
    let observation = tool(provider.url())
        .read(&inputs(&viewer(), &[prd()]))
        .unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    assert_eq!(outputs.len(), 2);
    assert!(outputs.get(&PortName::parse("project").unwrap()).is_some());
    assert!(
        outputs
            .get(&PortName::parse("service_account").unwrap())
            .is_some()
    );
}
