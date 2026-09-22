//! [`Http`]: a synchronous HTTP client wrapper shared by every live
//! provider tool.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-providers-http` crate contract: connect timeout 10 s, total
//! timeout 30 s, `GET`/`PUT`/`PATCH`/`DELETE` retried up to three times on
//! `429`, `5xx`, and transport errors with jittered exponential backoff
//! that honours `Retry-After`; `POST` is never retried. `PATCH` joined
//! the retried set for `willikins-providers-appstore`
//! (`PATCH /v1/bundleIds/{id}`, App Store Connect's own convergence verb
//! for a bundle id's `name`), on the same idempotence reasoning `PUT`
//! already gets. Every response body is parsed into a caller-supplied
//! typed struct; an error body is parsed for its provider `message` field
//! only.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde::de::DeserializeOwned;
use ureq::Agent;
use ureq::http::header::RETRY_AFTER;

use crate::credential::Credential;
use crate::error::{
    MISSING_PERMISSION, ProviderError, ProviderFacts, bounded_message, provider_says,
};
use crate::retry_after;
use crate::sleeper::{self, Sleeper};

/// `GET`, `PUT`, and `DELETE` are retried this many times beyond the
/// initial attempt (four attempts total); `POST` is retried zero times.
const MAX_RETRIES: u32 = 3;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest a single `Retry-After` may make willikins wait. A
/// provider that asks for a day gets one minute: waiting longer blocks an
/// apply on a value the provider alone chooses, and a provider that means
/// it answers `429` again after the minute, which fails the call honestly
/// instead. Worst case per request is therefore four attempts of
/// [`TOTAL_TIMEOUT`] plus three waits of this.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

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
    /// `None`: every request carries `credential` via
    /// [`Credential::authorize`] (`Authorization: Bearer <token>`), the
    /// shape every provider but `SigNoz` uses. `Some(name)`: every request
    /// carries it via [`Credential::authorize_header`] instead, verbatim
    /// under the header named `name` — `SigNoz`'s documented
    /// `SigNoz-Api-Key` (`docs/research/2026-09-20-signoz-ingestion-keys.md`,
    /// section 1). Set once at construction, never per call, so a
    /// provider crate cannot build the header itself at a call site this
    /// crate's guards cannot see.
    credential_header: Option<&'static str>,
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
        Self::with_credential_header(base_url, default_headers, credential, None)
    }

    /// Build a client exactly like [`Self::new`], except the credential
    /// is sent under the header named `header_name` (verbatim, no
    /// `Bearer ` prefix) instead of `Authorization`. `header_name: None`
    /// is [`Self::new`]'s own behaviour.
    #[must_use]
    pub fn with_credential_header(
        base_url: impl Into<String>,
        default_headers: Vec<(String, String)>,
        credential: Credential,
        header_name: Option<&'static str>,
    ) -> Self {
        let config = Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(TOTAL_TIMEOUT))
            .http_status_as_error(false)
            // A provider API does not redirect. Following one would send
            // the request somewhere the provider's response chose (ureq
            // strips the `Authorization` header across a redirect, so the
            // credential does not travel, but the body that comes back
            // would still be parsed as that provider's answer), and a
            // same-host redirect re-sent without authorization would come
            // back `401` and be reported as a missing permission. Zero
            // redirects means the `3xx` itself is returned and surfaces as
            // a `ProviderError` naming the status.
            .max_redirects(0)
            .build();
        Self {
            agent: Agent::new_with_config(config),
            base_url: base_url.into(),
            default_headers,
            credential,
            credential_header: header_name,
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

    /// Attach [`Self::credential`] to `builder`, via
    /// [`Credential::authorize_header`] when [`Self::credential_header`]
    /// names one, [`Credential::authorize`] (the `Authorization: Bearer`
    /// default) otherwise. The one call site every request method below
    /// goes through, so a provider crate never chooses between the two
    /// itself.
    fn apply_credential<B>(&self, builder: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        match self.credential_header {
            Some(name) => self.credential.authorize_header(builder, name),
            None => self.credential.authorize(builder),
        }
    }

    /// `GET path`, retried.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] for a non-2xx response (after retrying,
    /// where applicable) or a transport failure that outlasted retrying.
    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ProviderError> {
        let url = self.url(path);
        let (status, body, facts) = self.run_retrying(true, || {
            let builder = self.apply_headers(self.agent.get(url.as_str()));
            self.apply_credential(builder).call()
        })?;
        Self::finish(status, &body, facts)
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
        let (status, response_body, facts) = self.run_retrying(true, || {
            let builder = self.apply_headers(self.agent.put(url.as_str()));
            self.apply_credential(builder).send_json(body)
        })?;
        Self::finish(status, &response_body, facts)
    }

    /// `PATCH path` with a JSON-serialized `body`, retried -- App Store
    /// Connect's own convergence verb (`PATCH /v1/bundleIds/{id}`, say),
    /// which no provider before this crate's Apple client needed.
    /// Idempotent the same way `PUT` is, so it is retried like
    /// [`Http::get`]/[`Http::put`] rather than treated like [`Http::post`].
    ///
    /// # Errors
    ///
    /// See [`Http::get`].
    pub fn patch<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T, ProviderError> {
        let url = self.url(path);
        let (status, response_body, facts) = self.run_retrying(true, || {
            let builder = self.apply_headers(self.agent.patch(url.as_str()));
            self.apply_credential(builder).send_json(body)
        })?;
        Self::finish(status, &response_body, facts)
    }

    /// `PUT path` with a JSON-serialized `body`, retried, for an endpoint
    /// documented to answer success with no body at all (or with a body a
    /// caller has no typed shape for and does not need — GitHub's Actions
    /// secret `PUT` answers `201` on creation and `204`, genuinely
    /// bodyless, on update, and a caller only cares that one of the two
    /// happened). Any 2xx succeeds; the body, if any, is never parsed.
    ///
    /// # Errors
    ///
    /// See [`Http::get`].
    pub fn put_empty(&self, path: &str, body: &impl Serialize) -> Result<(), ProviderError> {
        let url = self.url(path);
        let (status, response_body, facts) = self.run_retrying(true, || {
            let builder = self.apply_headers(self.agent.put(url.as_str()));
            self.apply_credential(builder).send_json(body)
        })?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(provider_error_from_body(status, &response_body).with_facts(facts))
        }
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
        let (status, response_body, facts) = self.run_retrying(false, || {
            let builder = self.apply_headers(self.agent.post(url.as_str()));
            self.apply_credential(builder).send_json(body)
        })?;
        Self::finish(status, &response_body, facts)
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
        let (status, body, facts) = self.run_retrying(true, || {
            let builder = self.apply_headers(self.agent.delete(url.as_str()));
            self.apply_credential(builder).call()
        })?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(provider_error_from_body(status, &body).with_facts(facts))
        }
    }

    /// `DELETE path` with a JSON-serialized `body`, retried. HTTP does not
    /// forbid a body on `DELETE`, and Doppler's service-token revocation
    /// endpoint requires one (`project`, `config`, and either `slug` or
    /// `token`) — `ureq`'s `DELETE` builder refuses a body unless asked
    /// for it explicitly ([`ureq::RequestBuilder::force_send_body`]),
    /// which this method does on the caller's behalf. Any response body
    /// is ignored on success, exactly like [`Http::delete`].
    ///
    /// # Errors
    ///
    /// See [`Http::get`].
    pub fn delete_with_body(&self, path: &str, body: &impl Serialize) -> Result<(), ProviderError> {
        let url = self.url(path);
        let (status, response_body, facts) = self.run_retrying(true, || {
            let builder = self
                .apply_headers(self.agent.delete(url.as_str()))
                .force_send_body();
            self.apply_credential(builder).send_json(body)
        })?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(provider_error_from_body(status, &response_body).with_facts(facts))
        }
    }

    /// Turn a `(status, body, facts)` triple that already survived
    /// retrying into either the caller's typed success value or a
    /// [`ProviderError`] carrying `facts`.
    fn finish<T: DeserializeOwned>(
        status: u16,
        body: &str,
        facts: ProviderFacts,
    ) -> Result<T, ProviderError> {
        if (200..300).contains(&status) {
            // `serde_json::Error`'s `Display` quotes the offending value
            // (`invalid type: string "...", expected u64`) and, under
            // `deny_unknown_fields`, the offending field name — both of
            // which are response-body text, which trust boundary 5 says a
            // message is never built from. The position is willikins' own
            // observation and carries nothing from the body.
            serde_json::from_str(body).map_err(|err| {
                ProviderError::new(
                    Some(status),
                    format!(
                        "could not parse the response body as the expected shape \
                         (line {}, column {})",
                        err.line(),
                        err.column()
                    ),
                )
                .with_facts(facts)
            })
        } else {
            Err(provider_error_from_body(status, body).with_facts(facts))
        }
    }

    /// Run `attempt` (one fully-built request send) up to
    /// [`MAX_RETRIES`] additional times when `retryable` and the response
    /// (or transport failure) says to. A `401`/`403` is never retried,
    /// regardless of `retryable` — see [`is_retryable_status`] — so a
    /// provider crate whose API layers a soft, retriable rate limit on
    /// top of a `403` (GitHub's secondary rate limit) does its own
    /// retrying on top of this method, using the [`ProviderFacts`] this
    /// returns to decide. Returns the final `(status, body_text,
    /// header_facts)` triple for any response actually received — 2xx or
    /// not — or a [`ProviderError`] only when every attempt failed at the
    /// transport level (which carries no header facts at all).
    fn run_retrying(
        &self,
        retryable: bool,
        mut attempt: impl FnMut() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<(u16, String, ProviderFacts), ProviderError> {
        let max_attempts = if retryable { MAX_RETRIES + 1 } else { 1 };
        for attempt_index in 0..max_attempts {
            let is_last = attempt_index + 1 == max_attempts;
            match attempt() {
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    let facts = response_facts(&response);
                    let body = response.body_mut().read_to_string().unwrap_or_default();
                    if !is_last && is_retryable_status(status) {
                        self.sleeper
                            .sleep(backoff_delay(attempt_index, facts.retry_after));
                        continue;
                    }
                    return Ok((status, body, facts));
                }
                Err(err) => {
                    if !is_last {
                        self.sleeper.sleep(backoff_delay(attempt_index, None));
                        continue;
                    }
                    return Err(ProviderError::new(None, transport_message(&err)));
                }
            }
        }
        unreachable!("the loop above always returns on its last iteration")
    }
}

