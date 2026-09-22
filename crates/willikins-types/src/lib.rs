//! Nominal domain types for willikins.
//!
//! Every value that crosses a tool boundary is one of these types. A bare
//! `String` never appears in a tool port. Secrecy is a property of the type:
//! a secret type never implements plain `Display`, prints a redacted marker
//! from `Debug`, and never serializes through plain serde.
//!
//! See `docs/plans/2026-09-11-willikins-design.md` for the invariants.
//!
//! Derived provider names come from [`naming::v1`]: `github_repo`,
//! `doppler_project`, `doppler_root_config`, and `buildkite_pipeline_slug`.
//! These are pure, total, and frozen — see [`NamingScheme`] for the
//! freeze rule.

extern crate self as willikins_types;

pub mod __private;
pub mod object;
#[cfg(test)]
mod probe;
pub mod sink;

/// Capability token gating access to secret values; see [`sink::SinkToken`].
pub use sink::SinkToken;

pub use object::{DomainObject, Rendered, downcast};

/// Error returned when a string does not parse as a domain type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, serde::Serialize, schemars::JsonSchema)]
#[error("{type_name}: {reason}")]
pub struct ParseError {
    /// The domain type that rejected the input.
    pub type_name: &'static str,
    /// Why it was rejected. Never contains the rejected value of a secret type.
    pub reason: String,
}

impl ParseError {
    /// Build a parse error for `type_name`.
    pub fn new(type_name: &'static str, reason: impl Into<String>) -> Self {
        Self {
            type_name,
            reason: reason.into(),
        }
    }
}

/// The greatest number of characters of rejected input a [`ParseError`]
/// message quotes; see [`quoted`].
pub const MAX_QUOTED_INPUT: usize = 64;

/// Quote `input` for a [`ParseError`] message: bounded to
/// [`MAX_QUOTED_INPUT`] characters, and escaped.
///
/// A rejected non-secret value is the caller's own text, and quoting it is
/// how an agent sees what it got wrong -- but the caller may be a hostile
/// document, and every one of these messages is printed straight to the
/// stdout an agent reads. Two things follow, and this function is the one
/// chokepoint for both (adversarial pass 2, finding 6):
///
/// - **Bounded.** A ten-megabyte literal used to be echoed whole. Longer
///   input is cut at a character boundary and followed by its full length,
///   so the message still says how big the value was.
/// - **Escaped.** A YAML double-quoted scalar may carry a newline or an
///   ANSI escape (`type: "Foo\nBar"`), and interpolating one raw lets a
///   document split one error line into two, or style the agent's stdout.
///   Every character goes through [`char::escape_debug`], so a control
///   character prints as `\n` or `\u{1b}` and never as itself.
///
/// The bound counts *input* characters, not escaped ones, so the quote can
/// be up to six times [`MAX_QUOTED_INPUT`] characters long -- still a
/// constant, which is the whole point.
///
/// Never call this on a secret value: a secret type's parser reports only
/// its constraint, never its input, and nothing here would make quoting
/// one safe.
#[must_use]
pub fn quoted(input: &str) -> String {
    let cut = input
        .char_indices()
        .nth(MAX_QUOTED_INPUT)
        .map(|(index, _)| index);
    let head: String = input[..cut.unwrap_or(input.len())].escape_debug().collect();
    match cut {
        None => format!("`{head}`"),
        Some(_) => format!("`{head}`... ({} characters)", input.chars().count()),
    }
}

/// A nominal domain type.
///
/// Implemented by `#[derive(DomainType)]` from `willikins-derive` for newtypes,
/// and by hand for structured identities such as `GitHubRepo`.
pub trait DomainType: Sized + Clone + std::fmt::Debug + PartialEq + Eq {
    /// Stable name used in tool port declarations, the type catalog, and
    /// JSON schemas. Never changes once published.
    const TYPE_NAME: &'static str;
    /// Whether values of this type are secret. Secret values may only flow
    /// into secret-accepting sinks and are redacted everywhere else.
    const IS_SECRET: bool = false;

    /// One-line description shown to agents in the type catalog.
    fn description() -> &'static str;
    /// A valid example value. For secret types, a placeholder that parses.
    fn example() -> &'static str;
    /// Parse and validate. Format validation only; never touches a network.
    fn parse(input: &str) -> Result<Self, ParseError>;
    /// JSON schema for the serialized form.
    fn json_schema() -> schemars::Schema;
}

