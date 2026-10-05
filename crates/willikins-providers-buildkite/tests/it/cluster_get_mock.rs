//! Acceptance test 6 for `buildkite.cluster.get`. The last section
//! (milestone 3e task K1) covers the tool's own optional `token` port.

use std::sync::{Arc, Mutex};

use willikins_core::{Observation, PortName, Tool, ToolErrorKind, Value};
use willikins_providers_buildkite::{BuildkiteClient, BuildkiteClusterGet, MAX_CLUSTER_PAGES};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{BuildkiteClusterName, BuildkiteOrg, DomainType};

fn client_against(url: String) -> Arc<BuildkiteClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    let http = Http::new(url, Vec::new(), credential);
    Arc::new(BuildkiteClient::new(http))
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("org").unwrap(),
        Value::known(BuildkiteOrg::parse("willikins-test").unwrap()),
    );
    inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(BuildkiteClusterName::parse("Default cluster").unwrap()),
    );
    inputs
}

fn cluster_json(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "graphql_id": "unused", "name": name, "default_queue_id": null})
}

#[test]
fn one_match_reports_present_with_the_id() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(
            serde_json::json!([cluster_json(
                "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "Default cluster"
            )])
            .to_string(),
        )
        .create();
    let tool = BuildkiteClusterGet::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    let cluster = outputs.get(&PortName::parse("cluster").unwrap()).unwrap();
    assert_eq!(
        cluster.render().to_string(),
        "018e5a22-d14c-7085-bb28-db0f83f43a1c"
    );
}

#[test]
fn zero_matches_is_not_found_naming_the_cluster_name() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!([cluster_json("some-id", "Other cluster")]).to_string())
        .create();
    let tool = BuildkiteClusterGet::new(client_against(provider.url()));
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert!(err.message.contains("Default cluster"), "{}", err.message);
}

#[test]
fn two_matches_is_conflict_naming_the_name_and_the_count_never_the_ids() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(
            serde_json::json!([
                cluster_json("018e5a22-d14c-7085-bb28-db0f83f43a1c", "Default cluster"),
                cluster_json("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "Default cluster"),
            ])
            .to_string(),
        )
        .create();
    let tool = BuildkiteClusterGet::new(client_against(provider.url()));
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("Default cluster"), "{}", err.message);
    assert!(err.message.contains('2'), "{}", err.message);
    assert!(
        !err.message.contains("018e5a22") && !err.message.contains("aaaaaaaa"),
        "the conflict must never name an id: {}",
        err.message
    );
}

#[test]
fn a_match_on_the_second_page_is_present_and_pages_with_page_and_per_page() {
    let mut provider = MockProvider::start();
    let full_page: Vec<serde_json::Value> = (0..100)
        .map(|i| cluster_json(&format!("00000000-0000-0000-0000-{i:012}"), "Some cluster"))
        .collect();
    let page_one = provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("Link", "<https://api.buildkite.com/v2/organizations/willikins-test/clusters?page=2&per_page=100&api_key=SHOULD-NEVER-BE-READ>; rel=\"next\"")
        .with_body(serde_json::json!(full_page).to_string())
        .expect(1)
        .create();
    let page_two = provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("page".into(), "2".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_body(
            serde_json::json!([cluster_json(
                "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "Default cluster"
            )])
            .to_string(),
        )
        .expect(1)
        .create();
    let tool = BuildkiteClusterGet::new(client_against(provider.url()));
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    page_one.assert();
    page_two.assert();
}

#[test]
fn eleven_full_pages_is_a_provider_error_naming_the_page_bound() {
    let mut provider = MockProvider::start();
    let full_page: Vec<serde_json::Value> = (0..100)
        .map(|i| cluster_json(&format!("00000000-0000-0000-0000-{i:012}"), "Some cluster"))
        .collect();
    let mock = provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!(full_page).to_string())
        .expect(MAX_CLUSTER_PAGES as usize)
        .create();
    let tool = BuildkiteClusterGet::new(client_against(provider.url()));
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    mock.assert();
}

