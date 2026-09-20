//! [`DopplerClient`]: typed calls for exactly the endpoints the five
//! Doppler tools need. No tool ever builds a URL or query string itself;
//! every path this client builds is assembled from already-validated
//! domain types ([`DopplerProject`], [`DopplerConfigName`],
//! [`DopplerTokenName`], [`SecretName`]), whose grammars are
//! alphanumeric-and-hyphen-or-underscore, so none of them can smuggle a
//! `/`, a `?`, or `&` into the request line.
//!
//! Doppler documents no error-body schema for any non-2xx response
//! (research note `docs/research/2026-09-12-m2-dependencies.md`, section
//! 3): the shared [`willikins_providers_http::Http`] client already
//! treats an error body as opaque, reading a `messages` array when one is
//! present and otherwise reporting the status alone, so this client adds
//! no error-shape parsing of its own.

use serde::{Deserialize, Serialize};

use willikins_providers_http::{Credential, CredentialError, Http, ProviderError};
use willikins_types::{
    DopplerConfigName, DopplerProject, DopplerSecretValue, DopplerServiceToken, DopplerTokenName,
    SecretName,
};

/// Doppler's REST API base URL.
pub const DOPPLER_API_BASE_URL: &str = "https://api.doppler.com";

/// The environment variable a Doppler [`Credential`] is read from.
pub const CREDENTIAL_VAR: &str = "WILLIKINS_DOPPLER_TOKEN";

/// The shape of a Doppler token this crate accepts for provisioning: a
/// Service Account token (`dp.sa.`) or a Personal token (`dp.pt.`). A
/// Service token (`dp.st.`) is secrets-only within one config and cannot
/// provision (research note section 3, "Doppler authentication and token
/// types"); [`credential_from_env`] refuses one with
/// [`DopplerCredentialError::WrongKind`] rather than the shared crate's
/// generic [`CredentialError::Malformed`].
pub const CREDENTIAL_PATTERN: &str = r"^dp\.(sa|pt)\.[a-zA-Z0-9]{40,44}$";

/// The project `description` that marks a Doppler project as willikins'
/// own. Configs and service tokens under an owned project are ours by
/// construction — Doppler has no per-config or per-token ownership
/// marker of its own.
pub const MANAGED_DESCRIPTION: &str = "managed-by: willikins";

/// A message fragment Doppler answers with on a `400` for a project name
/// this token cannot see. Observed live 2026-09-20
/// (`docs/solutions/providers/doppler-400s-a-missing-project-when-quiescent.md`):
/// the *same* absent project name answers `404` "Could not find requested
/// project" in the minutes after another project in the workplace was
/// created, and this `400` "This token does not have access to requested
/// project" when the workplace has been quiescent, or shortly after a
/// project was deleted. Same token, same endpoint, same shape of name —
/// the difference is purely how recently the workplace changed.
///
/// See [`looks_like_a_missing_project`] for what this crate does with
/// that fact, and what it deliberately does not assume.
const DOPPLER_NO_ACCESS_MESSAGE: &str = "does not have access to requested project";

