//! Live Buildkite provider tools: `buildkite.pipeline.ensure`,
//! `buildkite.cluster.get`.
//!
//! See `docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`'s
//! `willikins-providers-buildkite` crate contract and trust boundaries 6
//! ("A third execution-context credential, with no least-privilege
//! split"), 7 ("Two Buildkite response values are credential-bearing and
//! never leave the client"), and 8 ("The pipeline configuration is
//! willikins' own frozen constant"). Facts about Buildkite's REST API are
//! from `docs/research/2026-09-16-m3a-buildkite.md`, which quotes
//! Buildkite's published documentation verbatim.
//!
//! This crate's shape mirrors `willikins-providers-doppler` deliberately:
//! same credential/`Http` split, same read-then-create idempotence
//! pattern, same re-read-on-ambiguous-create-failure recovery.

mod client;
pub mod tools;

pub use client::{
    BUILDKITE_API_BASE_URL, BuildkiteClient, BuildkiteCredentialError, CREDENTIAL_PATTERN,
    CREDENTIAL_VAR, MANAGED_DESCRIPTION, MAX_CLUSTER_PAGES, UPLOAD_CONFIGURATION,
    credential_from_env, http_client, pipeline_web_url, ssh_repository_url,
};
pub use tools::{BuildkiteClusterGet, BuildkitePipelineEnsure};
