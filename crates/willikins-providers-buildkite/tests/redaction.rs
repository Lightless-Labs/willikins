//! Acceptance test 7's redaction share (trust boundary 6): a credential
//! marker reaches no error, no `Debug`, no `Observation`/`Ensured`
//! rendering, and no request header but `Authorization`; no authored
//! fixture carries a credential-shaped literal either.

use std::path::Path;
use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, PortName, Tool, Value};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkitePipelineBootstrapGate, BuildkitePipelineEnsure,
};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{
    BuildkiteClusterId, BuildkiteOrg, BuildkitePipelineSlug, DomainType, GitHubOrg, GitHubRepo,
    ProjectSlug, RepoFile, RepoPath,
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

/// The bound-`token`-port sibling of [`CREDENTIAL_MARKER`]: milestone 3e
/// task K1's optional `token` port takes an entirely separate code path
/// (`ScopedClient::Bound`, never the default client's own `Credential`),
/// so it earns its own marker rather than reusing one that only ever
/// exercised the default path. `concat!`-joined for the same reason.
const PORT_TOKEN_MARKER: &str = concat!("bkua_", "wlknPortTokenMarker0000000000000000");

/// Inputs for `buildkite.pipeline.ensure` naming `willikins-test/third-thoughts`,
/// with the `token` port bound to [`PORT_TOKEN_MARKER`] -- shared by both
/// bound-token-port redaction tests below.
fn marker_token_inputs() -> willikins_core::Inputs {
    let mut request_inputs = inputs();
    request_inputs.insert(
        PortName::parse("token").unwrap(),
        Value::known(willikins_types::BuildkiteToken::parse(PORT_TOKEN_MARKER).unwrap()),
    );
    request_inputs
}

/// Milestone 3e, task K1's own redaction proof, header half: a marker
/// carried by a document-bound `token` port (never the tool's default
/// credential) reaches no part of a recorded request but the
/// `Authorization` header -- the same guarantee
/// [`every_request_carries_the_marker_in_authorization_and_in_no_other_header`]
/// proves for the default-credential path, proven again for the new one
/// `ScopedClient::default_for` adds.
#[test]
fn a_bound_token_port_marker_reaches_no_header_but_authorization() {
    let mut provider = MockProvider::start();
    let captured_headers = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let capture = captured_headers.clone();
    provider
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(404)
        .with_body_from_request(move |request| {
            let headers = request
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        value.to_str().unwrap_or("<non-utf8>").to_string(),
                    )
                })
                .collect();
            *capture.lock().expect("not poisoned") = headers;
            br#"{"message":"Not Found"}"#.to_vec()
        })
        .create();

    // The tool's own default credential is a plain, unrelated value --
    // if the bound `token` port were ever ignored in favour of it, the
    // marker below would never appear anywhere, which the final
    // assertion below would catch.
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_unrelated");
    let http = Http::new(provider.url(), Vec::new(), credential);
    let tool = BuildkitePipelineEnsure::new(Arc::new(BuildkiteClient::new(http)));
    tool.read(&marker_token_inputs()).ok();

    let headers = captured_headers.lock().expect("not poisoned").clone();
    assert!(
        !headers.is_empty(),
        "the mock handler ran and captured headers"
    );
    let mut saw_authorization_with_marker = false;
    for (name, value) in &headers {
        if name.eq_ignore_ascii_case("authorization") {
            saw_authorization_with_marker |= value.contains(PORT_TOKEN_MARKER);
        } else {
            assert!(
                !value.contains(PORT_TOKEN_MARKER),
                "header `{name}` leaked the bound-token-port marker: {value}"
            );
        }
    }
    assert!(
        saw_authorization_with_marker,
        "the Authorization header should carry the bound token's marker, proving it (not the \
         default credential) authorized the request"
    );
}

