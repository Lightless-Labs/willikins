//! [`DopplerClient`]: typed calls for exactly the endpoints the seven
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
    DopplerConfig, DopplerConfigName, DopplerProject, DopplerProjectRole, DopplerSecretValue,
    DopplerServiceToken, DopplerTokenName, DopplerValue, EnvironmentSlug, SecretName,
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
/// (`docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`):
/// the *same* absent project name answers `404` "Could not
/// find requested project" or this `400` "This token does not have
/// access to requested project" depending on **whether the calling token
/// can see any project in the workplace at all**. None visible: `404`,
/// for every name, existing or not. One or more visible: `400`, for
/// every name this token cannot see. Same token, same endpoint, same
/// shape of name — what differs is the token's own visible project set.
///
/// Which means the `400` is the answer every provisioning run after the
/// first one gets, and the `404` only the very first.
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
/// project.ensure` node refuse to plan at all whenever this token could
/// already see one project — which is every run after the first, not a
/// corner case.
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

    /// `POST /v3/configs` with `project`, `environment`, and `name` —
    /// Doppler's dedicated *branch* config create endpoint (its own
    /// `OpenAPI` spec, `configs-create.md`, fetched verbatim 2026-09-29:
    /// request `{project, environment, name}`, response `{"config":
    /// {...}}`, the same envelope [`Self::get_config`] already parses).
    /// `name` must be the config's **already-prefixed** full name
    /// (`<environment>_<branch>`), never a bare suffix: the milestone 2
    /// live write cycle proved this empirically (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`,
    /// "Notes for milestone 3" — `name: "probe"` under environment `dev`
    /// answered `400`, while `name: "dev_probe"` was stored as
    /// `dev_probe` with `root: false`; Doppler does not prepend the
    /// environment server-side). `willikins_providers_doppler::tools::DopplerBranchConfigEnsure`
    /// assembles that prefix itself before this method ever sees the
    /// name. Never retried, for the same reason as [`Self::create_project`].
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn create_branch_config(
        &self,
        project: &DopplerProject,
        environment: &EnvironmentSlug,
        name: &DopplerConfigName,
    ) -> Result<(), ProviderError> {
        let body = CreateBranchConfigBody {
            project: project.to_string(),
            environment: environment.to_string(),
            name: name.to_string(),
        };
        self.http.post::<ConfigEnvelope>("/v3/configs", &body)?;
        Ok(())
    }

    /// `POST /v3/configs/config/inheritable` with `project`, `config`,
    /// and `inheritable`. Never retried, for the same reason as
    /// [`Self::create_project`].
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn set_config_inheritable(
        &self,
        config: &DopplerConfig,
        inheritable: bool,
    ) -> Result<(), ProviderError> {
        let body = SetInheritableBody {
            project: config.project().to_string(),
            config: config.name().to_string(),
            inheritable,
        };
        self.http
            .post::<ConfigEnvelope>("/v3/configs/config/inheritable", &body)?;
        Ok(())
    }

    /// `POST /v3/configs/config/inherits` with `project`, `config`, and
    /// `inherits` — the *whole* set `config` is to inherit, replacing
    /// whatever it inherited before (the request schema names one array,
    /// not an add/remove pair — research note `docs/research/
    /// 2026-09-12-m2-dependencies.md`, "Config Inheritance"). Never
    /// retried, for the same reason as [`Self::create_project`].
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn set_config_inherits(
        &self,
        config: &DopplerConfig,
        inherits: &[DopplerConfig],
    ) -> Result<(), ProviderError> {
        let body = SetInheritsBody {
            project: config.project().to_string(),
            config: config.name().to_string(),
            inherits: inherits
                .iter()
                .map(|parent| ConfigRefRequest {
                    project: parent.project().to_string(),
                    config: parent.name().to_string(),
                })
                .collect(),
        };
        self.http
            .post::<ConfigEnvelope>("/v3/configs/config/inherits", &body)?;
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

    /// Identical endpoint and shape to [`Self::get_secret`], deserialized
    /// into [`DopplerValue`] instead of [`DopplerSecretValue`] — the whole
    /// reason `doppler.value.get` exists (see that tool's module doc).
    /// Doppler's response carries no `secret`/`non-secret` distinction of
    /// its own; the difference is entirely which Rust type this client
    /// parses `value.computed` into, which is exactly the "choosing a
    /// tool is the author's declaration of non-secrecy" design the tool's
    /// module doc states. `DopplerValue` rather than
    /// [`willikins_types::Text`] since milestone 3i, task B8: the value
    /// most often read this way is an App Store Connect issuer id or key
    /// id, and an identifier type is what masks it on every output
    /// surface by default.
    ///
    /// # Errors
    ///
    /// See [`Self::get_secret`].
    pub(crate) fn get_value(
        &self,
        project: &DopplerProject,
        config: &DopplerConfigName,
        name: &SecretName,
    ) -> Result<Option<DopplerValue>, ProviderError> {
        let path =
            format!("/v3/configs/config/secret?project={project}&config={config}&name={name}");
        self.http
            .get::<DopplerValueBody>(&path)
            .map(|body| body.value.computed)
    }

    /// `POST /v3/configs/config/secrets` with `{"project", "config",
    /// "secrets": {"<name>": "<value>"}}` — Doppler's documented simple
    /// upsert shape (research note `docs/research/2026-09-12-m2-dependencies.md`,
    /// section "Doppler secrets": "Either `secrets` or `change_requests`
    /// is required (can't use both)"). This client always sends the flat
    /// `secrets` map, never `change_requests`: the one key it sets is
    /// merged into whatever the config already holds, not a wholesale
    /// replace of the config's other secrets. Never retried, for the same
    /// reason as [`Self::create_project`]: a caller that needs to know
    /// whether a retried, ambiguous failure still wrote re-reads (though
    /// `doppler.secret.set`'s own `read` never does — see its module
    /// docs for why).
    ///
    /// The response body (which echoes the value back, per the same
    /// endpoint's documented shape) is parsed only as an opaque
    /// [`serde_json::Value`] and immediately discarded: this client never
    /// inspects a field of it, the same way [`Self::create_environment`]
    /// discards its own response.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub(crate) fn set_secret(
        &self,
        project: &DopplerProject,
        config: &DopplerConfigName,
        name: &SecretName,
        value: &str,
    ) -> Result<(), ProviderError> {
        let mut secrets = serde_json::Map::new();
        secrets.insert(
            name.to_string(),
            serde_json::Value::String(value.to_string()),
        );
        let body = SetSecretsBody {
            project: project.to_string(),
            config: config.to_string(),
            secrets,
        };
        self.http
            .post::<serde_json::Value>("/v3/configs/config/secrets", &body)?;
        Ok(())
    }

    /// `GET /v3/configs/config/secrets/names?project=<project>&config=<config>&include_dynamic_secrets=false&include_managed_secrets=false`.
    ///
    /// Doppler's reference (`https://docs.doppler.com/reference/secrets-names.md`,
    /// fetched 2026-10-04): "List Names", "Secret Names". Query: `project`
    /// (required), `config` (required), `include_dynamic_secrets`
    /// (boolean, default `false`, "Whether or not to issue leases and
    /// include dynamic secret values for the config"),
    /// `include_managed_secrets` (boolean, default `true`, "Whether to
    /// include Doppler's auto-generated (managed) secrets"). `200`:
    /// `{"names": ["STRIPE", "ALGOLIA", "DATABASE", "USER"]}`.
    ///
    /// **Why `include_dynamic_secrets=false`, explicit rather than relied
    /// on as the default.** `true` issues leases — a side effect this
    /// read must never cause (milestone 3j, trust boundary 1: "The gate
    /// calls only `GET /v3/configs/config/secrets/names` and `GET
    /// /v3/configs/config`... It never asks Doppler to issue a
    /// dynamic-secret lease."). The default is already `false`, but
    /// pinning it in the query (and in this method's mock tests, with
    /// `match_query`) means a future refactor that drops the parameter
    /// cannot silently flip this call onto the leasing path.
    ///
    /// **Why `include_managed_secrets=false`.** Managed names are
    /// Doppler's own auto-injected `DOPPLER_*` variables, which an
    /// operator never stores there themselves; including them would
    /// answer `true` for a name no document put in the config, which is
    /// not what a caller of this method is asking. Doppler's own default
    /// for this parameter is `true` (include them), so this call departs
    /// from the default deliberately, in the other direction from
    /// `include_dynamic_secrets`.
    ///
    /// **What this returns, and what it never does.** Only whether `name`
    /// is present in the listed array, compared to `name.as_str()` byte
    /// for byte. The listed names are never parsed as [`SecretName`] (a
    /// name Doppler's own grammar allows but willikins' refuses must not
    /// fail this read), and the list itself never leaves this method: it
    /// is not returned, logged, or formatted anywhere (milestone 3j, trust
    /// boundary 2: "The list never leaves the client... No listed name
    /// other than the one asked for reaches an output, a `ToolError`, the
    /// journal, `tracing` or a panic message.").
    ///
    /// **Verify item 1, settled 2026-10-04.** Doppler's reference page
    /// does not say whether this endpoint lists a name inherited from a
    /// base config the caller's `config` inherits. The milestone 3j live
    /// names cycle (`tests/live_secret_name_gate_cycle.rs`, step 8)
    /// answered it in the sandbox workplace: it does, so a name inherited
    /// through config inheritance reads `true` here directly.
    /// `doppler.secret_name.gate` also walks `inherits` itself, which keeps
    /// it correct even if that answer ever changes; this method does not
    /// need to know either way.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`]. A malformed `2xx` body (for example
    /// `{"names": null}` or a body missing `names` entirely) is a
    /// [`ProviderError`] carrying that status and a static message, never
    /// the response text, from [`Http::get`]'s own parse-failure arm.
    /// `doppler.secret_name.gate`, the only caller, wraps that into a
    /// message naming `config` and `name` instead.
    pub(crate) fn secret_name_listed(
        &self,
        project: &DopplerProject,
        config: &DopplerConfigName,
        name: &SecretName,
    ) -> Result<bool, ProviderError> {
        let path = format!(
            "/v3/configs/config/secrets/names?project={project}&config={config}&include_dynamic_secrets=false&include_managed_secrets=false"
        );
        let body = self.http.get::<SecretNamesBody>(&path)?;
        Ok(body.names.iter().any(|listed| listed == name.as_str()))
    }

    /// `GET /v3/workplace/service_accounts?page=N&per_page=100`, paged
    /// until a page shorter than [`LIST_PER_PAGE`] is seen, collecting
    /// every listed service account's `name` and `slug` (milestone 3h
    /// task D2, `doppler.project_member.ensure`'s name-resolution step).
    ///
    /// **`pub`, not `pub(crate)`, and so for its three siblings below
    /// ([`Self::list_project_members`], [`Self::add_project_member`],
    /// [`Self::update_project_member`]).** `doppler.project_member.ensure`
    /// (the tool that will call these) is task D3, not yet written, so
    /// this task's own mock tests (`tests/project_member_client_mock.rs`)
    /// call these methods directly -- and a `tests/*.rs` target links this
    /// crate as an external dependency, which cannot see a `pub(crate)`
    /// item at all. The same reasoning, and the same visibility override,
    /// as `willikins_providers_buildkite::BuildkiteClient::delete_pipeline`:
    /// the restriction to callers inside this crate is enforced by this
    /// doc comment and by there being no second caller once D3 lands, not
    /// by visibility.
    ///
    /// Refuses past [`LIST_MAX_PAGES`] pages with a bounded
    /// [`ProviderError`] rather than looping forever against a workplace
    /// this tool was never meant to serve -- the same posture
    /// `willikins_providers_buildkite::BuildkiteClient::list_clusters_page`'s
    /// caller takes, except the loop lives in this client rather than in
    /// the tool: nothing about *which* page a caller wants varies here,
    /// unlike Buildkite's org-scoped listing, so there is only ever one
    /// sensible caller for the whole set.
    ///
    /// A `403` is remapped to a fixed message naming the missing
    /// workplace permission ([`SERVICE_ACCOUNTS_403_MESSAGE`]) rather than
    /// whatever Doppler's own body said -- trust boundary 5. Every other
    /// status or a transport failure is returned unchanged.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`], plus the two cases above.
    pub fn list_service_accounts(&self) -> Result<Vec<ServiceAccountEntry>, ProviderError> {
        let mut accounts = Vec::new();
        for page in 1..=LIST_MAX_PAGES {
            let path =
                format!("/v3/workplace/service_accounts?page={page}&per_page={LIST_PER_PAGE}");
            let envelope: ServiceAccountsEnvelope = self
                .http
                .get(&path)
                .map_err(|err| remap_403(err, SERVICE_ACCOUNTS_403_MESSAGE))?;
            let len = envelope.service_accounts.len();
            accounts.extend(envelope.service_accounts);
            if len < LIST_PER_PAGE as usize {
                return Ok(accounts);
            }
        }
        Err(ProviderError::new(
            None,
            format!(
                "this workplace has more service accounts than this client will page through \
                 (more than {} at {LIST_PER_PAGE} per page)",
                LIST_MAX_PAGES * LIST_PER_PAGE
            ),
        ))
    }

    /// `GET /v3/projects/project/members?project=<project>&page=N&per_page=100`,
    /// paged the same way as [`Self::list_service_accounts`], collecting
    /// every listed member.
    ///
    /// A `403` is remapped to a fixed message naming the missing
    /// workplace permission and the project-admin requirement
    /// ([`PROJECT_MEMBERS_403_MESSAGE`]). A missing project's `404` (or
    /// the `400` [`looks_like_a_missing_project`] already recognises) is
    /// returned unchanged -- the caller (`doppler.project_member.ensure`'s
    /// `read`) treats that as `Absent`, exactly as every other `read` in
    /// this crate does.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`], plus the `403` remapping above.
    pub fn list_project_members(
        &self,
        project: &DopplerProject,
    ) -> Result<Vec<ProjectMemberEntry>, ProviderError> {
        let mut members = Vec::new();
        for page in 1..=LIST_MAX_PAGES {
            let path = format!(
                "/v3/projects/project/members?project={project}&page={page}&per_page={LIST_PER_PAGE}"
            );
            let envelope: ProjectMembersEnvelope = self
                .http
                .get(&path)
                .map_err(|err| remap_403(err, PROJECT_MEMBERS_403_MESSAGE))?;
            let len = envelope.members.len();
            members.extend(envelope.members.into_iter().map(ProjectMemberEntry::from));
            if len < LIST_PER_PAGE as usize {
                return Ok(members);
            }
        }
        Err(ProviderError::new(
            None,
            format!(
                "project `{project}` has more members than this client will page through (more \
                 than {} at {LIST_PER_PAGE} per page)",
                LIST_MAX_PAGES * LIST_PER_PAGE
            ),
        ))
    }

    /// `POST /v3/projects/project/members?project=<project>` with body
    /// `{"type": "service_account", "slug": <slug>, "role": <role>,
    /// "environments": [...]}` -- Doppler's project-member add endpoint
    /// (`docs.doppler.com/reference/project_members-add.md`, fetched
    /// verbatim 2026-10-01). `slug` is [`DopplerSlug`], resolved by the
    /// caller from [`Self::list_service_accounts`]; this client never
    /// resolves a name itself. Never retried, for the same reason as
    /// [`Self::create_project`]: `doppler.project_member.ensure::ensure`
    /// re-reads after a failed write rather than trusting this call's own
    /// ambiguous failure.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub fn add_project_member(
        &self,
        project: &DopplerProject,
        slug: &DopplerSlug,
        role: &DopplerProjectRole,
        environments: &[EnvironmentSlug],
    ) -> Result<(), ProviderError> {
        let path = format!("/v3/projects/project/members?project={project}");
        let body = AddProjectMemberBody {
            member_type: "service_account",
            slug: slug.to_string(),
            role: role.to_string(),
            environments: environments.iter().map(ToString::to_string).collect(),
        };
        self.http.post::<serde_json::Value>(&path, &body)?;
        Ok(())
    }

    /// `PATCH /v3/projects/project/members/member/service_account/{slug}?project=<project>`
    /// with body `{"role": <role>}`, plus `"environments": [...]` only
    /// when `environments` is `Some` -- Doppler's project-member update
    /// endpoint (`docs.doppler.com/reference/project_members-update.md`,
    /// fetched verbatim 2026-10-01). `environments: None` omits the field
    /// entirely from the request body (never sends `null`), so a `PATCH`
    /// that only raises `role` cannot narrow the member's existing
    /// environment grant (decision (a): "when the member already has
    /// `access_all_environments`, omit `environments`"); the caller
    /// decides when to pass `None` versus `Some(&sorted_union)`.
    ///
    /// **Retried like every other `PATCH`** in this crate
    /// ([`Http::patch`]'s own doc: idempotent, the same reasoning
    /// `willikins-providers-appstore`'s bundle-id update uses). This body
    /// is the full desired state (`role` plus the whole environment set
    /// to grant), so a transport-level retry of an already-applied write
    /// repeats the same convergent `PATCH`, not a second distinct change
    /// -- unlike [`Self::add_project_member`]'s `POST`, which creates.
    ///
    /// # Errors
    ///
    /// See [`Self::get_project`].
    pub fn update_project_member(
        &self,
        project: &DopplerProject,
        slug: &DopplerSlug,
        role: &DopplerProjectRole,
        environments: Option<&[EnvironmentSlug]>,
    ) -> Result<(), ProviderError> {
        let path =
            format!("/v3/projects/project/members/member/service_account/{slug}?project={project}");
        let body = UpdateProjectMemberBody {
            role: role.to_string(),
            environments: environments.map(|envs| envs.iter().map(ToString::to_string).collect()),
        };
        self.http.patch::<serde_json::Value>(&path, &body)?;
        Ok(())
    }
}

