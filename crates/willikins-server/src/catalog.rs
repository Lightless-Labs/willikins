//! [`Butler::live_catalog`] and [`Butler::fake_catalog`]: the two ways a
//! caller builds the [`willikins_core::Catalog`] a [`crate::ButlerConfig`]
//! needs.
//!
//! `live_catalog` assembles the live catalog (thirty-nine tools since
//! milestone 3i task A4 added `buildkite.pipeline.bootstrap.ensure`, after
//! milestone 3h task D3 added `doppler.project_member.ensure`, after
//! milestone 3g task B1 added `buildkite.pipeline.bootstrap.gate`, after
//! task G2 added `github.scaffold.ensure`, after task T1
//! added `repo.file.render`, after the App Attest
//! gate task added `appstore.bundle_id_capability.gate`, after
//! milestone 3e task K1 added `buildkite.token.parse`, after task R2
//! added `github.token.parse`, after milestone 3e's own task B1 added
//! `doppler.branch_config.ensure`, after task 3 added the two Sample
//! gates, `appstore.app.get` and `appstore.app_group.gate`) --
//! `willikins-tools`' eleven pure tools (`naming.v1`, `template.render`,
//! `env.get`, `base64.decode`, `apple.signing_key.parse`,
//! `apple.issuer_id.parse`, `apple.key_id.parse`, `github.token.parse`,
//! `buildkite.token.parse`, `operator.acknowledge`, `repo.file.render`),
//! `willikins-providers-github`'s four
//! live tools (milestone 3e task 2 added `github.repo.get`; milestone 3g
//! task G2 added `github.scaffold.ensure`),
//! `willikins-providers-doppler`'s eleven (milestone 3 added
//! `doppler.config.inheritable.ensure` and
//! `doppler.config.inherits.ensure`; the `SigNoz` task added
//! `doppler.secret.set`; the App Store Connect credential correction
//! added `doppler.value.get`; milestone 3e task B1 added
//! `doppler.branch_config.ensure`; milestone 3h task D3 added
//! `doppler.project_member.ensure`), `willikins-providers-buildkite`'s
//! four (milestone 3a; milestone 3g task B1 added
//! `buildkite.pipeline.bootstrap.gate`; milestone 3i task A4 added
//! `buildkite.pipeline.bootstrap.ensure`), `willikins-providers-signoz`'s
//! one (the `SigNoz`
//! task), and `willikins-providers-appstore`'s seven (two from the App
//! Store Connect provider crate; milestone 3c added
//! `appstore.certificate.get`; milestone 3c task 2 added
//! `appstore.profile.ensure`; milestone 3e task 3 added the two Sample
//! gates, `appstore.app.get` and `appstore.app_group.gate`; the App
//! Attest gate task added `appstore.bundle_id_capability.gate`) -- exactly as
//! `crates/willikins-providers-doppler/tests/live_catalog.rs` built it
//! before this task; that test now calls [`live_catalog_with`] (this
//! module's own assembly, taking `Http`s rather than `Credential`s so a
//! test can point them at a mock server or nowhere) instead of keeping a
//! second copy, so the assembly exists exactly once.

use std::sync::Arc;

use willikins_core::{Catalog, Tool, ToolName, Workflow, helpers};
use willikins_providers_appstore::{
    AppstoreAppGet, AppstoreAppGroupGate, AppstoreBundleIdCapabilityEnsure,
    AppstoreBundleIdCapabilityGate, AppstoreBundleIdEnsure, AppstoreCertificateGet,
    AppstoreProfileEnsure,
};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkiteClusterGet, BuildkitePipelineBootstrapEnsure,
    BuildkitePipelineBootstrapGate, BuildkitePipelineEnsure,
};
use willikins_providers_doppler::{
    DopplerBranchConfigEnsure, DopplerClient, DopplerConfigEnsure, DopplerConfigInheritableEnsure,
    DopplerConfigInheritableGate, DopplerConfigInheritsEnsure, DopplerProjectEnsure,
    DopplerProjectMemberEnsure, DopplerSecretGet, DopplerSecretSet, DopplerServiceTokenEnsure,
    DopplerServiceTokenRotate, DopplerValueGet,
};
use willikins_providers_github::{
    GitHubActionsSecretEnsure, GitHubClient, GitHubRepoEnsure, GitHubRepoGet, GitHubScaffoldEnsure,
};
use willikins_providers_http::{Credential, Http};
use willikins_providers_signoz::{SigNozClient, SigNozIngestionKeyEnsure};

