//! Milestone 3h task D2: `DopplerClient`'s four new project-member
//! methods, exercised directly against a mock server (the tool that will
//! call them, `doppler.project_member.ensure`, is task D3 and does not
//! exist yet). See `client.rs`'s own doc on `list_service_accounts` for
//! why these methods are `pub` rather than `pub(crate)`.
//!
//! Every mock here pins the exact path and query string (the 3e lesson:
//! a mock that accepts any query hid a live-breaking change), except the
//! two "refuses past 50 pages" tests, which mirror
//! `willikins_providers_buildkite`'s own
//! `eleven_full_pages_is_a_provider_error_naming_the_page_bound` in using
//! `Matcher::Any` for the one case where the whole point is "every page
//! looks the same".

use std::sync::Arc;

use willikins_providers_doppler::{DopplerClient, DopplerSlug, LIST_MAX_PAGES, LIST_PER_PAGE};
use willikins_providers_http::testing::{MockProvider, json_body};
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, DopplerProject, DopplerProjectRole, EnvironmentSlug};

/// A distinctive marker: if it ever showed up in a surfaced error
/// message, trust boundary 5 (a message is never built from a provider's
/// raw body) broke for one of these four methods.
const MARKER: &str = "wlkn-test-marker-9fq3lxr0b2e";

fn client_against(url: String) -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new(url, Vec::new(), credential);
    Arc::new(DopplerClient::new(http))
}

fn project() -> DopplerProject {
    DopplerProject::parse("third-thoughts").unwrap()
}

fn slug(value: &str) -> DopplerSlug {
    DopplerSlug::try_from(value.to_string()).unwrap()
}

fn role(value: &str) -> DopplerProjectRole {
    DopplerProjectRole::parse(value).unwrap()
}

fn env(value: &str) -> EnvironmentSlug {
    EnvironmentSlug::parse(value).unwrap()
}

fn service_account_json(name: &str, slug: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "slug": slug,
        "created_at": "2026-10-01T00:00:00.000Z",
        "workplace_role": {
            "name": "Custom",
            "permissions": ["team"],
            "identifier": "custom",
            "created_at": "2026-10-01T00:00:00.000Z",
            "is_custom_role": false,
            "is_inline_role": false,
        },
    })
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

// ---------------------------------------------------------------------
// list_service_accounts
// ---------------------------------------------------------------------

#[test]
fn list_service_accounts_returns_every_entry_on_a_single_short_page() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/workplace/service_accounts")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "service_accounts": [
                    service_account_json("buildkite-ci", "11111111-1111-1111-1111-111111111111"),
                    service_account_json("Buildkite CI", "22222222-2222-2222-2222-222222222222"),
                ],
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());
    let accounts = client.list_service_accounts().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0].name, "buildkite-ci");
    assert_eq!(
        accounts[0].slug.to_string(),
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(accounts[1].name, "Buildkite CI");
}

#[test]
fn list_service_accounts_pages_past_a_full_first_page() {
    let mut provider = MockProvider::start();
    let full_page: Vec<serde_json::Value> = (0..100)
        .map(|i| service_account_json("sa", &format!("00000000-0000-0000-0000-{i:012}")))
        .collect();
    let page_one = provider
        .mock("GET", "/v3/workplace/service_accounts")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(serde_json::json!({"service_accounts": full_page}).to_string())
        .expect(1)
        .create();
    let page_two = provider
        .mock("GET", "/v3/workplace/service_accounts")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("page".into(), "2".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "service_accounts": [service_account_json(
                    "buildkite-ci",
                    "33333333-3333-3333-3333-333333333333"
                )],
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());
    let accounts = client.list_service_accounts().unwrap();
    assert_eq!(accounts.len(), 101);
    page_one.assert();
    page_two.assert();
}

#[test]
fn list_service_accounts_refuses_past_fifty_pages() {
    let mut provider = MockProvider::start();
    let full_page: Vec<serde_json::Value> = (0..100)
        .map(|i| service_account_json("sa", &format!("00000000-0000-0000-0000-{i:012}")))
        .collect();
    let mock = provider
        .mock("GET", "/v3/workplace/service_accounts")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"service_accounts": full_page}).to_string())
        .expect(LIST_MAX_PAGES as usize)
        .create();
    let client = client_against(provider.url());
    let err = client.list_service_accounts().unwrap_err();
    assert!(
        err.message
            .contains(&(LIST_MAX_PAGES * LIST_PER_PAGE).to_string()),
        "{}",
        err.message
    );
    mock.assert();
}

#[test]
fn list_service_accounts_403_maps_to_the_fixed_message_and_never_echoes_the_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/workplace/service_accounts")
        .match_query(mockito::Matcher::Any)
        .with_status(403)
        .with_body(serde_json::json!({"messages": [MARKER]}).to_string())
        .create();
    let client = client_against(provider.url());
    let err = client.list_service_accounts().unwrap_err();
    assert_eq!(err.status, Some(403));
    assert!(
        err.message.contains("View Service Accounts"),
        "{}",
        err.message
    );
    assert!(err.message.contains("service_accounts"), "{}", err.message);
    assert!(!err.message.contains(MARKER), "{}", err.message);
}