/// Pages [`DopplerClient::list_service_accounts`] and
/// [`DopplerClient::list_project_members`] request at a time -- Doppler's
/// documented maximum (research note section 3, "Pagination"; the same
/// value `willikins_providers_buildkite`'s `CLUSTERS_PER_PAGE` uses for
/// its own listing). `pub` (not `pub(crate)`) for the same reason
/// [`DopplerClient::list_service_accounts`]'s own doc gives: this task's
/// mock tests, in `tests/project_member_client_mock.rs`, pin the exact
/// bound by name rather than repeating the literal.
pub const LIST_PER_PAGE: u32 = 100;

/// The greatest number of pages [`DopplerClient::list_service_accounts`]
/// and [`DopplerClient::list_project_members`] will fetch before giving
/// up: 50 pages at `LIST_PER_PAGE` is 5,000 entries, comfortably past
/// what any workplace or project `doppler.project_member.ensure` was
/// built for would hold. Past this bound each method reports a
/// [`ProviderError`] naming the bound, mirroring
/// `willikins_providers_buildkite::MAX_CLUSTER_PAGES`'s own reasoning.
/// `pub` for the same reason as [`LIST_PER_PAGE`].
pub const LIST_MAX_PAGES: u32 = 50;

/// What a `403` on [`DopplerClient::list_service_accounts`] says instead
/// of Doppler's own response body (trust boundary 5) -- the exact
/// permission `doppler.project_member.ensure`'s own `read` names
/// (milestone 3h plan, decision (a), step 2).
const SERVICE_ACCOUNTS_403_MESSAGE: &str = "the willikins Doppler service account cannot list \
     service accounts: its workplace role needs View Service Accounts (`service_accounts`)";

