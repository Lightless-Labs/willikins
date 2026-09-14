//! Mock-server test support shared by every live provider crate.
//!
//! Behind the `test-support` feature (always on for this crate's own
//! tests, through its self dev-dependency — the same pattern
//! `willikins-core`'s `test-support` feature uses). `willikins-providers-github`
//! and `willikins-providers-doppler` (tasks 7 and 8) depend on this crate
//! with `features = ["test-support"]` in their own `[dev-dependencies]`.

use std::path::Path;

/// One mock HTTP server for a test, wrapping a [`mockito::ServerGuard`].
///
/// Build an [`crate::Http`] against [`MockProvider::url`] and configure
/// expectations with [`MockProvider::mock`], exactly as you would against
/// a bare [`mockito::Server`] — this type exists only to keep the
/// `mockito` import in one place for callers that otherwise depend on
/// this crate alone.
pub struct MockProvider {
    server: mockito::ServerGuard,
}

impl MockProvider {
    /// Start a fresh mock server. Panics the way [`mockito::Server::new`]
    /// does on failure to bind: a test fixture, not caller input.
    #[must_use]
    pub fn start() -> Self {
        Self {
            server: mockito::Server::new(),
        }
    }

    /// The mock server's base URL (no trailing slash), suitable as
    /// [`crate::Http::new`]'s `base_url`.
    #[must_use]
    pub fn url(&self) -> String {
        self.server.url()
    }

    /// Begin configuring a mock for `method`/`path` (for example `"GET"`,
    /// `"/repos/acme/widget"`). Call `.create()` on the result to mount it
    /// (see `mockito::Mock`).
    pub fn mock(&mut self, method: &str, path: &str) -> mockito::Mock {
        self.server.mock(method, path)
    }
}

/// Load a recorded response fixture: `<fixtures_dir>/<provider>/<name>.json`,
/// parsed as JSON. `fixtures_dir` is caller-given (typically
/// `Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")` in the calling
/// crate), so each provider crate keeps its own recordings under its own
/// `fixtures/` directory rather than this one.
///
/// # Panics
///
/// Panics, naming the path, when the file is missing or is not valid
/// JSON: a fixture is test-authored, so a bad one is a bug in the test
/// suite, not a condition to recover from at runtime.
#[must_use]
pub fn load_fixture(fixtures_dir: &Path, provider: &str, name: &str) -> serde_json::Value {
    let path = fixtures_dir.join(provider).join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("reading fixture {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("parsing fixture {} as JSON: {err}", path.display()))
}

/// A [`mockito::Matcher`] asserting a request body equals `value` exactly.
#[must_use]
pub fn json_body(value: serde_json::Value) -> mockito::Matcher {
    mockito::Matcher::Json(value)
}

/// A [`mockito::Matcher`] asserting a request body contains at least
/// `value`'s fields (extra fields in the real body are allowed).
#[must_use]
pub fn partial_json_body(value: serde_json::Value) -> mockito::Matcher {
    mockito::Matcher::PartialJson(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    #[test]
    fn load_fixture_reads_and_parses_the_recorded_json() {
        let value = load_fixture(&fixtures_dir(), "selftest", "example");
        assert_eq!(value["name"], "self-test-fixture");
    }

    /// One self-test proving the whole seam works end to end: a mock
    /// server answers a real request (sent through [`crate::Http`], with
    /// a real [`crate::Credential`]) with a loaded fixture's body, and a
    /// JSON body matcher accepts a request that matches while an
    /// unrelated one goes unmatched.
    #[test]
    fn mock_provider_serves_a_loaded_fixture_and_matches_the_request_body() {
        let mut provider = MockProvider::start();
        let fixture = load_fixture(&fixtures_dir(), "selftest", "example");
        let mock = provider
            .mock("PUT", "/echo")
            .match_body(partial_json_body(serde_json::json!({"ping": "pong"})))
            .with_status(200)
            .with_body(fixture.to_string())
            .create();

        let credential = crate::Credential::for_testing("WILLIKINS_TEST_SELFTEST", "test-token");
        let http = crate::Http::new(provider.url(), Vec::new(), credential);
        let response: serde_json::Value = http
            .put("/echo", &serde_json::json!({"ping": "pong", "extra": true}))
            .expect("mock server answers 200");

        assert_eq!(response, fixture);
        mock.assert();
    }
}
