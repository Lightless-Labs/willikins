//! [`GitHubClient`]: typed calls for exactly the endpoints
//! `github.repo.ensure` and `github.actions_secret.ensure` need. No tool
//! ever builds a URL or path itself; every path this client builds is
//! assembled from already-validated domain types
//! ([`willikins_types::GitHubOrg`], [`willikins_types::ProjectSlug`] via
//! [`willikins_types::GitHubRepo`], [`willikins_types::ActionsSecretName`]),
//! whose grammars are alphanumeric-and-hyphen-or-underscore, so none of
//! them can smuggle a `/` or a query string into the request line.
//!
//! # GitHub's secondary rate limit
//!
//! `willikins-providers-http`'s shared [`Http`] client never retries a
//! `401` or `403` (see its module docs): a `403` might mean "the
//! credential lacks a permission" or, on GitHub specifically, "you have
//! been secondary-rate-limited", and only the second is worth waiting
//! out. This client tells the two apart itself, using the header facts
//! [`ProviderError`] now carries
//! ([`ProviderError::looks_rate_limited`]): every retryable call here
//! (every one but repository creation, which — like every `POST` in this
//! workspace — is never retried at all, ambiguous-failure risk aside)
//! retries such a `403` under [`willikins_providers_http::MAX_RETRY_AFTER`]'s
//! own 60-second cap, then gives up and reports it as
//! [`willikins_core::ToolErrorKind::Provider`] naming the rate limit and,
//! when GitHub sent one, the reset time — never as a missing permission.
//!
//! # GitHub credentials as ports (milestone 3e, R2)
//!
//! Every tool in this crate is still built once, at catalog-construction
//! time, from a [`GitHubClient`] that itself owns a [`Credential`] read
//! from `WILLIKINS_GITHUB_TOKEN` ([`credential_from_env`]) — nothing
//! about that changes, so every document and server invocation that
//! predates this section keeps working exactly as it did.
//!
//! What is new: each of the three tools' `ToolSpec` now also declares an
//! **optional** `token` port, [`willikins_types::GitHubToken`], the same
//! "credentials are ports, resolvers are nodes" shape
//! (`docs/plans/2026-09-11-willikins-design.md`, 2026-09-21 addendum)
//! `willikins-providers-appstore` already uses for the three parts of its
//! credential. A document that binds it (typically `doppler.secret.get`
//! into `github.token.parse`) gets a *fresh* [`GitHubClient`] built from
//! that resolved token instead — [`client_for_token`] mints it. Unlike
//! `willikins-providers-appstore`'s own `client_for`, which mints a JWT
//! and so always talks to the real API, a bound `GitHubToken` is used
//! verbatim as a bearer credential and must still reach whatever
//! `base_url` the tool's own default client already carries (a mock
//! server in a test, the real API in production) — so `client_for_token`
//! takes the default client and calls [`GitHubClient::with_credential`]
//! (in turn [`Http::with_credential`]) to swap only the credential,
//! never [`GITHUB_API_BASE_URL`] unconditionally. A document that does
//! not bind `token` is unaffected: [`ScopedClient::default_for`] simply
//! borrows the tool's own held client, exactly as before this addition.
//! Optional, not required, on purpose: making it required would demand
//! every existing document bind it, which is precisely what this task's
//! own boundary rules out.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use willikins_core::{ToolError, ToolErrorKind};
use willikins_providers_http::{
    Credential, Http, MAX_RETRY_AFTER, ProviderError, RealSleeper, Sleeper,
};
use willikins_types::{ActionsSecretName, GitHubRepo, GitHubToken, RepoVisibility};

/// GitHub's REST API base URL.
pub const GITHUB_API_BASE_URL: &str = "https://api.github.com";

/// The environment variable a GitHub [`Credential`] is read from.
pub const CREDENTIAL_VAR: &str = "WILLIKINS_GITHUB_TOKEN";

/// The shape of a GitHub personal access token this crate accepts: a
/// fine-grained token (`github_pat_`) or a classic one (`ghp_`). GitHub
/// publishes the prefixes but not the body's length or charset (research
/// note section 2, "Facts that still need a browser"), so the pattern
/// only pins what GitHub does document.
pub const CREDENTIAL_PATTERN: &str = "^(github_pat_|ghp_)[A-Za-z0-9_]+$";

