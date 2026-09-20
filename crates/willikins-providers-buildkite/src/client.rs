//! [`BuildkiteClient`]: typed calls for exactly the endpoints the two
//! Buildkite tools need, plus one extra (`delete_pipeline`) used only by
//! the opt-in live write cycle to clean up after itself.
//!
//! No tool ever builds a URL or query string itself; every path this
//! client builds is assembled from already-validated domain types
//! ([`BuildkiteOrg`], [`BuildkitePipelineSlug`]), whose grammars are
//! alphanumeric-and-hyphen only, so none of them can smuggle a `/`, a
//! `?`, or `&` into the request line.
//!
//! See `docs/research/2026-09-16-m3a-buildkite.md` for every fact this
//! module rests on, and
//! `docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`'s
//! trust boundaries 6, 7, and 8.

use serde::{Deserialize, Serialize};

use willikins_providers_http::{Credential, CredentialError, Http, ProviderError};
use willikins_types::{BuildkiteClusterId, BuildkiteOrg, BuildkitePipelineSlug, GitHubRepo};

/// Buildkite's REST API base URL.
pub const BUILDKITE_API_BASE_URL: &str = "https://api.buildkite.com";

/// The environment variable a Buildkite [`Credential`] is read from.
pub const CREDENTIAL_VAR: &str = "WILLIKINS_BUILDKITE_TOKEN";

/// The shape of a Buildkite token this crate accepts for provisioning: an
/// **API access token** (`bkua_`, "Buildkite user access" -- research note
/// section 3). The body class is deliberately tolerant
/// (`[A-Za-z0-9_-]`, unbounded above): Buildkite masks every published
/// token body, so no exact length may be inferred, and a pattern that
/// refused a real token would block the operator while a pattern that
/// accepted a malformed one costs one clear `401`.
pub const CREDENTIAL_PATTERN: &str = "^bkua_[A-Za-z0-9_-]{20,}$";

/// The pipeline `description` that marks a Buildkite pipeline as
/// willikins' own. The same bytes as the Doppler marker
/// (`willikins_providers_doppler::MANAGED_DESCRIPTION`) -- Buildkite has
/// no per-pipeline ownership marker of its own, so this crate supplies
/// one exactly the way `doppler.project.ensure` does.
pub const MANAGED_DESCRIPTION: &str = "managed-by: willikins";

/// The frozen bootstrap configuration every pipeline this crate creates
/// is created with (plan decision (a)). Buildkite's own documented
/// minimal configuration for keeping the real pipeline definition in the
/// repository (research note section 1, "The configuration string"):
///
/// ```text
/// steps:
///  - command: "buildkite-agent pipeline upload"
/// ```
///
/// This is the *only* command-shaped string in this crate's whole
/// surface, it is never built from an input, and there is no second one
/// anywhere: no tool in this crate accepts a configuration, a command, a
/// step, or any YAML at all. A workflow that wants different CI behaviour
/// changes the repository's own `.buildkite/pipeline.yml`, which the
/// pipeline this constant creates immediately reads and runs.
pub const UPLOAD_CONFIGURATION: &str = "steps:\n - command: \"buildkite-agent pipeline upload\"";

/// The greatest number of pages [`BuildkiteClient::list_clusters_page`]'s
/// caller (`buildkite.cluster.get`) will fetch before giving up: 10 pages
/// at `per_page=100` is 1,000 clusters, comfortably past what any
/// operator using a name-keyed lookup would have. Past this bound the
/// caller reports [`willikins_core::ToolError::Provider`] naming the page
/// bound, rather than looping forever against an organisation this tool
/// was never meant to serve.
pub const MAX_CLUSTER_PAGES: u32 = 10;

/// How many clusters [`BuildkiteClient::list_clusters_page`] asks for on
/// each page. Buildkite's documented maximum (research note section 3,
/// "Pagination"). `pub(crate)` so `buildkite.cluster.get` can recognise a
/// short (final) page without a second copy of this number.
pub(crate) const CLUSTERS_PER_PAGE: u32 = 100;

