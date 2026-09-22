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
//! through that type's own token-less `reveal_for_signing` — never
//! through `expose_secret` directly — because `willikins-providers-appstore`'s
//! `Tool::read` needs to build one too, and `Tool::read` never receives a
//! `SinkToken` at all (see `apple_credential`'s own module doc, "`new`
//! takes no `SinkToken`", for why an earlier `&SinkToken`-taking version
//! of this constructor did not survive that provider crate's own
//! `read`). `Credential::from_bearer_token` is the one place this
//! crate's own execution-context `Credential` accepts a value it did not
//! itself read from the environment or a test — a minted
//! `AppleSigningCredential::sign` output is exactly such a value, and
//! that constructor's own doc explains why wrapping it there is not a
//! reopening of "sibling, not variant".

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
    MAX_MESSAGE_CHARS, MISSING_PERMISSION, ProviderError, ProviderFacts, UNAUTHENTICATED,
    bounded_message, provider_says,
};
pub use http::{Http, MAX_RETRY_AFTER};
pub use sleeper::{RealSleeper, Sleeper};
