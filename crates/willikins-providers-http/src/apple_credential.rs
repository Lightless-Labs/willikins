//! [`AppleSigningCredential`]: the App Store Connect API credential's
//! *assembly point* — three already-resolved parts (issuer id, key id,
//! EC P-256 private key) become one thing able to mint an ES256 JWT — and
//! [`AppleToken`], the JWT it mints.
//!
//! # Three ports, not one bundled triple
//!
//! `willikins_types::appstore`'s module doc states the correction this
//! module exists to carry through: the issuer id and the key id are
//! ordinary non-secret domain types
//! (`willikins_types::AppleIssuerId`/`AppleKeyId`), and only the private
//! key (`willikins_types::AppleSigningKey`) is secret. All three arrive
//! at this type's [`AppleSigningCredential::new`] as three independent,
//! already-resolved values — never read from the process environment by
//! this crate, never bundled into one input a document has to construct
//! together. A document is free to bind the two ids to a literal, a
//! workflow input, an `env.get` output, or a `doppler.value.get` output,
//! and the key to whatever secret-resolving chain the operator's own
//! vault needs (`doppler.secret.get` optionally through `base64.decode`,
//! ending at `apple.signing_key.parse`) — see the design addendum
//! "Credentials are ports, resolvers are nodes"
//! (`docs/plans/2026-09-11-willikins-design.md`). This struct is not
//! where that freedom lives; it is only the place downstream of it where
//! the three already-typed values get assembled into something that can
//! sign, the same way a future Apple tool's own credential-binding code
//! reads three typed input ports rather than one.
//!
//! # A sibling of [`Credential`], not a variant
//!
//! [`Credential`] is one bearer string, read once from an environment
//! variable (`Credential::from_env`) and attached to a request verbatim
//! (`authorize`/`authorize_header`) — see this crate's own module doc,
//! "Two kinds of secret". App Store Connect's credential does not fit
//! that shape on either end: it is never read from the environment (the
//! previous section), and Apple never accepts a static bearer token —
//! `Authorization: Bearer` must carry a self-signed ES256 JWT with a hard
//! 20-minute lifetime (`docs/research/2026-09-16-app-store-connect.md`),
//! so a credential that "just" holds a string and attaches it verbatim
//! cannot serve this provider at all. This type mints a fresh
//! [`AppleToken`] on every call to [`AppleSigningCredential::sign`]
//! rather than remembering one from `from_env`.
//!
//! Making this a `Credential` variant would mean branching `from_env`,
//! `authorize`, and `authorize_header` internally for a construction path
//! that does not read the environment and an attachment path that is not
//! "copy these bytes into a header" — every one of `Credential`'s
//! existing guarantees (single string, redacted `Debug`, no `Serialize`)
//! still needs to hold for GitHub and Doppler's tokens, and forking that
//! logic on an `enum` would make both harder to audit than two small
//! types. So this is a new, independent type: same crate, same "an
//! execution-context credential is never a domain type, never a graph
//! `Value`" rule (this file reads `willikins_types::AppleSigningKey`'s
//! bytes exactly once, through its `expose(&SinkToken)`, in
//! [`AppleSigningCredential::new`] — the same shape `Credential` itself
//! never needed because it is never handed a domain type at all), but
//! its own construction and its own one real method.
//!
//! # What this does not yet build
//!
//! There is no Apple HTTP client in this workspace yet — that is a
//! future provider crate, milestone 5's, mirroring
//! `willikins-providers-github`/`-doppler`'s `Client` types. This type's
//! [`AppleSigningCredential::sign`] is deliberately `pub`, not
//! crate-private the way `Credential::authorize` is: that method stays
//! private because `Http::apply_credential` is the *only* other thing in
//! this crate that ever attaches it, but no such integration exists for a
//! credential that must re-sign per call rather than being set once, so
//! there is nothing in this crate yet to keep it private *from*. Building
//! that integration (or the Apple client's own request path) is left to
//! the provider crate that needs it; recorded as a gap here rather than
//! guessed at.

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;

use willikins_types::SinkToken;
use willikins_types::{AppleIssuerId, AppleKeyId, AppleSigningKey};

/// The audience every App Store Connect API JWT must carry. Apple's own
/// documentation names this exact literal
/// (`docs/research/2026-09-16-app-store-connect.md`).
pub const AUDIENCE: &str = "appstoreconnect-v1";

/// This crate's own token lifetime: comfortably under Apple's 20-minute
/// hard ceiling, so a client can mint one token per request without
/// tuning anything, and a run long enough to need a second token gets one
/// well before the first would be rejected.
pub const TOKEN_LIFETIME_SECS: i64 = 15 * 60;

// Checked at compile time, unconditionally -- stronger than a test that
// only runs when someone runs the test suite: if this constant is ever
// widened past Apple's own ceiling, the build fails.
const _: () = assert!(TOKEN_LIFETIME_SECS < 20 * 60);

