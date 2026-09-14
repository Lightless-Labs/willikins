//! Adversarial pass over task 6, trust boundary 1: a credential's bytes
//! leave the process in an `Authorization` header and nowhere else — not
//! through an error about a header that could not be built, not through a
//! transport failure's own words, and not to a third host a provider's
//! `Location` chose.

use std::sync::Arc;
use std::time::Duration;

use willikins_providers_http::{Credential, Http, Sleeper};

/// A distinctive string no error may ever carry.
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

// -----------------------------------------------------------------
// Claim 1: a credential's bytes leave only in an Authorization header.
// -----------------------------------------------------------------

/// A credential whose bytes cannot be a header value (a format regex that
/// matched everything, or an operator who exported a value with a newline)
/// fails the request without printing the value: `http`'s
/// `InvalidHeaderValue` prints fixed text, and this pins that it stays so.
#[test]
fn a_credential_that_is_not_a_valid_header_value_never_reaches_the_error() {
    let mut server = mockito::Server::new();
    server.mock("GET", "/thing").with_status(200).create();
    let http = Http::new(
        server.url(),
        Vec::new(),
        Credential::for_testing("WILLIKINS_TEST_ADVERSARIAL_BAD", &format!("bad\n{MARKER}")),
    )
    .with_sleeper(Arc::new(NoWait));
    let err = http
        .get::<Thing>("/thing")
        .expect_err("the header value is invalid");
    assert!(
        !err.message.contains(MARKER),
        "the invalid header value leaked: {}",
        err.message
    );
}

/// A transport failure's message never carries the request URL (trust
/// boundary 5), including the `BadUri` shape, which is the one `ureq`
/// error that prints the URI it was given.
#[test]
fn a_transport_failure_message_never_carries_the_request_url() {
    let (refused, _sleeper) = client("http://127.0.0.1:1".to_string());
    let err = refused
        .get::<Thing>("/secret-path-name")
        .expect_err("connection refused");
    assert_eq!(err.status, None);
    assert!(
        !err.message.contains("127.0.0.1") && !err.message.contains("secret-path-name"),
        "a refused connection named the URL: {}",
        err.message
    );

    let (bad_uri, _sleeper) = client("not a url at all".to_string());
    let err = bad_uri
        .get::<Thing>("/secret-path-name")
        .expect_err("not a URL");
    assert_eq!(err.status, None);
    assert!(
        !err.message.contains("secret-path-name") && !err.message.contains("not a url"),
        "a bad URI named the URL: {}",
        err.message
    );
}

/// A provider-supplied redirect is not followed: the response is returned
/// as it stands, so nothing parses a third party's body into a typed
/// provider response.
#[test]
fn a_redirect_is_not_followed() {
    let mut elsewhere = mockito::Server::new();
    let never_called = elsewhere
        .mock("GET", "/thing")
        .with_status(200)
        .with_body(r#"{"name":"impostor"}"#)
        .expect(0)
        .create();

    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(302)
        .with_header("Location", &format!("{}/thing", elsewhere.url()))
        .create();

    let (http, _sleeper) = client(server.url());
    let err = http
        .get::<Thing>("/thing")
        .expect_err("a redirect is not a success");
    assert_eq!(err.status, Some(302));
    never_called.assert();
}