/// What a `403` on [`DopplerClient::list_project_members`] says instead
/// of Doppler's own response body -- the exact permission
/// `doppler.project_member.ensure`'s own `read` names (milestone 3h plan,
/// decision (a), step 3).
const PROJECT_MEMBERS_403_MESSAGE: &str = "cannot list this project's members: the workplace \
     role needs View Team (`team`) and the account must be admin of the project";

/// If `err` is a `403`, replace it with a fresh [`ProviderError`] carrying
/// `message` instead -- never Doppler's own body (trust boundary 5: "a
/// message is never built from the raw body"). Any other status, or a
/// transport failure (`status: None`), is returned unchanged: in
/// particular a `404`/`400` on `list_project_members` passes through so
/// [`looks_like_a_missing_project`] still sees it.
fn remap_403(err: ProviderError, message: &'static str) -> ProviderError {
    if err.status == Some(403) {
        ProviderError::new(Some(403), message)
    } else {
        err
    }
}

/// A Doppler internal slug -- the identifier
/// [`DopplerClient::add_project_member`] and
/// [`DopplerClient::update_project_member`] address a service account by,
/// distinct from the operator-facing [`willikins_types::DopplerServiceAccountName`]
/// a workflow names. Validated only enough to use safely in a URL path
/// segment ([`DopplerClient::update_project_member`]'s `{slug}`) or a
/// request body value this client builds itself: one or more of
/// `[A-Za-z0-9_-]`, which Doppler's own UUID-shaped slugs already satisfy
/// and which refuses `/`, `?`, `&`, and `#` the way every other domain
/// type's grammar in this crate does.
///
/// Deliberately **not** a `willikins_types` domain type registered in
/// `domain_types!`: Doppler documents no committed shape for this field
/// (its examples are UUIDs, but nothing says that is permanent), and this
/// crate never surfaces it to a document or an output --
/// `doppler.project_member.ensure`'s outputs are pass-through only
/// (SHARED VALUES), so a slug is resolved, used, and discarded entirely
/// inside this crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DopplerSlug(String);

