//! App Store Connect domain types: [`AppleIssuerId`], [`AppleKeyId`] and
//! [`AppleSigningKey`], the three parts of the credential triple
//! `docs/research/2026-09-16-app-store-connect.md` describes (issuer-ID
//! UUID, key ID, P-256 private key — "of which only the last is
//! secret"). All three are domain types and all three are ordinary graph
//! ports: the issuer id and key id are non-secret and carry real
//! grammars of their own (a UUID; a short alphanumeric identifier), so
//! each is free to be bound to a literal, a workflow input, an
//! `env.get` output or a `doppler.value.get`/`doppler.secret.get` output,
//! at the document author's choice — never something bundled inside a
//! credential struct, because a part that is not a domain type cannot be
//! a port, and a part that is not a port can only come from the
//! execution context, which couples every document to wherever *one*
//! operator keeps their ids. `willikins-providers-http`'s
//! `apple_credential` module is the assembly point that reads all three
//! already-resolved ports and mints a JWT from them; see that module's
//! doc for why the assembly point itself is an execution-context type,
//! not a graph one, while the parts it assembles are graph types.
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
use std::str::FromStr;

use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};

use crate::name::is_invisible_or_bidi_control;
use crate::object::{DomainObject, Rendered};
use crate::{DomainType, ParseError, SinkToken};

/// An App Store Connect API key's issuer id: a UUID, team keys only
/// (`docs/research/2026-09-16-app-store-connect.md`, section 1 — Apple's
/// own example is `57246542-96fe-1a63-e053-0824d011072a`). Not secret: it
/// appears in cleartext as the `iss` claim of every JWT this credential
/// mints.
///
/// Grammar and derive choice both follow [`crate::BuildkiteClusterId`]
/// exactly: lowercase hex only (a mis-cased id would compare unequal to
/// whatever a document or provider response holds), and no separate
/// `min_len`/`max_len` because the pattern's five hyphen-separated hex
/// groups (8-4-4-4-12) already fix the length at 36 characters. A new
/// type rather than a reuse of `BuildkiteClusterId` itself: both are
/// UUIDs today, but a type name at a port carries provider meaning, and
/// a `BuildkiteClusterId` binding to an Apple issuer-id port by accident
/// — because the underlying grammars happen to coincide — is exactly
/// what a distinct type name at each port exists to rule out. `#[derive(
/// DomainType)]` is used, not hand-written: unlike [`AppleSigningKey`],
/// nothing about a UUID needs semantic validation a regex cannot express
/// (checking a point lies on a curve, say) — a pattern match is the whole
/// grammar.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
    description = "An App Store Connect API key's issuer id (a UUID, team keys only).",
    example = "57246542-96fe-1a63-e053-0824d011072a"
)]
pub struct AppleIssuerId(String);

