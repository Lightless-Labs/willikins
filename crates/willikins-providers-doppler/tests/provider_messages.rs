//! What a Doppler error body is allowed to become.
//!
//! Doppler documents no error schema at all (research note section 3),
//! so the plan tells this crate to read a `messages` array when one is
//! present and otherwise report the status alone. The existing per-tool
//! mock tests only bound the message's *length*; nothing pinned that the
//! provider's own words arrive under the `provider says:` label that
//! keeps them from impersonating willikins (trust boundary 4's rule,
//! applied to provider text), nor that a body without `messages` yields
//! no such label at all.
//!
//! A message that could pass for willikins' own is the sharp case:
//! `[REDACTED DopplerServiceToken]` is a string willikins itself emits,
//! and a provider answering it verbatim must still be distinguishable
//! from willikins having redacted something.

use std::sync::Arc;

use willikins_core::{PortName, Tool, ToolErrorKind, Value};
use willikins_providers_doppler::{DopplerClient, DopplerProjectEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, DopplerProject};

const PROJECT_PATH: &str = "/v3/projects/project?project=third-thoughts";

fn client_against(url: String) -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    Arc::new(DopplerClient::new(Http::new(url, Vec::new(), credential)))
}

fn inputs() -> willikins_core::Inputs {
    let mut inputs = willikins_core::Inputs::new();
    inputs.insert(
        PortName::parse("project").unwrap(),
        Value::known(DopplerProject::parse("third-thoughts").unwrap()),
    );
    inputs
}

fn error_for(status: usize, body: &str) -> willikins_core::ToolError {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", PROJECT_PATH)
        .with_status(status)
        .with_body(body)
        .create();
    DopplerProjectEnsure::new(client_against(provider.url()))
        .read(&inputs())
        .expect_err("a failing status")
}

/// One `messages` entry arrives labelled, verbatim, under the label.
#[test]
fn a_messages_array_arrives_under_the_provider_says_label() {
    let err = error_for(
        400,
        r#"{"success":false,"messages":["Project name is invalid."]}"#,
    );
    assert_eq!(err.kind, ToolErrorKind::Provider);
    assert_eq!(err.message, "provider says: Project name is invalid.");
}

/// Several entries are joined, still under one label.
#[test]
fn several_messages_are_joined_under_one_label() {
    let err = error_for(400, r#"{"messages":["first thing","second thing"]}"#);
    assert_eq!(err.message, "provider says: first thing; second thing");
}

/// A body with no `messages` at all — Doppler's undocumented shapes
/// include an empty object and plain text — reports the status alone,
/// and is never labelled as the provider's words.
#[test]
fn a_body_without_messages_reports_the_status_alone() {
    for body in [
        "{}",
        "",
        "not json at all",
        r#"{"success":false}"#,
        r#"{"messages":[]}"#,
    ] {
        let err = error_for(500, body);
        assert_eq!(
            err.message, "provider returned status 500",
            "body {body:?} must report the status alone"
        );
        assert!(
            !err.message.contains("provider says"),
            "body {body:?} must not be labelled as the provider's words"
        );
    }
}

/// A `messages` entry that is not a string is not a message: dropped,
/// never `Debug`-formatted into one.
#[test]
fn a_non_string_messages_entry_is_dropped_not_rendered() {
    let err = error_for(500, r#"{"messages":[{"detail":"structured"},42]}"#);
    assert_eq!(err.message, "provider returned status 500");
    assert!(!err.message.contains("structured"), "{}", err.message);
    assert!(!err.message.contains("42"), "{}", err.message);
}

/// The impersonation case: a provider answering willikins' own redaction
/// marker still arrives labelled, so an agent reading the message can
/// tell "the provider said this" from "willikins redacted this".
#[test]
fn provider_text_cannot_impersonate_willikins_own_redaction_marker() {
    let err = error_for(400, r#"{"messages":["[REDACTED DopplerServiceToken]"]}"#);
    assert_eq!(
        err.message, "provider says: [REDACTED DopplerServiceToken]",
        "the marker must stay behind the label"
    );
    assert!(!err.message.starts_with("[REDACTED"), "{}", err.message);
}

/// A `401`/`403` body is dropped before anything can hold it, `messages`
/// and all: an operator learns a permission is missing, never what
/// Doppler said about the token.
#[test]
fn a_401_or_403_body_never_reaches_the_message_even_as_messages() {
    for status in [401, 403] {
        let err = error_for(
            status,
            r#"{"messages":["Invalid token dp.st.leakedvalue"]}"#,
        );
        assert_eq!(err.kind, ToolErrorKind::Provider, "status {status}");
        assert!(!err.message.contains("leakedvalue"), "{}", err.message);
        assert!(!err.message.contains("provider says"), "{}", err.message);
        assert!(err.message.contains("permission"), "{}", err.message);
    }
}