/// Why [`credential_from_env`] refused to build a [`Credential`].
///
/// Never carries the environment variable's value: the `WrongKind`
/// variant is raised from [`CredentialError::Malformed`] alone, without
/// ever having read the value itself -- the same rule
/// `willikins_providers_doppler::DopplerCredentialError` follows, for the
/// same reason its own doc gives.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildkiteCredentialError {
    /// The environment variable was unset or empty. See
    /// [`CredentialError::Missing`].
    #[error(transparent)]
    Missing(CredentialError),
    /// The environment variable was set but did not match
    /// [`CREDENTIAL_PATTERN`]: either it is not shaped like any Buildkite
    /// token at all, or it is an agent (cluster) token (`bkct_`), which
    /// cannot provision.
    #[error(
        "a Buildkite API access token (`bkua_`) is needed to provision; an agent token \
         (`bkct_`) cannot create or read pipelines"
    )]
    WrongKind,
}

/// Read [`CREDENTIAL_VAR`] from the process environment and validate it
/// against [`CREDENTIAL_PATTERN`].
///
/// Deliberately does not pre-inspect the raw environment variable's value
/// to give a more specific message, for the same reason
/// `willikins_providers_doppler::credential_from_env` gives: doing so
/// would read a Buildkite token's bytes into a plain `String` inside this
/// crate, a second site invisible to
/// `crates/willikins-core/tests/expose_secret_guard.rs`.
///
/// # Errors
///
/// See [`Credential::from_env`], mapped through
/// [`BuildkiteCredentialError`].
///
/// # Panics
///
/// Never in practice: [`CREDENTIAL_PATTERN`] is a fixed, compile-time-known
/// literal, and this module's own `tests` submodule builds a `Regex` from
/// the exact same constant, so `Regex::new` on it cannot fail.
pub fn credential_from_env() -> Result<Credential, BuildkiteCredentialError> {
    let pattern =
        regex::Regex::new(CREDENTIAL_PATTERN).expect("CREDENTIAL_PATTERN is a valid regex");
    Credential::from_env(CREDENTIAL_VAR, &pattern).map_err(|err| match err {
        CredentialError::Missing { .. } => BuildkiteCredentialError::Missing(err),
        CredentialError::Malformed { .. } => BuildkiteCredentialError::WrongKind,
    })
}

/// Build an [`Http`] against Buildkite's real API, carrying `credential`.
/// Buildkite needs only the bearer `Authorization` header every [`Http`]
/// request already carries.
#[must_use]
pub fn http_client(credential: Credential) -> Http {
    Http::new(BUILDKITE_API_BASE_URL, Vec::new(), credential)
}

/// The repository URL this crate sends on create and compares on read:
/// `git@github.com:{owner}/{name}.git`, built from an already-parsed
/// [`GitHubRepo`] rather than accepted as a string anywhere (plan
/// decision (a), "The repository URL"). Never itself a port; a second
/// frozen form alongside [`UPLOAD_CONFIGURATION`].
#[must_use]
pub fn ssh_repository_url(repo: &GitHubRepo) -> String {
    format!("git@github.com:{}/{}.git", repo.owner(), repo.name())
}

/// The `https://buildkite.com/{org}/{slug}` URL Buildkite documents as a
/// pipeline's `web_url` (research note section 1, "Create"). Built
/// directly from already-parsed domain types rather than trusting the
/// provider's own `web_url` field, which this client deserializes (trust
/// boundary 7 names it as one of the pipeline response's six allowed
/// fields) but never otherwise reads.
#[must_use]
pub fn pipeline_web_url(org: &BuildkiteOrg, slug: &BuildkitePipelineSlug) -> String {
    format!("https://buildkite.com/{org}/{slug}")
}

/// A typed Buildkite REST client, bound to one [`Http`] (which itself
/// owns the [`Credential`]).
pub struct BuildkiteClient {
    http: Http,
}

impl BuildkiteClient {
    /// Build a client over `http`.
    #[must_use]
    pub fn new(http: Http) -> Self {
        Self { http }
    }

    /// `GET /v2/organizations/{org}/pipelines/{slug}`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] for any non-2xx response (a `404`
    /// meaning "absent", handled by the caller) or a transport failure.
    pub(crate) fn get_pipeline(
        &self,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
    ) -> Result<PipelineBody, ProviderError> {
        let path = format!("/v2/organizations/{org}/pipelines/{slug}");
        self.http.get(&path)
    }