/// Every tool name [`live_catalog_with`] (and so [`Butler::live_catalog`])
/// inserts, in insertion order -- pinned by
/// `tests::the_live_catalog_has_exactly_these_tools_and_no_fake_tool_fits`.
pub const LIVE_TOOL_NAMES: [&str; 39] = [
    "naming.v1",
    "template.render",
    "operator.acknowledge",
    "env.get",
    "base64.decode",
    "apple.signing_key.parse",
    "apple.issuer_id.parse",
    "apple.key_id.parse",
    "github.token.parse",
    "buildkite.token.parse",
    "repo.file.render",
    "github.repo.ensure",
    "github.actions_secret.ensure",
    "github.repo.get",
    "github.scaffold.ensure",
    "doppler.project.ensure",
    "doppler.config.ensure",
    "doppler.branch_config.ensure",
    "doppler.config.inheritable.ensure",
    "doppler.config.inheritable.gate",
    "doppler.config.inherits.ensure",
    "doppler.project_member.ensure",
    "doppler.service_token.ensure",
    "doppler.service_token.rotate",
    "doppler.secret.get",
    "doppler.secret.set",
    "doppler.value.get",
    "signoz.ingestion_key.ensure",
    "buildkite.pipeline.ensure",
    "buildkite.cluster.get",
    "buildkite.pipeline.bootstrap.gate",
    "buildkite.pipeline.bootstrap.ensure",
    "appstore.bundle_id.ensure",
    "appstore.bundle_id_capability.ensure",
    "appstore.certificate.get",
    "appstore.profile.ensure",
    "appstore.app.get",
    "appstore.app_group.gate",
    "appstore.bundle_id_capability.gate",
];

/// Insert `willikins-tools`' eleven pure tools -- no provider, no
/// credential, always present regardless of which providers a document
/// uses. `env.get`, `base64.decode`, and `apple.signing_key.parse` joined
/// `naming.v1` and `template.render` here once the App Store Connect
/// credential correction gave a resolver chain (`env.get` or
/// `doppler.secret.get`, optionally through `base64.decode`, ending at
/// `apple.signing_key.parse`) real documents to run in; `apple.issuer_id.parse`
/// and `apple.key_id.parse` joined once `willikins-providers-appstore`
/// gave a `Text`-emitting resolver (`doppler.value.get`) a typed port to
/// reach; `operator.acknowledge` joined in G3, the gate over an operator's
/// own acknowledgement (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`,
/// decision (j), point 6); `github.token.parse` joined in task R2, the same
/// resolver-chain shape given to `willikins-providers-github`'s three
/// tools' new optional `token` port; `buildkite.token.parse` joined in
/// task K1, mirroring R2 exactly for `willikins-providers-buildkite`'s
/// own two tools' new optional `token` port; `repo.file.render` joined in
/// milestone 3g task T1
/// (`docs/plans/2026-09-30-milestone-3g-file-writing.md`, decision (d)),
/// the tool Sample's document renders its scaffold files through.
fn insert_pure_tools(catalog: &mut Catalog) {
    insert(catalog, Arc::new(willikins_tools::NamingV1::new()));
    insert(catalog, Arc::new(willikins_tools::TemplateRender::new()));
    insert(
        catalog,
        Arc::new(willikins_tools::OperatorAcknowledge::new()),
    );
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
    insert(catalog, Arc::new(willikins_tools::GitHubTokenParse::new()));
    insert(
        catalog,
        Arc::new(willikins_tools::BuildkiteTokenParse::new()),
    );
    insert(catalog, Arc::new(willikins_tools::RepoFileRender::new()));
}

