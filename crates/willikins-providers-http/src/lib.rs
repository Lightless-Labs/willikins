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
//! domain type, never enters `willikins_types`' type registry, is never a
//! [`willikins_core::Value`], and this crate does not depend on
//! `willikins-types` with the `executor` feature and mints no
//! `willikins_types::SinkToken`. Its bytes are read in exactly one
//! function, [`Credential::authorize`]; `clippy.toml` disallows
//! `secrecy::ExposeSecret::expose_secret` everywhere else in the
//! workspace, and `crates/willikins-core/tests/expose_secret_guard.rs`
//! makes that a test rather than a reviewer's grep.

mod credential;
mod error;
mod http;
mod retry_after;
mod sleeper;

/// Mock-server test support for a live provider crate's own tests. See
/// [`testing`] module docs.
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use credential::{Credential, CredentialError};
pub use error::{
    MAX_MESSAGE_CHARS, MISSING_PERMISSION, ProviderError, bounded_message, provider_says,
};
pub use http::Http;
pub use sleeper::{RealSleeper, Sleeper};