/// Extract [`ProviderFacts`] from a response's headers: `Retry-After`
/// (parsed the way a retryable status's own backoff already was),
/// `x-ratelimit-remaining`, and `x-ratelimit-reset`, each `None` when
/// absent or not a plain integer. Read for every response, 2xx included
/// (a 2xx never keeps them — [`Http::finish`] discards `facts` on
/// success), so a caller need not special-case which statuses might carry
/// them.
fn response_facts(response: &ureq::http::Response<ureq::Body>) -> ProviderFacts {
    let headers = response.headers();
    let header_u64 = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    ProviderFacts {
        retry_after: headers
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| retry_after::parse(value, SystemTime::now())),
        rate_limit_remaining: header_u64("x-ratelimit-remaining"),
        rate_limit_reset: header_u64("x-ratelimit-reset"),
    }
}

/// Describe a transport failure without naming the request URL (trust
/// boundary 5). Most `ureq::Error` variants print only what went wrong,
/// but three of them print a URL, a proxy URL, or text taken from the
/// response, so those get fixed words instead of their `Display`.
fn transport_message(err: &ureq::Error) -> String {
    let detail = match err {
        ureq::Error::BadUri(_) => "the request URL was not valid".to_string(),
        ureq::Error::RequireHttpsOnly(_) => "the request URL was not https".to_string(),
        ureq::Error::ConnectProxyFailed(_) => "connecting through the proxy failed".to_string(),
        other => bounded_message(&other.to_string()),
    };
    format!("request failed: {detail}")
}