/// Insert `willikins-providers-appstore`'s seven live tools (task 3 added
/// two of these, the gates `appstore.app.get` and `appstore.app_group.gate`;
/// the App Attest gate task added a third gate,
/// `appstore.bundle_id_capability.gate` -- this doc comment's own count
/// had drifted to "four" before this task, corrected here). Unlike every
/// other `insert_*_tools` function in this module, this one takes no
/// `Http` and no credential at all: the App Store Connect credential's
/// three parts are ordinary graph ports, resolved per-call from a
/// document's own inputs, never read from the process environment by
/// this crate (`willikins_providers_appstore`'s own module doc). So
/// these four tools are inserted unconditionally, the same as
/// [`insert_pure_tools`], in both [`live_catalog_with`] and
/// [`live_catalog_for_document`] -- there is no environment credential
/// to gate them behind, and `tests::the_provider_tool_name_arrays_partition_live_tool_names`
/// tracks them alongside the nine pure tool names for exactly that
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
    insert(
        catalog,
        Arc::new(AppstoreCertificateGet::new(
            willikins_providers_appstore::APPSTORE_API_BASE_URL,
        )),
    );
    insert(
        catalog,
        Arc::new(AppstoreProfileEnsure::new(
            willikins_providers_appstore::APPSTORE_API_BASE_URL,
        )),
    );
    insert(
        catalog,
        Arc::new(AppstoreAppGet::new(
            willikins_providers_appstore::APPSTORE_API_BASE_URL,
        )),
    );
    insert(
        catalog,
        Arc::new(AppstoreAppGroupGate::new(
            willikins_providers_appstore::APPSTORE_API_BASE_URL,
        )),
    );
    insert(
        catalog,
        Arc::new(AppstoreBundleIdCapabilityGate::new(
            willikins_providers_appstore::APPSTORE_API_BASE_URL,
        )),
    );
}

/// Insert `willikins-providers-github`'s four live tools, built from
/// `http` (milestone 3g task G2 added `github.scaffold.ensure`).
fn insert_github_tools(catalog: &mut Catalog, http: Http) {
    let github = Arc::new(GitHubClient::new(http));
    insert(
        catalog,
        Arc::new(GitHubRepoEnsure::new(Arc::clone(&github))),
    );
    insert(
        catalog,
        Arc::new(GitHubActionsSecretEnsure::new(Arc::clone(&github))),
    );
    insert(catalog, Arc::new(GitHubRepoGet::new(Arc::clone(&github))));
    insert(catalog, Arc::new(GitHubScaffoldEnsure::new(github)));
}

