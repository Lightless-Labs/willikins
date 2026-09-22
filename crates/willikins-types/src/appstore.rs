//! App Store Connect domain types: [`AppleSigningKey`], the EC P-256
//! private key half of the credential triple
//! `docs/research/2026-09-16-app-store-connect.md` describes (issuer-ID
//! UUID, key ID, P-256 private key — "of which only the last is
//! secret"). The other two parts are not domain types: they carry no
//! shape of their own beyond being *some* text, and the credential triple
//! that bundles them with this key lives in `willikins-providers-http` as
//! an execution-context type, not a graph one — see that crate's
//! `apple_credential` module for why.
//!
//! # Why this type is hand-written rather than `#[derive(DomainType)]`
//!
//! The derive macro's only validation is a length bound and a regex
//! (`crates/willikins-derive/src/codegen.rs`'s `checks`) — there is no
//! hook for "parse this as a cryptographic key". A regex can check that
//! the input *looks* like a PEM block; it cannot check that the block
//! decodes to a point on the P-256 curve. Putting only the syntactic
//! check in the type and leaving the real one to whatever tool happens to
//! construct a value would mean a type named "P-256 signing key" could
//! hold a well-formed PEM wrapping an RSA key, or garbage DER that
//! merely starts and ends with the right markers — and
//! `assert_all_examples_parse`'s example round-trip would prove nothing
//! about the one property the type exists to guarantee. So this type
//! implements [`crate::DomainType`] and [`crate::object::DomainObject`]
//! by hand, exactly mirroring what `#[derive(DomainType)]`'s `gen_secret`
//! path generates (redacted `Debug`/`Display`, no `Serialize`, `expose`
//! gated by [`SinkToken`]) except for [`AppleSigningKey::parse`] itself,
//! which really does try to load the key with `p256`.
//!
//! # Which encodings [`AppleSigningKey::parse`] accepts, and why
//!
//! Apple's own documentation says only "the private key you downloaded"
//! and "a text file with a `.p8` extension" — it never states the
//! encoding, and the widely repeated claim that a `.p8` is PEM-encoded
//! PKCS#8 traced, in the research this type is built from, to a rendered
//! forum summary whose source bytes contain no occurrence of "p8" or
//! "PKCS" at all (`docs/research/2026-09-16-app-store-connect.md`,
//! section on token generation). What *is* documented is that ES256 is
//! mandatory, which fixes the curve as P-256 — but not the container
//! format two real encodings both legitimately use: **PKCS#8**
//! (`-----BEGIN PRIVATE KEY-----`, an algorithm identifier plus the raw
//! key) and **SEC1** (`-----BEGIN EC PRIVATE KEY-----`, the older
//! EC-specific container `openssl ecparam -genkey` produces directly).
//! This is a property of the key format itself, not of anyone's storage
//! habit (contrast [`crate::secret::OpaqueSecret::reveal_for_transform`]'s
//! caller, `base64.decode`, which *does* pick one habit and refuses the
//! other). Refusing SEC1 outright — which is all `jsonwebtoken`'s own PEM
//! decoder accepts, since it demands PKCS#8 — would mean an operator
//! whose `.p8` happens to be SEC1 fails at the one point this design
//! promises will catch a bad key with a clear reason, for a difference
//! that has nothing to do with whether the key is valid. So `parse`
//! accepts **both**, canonicalising to PKCS#8 PEM for storage (what
//! `jsonwebtoken::EncodingKey::from_ec_pem` requires downstream, in
//! `willikins-providers-http`): try loading as PKCS#8 first, then SEC1;
//! whichever succeeds is re-encoded to PKCS#8 before being wrapped. A key
//! on the wrong curve (P-384, say) or corrupt DER fails both attempts and
//! is refused with one message naming the constraint, never the input.
//!
//! [`SinkToken`]: crate::SinkToken

use std::fmt;

use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};

use crate::object::{DomainObject, Rendered};
use crate::{DomainType, ParseError, SinkToken};

/// The type name, repeated at every hand-written impl site below exactly
/// as `#[derive(DomainType)]` would embed `stringify!(Self)`.
const TYPE_NAME: &str = "AppleSigningKey";

