//! Acceptance test 2's share for `doppler.service_token.rotate`: `read`
//! always reports `Absent` (see the crate's own module docs), and
//! `ensure`'s `DELETE` bodies are asserted one per listed token.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerServiceTokenRotate};
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
const DELETE_PATH: &str = "/v3/configs/config/tokens/token";

fn token_value(outputs: &willikins_core::Outputs) -> Value {
    outputs
        .get(&PortName::parse("token").unwrap())
        .unwrap()
        .clone()
}

/// `read` always reports `Absent`, even when a token by that name is
/// already listed — see the crate's own module docs for why.
#[test]
fn read_always_reports_absent_even_when_a_token_is_listed() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// **The 2026-09-16 smoke-run defect, this tool's share.** Same cause as
/// `doppler.service_token.ensure`'s identical fix (see that crate's
/// mock test and the fixtures directory's README): the listing `GET`
/// this `read` always performs 404s when the parent project or config
/// does not exist yet, which at plan time is exactly the state before
/// this workflow's own `doppler.project.ensure`/`doppler.config.ensure`
/// nodes have run. `read` already reports `Absent` unconditionally once
/// the listing succeeds (a `Destructive` step must never plan as
/// `NoOp`) — the fix is only that a 404 must not stop it from reaching
/// that unconditional answer. `read_still_propagates_a_listing_failure`
/// below still pins that a *real* failure (a 5xx, a bad credential)
/// keeps failing at plan time; only "the parent is not there yet" is
/// now tolerated.
#[test]
fn read_reports_absent_when_the_parent_project_or_config_does_not_exist_yet() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(404)
        .with_body(fixture("service_tokens_list_project_missing").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// **2026-09-20 defect, this tool's share.** The same listing endpoint
/// can also answer `400` "This token does not have access to requested
/// project" for a missing parent, whenever this token can already see
/// any project in the workplace
/// (`docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`).
/// `read` tolerates this the same way it tolerates the 404 above; `Absent`
/// either way, since this step never plans as `NoOp`.
#[test]
fn read_reports_absent_when_the_parent_answers_400_naming_no_access() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Absent { .. }));
}

/// A `400` naming anything other than "no access" still fails — the
/// tolerance is one message, not every `400`.
#[test]
fn read_still_propagates_a_400_with_an_unrelated_message() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(400)
        .with_body(r#"{"success": false, "messages": ["Could not find requested project."]}"#)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

/// The listing `GET` still runs during `read`, so a bad credential or
/// config fails at plan time rather than being silently swallowed.
#[test]
fn read_still_propagates_a_listing_failure() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(503)
        .with_body("{}")
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn read_maps_a_transport_timeout_to_a_provider_error() {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new("http://127.0.0.1:1", Vec::new(), credential);
    let client = Arc::new(DopplerClient::new(http));
    let tool = DopplerServiceTokenRotate::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

/// `ensure` on no existing token: no `DELETE` at all, straight to mint.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_with_no_existing_token_makes_no_delete_and_mints() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_absent").to_string())
        .create();
    let delete = provider.mock("DELETE", DELETE_PATH).expect(0).create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(service_token_created_body())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    assert!(token_value(&ensured.outputs).is_known());
    delete.assert();
    create.assert();
}

/// `ensure` on one existing token: exactly one `DELETE`, its body
/// asserted, then the mint.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_deletes_the_one_listed_token_by_slug_then_mints() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let delete = provider
        .mock("DELETE", DELETE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts",
            "config": "prd",
            "slug": "56c69f96-3045-11ea-978f-2e728ce88125",
        })))
        .with_status(200)
        .with_body(fixture("service_token_delete").to_string())
        .expect(1)
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(service_token_created_body())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    delete.assert();
    create.assert();
}

/// One `DELETE` per listed token sharing the rotated name — never a
/// single call for however many are listed.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_deletes_every_listed_token_sharing_the_name() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(
            serde_json::json!({"tokens": [
                {"name": "ci", "slug": "slug-one"},
                {"name": "ci", "slug": "slug-two"},
                {"name": "other", "slug": "slug-three"},
            ]})
            .to_string(),
        )
        .create();
    let delete_one = provider
        .mock("DELETE", DELETE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts", "config": "prd", "slug": "slug-one",
        })))
        .with_status(200)
        .with_body(r#"{"success":true}"#)
        .expect(1)
        .create();
    let delete_two = provider
        .mock("DELETE", DELETE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts", "config": "prd", "slug": "slug-two",
        })))
        .with_status(200)
        .with_body(r#"{"success":true}"#)
        .expect(1)
        .create();
    let delete_other = provider
        .mock("DELETE", DELETE_PATH)
        .match_body(json_body(serde_json::json!({
            "project": "third-thoughts", "config": "prd", "slug": "slug-three",
        })))
        .expect(0)
        .create();
    provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(service_token_created_body())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    tool.ensure(&inputs(), &token).unwrap();
    delete_one.assert();
    delete_two.assert();
    delete_other.assert();
}