/// Whether `err` looks like Doppler saying "no project by this name is
/// visible to this token": a `404`, or a `400` whose message names
/// [`DOPPLER_NO_ACCESS_MESSAGE`].
///
/// **What this does not prove.** Doppler documents no error-body schema
/// for any non-2xx response at all, so a status is all either code ever
/// is. Neither status, nor this message, distinguishes "this project
/// does not exist" from "this project exists, but outside this token's
/// grant": a service-account token deliberately granted nothing was
/// probed on 2026-09-16
/// (`docs/research/2026-09-12-m2-dependencies.md`, "Service-account
/// access") and found the *opposite* pairing — a bare `404` "Could not
/// find requested project" for a project that genuinely exists but sits
/// outside its grant. So a `404` cannot be trusted to mean "does not
/// exist" either; it never could, and this predicate does not change
/// that.
///
/// **What this crate commits to instead.** At plan time, every one of
/// these answers means the same thing this crate can act on: "this
/// token cannot see a project by this name right now". Every `read` in
/// this crate already mapped a bare `404` to `Absent` (or, for the
/// token-list endpoint, to `false`) on exactly that reasoning; widening
/// the same reasoning to this `400` closes the gap that let a `doppler.
/// project.ensure` node refuse to plan at all whenever the workplace
/// happened to be quiescent — the ordinary case, not the rare one.
/// Nothing here treats a `400`/`404` as *proof* of absence: the create
/// call (or, for `doppler.secret.get`, apply time — there is no create
/// path to defer to) stays the only arbiter, the same deferral
/// `doppler.project.ensure::ensure` already relies on by re-reading
/// after a failed create rather than parsing its error body.
///
/// **Where the tolerance stops.** A `400` whose message does not name
/// [`DOPPLER_NO_ACCESS_MESSAGE`] — Doppler's *other* documented `400`,
/// "Project name already exists in this workplace." on a duplicate
/// create, chief among them — still fails. That message is checked for
/// exactly because the two are otherwise both bare `400`s with no other
/// distinguishing signal: widening past this one fragment would also
/// swallow a duplicate create's own conflict.
///
/// **Why this is public.** The crate's own live tests
/// (`tests/live_probe.rs`, `tests/live_write_cycle.rs`) ask Doppler the
/// same question about the same endpoint -- "is this project name
/// absent?" -- and were written when a `404` was believed to be the
/// only answer to it. They are integration tests, outside the crate,
/// so sharing this one predicate is the only way they can read a
/// missing project exactly as the five tools do; the alternative was a
/// second copy of the message fragment in a test file, which is how
/// one predicate becomes two that drift.
pub fn looks_like_a_missing_project(err: &ProviderError) -> bool {
    match err.status {
        Some(404) => true,
        Some(400) => err.message.contains(DOPPLER_NO_ACCESS_MESSAGE),
        _ => false,
    }
}

/// Why [`credential_from_env`] refused to build a [`Credential`].
///
/// Never carries the environment variable's value: the `WrongKind`
/// variant is raised from [`CredentialError::Malformed`] alone, without
/// ever having read the value itself (see that function's docs for why
/// this crate does not pre-inspect the environment variable).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DopplerCredentialError {
    /// The environment variable was unset or empty. See
    /// [`CredentialError::Missing`].
    #[error(transparent)]
    Missing(CredentialError),
    /// The environment variable was set but did not match
    /// [`CREDENTIAL_PATTERN`] — either it is not shaped like any Doppler
    /// token at all, or it is a Service token (`dp.st.`), which cannot
    /// provision.
    #[error(
        "a Doppler service-account (`dp.sa.`) or personal (`dp.pt.`) token is needed to \
         provision; a service token (`dp.st.`) cannot create projects, environments, or tokens"
    )]
    WrongKind,
}

/// Read [`CREDENTIAL_VAR`] from the process environment and validate it
/// against [`CREDENTIAL_PATTERN`].
///
/// Deliberately does not pre-inspect the raw environment variable's
/// value to give a more specific message: doing so would read a
/// Doppler token's bytes into a plain `String` inside this crate, a
/// second site outside `willikins-providers-http` invisible to
/// `crates/willikins-core/tests/expose_secret_guard.rs` and exactly the
/// surface trust boundary 1 ("Two kinds of secret") confines credential
/// bytes away from. Instead, every [`CredentialError::Malformed`] the
/// shared [`Credential::from_env`] reports (which covers both "not
/// shaped like any Doppler token" and "shaped like a `dp.st.` token") is
/// mapped to the one [`DopplerCredentialError::WrongKind`] message: it
/// says which kind is needed without knowing, or repeating, what was
/// actually supplied.
///
/// # Errors
///
/// See [`Credential::from_env`], mapped through
/// [`DopplerCredentialError`].
///
/// # Panics
///
/// Never in practice: [`CREDENTIAL_PATTERN`] is a fixed, compile-time-known
/// literal, and this module's own `tests` submodule builds a `Regex` from the
/// exact same constant, so `Regex::new` on it cannot fail.
pub fn credential_from_env() -> Result<Credential, DopplerCredentialError> {
    let pattern =
        regex::Regex::new(CREDENTIAL_PATTERN).expect("CREDENTIAL_PATTERN is a valid regex");
    Credential::from_env(CREDENTIAL_VAR, &pattern).map_err(|err| match err {
        CredentialError::Missing { .. } => DopplerCredentialError::Missing(err),
        CredentialError::Malformed { .. } => DopplerCredentialError::WrongKind,
    })
}