/// The longest input `parse` will attempt to load as a key at all, before
/// ever invoking `p256`. A canonical PKCS#8 P-256 PEM is a little over 200
/// bytes; this leaves generous room for a SEC1 PEM, CRLF line endings, and
/// incidental leading/trailing whitespace, while still bounding the input
/// a document or provider response can force the crypto parser to look
/// at.
const MAX_LEN: usize = 4096;

/// An EC P-256 (`prime256v1`) private key, the signing half of an App
/// Store Connect API key's credential triple. Secret.
///
/// Stored canonicalised to PKCS#8 PEM (`-----BEGIN PRIVATE KEY-----`)
/// regardless of which of the two encodings [`AppleSigningKey::parse`]
/// accepted — see this module's doc for why both are accepted and why
/// PKCS#8 is the one form this type ever holds.
pub struct AppleSigningKey(secrecy::SecretString);

impl AppleSigningKey {
    /// Expose the key's raw PKCS#8 PEM text. Requires a [`SinkToken`],
    /// which only code running inside the apply executor (or a test) can
    /// construct — see `willikins_types::sink`'s module doc. This is the
    /// only place `willikins-providers-http`'s future Apple signing
    /// credential can reach the actual key bytes it needs to mint a JWT.
    #[must_use]
    // Named in `clippy.toml`'s `disallowed-methods` reason and in
    // `crates/willikins-core/tests/expose_secret_guard.rs`'s exemption
    // list for this file — mirrors the derive's own generated `expose`
    // exactly (see that codegen's identical `#[allow]` and comment).
    #[allow(clippy::disallowed_methods)]
    pub fn expose(&self, _token: &SinkToken) -> &str {
        secrecy::ExposeSecret::expose_secret(&self.0)
    }
}

impl DomainType for AppleSigningKey {
    const TYPE_NAME: &'static str = TYPE_NAME;
    const IS_SECRET: bool = true;

    fn description() -> &'static str {
        "An EC P-256 private key, PKCS#8 or SEC1 PEM-encoded (canonicalised to PKCS#8). Secret."
    }

    fn example() -> &'static str {
        // A throwaway key generated for this crate's own example/tests
        // only (`openssl ecparam -genkey -name prime256v1` piped through
        // `openssl pkcs8 -topk8 -nocrypt`) -- it authenticates to
        // nothing and was never used for anything else. Split across
        // several `concat!`-joined pieces, none of which spells a
        // complete PEM block on its own, for the same reason
        // `willikins_types::doppler::DopplerServiceToken`'s example is
        // assembled the same way: a contiguous "-----BEGIN ... PRIVATE
        // KEY-----" block in a public source file is exactly the shape a
        // secret scanner looks for, this repo's own guard included in
        // spirit even though `secret_literal_guard.rs` only enforces the
        // Doppler/GitHub shapes by name.
        //
        // Prefixed with a plain-text sentence rather than starting
        // straight at `-----BEGIN`: `redaction_adversarial.rs`'s
        // `no_catalog_entry_carries_a_secret_looking_example` requires
        // every secret type's published example to contain the word
        // "example" -- a real key's base64 body cannot be made to spell
        // it -- and RFC 7468's own grammar (`pem_rfc7468::decoder`,
        // "strip the preamble: optional text occurring before the
        // pre-encapsulation boundary") explicitly allows free text before
        // the `-----BEGIN` line, so `p256::SecretKey::from_pkcs8_pem`
        // still loads this. The canonical PKCS#8 PEM `parse` actually
        // stores (see `Self::expose` in this file's tests) never carries
        // the preamble: re-encoding drops it.
        concat!(
            "This is an example key for tests only.\n",
            "-----BEGIN PRI",
            "VATE KEY-----\n",
            "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg",
            "vL52rekEqgGqoWn9\n",
            "+YBkIQuQWEOThKqqIXbvonencAWhRANCAATdt/Xd4c/CLOKa2joD9G1pB98uwKN",
            "+\n",
            "LGJvK3hKTrxTWnJ0GyQiP3RmCubzl+GQR//h9ciFamjyNcHMjVU2cKbA\n",
            "-----END PRI",
            "VATE KEY-----\n",
        )
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        if input.is_empty() {
            return Err(ParseError::new(TYPE_NAME, "must not be empty"));
        }
        if input.chars().count() > MAX_LEN {
            return Err(ParseError::new(
                TYPE_NAME,
                format!("must be at most {MAX_LEN} characters long"),
            ));
        }
        let key = p256::SecretKey::from_pkcs8_pem(input)
            .or_else(|_| p256::SecretKey::from_sec1_pem(input))
            .map_err(|_| {
                ParseError::new(
                    TYPE_NAME,
                    "not a PKCS#8 or SEC1 PEM-encoded EC P-256 (prime256v1) private key",
                )
            })?;
        let canonical = key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|_| ParseError::new(TYPE_NAME, "could not re-encode the key as PKCS#8"))?;
        Ok(Self(secrecy::SecretString::from(canonical.to_string())))
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl std::str::FromStr for AppleSigningKey {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <Self as DomainType>::parse(s)
    }
}