/// The App Store Connect API credential triple: issuer ID, key ID, and
/// the EC P-256 private key that signs a JWT locally. Only the key is
/// secret (`docs/research/2026-09-16-app-store-connect.md`): the issuer
/// ID and key ID both appear in cleartext in every token this type mints
/// (`iss` in the claims, `kid` in the header), so neither is redacted --
/// only [`Debug`] on the whole type is, so that a stray `{credential:?}`
/// still never prints the key.
pub struct AppleSigningCredential {
    issuer_id: String,
    key_id: String,
    key: EncodingKey,
}

/// The JWT claims this type mints. `aud` is always [`AUDIENCE`]; `iss`
/// and `kid` (the latter in the [`Header`], not here) come from the
/// credential; `iat`/`exp` come from whatever timestamp the caller
/// passes to [`AppleSigningCredential::sign`].
#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    iat: i64,
    exp: i64,
    aud: &'static str,
}

impl AppleSigningCredential {
    /// Build a credential from its already-resolved parts.
    ///
    /// Reads `key`'s PKCS#8 PEM bytes exactly once, through
    /// [`AppleSigningKey::expose`], to build the
    /// [`EncodingKey`] this type holds instead. `token` is not minted
    /// here: it is a real [`SinkToken`] the caller already holds (inside
    /// `Tool::ensure`, or a future Apple tool's own credential-binding
    /// code), passed through exactly as `github.actions_secret.ensure`
    /// passes one to `DomainObject::expose` — see that tool's module
    /// doc.
    ///
    /// # Errors
    ///
    /// Returns [`AppleCredentialError::InvalidKey`] if `key`'s PEM does
    /// not load as an EC key `jsonwebtoken` can sign with — which should
    /// not happen for a value [`AppleSigningKey::parse`] already
    /// accepted, since both ultimately require a valid P-256 key, but is
    /// still a `Result` rather than a panic: this constructor is the
    /// only place in the workspace that hands a parsed key's bytes to a
    /// second crypto library, and that library's own acceptance is not
    /// this crate's to assume.
    ///
    /// `issuer_id` and `key_id` are the two non-secret ports
    /// (`willikins_types::AppleIssuerId`/`AppleKeyId`) — a document may
    /// have bound either to a literal, a workflow input, or a resolver's
    /// output; this constructor does not care which. Only `key` came
    /// through a resolver chain ending at `apple.signing_key.parse`,
    /// because it is the one part `check` refuses to bind to a workflow
    /// input at all.
    pub fn new(
        issuer_id: &AppleIssuerId,
        key_id: &AppleKeyId,
        key: &AppleSigningKey,
        token: &SinkToken,
    ) -> Result<Self, AppleCredentialError> {
        let pem = key.expose(token);
        let key = EncodingKey::from_ec_pem(pem.as_bytes())
            .map_err(|_| AppleCredentialError::InvalidKey)?;
        Ok(Self {
            issuer_id: issuer_id.as_str().to_owned(),
            key_id: key_id.as_str().to_owned(),
            key,
        })
    }

    /// Mint a fresh ES256 JWT: header `alg: ES256`, `kid` the credential's
    /// key ID; claims `iss` the credential's issuer ID, `aud`
    /// [`AUDIENCE`], `iat` `now_unix`, `exp` `now_unix +
    /// `[`TOKEN_LIFETIME_SECS`].
    ///
    /// `now_unix` is a parameter rather than read from the system clock
    /// here, so a caller can pin it in a test and so a long-running
    /// caller controls its own re-sign cadence rather than this type
    /// silently deciding one. There is no cache: this type holds no
    /// previously minted token and no expiry to compare against, so
    /// every call re-mints unconditionally, including a call made one
    /// second before an earlier token (from an earlier call) would have
    /// expired — a long-running caller that wants to reuse a token until
    /// it is genuinely near Apple's ceiling decides that cadence itself,
    /// by choosing when to call this method again; this type never
    /// decides it for them by handing back a stale token.
    ///
    /// # Errors
    ///
    /// Returns [`AppleCredentialError::Signing`] if `jsonwebtoken` itself
    /// fails to encode — it does not inspect the claims or the key
    /// further than that.
    pub fn sign(&self, now_unix: i64) -> Result<AppleToken, AppleCredentialError> {
        let claims = Claims {
            iss: &self.issuer_id,
            iat: now_unix,
            exp: now_unix + TOKEN_LIFETIME_SECS,
            aud: AUDIENCE,
        };
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let jwt = encode(&header, &claims, &self.key).map_err(|_| AppleCredentialError::Signing)?;
        Ok(AppleToken(jwt))
    }
}

/// A minted ES256 App Store Connect API JWT: the compact
/// `header.claims.signature` string [`AppleSigningCredential::sign`]
/// produces.
///
/// Not a domain type — it is an execution-context value, like
/// [`AppleSigningCredential`] itself, never entering `willikins_types`'
/// registry or a graph `Value` — but it authenticates as the whole team,
/// for up to Apple's 20-minute ceiling, to anyone who reads it, so it
/// gets the same redaction discipline a graph secret gets by construction
/// rather than by a caller's memory: [`Debug`] prints a fixed redacted
/// marker and there is no [`std::fmt::Display`] at all, so a stray
/// `{token}` in a format string fails to compile rather than printing the
/// JWT, and a caller must go through [`AppleToken::as_str`] — which names
/// what it is doing — to reach the bytes it sets on an `Authorization:
/// Bearer` header.
///
/// The "no `Display`" half of that guarantee is structural, not a
/// runnable test (there is no such thing as a test that proves a trait
/// impl is absent): simply never writing `impl std::fmt::Display for
/// AppleToken` here is the whole mechanism, the same way
/// `willikins_types::sink`'s own module doc describes for `SinkToken::new`
/// gated by a cargo feature. `cargo clippy`'s `missing_docs`/`pedantic`
/// lints do not fill that gap either; only code review does.
#[derive(Clone, PartialEq, Eq)]
pub struct AppleToken(String);