/// An App Store Connect API key's key id: goes in the JWT header's `kid`
/// (`docs/research/2026-09-16-app-store-connect.md`, section 1 — Apple's
/// own example is `2X9R4HXF34`). Not secret: it appears in cleartext in
/// the header of every JWT this credential mints.
///
/// Apple documents no grammar for this id at all — the research note's
/// "Unresolved" list is silent on it, and no help or specification page
/// fetched there states a length or character set. The pattern here is
/// the shape Apple's own examples and every widely observed real key id
/// use — uppercase letters and digits, ten characters — following
/// [`crate::BuildkiteOrg`]'s precedent for a grammar Apple never
/// publishes: chosen conservatively from the shape the provider issues,
/// not invented. Bounded a little wider than the observed examples --
/// the pattern's own `{2,32}` quantifier, not a separate `max_len` --
/// so a real key id in a length Apple has not been observed to use is
/// not refused by a type that guessed too narrowly; a bundle
/// identifier's own undocumented grammar
/// (`docs/research/2026-09-16-app-store-connect.md`, section 2) is the
/// cautionary tale against pretending an unstated grammar is exact.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[A-Z0-9]{2,32}",
    description = "An App Store Connect API key's key id (goes in the JWT `kid` header).",
    example = "2X9R4HXF34"
)]
pub struct AppleKeyId(String);

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

    /// Apply `f` to this key's raw PKCS#8 PEM bytes, producing whatever
    /// `f` produces — token-less, unlike [`Self::expose`].
    ///
    /// # Why this exists, and why it is safe despite taking no
    /// [`SinkToken`]
    ///
    /// `willikins_types::secret`'s module doc establishes one token-less
    /// exception (`OpaqueSecret::reveal_for_transform`, and
    /// `DopplerSecretValue`'s twin) for a pure transform or parse tool's
    /// `Tool::read`, which never receives a `SinkToken` — and that doc is
    /// explicit that the exception is **not** for "a tool that does real
    /// work" (a tool touching a real resource takes a *specific* secret
    /// domain type, never `OpaqueSecret`, precisely so a reviewer can see
    /// what the value actually is).
    ///
    /// `appstore.bundle_id.ensure` and
    /// `appstore.bundle_id_capability.ensure` are exactly that kind of
    /// real-work tool, and they hit the same wall from a different
    /// direction: their `key` input is a real [`AppleSigningKey`], bound
    /// through a resolver chain ending at this type's own `parse`
    /// (`docs/plans/2026-09-11-willikins-design.md`'s "Credentials are
    /// ports, resolvers are nodes" addendum), and their `Tool::read`
    /// must mint a JWT from it to make the authenticated `GET` that
    /// `plan` depends on — `read` never receives a `SinkToken` either.
    /// The key is not being *moved* anywhere a document or an agent
    /// could read it back: it is being *used as a credential* to
    /// authorize one outbound request, precisely the role
    /// `willikins_providers_http::Credential::authorize` already plays
    /// for every other provider's `read`, with a real, non-`Debug`,
    /// non-`Display` [`willikins_providers_http::AppleToken`] as the
    /// only thing that ever leaves the closure `f` runs in. That is a
    /// narrower claim than "any secret may flow through here" — it is
    /// "this specific type's bytes may be used, once, to sign" — so it
    /// earns its own named exception rather than reusing
    /// `reveal_for_transform`'s (which stays scoped to
    /// [`crate::OpaqueSecret`] and [`crate::doppler::DopplerSecretValue`]
    /// alone, as that method's own doc says).
    ///
    /// [`SinkToken`]: crate::SinkToken
    // The third production call site of `expose_secret` outside the
    // derive's own codegen -- named in `clippy.toml`'s
    // `disallowed-methods` reason and walked by
    // `crates/willikins-core/tests/expose_secret_guard.rs`, which exempts
    // exactly this function (alongside `expose` and `eq`) in this file.
    #[allow(clippy::disallowed_methods)]
    pub fn reveal_for_signing<T, E>(&self, f: impl FnOnce(&str) -> Result<T, E>) -> Result<T, E> {
        f(secrecy::ExposeSecret::expose_secret(&self.0))
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

// ---------------------------------------------------------------------
// Bundle identifier and capability types: `appstore.bundle_id.ensure`
// and `appstore.bundle_id_capability.ensure`'s own ports.
// `docs/research/2026-09-16-app-store-connect.md`, section 2, is every
// fact these types rest on.
// ---------------------------------------------------------------------

/// A bundle identifier string, such as `com.example.MyApp`: the natural
/// key `appstore.bundle_id.ensure` reads and creates by
/// (`filter[identifier]`, compared byte-for-byte client-side since the
/// filter's own matching semantics are undocumented -- research note,
/// section 2). Immutable once created (`BundleIdUpdateRequest` declares
/// only `name`): a document that gets this wrong creates a second,
/// permanent identifier rather than converging the first.
///
/// Apple documents no format for this string at all -- not a `pattern`,
/// not a `maxLength`, verified programmatically across the create schema
/// (research note, section 2). The pattern and the 255-character bound
/// below are chosen conservatively from the shape every Apple example
/// and real-world bundle id uses (reverse-DNS: `com.example.MyApp`),
/// following [`crate::BuildkiteOrg`]'s precedent for an undocumented
/// grammar -- generous enough that a real identifier is not refused by a
/// type that guessed too narrowly, not a claim that Apple enforces this
/// bound. Verify against a live `POST` before relying on the exact
/// figure (research note's own "Unresolved" list, same section).
///
/// Not secret -- it is the one part of a bundle id record every reader
/// of App Store Connect's UI already sees.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*",
    max_len = 255,
    description = "An App Store Connect bundle identifier string, such as `com.example.MyApp`. Immutable once created.",
    example = "com.example.MyApp"
)]
pub struct AppleBundleIdentifier(String);

/// The maximum length of an [`AppleBundleIdName`], in characters.
const BUNDLE_ID_NAME_MAX_LEN: usize = 255;