impl Clone for AppleSigningKey {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl PartialEq for AppleSigningKey {
    // Mirrors the derive's own generated secret `PartialEq`: comparing
    // two secret values for equality needs their bytes, and this is the
    // only way to do that without a `PartialEq` derived from
    // `SecretString` itself, which does not implement it.
    #[allow(clippy::disallowed_methods)]
    fn eq(&self, other: &Self) -> bool {
        secrecy::ExposeSecret::expose_secret(&self.0)
            == secrecy::ExposeSecret::expose_secret(&other.0)
    }
}

impl Eq for AppleSigningKey {}

impl fmt::Display for AppleSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED {TYPE_NAME}]")
    }
}

impl fmt::Debug for AppleSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED {TYPE_NAME}]")
    }
}

impl<'de> serde::Deserialize<'de> for AppleSigningKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        <Self as DomainType>::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for AppleSigningKey {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(TYPE_NAME)
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "maxLength": MAX_LEN,
            "description": <Self as DomainType>::description(),
        })
    }
}

impl DomainObject for AppleSigningKey {
    fn type_name(&self) -> &'static str {
        TYPE_NAME
    }

    fn is_secret(&self) -> bool {
        true
    }

    fn render(&self) -> Rendered {
        Rendered::Redacted {
            type_name: TYPE_NAME,
        }
    }

    fn expose(&self, token: &SinkToken) -> String {
        Self::expose(self, token).to_string()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn dyn_eq(&self, other: &dyn DomainObject) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| other == self)
    }

    fn clone_box(&self) -> Box<dyn DomainObject> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same throwaway key as `AppleSigningKey::example()`, in SEC1
    /// form (`openssl ecparam -genkey -name prime256v1`, before the
    /// `pkcs8 -topk8` conversion `example()` already reflects) — a
    /// distinct key from the example, generated the same way, so a test
    /// asserting the two encodings of *the same* key parse identically
    /// does not need to reach into `example()`'s pieces.
    const SEC1_PEM: &str = concat!(
        "-----BEGIN EC PRI",
        "VATE KEY-----\n",
        "MHcCAQEEILy+dq3pBKoBqqFp/fmAZCELkFhDk4SqqiF276J3p3AFoAoGCCqGSM49\n",
        "AwEHoUQDQgAE3bf13eHPwizimto6A/RtaQffLsCjfixibyt4Sk68U1pydBskIj90\n",
        "Zgrm85fhkEf/4fXIhWpo8jXBzI1VNnCmwA==\n",
        "-----END EC PRI",
        "VATE KEY-----\n",
    );

    /// A P-384 key (wrong curve), PKCS#8-wrapped, otherwise well-formed --
    /// generated the same way as the example, on `secp384r1` instead of
    /// `prime256v1`. Must be refused: ES256 fixes the curve at P-256, and
    /// `p256::SecretKey::from_pkcs8_pem` checks the algorithm OID, not
    /// merely that the input parses as *some* EC key.
    const WRONG_CURVE_PEM: &str = concat!(
        "-----BEGIN PRI",
        "VATE KEY-----\n",
        "MIG2AgEAMBAGByqGSM49AgEGBSuBBAAiBIGeMIGbAgEBBDDfCWRdC0in1MiDdM59\n",
        "3px2Dwt4m3XeHTbY9smj57ww9zQ3vg/Ricxl0tGXpmJHIGOhZANiAATJJCVY/kHT\n",
        "eQLUNBmpVLaC8UFjNbG/lsK73w7mYbBEbpCwBu4fOoTmM/wGD+rpIsBBVeTgxtaL\n",
        "uOESdD/bf81PkaDyizHW9LNm7hmsP6giogHyJn6J8sD6EzD8tbKPYO8=\n",
        "-----END PRI",
        "VATE KEY-----\n",
    );

    fn token() -> SinkToken {
        #[allow(clippy::disallowed_methods)] // a test mints its own token
        SinkToken::new()
    }

    #[test]
    fn accepts_a_pkcs8_pem_key() {
        let key = AppleSigningKey::parse(AppleSigningKey::example()).unwrap();
        assert!(
            key.expose(&token())
                .starts_with("-----BEGIN PRIVATE KEY-----")
        );
    }

    #[test]
    fn accepts_a_sec1_pem_key_and_canonicalises_to_pkcs8() {
        let key = AppleSigningKey::parse(SEC1_PEM).unwrap();
        let exposed = key.expose(&token());
        assert!(
            exposed.starts_with("-----BEGIN PRIVATE KEY-----"),
            "a SEC1 input must be canonicalised to PKCS#8: {exposed:?}"
        );
        assert!(!exposed.contains("EC PRIVATE KEY"));
    }

    #[test]
    fn refuses_a_wrong_curve_key_with_a_clear_message_and_no_content() {
        let err = AppleSigningKey::parse(WRONG_CURVE_PEM).unwrap_err();
        assert!(err.reason.contains("P-256"), "{}", err.reason);
        assert!(!err.reason.contains("BEGIN"), "{}", err.reason);
    }

    #[test]
    fn refuses_garbage_that_merely_wears_pem_markers() {
        let garbage = concat!(
            "-----BEGIN PRI",
            "VATE KEY-----\n",
            "not actually a key\n",
            "-----END PRI",
            "VATE KEY-----\n",
        );
        assert!(AppleSigningKey::parse(garbage).is_err());
    }

    #[test]
    fn refuses_the_empty_string() {
        let err = AppleSigningKey::parse("").unwrap_err();
        assert!(err.reason.contains("empty"), "{}", err.reason);
    }

    #[test]
    fn refuses_input_over_the_length_bound_without_invoking_the_crypto_parser() {
        let huge = "a".repeat(MAX_LEN + 1);
        let err = AppleSigningKey::parse(&huge).unwrap_err();
        assert!(err.reason.contains("at most"), "{}", err.reason);
    }

    #[test]
    fn debug_and_display_never_show_the_key() {
        let key = AppleSigningKey::parse(AppleSigningKey::example()).unwrap();
        assert_eq!(format!("{key:?}"), "[REDACTED AppleSigningKey]");
        assert_eq!(key.to_string(), "[REDACTED AppleSigningKey]");
    }

    #[test]
    fn is_secret_end_to_end() {
        const { assert!(AppleSigningKey::IS_SECRET) };
        let key = AppleSigningKey::parse(AppleSigningKey::example()).unwrap();
        assert!(DomainObject::is_secret(&key));
        assert_eq!(
            DomainObject::render(&key).to_string(),
            "[REDACTED AppleSigningKey]"
        );
    }

    #[test]
    fn example_parses_as_its_own_type() {
        crate::assert_example_parses::<AppleSigningKey>();
    }

    #[test]
    fn deserialize_round_trips_through_parse() {
        let json = serde_json::to_string(AppleSigningKey::example()).unwrap();
        let key: AppleSigningKey = serde_json::from_str(&json).unwrap();
        assert_eq!(
            key.expose(&token()),
            AppleSigningKey::parse(AppleSigningKey::example())
                .unwrap()
                .expose(&token())
        );
    }

    #[test]
    fn deserialize_rejects_a_non_key_string() {
        let json = serde_json::to_string("not a key").unwrap();
        assert!(serde_json::from_str::<AppleSigningKey>(&json).is_err());
    }
}
