//! [`Http`]: a synchronous HTTP client wrapper shared by every live
//! provider tool.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-providers-http` crate contract: connect timeout 10 s, total
//! timeout 30 s, `GET`/`PUT`/`DELETE` retried up to three times on `429`,
//! `5xx`, and transport errors with jittered exponential backoff that
//! honours `Retry-After`; `POST` is never retried. Every response body is
//! parsed into a caller-supplied typed struct; an error body is parsed for
//! its provider `message` field only.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde::de::DeserializeOwned;
use ureq::Agent;
use ureq::http::header::RETRY_AFTER;

use crate::credential::Credential;
use crate::error::{ProviderError, bounded_message};
use crate::retry_after;
use crate::sleeper::{self, Sleeper};

/// `GET`, `PUT`, and `DELETE` are retried this many times beyond the
/// initial attempt (four attempts total); `POST` is retried zero times.
const MAX_RETRIES: u32 = 3;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Base delay exponential backoff scales from, before jitter. Only ever
/// observed by a test through an injected [`Sleeper`], so its exact value
/// does not affect correctness — only how long a caller with the real
/// sleeper waits.
const BASE_BACKOFF: Duration = Duration::from_millis(200);

/// A synchronous HTTP client bound to one provider's base URL and one
/// [`Credential`].
pub struct Http {
    agent: Agent,
    base_url: String,
    default_headers: Vec<(String, String)>,
    credential: Credential,
    sleeper: Arc<dyn Sleeper>,
}

impl Http {
    /// Build a client for `base_url` (no trailing slash), sending
    /// `default_headers` on every request in addition to `credential`'s
    /// `Authorization` header.
    #[must_use]
    pub fn new(
        base_url: impl Into<String>,
        default_headers: Vec<(String, String)>,
        credential: Credential,
    ) -> Self {
        let config = Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(TOTAL_TIMEOUT))
            .http_status_as_error(false)
            .build();
        Self {
            agent: Agent::new_with_config(config),
            base_url: base_url.into(),
            default_headers,
            credential,
            sleeper: sleeper::real(),
        }
    }

    /// Replace the sleeper a retry loop waits with — the seam a test
    /// injects a non-waiting one through.
    #[must_use]
    pub fn with_sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        self.sleeper = sleeper;
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn apply_headers<B>(&self, mut builder: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        for (name, value) in &self.default_headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        builder
    }

    /// `GET path`, retried.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] for a non-2xx response (after retrying,
    /// where applicable) or a transport failure that outlasted retrying.
    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ProviderError> {
        let url = self.url(path);
        let (status, body) = self.run_retrying(true, || {
            let builder = self.apply_headers(self.agent.get(url.as_str()));
            self.credential.authorize(builder).call()
        })?;
        Self::finish(status, &body)
    }

    /// `PUT path` with a JSON-serialized `body`, retried.
    ///
    /// # Errors
    ///
    /// See [`Http::get`].
    pub fn put<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T, ProviderError> {
        let url = self.url(path);
        let (status, response_body) = self.run_retrying(true, || {
            let builder = self.apply_headers(self.agent.put(url.as_str()));
            self.credential.authorize(builder).send_json(body)
        })?;
        Self::finish(status, &response_body)
    }

    /// `POST path` with a JSON-serialized `body`. Never retried: a `POST`
    /// that fails ambiguously (a transport error with no response) may or
    /// may not have taken effect, and retrying it blind risks a second
    /// creation. A caller that needs to know re-`read`s instead.
    ///
    /// # Errors
    ///
    /// See [`Http::get`].
    pub fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T, ProviderError> {
        let url = self.url(path);
        let (status, response_body) = self.run_retrying(false, || {
            let builder = self.apply_headers(self.agent.post(url.as_str()));
            self.credential.authorize(builder).send_json(body)
        })?;
        Self::finish(status, &response_body)
    }

    /// `DELETE path`, retried. Any response body is ignored on success:
    /// every delete endpoint this workspace calls answers with an empty
    /// `204`.
    ///
    /// # Errors
    ///
    /// See [`Http::get`].
    pub fn delete(&self, path: &str) -> Result<(), ProviderError> {
        let url = self.url(path);
        let (status, body) = self.run_retrying(true, || {
            let builder = self.apply_headers(self.agent.delete(url.as_str()));
            self.credential.authorize(builder).call()
        })?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(provider_error_from_body(status, &body))
        }
    }

    /// Turn a `(status, body)` pair that already survived retrying into
    /// either the caller's typed success value or a [`ProviderError`].
    fn finish<T: DeserializeOwned>(status: u16, body: &str) -> Result<T, ProviderError> {
        if (200..300).contains(&status) {
            serde_json::from_str(body).map_err(|err| {
                ProviderError::new(
                    Some(status),
                    bounded_message(&format!("could not parse response body: {err}")),
                )
            })
        } else {
            Err(provider_error_from_body(status, body))
        }
    }

    /// Run `attempt` (one fully-built request send) up to
    /// [`MAX_RETRIES`] additional times when `retryable` and the response
    /// (or transport failure) says to. Returns the final `(status,
    /// body_text)` pair for any response actually received — 2xx or
    /// not — or a [`ProviderError`] only when every attempt failed at the
    /// transport level.
    fn run_retrying(
        &self,
        retryable: bool,
        mut attempt: impl FnMut() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<(u16, String), ProviderError> {
        let max_attempts = if retryable { MAX_RETRIES + 1 } else { 1 };
        for attempt_index in 0..max_attempts {
            let is_last = attempt_index + 1 == max_attempts;
            match attempt() {
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    let body = response.body_mut().read_to_string().unwrap_or_default();
                    if !is_last && is_retryable_status(status) {
                        let retry_after = response
                            .headers()
                            .get(RETRY_AFTER)
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| retry_after::parse(value, SystemTime::now()));
                        self.sleeper
                            .sleep(backoff_delay(attempt_index, retry_after));
                        continue;
                    }
                    return Ok((status, body));
                }
                Err(err) => {
                    if !is_last {
                        self.sleeper.sleep(backoff_delay(attempt_index, None));
                        continue;
                    }
                    return Err(ProviderError::new(
                        None,
                        bounded_message(&format!("request failed: {err}")),
                    ));
                }
            }
        }
        unreachable!("the loop above always returns on its last iteration")
    }
}