    /// `POST /v2/organizations/{org}/pipelines` with exactly `name`
    /// (equal to `slug`), `slug`, `cluster_id`, `repository`,
    /// `description` (the [`MANAGED_DESCRIPTION`] marker), and
    /// `configuration` (the frozen [`UPLOAD_CONFIGURATION`]) -- no other
    /// field, and never `steps`, `env`, `provider_settings`, `teams`,
    /// `tags`, or `visibility` (trust boundary 8; acceptance test 3).
    /// Never retried, by this client or by [`Http`] underneath it: an
    /// ambiguous failure here is resolved by the caller re-`read`ing.
    ///
    /// # Errors
    ///
    /// See [`Self::get_pipeline`].
    pub(crate) fn create_pipeline(
        &self,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
        cluster: &BuildkiteClusterId,
        repository: &str,
    ) -> Result<(), ProviderError> {
        let path = format!("/v2/organizations/{org}/pipelines");
        let body = CreatePipelineBody {
            name: slug.to_string(),
            slug: slug.to_string(),
            cluster_id: cluster.to_string(),
            repository: repository.to_string(),
            description: MANAGED_DESCRIPTION.to_string(),
            configuration: UPLOAD_CONFIGURATION.to_string(),
        };
        self.http.post::<PipelineBody>(&path, &body)?;
        Ok(())
    }

    /// `GET /v2/organizations/{org}/clusters?page={page}&per_page=100`.
    ///
    /// The response is a bare JSON array of cluster objects, from which
    /// only `id` and `name` are ever deserialized (trust boundary 7). The
    /// `Link` pagination header is never read, followed, or logged: its
    /// documented example embeds an `api_key` query parameter, so a
    /// caller pages with these explicit `page`/`per_page` parameters
    /// instead, stopping at the first page shorter than
    /// [`CLUSTERS_PER_PAGE`] or at [`MAX_CLUSTER_PAGES`].
    ///
    /// # Errors
    ///
    /// See [`Self::get_pipeline`].
    pub(crate) fn list_clusters_page(
        &self,
        org: &BuildkiteOrg,
        page: u32,
    ) -> Result<Vec<ClusterBody>, ProviderError> {
        let path =
            format!("/v2/organizations/{org}/clusters?page={page}&per_page={CLUSTERS_PER_PAGE}");
        self.http.get(&path)
    }

    /// `DELETE /v2/organizations/{org}/pipelines/{slug}`.
    ///
    /// **Used only by the opt-in live write cycle** (`tests/live_write_cycle.rs`,
    /// behind the `live-tests` feature), to remove the pipeline it
    /// created; no tool in this crate calls it, matching the plan's
    /// "Pipeline update, delete, and archive tools" out-of-scope note.
    /// `pub` rather than `pub(crate)` only because that test lives in a
    /// separate crate (`tests/*.rs` targets link this crate as an
    /// external dependency and cannot see `pub(crate)` items) -- the
    /// restriction is enforced by this doc comment and by there being no
    /// second caller, not by visibility.
    ///
    /// # Errors
    ///
    /// See [`Self::get_pipeline`].
    pub fn delete_pipeline(
        &self,
        org: &BuildkiteOrg,
        slug: &BuildkitePipelineSlug,
    ) -> Result<(), ProviderError> {
        let path = format!("/v2/organizations/{org}/pipelines/{slug}");
        self.http.delete(&path)
    }
}

