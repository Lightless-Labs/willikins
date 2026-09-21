//! Live `SigNoz` provider tools: `signoz.ingestion_key.ensure`.
//!
//! See `docs/research/2026-09-20-signoz-ingestion-keys.md` for every fact
//! this crate rests on, and `docs/plans/2026-09-11-willikins-design.md`'s
//! "Credentials are ports, resolvers are nodes" addendum (2026-09-21) for
//! this crate's credential port and its execution-context fallback.
//!
//! Shaped exactly like `willikins_providers_buildkite`: a client over
//! `willikins_providers_http` with its credential validated at
//! construction, the tenant host as configuration (never a graph port —
//! research section 2), one tool. The one structural difference from
//! every sibling live provider crate is the credential header: `SigNoz`'s
//! `OpenAPI` `securitySchemes` names `SigNoz-Api-Key`, not
//! `Authorization`, so this crate's [`client::http_client`] is the one
//! call site in the workspace that reaches for
//! `willikins_providers_http::Http::with_credential_header` instead of
//! `Http::new`.

mod client;
pub mod tools;

pub use client::{
    CREDENTIAL_HEADER, CREDENTIAL_PATTERN, CREDENTIAL_VAR, CreatedIngestionKey, HOST_VAR,
    IngestionKeyListEntry, SigNozClient, SigNozCredentialError, SigNozHostError, base_url_from_env,
    credential_from_env, http_client, looks_like_a_duplicate_name,
};
pub use tools::SigNozIngestionKeyEnsure;