/// The same proof's other half: neither a `404`'s `Observation` nor a
/// genuinely failing call's `ToolError` (`Debug` or `message`) ever
/// carries the bound-token-port marker.
#[test]
fn a_bound_token_port_marker_reaches_no_observation_or_error() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(404)
        .with_body(r#"{"message":"Not Found"}"#)
        .create();
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_unrelated");
    let http = Http::new(provider.url(), Vec::new(), credential);
    let observation = BuildkitePipelineEnsure::new(Arc::new(BuildkiteClient::new(http)))
        .read(&marker_token_inputs())
        .expect("a 404 is Absent, not an error");

    let mut failing = MockProvider::start();
    failing
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(500)
        .with_body(r#"{"message":"boom"}"#)
        .create();
    let failing_credential =
        Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN_2", "bkua_other");
    let failing_http = Http::new(failing.url(), Vec::new(), failing_credential);
    let err = BuildkitePipelineEnsure::new(Arc::new(BuildkiteClient::new(failing_http)))
        .read(&marker_token_inputs())
        .expect_err("500");

    for text in [
        format!("{observation:?}"),
        format!("{err:?}"),
        err.message.clone(),
    ] {
        assert!(
            !text.contains(PORT_TOKEN_MARKER),
            "the bound-token-port marker leaked into: {text}"
        );
    }
}

// ---------------------------------------------------------------------
// Milestone 3g task B1: decision (h) widens trust boundary 6 (narrowly)
// to let `buildkite.pipeline.bootstrap.gate` read a pipeline's stored
// `configuration` -- "the gate uses its own `PipelineConfigurationBody`,
// compares, and drops it: the value never reaches an output, an error,
// the journal, `tracing`, or `Debug`". A stored configuration may carry
// an operator's own `env`, so this is proven the same way every other
// credential-shaped marker is proven above: a distinctive marker planted
// in `configuration`, then grepped for everywhere this crate could have
// let it leak.
// ---------------------------------------------------------------------

/// Stands in for a secret an operator's own `env:` block in a stored
/// pipeline configuration might carry -- secret-*shaped*, the same way
/// [`CREDENTIAL_MARKER`] is, since decision (h) names exactly this case
/// ("a stored bootstrap may carry an operator's own `env`"); `concat!`-joined
/// for the same secret-literal-guard reason.
const CONFIGURATION_MARKER: &str = concat!("bkua_", "wlknConfigurationMarker00000000000000");

fn bootstrap_gate_inputs(expected_content: &str) -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("org").unwrap(), Value::known(org()));
    inputs.insert(PortName::parse("slug").unwrap(), Value::known(slug()));
    inputs.insert(
        PortName::parse("expected").unwrap(),
        Value::known(
            RepoFile::new(
                RepoPath::parse("apps/walter/.buildkite/bootstrap.yml").unwrap(),
                expected_content,
            )
            .unwrap(),
        ),
    );
    inputs
}

/// A stored `configuration` carrying the marker, different from
/// `expected` (so the gate reads `Absent`), never reaches the
/// `Observation`'s own `Debug`, the `Ensured`'s, or any `ToolError`
/// message -- across `read`, a failing `ensure`, and a provider error.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn a_configuration_marker_reaches_no_observation_ensured_or_error() {
    // `expected` is bound to the *same* YAML the mock serves (marker
    // included -- `RepoFile` accepts any grammar-legal content), so this
    // reads `Present`, the branch that actually carries outputs. A test
    // that only exercised `Absent` would never prove the marker stays
    // out of a successful gate's own rendering.
    let configuration = format!(
        "steps:\n  - command: \"echo hi\"\n    env:\n      TOKEN: \"{CONFIGURATION_MARKER}\"\n"
    );
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(200)
        .with_body(serde_json::json!({"configuration": configuration.clone()}).to_string())
        .create();
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    let client = Arc::new(BuildkiteClient::new(Http::new(
        provider.url(),
        Vec::new(),
        credential,
    )));
    let tool = BuildkitePipelineBootstrapGate::new(client);
    let inputs = bootstrap_gate_inputs(&configuration);

    let observation = tool.read(&inputs).expect("reads");
    assert!(
        matches!(observation, willikins_core::Observation::Present(_)),
        "{observation:?}"
    );
    let token = willikins_core::SinkToken::new();
    let ensured = tool.ensure(&inputs, &token).expect("ensures");
    assert!(!ensured.changed);

    let mut failing = MockProvider::start();
    failing
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(500)
        // The marker sits in `configuration`, which a `5xx` body-mapping
        // never reads at all (only `message` is), not in `message`
        // itself -- trust boundary 5 already permits (bounded, escaped)
        // provider `message` text into a `ToolError`, so a marker placed
        // there would fail this test for the wrong reason.
        .with_body(
            serde_json::json!({"message": "boom", "configuration": CONFIGURATION_MARKER})
                .to_string(),
        )
        .create();
    let failing_credential =
        Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    let failing_client = Arc::new(BuildkiteClient::new(Http::new(
        failing.url(),
        Vec::new(),
        failing_credential,
    )));
    let err = BuildkitePipelineBootstrapGate::new(failing_client)
        .read(&inputs)
        .expect_err("500");

    for text in [
        format!("{observation:?}"),
        format!("{ensured:?}"),
        format!("{err:?}"),
        err.message.clone(),
    ] {
        assert!(
            !text.contains(CONFIGURATION_MARKER),
            "the configuration marker leaked into: {text}"
        );
    }
}