/// Build an [`Http`] against Doppler's real API, carrying `credential`.
/// Doppler needs no headers beyond the bearer `Authorization` header
/// every [`Http`] request already carries.
#[must_use]
pub fn http_client(credential: Credential) -> Http {
    Http::new(DOPPLER_API_BASE_URL, Vec::new(), credential)
}

/// A typed Doppler REST client, bound to one [`Http`] (which itself owns
/// the [`Credential`]).
pub struct DopplerClient {
    http: Http,
}

impl DopplerClient {
    /// Build a client over `http`.
    #[must_use]
    pub fn new(http: Http) -> Self {
        Self { http }
    }

    /// `GET /v3/projects/project?project=<name>`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] for any non-2xx response (a `404`
    /// meaning "absent", handled by the caller) or a transport failure.
    pub(crate) fn get_project(
        &self,
        project: &DopplerProject,
    ) -> Result<ProjectBody, ProviderError> {
        let path = format!("/v3/projects/project?project={project}");
        self.http
            .get::<ProjectEnvelope>(&path)
            .map(|envelope| envelope.project)
    }

    /// `POST /v3/projects` with `name` and the [`MANAGED_DESCRIPTION`]
    /// marker. Never retried, by this client or by [`Http`] underneath
    /// it: an ambiguous failure here is resolved by the caller
    /// re-`read`ing.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn create_project(&self, project: &DopplerProject) -> Result<(), ProviderError> {
        let body = CreateProjectBody {
            name: project.to_string(),
            description: MANAGED_DESCRIPTION.to_string(),
        };
        self.http.post::<ProjectEnvelope>("/v3/projects", &body)?;
        Ok(())
    }

    /// `GET /v3/configs/config?project=<project>&config=<name>`, returning
    /// the one field `doppler.config.ensure` decides on: `root`.
    ///
    /// Existence alone is *not* enough to answer "is this the root config
    /// I asked for". Doppler names a branch config `<environment>_<name>`
    /// (research note section 3: `prd_aws` under environment `prd`), and
    /// [`willikins_types::naming::v1::doppler_root_config`] names a root
    /// config after its environment's snake join — so a project holding
    /// an environment `pre` with a branch config `prod` already answers
    /// `200` at the name this crate derives for the environment
    /// `pre-prod`. The plan's port table conditions `Present` on `root:
    /// true` for exactly that reason.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn get_config(
        &self,
        project: &DopplerProject,
        name: &DopplerConfigName,
    ) -> Result<ConfigBody, ProviderError> {
        let path = format!("/v3/configs/config?project={project}&config={name}");
        self.http
            .get::<ConfigEnvelope>(&path)
            .map(|envelope| envelope.config)
    }

    /// `POST /v3/environments?project=<project>` with `name` and `slug`
    /// both equal to `config_name` — the root config's own name (research
    /// note section 3, "Doppler configs": creating an environment creates
    /// its root config with the environment's own identifier). Never
    /// retried, for the same reason as [`Self::create_project`].
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn create_environment(
        &self,
        project: &DopplerProject,
        config_name: &DopplerConfigName,
    ) -> Result<(), ProviderError> {
        let path = format!("/v3/environments?project={project}");
        let body = CreateEnvironmentBody {
            name: config_name.to_string(),
            slug: config_name.to_string(),
        };
        self.http.post::<serde_json::Value>(&path, &body)?;
        Ok(())
    }

    /// `GET /v3/configs/config/tokens?project=<project>&config=<config>`.
    /// The response omits `key` and `access` for every listed token
    /// (research note section 3, "Doppler service tokens").
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn list_service_tokens(
        &self,
        project: &DopplerProject,
        config: &DopplerConfigName,
    ) -> Result<Vec<TokenListEntry>, ProviderError> {
        let path = format!("/v3/configs/config/tokens?project={project}&config={config}");
        self.http
            .get::<TokensEnvelope>(&path)
            .map(|envelope| envelope.tokens)
    }

    /// `POST /v3/configs/config/tokens` with `project`, `config`, `name`,
    /// and `access: "read"`. Never retried: minting is a `POST`, and a
    /// caller that needs to know whether a retried, ambiguous failure
    /// still minted a token re-lists instead.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn create_service_token(
        &self,
        project: &DopplerProject,
        config: &DopplerConfigName,
        name: &DopplerTokenName,
    ) -> Result<TokenCreateBody, ProviderError> {
        let body = CreateTokenBody {
            project: project.to_string(),
            config: config.to_string(),
            name: name.to_string(),
            access: "read",
        };
        self.http
            .post::<TokenCreateEnvelope>("/v3/configs/config/tokens", &body)
            .map(|envelope| envelope.token)
    }

    /// `DELETE /v3/configs/config/tokens/token` with `project`, `config`,
    /// and `slug` in the body (Doppler's revoke endpoint takes the
    /// identifying fields in the request body, not the path or query —
    /// research note section 3, "Doppler service tokens").
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn delete_service_token(
        &self,
        project: &DopplerProject,
        config: &DopplerConfigName,
        slug: &str,
    ) -> Result<(), ProviderError> {
        let body = DeleteTokenBody {
            project: project.to_string(),
            config: config.to_string(),
            slug: slug.to_string(),
        };
        self.http
            .delete_with_body("/v3/configs/config/tokens/token", &body)
    }

    /// `GET /v3/configs/config/secret?project=<project>&config=<config>&name=<name>`.
    ///
    /// `Ok(None)` is "no such secret". Doppler does **not** answer `404`
    /// for a name that does not exist, which is what the milestone plan's
    /// port table assumed: it answers `200` with `value.computed` set to
    /// `null` (observed live on 2026-09-14 by
    /// `tests/live_write_cycle.rs`; `fixtures/doppler/secret_get_absent.json`
    /// carries the shape). `computed` is therefore [`Option`], the same
    /// shape [`ProjectBody::description`] and [`ConfigBody::root`] use, so
    /// that a `null` *and* a missing key both land on the one safe answer
    /// instead of failing the whole parse.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`]. A `404` would name "no such secret" and
    /// never a value, because there is none to name; nothing observed
    /// live produces one.
    pub(crate) fn get_secret(
        &self,
        project: &DopplerProject,
        config: &DopplerConfigName,
        name: &SecretName,
    ) -> Result<Option<DopplerSecretValue>, ProviderError> {
        let path =
            format!("/v3/configs/config/secret?project={project}&config={config}&name={name}");
        self.http
            .get::<SecretBody>(&path)
            .map(|body| body.value.computed)
    }
}