/// A Buildkite pipeline's REST representation, deserializing **exactly**
/// the six fields trust boundary 7 names: `id`, `slug`, `web_url`,
/// `repository`, `cluster_id`, `description`. No `provider` (which would
/// carry the credential-bearing `webhook_url`), `steps`, `configuration`,
/// or `env` field exists on this type at all -- Buildkite may send them,
/// and `serde`'s default behaviour (no `deny_unknown_fields`) silently
/// discards them before they ever become a `String` in this process.
///
/// `id`, `slug`, and `web_url` are deserialized but never read past that
/// point (`#[allow(dead_code)]` below, rather than dropping the fields):
/// keeping them on the struct is what makes this type traceable, field
/// for field, to trust boundary 7's list, and a reviewer can see at a
/// glance that nothing else -- `provider`, `steps`, `configuration`,
/// `env` -- was added beside them.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct PipelineBody {
    /// The pipeline's opaque id. Never compared or surfaced; deserialized
    /// only to keep this struct's shape traceable to the real response.
    pub(crate) id: String,
    /// The pipeline's slug. Not itself part of `read`'s decision (the key
    /// is already known before the call is made).
    pub(crate) slug: String,
    /// The documented `https://buildkite.com/{org}/{slug}` URL. Never
    /// consulted: [`pipeline_web_url`] builds the same string from
    /// already-typed domain types instead, so a malformed or
    /// unexpectedly-shaped `web_url` never fails a read.
    pub(crate) web_url: String,
    /// The repository URL, compared exactly against [`ssh_repository_url`].
    pub(crate) repository: String,
    /// The cluster this pipeline belongs to, nullable in Buildkite's own
    /// schema (the create response's stale `null` example -- research
    /// note section 1).
    pub(crate) cluster_id: Option<String>,
    /// The ownership marker field, compared exactly against
    /// [`MANAGED_DESCRIPTION`]. Nullable: a pipeline created with no
    /// description at all.
    pub(crate) description: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreatePipelineBody {
    name: String,
    slug: String,
    cluster_id: String,
    repository: String,
    description: String,
    configuration: String,
}

/// A Buildkite cluster's REST representation, deserializing only `id` and
/// `name` (trust boundary 7's cluster half): no `default_queue_id`,
/// `graphql_id`, or any other field this crate does not need.
#[derive(Debug, Deserialize)]
pub(crate) struct ClusterBody {
    /// The cluster's opaque UUID.
    pub(crate) id: String,
    /// The cluster's human-written, mutable, non-unique name.
    pub(crate) name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // A distinctive marker: if it ever showed up in a rendered error, a
    // redaction rule broke. Only used below to prove `WrongKind`'s
    // message never echoes anything about the value that triggered it.
    const MARKER: &str = "wlkn-test-marker-9fh2plq7rzk";

    fn pattern() -> regex::Regex {
        regex::Regex::new(CREDENTIAL_PATTERN).expect("CREDENTIAL_PATTERN is a valid regex")
    }

    #[test]
    fn accepts_an_api_access_token() {
        let token = format!("bkua_{}", "a".repeat(20));
        assert!(pattern().is_match(&token), "{token}");
        let token = format!("bkua_{}", "a".repeat(64));
        assert!(pattern().is_match(&token), "{token}");
    }

    #[test]
    fn rejects_an_agent_token() {
        let token = format!("bkct_{}", "a".repeat(20));
        assert!(!pattern().is_match(&token), "{token}");
    }

    #[test]
    fn rejects_a_suffix_one_short_of_the_minimum() {
        let token = format!("bkua_{}", "a".repeat(19));
        assert!(!pattern().is_match(&token), "{token}");
    }

    #[test]
    fn rejects_a_trailing_newline() {
        let token = format!("bkua_{}\n", "a".repeat(20));
        assert!(!pattern().is_match(&token), "{token:?}");
    }

    #[test]
    fn wrong_kind_message_names_the_needed_kind_and_the_wrong_one_and_echoes_nothing() {
        let message = BuildkiteCredentialError::WrongKind.to_string();
        assert!(message.contains("bkua_"), "{message}");
        assert!(message.contains("bkct_"), "{message}");
        assert!(!message.contains(MARKER), "{message}");
    }

    #[test]
    fn ssh_repository_url_builds_the_frozen_form() {
        use willikins_types::{DomainType, GitHubOrg, ProjectSlug};
        let repo = GitHubRepo::new(
            GitHubOrg::parse("lightless-labs").unwrap(),
            ProjectSlug::parse("third-thoughts").unwrap(),
        );
        assert_eq!(
            ssh_repository_url(&repo),
            "git@github.com:lightless-labs/third-thoughts.git"
        );
    }

    #[test]
    fn pipeline_web_url_builds_the_documented_form() {
        use willikins_types::DomainType;
        let org = BuildkiteOrg::parse("willikins-test").unwrap();
        let slug = BuildkitePipelineSlug::parse("third-thoughts").unwrap();
        assert_eq!(
            pipeline_web_url(&org, &slug),
            "https://buildkite.com/willikins-test/third-thoughts"
        );
    }

    #[test]
    fn upload_configuration_is_the_documented_bootstrap() {
        assert_eq!(
            UPLOAD_CONFIGURATION,
            "steps:\n - command: \"buildkite-agent pipeline upload\""
        );
    }
}
