//! [`Credential`]: the butler's own execution-context secret, as opposed to
//! a graph secret (`DopplerServiceToken`, `DopplerSecretValue`) that flows
//! through a workflow's ports.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s "Trust
//! boundaries" section, item 1 ("Two kinds of secret"): a `Credential`
//! never becomes a domain type, never enters the type registry, is never a
//! [`willikins_core::Value`], and its bytes are read in exactly one
//! function in this crate, `Credential::authorize`, which sets the
//! outgoing `Authorization` header. `clippy.toml` disallows
//! `secrecy::ExposeSecret::expose_secret` everywhere else in the
//! workspace; the acceptance test walking every call site lives in
//! `crates/willikins-core/tests/expose_secret_guard.rs`.

use regex::Regex;
use secrecy::{ExposeSecret, SecretString};

/// One execution-context credential: an environment variable's value,
/// validated against a provider's documented token shape and never
/// printed, serialized, or cloned out of this crate as plain bytes.
pub struct Credential {
    /// The environment variable this credential was read from, kept only
    /// for its redacted `Debug` marker and error messages — never the
    /// value.
    var: &'static str,
    secret: SecretString,
}

impl Credential {
    /// Read `var` from the process environment and validate it against
    /// `format`.
    ///
    /// # Errors
    ///
    /// Returns [`CredentialError::Missing`] when `var` is unset (or set to
    /// the empty string, which every provider token shape this crate
    /// validates against already rejects, but `Missing` is the more
    /// honest report for an operator who exported `FOO=` by mistake).
    /// Returns [`CredentialError::Malformed`] when `var` is set but its
    /// value does not match `format`. Neither variant carries the value.
    pub fn from_env(var: &'static str, format: &Regex) -> Result<Credential, CredentialError> {
        Self::from_value(var, std::env::var(var).ok(), format)
    }

    /// [`Credential::from_env`]'s decision logic, taking the (possibly
    /// absent) value directly rather than reading the process
    /// environment.
    ///
    /// Split out so tests can exercise every branch — missing, empty,
    /// malformed, valid — without mutating real process-wide environment
    /// state: `std::env::set_var`/`remove_var` are `unsafe` (edition 2024,
    /// since they race any other thread reading the environment), and
    /// this workspace forbids `unsafe_code` outright, including in tests.
    fn from_value(
        var: &'static str,
        value: Option<String>,
        format: &Regex,
    ) -> Result<Credential, CredentialError> {
        let value = match value {
            Some(value) if !value.is_empty() => value,
            _ => return Err(CredentialError::Missing { var }),
        };
        if !format.is_match(&value) {
            return Err(CredentialError::Malformed { var });
        }
        Ok(Credential {
            var,
            secret: SecretString::from(value),
        })
    }

    /// Set the `Authorization: Bearer <token>` header on `request`.
    ///
    /// One of two non-test, non-derive call sites of
    /// `secrecy::ExposeSecret::expose_secret` in the workspace (the other
    /// is [`Self::authorize_header`]).
    ///
    /// Crate-private on purpose: the returned builder carries the bearer
    /// token in a header any caller could read back with
    /// `headers_ref()`, which would be a way out of a `Credential` that
    /// neither `clippy.toml`'s `disallowed-methods` entry nor
    /// `expose_secret_guard.rs` can see. Everything outside this crate
    /// goes through [`crate::Http`], which sends the header and never
    /// hands it back.
    #[must_use]
    // One of the two allowed production call sites outside the derive's
    // own codegen, named in `clippy.toml`'s `disallowed-methods` reason
    // and walked by `crates/willikins-core/tests/expose_secret_guard.rs`.
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn authorize<B>(&self, request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        let token = self.secret.expose_secret();
        request.header("Authorization", format!("Bearer {token}"))
    }