/// The provider's own words in an error body, if it has any that are
/// text: GitHub answers `{"message": "..."}`, Doppler (which documents no
/// error schema at all) answers `{"messages": ["...", "..."]}`. A
/// `message` that is an object or an array is not a message and is
/// dropped rather than `Debug`-formatted into one.
fn provider_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.get("message").and_then(serde_json::Value::as_str) {
        return Some(text.to_string());
    }
    let joined = value
        .get("messages")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>()
        .join("; ");
    (!joined.is_empty()).then_some(joined)
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
        return delay.min(MAX_RETRY_AFTER);
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
    // A `401` or `403` body is dropped here, before anything can hold it:
    // GitHub's says "Bad credentials", Doppler's names the token, and
    // neither tells an operator anything the fixed message does not.
    if matches!(status, 401 | 403) {
        return ProviderError::new(Some(status), MISSING_PERMISSION);
    }

    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();

    let message = parsed.as_ref().and_then(provider_text).map_or_else(
        || bounded_message(&format!("provider returned status {status}")),
        |text| provider_says(&text),
    );

    let already_exists = parsed
        .as_ref()
        .and_then(|value| value.get("errors"))
        .and_then(|value| value.as_array())
        .is_some_and(|errors| errors.iter().any(is_already_exists_error));

    if already_exists {
        ProviderError::already_exists(Some(status), message)
    } else {
        ProviderError::new(Some(status), message)
    }
}