impl TryFrom<String> for DopplerSlug {
    type Error = DopplerSlugError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let valid = !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid {
            Ok(Self(value))
        } else {
            Err(DopplerSlugError)
        }
    }
}

impl From<DopplerSlug> for String {
    fn from(value: DopplerSlug) -> String {
        value.0
    }
}

impl std::fmt::Display for DopplerSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why [`DopplerSlug::try_from`] refused a value. Carries nothing from
/// the rejected string -- only `Debug`/`Display` of the fixed message
/// below, matching every other domain type's refusal in this workspace.
/// Never surfaced past a parse failure anyway: `Http`'s own response
/// parsing builds its message from the error's line and column alone,
/// never this type's `Display` (trust boundary 5).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a valid Doppler slug: expected one or more ASCII letters, digits, `-`, or `_`")]
pub struct DopplerSlugError;

/// Doppler's service-accounts-list envelope: `{"service_accounts": [...]}`.
#[derive(Debug, Deserialize)]
struct ServiceAccountsEnvelope {
    service_accounts: Vec<ServiceAccountEntry>,
}

/// One listed workplace service account: `name` (compared against
/// [`willikins_types::DopplerServiceAccountName`] byte for byte by the
/// caller) and `slug` (used to address it in a later call). `name` is a
/// bare `String`, never validated against any willikins grammar on the
/// way in: an unrelated service account's display name may be any string
/// Doppler accepts, and a listing must not fail to parse merely because
/// one entry nobody asked for does not look like a well-formed
/// `DopplerServiceAccountName` -- unlike a document's own declared
/// values, nothing here asked for this entry to exist. `created_at` and
/// `workplace_role` are in Doppler's response but never deserialized:
/// `doppler.project_member.ensure` needs neither.
#[derive(Debug, Deserialize)]
pub struct ServiceAccountEntry {
    /// This account's display name, as an operator spelled it (or as
    /// Doppler's UI let them type it) -- compared byte for byte against
    /// a [`willikins_types::DopplerServiceAccountName`] by the caller.
    pub name: String,
    /// This account's internal slug, used to address it in a later
    /// `add_project_member`/`update_project_member` call.
    pub slug: DopplerSlug,
}