/// A bundle id's human-written `name` attribute: the *only* free-text
/// attribute App Store Connect's bundle id resource has (research note,
/// section 2: "the ownership marker willikins uses everywhere else ...
/// has no slot here"). This crate's own design decision for that gap: `name` is an
/// ordinary port, compared exactly on `read`, and a differing `name`
/// converges through `PATCH` rather than reading `Foreign` — there is no
/// `Foreign` observation for this resource at all (contrast
/// `buildkite.pipeline.ensure`'s ownership marker in its `description`
/// field). This is a real, stated trade: two different callers naming
/// the same `identifier` with different `name`s will silently rewrite
/// each other's, rather than conflict, exactly the gap the research note
/// warns "either the marker lives in `name` or `Foreign` is
/// undetectable" describes. `appstore.bundle_id.ensure`'s own module doc
/// repeats this.
///
/// Hand-written, mirroring [`crate::BuildkiteClusterName`] exactly: 1 to
/// 255 characters (Apple states no bound; this is this crate's own
/// conservative choice), no control character and none of the invisible
/// or bidirectional characters [`is_invisible_or_bidi_control`] rejects
/// -- a `name` reaches a `PATCH` body and a rendered output alike.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppleBundleIdName(String);

impl AppleBundleIdName {
    /// The name text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppleBundleIdName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for AppleBundleIdName {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for AppleBundleIdName {
    const TYPE_NAME: &'static str = "AppleBundleIdName";

    fn description() -> &'static str {
        "A bundle id's human-written `name` attribute."
    }

    fn example() -> &'static str {
        "third-thoughts"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        if input.is_empty() {
            return Err(ParseError::new(Self::TYPE_NAME, "must not be empty"));
        }
        let len = input.chars().count();
        if len > BUNDLE_ID_NAME_MAX_LEN {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("is {len} characters, the limit is {BUNDLE_ID_NAME_MAX_LEN}"),
            ));
        }
        if let Some(c) = input.chars().find(|c| c.is_control()) {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("must not contain control characters (found {c:?})"),
            ));
        }
        if let Some(c) = input.chars().find(|&c| is_invisible_or_bidi_control(c)) {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!(
                    "must not contain invisible or bidirectional control character (found {c:?})"
                ),
            ));
        }
        Ok(Self(input.to_owned()))
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for AppleBundleIdName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for AppleBundleIdName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for AppleBundleIdName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("AppleBundleIdName")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": BUNDLE_ID_NAME_MAX_LEN,
            "description": "A bundle id's human-written `name` attribute.",
            "examples": ["third-thoughts"]
        })
    }
}

crate::impl_domain_object_non_secret!(AppleBundleIdName);

/// A bundle id's `platform`: a closed three-member enum
/// (`BundleIdPlatform` in Apple's own schema — research note, section
/// 2). `UNIVERSAL` is Apple's own guidance for a single App ID shared
/// across platforms; there is no `TV_OS` or `WATCH_OS` member. Immutable
/// once created, exactly like [`AppleBundleIdentifier`] — a platform
/// mismatch on `read` is a terminal [`willikins_core::Observation::Mismatch`]
/// (no `PATCH` can repair it).
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "IOS|MAC_OS|UNIVERSAL",
    description = "An App Store Connect bundle id's platform: IOS, MAC_OS, or UNIVERSAL. Immutable once created.",
    example = "UNIVERSAL"
)]
pub struct AppleBundleIdPlatform(String);

/// A bundle id's Apple-assigned opaque record id -- the handle a
/// downstream tool (`appstore.bundle_id_capability.ensure`'s own
/// `read`, internally; a future certificate or profile tool) needs once
/// a bundle id is known to exist. Apple documents no grammar for this
/// id at all; every observed example is a short run of uppercase
/// letters and digits (the same shape [`crate::AppleKeyId`] uses, for
/// the same undocumented-grammar reason its own doc gives), so this
/// pattern is chosen the same way, generously bounded above the
/// observed length rather than pinned to it.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[A-Za-z0-9]{2,64}",
    description = "An App Store Connect bundle id's Apple-assigned opaque record id.",
    example = "T6G4XCV345"
)]
pub struct AppleBundleIdId(String);