/// Read [`CREDENTIAL_VAR`] from the process environment and validate it
/// against [`CREDENTIAL_PATTERN`].
///
/// # Errors
///
/// See [`Credential::from_env`].
///
/// # Panics
///
/// Never in practice: [`CREDENTIAL_PATTERN`] is a fixed, compile-time-known
/// literal already exercised by this crate's own tests, so
/// `Regex::new` on it cannot fail. Kept as an `expect` (not a
/// `LazyLock`-hidden panic) because the caller of a rarely-invoked,
/// process-lifetime function does not need a static initializer for one
/// regex build.
pub fn credential_from_env() -> Result<Credential, willikins_providers_http::CredentialError> {
    let pattern =
        regex::Regex::new(CREDENTIAL_PATTERN).expect("CREDENTIAL_PATTERN is a valid regex");
    Credential::from_env(CREDENTIAL_VAR, &pattern)
}

/// The three headers GitHub's docs require on every request: `Accept`
/// (`application/vnd.github+json`), `X-GitHub-Api-Version`
/// (`2022-11-28`), and a `User-Agent` naming this crate and its version
/// (GitHub rejects requests with no `User-Agent` at all).
#[must_use]
pub fn default_headers() -> Vec<(String, String)> {
    vec![
        (
            "Accept".to_string(),
            "application/vnd.github+json".to_string(),
        ),
        ("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string()),
        (
            "User-Agent".to_string(),
            format!("willikins/{}", env!("CARGO_PKG_VERSION")),
        ),
    ]
}

/// Build an [`Http`] against GitHub's real API, carrying [`default_headers`]
/// and `credential`. The convenience constructor every caller outside this
/// crate's own tests should use, so the three required headers cannot be
/// forgotten at a call site.
#[must_use]
pub fn http_client(credential: Credential) -> Http {
    Http::new(GITHUB_API_BASE_URL, default_headers(), credential)
}

/// Turn a [`ProviderError`] into a [`ToolError`], with one addition to
/// the shared `From<ProviderError> for ToolError` mapping: a `403` that
/// [`ProviderError::looks_rate_limited`] (this client's own
/// [`GitHubClient::retry_secondary_limit`] already retried it under
/// [`MAX_RETRY_AFTER`]'s cap and it still failed) is reported naming the
/// rate limit and, when GitHub sent one, the reset time, never as the
/// generic missing-permission message a bare `403` gets.
pub(crate) fn to_tool_error(err: ProviderError) -> ToolError {
    if err.looks_rate_limited() {
        let message = err.rate_limit_reset.map_or_else(
            || "GitHub's secondary rate limit is in effect; try again shortly".to_string(),
            |reset| {
                format!(
                    "GitHub's secondary rate limit is in effect; it resets at unix time {reset}"
                )
            },
        );
        return ToolError {
            kind: ToolErrorKind::Provider,
            message,
        };
    }
    err.into()
}

/// How many extra attempts a call here makes after a `403` that
/// [`ProviderError::looks_rate_limited`], beyond the one `willikins-providers-http`
/// itself already made. Mirrors the shared client's own `MAX_RETRIES`
/// (three, four attempts total): a rate limit that survives four spaced
/// attempts under a 60-second-per-wait cap is not going to clear itself
/// inside one `apply`.
const MAX_SECONDARY_LIMIT_RETRIES: u32 = 3;

/// The wait between secondary-rate-limit retries when GitHub's response
/// named neither a `Retry-After` nor an `x-ratelimit-reset`: a `403` this
/// client decided was rate-limited without either header is not one this
/// workspace has seen from GitHub, but a wait is still safer than a tight
/// retry loop.
const DEFAULT_SECONDARY_LIMIT_BACKOFF: Duration = Duration::from_secs(5);

/// A typed GitHub REST client, bound to one [`Http`] (which itself owns
/// the [`willikins_providers_http::Credential`]).
pub struct GitHubClient {
    http: Http,
    sleeper: Arc<dyn Sleeper>,
}

impl GitHubClient {
    /// Build a client over `http`, which must already carry the
    /// `Accept`, `X-GitHub-Api-Version`, and `User-Agent` headers this
    /// crate's tools require (set once, by whoever constructs `http`, not
    /// per call).
    #[must_use]
    pub fn new(http: Http) -> Self {
        Self {
            http,
            sleeper: Arc::new(RealSleeper),
        }
    }

    /// Replace the sleeper this client's own secondary-rate-limit retry
    /// loop waits with — the seam a test injects a non-waiting one
    /// through, the same pattern [`Http::with_sleeper`] uses for its own
    /// 429/5xx retries.
    #[must_use]
    pub fn with_sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        self.sleeper = sleeper;
        self
    }

    /// Build a client identical to this one — same route, headers, and
    /// sleeper — except for its credential, which [`client_for_token`]
    /// uses so a document-bound `token` port still reaches whatever
    /// `base_url` this client was built against (a mock server in a test,
    /// the real API in production), never [`GITHUB_API_BASE_URL`]
    /// unconditionally. See [`Http::with_credential`].
    #[must_use]
    fn with_credential(&self, credential: Credential) -> Self {
        Self {
            http: self.http.with_credential(credential),
            sleeper: Arc::clone(&self.sleeper),
        }
    }

    /// Run `attempt` (one already-retried-by-`Http` call), retrying again
    /// when it fails with a `403` that [`ProviderError::looks_rate_limited`],
    /// up to [`MAX_SECONDARY_LIMIT_RETRIES`] additional times. Any other
    /// error, or a rate limit that outlasts every retry, is returned as
    /// `Http` produced it, header facts and all — this method never
    /// itself decides what message the caller shows, so both the retried
    /// and the exhausted cases stay in `ProviderError`'s existing shape.
    fn retry_secondary_limit<T>(
        &self,
        mut attempt: impl FnMut() -> Result<T, ProviderError>,
    ) -> Result<T, ProviderError> {
        let mut tries = 0;
        loop {
            match attempt() {
                Err(err) if tries < MAX_SECONDARY_LIMIT_RETRIES && err.looks_rate_limited() => {
                    let delay = err
                        .retry_after
                        .or_else(|| err.rate_limit_reset.map(seconds_until))
                        .unwrap_or(DEFAULT_SECONDARY_LIMIT_BACKOFF)
                        .min(MAX_RETRY_AFTER);
                    self.sleeper.sleep(delay);
                    tries += 1;
                }
                other => return other,
            }
        }
    }

    /// `GET /repos/{owner}/{name}`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] for any non-2xx response (a `404`
    /// meaning "absent or invisible to us" and a `301` meaning "renamed
    /// away" are ordinary members of this, not special-cased here — see
    /// the tools that call this) or a transport failure.
    pub(crate) fn get_repo(&self, repo: &GitHubRepo) -> Result<RepoBody, ProviderError> {
        let path = repo_path(repo);
        self.retry_secondary_limit(|| self.http.get(&path))
    }

    /// `POST /orgs/{org}/repos`. Never retried, by this client or by
    /// `Http` underneath it: an ambiguous failure here (including a
    /// secondary-rate-limit `403`, which — unlike a `GET` — cannot be
    /// known to have taken no effect) is resolved by the caller
    /// re-`read`ing, exactly as a `POST`'s own idempotency hazard already
    /// requires (research note, section 2, "Idempotency hazards").
    ///
    /// # Errors
    ///
    /// See [`Self::get_repo`], plus a `422` whose body named an
    /// already-exists shape sets [`ProviderError::already_exists`].
    pub(crate) fn create_repo(
        &self,
        repo: &GitHubRepo,
        visibility: RepoVisibility,
    ) -> Result<(), ProviderError> {
        let path = format!("/orgs/{}/repos", repo.owner());
        let body = CreateRepoBody {
            name: repo.name().to_string(),
            visibility,
        };
        self.http.post::<serde_json::Value>(&path, &body)?;
        Ok(())
    }

    /// `PUT /repos/{owner}/{name}/topics`, replacing the whole topic set
    /// with exactly [`crate::MANAGED_TOPIC`].
    ///
    /// # Errors
    ///
    /// See [`Self::get_repo`].
    pub(crate) fn put_managed_topic(&self, repo: &GitHubRepo) -> Result<(), ProviderError> {
        let path = format!("{}/topics", repo_path(repo));
        let body = TopicsBody {
            names: vec![crate::MANAGED_TOPIC.to_string()],
        };
        self.retry_secondary_limit(|| self.http.put::<serde_json::Value>(&path, &body))?;
        Ok(())
    }

    /// `GET /repos/{owner}/{name}/actions/secrets/{name}`. The response
    /// never carries a value (GitHub's `actions-secret` schema has no
    /// such field at all), so this discards the body entirely and reports
    /// only existence.
    ///
    /// # Errors
    ///
    /// See [`Self::get_repo`].
    pub(crate) fn get_actions_secret(
        &self,
        repo: &GitHubRepo,
        name: &ActionsSecretName,
    ) -> Result<(), ProviderError> {
        let path = format!("{}/actions/secrets/{name}", repo_path(repo));
        self.retry_secondary_limit(|| self.http.get::<serde_json::Value>(&path))?;
        Ok(())
    }

    /// `GET /repos/{owner}/{name}/actions/secrets/public-key`.
    ///
    /// # Errors
    ///
    /// See [`Self::get_repo`].
    pub(crate) fn get_public_key(&self, repo: &GitHubRepo) -> Result<PublicKeyBody, ProviderError> {
        let path = format!("{}/actions/secrets/public-key", repo_path(repo));
        self.retry_secondary_limit(|| self.http.get(&path))
    }

    /// `PUT /repos/{owner}/{name}/actions/secrets/{name}`. `201`
    /// (created) and `204` (updated) both succeed; the response is
    /// never parsed (see [`Http::put_empty`]'s docs — this endpoint is
    /// documented to answer `204` with no body at all).
    ///
    /// # Errors
    ///
    /// See [`Self::get_repo`].
    pub(crate) fn put_actions_secret(
        &self,
        repo: &GitHubRepo,
        name: &ActionsSecretName,
        encrypted_value: &str,
        key_id: &str,
    ) -> Result<(), ProviderError> {
        let path = format!("{}/actions/secrets/{name}", repo_path(repo));
        let body = PutSecretBody {
            encrypted_value: encrypted_value.to_string(),
            key_id: key_id.to_string(),
        };
        self.retry_secondary_limit(|| self.http.put_empty(&path, &body))
    }
}

