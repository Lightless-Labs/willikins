//! [`SigNozClient`]: typed calls for the one endpoint pair
//! `signoz.ingestion_key.ensure` needs.
//!
//! See `docs/research/2026-09-20-signoz-ingestion-keys.md` for every fact
//! this module rests on, confirmed live against the operator's own
//! account on 2026-09-21 (section header, and the computed task this
//! crate was built from). No tool ever builds a URL itself; the one path
//! segment this client ever interpolates is an already-validated
//! [`SigNozIngestionKeyName`], whose grammar
//! (`[A-Za-z0-9][A-Za-z0-9_-]*`) cannot smuggle a `/`, a `?`, or `&` into
//! the request line.
//!
//! **What was not re-verified live, and where that leaves this client.**
//! The research explicitly flags two open questions this client resolves
//! conservatively rather than guessing live behaviour it never observed:
//! whether `name` is filterable server-side via `GET
//! .../ingestion_keys/search` (this client always lists and filters
//! client-side instead — always correct, possibly one page short of
//! efficient) and whether the list endpoint paginates at all (this
//! client reads one response as the complete list, which is what every
//! confirmed fact describes and what the operator's seven-key account
//! exercises).

use serde::{Deserialize, Serialize};

use willikins_providers_http::{Credential, CredentialError, Http, ProviderError};
use willikins_types::{SigNozIngestionKeyName, SigNozIngestionKeyValue};

/// The environment variable a `SigNoz` [`Credential`] (the operator's own
/// **API key**, never an ingestion key — see this crate's module docs)
/// is read from.
pub const CREDENTIAL_VAR: &str = "WILLIKINS_SIGNOZ_API_KEY";

/// The environment variable this crate reads its tenant host from
/// (`docs/research/2026-09-20-signoz-ingestion-keys.md` section 2: the
/// region host is deployment configuration, never a literal or a graph
/// port). Holds a complete origin (`https://<tenant>.<region>.signoz.cloud`)
/// or a bare hostname (`<tenant>.<region>.signoz.cloud`) — see
/// [`base_url_from_env`], which prefixes a bare hostname with `https://`
/// before it becomes `Http`'s `base_url`.
pub const HOST_VAR: &str = "WILLIKINS_SIGNOZ_HOST";

/// The header `SigNoz`'s own `OpenAPI` `securitySchemes` names for its API
/// key scheme — never `Authorization` (research section 1).
pub const CREDENTIAL_HEADER: &str = "SigNoz-Api-Key";

/// The shape this crate accepts for provisioning. `SigNoz` documents no
/// published pattern for an API key's own bytes (unlike Doppler's or
/// GitHub's provider-published token grammars) — the research pass found
/// none, and none was invented here. This pattern is therefore
/// deliberately permissive: any non-trivial-length token of the
/// characters an HTTP header value can hold outright (no percent-encoding
/// needed), wide enough not to reject a real key, narrow enough that an
/// empty or single-character `WILLIKINS_SIGNOZ_API_KEY` still reports
/// [`CredentialError::Malformed`] rather than reaching the network.
/// Widened once against the operator's real sandbox key (2026-09-21, the
/// offline `tests/credential_env.rs` check — never against the value
/// itself, only against whether it matched): the initial pattern
/// (`[A-Za-z0-9._~-]`) refused it, so the standard base64 alphabet's two
/// extra symbols and its padding character (`+`, `/`, `=`) were added.
/// Still a shape guess, not a provider-published grammar; a tighter
/// pattern is a `todos/` item once one is.
pub const CREDENTIAL_PATTERN: &str = "^[A-Za-z0-9._~+/=-]{16,256}$";

/// Why [`credential_from_env`] refused to build a [`Credential`].
///
/// Never carries the environment variable's value, for the same reason
/// every sibling provider crate's own credential error does not.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SigNozCredentialError {
    /// The environment variable was unset or empty. See
    /// [`CredentialError::Missing`].
    #[error(transparent)]
    Missing(CredentialError),
    /// The environment variable was set but did not match
    /// [`CREDENTIAL_PATTERN`].
    #[error("a SigNoz API key is needed to provision (`{CREDENTIAL_VAR}`)")]
    Malformed,
}

