//! [`Butler::live_catalog`] and [`Butler::fake_catalog`]: the two ways a
//! caller builds the [`willikins_core::Catalog`] a [`crate::ButlerConfig`]
//! needs.
//!
//! `live_catalog` assembles the twenty-three-tool live catalog --
//! `willikins-tools`' seven pure tools (`naming.v1`, `template.render`,
//! `env.get`, `base64.decode`, `apple.signing_key.parse`,
//! `apple.issuer_id.parse`, `apple.key_id.parse`),
//! `willikins-providers-github`'s two
//! live tools, `willikins-providers-doppler`'s nine (milestone 3 added
//! `doppler.config.inheritable.ensure` and
//! `doppler.config.inherits.ensure`; the `SigNoz` task added
//! `doppler.secret.set`; the App Store Connect credential correction
//! added `doppler.value.get`), `willikins-providers-buildkite`'s two
//! (milestone 3a), `willikins-providers-signoz`'s one (the `SigNoz`
//! task), and `willikins-providers-appstore`'s two (the App Store
//! Connect provider crate) -- exactly as
//! `crates/willikins-providers-doppler/tests/live_catalog.rs` built it
//! before this task; that test now calls [`live_catalog_with`] (this
//! module's own assembly, taking `Http`s rather than `Credential`s so a
//! test can point them at a mock server or nowhere) instead of keeping a
//! second copy, so the assembly exists exactly once.

use std::sync::Arc;

use willikins_core::{Catalog, Tool, ToolName, Workflow};
use willikins_providers_appstore::{AppstoreBundleIdCapabilityEnsure, AppstoreBundleIdEnsure};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkiteClusterGet, BuildkitePipelineEnsure,
};
use willikins_providers_doppler::{
    DopplerClient, DopplerConfigEnsure, DopplerConfigInheritableEnsure,
    DopplerConfigInheritsEnsure, DopplerProjectEnsure, DopplerSecretGet, DopplerSecretSet,
    DopplerServiceTokenEnsure, DopplerServiceTokenRotate, DopplerValueGet,
};
use willikins_providers_github::{GitHubActionsSecretEnsure, GitHubClient, GitHubRepoEnsure};
use willikins_providers_http::{Credential, Http};
use willikins_providers_signoz::{SigNozClient, SigNozIngestionKeyEnsure};

/// Every tool name [`live_catalog_with`] (and so [`Butler::live_catalog`])
/// inserts, in insertion order -- pinned by
/// `tests::the_live_catalog_has_exactly_these_tools_and_no_fake_tool_fits`.
pub const LIVE_TOOL_NAMES: [&str; 23] = [
    "naming.v1",
    "template.render",
    "env.get",
    "base64.decode",
    "apple.signing_key.parse",
    "apple.issuer_id.parse",
    "apple.key_id.parse",
    "github.repo.ensure",
    "github.actions_secret.ensure",
    "doppler.project.ensure",
    "doppler.config.ensure",
    "doppler.config.inheritable.ensure",
    "doppler.config.inherits.ensure",
    "doppler.service_token.ensure",
    "doppler.service_token.rotate",
    "doppler.secret.get",
    "doppler.secret.set",
    "doppler.value.get",
    "signoz.ingestion_key.ensure",
    "buildkite.pipeline.ensure",
    "buildkite.cluster.get",
    "appstore.bundle_id.ensure",
    "appstore.bundle_id_capability.ensure",
];

/// Insert `willikins-tools`' seven pure tools -- no provider, no
/// credential, always present regardless of which providers a document
/// uses. `env.get`, `base64.decode`, and `apple.signing_key.parse` joined
/// `naming.v1` and `template.render` here once the App Store Connect
/// credential correction gave a resolver chain (`env.get` or
/// `doppler.secret.get`, optionally through `base64.decode`, ending at
/// `apple.signing_key.parse`) real documents to run in; `apple.issuer_id.parse`
/// and `apple.key_id.parse` joined once `willikins-providers-appstore`
/// gave a `Text`-emitting resolver (`doppler.value.get`) a typed port to
/// reach.
fn insert_pure_tools(catalog: &mut Catalog) {
    insert(catalog, Arc::new(willikins_tools::NamingV1::new()));
    insert(catalog, Arc::new(willikins_tools::TemplateRender::new()));
    insert(catalog, Arc::new(willikins_tools::EnvGet::new()));
    insert(catalog, Arc::new(willikins_tools::Base64Decode::new()));
    insert(
        catalog,
        Arc::new(willikins_tools::AppleSigningKeyParse::new()),
    );
    insert(
        catalog,
        Arc::new(willikins_tools::AppleIssuerIdParse::new()),
    );
    insert(catalog, Arc::new(willikins_tools::AppleKeyIdParse::new()));
}

