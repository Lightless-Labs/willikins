//! Acceptance test 2's share for `doppler.config.ensure`. The plan's port
//! table makes `Present` conditional on the fetched config's `root` flag
//! (`200` *and* `root: true`), so a `200` carrying `root: false` is a
//! different config that happens to sit at this name: `Foreign`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Observation, PortName, SinkToken, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerConfigEnsure};
use willikins_providers_http::testing::{MockProvider, json_body, load_fixture};
use willikins_providers_http::{Credential, Http, Sleeper};
use willikins_types::{DomainType, DopplerProject, EnvironmentSlug};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "doppler", name)
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

fn project() -> DopplerProject {
    DopplerProject::parse("third-thoughts").unwrap()
}

fn environment() -> EnvironmentSlug {
    EnvironmentSlug::parse("prd").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
    inputs.insert(
        PortName::parse("environment").unwrap(),
        Value::known(environment()),
    );
    inputs
}

#[test]
fn read_reports_absent_on_404() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    let value = predicted
        .get(&PortName::parse("config").unwrap())
        .unwrap()
        .render()
        .to_string();
    assert_eq!(value, "third-thoughts/prd");
}

/// **2026-09-20 defect, this tool's share.** Same cause as
/// `doppler.project.ensure`'s identical fix
/// (`docs/solutions/providers/doppler-400s-a-missing-project-when-quiescent.md`):
/// the parent project this `GET` needs can answer `400` "This token does
/// not have access to requested project" instead of `404`, depending on
/// whether this token can already see any project in the workplace, and
/// both must read as `Absent`.
#[test]
fn read_reports_absent_on_a_400_naming_no_access() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(400)
        .with_body(fixture("error_400_no_access").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    let Observation::Absent { predicted } = observation else {
        panic!("expected Absent, got {observation:?}");
    };
    let value = predicted
        .get(&PortName::parse("config").unwrap())
        .unwrap()
        .render()
        .to_string();
    assert_eq!(value, "third-thoughts/prd");
}

/// A `400` naming anything other than "no access" still fails.
#[test]
fn read_still_propagates_a_400_with_an_unrelated_message() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(400)
        .with_body(r#"{"success": false, "messages": ["Could not find requested project."]}"#)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
fn read_reports_present_on_200() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(matches!(observation, Observation::Present(_)));
}

/// A `200` whose `root` is `false` is **not** this tool's config. The
/// plan's port table conditions `Present` on `root: true` for a reason
/// that is reachable, not theoretical: Doppler names a branch config
/// `<environment>_<name>` (research note section 3: `prd_aws` under
/// environment `prd`), and `naming::v1::doppler_root_config` names a root
/// config after its environment's *snake join*. A project holding an
/// environment `pre` with a branch config `prod` therefore already has a
/// config named `pre_prod` -- exactly the name this tool derives for the
/// environment `pre-prod`. Reporting that `Present` would leave the
/// `pre-prod` environment uncreated and hand every downstream step
/// (`doppler.service_token.ensure` above all) a `DopplerConfig` pointing
/// into a different environment entirely.
#[test]
fn read_reports_foreign_when_root_is_false() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(serde_json::json!({"config": {"name": "prd", "root": false}}).to_string())
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let observation = tool.read(&inputs()).unwrap();
    assert!(
        matches!(observation, Observation::Foreign),
        "got {observation:?}"
    );
}

/// A `root` field that is missing, or explicitly `null`, is `Foreign`
/// too, never a parse failure and never `Present`: absence of the proof
/// that this is a root config is not proof that it is one.
#[test]
fn read_reports_foreign_when_root_is_missing_or_null() {
    for body in [
        serde_json::json!({"config": {"name": "prd"}}),
        serde_json::json!({"config": {"name": "prd", "root": null}}),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v3/configs/config?project=third-thoughts&config=prd",
            )
            .with_status(200)
            .with_body(body.to_string())
            .create();
        let (client, _sleeper) = client_against(provider.url());
        let tool = DopplerConfigEnsure::new(client);
        let observation = tool
            .read(&inputs())
            .unwrap_or_else(|err| panic!("body {body} must read, got {err:?}"));
        assert!(
            matches!(observation, Observation::Foreign),
            "body {body} must read as Foreign, got {observation:?}"
        );
    }
}