/// Read [`CREDENTIAL_VAR`] from the process environment and validate it
/// against [`CREDENTIAL_PATTERN`].
///
/// # Errors
///
/// See [`Credential::from_env`], mapped through [`SigNozCredentialError`].
///
/// # Panics
///
/// Never in practice: [`CREDENTIAL_PATTERN`] is a fixed, compile-time-known
/// literal, and this module's own `tests` submodule builds a `Regex` from
/// the exact same constant, so `Regex::new` on it cannot fail.
pub fn credential_from_env() -> Result<Credential, SigNozCredentialError> {
    let pattern =
        regex::Regex::new(CREDENTIAL_PATTERN).expect("CREDENTIAL_PATTERN is a valid regex");
    Credential::from_env(CREDENTIAL_VAR, &pattern).map_err(|err| match err {
        CredentialError::Missing { .. } => SigNozCredentialError::Missing(err),
        CredentialError::Malformed { .. } => SigNozCredentialError::Malformed,
    })
}

/// Why [`base_url_from_env`] could not build a tenant host.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SigNozHostError {
    /// [`HOST_VAR`] was unset or empty.
    #[error("environment variable `{HOST_VAR}` is not set")]
    Missing,
}

/// Read [`HOST_VAR`] from the process environment, tolerating a bare
/// hostname (`tenant.region.signoz.cloud`) as well as a complete origin
/// (`https://tenant.region.signoz.cloud`): a value with no `://` is
/// prefixed with `https://` rather than sent to [`Http`] as-is, since a
/// bare hostname is not itself a valid base URL. This is a tolerance on
/// the *shape* of the variable's value, decided without reading what the
/// value actually is beyond that one substring check, so it never
/// contradicts this crate's own posture on not pre-inspecting credential
/// bytes -- `HOST_VAR` is not a secret, but the same discipline of
/// deciding from shape alone, not content, applies.
///
/// # Errors
///
/// Returns [`SigNozHostError::Missing`] when [`HOST_VAR`] is unset or
/// empty.
pub fn base_url_from_env() -> Result<String, SigNozHostError> {
    match std::env::var(HOST_VAR) {
        Ok(value) if !value.is_empty() => Ok(with_scheme(&value)),
        _ => Err(SigNozHostError::Missing),
    }
}

/// Prefix `host` with `https://` unless it already names a scheme.
fn with_scheme(host: &str) -> String {
    if host.contains("://") {
        host.to_string()
    } else {
        format!("https://{host}")
    }
}

/// Build an [`Http`] against `SigNoz`'s real API at `base_url`, carrying
/// `credential` under [`CREDENTIAL_HEADER`] rather than `Authorization`
/// (research section 1) — the one call in this crate that chooses
/// [`Http::with_credential_header`] over [`Http::new`].
#[must_use]
pub fn http_client(base_url: impl Into<String>, credential: Credential) -> Http {
    Http::with_credential_header(base_url, Vec::new(), credential, Some(CREDENTIAL_HEADER))
}

/// A typed `SigNoz` REST client, bound to one [`Http`] (which itself owns
/// the [`Credential`] and the tenant host).
pub struct SigNozClient {
    http: Http,
}

impl SigNozClient {
    /// Build a client over `http`.
    #[must_use]
    pub fn new(http: Http) -> Self {
        Self { http }
    }