/// Whether `status` is worth retrying: `429` (rate limited) or any `5xx`.
fn is_retryable_status(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// The delay before retry number `attempt` (0-based): `retry_after` when
/// the response named one, otherwise jittered exponential backoff from
/// [`BASE_BACKOFF`].
fn backoff_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(delay) = retry_after {
        return delay;
    }
    let exponential = BASE_BACKOFF.saturating_mul(1 << attempt.min(16));
    let jitter = rand::random_range(0.5..1.5);
    Duration::from_secs_f64(exponential.as_secs_f64() * jitter)
}

/// Build a [`ProviderError`] from a status this crate treats as a failure
/// and its response body: the body is parsed as JSON only to pull a
/// `message` field (and, for the `409`/`422` "already exists" case, an
/// `errors[].code` field) out of it; anything else in the body — and the
/// body itself, if it does not parse as JSON at all — never reaches the
/// resulting message.
fn provider_error_from_body(status: u16, body: &str) -> ProviderError {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();

    let message = parsed
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .map_or_else(
            || bounded_message(&format!("provider returned status {status}")),
            bounded_message,
        );

    let already_exists = parsed
        .as_ref()
        .and_then(|value| value.get("errors"))
        .and_then(|value| value.as_array())
        .is_some_and(|errors| {
            errors.iter().any(|error| {
                error.get("code").and_then(serde_json::Value::as_str) == Some("already_exists")
            })
        });

    if already_exists {
        ProviderError::already_exists(Some(status), message)
    } else {
        ProviderError::new(Some(status), message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A [`Sleeper`] that never actually waits, recording every requested
    /// duration so a test can assert on retry timing decisions without
    /// spending real wall-clock time.
    #[derive(Default)]
    struct RecordingSleeper {
        durations: Mutex<Vec<Duration>>,
    }

    impl Sleeper for RecordingSleeper {
        fn sleep(&self, duration: Duration) {
            self.durations.lock().expect("not poisoned").push(duration);
        }
    }

    fn credential() -> Credential {
        Credential::for_testing("WILLIKINS_TEST_HTTP_CREDENTIAL", "test-token")
    }

    fn client(base_url: String) -> (Http, Arc<RecordingSleeper>) {
        let sleeper = Arc::new(RecordingSleeper::default());
        let http = Http::new(base_url, Vec::new(), credential()).with_sleeper(sleeper.clone());
        (http, sleeper)
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Thing {
        name: String,
    }

    #[test]
    fn get_success_parses_the_body() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/thing")
            .with_status(200)
            .with_body(r#"{"name":"widget"}"#)
            .create();
        let (http, _sleeper) = client(server.url());
        let thing: Thing = http.get("/thing").expect("succeeds");
        assert_eq!(thing.name, "widget");
        mock.assert();
    }

    #[test]
    fn a_429_with_retry_after_one_then_200_succeeds_with_two_calls() {
        let mut server = mockito::Server::new();
        let first = server
            .mock("GET", "/thing")
            .with_status(429)
            .with_header("Retry-After", "1")
            .with_body("{}")
            .expect(1)
            .create();
        let second = server
            .mock("GET", "/thing")
            .with_status(200)
            .with_body(r#"{"name":"widget"}"#)
            .expect(1)
            .create();
        let (http, _sleeper) = client(server.url());
        let thing: Thing = http.get("/thing").expect("succeeds after one retry");
        assert_eq!(thing.name, "widget");
        first.assert();
        second.assert();
    }

    #[test]
    fn three_503s_then_200_succeeds() {
        let mut server = mockito::Server::new();
        let failing = server
            .mock("GET", "/thing")
            .with_status(503)
            .with_body("{}")
            .expect(3)
            .create();
        let succeeding = server
            .mock("GET", "/thing")
            .with_status(200)
            .with_body(r#"{"name":"widget"}"#)
            .expect(1)
            .create();
        let (http, _sleeper) = client(server.url());
        let thing: Thing = http.get("/thing").expect("succeeds on the fourth attempt");
        assert_eq!(thing.name, "widget");
        failing.assert();
        succeeding.assert();
    }

    #[test]
    fn four_503s_fail_with_provider_and_a_bounded_message() {
        let mut server = mockito::Server::new();
        let long_message = "x".repeat(10_000);
        let mock = server
            .mock("GET", "/thing")
            .with_status(503)
            .with_body(format!(r#"{{"message":"{long_message}"}}"#))
            .expect(4)
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http.get::<Thing>("/thing").expect_err("still failing");
        assert_eq!(err.status, Some(503));
        assert_eq!(err.message.chars().count(), 256);
        mock.assert();
    }

    #[test]
    fn a_post_503_is_not_retried() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/thing")
            .with_status(503)
            .with_body("{}")
            .expect(1)
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http
            .post::<Thing>("/thing", &serde_json::json!({}))
            .expect_err("not retried");
        assert_eq!(err.status, Some(503));
        mock.assert();
    }

    #[test]
    fn a_transport_failure_maps_to_provider() {
        // Nothing is listening on this port: every attempt is a
        // connection failure, which is a transport error, not a status
        // code.
        let (http, _sleeper) = client("http://127.0.0.1:1".to_string());
        let err = http.get::<Thing>("/thing").expect_err("connection refused");
        assert_eq!(err.status, None);
    }

    #[test]
    fn a_body_that_is_not_json_yields_a_message_naming_the_status() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/thing")
            .with_status(500)
            .with_body("<html>not json</html>")
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http.get::<Thing>("/thing").expect_err("not json");
        assert_eq!(err.status, Some(500));
        assert!(err.message.contains("500"));
    }

    #[test]
    fn a_422_with_already_exists_is_conflict_and_without_it_is_provider() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/exists")
            .with_status(422)
            .with_body(r#"{"message":"already exists","errors":[{"code":"already_exists"}]}"#)
            .create();
        server
            .mock("GET", "/other")
            .with_status(422)
            .with_body(r#"{"message":"bad field","errors":[{"code":"invalid"}]}"#)
            .create();
        let (http, _sleeper) = client(server.url());

        let already_exists_err = http.get::<Thing>("/exists").expect_err("422");
        assert!(already_exists_err.already_exists);

        let other_err = http.get::<Thing>("/other").expect_err("422");
        assert!(!other_err.already_exists);
    }

    #[test]
    fn put_and_delete_are_retried_like_get() {
        let mut server = mockito::Server::new();
        let put_failing = server
            .mock("PUT", "/thing")
            .with_status(503)
            .with_body("{}")
            .expect(1)
            .create();
        let put_succeeding = server
            .mock("PUT", "/thing")
            .with_status(200)
            .with_body(r#"{"name":"widget"}"#)
            .expect(1)
            .create();
        let delete_failing = server
            .mock("DELETE", "/thing")
            .with_status(503)
            .with_body("{}")
            .expect(1)
            .create();
        let delete_succeeding = server
            .mock("DELETE", "/thing")
            .with_status(204)
            .expect(1)
            .create();

        let (http, _sleeper) = client(server.url());
        let thing: Thing = http
            .put("/thing", &serde_json::json!({"name": "widget"}))
            .expect("succeeds after one retry");
        assert_eq!(thing.name, "widget");
        put_failing.assert();
        put_succeeding.assert();

        http.delete("/thing").expect("succeeds after one retry");
        delete_failing.assert();
        delete_succeeding.assert();
    }

    #[test]
    fn every_request_carries_the_default_headers_and_the_authorization_header() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/thing")
            .match_header("X-Extra", "value")
            .match_header(
                "authorization",
                mockito::Matcher::Regex("Bearer .+".to_string()),
            )
            .with_status(200)
            .with_body(r#"{"name":"widget"}"#)
            .create();
        let sleeper = Arc::new(RecordingSleeper::default());
        let http = Http::new(
            server.url(),
            vec![("X-Extra".to_string(), "value".to_string())],
            credential(),
        )
        .with_sleeper(sleeper);
        let thing: Thing = http.get("/thing").expect("succeeds");
        assert_eq!(thing.name, "widget");
        mock.assert();
    }
}
