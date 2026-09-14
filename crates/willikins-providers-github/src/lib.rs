//! Live GitHub provider tools: `github.repo.ensure` and
//! `github.actions_secret.ensure`.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-providers-github` crate contract and trust boundaries 1
//! ("Two kinds of secret") and 5 ("Provider responses are a redaction
//! boundary"). Facts about GitHub's REST API are from
//! `docs/research/2026-09-12-m2-dependencies.md`, section 2, which quotes
//! GitHub's published `OpenAPI` description verbatim.

mod client;
mod seal;
pub mod tools;

pub use client::{
    CREDENTIAL_PATTERN, CREDENTIAL_VAR, GITHUB_API_BASE_URL, GitHubClient, credential_from_env,
    default_headers, http_client,
};
pub use tools::{GitHubActionsSecretEnsure, GitHubRepoEnsure};

/// The repository topic willikins uses to mark a repository as its own.
/// GitHub's topic rule (lowercase letters, digits, hyphens; at most 50
/// characters, at most 20 topics per repository) is satisfied trivially.
pub(crate) const MANAGED_TOPIC: &str = "managed-by-willikins";