    /// `GET /api/v2/gateway/ingestion_keys`.
    ///
    /// Deserializes only `id` and `name` from each listed entry — never
    /// `value`, which the endpoint does return (research section 3, the
    /// fact its own `OpenAPI` schema omits) but which this crate
    /// deliberately never reads back; see `signoz.ingestion_key.ensure`'s
    /// module docs for the decision and its reasoning. `serde`'s default
    /// behaviour (no `deny_unknown_fields`) discards every other field,
    /// `value` included, before it ever becomes a `String` in this
    /// process. `id` is kept (unlike `value`) because the live write
    /// cycle's own cleanup needs it: `signoz.ingestion_key.ensure` itself
    /// still never reads it, matching decision (a) of its own module
    /// docs -- the tool's key is the name, not the id, because the id is
    /// only known after creation.
    ///
    /// `pub`, not `pub(crate)`: the opt-in live write cycle
    /// (`tests/live_write_cycle.rs`) lives in a separate crate, the same
    /// reason `willikins_providers_buildkite::BuildkiteClient::delete_pipeline`
    /// is `pub`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] for any non-2xx response or a transport
    /// failure.
    pub fn list_ingestion_keys(&self) -> Result<Vec<IngestionKeyListEntry>, ProviderError> {
        self.http
            .get::<IngestionKeyListEnvelope>("/api/v2/gateway/ingestion_keys")
            .map(|envelope| envelope.data)
    }

    /// `POST /api/v2/gateway/ingestion_keys` with exactly `name` — no
    /// `expires_at`, no `tags` (both documented optional create fields;
    /// neither is a port this milestone's tool exposes, matching
    /// `buildkite.pipeline.ensure`'s own "no field beyond what a port
    /// names" discipline). Never retried: minting is a `POST`, and the
    /// belt-and-braces `409` path re-lists rather than trusting a
    /// retried, ambiguous failure.
    ///
    /// `pub` for the same reason as [`Self::list_ingestion_keys`]: the
    /// live write cycle calls it directly to seed the one key it then
    /// deletes.
    ///
    /// # Errors
    ///
    /// See [`Self::list_ingestion_keys`]. A `409` is returned like any
    /// other status; the caller (`signoz.ingestion_key.ensure::ensure`)
    /// is the one place that inspects it.
    pub fn create_ingestion_key(
        &self,
        name: &SigNozIngestionKeyName,
    ) -> Result<CreatedIngestionKey, ProviderError> {
        let body = CreateIngestionKeyBody {
            name: name.to_string(),
        };
        self.http
            .post::<CreateIngestionKeyEnvelope>("/api/v2/gateway/ingestion_keys", &body)
            .map(|envelope| envelope.data)
    }

    /// `DELETE /api/v2/gateway/ingestion_keys/{keyId}`, answering `204`
    /// (research section 2's operation table).
    ///
    /// **Used only by the opt-in live write cycle**
    /// (`tests/live_write_cycle.rs`), to remove the key it created — no
    /// tool in this crate calls it, matching
    /// `BuildkiteClient::delete_pipeline`'s own scope note. `pub` for the
    /// same reason that method is.
    ///
    /// # Errors
    ///
    /// See [`Self::list_ingestion_keys`].
    pub fn delete_ingestion_key(&self, id: &str) -> Result<(), ProviderError> {
        let path = format!("/api/v2/gateway/ingestion_keys/{id}");
        self.http.delete(&path)
    }
}

/// Whether `err` is the documented `409` "already exists" conflict
/// (research section 3): `SigNoz`'s own `409` body shape (`{"status":
/// "error", "error": {"type": "already-exists", ...}}`) is a singular
/// `error` object, not the `errors[]` array shape
/// [`willikins_providers_http::http`]'s own `already_exists` detection
/// recognises (that shape is GitHub's) — so this crate does not lean on
/// [`ProviderError::already_exists`] at all, and checks the one fact
/// that *is* documented and already carried on every `ProviderError`:
/// the status code alone. `willikins-providers-doppler`'s
/// `looks_like_a_missing_project` reads a message substring for the same
/// kind of reason (its provider's `400`s are ambiguous by status alone);
/// this one does not need to, because `SigNoz`'s `409` has exactly one
/// documented meaning.
#[must_use]
pub fn looks_like_a_duplicate_name(err: &ProviderError) -> bool {
    err.status == Some(409)
}