/// Doppler's project-members-list envelope: `{"members": [...]}`. Also
/// `POST`'s and `PATCH`'s own response shape
/// (`{"member": {...}}`'s plural sibling), though neither write method
/// parses its response body past discarding it as an opaque
/// [`serde_json::Value`] -- `doppler.project_member.ensure::ensure`
/// re-reads instead, exactly as [`DopplerClient::set_secret`]'s own doc
/// explains for the same shape of call.
#[derive(Debug, Deserialize)]
struct ProjectMembersEnvelope {
    members: Vec<ProjectMemberWire>,
}

/// The wire shape of one listed project member, before
/// [`ProjectMemberEntry::from`] flattens its nested `role` object.
#[derive(Debug, Deserialize)]
struct ProjectMemberWire {
    #[serde(rename = "type")]
    member_type: String,
    slug: DopplerSlug,
    role: ProjectMemberRoleBody,
    access_all_environments: bool,
    /// `null` on a member whose access spans every environment (live,
    /// 2026-10-02: every project's own creator lists that way), so absent
    /// and `null` both read as empty.
    #[serde(default, deserialize_with = "null_as_empty")]
    environments: Vec<String>,
}

/// Deserializes `null` as an empty list (Doppler's own spelling of "no
/// explicit environments" on an all-environments member).
fn null_as_empty<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

/// `{"identifier": "..."}` -- Doppler's nested role shape on a listed
/// member (`docs.doppler.com/reference/project_members-list.md`, fetched
/// verbatim 2026-10-01).
#[derive(Debug, Deserialize)]
struct ProjectMemberRoleBody {
    identifier: String,
}

/// One listed project member: enough fields for
/// `doppler.project_member.ensure`'s `read` to classify every row of the
/// milestone 3h plan's decision (a) table. `role` and `environments` are
/// bare `String`/`Vec<String>`, never [`DopplerProjectRole`] or
/// [`EnvironmentSlug`]: the table has rows for exactly the values those
/// grammars refuse by construction -- an "unrankable role" (`admin`,
/// `owner`, a custom identifier) and an environment this tool was never
/// asked about ("extra environments the member already has are never a
/// mismatch") -- so parsing a listing into either grammar would fail
/// closed on the very members `read` must still classify. `slug` is
/// [`DopplerSlug`], the one field this client later places in a URL path
/// segment, so it alone is validated on the way in.
#[derive(Debug)]
pub struct ProjectMemberEntry {
    /// Doppler's `type` for this member: `"service_account"`,
    /// `"workplace_user"`, `"group"`, or `"invite"`. The caller filters
    /// to `"service_account"`; every other value is a member this tool
    /// was never asked about.
    pub member_type: String,
    /// This member's internal slug, compared against the resolved
    /// service account slug and, on a write, placed in
    /// `update_project_member`'s URL path segment.
    pub slug: DopplerSlug,
    /// This member's project role identifier: `"viewer"`,
    /// `"collaborator"`, `"admin"`, `"owner"`, or a custom role's own
    /// identifier.
    pub role: String,
    /// Whether this member's access spans every environment, including
    /// ones added after the grant.
    pub access_all_environments: bool,
    /// The environment slugs this member was explicitly granted, as
    /// Doppler spelled them. Empty when `access_all_environments` is
    /// `true`.
    pub environments: Vec<String>,
}