/// The label a bound `token` port's minted [`Credential`] carries in its
/// own redacted `Debug` — distinct from [`CREDENTIAL_VAR`] on purpose: a
/// 401 against a Doppler-sourced token must never point an operator at
/// `WILLIKINS_GITHUB_TOKEN`, which this credential was never read from.
/// Mirrors `willikins_providers_appstore::client`'s own `CREDENTIAL_LABEL`.
const BOUND_TOKEN_LABEL: &str = "GitHubToken port";

/// Build a fresh [`GitHubClient`] from a document-bound `token` port,
/// minting a [`Credential`] straight from its resolved bytes via
/// [`GitHubToken::reveal_for_authorization`] — the only place this crate
/// reads them — while keeping `default`'s own route (base URL, default
/// headers, sleeper): [`Http::with_credential`] swaps only the
/// credential, so this reaches whatever `default` was built against (a
/// mock server in a test, the real API in production), never
/// [`crate::GITHUB_API_BASE_URL`] unconditionally. See this module's own
/// "GitHub credentials as ports" doc section.
pub(crate) fn client_for_token(default: &GitHubClient, token: &GitHubToken) -> GitHubClient {
    let credential = token.reveal_for_authorization(|bytes| {
        Credential::from_bearer_token(BOUND_TOKEN_LABEL, bytes.to_owned())
    });
    default.with_credential(credential)
}

