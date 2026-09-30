//! [`GitHubClient`]: typed calls for exactly the endpoints
//! `github.repo.ensure` and `github.actions_secret.ensure` need. No tool
//! ever builds a URL or path itself; every path this client builds is
//! assembled from already-validated domain types
//! ([`willikins_types::GitHubOrg`], [`willikins_types::ProjectSlug`] via
//! [`willikins_types::GitHubRepo`], [`willikins_types::ActionsSecretName`]),
//! whose grammars are alphanumeric-and-hyphen-or-underscore, so none of
//! them can smuggle a `/` or a query string into the request line.
//!
//! # Reading a repository's files (milestone 3g, task G1, commit 1)
//!
//! `docs/plans/2026-09-30-milestone-3g-file-writing.md` decision (b): reads go through the
//! REST git database, pinned to one commit ([`Self::get_branch_head`] →
//! [`Self::get_commit_root_tree`] → [`Self::resolve_tree_paths`], which walks only the
//! non-recursive trees a declared [`RepoPath`] actually needs, memoised per directory, and
//! never downloads a file's content — content is compared by [`git_blob_sha`], computed
//! locally). The only blob this client ever downloads is a scaffold's marker
//! ([`Self::get_blob`]). The write (GraphQL's `createCommitOnBranch`) is this task's second
//! commit. Neither `github.scaffold.ensure` (task G2) nor its business rules for what
//! `Present`/`Foreign`/`Absent`/a conflict mean live here — this module exposes only the
//! typed calls.
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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use willikins_core::{ToolError, ToolErrorKind};
use willikins_providers_http::{
    Credential, Http, MAX_RETRY_AFTER, ProviderError, RealSleeper, Sleeper,
};
use willikins_types::{
    ActionsSecretName, GitBranchName, GitHubRepo, GitHubToken, RepoPath, RepoVisibility,
};

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