    /// Set `header_name: <token>` on `request` verbatim — no `Bearer `
    /// prefix, no `Authorization` name. The generalised sibling of
    /// [`Self::authorize`], for a provider whose documented scheme puts
    /// the credential in a header of its own (`SigNoz`: `SigNoz-Api-Key`,
    /// per `docs/research/2026-09-20-signoz-ingestion-keys.md` section 1
    /// — never `Authorization`).
    ///
    /// Exists so a provider crate never builds the header by hand at its
    /// own call site, which would be a second, guard-invisible place this
    /// crate's bytes leave [`SecretString`]: this method, like
    /// [`Self::authorize`], is the *only* sanctioned place, and both are
    /// walked the same way by `expose_secret_guard.rs`.
    ///
    /// Crate-private for the same reason [`Self::authorize`] is: the
    /// returned builder could otherwise be asked for its own header back.
    #[must_use]
    // The second allowed production call site outside the derive's own
    // codegen — see `authorize`'s doc comment, and
    // `crates/willikins-core/tests/expose_secret_guard.rs`'s
    // `willikins-providers-http` exemption, which names both functions.
    #[allow(clippy::disallowed_methods)]
    pub(crate) fn authorize_header<B>(
        &self,
        request: ureq::RequestBuilder<B>,
        header_name: &'static str,
    ) -> ureq::RequestBuilder<B> {
        let token = self.secret.expose_secret();
        request.header(header_name, token)
    }
}

/// Test-only construction, skipping both the environment read and the
/// format check: every live provider crate's own tests (and this crate's,
/// see `tests/credential_leak.rs` and `src/http.rs`'s tests) need a
/// `Credential` carrying a known value without racing other tests over
/// process environment state. Behind the same `test-support` feature the
/// mock-server [`crate::testing`] module is behind.
#[cfg(any(test, feature = "test-support"))]
impl Credential {
    /// Build a `Credential` directly from `value`, as if it had been read
    /// from `var` and had already passed its provider's format check.
    #[must_use]
    pub fn for_testing(var: &'static str, value: &str) -> Credential {
        Credential {
            var,
            secret: SecretString::from(value),
        }
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED Credential({})]", self.var)
    }
}

impl Clone for Credential {
    fn clone(&self) -> Self {
        Self {
            var: self.var,
            secret: self.secret.clone(),
        }
    }
}