/// A `configuration` field of the wrong JSON type (an object instead of
/// a string, carrying the marker inside it) is a provider failure -- and
/// even then, the marker never reaches the resulting `ToolError`: a
/// `serde` type-mismatch error must never echo the offending value.
#[test]
fn a_wrong_typed_configuration_carrying_the_marker_reaches_no_error() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v2/organizations/willikins-test/pipelines/third-thoughts",
        )
        .with_status(200)
        .with_body(
            serde_json::json!({
                "configuration": {"env": {"TOKEN": CONFIGURATION_MARKER}},
            })
            .to_string(),
        )
        .create();
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    let client = Arc::new(BuildkiteClient::new(Http::new(
        provider.url(),
        Vec::new(),
        credential,
    )));
    let tool = BuildkitePipelineBootstrapGate::new(client);
    let inputs = bootstrap_gate_inputs("steps:\n  - command: \"echo hi\"\n");

    let err = tool
        .read(&inputs)
        .expect_err("a type mismatch is a failure");
    assert!(
        !format!("{err:?}").contains(CONFIGURATION_MARKER),
        "the marker leaked into: {err:?}"
    );
    assert!(
        !err.message.contains(CONFIGURATION_MARKER),
        "the marker leaked into: {}",
        err.message
    );
}

/// Acceptance 9: "only `GET` is ever recorded". Records every request's
/// method across `read` and `ensure`, on both a matching (`Present`) and
/// a differing (`Absent`) stored configuration -- a gate that ever sent
/// anything but `GET` (a write this tool must never issue) would show up
/// here.
#[test]
#[allow(clippy::disallowed_methods)] // a test mints its own token
fn only_get_is_ever_recorded_across_read_and_ensure_present_and_absent() {
    let recorded = Arc::new(Mutex::new(Vec::<(String, String)>::new()));

    for (case, configuration) in [
        ("present", "steps:\n  - command: \"echo hi\"\n"),
        (
            "absent",
            "steps:\n  - command: \"buildkite-agent pipeline upload\"\n",
        ),
    ] {
        let mut provider = MockProvider::start();
        let capture = recorded.clone();
        provider
            .mock(
                "GET",
                "/v2/organizations/willikins-test/pipelines/third-thoughts",
            )
            .with_status(200)
            .with_body_from_request(move |request| {
                let mut seen = capture.lock().expect("not poisoned");
                seen.push((
                    request.method().to_string(),
                    request.path_and_query().to_string(),
                ));
                serde_json::json!({"configuration": configuration})
                    .to_string()
                    .into_bytes()
            })
            .create();

        let credential =
            Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
        let client = Arc::new(BuildkiteClient::new(Http::new(
            provider.url(),
            Vec::new(),
            credential,
        )));
        let tool = BuildkitePipelineBootstrapGate::new(client);
        let inputs = bootstrap_gate_inputs("steps:\n  - command: \"echo hi\"\n");

        tool.read(&inputs)
            .unwrap_or_else(|err| panic!("{case}: read failed: {err}"));
        let token = willikins_core::SinkToken::new();
        tool.ensure(&inputs, &token)
            .unwrap_or_else(|err| panic!("{case}: ensure failed: {err}"));
    }

    let recorded = recorded.lock().expect("not poisoned").clone();
    assert_eq!(
        recorded.len(),
        4,
        "read and ensure each made one request, twice"
    );
    for (method, path) in &recorded {
        assert_eq!(
            method, "GET",
            "a request other than GET was recorded: {path}"
        );
    }
}