/// Build an [`Http`] against GitHub's real API, carrying [`default_headers`]
/// but no [`Credential`] at all: every request this crate's tools send
/// through it refuses locally, naming [`CREDENTIAL_VAR`], unless
/// [`willikins_providers_http::Http::with_credential`] first replaces it
/// (which every tool's own `token`-port handling already does when a
/// document binds one — see [`client_for_token`]).
///
/// For a caller that has already established, from the document being
/// planned, that every `github.*` node binds its own `token` port (so
/// [`CREDENTIAL_VAR`] is not needed at all): `willikins_server::catalog`'s
/// per-document credential narrowing is the one caller today.
#[must_use]
pub fn http_client_without_credential() -> Http {
    Http::without_credential(GITHUB_API_BASE_URL, default_headers(), CREDENTIAL_VAR)
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

    /// `GET /repos/{owner}/{repo}/git/ref/heads/{branch}`, returning the
    /// branch's current head commit sha. A `404` means the branch itself
    /// does not exist -- this client never creates one (decision (b),
    /// `docs/plans/2026-09-30-milestone-3g-file-writing.md`).
    ///
    /// # Errors
    ///
    /// See [`Self::get_repo`].
    #[allow(dead_code)] // not yet called outside tests: wired in by github.scaffold.ensure, milestone 3g task G2
    pub(crate) fn get_branch_head(
        &self,
        repo: &GitHubRepo,
        branch: &GitBranchName,
    ) -> Result<String, ProviderError> {
        let path = format!("{}/git/ref/heads/{branch}", repo_path(repo));
        self.retry_secondary_limit(|| self.http.get::<RefBody>(&path))
            .map(|body| body.object.sha)
    }

    /// `GET /repos/{owner}/{repo}/git/commits/{commit_sha}`, returning the
    /// root tree sha that commit points to.
    ///
    /// # Errors
    ///
    /// See [`Self::get_repo`].
    #[allow(dead_code)] // see get_branch_head's own note above
    pub(crate) fn get_commit_root_tree(
        &self,
        repo: &GitHubRepo,
        commit_sha: &str,
    ) -> Result<String, ProviderError> {
        let path = format!("{}/git/commits/{commit_sha}", repo_path(repo));
        self.retry_secondary_limit(|| self.http.get::<CommitBody>(&path))
            .map(|body| body.tree.sha)
    }

    /// `GET /repos/{owner}/{repo}/git/trees/{tree_sha}`, **never**
    /// `?recursive=1`: a caller only ever needs one directory level at a
    /// time (see [`Self::resolve_tree_paths`]), and a recursive read of a
    /// busy monorepo's root risks GitHub's own truncation.
    #[allow(dead_code)] // see get_branch_head's own note above
    fn get_tree_entries(
        &self,
        repo: &GitHubRepo,
        tree_sha: &str,
    ) -> Result<Vec<TreeEntryBody>, ProviderError> {
        let path = format!("{}/git/trees/{tree_sha}", repo_path(repo));
        self.retry_secondary_limit(|| self.http.get::<TreeBody>(&path))
            .map(|body| body.tree)
    }

    /// Resolve every one of `paths` against the tree rooted at
    /// `root_tree_sha` (itself [`Self::get_commit_root_tree`]'s own
    /// output), walking only the directories those paths actually pass
    /// through and fetching each such directory's tree **at most once**,
    /// even when several paths share a prefix: a tree sha is
    /// content-addressed, so caching by it (rather than by directory path)
    /// is both simpler and correct even if the same directory were somehow
    /// reachable two different ways. No blob's content is ever downloaded
    /// here — see [`git_blob_sha`] for how a caller compares content
    /// without one.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] the first time a directory read fails;
    /// paths later in `paths` are never attempted.
    #[allow(dead_code)] // see get_branch_head's own note above
    pub(crate) fn resolve_tree_paths(
        &self,
        repo: &GitHubRepo,
        root_tree_sha: &str,
        paths: &[RepoPath],
    ) -> Result<HashMap<RepoPath, PathEntry>, ProviderError> {
        let mut cache: HashMap<String, Vec<TreeEntryBody>> = HashMap::new();
        let mut results = HashMap::with_capacity(paths.len());
        for path in paths {
            let entry = self.resolve_one_path(repo, root_tree_sha, path, &mut cache)?;
            results.insert(path.clone(), entry);
        }
        Ok(results)
    }

    /// Walk one [`RepoPath`]'s segments from `root_tree_sha`, directory by
    /// directory, through `cache` (shared across every path
    /// [`Self::resolve_tree_paths`] resolves in the same call).
    #[allow(dead_code)] // see get_branch_head's own note above
    fn resolve_one_path(
        &self,
        repo: &GitHubRepo,
        root_tree_sha: &str,
        path: &RepoPath,
        cache: &mut HashMap<String, Vec<TreeEntryBody>>,
    ) -> Result<PathEntry, ProviderError> {
        let segments: Vec<&str> = path.segments().collect();
        let mut current_tree_sha = root_tree_sha.to_string();
        for (index, segment) in segments.iter().enumerate() {
            let entries = self.cached_tree_entries(repo, &current_tree_sha, cache)?;
            let Some(found) = entries.iter().find(|entry| entry.path == *segment) else {
                return Ok(PathEntry::Absent);
            };
            if index + 1 == segments.len() {
                return Ok(found.classify());
            }
            if found.entry_type != "tree" {
                // An intermediate segment exists but is not a directory,
                // so the declared path underneath it cannot exist either.
                return Ok(PathEntry::Absent);
            }
            current_tree_sha = found.sha.clone();
        }
        // `RepoPath::parse` refuses an empty path, so `segments` always
        // holds at least one element and the loop above always returns
        // before reaching here.
        unreachable!("a RepoPath always has at least one segment")
    }

    /// Fetch `tree_sha`'s entries, memoised in `cache` for the lifetime of
    /// one [`Self::resolve_tree_paths`] call — the "each directory fetched
    /// once" half of decision (b).
    #[allow(dead_code)] // see get_branch_head's own note above
    fn cached_tree_entries<'a>(
        &self,
        repo: &GitHubRepo,
        tree_sha: &str,
        cache: &'a mut HashMap<String, Vec<TreeEntryBody>>,
    ) -> Result<&'a Vec<TreeEntryBody>, ProviderError> {
        if !cache.contains_key(tree_sha) {
            let entries = self.get_tree_entries(repo, tree_sha)?;
            cache.insert(tree_sha.to_string(), entries);
        }
        Ok(cache
            .get(tree_sha)
            .expect("just inserted above, or already present"))
    }

    /// `GET /repos/{owner}/{repo}/git/blobs/{sha}`, decoded to raw bytes.
    /// The only caller in this milestone is `github.scaffold.ensure`'s own
    /// `read` (task G2), and only ever for the scaffold's marker blob —
    /// decision (b) forbids downloading any other file's content, which is
    /// instead compared by [`git_blob_sha`].
    ///
    /// # Errors
    ///
    /// See [`Self::get_repo`], plus a statusless [`ProviderError`] when
    /// GitHub's `encoding` field is not `"base64"` or the content does not
    /// decode as base64 — both would otherwise silently read as an empty
    /// or nonsensical marker rather than a loud failure.
    #[allow(dead_code)] // see get_branch_head's own note above
    pub(crate) fn get_blob(&self, repo: &GitHubRepo, sha: &str) -> Result<Vec<u8>, ProviderError> {
        let path = format!("{}/git/blobs/{sha}", repo_path(repo));
        let body: BlobBody = self.retry_secondary_limit(|| self.http.get(&path))?;
        if body.encoding != "base64" {
            return Err(ProviderError::new(
                None,
                "GitHub returned a blob in an encoding this client does not support",
            ));
        }
        // GitHub wraps base64 content with a newline every 60 characters;
        // the standard engine's decoder rejects embedded whitespace, so
        // it is stripped first.
        let cleaned: String = body
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        STANDARD.decode(cleaned).map_err(|_| {
            ProviderError::new(None, "GitHub returned a blob that was not valid base64")
        })
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

/// The state one declared [`RepoPath`] resolves to inside a tree
/// [`GitHubClient::resolve_tree_paths`] already pinned to one commit.
/// Content is never downloaded to produce this — see [`git_blob_sha`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // see get_branch_head's own note above
pub(crate) enum PathEntry {
    /// No entry of that name exists, or an ancestor directory along the
    /// way does not exist or is itself not a directory.
    Absent,
    /// A regular or executable file. `mode` is GitHub's own string
    /// (`"100644"` or `"100755"`), `sha` its git blob sha.
    Blob { mode: String, sha: String },
    /// Something other than a file at that exact path: a subdirectory
    /// (`"040000"`), a symlink (`"120000"`), or a submodule (`"160000"`).
    NonBlob,
}

/// GitHub's `git-ref` schema: the one field this crate reads.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // see get_branch_head's own note above
struct RefBody {
    object: RefObject,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // see get_branch_head's own note above
struct RefObject {
    sha: String,
}

/// GitHub's `git-commit` schema: the one field this crate reads.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // see get_branch_head's own note above
struct CommitBody {
    tree: TreeRef,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // see get_branch_head's own note above
struct TreeRef {
    sha: String,
}

/// GitHub's `git-tree` schema, read non-recursively: one directory level
/// of entries.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // see get_branch_head's own note above
struct TreeBody {
    tree: Vec<TreeEntryBody>,
}

/// One entry of a non-recursive git tree: a name relative to its parent
/// directory, its mode, its git object type, and its own sha.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // see get_branch_head's own note above
struct TreeEntryBody {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    entry_type: String,
    sha: String,
}

impl TreeEntryBody {
    /// Classify this entry the way decision (b)'s read table does: a
    /// regular or executable file is [`PathEntry::Blob`]; a subdirectory,
    /// symlink, or submodule is [`PathEntry::NonBlob`] — by `mode`, since
    /// GitHub's `type` field alone does not distinguish a symlink
    /// (`"120000"`) from a regular file (`"100644"`/`"100755"`): both
    /// report `type: "blob"`.
    #[allow(dead_code)] // see get_branch_head's own note above
    fn classify(&self) -> PathEntry {
        match self.mode.as_str() {
            "100644" | "100755" => PathEntry::Blob {
                mode: self.mode.clone(),
                sha: self.sha.clone(),
            },
            _ => PathEntry::NonBlob,
        }
    }
}

/// GitHub's `git-blob` schema: the two fields this crate reads.
#[derive(Debug, Deserialize)]
struct BlobBody {
    content: String,
    encoding: String,
}

/// Compute the git blob sha for `content`: SHA-1 over
/// `blob <byte length>\0<bytes>`, the same algorithm `git hash-object`
/// uses. Lets a caller compare a would-be file's content against a tree
/// entry's own [`PathEntry::Blob`] sha without ever downloading that
/// entry's content (decision (b),
/// `docs/plans/2026-09-30-milestone-3g-file-writing.md`).
#[must_use]
#[allow(dead_code)] // see get_branch_head's own note above
pub(crate) fn git_blob_sha(content: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", content.len()));
    hasher.update(content);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(40);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
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

    // -----------------------------------------------------------------
    // Milestone 3g, task G1: reads and the `createCommitOnBranch` write.
    // -----------------------------------------------------------------

    fn client_against(url: String) -> GitHubClient {
        let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        GitHubClient::new(Http::new(url, Vec::new(), credential))
    }

    fn repo() -> GitHubRepo {
        GitHubRepo::parse("acme/widget").unwrap()
    }

    fn tree_entry_json(name: &str, mode: &str, entry_type: &str, sha: &str) -> serde_json::Value {
        serde_json::json!({
            "path": name,
            "mode": mode,
            "type": entry_type,
            "sha": sha,
            "size": 10,
            "url": "https://api.github.com/x",
        })
    }

    #[test]
    fn git_blob_sha_matches_known_git_hash_object_vectors() {
        // `git hash-object --stdin` on each of these three, captured
        // locally: an empty file, a plain ASCII file, and a UTF-8 file —
        // acceptance 6's own "known vectors".
        assert_eq!(
            git_blob_sha(b""),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
        assert_eq!(
            git_blob_sha(b"hello world\n"),
            "3b18e512dba79e4c8300dd08aeb37f8e728b8dad"
        );
        assert_eq!(
            git_blob_sha("héllo wörld\n".as_bytes()),
            "9d4a8bab579c9317dc648e018736aec79914b21a"
        );
    }

    #[test]
    fn get_branch_head_then_commit_root_tree_pin_the_exact_endpoints() {
        let mut provider = willikins_providers_http::testing::MockProvider::start();
        let branch = GitBranchName::parse("main").unwrap();
        let ref_mock = provider
            .mock("GET", "/repos/acme/widget/git/ref/heads/main")
            .with_status(200)
            .with_body(serde_json::json!({"object": {"sha": "head-commit-sha"}}).to_string())
            .expect(1)
            .create();
        let commit_mock = provider
            .mock("GET", "/repos/acme/widget/git/commits/head-commit-sha")
            .with_status(200)
            .with_body(serde_json::json!({"tree": {"sha": "root-tree-sha"}}).to_string())
            .expect(1)
            .create();

        let client = client_against(provider.url());
        let head = client.get_branch_head(&repo(), &branch).unwrap();
        assert_eq!(head, "head-commit-sha");
        let root_tree = client.get_commit_root_tree(&repo(), &head).unwrap();
        assert_eq!(root_tree, "root-tree-sha");

        ref_mock.assert();
        commit_mock.assert();
    }

    /// Acceptance 6, in full: a multi-directory layout resolved from one
    /// root tree, several declared paths sharing prefixes so each
    /// directory's tree is fetched exactly once (`.expect(1)` per mock), a
    /// symlink/tree/submodule each reported as [`PathEntry::NonBlob`], an
    /// absent file and a file under a directory that never existed both
    /// [`PathEntry::Absent`], and no blob ever downloaded by this call at
    /// all.
    #[test]
    #[allow(clippy::too_many_lines)] // one scenario proving every row of decision (b)'s read table at once
    fn resolve_tree_paths_walks_declared_paths_fetching_each_directory_once() {
        let mut provider = willikins_providers_http::testing::MockProvider::start();
        let root_tree = provider
            .mock("GET", "/repos/acme/widget/git/trees/root-tree-sha")
            .with_status(200)
            .with_body(
                serde_json::json!({"sha": "root-tree-sha", "tree": [
                    tree_entry_json("apps", "040000", "tree", "apps-tree-sha"),
                ]})
                .to_string(),
            )
            .expect(1)
            .create();
        let apps_tree = provider
            .mock("GET", "/repos/acme/widget/git/trees/apps-tree-sha")
            .with_status(200)
            .with_body(
                serde_json::json!({"sha": "apps-tree-sha", "tree": [
                    tree_entry_json("walter", "040000", "tree", "walter-tree-sha"),
                ]})
                .to_string(),
            )
            .expect(1)
            .create();
        let walter_tree = provider
            .mock("GET", "/repos/acme/widget/git/trees/walter-tree-sha")
            .with_status(200)
            .with_body(
                serde_json::json!({"sha": "walter-tree-sha", "tree": [
                    tree_entry_json("ios", "040000", "tree", "ios-tree-sha"),
                    tree_entry_json("BUILD.bazel", "100644", "blob", "buildbazel-sha"),
                ]})
                .to_string(),
            )
            .expect(1)
            .create();
        let ios_tree = provider
            .mock("GET", "/repos/acme/widget/git/trees/ios-tree-sha")
            .with_status(200)
            .with_body(
                serde_json::json!({"sha": "ios-tree-sha", "tree": [
                    tree_entry_json("Resources", "040000", "tree", "resources-tree-sha"),
                    tree_entry_json("BUILD.bazel", "100644", "blob", "ios-build-sha"),
                    tree_entry_json("some-symlink", "120000", "blob", "symlink-sha"),
                    tree_entry_json("some-submodule", "160000", "commit", "submodule-sha"),
                ]})
                .to_string(),
            )
            .expect(1)
            .create();
        let resources_tree = provider
            .mock("GET", "/repos/acme/widget/git/trees/resources-tree-sha")
            .with_status(200)
            .with_body(
                serde_json::json!({"sha": "resources-tree-sha", "tree": [
                    tree_entry_json("Info.plist", "100644", "blob", "info-plist-sha"),
                ]})
                .to_string(),
            )
            .expect(1)
            .create();
        let no_blob = provider
            .mock("GET", "/repos/acme/widget/git/blobs/buildbazel-sha")
            .expect(0)
            .create();

        let client = client_against(provider.url());
        let build_bazel = RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
        let ios_build_bazel = RepoPath::parse("apps/walter/ios/BUILD.bazel").unwrap();
        let info_plist = RepoPath::parse("apps/walter/ios/Resources/Info.plist").unwrap();
        let symlink = RepoPath::parse("apps/walter/ios/some-symlink").unwrap();
        let submodule = RepoPath::parse("apps/walter/ios/some-submodule").unwrap();
        let under_missing_dir = RepoPath::parse("apps/walter/ios/missing-dir/x").unwrap();
        let missing_file = RepoPath::parse("apps/walter/missing.txt").unwrap();
        let paths = vec![
            build_bazel.clone(),
            ios_build_bazel.clone(),
            info_plist.clone(),
            symlink.clone(),
            submodule.clone(),
            under_missing_dir.clone(),
            missing_file.clone(),
        ];

        let resolved = client
            .resolve_tree_paths(&repo(), "root-tree-sha", &paths)
            .unwrap();

        assert_eq!(
            resolved[&build_bazel],
            PathEntry::Blob {
                mode: "100644".to_string(),
                sha: "buildbazel-sha".to_string(),
            }
        );
        assert_eq!(
            resolved[&ios_build_bazel],
            PathEntry::Blob {
                mode: "100644".to_string(),
                sha: "ios-build-sha".to_string(),
            }
        );
        assert_eq!(
            resolved[&info_plist],
            PathEntry::Blob {
                mode: "100644".to_string(),
                sha: "info-plist-sha".to_string(),
            }
        );
        assert_eq!(resolved[&symlink], PathEntry::NonBlob);
        assert_eq!(resolved[&submodule], PathEntry::NonBlob);
        assert_eq!(resolved[&under_missing_dir], PathEntry::Absent);
        assert_eq!(resolved[&missing_file], PathEntry::Absent);

        root_tree.assert();
        apps_tree.assert();
        walter_tree.assert();
        ios_tree.assert();
        resources_tree.assert();
        no_blob.assert();
    }

    #[test]
    fn get_blob_decodes_base64_content_stripping_githubs_line_wrapping() {
        let mut provider = willikins_providers_http::testing::MockProvider::start();
        let marker_text = "managed-by: willikins\napps/walter/BUILD.bazel deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n";
        let encoded = STANDARD.encode(marker_text.as_bytes());
        // GitHub wraps its base64 `content` with a newline every 60
        // characters; reproducing that here proves this client's own
        // stripping, not merely that unwrapped base64 decodes.
        let wrapped = encoded
            .as_bytes()
            .chunks(60)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let mock = provider
            .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
            .with_status(200)
            .with_body(serde_json::json!({"content": wrapped, "encoding": "base64"}).to_string())
            .create();
        let client = client_against(provider.url());
        let bytes = client.get_blob(&repo(), "marker-sha").unwrap();
        assert_eq!(bytes, marker_text.as_bytes());
        mock.assert();
    }

    #[test]
    fn get_blob_refuses_an_encoding_other_than_base64() {
        let mut provider = willikins_providers_http::testing::MockProvider::start();
        provider
            .mock("GET", "/repos/acme/widget/git/blobs/marker-sha")
            .with_status(200)
            .with_body(
                serde_json::json!({"content": "plain text", "encoding": "utf-8"}).to_string(),
            )
            .create();
        let client = client_against(provider.url());
        let err = client.get_blob(&repo(), "marker-sha").unwrap_err();
        assert_eq!(err.status, None);
    }
}
