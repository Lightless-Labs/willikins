//! [`DocumentSha256`]: a validated SHA-256 hex digest, exactly 64
//! lower-case hex characters -- what half of plan identity
//! (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s "Plan
//! identity" trust boundary) rests on.
//!
//! [`crate::Event::PlanRecorded::document_sha256`] and
//! [`crate::journal::PlanRecord::document_sha256`] carry this type rather
//! than a bare `String`: pass-1 item 3
//! (`docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`)
//! noted it was the one agent-facing text field in the workspace with no
//! grammar and no length bound. The wire format is unchanged -- the same
//! 64-character hex string -- so a journal line whose digest is not
//! exactly that is a replay error of the existing malformed-JSON class
//! (`Deserialize` below reports it through `serde::de::Error::custom`,
//! the same door every other validated wire type in this workspace uses).

use std::fmt;

use sha2::{Digest, Sha256};

/// A validated SHA-256 digest: exactly 64 lower-case hex characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocumentSha256(String);

impl DocumentSha256 {
    /// Compute the digest of `bytes`.
    #[must_use]
    pub fn compute(bytes: &[u8]) -> Self {
        use std::fmt::Write as _;
        let digest = Sha256::digest(bytes);
        let mut hex = String::with_capacity(64);
        for byte in digest {
            let _ = write!(hex, "{byte:02x}");
        }
        Self(hex)
    }

    /// Parse a `DocumentSha256`, refusing anything that is not exactly 64
    /// lower-case hex characters.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentSha256Error`] when `input` is not 64 characters,
    /// or holds a character other than `0`-`9` or lower-case `a`-`f`.
    pub fn parse(input: &str) -> Result<Self, DocumentSha256Error> {
        let valid = input.len() == 64
            && input
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if valid {
            Ok(Self(input.to_string()))
        } else {
            Err(DocumentSha256Error {
                len: input.chars().count(),
            })
        }
    }

    /// Borrow this digest's hex text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why [`DocumentSha256::parse`] refused its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a document sha256 must be exactly 64 lower-case hex characters, got {len} characters")]
pub struct DocumentSha256Error {
    /// The rejected input's character count.
    pub len: usize,
}

impl fmt::Display for DocumentSha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for DocumentSha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for DocumentSha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for DocumentSha256 {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("DocumentSha256")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-9a-f]{64}$",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_the_well_known_sha256_of_empty_bytes() {
        assert_eq!(
            DocumentSha256::compute(b"").as_str(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn compute_matches_the_well_known_sha256_test_vector_for_abc() {
        // FIPS 180-4's own SHA-256 example message.
        assert_eq!(
            DocumentSha256::compute(b"abc").as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn accepts_exactly_64_lower_case_hex_characters() {
        let text = "a".repeat(64);
        assert!(DocumentSha256::parse(&text).is_ok());
    }

    #[test]
    fn refuses_63_characters() {
        let text = "a".repeat(63);
        assert!(DocumentSha256::parse(&text).is_err());
    }

    #[test]
    fn refuses_65_characters() {
        let text = "a".repeat(65);
        assert!(DocumentSha256::parse(&text).is_err());
    }

    #[test]
    fn refuses_upper_case_hex() {
        let text = "A".repeat(64);
        assert!(DocumentSha256::parse(&text).is_err());
    }

    #[test]
    fn refuses_a_non_hex_character() {
        let text = format!("{}g", "a".repeat(63));
        assert!(DocumentSha256::parse(&text).is_err());
    }

    #[test]
    fn display_is_the_hex_text() {
        let text = "b".repeat(64);
        let digest = DocumentSha256::parse(&text).unwrap();
        assert_eq!(digest.to_string(), text);
    }

    #[test]
    fn serde_round_trips() {
        let text = "c".repeat(64);
        let digest = DocumentSha256::parse(&text).unwrap();
        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(
            serde_json::from_str::<DocumentSha256>(&json).unwrap(),
            digest
        );
    }

    #[test]
    fn deserialize_refuses_a_malformed_digest() {
        let json = "\"not-a-digest\"";
        assert!(serde_json::from_str::<DocumentSha256>(json).is_err());
    }

    #[test]
    fn json_schema_is_a_64_character_hex_pattern() {
        let schema = serde_json::to_value(schemars::schema_for!(DocumentSha256)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["pattern"], "^[0-9a-f]{64}$");
    }

    #[test]
    fn compute_is_deterministic_and_distinguishes_content() {
        let a = DocumentSha256::compute(b"one");
        let b = DocumentSha256::compute(b"one");
        let c = DocumentSha256::compute(b"two");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_str().len(), 64);
    }
}