/// Insert `willikins-providers-appstore`'s two live tools. Unlike every
/// other `insert_*_tools` function in this module, this one takes no
/// `Http` and no credential at all: the App Store Connect credential's
/// three parts are ordinary graph ports, resolved per-call from a
/// document's own inputs, never read from the process environment by
/// this crate (`willikins_providers_appstore`'s own module doc). So
/// these two tools are inserted unconditionally, the same as
/// [`insert_pure_tools`], in both [`live_catalog_with`] and
/// [`live_catalog_for_document`] -- there is no environment credential
/// to gate them behind, and `tests::the_provider_tool_name_arrays_partition_live_tool_names`
/// tracks them alongside the five pure tool names for exactly that
/// reason.
fn insert_appstore_tools(catalog: &mut Catalog) {
    insert(
        catalog,
        Arc::new(AppstoreBundleIdEnsure::new(
            willikins_providers_appstore::APPSTORE_API_BASE_URL,
        )),
    );
    insert(
        catalog,
        Arc::new(AppstoreBundleIdCapabilityEnsure::new(
            willikins_providers_appstore::APPSTORE_API_BASE_URL,
        )),
    );
}

/// Insert `willikins-providers-github`'s two live tools, built from
/// `http`.
fn insert_github_tools(catalog: &mut Catalog, http: Http) {
    let github = Arc::new(GitHubClient::new(http));
    insert(
        catalog,
        Arc::new(GitHubRepoEnsure::new(Arc::clone(&github))),
    );
    insert(catalog, Arc::new(GitHubActionsSecretEnsure::new(github)));
}

/// Insert `willikins-providers-doppler`'s nine live tools, built from
/// `http`.
fn insert_doppler_tools(catalog: &mut Catalog, http: Http) {
    let doppler = Arc::new(DopplerClient::new(http));
    insert(
        catalog,
        Arc::new(DopplerProjectEnsure::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerConfigEnsure::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerConfigInheritableEnsure::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerConfigInheritsEnsure::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerServiceTokenEnsure::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerServiceTokenRotate::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerSecretGet::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerSecretSet::new(Arc::clone(&doppler))),
    );
    insert(catalog, Arc::new(DopplerValueGet::new(doppler)));
}

/// Insert `willikins-providers-signoz`'s one live tool, built from
/// `http`.
fn insert_signoz_tools(catalog: &mut Catalog, http: Http) {
    let signoz = Arc::new(SigNozClient::new(http));
    insert(catalog, Arc::new(SigNozIngestionKeyEnsure::new(signoz)));
}

/// Insert `willikins-providers-buildkite`'s two live tools, built from
/// `http`.
fn insert_buildkite_tools(catalog: &mut Catalog, http: Http) {
    let buildkite = Arc::new(BuildkiteClient::new(http));
    insert(
        catalog,
        Arc::new(BuildkitePipelineEnsure::new(Arc::clone(&buildkite))),
    );
    insert(catalog, Arc::new(BuildkiteClusterGet::new(buildkite)));
}

/// Insert `tool` into `catalog`, panicking (the live catalog's own tool
/// list is fixed at compile time, so a rejection here is a programming
/// error, never a runtime condition a caller can hit) if the catalog's
/// type registry rejects its spec.
fn insert(catalog: &mut Catalog, tool: Arc<dyn Tool>) {
    let name = tool.spec().name.clone();
    catalog
        .insert(tool)
        .unwrap_or_else(|err| unreachable!("live catalog rejected `{name}`: {err}"));
}