/// Whether one `errors[]` entry (the shared `validation-error` shape) says
/// "this name is already taken": GitHub uses two different shapes for the
/// same fact — a plain `code: "already_exists"`, or `code: "custom"` on
/// `field: "name"` (a real captured example: `{"resource":"Repository",
/// "code":"custom","field":"name","message":"name already exists on this
/// account"}`, research note section 2). Generic on purpose: any provider
/// that reports a naming conflict as a "custom" error on its `name` field
/// gets the same treatment, not only GitHub.
fn is_already_exists_error(error: &serde_json::Value) -> bool {
    let code = error.get("code").and_then(serde_json::Value::as_str);
    match code {
        Some("already_exists") => true,
        Some("custom") => error.get("field").and_then(serde_json::Value::as_str) == Some("name"),
        _ => false,
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
        // The provider's own words are bounded to 256 characters; the
        // `provider says:` label is willikins' own and sits outside that
        // bound.
        assert_eq!(err.message, format!("provider says: {}", "x".repeat(256)));
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
    fn patch_succeeds_and_is_retried_like_put() {
        let mut server = mockito::Server::new();
        let failing = server
            .mock("PATCH", "/thing")
            .with_status(503)
            .with_body("{}")
            .expect(1)
            .create();
        let succeeding = server
            .mock("PATCH", "/thing")
            .with_status(200)
            .with_body(r#"{"name":"widget"}"#)
            .expect(1)
            .create();
        let (http, _sleeper) = client(server.url());
        let thing: Thing = http
            .patch("/thing", &serde_json::json!({"name": "widget"}))
            .expect("succeeds after one retry");
        assert_eq!(thing.name, "widget");
        failing.assert();
        succeeding.assert();
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
    fn a_request_builder_carrying_the_credential_does_not_print_it_in_debug() {
        // `ureq::RequestBuilder`'s `Debug` prints method and URI only, and
        // this pins that: a `{builder:?}` anywhere in a provider crate must
        // not become a way to read the bearer token.
        let credential = Credential::for_testing("WILLIKINS_TEST_HTTP_DEBUG", "sekrit-token-value");
        let builder = credential.authorize(ureq::get("http://127.0.0.1:1/probe"));
        let debug = format!("{builder:?}");
        assert!(!debug.contains("sekrit-token-value"), "{debug}");
    }

    #[test]
    fn a_transport_message_never_repeats_a_url_a_ureq_error_carries() {
        // The three `ureq::Error` variants whose `Display` prints a URL, a
        // proxy URL, or text taken from the response.
        let url = "https://secret-host.example/secret-path";
        for err in [
            ureq::Error::BadUri(url.to_string()),
            ureq::Error::RequireHttpsOnly(url.to_string()),
            ureq::Error::ConnectProxyFailed(url.to_string()),
        ] {
            let message = transport_message(&err);
            assert!(
                !message.contains("secret-host") && !message.contains("secret-path"),
                "a {err:?} leaked its URL: {message}"
            );
            assert!(message.starts_with("request failed:"), "{message}");
        }
        // An ordinary io failure still explains itself.
        let io = ureq::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "connection refused",
        ));
        assert!(transport_message(&io).contains("connection refused"));
    }

    #[test]
    fn jitter_is_bounded_and_never_negative() {
        for attempt in 0..MAX_RETRIES {
            let base = BASE_BACKOFF.as_secs_f64() * f64::from(1_u32 << attempt);
            for _ in 0..200 {
                let delay = backoff_delay(attempt, None).as_secs_f64();
                assert!(
                    delay >= base * 0.5 && delay < base * 1.5,
                    "attempt {attempt} waited {delay}s, outside [{}, {})",
                    base * 0.5,
                    base * 1.5
                );
            }
        }
    }

    #[test]
    fn a_retry_after_is_honoured_up_to_the_cap_and_no_further() {
        assert_eq!(
            backoff_delay(0, Some(Duration::from_secs(5))),
            Duration::from_secs(5)
        );
        assert_eq!(backoff_delay(0, Some(Duration::ZERO)), Duration::ZERO);
        assert_eq!(
            backoff_delay(0, Some(Duration::from_secs(86_400))),
            MAX_RETRY_AFTER
        );
        assert_eq!(backoff_delay(2, Some(Duration::MAX)), MAX_RETRY_AFTER);
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

    #[test]
    fn a_403_is_never_retried_regardless_of_headers() {
        // Retrying a 403 at all is a provider crate's own decision (task
        // 7's GitHub secondary-rate-limit handling): this crate's own
        // retry loop only ever acts on 429/5xx, so even a 403 carrying a
        // `Retry-After` gets exactly one attempt here.
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/thing")
            .with_status(403)
            .with_header("Retry-After", "1")
            .with_body("{}")
            .expect(1)
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http.get::<Thing>("/thing").expect_err("403 is a failure");
        assert_eq!(err.status, Some(403));
        assert_eq!(err.message, MISSING_PERMISSION);
        mock.assert();
    }

    #[test]
    fn a_bare_403_carries_no_rate_limit_facts() {
        let mut server = mockito::Server::new();
        server.mock("GET", "/thing").with_status(403).create();
        let (http, _sleeper) = client(server.url());
        let err = http.get::<Thing>("/thing").expect_err("403 is a failure");
        assert_eq!(err.retry_after, None);
        assert_eq!(err.rate_limit_remaining, None);
        assert_eq!(err.rate_limit_reset, None);
        assert!(!err.looks_rate_limited());
    }

    #[test]
    fn a_403_with_retry_after_carries_it_and_looks_rate_limited() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/thing")
            .with_status(403)
            .with_header("Retry-After", "30")
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http.get::<Thing>("/thing").expect_err("403 is a failure");
        assert_eq!(err.retry_after, Some(Duration::from_secs(30)));
        assert!(err.looks_rate_limited());
    }

    #[test]
    fn a_403_with_rate_limit_remaining_zero_carries_the_reset_and_looks_rate_limited() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/thing")
            .with_status(403)
            .with_header("x-ratelimit-remaining", "0")
            .with_header("x-ratelimit-reset", "1700000000")
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http.get::<Thing>("/thing").expect_err("403 is a failure");
        assert_eq!(err.retry_after, None);
        assert_eq!(err.rate_limit_remaining, Some(0));
        assert_eq!(err.rate_limit_reset, Some(1_700_000_000));
        assert!(err.looks_rate_limited());
    }

    #[test]
    fn a_403_with_rate_limit_remaining_nonzero_does_not_look_rate_limited() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/thing")
            .with_status(403)
            .with_header("x-ratelimit-remaining", "12")
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http.get::<Thing>("/thing").expect_err("403 is a failure");
        assert_eq!(err.rate_limit_remaining, Some(12));
        assert!(!err.looks_rate_limited());
    }

    #[test]
    fn a_2xx_response_never_carries_facts_through_to_the_caller() {
        // `finish` discards `facts` on the success path; this pins that
        // there is no way to observe them from a successful call at all
        // (they simply have nowhere to go — `T` is the caller's own type).
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/thing")
            .with_status(200)
            .with_header("x-ratelimit-remaining", "0")
            .with_body(r#"{"name":"widget"}"#)
            .create();
        let (http, _sleeper) = client(server.url());
        let thing: Thing = http.get("/thing").expect("succeeds");
        assert_eq!(thing.name, "widget");
    }

    #[test]
    fn a_transport_failure_carries_no_facts() {
        let (http, _sleeper) = client("http://127.0.0.1:1".to_string());
        let err = http.get::<Thing>("/thing").expect_err("connection refused");
        assert_eq!(err.retry_after, None);
        assert_eq!(err.rate_limit_remaining, None);
        assert_eq!(err.rate_limit_reset, None);
    }

    #[test]
    fn put_empty_succeeds_on_201_and_on_204_without_parsing_a_body() {
        let mut server = mockito::Server::new();
        let created = server.mock("PUT", "/secrets/one").with_status(201).create();
        let updated = server.mock("PUT", "/secrets/two").with_status(204).create();
        let (http, _sleeper) = client(server.url());
        http.put_empty("/secrets/one", &serde_json::json!({"k": "v"}))
            .expect("201 succeeds with no body to parse");
        http.put_empty("/secrets/two", &serde_json::json!({"k": "v"}))
            .expect("204 succeeds with no body to parse");
        created.assert();
        updated.assert();
    }

    #[test]
    fn put_empty_is_retried_on_503_then_succeeds() {
        let mut server = mockito::Server::new();
        let failing = server
            .mock("PUT", "/secrets/one")
            .with_status(503)
            .with_body("{}")
            .expect(1)
            .create();
        let succeeding = server
            .mock("PUT", "/secrets/one")
            .with_status(204)
            .expect(1)
            .create();
        let (http, _sleeper) = client(server.url());
        http.put_empty("/secrets/one", &serde_json::json!({}))
            .expect("succeeds after one retry");
        failing.assert();
        succeeding.assert();
    }

    #[test]
    fn put_empty_fails_with_the_provider_error_on_a_client_error() {
        let mut server = mockito::Server::new();
        server
            .mock("PUT", "/secrets/one")
            .with_status(422)
            .with_body(r#"{"message":"bad shape"}"#)
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http
            .put_empty("/secrets/one", &serde_json::json!({}))
            .expect_err("422 is a failure");
        assert_eq!(err.status, Some(422));
    }

    #[test]
    fn delete_with_body_sends_the_body_and_succeeds_on_204() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("DELETE", "/tokens/token")
            .match_body(mockito::Matcher::Json(
                serde_json::json!({"project": "p", "config": "c", "slug": "s"}),
            ))
            .with_status(204)
            .expect(1)
            .create();
        let (http, _sleeper) = client(server.url());
        http.delete_with_body(
            "/tokens/token",
            &serde_json::json!({"project": "p", "config": "c", "slug": "s"}),
        )
        .expect("204 succeeds");
        mock.assert();
    }

    #[test]
    fn delete_with_body_is_retried_on_503_then_succeeds() {
        let mut server = mockito::Server::new();
        let failing = server
            .mock("DELETE", "/tokens/token")
            .with_status(503)
            .with_body("{}")
            .expect(1)
            .create();
        let succeeding = server
            .mock("DELETE", "/tokens/token")
            .with_status(204)
            .expect(1)
            .create();
        let (http, _sleeper) = client(server.url());
        http.delete_with_body("/tokens/token", &serde_json::json!({}))
            .expect("succeeds after one retry");
        failing.assert();
        succeeding.assert();
    }

    #[test]
    fn delete_with_body_fails_with_the_provider_error_on_a_client_error() {
        let mut server = mockito::Server::new();
        server
            .mock("DELETE", "/tokens/token")
            .with_status(404)
            .with_body(r#"{"messages":["no such token"]}"#)
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http
            .delete_with_body("/tokens/token", &serde_json::json!({}))
            .expect_err("404 is a failure");
        assert_eq!(err.status, Some(404));
    }

    #[test]
    fn a_custom_error_on_field_name_is_treated_as_already_exists() {
        // GitHub's second shape for "this name is taken", a real captured
        // 422 body (research note section 2): `code: "custom"` on
        // `field: "name"`, distinct from the plain `already_exists` code.
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/orgs/acme/repos")
            .with_status(422)
            .with_body(
                r#"{"message":"Repository creation failed.","errors":[{"resource":"Repository","code":"custom","field":"name","message":"name already exists on this account"}]}"#,
            )
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http
            .post::<Thing>("/orgs/acme/repos", &serde_json::json!({"name": "x"}))
            .expect_err("422");
        assert!(err.already_exists);
    }

    #[test]
    fn a_custom_error_on_a_different_field_is_not_already_exists() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/orgs/acme/repos")
            .with_status(422)
            .with_body(
                r#"{"message":"bad","errors":[{"code":"custom","field":"visibility","message":"nope"}]}"#,
            )
            .create();
        let (http, _sleeper) = client(server.url());
        let err = http
            .post::<Thing>("/orgs/acme/repos", &serde_json::json!({"name": "x"}))
            .expect_err("422");
        assert!(!err.already_exists);
    }
}
