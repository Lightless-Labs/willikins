//! Live Doppler provider tools: `doppler.project.ensure`,
//! `doppler.config.ensure`, `doppler.config.inheritable.ensure`,
//! `doppler.config.inherits.ensure`, `doppler.service_token.ensure`,
//! `doppler.service_token.rotate`, and `doppler.secret.get`.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-providers-doppler` crate contract and trust boundaries 1
//! ("Two kinds of secret") and 5 ("Provider responses are a redaction
//! boundary"). Facts about Doppler's REST API are from
//! `docs/research/2026-09-12-m2-dependencies.md`, section 3, which quotes
//! Doppler's published `OpenAPI` schemas verbatim.
//!
//! # `doppler.service_token.rotate`'s `read`
//!
//! The plan's port table says this tool's `read` behaves "as `ensure`'s
//! read" — reporting `Present` when a token by that name is already
//! listed. This crate deliberately does not do that: it always reports
//! [`willikins_core::Observation::Absent`], performing the listing `GET`
//! first so a bad credential, or a genuine provider failure, still fails
//! at plan time, but never letting the observation itself read
//! `Present`. Since 2026-09-16, a 404 from that listing call is *not*
//! one of the failures this fails on: it means the parent project or
//! config does not exist yet, which at plan time is exactly the state
//! before this workflow's own `doppler.project.ensure`/
//! `doppler.config.ensure` nodes have run — see
//! `tools::service_token_rotate`'s and `tools::service_token_ensure`'s
//! own doc comments and `fixtures/doppler/README.md` for the defect this
//! fixed. Since 2026-09-20 the same is true of the `400` "This token
//! does not have access to requested project" that the identical
//! missing parent answers once this token can see any project in the
//! workplace at all (which is every run after the first): both shapes go
//! through the one [`looks_like_a_missing_project`] predicate
//! (`docs/solutions/providers/doppler-400s-a-missing-project-once-any-project-is-visible.md`).
//!
//! This mirrors `willikins_providers_fake::tools::DopplerServiceTokenRotate`'s
//! own, already-shipped decision, and for the same reason its module doc
//! gives: a `Present` observation plans as `Action::NoOp`, and a plan is
//! what a human approves before a `Destructive` step runs. A plan that
//! showed "nothing will happen" for a step about to revoke a live token
//! would misrepresent it to the very approver the class exists to gate.
//! `willikins_core::apply`'s executor calls `Tool::ensure` whenever a
//! node's required inputs are fully known regardless of its planned
//! action (only an *unknown* required input on an `Action::NoOp` node
//! skips the call, converging instead) — so this choice changes nothing
//! about when a live apply actually rotates a token, only what a human
//! reviewing the plan is told is about to happen. Recorded here because
//! it reads as a narrowing of the plan's literal wording, not as
//! following it.

mod client;
pub mod tools;

pub use client::{
    CREDENTIAL_PATTERN, CREDENTIAL_VAR, DOPPLER_API_BASE_URL, DopplerClient,
    DopplerCredentialError, MANAGED_DESCRIPTION, credential_from_env, http_client,
    looks_like_a_missing_project,
};
pub use tools::{
    DopplerConfigEnsure, DopplerConfigInheritableEnsure, DopplerConfigInheritsEnsure,
    DopplerProjectEnsure, DopplerSecretGet, DopplerServiceTokenEnsure, DopplerServiceTokenRotate,
};