/// Assemble the live catalog from an already-built `Http` for each
/// provider. The lower-level half of [`Butler::live_catalog`]: split out
/// so a test (this crate's own, and
/// `willikins-providers-doppler/tests/live_catalog.rs`) can supply an
/// `Http` pointed at a mock server or nowhere without duplicating the
/// tool list, while production code goes through
/// [`Butler::live_catalog`], which builds three of these four `Http`s
/// from `Credential`s against each provider's real base URL and default
/// headers (`willikins_providers_github::http_client`,
/// `willikins_providers_doppler::http_client`,
/// `willikins_providers_buildkite::http_client`) -- the fourth, `SigNoz`'s,
/// it takes pre-built, since that provider needs a tenant host as well
/// as a credential (see [`live_catalog`]'s own doc comment).
///
/// Always inserts all 15 tools, unconditionally requiring all four
/// `Http`s -- this is the "many documents, refuse up front" shape a
/// long-lived server (and `willikins apply --plan-id`, which resolves a
/// plan against an arbitrary document in a whole trusted directory) needs.
/// A single document's `plan`/`apply` instead goes through
/// [`live_catalog_for_document`], which only requires the providers that
/// document's own tools actually use.
#[must_use]
pub fn live_catalog_with(
    github_http: Http,
    doppler_http: Http,
    buildkite_http: Http,
    signoz_http: Http,
) -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    insert_pure_tools(&mut catalog);
    insert_appstore_tools(&mut catalog);
    insert_github_tools(&mut catalog, github_http);
    insert_doppler_tools(&mut catalog, doppler_http);
    insert_buildkite_tools(&mut catalog, buildkite_http);
    insert_signoz_tools(&mut catalog, signoz_http);
    catalog
}

/// The live catalog, against each provider's real base URL, built from
/// `github`, `doppler`, and `buildkite` credentials, plus an
/// already-built `SigNoz` `Http` (`signoz_http`) — unlike the other three,
/// `SigNoz` needs both a credential *and* a tenant host
/// ([`willikins_providers_signoz::HOST_VAR`]) to build one, so its
/// caller ([`live_catalog_from_env`]) builds it up front rather than
/// this function taking a bare `Credential` it could not turn into an
/// `Http` alone. See [`live_catalog_with`].
#[must_use]
pub fn live_catalog(
    github: Credential,
    doppler: Credential,
    buildkite: Credential,
    signoz_http: Http,
) -> Catalog {
    live_catalog_with(
        willikins_providers_github::http_client(github),
        willikins_providers_doppler::http_client(doppler),
        willikins_providers_buildkite::http_client(buildkite),
        signoz_http,
    )
}

/// Why [`live_catalog_from_env`] could not build the live catalog: one of
/// the two provisioning credentials (`WILLIKINS_GITHUB_TOKEN`,
/// `WILLIKINS_DOPPLER_TOKEN`) was missing or did not look like a valid
/// token for its provider. Never carries the credential's value -- each
/// variant is built from the provider crate's own already-redacted
/// `Display`, exactly like `crate::cli`'s `StartError::Credential` (which
/// this function's own logic used to duplicate before task 11 gave it a
/// second caller, the CLI's `--live` flag, and this became the one shared
/// path instead of two).
///
/// Kind-tagged (`{"kind": "GitHub" | "Doppler", "error": ...}`, plus the
/// `message` added by
/// [`willikins_core::Reported`]), the same convention every other error in
/// this workspace follows, so a caller prints it exactly like a
/// `ButlerError`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum LiveCredentialError {
    /// `WILLIKINS_GITHUB_TOKEN` is missing or malformed.
    GitHub {
        /// The provider crate's own message. Names the variable only.
        /// Named `error`, not `message`: `willikins-cli` prints this
        /// through [`Reported`](willikins_core::Reported), which adds a
        /// `message` of its own, and a field of that name here would
        /// make the two collide into one duplicated JSON key.
        error: String,
    },
    /// `WILLIKINS_DOPPLER_TOKEN` is missing or malformed.
    Doppler {
        /// The provider crate's own message. Never the value -- but,
        /// unlike [`Self::GitHub`], not always the variable either:
        /// `DopplerCredentialError::WrongKind` (a `dp.st.` service
        /// token, or anything else the provisioning pattern rejects)
        /// describes the token *kinds* that can provision and names no
        /// variable at all. What identifies it is this enum's own
        /// `kind` tag, and -- through
        /// [`DocumentCredentialError`] -- the document and tool that
        /// needed it. See [`Self::GitHub`] for why this is not called
        /// `message`.
        error: String,
    },
    /// `WILLIKINS_BUILDKITE_TOKEN` is missing or malformed.
    Buildkite {
        /// The provider crate's own message. Never the value, and --
        /// like [`Self::Doppler`], whose note says why -- not always
        /// the variable either. See [`Self::GitHub`] for why this is
        /// not called `message`.
        error: String,
    },
    /// `WILLIKINS_SIGNOZ_API_KEY` is missing or malformed, *or*
    /// `WILLIKINS_SIGNOZ_HOST` is unset -- `SigNoz` is the one provider
    /// this crate needs two environment variables to reach, not one, so
    /// this variant covers both failures rather than gaining a fourth
    /// enum case for the host alone: either way the operator is missing
    /// something this provider needs before it can be reached at all,
    /// and `kind: "SigNoz"` already tells them which provider. See
    /// [`Self::GitHub`] for why this is not called `message`.
    SigNoz {
        /// The provider crate's own message -- from
        /// `willikins_providers_signoz::SigNozCredentialError` or
        /// `willikins_providers_signoz::SigNozHostError`, whichever
        /// failed first. Never the credential's value.
        error: String,
    },
}

