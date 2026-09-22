//! Execution-context credentials, a synchronous HTTP client wrapper, and
//! provider-response error mapping for willikins' live provider crates
//! (`willikins-providers-github`, `willikins-providers-doppler`).
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-providers-http` crate contract, and trust boundary 1
//! ("Two kinds of secret") and 5 ("Provider responses are a redaction
//! boundary").
//!
//! # What never lives here
//!
//! A [`Credential`] is an *execution-context credential* (the butler's
//! own GitHub or Doppler token), never a *graph secret* — it is not a
//! domain type and never enters `willikins_types`' type registry or
//! becomes a [`willikins_core::Value`]. Its bytes are read in exactly two
//! functions, `Credential::authorize` and `Credential::authorize_header`
//! (both crate-private, so the builder either returns cannot be handed
//! out with the header still on it) — the second exists for a provider
//! whose documented scheme is not `Authorization: Bearer` (`SigNoz`:
//! `SigNoz-Api-Key`), and both are the only place `Http` ever attaches a
//! credential, via `Http`'s own `apply_credential`. `clippy.toml`
//! disallows
//! `secrecy::ExposeSecret::expose_secret` everywhere else in the
//! workspace, and `crates/willikins-core/tests/expose_secret_guard.rs`
//! makes that a test rather than a reviewer's grep.
//!
//! `apple_credential::AppleSigningCredential` is a different shape and
//! this paragraph does not describe it: it *is* built from a graph
//! secret (`willikins_types::AppleSigningKey`), read exactly once
//! through that type's own `expose(&SinkToken)` — never through
//! `expose_secret` — with a real `SinkToken` the caller already holds,
//! not one this crate mints. This crate does not itself request the
//! `executor` cargo feature that guards `SinkToken::new`; it depends on
//! `willikins-core`, which does, so the feature is on regardless of what
//! this crate asks for — the only place that matters in practice is this
//! crate's own `#[cfg(test)]` code (`apple_credential`'s tests mint their
//! own token, the same way every other provider crate's tests do), never
//! anything reachable from `Tool::read`.

mod apple_credential;
mod credential;
mod error;
mod http;
mod retry_after;
mod sleeper;

/// Mock-server test support for a live provider crate's own tests. See
/// [`testing`] module docs.
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use apple_credential::{AppleCredentialError, AppleSigningCredential, AppleToken};
pub use credential::{Credential, CredentialError};
pub use error::{
    MAX_MESSAGE_CHARS, MISSING_PERMISSION, ProviderError, ProviderFacts, bounded_message,
    provider_says,
};
pub use http::{Http, MAX_RETRY_AFTER};
pub use sleeper::{RealSleeper, Sleeper};