/// Doppler's project envelope: `{"project": {...}}`.
#[derive(Debug, Deserialize)]
struct ProjectEnvelope {
    project: ProjectBody,
}

/// The one field of Doppler's project object this crate consults:
/// `description`, used to detect the [`MANAGED_DESCRIPTION`] ownership
/// marker. Nullable in Doppler's schema (a project created with no
/// description at all).
#[derive(Debug, Deserialize)]
pub(crate) struct ProjectBody {
    pub(crate) description: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateProjectBody {
    name: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct CreateEnvironmentBody {
    name: String,
    slug: String,
}

/// Doppler's config envelope: `{"config": {...}}`.
#[derive(Debug, Deserialize)]
struct ConfigEnvelope {
    config: ConfigBody,
}

/// The one field of Doppler's config object this crate consults: `root`,
/// which separates an environment's own root config from a branch config
/// under it. [`Option`] rather than a defaulted `bool` so that a missing
/// key *and* an explicit `null` both land on the same, safe answer
/// ("not proven to be a root config") instead of one of them failing the
/// whole parse — the same shape [`ProjectBody::description`] uses.
#[derive(Debug, Deserialize)]
pub(crate) struct ConfigBody {
    pub(crate) root: Option<bool>,
}

/// Doppler's token-list envelope: `{"tokens": [...]}`.
#[derive(Debug, Deserialize)]
struct TokensEnvelope {
    tokens: Vec<TokenListEntry>,
}

/// One listed service token: `name` (to match against) and `slug` (to
/// revoke by). The list response omits `key` and `access` entirely
/// (research note section 3).
#[derive(Debug, Deserialize)]
pub(crate) struct TokenListEntry {
    pub(crate) name: String,
    pub(crate) slug: String,
}

#[derive(Debug, Serialize)]
struct CreateTokenBody {
    project: String,
    config: String,
    name: String,
    access: &'static str,
}

/// Doppler's token-create envelope: `{"token": {...}}`.
#[derive(Debug, Deserialize)]
struct TokenCreateEnvelope {
    token: TokenCreateBody,
}

/// The one field of Doppler's create-token response this crate reads:
/// `key`, the raw token value, present only in the create response and
/// never again (research note section 3). Deserialized straight into
/// [`DopplerServiceToken`] — trust boundary 5: "the response structs
/// that hold it hold the domain type, whose `Debug` is redacted" — so a
/// key that fails [`DopplerServiceToken`]'s pattern fails here, inside
/// [`willikins_providers_http::Http::finish`]'s body-parse step, echoing
/// neither the key nor its prefix (only a line/column position, which is
/// willikins' own observation, never response text).
#[derive(Debug, Deserialize)]
pub(crate) struct TokenCreateBody {
    pub(crate) key: DopplerServiceToken,
}

#[derive(Debug, Serialize)]
struct DeleteTokenBody {
    project: String,
    config: String,
    slug: String,
}

/// Doppler's secret-get response: `{"name": ..., "value": {...}}`, no
/// envelope key. `value.computed` (references resolved) is what a
/// consumer needs — never `value.raw`.
#[derive(Debug, Deserialize)]
struct SecretBody {
    value: SecretValueBody,
}

/// [`Option`] because Doppler answers `200` with `computed: null` for a
/// secret that does not exist: see [`DopplerClient::get_secret`]. A
/// missing key and an explicit `null` both parse to `None`; a `computed`
/// that is present but unusable (an empty string, a number) still fails
/// the parse, which is a malformed response and not an absence.
#[derive(Debug, Deserialize)]
struct SecretValueBody {
    computed: Option<DopplerSecretValue>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // A distinctive marker: if it ever showed up in a rendered error, a
    // redaction rule broke. Only used below to prove `WrongKind`'s
    // message never echoes anything about the value that triggered it —
    // `credential_from_env` itself is not called here (it reads the real
    // process environment, and mutating that is `unsafe` in edition 2024,
    // which this workspace forbids outright, including in tests). The
    // `Malformed` -> `WrongKind` mapping is exercised end-to-end only by
    // `tests/live_probe.rs`, which is gated and has never run (see that
    // file's own docs) — so this module tests the two things that do not
    // need a live environment read: the regex itself, and the message.
    const MARKER: &str = "wlkn-test-marker-2rz8shp5tap";

    fn pattern() -> regex::Regex {
        regex::Regex::new(CREDENTIAL_PATTERN).expect("CREDENTIAL_PATTERN is a valid regex")
    }

    #[test]
    fn accepts_a_service_account_token() {
        let token = format!("dp.sa.{}", "a".repeat(40));
        assert!(pattern().is_match(&token), "{token}");
        let token = format!("dp.sa.{}", "a".repeat(44));
        assert!(pattern().is_match(&token), "{token}");
    }

    #[test]
    fn accepts_a_personal_token() {
        let token = format!("dp.pt.{}", "a".repeat(44));
        assert!(pattern().is_match(&token), "{token}");
    }

    #[test]
    fn rejects_a_bare_service_token() {
        let token = format!("dp.st.{}", "a".repeat(40));
        assert!(!pattern().is_match(&token), "{token}");
    }

    #[test]
    fn rejects_a_service_token_with_an_environment_segment() {
        let token = format!("dp.st.dev.{}", "a".repeat(40));
        assert!(!pattern().is_match(&token), "{token}");
    }

    #[test]
    fn rejects_a_suffix_one_short_of_the_minimum() {
        let token = format!("dp.sa.{}", "a".repeat(39));
        assert!(!pattern().is_match(&token), "{token}");
    }

    #[test]
    fn rejects_a_suffix_one_past_the_maximum() {
        let token = format!("dp.sa.{}", "a".repeat(45));
        assert!(!pattern().is_match(&token), "{token}");
    }

    #[test]
    fn rejects_a_trailing_newline() {
        let token = format!("dp.sa.{}\n", "a".repeat(40));
        assert!(!pattern().is_match(&token), "{token:?}");
    }

    #[test]
    fn wrong_kind_message_names_both_accepted_kinds_and_echoes_nothing() {
        let message = DopplerCredentialError::WrongKind.to_string();
        assert!(message.contains("service-account"), "{message}");
        assert!(message.contains("personal"), "{message}");
        assert!(!message.contains(MARKER), "{message}");
    }

    #[test]
    fn a_404_looks_like_a_missing_project() {
        let err = ProviderError::new(Some(404), "provider says: Could not find requested project");
        assert!(looks_like_a_missing_project(&err));
    }

    /// The exact body the 2026-09-20 rehearsal saw against a quiescent
    /// workplace.
    #[test]
    fn a_400_naming_no_access_looks_like_a_missing_project() {
        let err = ProviderError::new(
            Some(400),
            "provider says: This token does not have access to requested project 'harbor-relay'",
        );
        assert!(looks_like_a_missing_project(&err));
    }

    /// Where the tolerance stops: Doppler's *other* `400`, the duplicate
    /// create conflict, must never be read as "missing" — the create
    /// call, not this predicate, is the arbiter of that distinction.
    #[test]
    fn a_400_naming_already_exists_does_not_look_like_a_missing_project() {
        let err = ProviderError::new(
            Some(400),
            "provider says: Project name already exists in this workplace.",
        );
        assert!(!looks_like_a_missing_project(&err));
    }

    #[test]
    fn a_400_with_an_unrelated_message_does_not_look_like_a_missing_project() {
        let err = ProviderError::new(Some(400), "provider says: Could not find requested project");
        assert!(!looks_like_a_missing_project(&err));
    }

    #[test]
    fn a_5xx_never_looks_like_a_missing_project() {
        let err = ProviderError::new(Some(503), "provider says: Internal server error.");
        assert!(!looks_like_a_missing_project(&err));
    }

    #[test]
    fn a_transport_failure_never_looks_like_a_missing_project() {
        let err = ProviderError::new(None, "request failed: connection reset");
        assert!(!looks_like_a_missing_project(&err));
    }
}