/// One entry of `GET /api/v2/gateway/ingestion_keys`'s `data` array —
/// `id` and `name` only. See [`SigNozClient::list_ingestion_keys`]'s doc
/// comment for why `value` (and every other field the response carries)
/// is deliberately absent from this struct rather than merely unread,
/// and why `id`, unlike `value`, is kept.
#[derive(Debug, Clone, Deserialize)]
pub struct IngestionKeyListEntry {
    /// The key's opaque id, needed only by the live write cycle's own
    /// cleanup (`SigNozClient::delete_ingestion_key`).
    pub id: String,
    /// The key's name — the tool's own natural key.
    pub name: String,
}

/// `GET /api/v2/gateway/ingestion_keys`'s envelope: `{"status": ...,
/// "data": [...]}` (research section 3's documented response shape,
/// generalised from create's own `{status, data}` envelope).
#[derive(Debug, Deserialize)]
struct IngestionKeyListEnvelope {
    data: Vec<IngestionKeyListEntry>,
}

#[derive(Debug, Serialize)]
struct CreateIngestionKeyBody {
    name: String,
}

/// `POST /api/v2/gateway/ingestion_keys`'s `201` `data` object (research
/// section 3): `id` and `value`, both required. `id` is deserialized but
/// never read past construction, the same `#[allow(dead_code)]` shape
/// `willikins_providers_buildkite::client::PipelineBody` uses for its own
/// unread-but-traceable fields — kept so this struct's shape stays
/// traceable, field for field, to the documented response. `value`
/// deserializes straight into [`SigNozIngestionKeyValue`], so a create
/// response whose `value` does not parse fails right here, inside
/// `Http::finish`'s deliberately content-free parse-error path, rather
/// than ever existing as a plain `String` this crate would have to
/// remember not to log.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CreatedIngestionKey {
    /// The newly minted key's opaque id, needed only by the live write
    /// cycle's own cleanup.
    pub id: String,
    /// The newly minted key's value. `pub` for the live write cycle,
    /// which must hold it just long enough to prove it parses and then
    /// drop it -- `signoz.ingestion_key.ensure` itself never reads this
    /// field back through any path but the create response it came from.
    pub value: SigNozIngestionKeyValue,
}

/// `POST /api/v2/gateway/ingestion_keys`'s envelope: `{"status": ...,
/// "data": {...}}` (research section 3).
#[derive(Debug, Deserialize)]
struct CreateIngestionKeyEnvelope {
    data: CreatedIngestionKey,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_scheme_prefixes_a_bare_hostname() {
        assert_eq!(
            with_scheme("tenant.region.signoz.cloud"),
            "https://tenant.region.signoz.cloud"
        );
    }

    #[test]
    fn with_scheme_leaves_a_complete_origin_alone() {
        assert_eq!(
            with_scheme("https://tenant.region.signoz.cloud"),
            "https://tenant.region.signoz.cloud"
        );
        assert_eq!(
            with_scheme("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080"
        );
    }

    fn pattern() -> regex::Regex {
        regex::Regex::new(CREDENTIAL_PATTERN).expect("CREDENTIAL_PATTERN is a valid regex")
    }

    #[test]
    fn accepts_a_plausible_length_key() {
        let key = "a".repeat(32);
        assert!(pattern().is_match(&key), "{key}");
    }

    #[test]
    fn accepts_the_standard_base64_alphabets_extra_symbols() {
        let key = format!("{}+/=", "a".repeat(29));
        assert!(pattern().is_match(&key), "{key}");
    }

    #[test]
    fn rejects_a_key_that_is_too_short() {
        let key = "a".repeat(8);
        assert!(!pattern().is_match(&key), "{key}");
    }

    #[test]
    fn rejects_a_trailing_newline() {
        let key = format!("{}\n", "a".repeat(32));
        assert!(!pattern().is_match(&key), "{key:?}");
    }

    #[test]
    fn looks_like_a_duplicate_name_matches_only_status_409() {
        let duplicate = ProviderError::new(Some(409), "key: willikins-example already exists");
        assert!(looks_like_a_duplicate_name(&duplicate));

        let not_409 = ProviderError::new(Some(404), "not found");
        assert!(!looks_like_a_duplicate_name(&not_409));

        let transport = ProviderError::new(None, "timed out");
        assert!(!looks_like_a_duplicate_name(&transport));
    }
}