/// Why [`Credential::from_env`] refused to build a credential. Never
/// carries the environment variable's value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    /// The environment variable was unset or empty.
    #[error("environment variable `{var}` is not set")]
    Missing {
        /// The variable that was missing.
        var: &'static str,
    },
    /// The environment variable was set but did not match the provider's
    /// documented token shape.
    #[error("environment variable `{var}` does not look like a valid credential")]
    Malformed {
        /// The variable that held the malformed value.
        var: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // A distinctive marker used across this module's tests: if it ever
    // shows up in a `Debug`, `Display`, or `Serialize` output, a
    // redaction rule broke.
    const MARKER: &str = "wlkn-test-marker-2rz8shp5tap";

    fn digits() -> Regex {
        Regex::new("^[0-9]+$").expect("valid pattern")
    }

    #[test]
    fn missing_when_absent() {
        let var = "WILLIKINS_TEST_CREDENTIAL_MISSING_1";
        let err = Credential::from_value(var, None, &digits()).unwrap_err();
        assert_eq!(err, CredentialError::Missing { var });
    }

    #[test]
    fn missing_when_empty() {
        let var = "WILLIKINS_TEST_CREDENTIAL_MISSING_2";
        let err = Credential::from_value(var, Some(String::new()), &digits()).unwrap_err();
        assert_eq!(err, CredentialError::Missing { var });
    }

    #[test]
    fn malformed_when_it_does_not_match() {
        let var = "WILLIKINS_TEST_CREDENTIAL_MALFORMED_1";
        let err = Credential::from_value(var, Some(MARKER.to_string()), &digits()).unwrap_err();
        assert_eq!(err, CredentialError::Malformed { var });
        let rendered = format!("{err}");
        assert!(
            !rendered.contains(MARKER),
            "CredentialError::Malformed's Display leaked the value: {rendered}"
        );
    }

    #[test]
    fn parses_a_matching_value() {
        let var = "WILLIKINS_TEST_CREDENTIAL_OK_1";
        let credential = Credential::from_value(var, Some("123456".to_string()), &digits())
            .expect("matches the pattern");
        let debug = format!("{credential:?}");
        assert_eq!(debug, format!("[REDACTED Credential({var})]"));
    }

    #[test]
    fn from_env_reads_through_to_the_real_process_environment() {
        // `PATH` is set in every process this test can plausibly run in,
        // and reading (not setting) an environment variable is not
        // `unsafe`, so this needs no special handling: it proves
        // `from_env` really does read the process environment (`.`
        // matches any single character, so any non-empty `PATH` passes).
        let credential =
            Credential::from_env("PATH", &Regex::new(".").expect("valid")).expect("PATH is set");
        assert_eq!(format!("{credential:?}"), "[REDACTED Credential(PATH)]");
    }

    /// The two regexes the plan's provider sections name, so these tests
    /// exercise the real token shapes rather than a stand-in.
    fn github() -> Regex {
        Regex::new("^(github_pat_|ghp_)[A-Za-z0-9_]+$").expect("valid pattern")
    }

    fn doppler() -> Regex {
        Regex::new(r"^dp\.(sa|pt)\.[a-zA-Z0-9]{40,44}$").expect("valid pattern")
    }

    #[test]
    fn a_value_with_surrounding_whitespace_or_a_newline_is_malformed() {
        let var = "WILLIKINS_TEST_CREDENTIAL_WHITESPACE";
        let good = format!("ghp_{}", "a".repeat(36));
        for value in [
            format!("{good}\n"),
            format!(" {good}"),
            format!("{good} "),
            format!("{good}\nghp_{}", "b".repeat(36)),
            format!("\t{good}"),
        ] {
            let err = Credential::from_value(var, Some(value.clone()), &github())
                .expect_err("whitespace is not part of the token shape");
            assert_eq!(err, CredentialError::Malformed { var }, "value {value:?}");
            let rendered = format!("{err}");
            assert!(!rendered.contains(&good), "value leaked: {rendered}");
        }
        Credential::from_value(var, Some(good), &github()).expect("the bare token is accepted");
    }

    #[test]
    fn a_doppler_service_token_is_refused_by_the_provisioning_regex() {
        let var = "WILLIKINS_TEST_CREDENTIAL_DOPPLER";
        let service = format!("dp.st.{}", "a".repeat(40));
        let err = Credential::from_value(var, Some(service.clone()), &doppler())
            .expect_err("a dp.st. token cannot provision");
        assert_eq!(err, CredentialError::Malformed { var });
        assert!(!format!("{err}").contains(&service));
        Credential::from_value(var, Some(format!("dp.sa.{}", "a".repeat(40))), &doppler())
            .expect("a service-account token is accepted");
    }

    #[test]
    fn a_format_that_matches_everything_still_never_prints_the_value() {
        // `(?s).*` lets even a newline through, which is the worst case a
        // provider crate could hand `from_env`: redaction may not depend
        // on the format regex having been strict.
        let var = "WILLIKINS_TEST_CREDENTIAL_PERMISSIVE";
        let permissive = Regex::new("(?s).*").expect("valid pattern");
        let credential =
            Credential::from_value(var, Some(format!("{MARKER}\n{MARKER}")), &permissive)
                .expect("matches everything");
        let debug = format!("{credential:?}");
        assert!(!debug.contains(MARKER), "Debug leaked: {debug}");
        assert_eq!(debug, format!("[REDACTED Credential({var})]"));
    }

    #[test]
    fn debug_never_carries_the_marker() {
        let credential = Credential::for_testing("WILLIKINS_TEST_CREDENTIAL_DEBUG_1", MARKER);
        let debug = format!("{credential:?}");
        assert!(!debug.contains(MARKER), "Debug leaked: {debug}");
    }

    #[test]
    fn authorize_sets_the_bearer_header_and_nothing_else_leaks() {
        let credential = Credential::for_testing("WILLIKINS_TEST_CREDENTIAL_AUTHORIZE", MARKER);
        let request = ureq::get("http://127.0.0.1:0/probe");
        let request = credential.authorize(request);
        let headers = request.headers_ref().expect("builder has no error yet");
        let value = headers
            .get("Authorization")
            .expect("Authorization header set")
            .to_str()
            .expect("ascii header value");
        assert_eq!(value, format!("Bearer {MARKER}"));
    }
}