impl std::fmt::Display for LiveCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitHub { error }
            | Self::Doppler { error }
            | Self::Buildkite { error }
            | Self::SigNoz { error } => {
                write!(f, "{error}")
            }
        }
    }
}

impl std::error::Error for LiveCredentialError {}

/// Build the live catalog from the process environment: each provider
/// crate's own `credential_from_env` (`WILLIKINS_GITHUB_TOKEN`,
/// `WILLIKINS_DOPPLER_TOKEN`, `WILLIKINS_BUILDKITE_TOKEN`), then
/// [`live_catalog`]. `crate::cli::run_serve`'s `--live`-equivalent (the
/// non-`--fake` default) goes through this, and so does the `willikins`
/// binary's own `apply --plan-id --live` (which, like the server, resolves
/// against an arbitrary document in a whole trusted `--workflows-dir`, not
/// one document it can inspect up front) -- every caller that must serve,
/// or might need to serve, more than the one document it was just handed.
/// A `plan <file> --live` / `apply <file> --live` invocation, which does
/// have exactly one document up front, goes through
/// [`live_catalog_for_document`] instead (the operator's own wart report:
/// running a Doppler-only document used to demand
/// `WILLIKINS_GITHUB_TOKEN` and `WILLIKINS_BUILDKITE_TOKEN` too).
///
/// # Errors
///
/// [`LiveCredentialError`] naming whichever credential was missing or
/// malformed, checking `WILLIKINS_GITHUB_TOKEN` first, then
/// `WILLIKINS_DOPPLER_TOKEN`, then `WILLIKINS_BUILDKITE_TOKEN`. No network
/// call is made either way.
pub fn live_catalog_from_env() -> Result<Catalog, LiveCredentialError> {
    let github = willikins_providers_github::credential_from_env().map_err(|error| {
        LiveCredentialError::GitHub {
            error: error.to_string(),
        }
    })?;
    let doppler = willikins_providers_doppler::credential_from_env().map_err(|error| {
        LiveCredentialError::Doppler {
            error: error.to_string(),
        }
    })?;
    let buildkite = willikins_providers_buildkite::credential_from_env().map_err(|error| {
        LiveCredentialError::Buildkite {
            error: error.to_string(),
        }
    })?;
    let signoz_http = signoz_http_from_env()?;
    Ok(live_catalog(github, doppler, buildkite, signoz_http))
}

/// Build a `SigNoz` [`Http`] from [`willikins_providers_signoz::CREDENTIAL_VAR`]
/// and [`willikins_providers_signoz::HOST_VAR`], both read from the
/// process environment -- the one step [`live_catalog_from_env`] and
/// [`live_catalog_for_document`] share, since `SigNoz` needs both a
/// credential and a host to reach at all (see [`live_catalog`]'s own doc
/// comment for why that is not folded into a bare `Credential` parameter
/// the way the other three providers' are).
///
/// # Errors
///
/// [`LiveCredentialError::SigNoz`] naming whichever failed first: the
/// credential, then the host.
fn signoz_http_from_env() -> Result<Http, LiveCredentialError> {
    let credential = willikins_providers_signoz::credential_from_env().map_err(|error| {
        LiveCredentialError::SigNoz {
            error: error.to_string(),
        }
    })?;
    let base_url = willikins_providers_signoz::base_url_from_env().map_err(|error| {
        LiveCredentialError::SigNoz {
            error: error.to_string(),
        }
    })?;
    Ok(willikins_providers_signoz::http_client(
        base_url, credential,
    ))
}