/// If the `DELETE` fails, nothing is minted: a rotation that fails to
/// revoke the old token but still hands out a new one is the opposite of
/// a rotation.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_failed_delete_mints_nothing() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    provider
        .mock("DELETE", DELETE_PATH)
        .with_status(500)
        .with_body("{}")
        .create();
    let create = provider.mock("POST", CREATE_PATH).expect(0).create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
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
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

#[test]
fn spec_class_is_destructive() {
    let (client, _sleeper) = client_against("http://127.0.0.1:0".to_string());
    let tool = DopplerServiceTokenRotate::new(client);
    assert_eq!(tool.spec().class, willikins_core::Class::Destructive);
}

/// The one deliberate departure from the plan's literal port table
/// ("`read`: as `ensure`'s read") pinned where it can actually be
/// checked: against the tool it is copied from. The fake
/// `doppler.service_token.rotate` already ships this decision — a
/// `Destructive` step must never plan as `Action::NoOp`, which would tell
/// the approver nothing is about to happen to a live token — and the live
/// tool must agree with it, in the *same* situation the fake is in: a
/// token by that name already exists.
///
/// If either side is ever "fixed" to report `Present`, this fails, and
/// whoever changes it has to change the other too.
#[test]
fn read_agrees_with_the_fake_tool_that_an_existing_token_is_still_absent() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(fixture("service_tokens_list_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let live = DopplerServiceTokenRotate::new(client)
        .read(&inputs())
        .unwrap();

    let state = Arc::new(Mutex::new(
        willikins_providers_fake::FakeState::new().with_doppler_service_token(&config(), &name()),
    ));
    let fake = willikins_providers_fake::tools::DopplerServiceTokenRotate::new(state)
        .read(&inputs())
        .unwrap();

    assert!(matches!(live, Observation::Absent { .. }), "live: {live:?}");
    assert!(matches!(fake, Observation::Absent { .. }), "fake: {fake:?}");
    assert_eq!(
        serde_json::to_value(&live).unwrap(),
        serde_json::to_value(&fake).unwrap(),
        "the live and fake rotate must observe an existing token identically"
    );
}

/// `ensure` mints exactly once however many tokens it had to revoke
/// first: two `DELETE`s, one `POST`, never one mint per revocation.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_mints_exactly_once_however_many_tokens_it_revoked() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(200)
        .with_body(
            serde_json::json!({"tokens": [
                {"name": "ci", "slug": "slug-one"},
                {"name": "ci", "slug": "slug-two"},
            ]})
            .to_string(),
        )
        .expect(1)
        .create();
    let deletes = provider
        .mock("DELETE", DELETE_PATH)
        .with_status(200)
        .with_body(r#"{"success":true}"#)
        .expect(2)
        .create();
    let create = provider
        .mock("POST", CREATE_PATH)
        .with_status(200)
        .with_body(service_token_created_body())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let sink = SinkToken::new();
    DopplerServiceTokenRotate::new(client)
        .ensure(&inputs(), &sink)
        .unwrap();
    deletes.assert();
    create.assert();
}

/// The missing-parent tolerance stops at `read`. `ensure` must be able to
/// list the tokens it is about to revoke: a rotation that mints a
/// replacement without having revoked anything is the opposite of a
/// rotation, so a listing 404 fails outright here — no `DELETE`, no mint —
/// where the same 404 in `read` is only "no token listed yet".
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_still_fails_outright_on_a_listing_404() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", LIST_PATH)
        .with_status(404)
        .with_body(fixture("service_tokens_list_project_missing").to_string())
        .create();
    let delete = provider.mock("DELETE", DELETE_PATH).expect(0).create();
    let create = provider.mock("POST", CREATE_PATH).expect(0).create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerServiceTokenRotate::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    delete.assert();
    create.assert();
}
