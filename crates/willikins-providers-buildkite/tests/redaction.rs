//! Acceptance test 7's redaction share (trust boundary 6): a credential
//! marker reaches no error, no `Debug`, no `Observation`/`Ensured`
//! rendering, and no request header but `Authorization`; no authored
//! fixture carries a credential-shaped literal either.

use std::path::Path;
use std::sync::{Arc, Mutex};

use willikins_core::{PortName, Tool, Value};
use willikins_providers_buildkite::{BuildkiteClient, BuildkitePipelineEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{
    BuildkiteClusterId, BuildkiteOrg, BuildkitePipelineSlug, DomainType, GitHubOrg, GitHubRepo,
    ProjectSlug,
};

/// Stands in for a real Buildkite API access token: shaped like one so
/// nothing rejects it before the point this test cares about, but
/// distinctive enough that its presence anywhere but the `Authorization`
/// header is unambiguously a bug. `concat!`-joined so this file holds no
/// literal spelling the whole thing contiguously (the secret-literal
/// guard's own rule, followed here too).
const CREDENTIAL_MARKER: &str = concat!("bkua_", "wlknCredentialMarker00000000000000000");

#[test]
fn no_authored_fixture_carries_a_credential_shaped_literal() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/buildkite");
    for entry in std::fs::read_dir(&fixtures_dir).expect("fixtures/buildkite/ is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() || path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        assert!(
            !text.contains(CREDENTIAL_MARKER),
            "{} carries the credential marker",
            path.display()
        );
        assert!(
            !text.contains("bkua_") && !text.contains("bkct_"),
            "{} carries a real Buildkite token prefix",
            path.display()
        );
    }
}

fn client_against(url: String) -> Arc<BuildkiteClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", CREDENTIAL_MARKER);
    Arc::new(BuildkiteClient::new(Http::new(url, Vec::new(), credential)))
}

fn org() -> BuildkiteOrg {
    BuildkiteOrg::parse("willikins-test").unwrap()
}

fn slug() -> BuildkitePipelineSlug {
    BuildkitePipelineSlug::parse("third-thoughts").unwrap()
}

fn repo() -> GitHubRepo {
    GitHubRepo::new(
        GitHubOrg::parse("lightless-labs").unwrap(),
        ProjectSlug::parse("third-thoughts").unwrap(),
    )
}

fn cluster() -> BuildkiteClusterId {
    BuildkiteClusterId::parse("018e5a22-d14c-7085-bb28-db0f83f43a1c").unwrap()
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("org").unwrap(), Value::known(org()));
    inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug()));
    inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    inputs.insert(PortName::parse("cluster").unwrap(), Value::known(cluster()));
    inputs
}

/// The credential marker reaches no part of a recorded request other than
/// the `Authorization` header, across a full create-then-read `ensure`
/// and a failing one.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_marker_credential_reaches_no_request_line_observation_ensured_or_error() {
    let mut provider = MockProvider::start();
    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));

    for (method, path, status, body) in [
        (
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
            404,
            String::new(),
        ),
        (
            "POST",
            "/v2/organizations/willikins-test/pipelines",
            201,
            serde_json::json!({
                "id": "id", "slug": "third-thoughts", "web_url": "https://buildkite.com/willikins-test/third-thoughts",
                "repository": "git@github.com:lightless-labs/third-thoughts.git",
                "cluster_id": "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                "description": "managed-by: willikins",
            })
            .to_string(),
        ),
    ] {
        let capture = recorded.clone();
        provider
            .mock(method, path)
            .with_status(status)
            .with_body_from_request(move |request| {
                let mut seen = capture.lock().expect("not poisoned");
                seen.push(request.path_and_query().to_string());
                seen.push(
                    String::from_utf8_lossy(&request.body().cloned().unwrap_or_default())
                        .into_owned(),
                );
                body.clone().into_bytes()
            })
            .create();
    }

    let client = client_against(provider.url());
    let tool = BuildkitePipelineEnsure::new(client);

    let observation = tool.read(&inputs()).expect("reads");
    let sink_token = willikins_core::SinkToken::new();
    let ensured = tool.ensure(&inputs(), &sink_token).expect("ensures");

    let mut failing = MockProvider::start();
    failing
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(500)
        .with_body(r#"{"message":"boom"}"#)
        .create();
    let failing_client = client_against(failing.url());
    let err = BuildkitePipelineEnsure::new(failing_client)
        .read(&inputs())
        .expect_err("500");

    let recorded = recorded.lock().expect("not poisoned").clone();
    assert!(!recorded.is_empty(), "the handlers ran");
    for text in recorded.iter().cloned().chain([
        format!("{observation:?}"),
        format!("{ensured:?}"),
        format!("{err:?}"),
        err.message.clone(),
    ]) {
        assert!(
            !text.contains(CREDENTIAL_MARKER),
            "the credential marker leaked into: {text}"
        );
    }
}

/// The other half: the marker must reach the `Authorization` header on
/// every request this crate makes, and no other header.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn every_request_carries_the_marker_in_authorization_and_in_no_other_header() {
    let mut provider = MockProvider::start();
    let captured = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));

    provider
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(200)
        .with_body_from_request({
            let capture = captured.clone();
            move |request| {
                let mut seen = capture.lock().expect("not poisoned");
                for (name, value) in request.headers() {
                    seen.push((
                        "GET".to_string(),
                        name.to_string(),
                        value.to_str().unwrap_or("<non-utf8>").to_string(),
                    ));
                }
                serde_json::json!({
                    "id": "id", "slug": "third-thoughts",
                    "web_url": "https://buildkite.com/willikins-test/third-thoughts",
                    "repository": "git@github.com:lightless-labs/third-thoughts.git",
                    "cluster_id": "018e5a22-d14c-7085-bb28-db0f83f43a1c",
                    "description": "managed-by: willikins",
                })
                .to_string()
                .into_bytes()
            }
        })
        .create();

    let client = client_against(provider.url());
    let observation = BuildkitePipelineEnsure::new(client)
        .read(&inputs())
        .expect("reads");
    assert!(matches!(
        observation,
        willikins_core::Observation::Present(_)
    ));

    let captured = captured.lock().expect("not poisoned").clone();
    let mut authorized = false;
    for (_verb, name, value) in &captured {
        if name.eq_ignore_ascii_case("authorization") {
            assert!(
                value.contains(CREDENTIAL_MARKER),
                "the Authorization header must carry the credential: {value}"
            );
            authorized = true;
        } else {
            assert!(
                !value.contains(CREDENTIAL_MARKER),
                "header `{name}` leaked the credential marker: {value}"
            );
        }
    }
    assert!(
        authorized,
        "the request must have carried an Authorization header"
    );
}