/// Insert `willikins-providers-doppler`'s eleven live tools, built from
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
        Arc::new(DopplerBranchConfigEnsure::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerConfigInheritableEnsure::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerConfigInheritableGate::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerConfigInheritsEnsure::new(Arc::clone(&doppler))),
    );
    insert(
        catalog,
        Arc::new(DopplerProjectMemberEnsure::new(Arc::clone(&doppler))),
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

/// Insert `willikins-providers-buildkite`'s four live tools, built from
/// `http` (milestone 3g task B1 added `buildkite.pipeline.bootstrap.gate`;
/// milestone 3i task A4 added `buildkite.pipeline.bootstrap.ensure`,
/// inserted right after the gate, matching [`LIVE_TOOL_NAMES`]' own
/// order).
fn insert_buildkite_tools(catalog: &mut Catalog, http: Http) {
    let buildkite = Arc::new(BuildkiteClient::new(http));
    insert(
        catalog,
        Arc::new(BuildkitePipelineEnsure::new(Arc::clone(&buildkite))),
    );
    insert(
        catalog,
        Arc::new(BuildkiteClusterGet::new(Arc::clone(&buildkite))),
    );
    insert(
        catalog,
        Arc::new(BuildkitePipelineBootstrapGate::new(Arc::clone(&buildkite))),
    );
    insert(
        catalog,
        Arc::new(BuildkitePipelineBootstrapEnsure::new(buildkite)),
    );
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
/// and the eleven pure tool names together are exactly [`LIVE_TOOL_NAMES`],
/// so the two lists cannot silently drift apart.
const GITHUB_TOOL_NAMES: [&str; 4] = [
    "github.repo.ensure",
    "github.actions_secret.ensure",
    "github.repo.get",
    "github.scaffold.ensure",
];

/// `willikins-providers-doppler`'s live tool names. See
/// [`GITHUB_TOOL_NAMES`].
const DOPPLER_TOOL_NAMES: [&str; 12] = [
    "doppler.project.ensure",
    "doppler.config.ensure",
    "doppler.branch_config.ensure",
    "doppler.config.inheritable.ensure",
    "doppler.config.inheritable.gate",
    "doppler.config.inherits.ensure",
    "doppler.project_member.ensure",
    "doppler.service_token.ensure",
    "doppler.service_token.rotate",
    "doppler.secret.get",
    "doppler.secret.set",
    "doppler.value.get",
];

/// `willikins-providers-buildkite`'s live tool names. See
/// [`GITHUB_TOOL_NAMES`]. Milestone 3i task A4 added
/// `buildkite.pipeline.bootstrap.ensure`.
const BUILDKITE_TOOL_NAMES: [&str; 4] = [
    "buildkite.pipeline.ensure",
    "buildkite.cluster.get",
    "buildkite.pipeline.bootstrap.gate",
    "buildkite.pipeline.bootstrap.ensure",
];

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

/// The name of the port through which one of `provider`'s own tools can
/// bind its own credential, bypassing that provider's environment
/// variable entirely -- `Some(port("token"))` for GitHub and Buildkite
/// (milestone 3e tasks R2 and K1, `willikins_types::GitHubToken` and
/// `BuildkiteToken`), `None` for Doppler (whose token is the root of the
/// whole credential chain -- nothing resolves *it*, so no document can
/// ever bind it) and `SigNoz` (which never grew one). `None` here is
/// what makes [`first_unbound_node_for`] fall back to "any node using
/// this provider needs it", unchanged from before this function existed.
fn credential_port(provider: Provider) -> Option<willikins_core::PortName> {
    match provider {
        Provider::GitHub | Provider::Buildkite => Some(helpers::port("token")),
        Provider::Doppler | Provider::SigNoz => None,
    }
}

/// The first node in `document`, in declaration order, whose tool
/// belongs to `provider` and which still needs that provider's
/// *environment* credential: a node whose tool has no credential port at
/// all ([`credential_port`] is `None` -- Doppler, `SigNoz`), or one whose
/// tool does have one but this node does not bind it. `None` when every
/// node of that provider binds its own credential port, or when
/// `document` calls no node from that provider at all.
///
/// This -- not "does `document` call this provider at all"
/// ([`first_tool_for`], still used unchanged to decide whether to
/// *insert* that provider's tools) -- is what
/// [`live_catalog_for_document`] gates each environment credential on:
/// a document whose every `github.*`/`buildkite.*` node binds `token`
/// (typically resolved from Doppler through `github.token.parse` /
/// `buildkite.token.parse`, `workflows/github-repo-token-from-doppler.yaml`
/// and `workflows/buildkite-cluster-token-from-doppler.yaml`'s own
/// shape) needs `WILLIKINS_GITHUB_TOKEN` / `WILLIKINS_BUILDKITE_TOKEN`
/// not at all.
fn first_unbound_node_for(
    document: &Workflow,
    provider: Provider,
) -> Option<(&willikins_core::NodeName, &ToolName)> {
    let port = credential_port(provider);
    document.nodes.iter().find_map(|(name, node)| {
        if provider_of(&node.tool) != Some(provider) {
            return None;
        }
        let leaves_it_unbound = match &port {
            Some(port) => !node.with.contains_key(port),
            None => true,
        };
        leaves_it_unbound.then_some((name, &node.tool))
    })
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
    /// The first node in `document` (declaration order) that still
    /// needs this credential's provider -- one that leaves the
    /// provider's own credential port unbound, or (for Doppler and
    /// `SigNoz`, which have no such port) simply the first node using
    /// that provider at all. See [`first_unbound_node_for`].
    pub node: willikins_core::NodeName,
    /// That node's tool.
    pub tool: ToolName,
}

impl std::fmt::Display for DocumentCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (needed because `{}`'s node `{}` uses `{}`)",
            self.source, self.document, self.node, self.tool
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
/// `willikins-tools`' pure tools are always inserted (no provider, no
/// credential). Each of GitHub/Doppler/Buildkite/`SigNoz` is inserted
/// whenever `document` has at least one node calling one of that
/// provider's tools ([`first_tool_for`]) -- a provider `document` never
/// calls needs no credential at all, valid or otherwise, and none of its
/// tools are present in the returned catalog (which is fine: `check`
/// only ever resolves the tools a document's own nodes name).
///
/// Inserting a provider's tools no longer always means reading and
/// validating its environment credential, though. GitHub and Buildkite's
/// tools each carry an optional, secret-typed `token` port
/// (`willikins_types::GitHubToken`/`BuildkiteToken`, milestone 3e tasks
/// R2, K1): when every node of that provider in `document` binds it
/// (typically resolved from Doppler, `github.token.parse`/
/// `buildkite.token.parse`'s own reason to exist), the environment
/// variable is not needed at all, and this function does not read it --
/// [`first_unbound_node_for`] is `None`, and the provider's tools are
/// built against `willikins_providers_github::http_client_without_credential`/
/// the Buildkite sibling instead, which never make a request without a
/// bound credential (see that constructor's own doc). Doppler and
/// `SigNoz` have no such port at all (Doppler's token is the root of
/// the whole credential chain; `SigNoz` never grew one), so any node
/// using either still needs that provider's environment variable exactly
/// as before this distinction existed.
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
/// `document`, and the first node (and its tool) that still needs it --
/// checking GitHub, then Doppler, then Buildkite, then `SigNoz` (the same
/// order [`live_catalog_from_env`] checks its first three), so a
/// document missing more than one needed credential always reports the
/// same one first. No network call is made either way.
pub fn live_catalog_for_document(document: &Workflow) -> Result<Catalog, DocumentCredentialError> {
    let mut catalog = Catalog::new(willikins_types::registry());
    insert_pure_tools(&mut catalog);
    insert_appstore_tools(&mut catalog);

    if first_tool_for(document, Provider::GitHub).is_some() {
        let http = match first_unbound_node_for(document, Provider::GitHub) {
            Some((node, tool)) => {
                let (node, tool) = (node.clone(), tool.clone());
                let credential =
                    willikins_providers_github::credential_from_env().map_err(|error| {
                        DocumentCredentialError {
                            source: LiveCredentialError::GitHub {
                                error: error.to_string(),
                            },
                            document: document.name.clone(),
                            node,
                            tool,
                        }
                    })?;
                willikins_providers_github::http_client(credential)
            }
            None => willikins_providers_github::http_client_without_credential(),
        };
        insert_github_tools(&mut catalog, http);
    }
    if let Some((node, tool)) = first_unbound_node_for(document, Provider::Doppler) {
        let (node, tool) = (node.clone(), tool.clone());
        let credential = willikins_providers_doppler::credential_from_env().map_err(|error| {
            DocumentCredentialError {
                source: LiveCredentialError::Doppler {
                    error: error.to_string(),
                },
                document: document.name.clone(),
                node,
                tool,
            }
        })?;
        insert_doppler_tools(
            &mut catalog,
            willikins_providers_doppler::http_client(credential),
        );
    }
    if first_tool_for(document, Provider::Buildkite).is_some() {
        let http = match first_unbound_node_for(document, Provider::Buildkite) {
            Some((node, tool)) => {
                let (node, tool) = (node.clone(), tool.clone());
                let credential =
                    willikins_providers_buildkite::credential_from_env().map_err(|error| {
                        DocumentCredentialError {
                            source: LiveCredentialError::Buildkite {
                                error: error.to_string(),
                            },
                            document: document.name.clone(),
                            node,
                            tool,
                        }
                    })?;
                willikins_providers_buildkite::http_client(credential)
            }
            None => willikins_providers_buildkite::http_client_without_credential(),
        };
        insert_buildkite_tools(&mut catalog, http);
    }
    if let Some((node, tool)) = first_unbound_node_for(document, Provider::SigNoz) {
        let (node, tool) = (node.clone(), tool.clone());
        let signoz_http = signoz_http_from_env().map_err(|source| DocumentCredentialError {
            source,
            document: document.name.clone(),
            node,
            tool,
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
    /// gate, together with the eleven pure tool names, is exactly
    /// [`LIVE_TOOL_NAMES`] -- so a tool added to one list and not the
    /// other (e.g. a new Doppler tool added to [`insert_doppler_tools`]
    /// but not [`DOPPLER_TOOL_NAMES`]) fails here instead of silently
    /// never gaining its own credential gate.
    #[test]
    fn the_provider_tool_name_arrays_partition_live_tool_names() {
        use std::collections::BTreeSet;

        // `DOPPLER_TOOL_NAMES` already carries `doppler.branch_config.ensure`
        // (milestone 3e task B1), so it needs no entry here.
        let mut from_provider_arrays: Vec<&str> = vec![
            "naming.v1",
            "template.render",
            "operator.acknowledge",
            "env.get",
            "base64.decode",
            "apple.signing_key.parse",
            "apple.issuer_id.parse",
            "apple.key_id.parse",
            "github.token.parse",
            "buildkite.token.parse",
            "repo.file.render",
            "appstore.bundle_id.ensure",
            "appstore.bundle_id_capability.ensure",
            "appstore.certificate.get",
            "appstore.profile.ensure",
            "appstore.app.get",
            "appstore.app_group.gate",
            "appstore.bundle_id_capability.gate",
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
            "github.token.parse",
            "buildkite.token.parse",
            "repo.file.render",
            "appstore.bundle_id.ensure",
            "appstore.bundle_id_capability.ensure",
            "appstore.certificate.get",
            "appstore.profile.ensure",
            "appstore.app.get",
            "appstore.app_group.gate",
            "appstore.bundle_id_capability.gate",
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

    // -------------------------------------------------------------
    // L1: `credential_port`, `first_unbound_node_for`, and the rule they
    // give `live_catalog_for_document` -- a provider's environment
    // credential is required only if at least one of that provider's
    // nodes leaves its own credential port unbound.
    // -------------------------------------------------------------

    /// [`node`], but with the tool's own optional `token` port bound to a
    /// workflow input -- the shape
    /// `workflows/github-repo-token-from-doppler.yaml` and
    /// `workflows/buildkite-cluster-token-from-doppler.yaml` both use
    /// (there, resolved from Doppler through `github.token.parse`/
    /// `buildkite.token.parse`; the binding's *source* does not matter
    /// here, only that the port is bound at all -- exactly what `plan`'s
    /// own `bind_ports` already treats as the deciding fact).
    fn node_with_token_bound(tool_name: &str) -> willikins_core::Node {
        node(tool_name).port(
            helpers::port("token"),
            willikins_core::Binding::Input(willikins_core::InputName::parse("token").unwrap()),
        )
    }

    /// [`workflow_using`]'s sibling for tests that need to control which
    /// nodes bind `token`: nodes named `step_0`, `step_1`, ... in order,
    /// each `(tool_name, token_bound)`.
    fn workflow_with_bindings(nodes: &[(&str, bool)]) -> Workflow {
        let mut workflow = Workflow::new(willikins_types::WorkflowName::parse("demo").unwrap());
        for (index, (tool_name, token_bound)) in nodes.iter().enumerate() {
            let node_name = willikins_core::NodeName::parse(&format!("step_{index}")).unwrap();
            let built = if *token_bound {
                node_with_token_bound(tool_name)
            } else {
                node(tool_name)
            };
            workflow = workflow.node(node_name, built);
        }
        workflow
    }

    #[test]
    fn credential_port_is_the_token_port_for_github_and_buildkite_only() {
        assert_eq!(
            credential_port(Provider::GitHub),
            Some(helpers::port("token"))
        );
        assert_eq!(
            credential_port(Provider::Buildkite),
            Some(helpers::port("token"))
        );
        assert_eq!(credential_port(Provider::Doppler), None);
        assert_eq!(credential_port(Provider::SigNoz), None);
    }

    #[test]
    fn first_unbound_node_for_is_none_when_every_github_node_binds_token() {
        let workflow = workflow_with_bindings(&[
            ("naming.v1", false),
            ("github.repo.ensure", true),
            ("github.repo.get", true),
        ]);
        assert!(first_unbound_node_for(&workflow, Provider::GitHub).is_none());
    }

    #[test]
    fn first_unbound_node_for_is_none_when_every_buildkite_node_binds_token() {
        let workflow = workflow_with_bindings(&[
            ("buildkite.cluster.get", true),
            ("buildkite.pipeline.ensure", true),
            ("buildkite.pipeline.bootstrap.ensure", true),
        ]);
        assert!(first_unbound_node_for(&workflow, Provider::Buildkite).is_none());
    }

    /// Milestone 3i task A4, acceptance 8: a document whose
    /// `buildkite.pipeline.bootstrap.ensure` node leaves `token` unbound
    /// still needs `WILLIKINS_BUILDKITE_TOKEN`, named by this node.
    #[test]
    fn first_unbound_node_for_names_an_unbound_bootstrap_ensure_node() {
        let workflow = workflow_with_bindings(&[("buildkite.pipeline.bootstrap.ensure", false)]);
        let (node_name, tool) = first_unbound_node_for(&workflow, Provider::Buildkite)
            .expect("the bootstrap ensure node leaves `token` unbound");
        assert_eq!(node_name.as_str(), "step_0");
        assert_eq!(tool.as_str(), "buildkite.pipeline.bootstrap.ensure");
    }

    /// Milestone 3i task A4, acceptance 8's other half: binding `token`
    /// on the one `buildkite.pipeline.bootstrap.ensure` node means no
    /// Buildkite credential is demanded at all.
    #[test]
    fn first_unbound_node_for_is_none_when_the_bootstrap_ensure_node_binds_token() {
        let workflow = workflow_with_bindings(&[("buildkite.pipeline.bootstrap.ensure", true)]);
        assert!(first_unbound_node_for(&workflow, Provider::Buildkite).is_none());
    }

    #[test]
    fn first_unbound_node_for_names_the_first_node_that_actually_leaves_it_unbound() {
        // The first github node binds `token`; the second does not --
        // the credential is still required, and the refusal must name
        // the second node (the one that actually needs it), not the
        // first (whose own binding means it never would).
        let workflow =
            workflow_with_bindings(&[("github.repo.ensure", true), ("github.repo.get", false)]);
        let (node_name, tool) = first_unbound_node_for(&workflow, Provider::GitHub)
            .expect("one node leaves `token` unbound");
        assert_eq!(node_name.as_str(), "step_1");
        assert_eq!(tool.as_str(), "github.repo.get");
    }

    #[test]
    fn first_unbound_node_for_is_none_for_a_provider_the_document_never_uses() {
        let workflow = workflow_with_bindings(&[("naming.v1", false)]);
        for provider in [
            Provider::GitHub,
            Provider::Doppler,
            Provider::Buildkite,
            Provider::SigNoz,
        ] {
            assert!(first_unbound_node_for(&workflow, provider).is_none());
        }
    }

    /// Doppler has no credential port at all
    /// ([`credential_port`] is `None`), so its environment credential is
    /// required whenever any node uses it, regardless of anything the
    /// node happens to bind under a port literally named `token` -- there
    /// is no real Doppler tool with such a port, but this proves the
    /// *rule* does not accidentally exempt one that might exist later,
    /// or one a document binds a same-named, unrelated port on.
    #[test]
    fn first_unbound_node_for_still_requires_doppler_regardless_of_any_token_like_binding() {
        let workflow = workflow_with_bindings(&[("doppler.project.ensure", true)]);
        let (node_name, tool) = first_unbound_node_for(&workflow, Provider::Doppler)
            .expect("doppler has no credential port to bind at all");
        assert_eq!(node_name.as_str(), "step_0");
        assert_eq!(tool.as_str(), "doppler.project.ensure");
    }

    #[test]
    fn first_unbound_node_for_matches_first_tool_for_when_no_node_binds_anything() {
        // The pre-L1 shape, still the common case: no node binds
        // `token`, so `first_unbound_node_for` finds exactly the same
        // node `first_tool_for` always did.
        let workflow = workflow_using(&[
            "naming.v1",
            "github.repo.ensure",
            "doppler.project.ensure",
            "github.actions_secret.ensure",
        ]);
        for provider in [Provider::GitHub, Provider::Doppler] {
            let expected = first_tool_for(&workflow, provider).map(ToolName::as_str);
            let actual = first_unbound_node_for(&workflow, provider)
                .map(|(_, tool)| tool)
                .map(ToolName::as_str);
            assert_eq!(actual, expected, "{provider:?}");
        }
    }
}