impl From<ProjectMemberWire> for ProjectMemberEntry {
    fn from(wire: ProjectMemberWire) -> Self {
        Self {
            member_type: wire.member_type,
            slug: wire.slug,
            role: wire.role.identifier,
            access_all_environments: wire.access_all_environments,
            environments: wire.environments,
        }
    }
}

#[derive(Debug, Serialize)]
struct AddProjectMemberBody {
    #[serde(rename = "type")]
    member_type: &'static str,
    slug: String,
    role: String,
    environments: Vec<String>,
}

#[derive(Debug, Serialize)]
struct UpdateProjectMemberBody {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    environments: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct SetSecretsBody {
    project: String,
    config: String,
    secrets: serde_json::Map<String, serde_json::Value>,
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

/// [`DopplerClient::create_branch_config`]'s request body.
#[derive(Debug, Serialize)]
struct CreateBranchConfigBody {
    project: String,
    environment: String,
    name: String,
}

/// Doppler's config envelope: `{"config": {...}}`.
#[derive(Debug, Deserialize)]
struct ConfigEnvelope {
    config: ConfigBody,
}

/// The fields of Doppler's config object this crate consults: `root`,
/// which separates an environment's own root config from a branch config
/// under it, and — since `doppler.config.inheritable.ensure` and
/// `doppler.config.inherits.ensure` — `inheritable` and `inherits`. All
/// three are [`Option`] rather than defaulted, so a missing key *and* an
/// explicit `null` both land on the same, safe answer ("not proven")
/// instead of one of them failing the whole parse — the same shape
/// [`ProjectBody::description`] uses. `inheritedBy` and `inheriting` are
/// deliberately left out: no tool reads them (research note section
/// "Config Inheritance": both answer bodies carry all four, but only
/// `inheritable` and `inherits` name what a config was *asked* to be,
/// which is the half `ensure` compares against). `environment` was added
/// for `doppler.branch_config.ensure`: the same name-collision risk
/// `doppler.config.ensure`'s own `root` check guards against (a config
/// sitting at the right string but the wrong logical place) applies to a
/// literal branch name too, so that tool's `Present` also requires this
/// field to equal the environment it was asked to ensure under.
#[derive(Debug, Deserialize)]
pub(crate) struct ConfigBody {
    pub(crate) root: Option<bool>,
    pub(crate) environment: Option<String>,
    pub(crate) inheritable: Option<bool>,
    pub(crate) inherits: Option<Vec<ConfigRefBody>>,
}

/// One entry of a config object's `inherits` array: the project and
/// config *names* of a base config this config inherits from (research
/// note section "Config Inheritance": "The `project` values in these
/// bodies are project names"). Deserialized straight into
/// [`DopplerProject`]/[`DopplerConfigName`], so an entry naming something
/// outside either grammar fails the parse rather than reading as a
/// plausible-looking config `doppler.config.inherits.ensure` never asked
/// for.
#[derive(Debug, Deserialize)]
pub(crate) struct ConfigRefBody {
    pub(crate) project: DopplerProject,
    pub(crate) config: DopplerConfigName,
}

#[derive(Debug, Serialize)]
struct SetInheritableBody {
    project: String,
    config: String,
    inheritable: bool,
}

/// One entry of a `POST /v3/configs/config/inherits` request's `inherits`
/// array — the request-side twin of [`ConfigRefBody`], built from an
/// already-validated [`DopplerConfig`] rather than parsed from one.
#[derive(Debug, Serialize)]
struct ConfigRefRequest {
    project: String,
    config: String,
}

#[derive(Debug, Serialize)]
struct SetInheritsBody {
    project: String,
    config: String,
    inherits: Vec<ConfigRefRequest>,
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

/// Doppler's secret-names-list response: `{"names": [...]}`, no envelope
/// key (milestone 3j, decision (b2)). [`DopplerClient::secret_name_listed`]
/// is the only reader.
///
/// **Deliberately no `Debug`, unlike every other response struct in this
/// file.** Every field here is a name the operator chose for *some*
/// secret in the config, not only the one [`DopplerClient::secret_name_listed`]'s
/// caller asked about — milestone 3j's trust boundary 2 says that other
/// names are never willikins' to print ("The other names in a config are
/// part of the operator's layout, and the gate has no reason to print
/// them"). A derived `Debug` would make every one of them reachable
/// through a panic message, an `assert_eq!` failure, or any future
/// `{:?}` of a value that happens to hold one — exactly the leak this
/// struct exists to make a compile error instead of a code-review
/// finding. `secret_name_listed` reduces this struct to the one `bool`
/// its caller actually gets, so there is never a need to format it.
#[derive(Deserialize)]
struct SecretNamesBody {
    names: Vec<String>,
}

/// The same envelope [`SecretBody`] parses, deserialized into
/// [`DopplerValue`] instead: [`DopplerClient::get_value`]'s response
/// shape.
#[derive(Debug, Deserialize)]
struct DopplerValueBody {
    value: DopplerValueValueBody,
}

/// See [`SecretValueBody`] -- identical reasoning, non-secret payload.
#[derive(Debug, Deserialize)]
struct DopplerValueValueBody {
    computed: Option<DopplerValue>,
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