#[test]
fn a_403_is_provider_naming_the_missing_permission_and_never_the_credential() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .with_status(403)
        .with_body(r#"{"message":"Forbidden"}"#)
        .create();
    let tool = BuildkiteClusterGet::new(client_against(provider.url()));
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.contains("permission"), "{}", err.message);
    assert!(!err.message.contains("bkua_"), "{}", err.message);
}

// ---------------------------------------------------------------------
// Acceptance test 5's `Link` half: the ignored pagination header
// ---------------------------------------------------------------------

/// Buildkite's own documented `Link` example embeds an `api_key` query
/// parameter (trust boundary 7), which is why this crate pages with
/// explicit `page`/`per_page` parameters and never reads, follows, or logs
/// `Link`. `a_match_on_the_second_page_is_present_and_pages_with_page_and_per_page`
/// already proves the *paging* is explicit — it would still pass if the
/// header's bytes leaked into an output or a message, because it never
/// looks. This marker is what makes the leak detectable.
///
/// **What this test is worth, stated plainly.** It cannot fail today:
/// `willikins_providers_http`'s `response_facts` reads three headers by
/// name (`Retry-After`, `x-ratelimit-remaining`, `x-ratelimit-reset`) and
/// `Link` is not one of them, so no header byte can reach a
/// `ProviderError` at all. It is a regression guard, not a discovery: the
/// milestone 3a plan's own risk list contemplates growing `response_facts`
/// to read Buildkite's `RateLimit-Reset` and `RateLimit-User-Reset`, and
/// this is what makes that change fail loudly if it ever generalises to
/// "record the response's headers" instead of two more named ones.
const LINK_API_KEY_MARKER: &str = "FIXTURE-LINK-API-KEY-DO-NOT-LEAK";

fn link_header() -> String {
    format!(
        "<https://api.buildkite.com/v2/organizations/willikins-test/clusters?page=2&per_page=100&api_key={LINK_API_KEY_MARKER}>; rel=\"next\""
    )
}

