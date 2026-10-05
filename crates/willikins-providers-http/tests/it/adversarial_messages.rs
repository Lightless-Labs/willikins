//! Adversarial pass over task 6, trust boundary 5: "a `ToolError.message`
//! is built from the HTTP status and the provider's own `message` field,
//! bounded and escaped [...], never from the raw body, and never with the
//! request URL or headers".
//!
//! A `ToolError` message is not ephemeral: `willikins-journal` stores it
//! verbatim inside a `NodeFinished` event, so anything a provider can push
//! into one it can push into the durable record an operator later reads.

use std::sync::Arc;
use std::time::Duration;

use willikins_core::{ToolError, ToolErrorKind};
use willikins_providers_http::{
    Credential, Http, MAX_MESSAGE_CHARS, MISSING_PERMISSION, Sleeper, UNAUTHENTICATED,
};

/// A distinctive string no message rule may ever let through.
const MARKER: &str = "wlkn-adversarial-marker-8h3qz1v";

/// A [`Sleeper`] that returns at once: these tests care about what a
/// message says, not about how long a retry waited.
struct NoWait;

impl Sleeper for NoWait {
    fn sleep(&self, _duration: Duration) {}
}

fn client(base_url: String) -> (Http, Arc<NoWait>) {
    let sleeper = Arc::new(NoWait);
    let http = Http::new(
        base_url,
        Vec::new(),
        Credential::for_testing("WILLIKINS_TEST_ADVERSARIAL", "test-token"),
    )
    .with_sleeper(sleeper.clone());
    (http, sleeper)
}

#[derive(serde::Deserialize, Debug)]
struct Thing {
    #[allow(dead_code)]
    name: String,
}

#[derive(serde::Deserialize, Debug)]
struct Counted {
    #[allow(dead_code)]
    count: u64,
}

// -----------------------------------------------------------------
// Claim 2: a provider response cannot smuggle text into a message.
// -----------------------------------------------------------------