// ---------------------------------------------------------------------
// list_project_members
// ---------------------------------------------------------------------

#[test]
fn list_project_members_returns_every_entry_on_a_single_short_page() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("project".into(), "third-thoughts".into()),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "members": [member_json(
                    "service_account",
                    "11111111-1111-1111-1111-111111111111",
                    "viewer",
                    false,
                    &["prd"],
                )],
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());
    let members = client.list_project_members(&project()).unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].member_type, "service_account");
    assert_eq!(
        members[0].slug.to_string(),
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(members[0].role, "viewer");
    assert!(!members[0].access_all_environments);
    assert_eq!(members[0].environments, vec!["prd".to_string()]);
}

/// Decision (a)'s "unrankable role" and "extra environments ... left
/// alone" rows require a listing to parse successfully even when a
/// member's role is `admin`/`owner`/a custom identifier, or an
/// environment is outside this tool's own grammar -- a typed
/// `DopplerProjectRole`/`EnvironmentSlug` would refuse exactly the values
/// `read` must still classify, so this client deliberately keeps both as
/// bare strings.
#[test]
fn list_project_members_parses_an_unrankable_role_and_a_foreign_environment() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({
                "members": [
                    member_json(
                        "service_account",
                        "11111111-1111-1111-1111-111111111111",
                        "admin",
                        false,
                        &["a-branch-config-nobody-asked-about"],
                    ),
                    member_json(
                        "workplace_user",
                        "22222222-2222-2222-2222-222222222222",
                        "collaborator",
                        true,
                        &[],
                    ),
                ],
            })
            .to_string(),
        )
        .create();
    let client = client_against(provider.url());
    let members = client.list_project_members(&project()).unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].role, "admin");
    assert_eq!(
        members[0].environments,
        vec!["a-branch-config-nobody-asked-about".to_string()]
    );
    assert_eq!(members[1].member_type, "workplace_user");
    assert!(members[1].access_all_environments);
}

/// Live, 2026-10-02 (the sandbox member cycle's step 4): Doppler lists a
/// member whose access spans every environment -- every project's own
/// creator, an admin -- with `"environments": null`, not `[]`. A listing
/// that carries one must still parse, its environments empty.
#[test]
fn list_project_members_parses_null_environments_on_an_all_environments_member() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("project".into(), "third-thoughts".into()),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "members": [{
                    "type": "service_account",
                    "slug": "22222222-2222-2222-2222-222222222222",
                    "role": { "identifier": "admin" },
                    "access_all_environments": true,
                    "environments": null,
                }],
                "success": true,
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());
    let members = client.list_project_members(&project()).unwrap();
    assert_eq!(members.len(), 1);
    assert!(members[0].access_all_environments);
    assert!(members[0].environments.is_empty());
}

#[test]
fn list_project_members_pages_past_a_full_first_page() {
    let mut provider = MockProvider::start();
    let full_page: Vec<serde_json::Value> = (0..100)
        .map(|i| {
            member_json(
                "service_account",
                &format!("00000000-0000-0000-0000-{i:012}"),
                "viewer",
                false,
                &["prd"],
            )
        })
        .collect();
    let page_one = provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("project".into(), "third-thoughts".into()),
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(serde_json::json!({"members": full_page}).to_string())
        .expect(1)
        .create();
    let page_two = provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("project".into(), "third-thoughts".into()),
            mockito::Matcher::UrlEncoded("page".into(), "2".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "members": [member_json(
                    "service_account",
                    "33333333-3333-3333-3333-333333333333",
                    "viewer",
                    false,
                    &["prd"],
                )],
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());
    let members = client.list_project_members(&project()).unwrap();
    assert_eq!(members.len(), 101);
    page_one.assert();
    page_two.assert();
}

#[test]
fn list_project_members_refuses_past_fifty_pages() {
    let mut provider = MockProvider::start();
    let full_page: Vec<serde_json::Value> = (0..100)
        .map(|i| {
            member_json(
                "service_account",
                &format!("00000000-0000-0000-0000-{i:012}"),
                "viewer",
                false,
                &["prd"],
            )
        })
        .collect();
    let mock = provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!({"members": full_page}).to_string())
        .expect(LIST_MAX_PAGES as usize)
        .create();
    let client = client_against(provider.url());
    let err = client.list_project_members(&project()).unwrap_err();
    assert!(
        err.message
            .contains(&(LIST_MAX_PAGES * LIST_PER_PAGE).to_string()),
        "{}",
        err.message
    );
    mock.assert();
}

#[test]
fn list_project_members_403_maps_to_the_fixed_message_and_never_echoes_the_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(403)
        .with_body(serde_json::json!({"messages": [MARKER]}).to_string())
        .create();
    let client = client_against(provider.url());
    let err = client.list_project_members(&project()).unwrap_err();
    assert_eq!(err.status, Some(403));
    assert!(err.message.contains("View Team"), "{}", err.message);
    assert!(err.message.contains("team"), "{}", err.message);
    assert!(err.message.contains("admin"), "{}", err.message);
    assert!(!err.message.contains(MARKER), "{}", err.message);
}