/// A bundle id capability's `capabilityType`: the closed 28-member enum
/// Apple's specification declares (research note, section 2, quoting
/// `CapabilityType`'s full enum verbatim). Every member is reproduced
/// here exactly as the specification spells it, in the specification's
/// own order, so a reviewer can diff the two directly.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "ICLOUD|IN_APP_PURCHASE|GAME_CENTER|PUSH_NOTIFICATIONS|WALLET|INTER_APP_AUDIO|MAPS|ASSOCIATED_DOMAINS|PERSONAL_VPN|APP_GROUPS|HEALTHKIT|HOMEKIT|WIRELESS_ACCESSORY_CONFIGURATION|APPLE_PAY|DATA_PROTECTION|SIRIKIT|NETWORK_EXTENSIONS|MULTIPATH|HOT_SPOT|NFC_TAG_READING|CLASSKIT|AUTOFILL_CREDENTIAL_PROVIDER|ACCESS_WIFI_INFORMATION|NETWORK_CUSTOM_PROTOCOL|COREMEDIA_HLS_LOW_LATENCY|SYSTEM_EXTENSION_INSTALL|USER_MANAGEMENT|APPLE_ID_AUTH",
    description = "An App Store Connect bundle id capability type.",
    example = "PUSH_NOTIFICATIONS"
)]
pub struct AppleCapabilityType(String);

// ---------------------------------------------------------------------
// Certificate types: `appstore.certificate.get`'s own ports. Milestone
// 3c (`docs/plans/2026-09-22-milestone-3c-app-store-signing.md`, "the
// type table") is every fact these three types rest on; the two research
// notes it cites are every fact *they* rest on.
// ---------------------------------------------------------------------

/// A certificate's `certificateType`: the two members of Apple's
/// eighteen-member `CertificateType` enum that can sign an
/// `IOS_APP_STORE` profile (the in-scope profile type a later milestone
/// task adds) -- `DISTRIBUTION` ("Apple Distribution", observed live: the
/// operator's one usable certificate's `name` begins with that label) and
/// `IOS_DISTRIBUTION` ("iOS Distribution", documented but never observed
/// live -- no such certificate exists on the team; carried forward as
/// milestone 3c's verify item 8). Every other of the eighteen (`APPLE_PAY`,
/// `DEVELOPMENT`, `MAC_APP_DISTRIBUTION`, and so on) is refused at parse
/// time: the refusal of every non-distribution type is the grammar.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "DISTRIBUTION|IOS_DISTRIBUTION",
    description = "An App Store Connect certificate type: DISTRIBUTION or IOS_DISTRIBUTION (the two that can sign an App Store distribution profile).",
    example = "DISTRIBUTION"
)]
pub struct AppleCertificateType(String);

/// A certificate's `serialNumber`: the one part of a certificate record
/// visible from the *private-key* side (it is in the `.p12` a signer
/// holds -- `openssl x509 -serial` -- and in Keychain Access), which is
/// why `appstore.certificate.get` selects by this rather than by the
/// opaque [`AppleCertificateId`] or the team-wide-identical `displayName`
/// (milestone 3c, decision (c)).
///
/// Observed live on all 5 certificates on the operator's team: uppercase
/// hexadecimal, 30 to 32 characters. Uppercase only, deliberately: the
/// selection read compares `serialNumber` byte-for-byte (`filter[serialNumber]`
/// is proven substring, milestone 3c's pre-flight), so a lowercase paste
/// of a real serial would compare unequal and read `NotFound` for a
/// certificate that exists, rather than silently normalising a case Apple
/// itself never issues. The `{1,64}` bound is generous above the observed
/// length, the same undocumented-grammar caution [`AppleKeyId`]'s own doc
/// states; this value also reaches a query string, so the grammar keeps
/// `&` and `=` out of it by construction (the character class admits
/// neither).
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[0-9A-F]{1,64}",
    description = "An App Store Connect certificate's serial number (uppercase hexadecimal).",
    example = "7B3F2A9C1D4E5F607182930A1B2C3D4E5F60"
)]
pub struct AppleCertificateSerial(String);