/// Runtime description of a domain type, for the type catalog.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TypeInfo {
    /// [`DomainType::TYPE_NAME`].
    pub name: &'static str,
    /// [`DomainType::IS_SECRET`].
    pub secret: bool,
    /// [`DomainType::description`].
    pub description: &'static str,
    /// [`DomainType::example`].
    pub example: &'static str,
    /// [`DomainType::json_schema`].
    pub schema: schemars::Schema,
}

impl TypeInfo {
    /// Describe `T`.
    pub fn of<T: DomainType>() -> Self {
        Self {
            name: T::TYPE_NAME,
            secret: T::IS_SECRET,
            description: T::description(),
            example: T::example(),
            schema: T::json_schema(),
        }
    }
}

pub use willikins_derive::DomainType;

pub mod appstore;
pub mod buildkite;
pub mod description;
pub mod doppler;
pub mod env;
pub mod github;
pub mod name;
pub mod naming;
pub mod propose;
pub mod registry;
pub mod reserved;
pub mod secret;
pub mod signoz;
pub mod slug;
pub mod text;
pub mod word;
pub mod workflow_name;

pub use appstore::{
    AppleBundleIdId, AppleBundleIdName, AppleBundleIdPlatform, AppleBundleIdentifier,
    AppleCapabilityType, AppleIssuerId, AppleKeyId, AppleSigningKey,
};
pub use buildkite::{
    BuildkiteClusterId, BuildkiteClusterName, BuildkiteOrg, BuildkitePipelineSlug,
};
pub use description::Description;
pub use doppler::{
    DopplerConfig, DopplerConfigName, DopplerProject, DopplerSecretValue, DopplerServiceToken,
    DopplerTokenName, SecretName,
};
pub use env::EnvVarName;
pub use github::{ActionsSecretName, GitHubOrg, GitHubRepo, HttpsUrl, RepoVisibility};
pub use name::ProjectName;
pub use naming::NamingScheme;
pub use propose::{ProposeError, propose_slug};
pub use registry::{TypeName, TypeRef, TypeRegistry};
pub use reserved::is_reserved;
pub use secret::OpaqueSecret;
pub use signoz::{SigNozIngestionKeyName, SigNozIngestionKeyValue};
pub use slug::{ComponentSlug, EnvironmentSlug, ProjectSlug};
pub use text::{TemplateSource, Text};
pub use word::{Word, WordList};
pub use workflow_name::WorkflowName;

// `type_infos()`, `registry()`, and `assert_all_examples_parse()` are
// generated together from this one list, so the type catalog and the type
// registry cannot drift apart. See `registry::domain_types!`.
registry::domain_types! {
    WordList,
    ProjectSlug,
    ComponentSlug,
    EnvironmentSlug,
    ProjectName,
    Text,
    TemplateSource,
    GitHubOrg,
    RepoVisibility,
    GitHubRepo,
    HttpsUrl,
    ActionsSecretName,
    EnvVarName,
    DopplerProject,
    DopplerConfigName,
    DopplerConfig,
    DopplerTokenName,
    SecretName,
    DopplerServiceToken,
    DopplerSecretValue,
    BuildkiteOrg,
    BuildkitePipelineSlug,
    BuildkiteClusterId,
    BuildkiteClusterName,
    WorkflowName,
    Description,
    SigNozIngestionKeyName,
    SigNozIngestionKeyValue,
    OpaqueSecret,
    AppleSigningKey,
    AppleIssuerId,
    AppleKeyId,
    AppleBundleIdentifier,
    AppleBundleIdName,
    AppleBundleIdPlatform,
    AppleBundleIdId,
    AppleCapabilityType,
}

/// Assert that `T::example()` parses as `T`.
///
/// For use by catalog tests that check every domain type's own example
/// against its own parser. Panics, naming the type, if it does not.
///
/// # Panics
///
/// Panics when `T::parse(T::example())` returns `Err`.
pub fn assert_example_parses<T: DomainType>() {
    if let Err(err) = T::parse(T::example()) {
        panic!(
            "{}: example {:?} does not parse as its own type: {err}",
            T::TYPE_NAME,
            T::example()
        );
    }
}