impl AppleToken {
    /// The compact JWT string, for the one legitimate use: setting it on
    /// an `Authorization: Bearer` header.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AppleToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED AppleToken]")
    }
}

impl std::fmt::Debug for AppleSigningCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED AppleSigningCredential(kid={})]", self.key_id)
    }
}

/// Why [`AppleSigningCredential::new`] or
/// [`AppleSigningCredential::sign`] failed. Never carries the key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppleCredentialError {
    /// The key's PEM did not load as an EC key `jsonwebtoken` can sign
    /// with.
    #[error("the App Store Connect signing key could not be loaded")]
    InvalidKey,
    /// `jsonwebtoken::encode` itself failed.
    #[error("could not sign an App Store Connect API token")]
    Signing,
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_types::DomainType;

    fn token() -> SinkToken {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        SinkToken::new()
    }

    fn issuer_id() -> AppleIssuerId {
        AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").unwrap()
    }

    fn key_id() -> AppleKeyId {
        AppleKeyId::parse("2X9R4HXF34").unwrap()
    }

    fn credential() -> AppleSigningCredential {
        let key = AppleSigningKey::parse(AppleSigningKey::example()).unwrap();
        AppleSigningCredential::new(&issuer_id(), &key_id(), &key, &token()).unwrap()
    }

    /// Base64url-decode one dot-separated JWT segment to its JSON text,
    /// for asserting on claims/header content without a JWT-parsing
    /// dependency this crate does not otherwise need.
    fn decode_segment(segment: &str) -> String {
        String::from_utf8(
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, segment)
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn signs_a_well_formed_es256_jwt() {
        let jwt = credential().sign(1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.as_str().split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "a JWT has three dot-separated parts: {}",
            jwt.as_str()
        );

        let header_json = decode_segment(parts[0]);
        assert!(header_json.contains("\"alg\":\"ES256\""), "{header_json}");
        assert!(
            header_json.contains("\"kid\":\"2X9R4HXF34\""),
            "{header_json}"
        );

        let claims_json = decode_segment(parts[1]);
        assert!(
            claims_json.contains("\"iss\":\"57246542-96fe-1a63-e053-0824d011072a\""),
            "{claims_json}"
        );
        assert!(
            claims_json.contains("\"aud\":\"appstoreconnect-v1\""),
            "{claims_json}"
        );
        assert!(claims_json.contains("\"iat\":1700000000"), "{claims_json}");
        assert!(claims_json.contains(&format!("\"exp\":{}", 1_700_000_000 + TOKEN_LIFETIME_SECS)));
    }

    #[test]
    fn signing_twice_at_different_times_gives_different_tokens() {
        let credential = credential();
        let first = credential.sign(1_700_000_000).unwrap();
        let second = credential.sign(1_700_000_001).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn signing_one_second_before_the_ceiling_remints_rather_than_reusing() {
        // There is no cache field on `AppleSigningCredential`, so this is
        // true by construction -- but the test proves it at the
        // boundary the module doc calls out explicitly: a call made one
        // second before an earlier token's own `exp` still produces an
        // independent token with its own fresh `iat`/`exp`, not the
        // earlier token handed back.
        let credential = credential();
        let first = credential.sign(1_700_000_000).unwrap();
        let near_ceiling = 1_700_000_000 + TOKEN_LIFETIME_SECS - 1;
        let second = credential.sign(near_ceiling).unwrap();
        assert_ne!(first, second);

        let claims_json = decode_segment(second.as_str().split('.').nth(1).unwrap());
        assert!(
            claims_json.contains(&format!("\"iat\":{near_ceiling}")),
            "{claims_json}"
        );
        assert!(
            claims_json.contains(&format!("\"exp\":{}", near_ceiling + TOKEN_LIFETIME_SECS)),
            "{claims_json}"
        );
    }

    #[test]
    fn debug_never_shows_the_key() {
        let debug = format!("{:?}", credential());
        assert!(debug.contains("REDACTED"));
        assert!(
            debug.contains("2X9R4HXF34"),
            "kid is not secret and may appear: {debug}"
        );
        assert!(!debug.contains("BEGIN"), "{debug}");
    }

    #[test]
    fn token_debug_never_shows_the_jwt() {
        let jwt = credential().sign(1_700_000_000).unwrap();
        let debug = format!("{jwt:?}");
        assert_eq!(debug, "[REDACTED AppleToken]");
        assert!(!debug.contains(jwt.as_str()));
    }
}