    /// The exact body the 2026-09-20 rehearsal saw, planning a second
    /// project in a workplace whose first one this token could already
    /// see.
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

    // -----------------------------------------------------------------
    // secret_name_listed (milestone 3j task B1)
    // -----------------------------------------------------------------

    use willikins_providers_http::testing::{MockProvider, load_fixture};
    use willikins_types::DomainType;

    /// A listed name that must never surface in any `Err`'s message or
    /// `Debug` output -- trust boundary 2: the list never leaves this
    /// client, so even a name this crate itself did not ask about must
    /// not become visible through a failure path.
    const WILLIKINS_LEAK_MARKER_NAME: &str = "WILLIKINS_LEAK_MARKER_NAME";

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    fn client_against(url: String) -> DopplerClient {
        let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
        DopplerClient::new(Http::new(url, Vec::new(), credential))
    }

    /// The SHARED VALUES placeholder base project, underscored
    /// (milestone 3j part A: `DopplerProject` now admits one). Doubles as
    /// a check that the underscore survives into this endpoint's query
    /// string unchanged.
    fn project() -> DopplerProject {
        DopplerProject::parse("shared_keys").unwrap()
    }

    fn config() -> DopplerConfigName {
        DopplerConfigName::parse("prd").unwrap()
    }

    fn name() -> SecretName {
        SecretName::parse("EXAMPLE_APNS_KEY").unwrap()
    }

    /// The exact query [`DopplerClient::secret_name_listed`] must send,
    /// pinned field by field so a mock that dropped one -- in particular
    /// `include_dynamic_secrets=false`, which guards against ever issuing
    /// a dynamic-secret lease -- would fail to match rather than quietly
    /// pass. `AllOf`, matching this crate's other client-method mock
    /// tests (`project_member_client_mock.rs`), rather than `Exact`: no
    /// other test in this crate pins a literal query string, and `AllOf`
    /// already catches the one case this method's own trust boundary
    /// cares about -- a dropped or flipped pair -- without this test
    /// becoming the first to depend on `mockito`'s exact query-string
    /// encoding and ordering.
    fn names_query() -> mockito::Matcher {
        mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("project".into(), "shared_keys".into()),
            mockito::Matcher::UrlEncoded("config".into(), "prd".into()),
            mockito::Matcher::UrlEncoded("include_dynamic_secrets".into(), "false".into()),
            mockito::Matcher::UrlEncoded("include_managed_secrets".into(), "false".into()),
        ])
    }

    // A `static_assertions`-style negative (trust boundary 2, SHARED
    // VALUES' "Deliberately no `Debug`"): this compiles only while
    // `SecretNamesBody` implements no `Debug`. If one is ever derived or
    // hand-written, both blanket impls below apply to it and resolving
    // `some_item` through the ambiguous trait becomes an ambiguity error
    // instead of a single candidate -- the same pattern
    // `willikins_providers_buildkite::client`'s own
    // `pipeline_bootstrap_body_has_no_debug` pins for
    // `PipelineBootstrapBody`. Nothing else in this crate's test suite
    // would catch a `Debug` derive landing here: acceptance 6's source
    // guard (`tests/secret_name_gate_mock.rs`) greps only
    // `secret_name_gate.rs`, and no test ever formats a `SecretNamesBody`
    // with `{:?}`, so a stray derive would otherwise be silent until
    // some future code actually used it to print a config's full name
    // list.
    #[test]
    fn secret_names_body_has_no_debug() {
        trait AmbiguousIfDebug<A> {
            fn some_item() {}
        }
        impl<T: ?Sized> AmbiguousIfDebug<()> for T {}
        #[allow(dead_code)]
        struct IsDebug;
        impl<T: ?Sized + std::fmt::Debug> AmbiguousIfDebug<IsDebug> for T {}
        let _ = <SecretNamesBody as AmbiguousIfDebug<_>>::some_item;
    }

    #[test]
    fn listed_among_several_names_reads_true() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(200)
            .with_body(serde_json::json!({"names": ["A", "EXAMPLE_APNS_KEY"]}).to_string())
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let listed = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap();
        assert!(listed);
        mock.assert();
    }

