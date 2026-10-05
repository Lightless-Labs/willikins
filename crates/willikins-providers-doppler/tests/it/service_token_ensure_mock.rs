//! Acceptance test 2's share for `doppler.service_token.ensure`. This
//! tool's port table gives it no `Foreign` state: any listed token by
//! that name is ours (Doppler has no per-token ownership marker; the
//! plan's own ownership rule says tokens under an owned config are ours).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerServiceTokenEnsure};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{DomainType, DopplerConfig, DopplerTokenName};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "doppler", name)
}

/// `service_token_post_created.json`'s `key` field is the placeholder
/// `DOPPLER_TOKEN_PLACEHOLDER`, not a token-shaped literal (see
/// `fixtures/doppler/README.md`): this substitutes in a token
/// `concat!`-assembled from parts, so the wire body still carries
/// something [`willikins_types::DopplerServiceToken`] parses, without
/// spelling a Doppler-token-shaped string in any file on disk.
fn service_token_created_body() -> String {
    fixture("service_token_post_created").to_string().replace(
        "DOPPLER_TOKEN_PLACEHOLDER",
        concat!("dp.st.dev.", "wlknFixtureServiceTokenNotARealCredential00"),
    )
}

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

fn config() -> DopplerConfig {
    DopplerConfig::parse("third-thoughts/prd").unwrap()
}

fn name() -> DopplerTokenName {
    DopplerTokenName::parse("ci").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name()));
    inputs
}

const LIST_PATH: &str = "/v3/configs/config/tokens?project=third-thoughts&config=prd";
const CREATE_PATH: &str = "/v3/configs/config/tokens";

fn token_value(outputs: &willikins_core::Outputs) -> Value {
    outputs
        .get(&PortName::parse("token").unwrap())
        .unwrap()
        .clone()
}

#[test]
fn read_reports_absent_with_an_unknown_token_when_empty() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    assert!(!token_value(&predicted).is_known());
}

#[test]
fn read_reports_present_with_an_unknown_token_when_listed() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Present(outputs) = observation else {
        panic!("expected Present, got {observation:?}");
    };
    assert!(!token_value(&outputs).is_known());
}

/// **The 2026-09-16 smoke-run defect.** Doppler's token-list endpoint
/// 404s when the `project` or `config` it was asked about does not exist
/// yet — which, at plan time, is exactly the state before this
/// workflow's own `doppler.project.ensure` and `doppler.config.ensure`
/// nodes have run. Before this fix, `read` propagated that 404 as a hard
/// `ToolError`, so planning the positive fixture against a fresh Doppler
/// account failed outright at this node
/// (`fixtures/doppler/service_tokens_list_project_missing.json` is the
/// exact body the live smoke run saw). "No parent yet" answers "is a
/// token named `name` already listed?" the same way "an empty list"
/// does: `Observation::Absent`, matching `doppler.config.ensure`'s
/// identical 404 handling next door. See the fixtures directory's
/// README for the full defect note.
#[test]
fn read_reports_absent_when_the_parent_project_or_config_does_not_exist_yet() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(404)
        .with_body(fixture("service_tokens_list_project_missing").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    assert!(!token_value(&predicted).is_known());
}

/// The same 404 tolerated by `ensure`'s own listing check: with the
/// parent still missing, `ensure` proceeds straight to the mint `POST`
/// rather than failing on the listing `GET` — the mint itself is what
/// surfaces a real failure if the parent is genuinely still absent by
/// apply time.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_mints_when_the_parent_project_or_config_does_not_exist_yet() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(404)
        .with_body(fixture("service_tokens_list_project_missing").to_string())
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(service_token_created_body())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    assert!(token_value(&ensured.outputs).is_known());
    create.assert();
}

/// **2026-09-20 defect, this tool's share.** The same listing endpoint
/// can also answer `400` "This token does not have access to requested
/// project" for a missing parent, whenever this token can already see
/// any project in the workplace
/// (`docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`).
/// `is_listed` now tolerates this the same way it tolerates the 404
/// above.
#[test]
fn read_reports_absent_when_the_parent_answers_400_naming_no_access() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    assert!(!token_value(&predicted).is_known());
}

/// The parity hole the defect exposed: the fake and the live provider
/// must agree on this exact question ("is a token listed, in a project
/// the provider does not hold at all?"). The fake never models a project
/// existing or not — a token is either seeded or it is not — so a fresh
/// `FakeState` with nothing seeded already answers `Absent` here, which
/// is also what the live tool must answer once the fix above lands. If
/// either side changes, this fails, and whoever changes it has to change
/// the other too.
#[test]
fn read_agrees_with_the_fake_tool_on_a_project_it_does_not_hold() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(404)
        .with_body(fixture("service_tokens_list_project_missing").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let live = DopplerServiceTokenEnsure::new(client)
        .read(&inputs())
        .unwrap();

    let state = Arc::new(Mutex::new(willikins_providers_fake::FakeState::new()));
    let fake = willikins_providers_fake::tools::DopplerServiceTokenEnsure::new(state)
        .read(&inputs())
        .unwrap();

    assert!(matches!(live, Observation::Absent { .. }), "live: {live:?}");
    assert!(matches!(fake, Observation::Absent { .. }), "fake: {fake:?}");
    assert_eq!(
        serde_json::to_value(&live).unwrap(),
        serde_json::to_value(&fake).unwrap(),
        "the live and fake ensure must observe a token in a missing project identically"
    );
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock("GET", LIST_PATH)
        .with_status(503)
        .with_body(format!(r#"{{"messages":["{long}"]}}"#))
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}

#[test]
fn read_is_retried_on_a_429_with_retry_after_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock("GET", LIST_PATH)
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
    first.assert();
    second.assert();
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
    let client = Arc::new(DopplerClient::new(http));
    let tool = DopplerServiceTokenEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_token_makes_no_post_and_stays_unknown() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let post = provider.mock("POST", CREATE_PATH).expect(0).create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    assert!(!token_value(&ensured.outputs).is_known());
    post.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_an_absent_token_creates_it_and_asserts_the_request_body() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts",
            "config": "prd",
            "name": "ci",
            "access": "read",
        })))
        .with_status(200)
        .with_body(service_token_created_body())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    assert!(token_value(&ensured.outputs).is_known());
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