/// Either the tool's own held [`GitHubClient`] (built once, from
/// `WILLIKINS_GITHUB_TOKEN`, at catalog-construction time — the
/// unbound-port, execution-context case) or a freshly minted one built
/// from a document-bound `token` port. `Deref`s to [`GitHubClient`] so
/// every existing `self.client.method(...)` call site becomes
/// `client.method(...)` regardless of which case applies.
pub(crate) enum ScopedClient<'a> {
    /// No `token` port was bound: use the client this tool already holds.
    Default(&'a GitHubClient),
    /// A `token` port was bound: use the client [`client_for_token`] just
    /// built from it.
    Bound(GitHubClient),
}

impl<'a> ScopedClient<'a> {
    /// Choose between `default` and a client built from `token`, exactly
    /// as this module's own "GitHub credentials as ports" doc section
    /// describes.
    pub(crate) fn default_for(default: &'a GitHubClient, token: Option<&GitHubToken>) -> Self {
        match token {
            Some(token) => Self::Bound(client_for_token(default, token)),
            None => Self::Default(default),
        }
    }
}

impl std::ops::Deref for ScopedClient<'_> {
    type Target = GitHubClient;

    fn deref(&self) -> &GitHubClient {
        match self {
            Self::Default(client) => client,
            Self::Bound(client) => client,
        }
    }
}

fn repo_path(repo: &GitHubRepo) -> String {
    format!("/repos/{}/{}", repo.owner(), repo.name())
}