/// A 2xx body that does not fit the caller's struct must not echo the body
/// back: `serde_json::Error`'s `Display` quotes the offending value
/// (`invalid type: string "…", expected u64`), and on the Doppler
/// `secret.get` shape that value is a plaintext secret.
#[test]
fn a_2xx_body_that_fails_to_deserialize_never_echoes_the_body() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(200)
        .with_body(format!(r#"{{"count":"{MARKER}"}}"#))
        .create();
    let (http, _sleeper) = client(server.url());
    let err = http
        .get::<Counted>("/thing")
        .expect_err("the body does not fit `Counted`");
    assert!(
        !err.message.contains(MARKER),
        "the parse error echoed the response body: {}",
        err.message
    );
    let tool_err: ToolError = err.into();
    assert!(
        !tool_err.message.contains(MARKER),
        "the parse error reached the agent: {}",
        tool_err.message
    );
}

/// An unknown-field parse failure quotes the field *name*, which on a
/// provider response is also body text.
#[test]
fn a_2xx_body_with_an_unexpected_shape_never_echoes_the_field_names() {
    #[derive(serde::Deserialize, Debug)]
    #[serde(deny_unknown_fields)]
    struct Strict {
        #[allow(dead_code)]
        name: String,
    }

    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(200)
        .with_body(format!(r#"{{"name":"widget","{MARKER}":1}}"#))
        .create();
    let (http, _sleeper) = client(server.url());
    let err = http.get::<Strict>("/thing").expect_err("unknown field");
    assert!(
        !err.message.contains(MARKER),
        "the parse error echoed a body field name: {}",
        err.message
    );
}

/// Trust boundary 5's 401/403 rule has to hold at *construction*, not only
/// at the `ToolError` conversion: `ProviderError` is public, its `message`
/// is public, and tasks 7 and 8 handle these values (and may log them)
/// before any conversion happens.
#[test]
fn a_401_or_403_body_never_reaches_the_provider_error_message() {
    let mut server = mockito::Server::new();
    for (path, status) in [("/unauthorized", 401), ("/forbidden", 403)] {
        server
            .mock("GET", path)
            .with_status(status)
            .with_body(format!(r#"{{"message":"token {MARKER} lacks a scope"}}"#))
            .create();
    }
    let (http, _sleeper) = client(server.url());

    for (path, status) in [("/unauthorized", 401), ("/forbidden", 403)] {
        let err = http.get::<Thing>(path).expect_err("an auth failure");
        assert_eq!(err.status, Some(status));
        assert!(
            !err.message.contains(MARKER),
            "ProviderError carried the {status} body: {}",
            err.message
        );
        // The 401/403 split (milestone 3c, decision (a)): the two
        // statuses now read differently. A `401` means the credential
        // itself was rejected and deliberately says nothing about a
        // permission; a `403` still names the permission problem, byte
        // for byte unchanged.
        if status == 401 {
            assert_eq!(
                err.message, UNAUTHENTICATED,
                "the 401 message: {}",
                err.message
            );
            assert!(
                !err.message.contains("permission"),
                "the 401 message should not claim a permission problem: {}",
                err.message
            );
        } else {
            assert_eq!(
                err.message, MISSING_PERMISSION,
                "the 403 message: {}",
                err.message
            );
        }
        let tool_err: ToolError = err.into();
        assert_eq!(tool_err.kind, ToolErrorKind::Provider);
        assert!(!tool_err.message.contains(MARKER));
    }
}

/// A provider's own words must be labelled as such. `escape_debug` leaves
/// `[`, `]` and letters alone, so a provider that answers with
/// `[REDACTED DopplerServiceToken]` would otherwise hand an agent a string
/// indistinguishable from willikins' own redaction marker — the same
/// problem trust boundary 4 solves for document text with `document says:`.
#[test]
fn provider_text_is_labelled_so_it_cannot_impersonate_willikins() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(500)
        .with_body(r#"{"message":"[REDACTED DopplerServiceToken]"}"#)
        .create();
    let (http, _sleeper) = client(server.url());
    let err = http.get::<Thing>("/thing").expect_err("500");
    let tool_err: ToolError = err.into();
    assert!(
        tool_err.message.starts_with("provider says:"),
        "provider text must be labelled: {}",
        tool_err.message
    );
}

/// willikins' own words about a response carry no provider label.
#[test]
fn willikins_own_words_are_not_labelled_as_the_providers() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(500)
        .with_body("<html>not json</html>")
        .create();
    let (http, _sleeper) = client(server.url());
    let err = http.get::<Thing>("/thing").expect_err("500");
    assert!(!err.message.contains("provider says:"), "{}", err.message);
    assert!(err.message.contains("500"), "{}", err.message);
}

/// A `message` field that is an object or an array is not a message: it is
/// dropped, never `Debug`-formatted into one.
#[test]
fn a_message_field_that_is_not_a_string_is_dropped() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/object")
        .with_status(500)
        .with_body(format!(r#"{{"message":{{"detail":"{MARKER}"}}}}"#))
        .create();
    server
        .mock("GET", "/array")
        .with_status(500)
        .with_body(format!(r#"{{"message":["{MARKER}"]}}"#))
        .create();
    let (http, _sleeper) = client(server.url());

    for path in ["/object", "/array"] {
        let err = http.get::<Thing>(path).expect_err("500");
        assert!(
            !err.message.contains(MARKER),
            "a non-string `message` leaked at {path}: {}",
            err.message
        );
        assert!(err.message.contains("500"), "{}", err.message);
    }
}

/// Doppler documents no error-body schema but sends a `messages` array; the
/// plan's Doppler section says the client reads it. Nothing downstream ever
/// sees the raw body, so this can only happen here.
#[test]
fn a_messages_array_is_read_when_there_is_no_message_field() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(404)
        .with_body(r#"{"messages":["Project not found"],"success":false}"#)
        .create();
    let (http, _sleeper) = client(server.url());
    let err = http.get::<Thing>("/thing").expect_err("404");
    assert!(
        err.message.contains("Project not found"),
        "the `messages` array was ignored: {}",
        err.message
    );
}

/// 300 four-byte characters truncate to 256 *characters* and stay valid
/// UTF-8 — never 256 bytes through the middle of one.
#[test]
fn a_long_multibyte_message_truncates_to_characters_not_bytes() {
    let long: String = "𝄞".repeat(300);
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(500)
        .with_body(format!(r#"{{"message":"{long}"}}"#))
        .create();
    let (http, _sleeper) = client(server.url());
    let err = http.get::<Thing>("/thing").expect_err("500");
    let clef_count = err.message.chars().filter(|c| *c == '𝄞').count();
    assert_eq!(clef_count, MAX_MESSAGE_CHARS);
    assert!(std::str::from_utf8(err.message.as_bytes()).is_ok());
}

/// Control characters and ANSI escapes in a provider message never reach a
/// terminal as control characters.
#[test]
fn control_characters_and_ansi_escapes_are_escaped() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(500)
        // JSON escapes keep the body itself well-formed: ESC, BEL, a
        // newline and a carriage return inside the provider's `message`.
        .with_body(r#"{"message":"\u001b[31mred\u0007\nnext line\r"}"#)
        .create();
    let (http, _sleeper) = client(server.url());
    let err = http.get::<Thing>("/thing").expect_err("500");
    for forbidden in ['\u{1b}', '\u{7}', '\n', '\r'] {
        assert!(
            !err.message.contains(forbidden),
            "{forbidden:?} survived escaping: {}",
            err.message
        );
    }
    assert!(err.message.contains("\\u{1b}"), "{}", err.message);
}