/// A `key` that fails [`willikins_types::DopplerServiceToken`]'s pattern
/// is a `ToolError` that echoes nothing: it fails inside `Http::finish`'s
/// body-parse step, which names only a line/column position, never
/// response text.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_malformed_key_in_the_response_is_a_provider_error_that_echoes_nothing() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let malformed_key = "dp.st.tooshort";
    provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(
            serde_json::json!({"token": {"name": "ci", "slug": "abc", "key": malformed_key}})
                .to_string(),
        )
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(
        !err.message.contains(malformed_key) && !err.message.contains("dp.st"),
        "{}",
        err.message
    );
}

/// Every other way a create response can fail to yield a token: the
/// `key` missing entirely, empty, `null`, or not a string at all. All
/// four are `Provider` errors that echo nothing — the numeric one
/// matters most, because `serde_json`'s own `Display` for a type
/// mismatch quotes the offending value verbatim, and `Http::finish`
/// throws that text away in favour of a line/column position.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_key_that_is_missing_empty_null_or_not_a_string_errors_and_echoes_nothing() {
    for token in [
        serde_json::json!({"name": "ci", "slug": "abc"}),
        serde_json::json!({"name": "ci", "slug": "abc", "key": ""}),
        serde_json::json!({"name": "ci", "slug": "abc", "key": null}),
        serde_json::json!({"name": "ci", "slug": "abc", "key": 123}),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", LIST_PATH)
            .with_status(200)
            .with_body(fixture("service_tokens_list_absent").to_string())
            .create();
        provider
            .mock("POST", CREATE_PATH)
            .with_status(200)
            .with_body(serde_json::json!({"token": token}).to_string())
            .create();
        let (client, _sleeper) = client_against(provider.url());
        let tool = DopplerServiceTokenEnsure::new(client);
        let sink = SinkToken::new();
        let err = tool
            .ensure(&inputs(), &sink)
            .err()
            .unwrap_or_else(|| panic!("token {token} must not yield a minted token"));
        assert_eq!(err.kind, ToolErrorKind::Provider, "token {token}");
        assert!(
            !err.message.contains("123") && !err.message.contains("key"),
            "token {token}: the error echoed the response: {}",
            err.message
        );
    }
}

/// `Present` means *no call at all* beyond the listing `GET`: not the
/// mint, and not a stray `DELETE` either (the sibling `rotate` tool's
/// `DELETE` path must never be reachable from `ensure`).
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_token_makes_no_call_but_the_listing_get() {
    let mut provider = MockProvider::start();
    let list = provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .expect(1)
        .create();
    let post = provider.mock("POST", CREATE_PATH).expect(0).create();
    let delete = provider
        .mock("DELETE", "/v3/configs/config/tokens/token")
        .expect(0)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let sink = SinkToken::new();
    tool.ensure(&inputs(), &sink).unwrap();
    list.assert();
    post.assert();
    delete.assert();
}

/// Where the tolerance stops, part one. The 404 above answers "is a token
/// listed", never "may this plan proceed": a parent still missing when
/// `ensure` runs — ordering should have created it by then — fails at the
/// mint `POST`, loudly and with Doppler's own status. That is what keeps
/// the `read` arm from swallowing anything, and it is the same answer for
/// the 404 that means "a project this credential is not granted", which
/// the status alone cannot be told apart from "not created yet".
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_fails_when_the_parent_is_still_missing_at_apply() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(404)
        .with_body(fixture("service_tokens_list_project_missing").to_string())
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(404)
        .with_body(fixture("service_tokens_list_project_missing").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    create.assert();
}

/// Where the tolerance stops, part two: since 2026-09-20 it is one
/// *message*, not every `400`. This body carries Doppler's other
/// documented `400` phrasing, "Could not find requested project." (a
/// full stop, no "does not have access"), which
/// `looks_like_a_missing_project` deliberately does not match — the
/// message check exists precisely so a duplicate-create conflict is
/// never misread as absence, and this pins that any other `400` wording
/// still fails rather than being guessed at.
#[test]
fn read_still_propagates_a_400_from_the_listing() {
    let mut provider = MockProvider::start();
    let list = provider
        .mock("GET", LIST_PATH)
        .with_status(400)
        .with_body(fixture("service_tokens_list_project_missing").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    list.assert();
}