/// A certificate's Apple-assigned opaque record id -- what
/// `appstore.certificate.get` outputs once its selection (`certificate_type`
/// + `serial_number`) resolves to exactly one match. Apple documents no
/// grammar for this id at all; the same undocumented-grammar reasoning as
/// [`AppleBundleIdId`] and [`AppleKeyId`] applies, so this pattern is
/// chosen the same conservative way rather than pinned to any one
/// observed example.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[A-Za-z0-9]{2,64}",
    description = "An App Store Connect certificate's Apple-assigned opaque record id.",
    example = "C3RT1F1CATE9"
)]
pub struct AppleCertificateId(String);

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

    #[test]
    fn issuer_id_and_key_id_are_registered_and_not_secret() {
        // The property the whole "credentials are ports, resolvers are
        // nodes" design rests on for these two parts: only the key is
        // secret, so only the key may ever refuse a workflow input.
        const { assert!(!AppleIssuerId::IS_SECRET) };
        const { assert!(!AppleKeyId::IS_SECRET) };
        let registry = crate::registry();
        assert_eq!(
            registry.is_secret(&crate::TypeName::parse("AppleIssuerId").unwrap()),
            Some(false)
        );
        assert_eq!(
            registry.is_secret(&crate::TypeName::parse("AppleKeyId").unwrap()),
            Some(false)
        );
    }

    #[test]
    fn issuer_id_accepts_apples_own_example() {
        assert!(AppleIssuerId::parse("57246542-96fe-1a63-e053-0824d011072a").is_ok());
    }

    #[test]
    fn issuer_id_refuses_uppercase_and_non_uuid_shapes() {
        assert!(AppleIssuerId::parse("57246542-96FE-1a63-e053-0824d011072a").is_err());
        assert!(AppleIssuerId::parse("not-a-uuid").is_err());
    }

    #[test]
    fn key_id_accepts_apples_own_example() {
        assert!(AppleKeyId::parse("2X9R4HXF34").is_ok());
    }

    #[test]
    fn key_id_refuses_lowercase_and_punctuation() {
        assert!(AppleKeyId::parse("2x9r4hxf34").is_err());
        assert!(AppleKeyId::parse("2X9R-4HXF34").is_err());
    }

    #[test]
    fn issuer_id_and_key_id_examples_parse_as_their_own_types() {
        crate::assert_example_parses::<AppleIssuerId>();
        crate::assert_example_parses::<AppleKeyId>();
    }

    // -------------------------------------------------------------
    // `AppleSigningKey::reveal_for_signing`
    // -------------------------------------------------------------

    #[test]
    fn reveal_for_signing_hands_the_same_bytes_expose_does() {
        let key = AppleSigningKey::parse(AppleSigningKey::example()).unwrap();
        let via_expose = key.expose(&token()).to_string();
        let via_reveal = key
            .reveal_for_signing(|pem| Ok::<_, std::convert::Infallible>(pem.to_string()))
            .unwrap();
        assert_eq!(via_expose, via_reveal);
    }

    #[test]
    fn reveal_for_signing_lets_the_closures_error_escape() {
        let key = AppleSigningKey::parse(AppleSigningKey::example()).unwrap();
        let err = key
            .reveal_for_signing(|_pem| Err::<(), &'static str>("refused"))
            .unwrap_err();
        assert_eq!(err, "refused");
    }

    // -------------------------------------------------------------
    // Bundle identifier and capability types
    // -------------------------------------------------------------

    #[test]
    fn bundle_identifier_accepts_a_reverse_dns_string() {
        assert!(AppleBundleIdentifier::parse("com.example.MyApp").is_ok());
    }

    #[test]
    fn bundle_identifier_refuses_a_slash_or_leading_dot() {
        assert!(AppleBundleIdentifier::parse("com/example").is_err());
        assert!(AppleBundleIdentifier::parse(".com.example").is_err());
        assert!(AppleBundleIdentifier::parse("").is_err());
    }

    #[test]
    fn bundle_identifier_is_registered_and_not_secret() {
        const { assert!(!AppleBundleIdentifier::IS_SECRET) };
        assert_eq!(
            crate::registry().is_secret(&crate::TypeName::parse("AppleBundleIdentifier").unwrap()),
            Some(false)
        );
    }

    #[test]
    fn bundle_id_name_round_trips_and_rejects_control_characters() {
        let name = AppleBundleIdName::parse("third-thoughts").unwrap();
        assert_eq!(name.as_str(), "third-thoughts");
        assert_eq!(name.to_string(), "third-thoughts");
        assert!(AppleBundleIdName::parse("bad\nname").is_err());
        assert!(AppleBundleIdName::parse("").is_err());
    }

    #[test]
    fn bundle_id_name_refuses_an_invisible_character() {
        assert!(AppleBundleIdName::parse("evil\u{200b}name").is_err());
    }

    #[test]
    fn bundle_id_platform_accepts_the_three_documented_members() {
        for platform in ["IOS", "MAC_OS", "UNIVERSAL"] {
            assert!(AppleBundleIdPlatform::parse(platform).is_ok(), "{platform}");
        }
        assert!(AppleBundleIdPlatform::parse("TV_OS").is_err());
        assert!(AppleBundleIdPlatform::parse("ios").is_err());
    }

    #[test]
    fn bundle_id_id_accepts_apples_shape() {
        assert!(AppleBundleIdId::parse("T6G4XCV345").is_ok());
        assert!(AppleBundleIdId::parse("x").is_err());
    }

    #[test]
    fn capability_type_accepts_every_documented_member() {
        for capability in [
            "ICLOUD",
            "IN_APP_PURCHASE",
            "GAME_CENTER",
            "PUSH_NOTIFICATIONS",
            "WALLET",
            "INTER_APP_AUDIO",
            "MAPS",
            "ASSOCIATED_DOMAINS",
            "PERSONAL_VPN",
            "APP_GROUPS",
            "HEALTHKIT",
            "HOMEKIT",
            "WIRELESS_ACCESSORY_CONFIGURATION",
            "APPLE_PAY",
            "DATA_PROTECTION",
            "SIRIKIT",
            "NETWORK_EXTENSIONS",
            "MULTIPATH",
            "HOT_SPOT",
            "NFC_TAG_READING",
            "CLASSKIT",
            "AUTOFILL_CREDENTIAL_PROVIDER",
            "ACCESS_WIFI_INFORMATION",
            "NETWORK_CUSTOM_PROTOCOL",
            "COREMEDIA_HLS_LOW_LATENCY",
            "SYSTEM_EXTENSION_INSTALL",
            "USER_MANAGEMENT",
            "APPLE_ID_AUTH",
        ] {
            assert!(
                AppleCapabilityType::parse(capability).is_ok(),
                "{capability}"
            );
        }
        assert!(AppleCapabilityType::parse("NOT_A_REAL_CAPABILITY").is_err());
    }

    #[test]
    fn bundle_id_types_examples_parse_as_their_own_types() {
        crate::assert_example_parses::<AppleBundleIdentifier>();
        crate::assert_example_parses::<AppleBundleIdPlatform>();
        crate::assert_example_parses::<AppleBundleIdId>();
        crate::assert_example_parses::<AppleCapabilityType>();
    }

    // -------------------------------------------------------------
    // Certificate types
    // -------------------------------------------------------------

    #[test]
    fn certificate_type_accepts_the_two_distribution_members() {
        assert!(AppleCertificateType::parse("DISTRIBUTION").is_ok());
        assert!(AppleCertificateType::parse("IOS_DISTRIBUTION").is_ok());
    }

    #[test]
    fn certificate_type_refuses_a_non_distribution_member_and_lowercase() {
        assert!(AppleCertificateType::parse("DEVELOPER_ID_APPLICATION_G2").is_err());
        assert!(AppleCertificateType::parse("DEVELOPMENT").is_err());
        assert!(AppleCertificateType::parse("distribution").is_err());
        assert!(AppleCertificateType::parse("").is_err());
    }

    #[test]
    fn certificate_type_is_registered_and_not_secret() {
        const { assert!(!AppleCertificateType::IS_SECRET) };
        assert_eq!(
            crate::registry().is_secret(&crate::TypeName::parse("AppleCertificateType").unwrap()),
            Some(false)
        );
    }

    #[test]
    fn certificate_serial_accepts_the_observed_shape() {
        assert!(AppleCertificateSerial::parse("7B3F2A9C1D4E5F607182930A1B2C3D4E5F60").is_ok());
        assert!(AppleCertificateSerial::parse("00").is_ok());
    }

    #[test]
    fn certificate_serial_refuses_lowercase_ampersand_equals_and_empty() {
        assert!(AppleCertificateSerial::parse("7b3f2a9c1d4e5f60").is_err());
        assert!(AppleCertificateSerial::parse("7B3F&2A9C").is_err());
        assert!(AppleCertificateSerial::parse("7B3F=2A9C").is_err());
        assert!(AppleCertificateSerial::parse("").is_err());
    }

    #[test]
    fn certificate_serial_refuses_more_than_64_characters() {
        let too_long = "A".repeat(65);
        assert!(AppleCertificateSerial::parse(&too_long).is_err());
        let ok = "A".repeat(64);
        assert!(AppleCertificateSerial::parse(&ok).is_ok());
    }

    #[test]
    fn certificate_id_accepts_apples_shape_and_refuses_too_short() {
        assert!(AppleCertificateId::parse("C3RT1F1CATE9").is_ok());
        assert!(AppleCertificateId::parse("x").is_err());
    }

    #[test]
    fn certificate_types_examples_parse_as_their_own_types() {
        crate::assert_example_parses::<AppleCertificateType>();
        crate::assert_example_parses::<AppleCertificateSerial>();
        crate::assert_example_parses::<AppleCertificateId>();
    }
}
