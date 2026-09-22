//! Live App Store Connect provider tools: `appstore.bundle_id.ensure`,
//! `appstore.bundle_id_capability.ensure`.
//!
//! See `docs/research/2026-09-16-app-store-connect.md` for every fact
//! this crate rests on, and `docs/plans/2026-09-11-willikins-design.md`'s
//! 2026-09-21 addendum "Credentials are ports, resolvers are nodes" for
//! the design this crate implements.
//!
//! # No credential at construction — unlike every other live provider
//! crate in this workspace
//!
//! `willikins-providers-buildkite`/`-doppler`/`-github`/`-signoz` each
//! hold one long-lived `Arc<Client>`, built once from a `Credential` read
//! out of the process environment at server startup. This crate cannot:
//! the App Store Connect credential's three parts (`AppleIssuerId`,
//! `AppleKeyId`, `AppleSigningKey`) are ordinary graph ports, free to be
//! bound to a literal, a workflow input, an `env.get` output, or a
//! `doppler.value.get`/`doppler.secret.get` output — never read from the
//! environment by this crate, and not known at all until a document's
//! own `read`/`ensure` call supplies them
//! (`willikins_types::appstore`'s module doc states the correction this
//! whole crate carries through). So each tool here holds only its own
//! API base URL, and builds a fresh [`client::AppstoreClient`] — a fresh
//! [`AppleSigningCredential`](willikins_providers_http::AppleSigningCredential),
//! a fresh JWT, a fresh [`willikins_providers_http::Http`] — on every
//! `Tool::read`/`Tool::ensure` call, from that call's own already-typed
//! inputs. This also means the App Store Connect credential's 15-minute
//! `AppleSigningCredential::TOKEN_LIFETIME_SECS` never has a chance to
//! matter here: every call mints its own token from scratch. See
//! [`client`]'s own module doc for how the credential's bytes reach
//! `Tool::read`, which never receives a `SinkToken`.
//!
//! # No `Foreign` observation
//!
//! `appstore.bundle_id.ensure` has no ownership-marker field to check
//! (see `willikins_types::AppleBundleIdName`'s own doc) and so has no
//! `Foreign` arm at all: a differing `name` converges through `PATCH`
//! rather than refusing. See that tool's own module doc for the full
//! trade this makes.

pub mod client;
pub mod tools;

pub use client::{
    APPSTORE_API_BASE_URL, AppstoreClient, CAPABILITIES_NEEDING_PORTAL_CONFIGURATION,
};
pub use tools::{AppstoreBundleIdCapabilityEnsure, AppstoreBundleIdEnsure};
