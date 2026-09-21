//! Acceptance test 2's redaction share (trust boundary 5, and the
//! milestone plan's "Redaction" bullet for this crate): a secret marker
//! and a token marker seeded in fixtures appear in no error, no `Debug`,
//! no `Observation`'s rendering, and no recorded request. Unlike
//! `willikins-providers-github`'s Actions secret `PUT`, none of the five
//! tools this file drives ever sends a secret value or a token value in
//! a request body — `doppler.service_token.rotate`'s `DELETE` identifies
//! a token by its `slug`, never its `key` — so "except where the API
//! requires the value" is nowhere among them, and every recorded request
//! line is checked, not only headers.
//!
//! **`doppler.secret.set` (added after this file) is the crate's one
//! exception**, by design: it is a sink, and a sink's whole job is to
//! send a value the API requires in the request body. It is deliberately
//! not driven through this shared file's "no recorded request line
//! carries the marker" assertion, which would be false for it on
//! purpose; its own file (`tests/secret_set_mock.rs`) proves the
//! narrower claim that *does* hold — the value reaches the `POST` body
//! and nowhere else (no error, no `Debug`, no `Observation`).

use std::path::Path;
use std::sync::{Arc, Mutex};

use willikins_core::{PortName, Tool, Value};
use willikins_providers_doppler::{
    DopplerClient, DopplerProjectEnsure, DopplerSecretGet, DopplerServiceTokenEnsure,
    DopplerServiceTokenRotate,
};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, DopplerConfig, DopplerProject, DopplerTokenName, SecretName};

/// Stands in for a real Doppler service-account token: shaped like one so
/// nothing rejects it before the point this test cares about, but
/// distinctive enough that its presence anywhere but the `Authorization`
/// header is unambiguously a bug. `concat!`-joined so this file holds no
/// literal spelling the whole thing contiguously.
const CREDENTIAL_MARKER: &str = concat!("dp.sa.", "wlknCredentialMarker00000000000000000");

/// Stands in for a real secret value.
const SECRET_MARKER: &str = "wlkn-secret-marker-9f2h7ap5rz8s";

/// Stands in for a real minted service token: shaped to match
/// [`willikins_types::DopplerServiceToken`]'s pattern. `concat!`-joined
/// for the same reason as [`CREDENTIAL_MARKER`].
const TOKEN_MARKER: &str = concat!("dp.st.", "wlknTokenMarker0000000000000000000000000");

#[test]
fn no_authored_fixture_carries_either_marker() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/doppler");
    for entry in std::fs::read_dir(&fixtures_dir).expect("fixtures/doppler/ is readable") {
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
            !text.contains(SECRET_MARKER),
            "{} carries the secret marker",
            path.display()
        );
        assert!(
            !text.contains(TOKEN_MARKER),
            "{} carries the token marker",
            path.display()
        );
    }
}

fn client_against(url: String) -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", CREDENTIAL_MARKER);
    Arc::new(DopplerClient::new(Http::new(url, Vec::new(), credential)))
}

/// A secret value carrying [`SECRET_MARKER`] (in both `raw` and
/// `computed`, distinguishably) reaches no error, no `Debug`, and no
/// `Observation`'s rendering.
#[test]
fn a_secret_marker_never_leaks_anywhere() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config/secret?project=third-thoughts&config=prd&name=DATABASE",
        )
        .with_status(200)
        .with_body(
            serde_json::json!({
                "name": "DATABASE",
                "value": {
                    "raw": format!("RAW-{SECRET_MARKER}"),
                    "computed": SECRET_MARKER,
                    "note": "",
                },
            })
            .to_string(),
        )
        .create();
    let tool = DopplerSecretGet::new(client_against(provider.url()));
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("config").unwrap(),
        Value::known(DopplerConfig::parse("third-thoughts/prd").unwrap()),
    );
    inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(SecretName::parse("DATABASE").unwrap()),
    );
    let observation = tool.read(&inputs).expect("reads");
    let debug = format!("{observation:?}");
    let json = serde_json::to_string(&observation).expect("Observation serializes");
    assert!(!debug.contains(SECRET_MARKER), "Debug leaked: {debug}");
    assert!(!json.contains(SECRET_MARKER), "JSON leaked: {json}");

    // A failing read (404) never names the value either — there is none
    // to name in that case, but the check costs nothing.
    let mut failing = MockProvider::start();
    failing
        .mock(
            "GET",
            "/v3/configs/config/secret?project=third-thoughts&config=prd&name=DATABASE",
        )
        .with_status(404)
        .create();
    let err = DopplerSecretGet::new(client_against(failing.url()))
        .read(&inputs)
        .expect_err("404");
    assert!(!err.message.contains(SECRET_MARKER), "{}", err.message);
}

