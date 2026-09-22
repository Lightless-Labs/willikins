//! [`OpaqueSecret`]: a blob in port clothing.
//!
//! Design addendum `docs/plans/2026-09-11-willikins-design.md`, "Credentials
//! are ports, resolvers are nodes": a document builds a credential by
//! chaining a resolver (`env.get`, `doppler.secret.get`) into a transform
//! (a base64 decode) into a parse (turning decoded bytes into a specific
//! secret domain type, such as an EC signing key). [`OpaqueSecret`] is the
//! type that flows between the resolver and the transform, and between the
//! transform and the parse: it carries bytes and asserts nothing about
//! their shape.
//!
//! **Why this exists instead of every resolver inventing its own blob
//! type.** A transform or parse tool is provider-independent by design —
//! `base64.decode` does not care whether its input came from Doppler, the
//! environment, or a future vault, and should not have to name each one's
//! output type to accept it. Committing every resolver that has no shape
//! of its own to emit `OpaqueSecret` gives every transform and parse tool
//! one type to declare. `env.get` is the first such resolver landed here;
//! `doppler.secret.get` predates this type and still emits
//! `DopplerSecretValue`.
//!
//! **The Doppler bridge — "wall one", and how it is built.** A document
//! chaining `doppler.secret.get` straight into `base64.decode` used to
//! fail `check`: the transform's input port declared `exact("OpaqueSecret",
//! true)`, and `DopplerSecretValue` is a different type. `base64.decode`
//! and `apple.signing_key.parse`'s input ports now declare
//! `willikins_core::tool::helpers::any_secret(true)` instead — the same
//! helper `doppler.secret.set`'s `value` port already uses, for the
//! analogous reason (a SigNoz-minted key reaching a Doppler config
//! without either tool naming the other's type). That widening only
//! fixes the *type check*; a pure tool's `Tool::read` still needs a
//! token-less way to read whichever secret type actually landed on the
//! port, since it never receives a `SinkToken`. [`reveal_transform_input`]
//! is that dispatch: it tries every type that has opted into a
//! token-less `reveal_for_transform` (today, [`OpaqueSecret`] and
//! `crate::doppler::DopplerSecretValue`, in that order) and refuses
//! anything else with a clear, content-free message. A transform or
//! parse tool calls it once instead of naming a concrete type, so a
//! third resolver output type earns support in one place, not in every
//! transform and parse tool that exists.
//!
//! **Accepted only by transforms and parses, never by a tool that does
//! real work.** Nothing enforces this in the type system — any tool could
//! declare an `OpaqueSecret` input port — so it is a convention this
//! type's doc comment states and code review holds: a tool that touches a
//! real resource (a repository, a config, a pipeline) takes a *specific*
//! secret domain type, never this one, so that the type name at its port
//! says what the value actually is.
/// An opaque secret value: a blob in port clothing, passed between a
/// resolver and a transform or parse tool. Secret.
#[derive(willikins_derive::DomainType)]
#[domain(
    min_len = 1,
    max_len = 65536,
    secret,
    description = "An opaque secret value passed between a resolver and a transform or parse tool. Asserts nothing about its shape.",
    example = "opaque-example-value"
)]
pub struct OpaqueSecret(secrecy::SecretString);

impl OpaqueSecret {
    /// Apply `f` to this secret's raw bytes, producing whatever `f`
    /// produces — typically another [`OpaqueSecret`] (a transform, such as
    /// `base64.decode`) or a different secret domain type entirely (a
    /// parse, such as the one turning a decoded blob into an EC signing
    /// key).
    ///
    /// # Why this exists, and why it is one of exactly two such methods
    /// in this crate (the other is `DopplerSecretValue`'s own
    /// `reveal_for_transform`, in `doppler.rs`)
    ///
    /// Every other secret domain type's bytes are reachable only through
    /// its derive-generated `expose(&SinkToken)`, and [`SinkToken`] can
    /// only be constructed inside the apply executor
    /// (`willikins_core::apply`) — see `willikins_types::sink`'s module
    /// doc. That is exactly right for a tool that *does something* with a
    /// secret (calls a provider, sets a header): such a tool only ever
    /// runs inside `Tool::ensure`, which the executor calls with a real
    /// token.
    ///
    /// A transform or parse tool is different: it is *pure*, so `plan`
    /// evaluates it through `Tool::read` — which
    /// [`willikins_core::Tool::read`]'s own contract says "never receives
    /// a token", precisely so a provider `read` has no legitimate way to
    /// leak a secret. But the whole point of "credentials are ports,
    /// resolvers are nodes" (this type's module doc) is that a resolver
    /// chain's result must be a real, known value *before* a downstream
    /// provider's own `read` runs during planning — so a transform or
    /// parse cannot defer its work to `ensure` the way a provider tool
    /// does; it must compute its output from `read` alone.
    ///
    /// [`Self::reveal_for_transform`] is the narrow, named exception that
    /// makes that possible without weakening the rule for every other
    /// secret type: it is hand-written on this type and, since the App
    /// Store Connect credential correction, on `DopplerSecretValue` alone
    /// (that type's own doc explains why: `doppler.secret.get` predates
    /// this type and still emits it) — never added to the
    /// derive macro itself (which would give it to `DopplerServiceToken`
    /// and every other secret type for free, invisibly to
    /// `clippy.toml`'s `disallowed-methods` reason and to
    /// `expose_secret_guard.rs`'s file-scoped exemption list — both of
    /// which name every call site by function and file), and its result is bytes that
    /// exist only inside `f`'s own closure and whatever secret domain
    /// type `f` wraps them back into before returning. Nothing here lets
    /// bytes reach a `String`, a log, or an error message: `f`'s return
    /// type is generic and this function never inspects it.
    ///
    /// [`SinkToken`]: crate::SinkToken
    // The second production call site of `expose_secret` outside the
    // derive's own codegen (the first is `willikins-providers-http`'s
    // `Credential::authorize`/`authorize_header`) — named in
    // `clippy.toml`'s `disallowed-methods` reason and walked by
    // `crates/willikins-core/tests/expose_secret_guard.rs`, which
    // exempts exactly this function in this file.
    #[allow(clippy::disallowed_methods)]
    pub fn reveal_for_transform<T, E>(&self, f: impl FnOnce(&str) -> Result<T, E>) -> Result<T, E> {
        f(secrecy::ExposeSecret::expose_secret(&self.0))
    }
}