/// A missing project's `404` must pass through unchanged: `remap_403`
/// only ever touches a `403`, so `doppler.project_member.ensure`'s own
/// `looks_like_a_missing_project` tolerance (D3) still sees it.
#[test]
fn list_project_members_404_passes_through_unchanged() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(404)
        .with_body(r#"{"messages": ["Could not find requested project"]}"#)
        .create();
    let client = client_against(provider.url());
    let err = client.list_project_members(&project()).unwrap_err();
    assert_eq!(err.status, Some(404));
    assert!(
        err.message.contains("Could not find requested project"),
        "{}",
        err.message
    );
}

/// A member whose `slug` is not `[A-Za-z0-9_-]+` fails the whole page's
/// parse rather than being silently dropped or smuggling a `/`, `?`, or
/// `&` into a later path segment -- and the rejected value itself never
/// reaches the error message (`Http::finish` builds it from line/column
/// alone).
#[test]
fn list_project_members_refuses_a_malformed_slug_and_never_echoes_it() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!({
                "members": [member_json("service_account", "has a space", "viewer", false, &["prd"])],
            })
            .to_string(),
        )
        .create();
    let client = client_against(provider.url());
    let err = client.list_project_members(&project()).unwrap_err();
    assert!(!err.message.contains("has a space"), "{}", err.message);
}

// ---------------------------------------------------------------------
// add_project_member
// ---------------------------------------------------------------------

#[test]
fn add_project_member_sends_exactly_the_documented_body() {
    let mut provider = MockProvider::start();
    let post = provider
        .mock("POST", "/v3/projects/project/members")
        .match_query(mockito::Matcher::UrlEncoded(
            "project".into(),
            "third-thoughts".into(),
        ))
        .match_body(json_body(serde_json::json!({
            "type": "service_account",
            "slug": "11111111-1111-1111-1111-111111111111",
            "role": "viewer",
            "environments": ["prd"],
        })))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "member": member_json(
                    "service_account",
                    "11111111-1111-1111-1111-111111111111",
                    "viewer",
                    false,
                    &["prd"],
                ),
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());
    client
        .add_project_member(
            &project(),
            &slug("11111111-1111-1111-1111-111111111111"),
            &role("viewer"),
            &[env("prd")],
        )
        .unwrap();
    post.assert();
}

#[test]
fn add_project_member_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    let post = provider
        .mock("POST", "/v3/projects/project/members")
        .match_query(mockito::Matcher::Any)
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let client = client_against(provider.url());
    let err = client
        .add_project_member(
            &project(),
            &slug("11111111-1111-1111-1111-111111111111"),
            &role("viewer"),
            &[env("prd")],
        )
        .unwrap_err();
    assert_eq!(err.status, Some(503));
    post.assert();
}

// ---------------------------------------------------------------------
// update_project_member
// ---------------------------------------------------------------------

#[test]
fn update_project_member_with_role_alone_omits_environments_entirely() {
    let mut provider = MockProvider::start();
    let patch = provider
        .mock(
            "PATCH",
            "/v3/projects/project/members/member/service_account/11111111-1111-1111-1111-111111111111",
        )
        .match_query(mockito::Matcher::UrlEncoded(
            "project".into(),
            "third-thoughts".into(),
        ))
        .match_body(json_body(serde_json::json!({"role": "collaborator"})))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "member": member_json(
                    "service_account",
                    "11111111-1111-1111-1111-111111111111",
                    "collaborator",
                    true,
                    &[],
                ),
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());
    client
        .update_project_member(
            &project(),
            &slug("11111111-1111-1111-1111-111111111111"),
            &role("collaborator"),
            None,
        )
        .unwrap();
    patch.assert();
}

#[test]
fn update_project_member_with_environments_sends_the_full_set() {
    let mut provider = MockProvider::start();
    let patch = provider
        .mock(
            "PATCH",
            "/v3/projects/project/members/member/service_account/11111111-1111-1111-1111-111111111111",
        )
        .match_query(mockito::Matcher::UrlEncoded(
            "project".into(),
            "third-thoughts".into(),
        ))
        .match_body(json_body(serde_json::json!({
            "role": "viewer",
            "environments": ["prd", "stg"],
        })))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "member": member_json(
                    "service_account",
                    "11111111-1111-1111-1111-111111111111",
                    "viewer",
                    false,
                    &["prd", "stg"],
                ),
            })
            .to_string(),
        )
        .expect(1)
        .create();
    let client = client_against(provider.url());
    client
        .update_project_member(
            &project(),
            &slug("11111111-1111-1111-1111-111111111111"),
            &role("viewer"),
            Some(&[env("prd"), env("stg")]),
        )
        .unwrap();
    patch.assert();
}
