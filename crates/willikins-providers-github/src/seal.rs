//! Sealing a GitHub Actions secret's plaintext for `PUT
//! .../actions/secrets/{name}`, using libsodium-compatible "sealed boxes"
//! (`crypto_box`, feature `seal`; never `crypto_box_seal`'s
//! since-renamed/relocated 0.10 line — see
//! `docs/research/2026-09-12-m2-dependencies.md`, section 2).
//!
//! GitHub's own guide encrypts with `nacl.public.SealedBox`, `PyNaCl`'s
//! name for the same primitive; `crypto_box`'s equivalent is a method on
//! `PublicKey` (sealing) and one on `SecretKey` (opening), not a type of
//! its own — this module's `seal` mirrors that shape rather than
//! reintroducing a `SealedBox` wrapper.
//!
//! Every seal uses a fresh ephemeral key pair internally (`crypto_box`'s
//! own behaviour, not this module's), so `encrypted_value` differs on
//! every call for the same plaintext: nothing here or in the caller may
//! dedupe a `PUT` by comparing ciphertexts.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use crypto_box::PublicKey;
use willikins_core::{ToolError, ToolErrorKind};

/// GitHub's public key is 32 bytes (Curve25519), the same key size
/// `crypto_box::KEY_SIZE` names.
const PUBLIC_KEY_BYTES: usize = 32;

/// Seal `plaintext` for GitHub's `public_key_base64` (the `key` field
/// `GET .../actions/secrets/public-key` returns), returning the
/// base64-encoded ciphertext `PUT .../actions/secrets/{name}` wants as
/// `encrypted_value`.
///
/// # Errors
///
/// Returns [`ToolError`] of kind [`ToolErrorKind::Provider`] when
/// `public_key_base64` does not decode to exactly [`PUBLIC_KEY_BYTES`]
/// bytes — a shape GitHub's own schema promises but this crate does not
/// take on faith — or when sealing itself fails (only possible, per
/// `crypto_box`'s own docs, if the ciphertext buffer allocation
/// overflows, never on well-formed input; kept as a checked path rather
/// than an `unwrap` because the input here is a provider's, not this
/// crate's own).
pub(crate) fn seal(public_key_base64: &str, plaintext: &[u8]) -> Result<String, ToolError> {
    let key_bytes = STANDARD
        .decode(public_key_base64)
        .map_err(|_| provider_error("GitHub's Actions public key was not valid base64"))?;
    let key_array: [u8; PUBLIC_KEY_BYTES] = key_bytes
        .try_into()
        .map_err(|_| provider_error("GitHub's Actions public key was not 32 bytes"))?;
    let public_key = PublicKey::from(key_array);
    let ciphertext = public_key
        .seal(&mut rand_core::OsRng, plaintext)
        .map_err(|_| provider_error("sealing the secret's value failed"))?;
    Ok(STANDARD.encode(ciphertext))
}

fn provider_error(message: &str) -> ToolError {
    ToolError {
        kind: ToolErrorKind::Provider,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_box::SecretKey;

    /// GitHub's own `OpenAPI` schema for `encrypted_value`, verbatim
    /// (research note section 2, `.paths."/repos/{owner}/{repo}/actions/secrets/{secret_name}".put.requestBody`):
    /// a base64-alphabet pattern.
    const GITHUB_ENCRYPTED_VALUE_PATTERN: &str =
        r"^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=|[A-Za-z0-9+/]{4})$";

    #[test]
    fn seal_then_unseal_with_the_matching_secret_key_recovers_the_plaintext() {
        let secret_key = SecretKey::generate(&mut rand_core::OsRng);
        let public_key = secret_key.public_key();
        let public_key_base64 = STANDARD.encode(public_key.as_bytes());

        let plaintext = b"a very secret token value";
        let encrypted_value = seal(&public_key_base64, plaintext).expect("seals");

        // Acceptance test 3: the base64 form matches what GitHub
        // documents, checked against the schema's own pattern, not just
        // "some base64 engine can decode it".
        let pattern = regex::Regex::new(GITHUB_ENCRYPTED_VALUE_PATTERN).expect("valid pattern");
        assert!(
            pattern.is_match(&encrypted_value),
            "{encrypted_value} does not match GitHub's documented encrypted_value pattern"
        );

        let ciphertext = STANDARD
            .decode(&encrypted_value)
            .expect("the sealed value is valid base64, matching GitHub's documented pattern");
        let opened = secret_key.unseal(&ciphertext).expect("unseals");
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn two_seals_of_the_same_plaintext_produce_different_ciphertexts() {
        // Each seal generates a fresh ephemeral key pair internally, so
        // nothing may dedupe a PUT by comparing `encrypted_value` bytes.
        let secret_key = SecretKey::generate(&mut rand_core::OsRng);
        let public_key_base64 = STANDARD.encode(secret_key.public_key().as_bytes());
        let plaintext = b"same-secret";
        let first = seal(&public_key_base64, plaintext).expect("seals");
        let second = seal(&public_key_base64, plaintext).expect("seals");
        assert_ne!(first, second);
    }

    #[test]
    fn the_ciphertext_never_contains_the_plaintext_or_its_base64() {
        let secret_key = SecretKey::generate(&mut rand_core::OsRng);
        let public_key_base64 = STANDARD.encode(secret_key.public_key().as_bytes());
        let plaintext = b"wlkn-test-marker-2rz8shp5tap";
        let encrypted_value = seal(&public_key_base64, plaintext).expect("seals");
        assert!(!encrypted_value.contains("wlkn-test-marker-2rz8shp5tap"));
        let plaintext_base64 = STANDARD.encode(plaintext);
        assert!(!encrypted_value.contains(&plaintext_base64));
    }

    #[test]
    fn a_public_key_of_the_wrong_length_is_refused() {
        let bad = STANDARD.encode(b"too short");
        let err = seal(&bad, b"plaintext").expect_err("32 bytes required");
        assert_eq!(err.kind, ToolErrorKind::Provider);
        assert!(err.message.contains("32 bytes"));
    }

    #[test]
    fn a_public_key_that_is_not_base64_is_refused() {
        let err = seal("not base64 at all!!", b"plaintext").expect_err("invalid base64");
        assert_eq!(err.kind, ToolErrorKind::Provider);
    }
}