/// Every arm the tool can reach — one match, none, two, and a provider
/// error — served with that header. The marker may appear in no
/// observation, no rendered output, and no error message.
#[test]
fn the_ignored_link_headers_api_key_reaches_no_output_debug_or_error() {
    let one = serde_json::json!([cluster_json(
        "018e5a22-d14c-7085-bb28-db0f83f43a1c",
        "Default cluster"
    )]);
    let none = serde_json::json!([cluster_json("some-id", "Other cluster")]);
    let two = serde_json::json!([
        cluster_json("018e5a22-d14c-7085-bb28-db0f83f43a1c", "Default cluster"),
        cluster_json("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "Default cluster"),
    ]);

    for (case, status, body) in [
        ("one match", 200, one.to_string()),
        ("no match", 200, none.to_string()),
        ("two matches", 200, two.to_string()),
        ("provider error", 500, r#"{"message":"boom"}"#.to_string()),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/v2/organizations/willikins-test/clusters")
            .match_query(mockito::Matcher::Any)
            .with_status(status)
            .with_header("Link", &link_header())
            .with_body(body)
            .create();
        let tool = BuildkiteClusterGet::new(client_against(provider.url()));

        let surfaces: Vec<String> = match tool.read(&inputs()) {
            Ok(observation) => {
                let mut seen = vec![format!("{observation:?}")];
                if let Observation::Present(outputs) = &observation {
                    for (_, value) in outputs.iter() {
                        seen.push(value.render().to_string());
                        seen.push(format!("{value:?}"));
                    }
                }
                seen
            }
            Err(err) => vec![err.message.clone(), format!("{err:?}")],
        };

        assert!(!surfaces.is_empty(), "{case}: nothing was captured");
        for text in surfaces {
            assert!(
                !text.contains(LINK_API_KEY_MARKER),
                "{case}: the ignored Link header's api_key leaked into: {text}"
            );
        }
    }
}

// ---------------------------------------------------------------------
// Milestone 3e task K1: the optional `token` port
// ---------------------------------------------------------------------

/// The gap the milestone 3e adversarial pass over `github.repo.get`'s own
/// optional `token` port found and left open (finding 3, "no tool-level
/// mock pins that an unbound request carries the environment
/// credential's value"): a mutation that made the unbound path authorize
/// with a fixed, bogus credential instead of falling back to the tool's
/// own default survived every mock suite there. Closed here, proactively,
/// for Buildkite's own two tools: a `read` with no `token` port bound
/// must carry *this tool's own* default credential -- not a generic,
/// interchangeable test token, but the exact bytes this test built the
/// tool's default client with -- in its `Authorization` header, and no
/// other value.
#[test]
fn unbound_read_authorizes_with_the_tools_own_default_credential() {
    let mut provider = MockProvider::start();
    let captured = Arc::new(Mutex::new(String::new()));
    let capture = captured.clone();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body_from_request(move |request| {
            let authorization = request
                .header("Authorization")
                .first()
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            *capture.lock().expect("not poisoned") = authorization;
            serde_json::json!([cluster_json(
                "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "Default cluster"
            )])
            .to_string()
            .into_bytes()
        })
        .create();
    let credential =
        Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_thetoolsowndefault");
    let http = Http::new(provider.url(), Vec::new(), credential);
    let tool = BuildkiteClusterGet::new(Arc::new(BuildkiteClient::new(http)));

    // `inputs()` binds no `token` port: this is the unbound, execution-
    // context path.
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    assert_eq!(
        captured.lock().expect("not poisoned").as_str(),
        "Bearer bkua_thetoolsowndefault"
    );
}

#[test]
fn read_authorizes_with_the_bound_token_port_not_the_default_credential() {
    let mut provider = MockProvider::start();
    let captured = Arc::new(Mutex::new(String::new()));
    let capture = captured.clone();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body_from_request(move |request| {
            let authorization = request
                .header("Authorization")
                .first()
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            *capture.lock().expect("not poisoned") = authorization;
            serde_json::json!([cluster_json(
                "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "Default cluster"
            )])
            .to_string()
            .into_bytes()
        })
        .create();
    let tool = BuildkiteClusterGet::new(client_against(provider.url()));
    let mut request_inputs = inputs();
    request_inputs.insert(
        PortName::parse("token").unwrap(),
        Value::known(
            willikins_types::BuildkiteToken::parse(concat!("bkua_", "theboundtokenexampleexample"))
                .unwrap(),
        ),
    );
    let observation = tool.read(&request_inputs).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    assert_eq!(
        captured.lock().expect("not poisoned").as_str(),
        concat!("Bearer bkua_", "theboundtokenexampleexample")
    );
}

/// The stronger version of the proof above: the tool's own default
/// credential is refused outright by the mock, so this can only pass if
/// the bound `token` port's credential is what actually authorized the
/// request -- a regression that made `ScopedClient` fall back to the
/// default (even a *valid* default, as the previous test alone would
/// tolerate) fails here.
#[test]
fn a_bound_token_port_is_used_even_when_the_default_credential_would_be_refused() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .match_header(
            "authorization",
            concat!("Bearer bkua_", "theboundtokenexampleexample"),
        )
        .with_status(200)
        .with_body(
            serde_json::json!([cluster_json(
                "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "Default cluster"
            )])
            .to_string(),
        )
        .create();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .match_header("authorization", "Bearer bkua_testtoken")
        .with_status(401)
        .with_body(r#"{"message":"Forbidden"}"#)
        .create();

    let tool = BuildkiteClusterGet::new(client_against(provider.url()));
    let mut request_inputs = inputs();
    request_inputs.insert(
        PortName::parse("token").unwrap(),
        Value::known(
            willikins_types::BuildkiteToken::parse(concat!("bkua_", "theboundtokenexampleexample"))
                .unwrap(),
        ),
    );
    let observation = tool
        .read(&request_inputs)
        .expect("the bound token, not the refused default, must authorize this request");
    assert!(matches!(observation, Observation::Present(_)));
}