// ---------------------------------------------------------------------
// Per-document live catalog: only the providers the document uses.
// ---------------------------------------------------------------------

/// Which of the three live providers a tool belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    GitHub,
    Doppler,
    Buildkite,
    SigNoz,
}

/// `willikins-providers-github`'s live tool names. Kept as its own array
/// (rather than slicing [`LIVE_TOOL_NAMES`]) so it names exactly the tools
/// [`insert_github_tools`] inserts; `tests::the_provider_tool_name_arrays_partition_live_tool_names`
/// pins that this array, [`DOPPLER_TOOL_NAMES`], [`BUILDKITE_TOOL_NAMES`],
/// and the five pure tool names together are exactly [`LIVE_TOOL_NAMES`],
/// so the two lists cannot silently drift apart.
const GITHUB_TOOL_NAMES: [&str; 2] = ["github.repo.ensure", "github.actions_secret.ensure"];

/// `willikins-providers-doppler`'s live tool names. See
/// [`GITHUB_TOOL_NAMES`].
const DOPPLER_TOOL_NAMES: [&str; 9] = [
    "doppler.project.ensure",
    "doppler.config.ensure",
    "doppler.config.inheritable.ensure",
    "doppler.config.inherits.ensure",
    "doppler.service_token.ensure",
    "doppler.service_token.rotate",
    "doppler.secret.get",
    "doppler.secret.set",
    "doppler.value.get",
];

/// `willikins-providers-buildkite`'s live tool names. See
/// [`GITHUB_TOOL_NAMES`].
const BUILDKITE_TOOL_NAMES: [&str; 2] = ["buildkite.pipeline.ensure", "buildkite.cluster.get"];

/// `willikins-providers-signoz`'s live tool names. See
/// [`GITHUB_TOOL_NAMES`]. `doppler.secret.set` needs the *Doppler*
/// credential (it is a Doppler API call), not `SigNoz`'s, so it lives in
/// [`DOPPLER_TOOL_NAMES`] above, not here, even though it exists to
/// receive what this provider mints.
const SIGNOZ_TOOL_NAMES: [&str; 1] = ["signoz.ingestion_key.ensure"];

/// Which provider `tool` belongs to, or `None` for a pure tool
/// (`naming.v1`, `template.render`) or a name no live tool carries at all
/// (an unknown-tool document [`crate::catalog::live_catalog_for_document`]'s
/// caller will still `check` against the catalog this returns, which
/// reports it the same way it always has).
fn provider_of(tool: &ToolName) -> Option<Provider> {
    let name = tool.as_str();
    if GITHUB_TOOL_NAMES.contains(&name) {
        Some(Provider::GitHub)
    } else if DOPPLER_TOOL_NAMES.contains(&name) {
        Some(Provider::Doppler)
    } else if BUILDKITE_TOOL_NAMES.contains(&name) {
        Some(Provider::Buildkite)
    } else if SIGNOZ_TOOL_NAMES.contains(&name) {
        Some(Provider::SigNoz)
    } else {
        None
    }
}

/// The first node in `document`, in declaration order, whose tool belongs
/// to `provider` -- `None` when `document` never calls that provider.
/// "First" only picks which tool a refusal names when more than one node
/// would do; it is not a claim about execution order.
fn first_tool_for(document: &Workflow, provider: Provider) -> Option<&ToolName> {
    document
        .nodes
        .values()
        .map(|node| &node.tool)
        .find(|tool| provider_of(tool) == Some(provider))
}