/// Seconds from now until `reset_epoch` (a Unix epoch second), zero if
/// that instant has already passed. `SystemTime::now()`'s own clock, not
/// injectable: only the *wait* is a test seam ([`GitHubClient::with_sleeper`]),
/// not the arithmetic that decides how long it should be.
fn seconds_until(reset_epoch: u64) -> Duration {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Duration::from_secs(reset_epoch.saturating_sub(now))
}

#[derive(Debug, Serialize)]
struct CreateRepoBody {
    name: String,
    visibility: RepoVisibility,
}

#[derive(Debug, Serialize)]
struct TopicsBody {
    names: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PutSecretBody {
    encrypted_value: String,
    key_id: String,
}

/// The fields of GitHub's `full repository` schema this crate consults:
/// its `visibility` (`private`/`public`) and its `topics`, used to detect
/// the `managed-by-willikins` ownership marker. `topics` is optional in
/// both senses the `OpenAPI` schema allows — the key may be absent, and
/// it may be present and `null`, since the schema marks no field required
/// at all — and both mean the same thing to this crate: the ownership
/// marker is not there, so the repository is `Foreign`. Neither may
/// become a parse failure reported as a `Provider` error.
#[derive(Debug, Deserialize)]
pub(crate) struct RepoBody {
    pub(crate) visibility: RepoVisibility,
    #[serde(default, deserialize_with = "topics_or_empty")]
    pub(crate) topics: Vec<String>,
    /// Whether the repository is archived (`github.repo.get`'s own
    /// `Conflict` arm; `github.repo.ensure` never reads this field).
    /// GitHub's schema marks it required, but this crate's own mock
    /// fixtures predate the field, so it defaults to `false` ("not
    /// archived") rather than failing every existing fixture's parse.
    #[serde(default)]
    pub(crate) archived: bool,
}

/// Deserialize `topics` treating an explicit `null` as an empty list.
/// `#[serde(default)]` alone covers only an absent key; a present `null`
/// would otherwise fail the whole body's deserialization.
fn topics_or_empty<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

/// GitHub's `actions-public-key` schema: both fields required.
#[derive(Debug, Deserialize)]
pub(crate) struct PublicKeyBody {
    pub(crate) key_id: String,
    pub(crate) key: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_types::DomainType;

    #[test]
    fn client_for_token_builds_a_client() {
        let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        let default = GitHubClient::new(Http::new("http://127.0.0.1:1", Vec::new(), credential));
        let token = GitHubToken::parse(GitHubToken::example()).unwrap();
        let _client = client_for_token(&default, &token);
    }

    /// The whole point of the fix: a bound token still reaches the
    /// *same route* the default client was built against (a mock
    /// server here, the real API in production) — never
    /// `GITHUB_API_BASE_URL` unconditionally, which would make this
    /// request go nowhere the mock could ever see it.
    #[test]
    fn client_for_token_preserves_the_default_clients_base_url() {
        let mut provider = willikins_providers_http::testing::MockProvider::start();
        // Pins the credential too, not only the route: only the *bound*
        // token's bearer value is accepted here.
        let mock = provider
            .mock("GET", "/repos/acme/widget")
            .match_header(
                "authorization",
                format!("Bearer {}", GitHubToken::example()).as_str(),
            )
            .with_status(200)
            .with_body(r#"{"visibility":"private","topics":[]}"#)
            .create();
        let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        let default = GitHubClient::new(Http::new(provider.url(), Vec::new(), credential));
        let token = GitHubToken::parse(GitHubToken::example()).unwrap();
        let bound = client_for_token(&default, &token);
        bound
            .get_repo(&willikins_types::GitHubRepo::parse("acme/widget").unwrap())
            .expect(
                "the bound client must reach the mock at its own route, authorized with the \
                 bound token, not GITHUB_API_BASE_URL or the default credential",
            );
        mock.assert();
    }

    #[test]
    fn scoped_client_default_for_borrows_the_default_without_a_bound_token() {
        let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        let default = GitHubClient::new(Http::new("http://127.0.0.1:1", Vec::new(), credential));
        let scoped = ScopedClient::default_for(&default, None);
        assert!(matches!(scoped, ScopedClient::Default(_)));
    }

    #[test]
    fn scoped_client_default_for_builds_a_fresh_client_with_a_bound_token() {
        let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        let default = GitHubClient::new(Http::new("http://127.0.0.1:1", Vec::new(), credential));
        let token = GitHubToken::parse(GitHubToken::example()).unwrap();
        let scoped = ScopedClient::default_for(&default, Some(&token));
        assert!(matches!(scoped, ScopedClient::Bound(_)));
    }
}