/// Dispatch a transform or parse tool's `AnySecret`-typed input to
/// whichever concrete type's token-less `reveal_for_transform` actually
/// matches `obj`, and apply `f` to the revealed bytes.
///
/// See this module's doc, "The Doppler bridge — 'wall one'". `obj` is a
/// type-erased [`crate::DomainObject`] because a `PortType::AnySecret`
/// port accepts any secret type the registry knows, not one this
/// function's caller names; only [`OpaqueSecret`] and
/// [`crate::doppler::DopplerSecretValue`] have opted into a token-less
/// reveal today, and both are tried, in that order. `unsupported` builds
/// the caller's own error type for every other secret type — a real
/// resource-touching tool's input (an `AppleSigningKey`, say), which a
/// document should never be routing through `base64.decode` in the first
/// place, and which this function refuses without inspecting or
/// exposing.
///
/// # Errors
///
/// Propagates `f`'s own `Err`, or `unsupported()` when `obj` is not one
/// of the types this function knows how to reveal.
pub fn reveal_transform_input<T, E>(
    obj: &dyn crate::DomainObject,
    f: impl FnOnce(&str) -> Result<T, E>,
    unsupported: impl FnOnce() -> E,
) -> Result<T, E> {
    if let Some(value) = crate::downcast::<OpaqueSecret>(obj) {
        return value.reveal_for_transform(f);
    }
    if let Some(value) = crate::downcast::<crate::doppler::DopplerSecretValue>(obj) {
        return value.reveal_for_transform(f);
    }
    Err(unsupported())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainType;

    #[test]
    fn parses_a_non_empty_value() {
        assert!(OpaqueSecret::parse("anything at all").is_ok());
    }

    #[test]
    fn rejects_the_empty_string() {
        let err = OpaqueSecret::parse("").unwrap_err();
        assert!(err.reason.contains("at least"), "{}", err.reason);
    }

    #[test]
    fn rejects_content_over_the_length_limit() {
        let too_long = "a".repeat(65537);
        assert!(OpaqueSecret::parse(&too_long).is_err());
    }

    #[test]
    fn debug_and_display_never_show_the_value() {
        let value = OpaqueSecret::parse("wlkn-test-marker-9fz2q").unwrap();
        assert_eq!(format!("{value:?}"), "[REDACTED OpaqueSecret]");
        assert_eq!(value.to_string(), "[REDACTED OpaqueSecret]");
    }

    #[test]
    fn reveal_for_transform_hands_the_closure_the_real_bytes() {
        let value = OpaqueSecret::parse("c29tZS1ieXRlcw==").unwrap();
        let seen: Result<String, std::convert::Infallible> =
            value.reveal_for_transform(|s| Ok(s.to_string()));
        assert_eq!(seen.unwrap(), "c29tZS1ieXRlcw==");
    }

    #[test]
    fn reveal_for_transform_propagates_the_closures_error() {
        let value = OpaqueSecret::parse("anything").unwrap();
        let err: Result<(), &str> = value.reveal_for_transform(|_| Err("nope"));
        assert_eq!(err.unwrap_err(), "nope");
    }

    #[test]
    fn example_parses_as_its_own_type() {
        crate::assert_example_parses::<OpaqueSecret>();
    }

    #[test]
    fn is_secret() {
        const { assert!(OpaqueSecret::IS_SECRET) };
    }

    #[test]
    fn reveal_transform_input_reveals_an_opaque_secret() {
        let value = OpaqueSecret::parse("opaque-bytes").unwrap();
        let seen: Result<String, ()> = reveal_transform_input(&value, |s| Ok(s.to_string()), || ());
        assert_eq!(seen.unwrap(), "opaque-bytes");
    }

    #[test]
    fn reveal_transform_input_reveals_a_doppler_secret_value() {
        // The whole point of "wall one": a `DopplerSecretValue` --
        // `doppler.secret.get`'s own output type, not `OpaqueSecret` --
        // is readable through the same dispatch, with no caller-side
        // branching on which type actually landed on the port.
        let value = crate::doppler::DopplerSecretValue::parse("doppler-bytes").unwrap();
        let seen: Result<String, ()> = reveal_transform_input(&value, |s| Ok(s.to_string()), || ());
        assert_eq!(seen.unwrap(), "doppler-bytes");
    }

    #[test]
    fn reveal_transform_input_refuses_a_secret_type_that_never_opted_in() {
        // `AppleSigningKey` is exactly the kind of secret this dispatch
        // must refuse: a real resource-touching tool's own input, which
        // has no `reveal_for_transform` and must not gain one implicitly
        // through this function.
        let key = crate::AppleSigningKey::parse(crate::AppleSigningKey::example()).unwrap();
        let seen: Result<String, &str> =
            reveal_transform_input(&key, |s| Ok(s.to_string()), || "unsupported");
        assert_eq!(seen.unwrap_err(), "unsupported");
    }
}