/// [`live_catalog_for_document`]'s own refusal: [`LiveCredentialError`]
/// naming the missing or malformed variable, plus which document and
/// which of its tools needed it -- so an operator reading the refusal
/// knows not just *that* a credential is missing but *why this document*
/// demanded it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DocumentCredentialError {
    /// The underlying provider refusal. `#[serde(flatten)]` keeps
    /// `kind` (`"GitHub"`/`"Doppler"`/`"Buildkite"`) at this struct's own
    /// top level, so a caller printing this through
    /// [`willikins_core::Reported`] gets the same `{"kind": ..., "error":
    /// ..., "message": ...}` shape a bare [`LiveCredentialError`] would --
    /// plus this struct's own `document` and `tool` fields alongside.
    #[serde(flatten)]
    pub source: LiveCredentialError,
    /// The document being planned or applied.
    pub document: willikins_types::WorkflowName,
    /// The first node in `document` (declaration order) whose tool needs
    /// this credential's provider.
    pub tool: ToolName,
}

impl std::fmt::Display for DocumentCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (needed because `{}` uses `{}`)",
            self.source, self.document, self.tool
        )
    }
}

impl std::error::Error for DocumentCredentialError {}

/// Build the live catalog for one document: only the providers its own
/// nodes actually call tools from, credentials read from the process
/// environment. A tool's spec needs no credential -- only executing it
/// does -- and after a document is parsed the set of providers it calls
/// is known exactly, so the requirement is computed from `document.nodes`
/// here rather than guessed or demanded wholesale.
///
/// `willikins-tools`' five pure tools are always inserted (no provider,
/// no credential). Each of GitHub/Doppler/Buildkite is inserted -- with its
/// credential read and validated from the environment -- only when
/// `document` has at least one node calling one of that provider's
/// tools. A provider `document` never calls needs no credential at all,
/// valid or otherwise, and none of its tools are present in the returned
/// catalog (which is fine: `check` only ever resolves the tools a
/// document's own nodes name).
///
/// This is `plan <file> --live` and `apply <file> --live`'s own catalog.
/// [`live_catalog_from_env`] (every credential, unconditionally) remains
/// `apply --plan-id --live`'s, since that mode resolves against a whole
/// trusted `--workflows-dir` it cannot narrow to one document up front --
/// the same "many documents, refuse before any of them" posture a
/// long-lived server needs, which this function deliberately does not
/// change.
///
/// # Errors
///
/// [`DocumentCredentialError`] naming the missing or malformed variable,
/// `document`, and the first tool that needed it -- checking GitHub, then
/// Doppler, then Buildkite (the same order [`live_catalog_from_env`]
/// checks), so a document missing more than one needed credential always
/// reports the same one first. No network call is made either way.
pub fn live_catalog_for_document(document: &Workflow) -> Result<Catalog, DocumentCredentialError> {
    let mut catalog = Catalog::new(willikins_types::registry());
    insert_pure_tools(&mut catalog);
    insert_appstore_tools(&mut catalog);

    if let Some(tool) = first_tool_for(document, Provider::GitHub) {
        let tool = tool.clone();
        let credential = willikins_providers_github::credential_from_env().map_err(|error| {
            DocumentCredentialError {
                source: LiveCredentialError::GitHub {
                    error: error.to_string(),
                },
                document: document.name.clone(),
                tool: tool.clone(),
            }
        })?;
        insert_github_tools(
            &mut catalog,
            willikins_providers_github::http_client(credential),
        );
    }
    if let Some(tool) = first_tool_for(document, Provider::Doppler) {
        let tool = tool.clone();
        let credential = willikins_providers_doppler::credential_from_env().map_err(|error| {
            DocumentCredentialError {
                source: LiveCredentialError::Doppler {
                    error: error.to_string(),
                },
                document: document.name.clone(),
                tool: tool.clone(),
            }
        })?;
        insert_doppler_tools(
            &mut catalog,
            willikins_providers_doppler::http_client(credential),
        );
    }
    if let Some(tool) = first_tool_for(document, Provider::Buildkite) {
        let tool = tool.clone();
        let credential = willikins_providers_buildkite::credential_from_env().map_err(|error| {
            DocumentCredentialError {
                source: LiveCredentialError::Buildkite {
                    error: error.to_string(),
                },
                document: document.name.clone(),
                tool: tool.clone(),
            }
        })?;
        insert_buildkite_tools(
            &mut catalog,
            willikins_providers_buildkite::http_client(credential),
        );
    }
    if let Some(tool) = first_tool_for(document, Provider::SigNoz) {
        let tool = tool.clone();
        let signoz_http = signoz_http_from_env().map_err(|source| DocumentCredentialError {
            source,
            document: document.name.clone(),
            tool: tool.clone(),
        })?;
        insert_signoz_tools(&mut catalog, signoz_http);
    }
    Ok(catalog)
}