    #[test]
    fn an_empty_list_reads_false() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(200)
            .with_body(serde_json::json!({"names": []}).to_string())
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let listed = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap();
        assert!(!listed);
        mock.assert();
    }

    /// No prefix, suffix, or case match counts: a near-miss name must not
    /// be read as the one asked for.
    #[test]
    fn a_prefix_suffix_or_case_near_miss_never_counts_as_listed() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(200)
            .with_body(
                serde_json::json!({
                    "names": ["EXAMPLE_APNS_KEY_OLD", "XEXAMPLE_APNS_KEY", "example_apns_key"]
                })
                .to_string(),
            )
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let listed = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap();
        assert!(!listed);
        mock.assert();
    }

    #[test]
    fn a_null_names_array_is_a_malformed_2xx_error() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(200)
            .with_body(serde_json::json!({"names": null}).to_string())
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let err = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap_err();
        assert_eq!(err.status, Some(200));
        mock.assert();
    }

    #[test]
    fn a_missing_names_field_is_a_malformed_2xx_error() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(200)
            .with_body("{}")
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let err = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap_err();
        assert_eq!(err.status, Some(200));
        mock.assert();
    }

    /// The genuine leak path for a `2xx`: a body that *does* list the
    /// marker but fails to parse as [`SecretNamesBody`] anyway (a `names`
    /// entry that is not a string). [`Http::finish`]'s malformed-body arm
    /// reports only a line/column position, so the marker -- despite
    /// being right there in the body -- must not reach the error.
    #[test]
    fn a_listed_marker_in_a_malformed_2xx_body_never_reaches_the_error() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(200)
            .with_body(serde_json::json!({"names": [WILLIKINS_LEAK_MARKER_NAME, 7]}).to_string())
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let err = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap_err();
        assert_eq!(err.status, Some(200));
        assert!(!err.message.contains(WILLIKINS_LEAK_MARKER_NAME), "{err:?}");
        assert!(
            !format!("{err:?}").contains(WILLIKINS_LEAK_MARKER_NAME),
            "{err:?}"
        );
        mock.assert();
    }

    #[test]
    fn a_404_is_an_err_carrying_that_status() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(404)
            .with_body(
                serde_json::json!({
                    "messages": ["Could not find requested project"],
                    "names": [WILLIKINS_LEAK_MARKER_NAME],
                })
                .to_string(),
            )
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let err = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap_err();
        assert_eq!(err.status, Some(404));
        assert!(!err.message.contains(WILLIKINS_LEAK_MARKER_NAME), "{err:?}");
        assert!(
            !format!("{err:?}").contains(WILLIKINS_LEAK_MARKER_NAME),
            "{err:?}"
        );
        // This endpoint's `404` meets (b3)'s shared predicate exactly like
        // every other read in this crate.
        assert!(looks_like_a_missing_project(&err));
        mock.assert();
    }

    #[test]
    fn a_400_no_access_is_an_err_carrying_that_status() {
        let mut provider = MockProvider::start();
        let mut body = load_fixture(&fixtures_dir(), "doppler", "error_400_no_access");
        body["names"] = serde_json::json!([WILLIKINS_LEAK_MARKER_NAME]);
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(400)
            .with_body(body.to_string())
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let err = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap_err();
        assert_eq!(err.status, Some(400));
        assert!(!err.message.contains(WILLIKINS_LEAK_MARKER_NAME), "{err:?}");
        assert!(
            !format!("{err:?}").contains(WILLIKINS_LEAK_MARKER_NAME),
            "{err:?}"
        );
        assert!(looks_like_a_missing_project(&err));
        mock.assert();
    }

    #[test]
    fn a_401_is_an_err_carrying_that_status_and_never_the_body() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(401)
            .with_body(
                serde_json::json!({
                    "messages": ["Unauthorized"],
                    "names": [WILLIKINS_LEAK_MARKER_NAME],
                })
                .to_string(),
            )
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let err = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap_err();
        assert_eq!(err.status, Some(401));
        assert!(!err.message.contains(WILLIKINS_LEAK_MARKER_NAME), "{err:?}");
        assert!(
            !format!("{err:?}").contains(WILLIKINS_LEAK_MARKER_NAME),
            "{err:?}"
        );
        mock.assert();
    }

    #[test]
    fn a_403_is_an_err_carrying_that_status_and_never_the_body() {
        let mut provider = MockProvider::start();
        let mock = provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(403)
            .with_body(
                serde_json::json!({
                    "messages": ["Forbidden"],
                    "names": [WILLIKINS_LEAK_MARKER_NAME],
                })
                .to_string(),
            )
            .expect(1)
            .create();
        let client = client_against(provider.url());
        let err = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap_err();
        assert_eq!(err.status, Some(403));
        assert!(!err.message.contains(WILLIKINS_LEAK_MARKER_NAME), "{err:?}");
        assert!(
            !format!("{err:?}").contains(WILLIKINS_LEAK_MARKER_NAME),
            "{err:?}"
        );
        mock.assert();
    }

    /// A `500` is retryable in [`Http`] itself (up to three extra
    /// attempts with real backoff), so -- unlike the statuses above --
    /// this test does not pin an exact call count: the point is the
    /// final status and the leak check, not how many times the mock was
    /// hit along the way, exactly as this crate's other GET-vs-5xx tests
    /// (for example `branch_config_ensure_mock.rs`'s
    /// `read_maps_a_5xx_to_a_bounded_provider_error`) already choose not
    /// to.
    #[test]
    fn a_500_is_an_err_carrying_that_status() {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/v3/configs/config/secrets/names")
            .match_query(names_query())
            .with_status(500)
            .with_body(
                serde_json::json!({
                    "messages": ["Internal server error"],
                    "names": [WILLIKINS_LEAK_MARKER_NAME],
                })
                .to_string(),
            )
            .create();
        let client = client_against(provider.url());
        let err = client
            .secret_name_listed(&project(), &config(), &name())
            .unwrap_err();
        assert_eq!(err.status, Some(500));
        assert!(!err.message.contains(WILLIKINS_LEAK_MARKER_NAME), "{err:?}");
        assert!(
            !format!("{err:?}").contains(WILLIKINS_LEAK_MARKER_NAME),
            "{err:?}"
        );
    }
}