/// The collision named above, end to end: environment `pre-prod` derives
/// the config name `pre_prod`, which a `200` answers with `root: false`
/// (the branch config `prod` under environment `pre`). `ensure` must
/// refuse -- `Conflict`, no `POST` -- rather than silently adopt it.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_refuses_a_branch_config_squatting_on_a_root_config_name() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=pre_prod",
        )
        .with_status(200)
        .with_body(
            serde_json::json!({"config": {"name": "pre_prod", "root": false,
                                          "environment": "pre"}})
            .to_string(),
        )
        .create();
    let post = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .expect(0)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
    inputs.insert(
        PortName::parse("environment").unwrap(),
        Value::known(EnvironmentSlug::parse("pre-prod").unwrap()),
    );
    let token = SinkToken::new();
    let err = tool.ensure(&inputs, &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(
        err.message.contains("third-thoughts/pre_prod"),
        "{}",
        err.message
    );
    post.assert();
}

/// A multi-word environment slug reaches the wire as its snake join, in
/// the `GET`'s query and in both halves of the `POST` body. Whether
/// Doppler's environment-slug grammar accepts an underscore at all is the
/// plan's own open question ("Doppler's environment-slug grammar versus
/// `naming::v1`"), answered only by a live run; what this pins is that
/// `naming::v1`'s answer is what gets sent, unmodified, in all three
/// places.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_multi_word_environment_sends_its_snake_join_as_both_name_and_slug() {
    let mut provider = MockProvider::start();
    let read = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=pre_prod",
        )
        .with_status(404)
        .expect(1)
        .create();
    let create = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .match_body(json_body(serde_json::json!({
            "name": "pre_prod",
            "slug": "pre_prod",
        })))
        .with_status(201)
        .with_body(fixture("environment_post_created").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("project").unwrap(), Value::known(project()));
    inputs.insert(
        PortName::parse("environment").unwrap(),
        Value::known(EnvironmentSlug::parse("pre-prod").unwrap()),
    );
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs, &token).unwrap();
    assert!(ensured.changed);
    read.assert();
    create.assert();
}

#[test]
fn read_maps_a_5xx_to_a_bounded_provider_error() {
    let mut provider = MockProvider::start();
    let long = "x".repeat(10_000);
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(503)
        .with_body(format!(r#"{{"messages":["{long}"]}}"#))
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert!(err.message.len() <= "provider says: ".len() + 256);
}

#[test]
fn read_is_retried_on_a_429_with_retry_after_then_succeeds() {
    let mut provider = MockProvider::start();
    let first = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(429)
        .with_header("Retry-After", "1")
        .expect(1)
        .create();
    let second = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
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
    let tool = DopplerConfigEnsure::new(client);
    let err = tool.read(&inputs()).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_creates_the_environment_and_asserts_the_request_body() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .create();
    let create = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .match_body(json_body(serde_json::json!({
            "name": "prd",
            "slug": "prd",
        })))
        .with_status(201)
        .with_body(fixture("environment_post_created").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(ensured.changed);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_post_is_never_retried_on_a_503() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .create();
    let create = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .with_status(503)
        .with_body("{}")
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let token = SinkToken::new();
    let err = tool.ensure(&inputs(), &token).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Provider);
    create.assert();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn an_error_after_create_re_reads_and_reports_unchanged_when_present() {
    let mut provider = MockProvider::start();
    let first_read = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .expect(1)
        .create();
    let create = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .with_status(500)
        .with_body("{}")
        .expect(1)
        .create();
    let second_read = provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .expect(1)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    first_read.assert();
    create.assert();
    second_read.assert();
}

/// Acceptance test 5's own claim: `doppler.project.ensure` seeded
/// `dev`/`stg`/`prd` finds them `Unchanged` — i.e. `doppler.config.ensure`
/// on an already-present config makes no `POST` at all.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn ensure_on_a_present_config_makes_no_post() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();
    let post = provider
        .mock("POST", "/v3/environments?project=third-thoughts")
        .expect(0)
        .create();
    let (client, _sleeper) = client_against(provider.url());
    let tool = DopplerConfigEnsure::new(client);
    let token = SinkToken::new();
    let ensured = tool.ensure(&inputs(), &token).unwrap();
    assert!(!ensured.changed);
    post.assert();
}
