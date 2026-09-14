//! Adversarial pass over task 6's retry policy: `GET`, `PUT` and `DELETE`
//! are retried exactly three times beyond the first attempt, a `POST` is
//! never retried at all (not even when the failure was a transport error
//! that may still have created something), and a `Retry-After` a provider
//! chooses can neither make willikins wait unboundedly nor panic it.

use std::io::Read;
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_providers_http::{Credential, Http, Sleeper};

/// The longest wait a `Retry-After` may ask for. Written out rather than
/// imported so this file pins the *number*, not whatever the crate
/// currently exports under that name.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// A [`Sleeper`] that records what it was asked to wait and returns at once.
#[derive(Default)]
struct RecordingSleeper {
    durations: Mutex<Vec<Duration>>,
}

impl RecordingSleeper {
    fn durations(&self) -> Vec<Duration> {
        self.durations.lock().expect("not poisoned").clone()
    }
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.durations.lock().expect("not poisoned").push(duration);
    }
}

fn client(base_url: String) -> (Http, Arc<RecordingSleeper>) {
    let sleeper = Arc::new(RecordingSleeper::default());
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
// Claim 3: retries, exactly, and never a POST.
// -----------------------------------------------------------------

#[test]
fn a_get_is_attempted_exactly_four_times_and_sleeps_exactly_three() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/thing")
        .with_status(503)
        .with_body("{}")
        .expect(4)
        .create();
    let (http, sleeper) = client(server.url());
    http.get::<Thing>("/thing").expect_err("still 503");
    mock.assert();
    assert_eq!(sleeper.durations().len(), 3);
}

#[test]
fn a_put_is_attempted_exactly_four_times() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("PUT", "/thing")
        .with_status(503)
        .with_body("{}")
        .expect(4)
        .create();
    let (http, sleeper) = client(server.url());
    http.put::<Thing>("/thing", &serde_json::json!({"name": "widget"}))
        .expect_err("still 503");
    mock.assert();
    assert_eq!(sleeper.durations().len(), 3);
}

#[test]
fn a_delete_is_attempted_exactly_four_times() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("DELETE", "/thing")
        .with_status(503)
        .with_body("{}")
        .expect(4)
        .create();
    let (http, sleeper) = client(server.url());
    http.delete("/thing").expect_err("still 503");
    mock.assert();
    assert_eq!(sleeper.durations().len(), 3);
}

#[test]
fn a_post_is_never_retried_on_429_or_5xx() {
    let mut server = mockito::Server::new();
    let rate_limited = server
        .mock("POST", "/rate-limited")
        .with_status(429)
        .with_header("Retry-After", "1")
        .with_body("{}")
        .expect(1)
        .create();
    let server_error = server
        .mock("POST", "/server-error")
        .with_status(500)
        .with_body("{}")
        .expect(1)
        .create();
    let (http, sleeper) = client(server.url());

    let err = http
        .post::<Thing>("/rate-limited", &serde_json::json!({}))
        .expect_err("429");
    assert_eq!(err.status, Some(429));
    let err = http
        .post::<Thing>("/server-error", &serde_json::json!({}))
        .expect_err("500");
    assert_eq!(err.status, Some(500));

    rate_limited.assert();
    server_error.assert();
    assert!(
        sleeper.durations().is_empty(),
        "a POST must never wait to retry: {:?}",
        sleeper.durations()
    );
}

/// Counts TCP connections a client makes, answering each with an immediate
/// close: the only way to observe how many times a *transport* failure was
/// retried, which no mock-server hit counter can see.
fn counting_listener() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
    let addr = listener.local_addr().expect("has an address");
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = accepted.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            counter.fetch_add(1, Ordering::SeqCst);
            // Read whatever the client already sent, then drop the
            // connection without answering: a transport failure, not a
            // status code.
            let mut scratch = [0_u8; 1024];
            let _ = stream.read(&mut scratch);
        }
    });
    (format!("http://{addr}"), accepted)
}

#[test]
fn a_post_transport_failure_is_attempted_once_and_a_get_four_times() {
    let (url, accepted) = counting_listener();
    let (http, _sleeper) = client(url);

    http.post::<Thing>("/thing", &serde_json::json!({}))
        .expect_err("the connection closed");
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "a POST whose outcome is unknown must never be re-sent"
    );

    accepted.store(0, Ordering::SeqCst);
    http.get::<Thing>("/thing")
        .expect_err("the connection closed");
    assert_eq!(accepted.load(Ordering::SeqCst), 4);
}