/// A minted service token carrying [`TOKEN_MARKER`] reaches no error, no
/// `Debug`, and no `Observation`/`Ensured` rendering, across both
/// `doppler.service_token.ensure` and `doppler.service_token.rotate`.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_token_marker_never_leaks_anywhere() {
    let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
    let name = DopplerTokenName::parse("ci").unwrap();
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config));
    inputs.insert(PortName::parse("name").unwrap(), Value::known(name));

    let created_body = serde_json::json!({
        "token": {"name": "ci", "slug": "some-slug", "key": TOKEN_MARKER},
    })
    .to_string();

    for tool_name in ["ensure", "rotate"] {
        let mut provider = MockProvider::start();
        provider
            .mock(
                "GET",
                "/v3/configs/config/tokens?project=third-thoughts&config=prd",
            )
            .with_status(200)
            .with_body(r#"{"tokens":[]}"#)
            .create();
        provider
            .mock("DELETE", "/v3/configs/config/tokens/token")
            .with_status(200)
            .with_body(r#"{"success":true}"#)
            .create();
        provider
            .mock("POST", "/v3/configs/config/tokens")
            .with_status(200)
            .with_body(created_body.clone())
            .create();
        let client = client_against(provider.url());
        let token = willikins_core::SinkToken::new();
        let ensured = if tool_name == "ensure" {
            DopplerServiceTokenEnsure::new(client)
                .ensure(&inputs, &token)
                .expect("ensures")
        } else {
            DopplerServiceTokenRotate::new(client)
                .ensure(&inputs, &token)
                .expect("ensures")
        };
        let debug = format!("{ensured:?}");
        assert!(
            !debug.contains(TOKEN_MARKER),
            "{tool_name} Debug leaked: {debug}"
        );
        let rendered = ensured
            .outputs
            .get(&PortName::parse("token").unwrap())
            .unwrap()
            .render()
            .to_string();
        assert!(
            !rendered.contains(TOKEN_MARKER),
            "{tool_name} render leaked: {rendered}"
        );
    }
}

/// The credential marker reaches no part of a recorded request other than
/// the `Authorization` header, across a full create-then-something
/// `ensure` and a failing one.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_marker_credential_reaches_no_request_line_observation_ensured_or_error() {
    let mut provider = MockProvider::start();
    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));

    for (method, path, status, body) in [
        (
            "GET",
            "/v3/projects/project?project=third-thoughts",
            404,
            String::new(),
        ),
        (
            "POST",
            "/v3/projects",
            201,
            serde_json::json!({"project": {"description": "managed-by: willikins"}}).to_string(),
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
    let tool = DopplerProjectEnsure::new(client);
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("project").unwrap(),
        Value::known(DopplerProject::parse("third-thoughts").unwrap()),
    );

    let observation = tool.read(&inputs).expect("reads");
    let sink_token = willikins_core::SinkToken::new();
    let ensured = tool.ensure(&inputs, &sink_token).expect("ensures");

    // And one failing call, so a `ToolError` is in the sweep too.
    let mut failing = MockProvider::start();
    failing
        .mock("GET", "/v3/projects/project?project=third-thoughts")
        .with_status(500)
        .with_body(r#"{"messages":["boom"]}"#)
        .create();
    let failing_client = client_against(failing.url());
    let err = DopplerProjectEnsure::new(failing_client)
        .read(&inputs)
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

/// The other half of the credential question, which the sweep above
/// cannot see: the marker must reach the `Authorization` header on
/// *every* request this crate makes, and no other header at all.
///
/// Doppler needs no default headers beyond the bearer one (unlike
/// GitHub's `Accept`/`X-GitHub-Api-Version`/`User-Agent`), so "no other
/// header" here also means "nothing this crate added". Swept across all
/// three verbs the five tools use — the `GET` listing, the `POST` mint,
/// and the `DELETE` revoke, the last of which sends a body and so takes
/// `Http::delete_with_body`'s distinct, newer code path.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn every_request_carries_the_marker_in_authorization_and_in_no_other_header() {
    let mut provider = MockProvider::start();
    let captured = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));

    for (method, path, body) in [
        (
            "GET",
            "/v3/configs/config/tokens?project=third-thoughts&config=prd",
            serde_json::json!({"tokens": [{"name": "ci", "slug": "s1"}]}).to_string(),
        ),
        (
            "DELETE",
            "/v3/configs/config/tokens/token",
            r#"{"success":true}"#.to_string(),
        ),
        (
            "POST",
            "/v3/configs/config/tokens",
            serde_json::json!({
                "token": {"name": "ci", "slug": "s2", "key": TOKEN_MARKER},
            })
            .to_string(),
        ),
    ] {
        let capture = captured.clone();
        let verb = method.to_string();
        provider
            .mock(method, path)
            .with_status(200)
            .with_body_from_request(move |request| {
                let mut seen = capture.lock().expect("not poisoned");
                for (name, value) in request.headers() {
                    seen.push((
                        verb.clone(),
                        name.to_string(),
                        value.to_str().unwrap_or("<non-utf8>").to_string(),
                    ));
                }
                body.clone().into_bytes()
            })
            .create();
    }

    let client = client_against(provider.url());
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("config").unwrap(),
        Value::known(DopplerConfig::parse("third-thoughts/prd").unwrap()),
    );
    inputs.insert(
        PortName::parse("name").unwrap(),
        Value::known(DopplerTokenName::parse("ci").unwrap()),
    );
    // `rotate` is the one tool that exercises all three verbs in a single
    // call: list, revoke every match, mint.
    let sink = willikins_core::SinkToken::new();
    DopplerServiceTokenRotate::new(client)
        .ensure(&inputs, &sink)
        .expect("rotates");

    let captured = captured.lock().expect("not poisoned").clone();
    let mut authorized = std::collections::BTreeSet::new();
    for (verb, name, value) in &captured {
        if name.eq_ignore_ascii_case("authorization") {
            assert!(
                value.contains(CREDENTIAL_MARKER),
                "{verb}: the Authorization header must carry the credential: {value}"
            );
            authorized.insert(verb.clone());
        } else {
            assert!(
                !value.contains(CREDENTIAL_MARKER),
                "{verb}: header `{name}` leaked the credential marker: {value}"
            );
            assert!(
                !value.contains(TOKEN_MARKER),
                "{verb}: header `{name}` leaked the token marker: {value}"
            );
        }
    }
    assert_eq!(
        authorized,
        ["GET", "POST", "DELETE"]
            .into_iter()
            .map(String::from)
            .collect::<std::collections::BTreeSet<_>>(),
        "every verb must have carried an Authorization header"
    );
}
