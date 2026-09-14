//! Acceptance test 4: a [`Credential`] built from an environment variable
//! holding a distinctive marker never leaks it through [`Debug`], a
//! [`CredentialError`], a [`ProviderError`], a [`ToolError`], or a
//! mockito-recorded request other than in the `Authorization` header.

use std::sync::{Arc, Mutex};

use willikins_core::ToolError;
use willikins_providers_http::{Credential, CredentialError, Http, ProviderError};

const MARKER: &str = "wlkn-leak-test-marker-q7f2vzn9xkd";
const VAR: &str = "WILLIKINS_TEST_LEAK_MARKER";

fn credential() -> Credential {
    // `Credential::for_testing` (behind `test-support`, active for every
    // test build of this crate) builds a `Credential` directly from a
    // known value: no real environment variable is read or set, so this
    // has nothing to do with `std::env::set_var`/`remove_var`, which are
    // `unsafe` (edition 2024) in a workspace that forbids `unsafe_code`
    // outright.
    Credential::for_testing(VAR, MARKER)
}

#[test]
fn debug_never_leaks_the_marker() {
    let credential = credential();
    let debug = format!("{credential:?}");
    assert!(!debug.contains(MARKER), "Debug leaked: {debug}");
}

#[test]
fn credential_error_never_leaks_the_marker() {
    let missing = CredentialError::Missing { var: VAR };
    assert!(!format!("{missing}").contains(MARKER));
    assert!(!format!("{missing:?}").contains(MARKER));

    let malformed = CredentialError::Malformed { var: VAR };
    assert!(!format!("{malformed}").contains(MARKER));
    assert!(!format!("{malformed:?}").contains(MARKER));
}

#[test]
fn tool_error_never_leaks_the_marker_on_401_or_403_even_when_the_body_names_it() {
    // 401/403 discard the response body entirely (trust boundary 5), so
    // this is really a `ProviderError` -> `ToolError` test, not a
    // credential test per se, but it is exactly the shape acceptance test
    // 4 asks for: the marker showing up in a *provider's* response text
    // must not end up in what an agent reads back as a `ToolError`.
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/forbidden")
        .with_status(403)
        .with_body(format!(r#"{{"message":"token {MARKER} lacks a scope"}}"#))
        .create();

    let http = Http::new(server.url(), Vec::new(), credential());
    let err: ProviderError = http
        .get::<serde_json::Value>("/forbidden")
        .expect_err("403 is an error");
    // `ProviderError.message` is deliberately built from the provider's own
    // response text verbatim (bounded and escaped) regardless of status —
    // trust boundary 5's redaction for 401/403 happens only at the
    // `ToolError` boundary below, which is what an agent actually sees. So
    // the marker legitimately appears in `err.message` at this point; only
    // `tool_err.message` carries the guarantee this test is about.
    let tool_err: ToolError = err.into();
    assert!(
        !tool_err.message.contains(MARKER),
        "ToolError leaked: {}",
        tool_err.message
    );
}

#[test]
fn a_recorded_request_carries_the_marker_only_in_the_authorization_header() {
    let mut server = mockito::Server::new();
    let captured_headers: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_body: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    let headers_for_closure = captured_headers.clone();
    let body_for_closure = captured_body.clone();
    server
        .mock("PUT", "/thing")
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
            *headers_for_closure.lock().expect("not poisoned") = headers;
            *body_for_closure.lock().expect("not poisoned") = request
                .utf8_lossy_body()
                .map(std::borrow::Cow::into_owned)
                .unwrap_or_default();
            b"{}".to_vec()
        })
        .with_status(200)
        .create();

    let http = Http::new(server.url(), Vec::new(), credential());
    let _: serde_json::Value = http
        .put(
            "/thing",
            &serde_json::json!({"field": "a value with no marker in it"}),
        )
        .expect("mock server answers 200");

    let body = captured_body.lock().expect("not poisoned").clone();
    assert!(
        !body.contains(MARKER),
        "request body leaked the marker: {body}"
    );

    let headers = captured_headers.lock().expect("not poisoned").clone();
    let mut headers_with_marker = headers.iter().filter(|(_, value)| value.contains(MARKER));
    let (name, value) = headers_with_marker
        .next()
        .expect("the marker appears in exactly one header (Authorization)");
    assert_eq!(name.to_lowercase(), "authorization");
    assert_eq!(*value, format!("Bearer {MARKER}"));
    assert!(
        headers_with_marker.next().is_none(),
        "the marker must not appear in any other header: {headers:?}"
    );
}