impl crate::Butler {
    /// The live catalog: `willikins-tools`' five pure tools plus every
    /// live GitHub, Doppler, Buildkite, and `SigNoz` tool, against each
    /// provider's real API. See this module's own docs.
    #[must_use]
    pub fn live_catalog(
        github: Credential,
        doppler: Credential,
        buildkite: Credential,
        signoz_http: Http,
    ) -> Catalog {
        live_catalog(github, doppler, buildkite, signoz_http)
    }

    /// A fresh all-fake catalog and its seedable state handle -- for
    /// tests, and for the CLI's default (non-`--live`) mode. A thin
    /// wrapper over `willikins_providers_fake::empty`, kept on `Butler`
    /// so every caller builds a `Catalog` through this crate's own
    /// surface rather than reaching into `willikins-providers-fake`
    /// directly.
    #[must_use]
    pub fn fake_catalog() -> (
        std::sync::Arc<std::sync::Mutex<willikins_providers_fake::FakeState>>,
        Catalog,
    ) {
        willikins_providers_fake::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_providers_http::Credential;
    use willikins_types::DomainType;

    /// A port nothing listens on: a `check` never calls a tool, and a
    /// test that accidentally did would fail as a transport error rather
    /// than reach the network.
    const NOWHERE: &str = "http://127.0.0.1:1";

    fn test_catalog() -> Catalog {
        let github = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        let doppler = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
        let buildkite =
            Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken12345678");
        let signoz =
            Credential::for_testing("WILLIKINS_TEST_SIGNOZ_API_KEY", "testsignozapikey00000000");
        live_catalog_with(
            Http::new(
                NOWHERE,
                willikins_providers_github::default_headers(),
                github,
            ),
            Http::new(NOWHERE, Vec::new(), doppler),
            Http::new(NOWHERE, Vec::new(), buildkite),
            Http::new(NOWHERE, Vec::new(), signoz),
        )
    }

    #[test]
    fn the_live_catalog_has_exactly_these_tools_and_no_fake_tool_fits() {
        let catalog = test_catalog();
        for name in LIVE_TOOL_NAMES {
            let tool_name = willikins_core::ToolName::parse(name).unwrap();
            let tool = catalog
                .get(&tool_name)
                .unwrap_or_else(|| panic!("the live catalog holds `{name}`"));
            tool.spec()
                .validate(willikins_types::registry())
                .unwrap_or_else(|err| panic!("`{name}`'s spec validates: {err:?}"));
        }
        assert_eq!(catalog.specs().count(), LIVE_TOOL_NAMES.len());

        let (_fake_state, fake_catalog) = willikins_providers_fake::empty();
        let mut catalog = test_catalog();
        for name in LIVE_TOOL_NAMES {
            let tool_name = willikins_core::ToolName::parse(name).unwrap();
            let fake = fake_catalog
                .get(&tool_name)
                .unwrap_or_else(|| panic!("the fake catalog holds `{name}` too"));
            catalog
                .insert(fake.clone())
                .expect_err(&format!("`{name}` is already in the live catalog"));
        }
    }

    // -------------------------------------------------------------
    // Per-document credential narrowing: `provider_of`, `first_tool_for`,
    // and `live_catalog_for_document`'s pure decision logic. The
    // credential-threading half (an actually missing/malformed
    // `WILLIKINS_*_TOKEN`) is exercised by the `willikins-cli` acceptance
    // tests in `crates/willikins-cli/tests/serve_and_live.rs`, as a
    // subprocess with a controlled environment -- not here, where mutating
    // the real process environment would race any other test in this
    // binary.
    // -------------------------------------------------------------

    fn node(tool_name: &str) -> willikins_core::Node {
        willikins_core::Node::new(ToolName::parse(tool_name).unwrap())
    }

    fn workflow_using(tool_names: &[&str]) -> Workflow {
        let mut workflow = Workflow::new(willikins_types::WorkflowName::parse("demo").unwrap());
        for (index, tool_name) in tool_names.iter().enumerate() {
            let node_name = willikins_core::NodeName::parse(&format!("step_{index}")).unwrap();
            workflow = workflow.node(node_name, node(tool_name));
        }
        workflow
    }

    /// Every array feeding [`live_catalog_for_document`]'s per-provider
    /// gate, together with the five pure tool names, is exactly
    /// [`LIVE_TOOL_NAMES`] -- so a tool added to one list and not the
    /// other (e.g. a new Doppler tool added to [`insert_doppler_tools`]
    /// but not [`DOPPLER_TOOL_NAMES`]) fails here instead of silently
    /// never gaining its own credential gate.
    #[test]
    fn the_provider_tool_name_arrays_partition_live_tool_names() {
        use std::collections::BTreeSet;

        let mut from_provider_arrays: Vec<&str> = vec![
            "naming.v1",
            "template.render",
            "env.get",
            "base64.decode",
            "apple.signing_key.parse",
            "apple.issuer_id.parse",
            "apple.key_id.parse",
            "appstore.bundle_id.ensure",
            "appstore.bundle_id_capability.ensure",
        ];
        from_provider_arrays.extend(GITHUB_TOOL_NAMES);
        from_provider_arrays.extend(DOPPLER_TOOL_NAMES);
        from_provider_arrays.extend(BUILDKITE_TOOL_NAMES);
        from_provider_arrays.extend(SIGNOZ_TOOL_NAMES);

        assert_eq!(
            from_provider_arrays.len(),
            LIVE_TOOL_NAMES.len(),
            "a tool name appears in more than one array, or is missing from one"
        );
        let left: BTreeSet<&str> = from_provider_arrays.into_iter().collect();
        let right: BTreeSet<&str> = LIVE_TOOL_NAMES.into_iter().collect();
        assert_eq!(left, right);
    }

    #[test]
    fn provider_of_maps_every_live_tool_to_its_provider() {
        for name in GITHUB_TOOL_NAMES {
            let tool = willikins_core::ToolName::parse(name).unwrap();
            assert_eq!(provider_of(&tool), Some(Provider::GitHub), "{name}");
        }
        for name in DOPPLER_TOOL_NAMES {
            let tool = willikins_core::ToolName::parse(name).unwrap();
            assert_eq!(provider_of(&tool), Some(Provider::Doppler), "{name}");
        }
        for name in BUILDKITE_TOOL_NAMES {
            let tool = willikins_core::ToolName::parse(name).unwrap();
            assert_eq!(provider_of(&tool), Some(Provider::Buildkite), "{name}");
        }
        for name in SIGNOZ_TOOL_NAMES {
            let tool = willikins_core::ToolName::parse(name).unwrap();
            assert_eq!(provider_of(&tool), Some(Provider::SigNoz), "{name}");
        }
    }

    #[test]
    fn provider_of_is_none_for_a_pure_tool() {
        for name in [
            "naming.v1",
            "template.render",
            "env.get",
            "base64.decode",
            "apple.signing_key.parse",
            "apple.issuer_id.parse",
            "apple.key_id.parse",
            "appstore.bundle_id.ensure",
            "appstore.bundle_id_capability.ensure",
        ] {
            let tool = willikins_core::ToolName::parse(name).unwrap();
            assert_eq!(provider_of(&tool), None, "{name}");
        }
    }

    /// The shape of the operator's own wart report: a document naming
    /// only Doppler tools calls for no GitHub or Buildkite credential at
    /// all.
    #[test]
    fn a_doppler_only_document_needs_no_github_or_buildkite_tool() {
        let workflow = workflow_using(&["doppler.project.ensure", "doppler.config.ensure"]);
        assert!(first_tool_for(&workflow, Provider::GitHub).is_none());
        assert!(first_tool_for(&workflow, Provider::Buildkite).is_none());
        let tool = first_tool_for(&workflow, Provider::Doppler)
            .expect("the document has a doppler.* node");
        assert_eq!(tool.as_str(), "doppler.project.ensure");
    }

    #[test]
    fn first_tool_for_names_the_first_matching_node_in_declaration_order() {
        let workflow = workflow_using(&[
            "naming.v1",
            "github.repo.ensure",
            "doppler.project.ensure",
            "github.actions_secret.ensure",
        ]);
        let tool =
            first_tool_for(&workflow, Provider::GitHub).expect("the document has a github node");
        assert_eq!(tool.as_str(), "github.repo.ensure");
    }
}