#[test]
fn a_retry_after_of_zero_waits_nothing() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(429)
        .with_header("Retry-After", "0")
        .with_body("{}")
        .expect(1)
        .create();
    server
        .mock("GET", "/thing")
        .with_status(200)
        .with_body(r#"{"name":"widget"}"#)
        .expect(1)
        .create();
    let (http, sleeper) = client(server.url());
    http.get::<Thing>("/thing").expect("succeeds on retry");
    assert_eq!(sleeper.durations(), vec![Duration::ZERO]);
}

/// A provider that asks for a day must not get one: a `Retry-After` is
/// honoured only up to [`MAX_RETRY_AFTER`], after which willikins retries
/// once more and reports the failure rather than blocking an apply.
#[test]
fn an_enormous_retry_after_is_capped() {
    for value in ["86400", "18446744073709551615"] {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/thing")
            .with_status(429)
            .with_header("Retry-After", value)
            .with_body("{}")
            .expect(1)
            .create();
        server
            .mock("GET", "/thing")
            .with_status(200)
            .with_body(r#"{"name":"widget"}"#)
            .expect(1)
            .create();
        let (http, sleeper) = client(server.url());
        http.get::<Thing>("/thing").expect("succeeds on retry");
        assert_eq!(
            sleeper.durations(),
            vec![MAX_RETRY_AFTER],
            "Retry-After: {value} was not capped"
        );
    }
}

/// A `Retry-After` that is neither seconds nor an `IMF-fixdate` — including
/// an absurd year, which must not overflow anything — falls back to
/// jittered backoff rather than panicking or waiting forever.
#[test]
fn a_malformed_retry_after_falls_back_to_backoff() {
    for value in [
        "soon",
        "-1",
        "1.5",
        "Sun, 01 Jan 9223372036854775807 00:00:00 GMT",
        "Thu, 01 Jan 292277026596 00:00:00 GMT",
        "Thursday, 01-Jan-70 00:00:10 GMT",
    ] {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/thing")
            .with_status(429)
            .with_header("Retry-After", value)
            .with_body("{}")
            .expect(1)
            .create();
        server
            .mock("GET", "/thing")
            .with_status(200)
            .with_body(r#"{"name":"widget"}"#)
            .expect(1)
            .create();
        let (http, sleeper) = client(server.url());
        http.get::<Thing>("/thing").expect("succeeds on retry");
        let waited = sleeper.durations();
        assert_eq!(waited.len(), 1, "Retry-After: {value}");
        assert!(
            waited[0] >= Duration::from_millis(100) && waited[0] < Duration::from_millis(300),
            "Retry-After: {value} did not fall back to the first backoff step: {:?}",
            waited[0]
        );
    }
}

/// An `IMF-fixdate` already in the past asks for an immediate retry.
#[test]
fn a_retry_after_date_in_the_past_waits_nothing() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/thing")
        .with_status(429)
        .with_header("Retry-After", "Wed, 21 Oct 2015 07:28:00 GMT")
        .with_body("{}")
        .expect(1)
        .create();
    server
        .mock("GET", "/thing")
        .with_status(200)
        .with_body(r#"{"name":"widget"}"#)
        .expect(1)
        .create();
    let (http, sleeper) = client(server.url());
    http.get::<Thing>("/thing").expect("succeeds on retry");
    assert_eq!(sleeper.durations(), vec![Duration::ZERO]);
}

/// A 429 followed by a 5xx keeps counting attempts against the same budget:
/// four attempts in total, whatever mix of retryable statuses they are.
#[test]
fn a_429_then_5xx_sequence_shares_one_attempt_budget() {
    let mut server = mockito::Server::new();
    let rate_limited = server
        .mock("GET", "/thing")
        .with_status(429)
        .with_header("Retry-After", "0")
        .with_body("{}")
        .expect(1)
        .create();
    let failing = server
        .mock("GET", "/thing")
        .with_status(503)
        .with_body("{}")
        .expect(3)
        .create();
    let (http, sleeper) = client(server.url());
    let err = http.get::<Thing>("/thing").expect_err("never recovers");
    assert_eq!(err.status, Some(503));
    rate_limited.assert();
    failing.assert();
    assert_eq!(sleeper.durations().len(), 3);
}
