//! [`Reason`]: the bounded, non-secret text an approver gives when
//! rejecting a plan ([`crate::Event::ApprovalRejected`]).

use std::fmt;

/// A human-supplied rejection reason, refused rather than truncated once
/// it exceeds [`Reason::MAX_CHARS`] -- the same "refuse, do not silently
/// reshape" convention `willikins_types::WorkflowName` and `Description`
/// use for agent-facing text, rather than the truncate-and-continue a
/// logging library might default to. A truncated reason could still hide
/// an operator's own words mid-sentence with no sign anything was cut;
/// refusing tells the caller immediately, at the point they can still
/// fix it, and never writes a partial reason into the audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason(String);

impl Reason {
    /// The most characters (not bytes) a `Reason` may hold.
    pub const MAX_CHARS: usize = 256;

    /// Parse a `Reason`, refusing text longer than [`Self::MAX_CHARS`]
    /// characters.
    ///
    /// # Errors
    ///
    /// Returns [`ReasonError`] when `input` has more than
    /// [`Self::MAX_CHARS`] characters.
    pub fn parse(input: &str) -> Result<Self, ReasonError> {
        let len = input.chars().count();
        if len > Self::MAX_CHARS {
            Err(ReasonError { len })
        } else {
            Ok(Self(input.to_string()))
        }
    }

    /// Borrow this reason as a plain string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why [`Reason::parse`] refused its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "a rejection reason must be at most {} characters, got {len}",
    Reason::MAX_CHARS
)]
pub struct ReasonError {
    /// The rejected input's character count.
    pub len: usize,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for Reason {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Reason {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Reason {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Reason")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "maxLength": Reason::MAX_CHARS,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exactly_the_bound() {
        let text = "a".repeat(Reason::MAX_CHARS);
        assert!(Reason::parse(&text).is_ok());
    }

    #[test]
    fn refuses_one_over_the_bound() {
        let text = "a".repeat(Reason::MAX_CHARS + 1);
        let err = Reason::parse(&text).unwrap_err();
        assert_eq!(err.len, Reason::MAX_CHARS + 1);
    }

    #[test]
    fn counts_characters_not_bytes() {
        // Each "é" is two UTF-8 bytes but one character; 256 of them must
        // still be accepted, and 257 must still be refused.
        let text = "é".repeat(Reason::MAX_CHARS);
        assert!(Reason::parse(&text).is_ok());
        let too_long = "é".repeat(Reason::MAX_CHARS + 1);
        assert!(Reason::parse(&too_long).is_err());
    }

    #[test]
    fn serde_round_trips() {
        let reason = Reason::parse("not enough test coverage yet").unwrap();
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(
            serde_json::from_str::<Reason>(&json).unwrap().as_str(),
            reason.as_str()
        );
    }

    #[test]
    fn deserialize_refuses_an_overlong_string() {
        let text = "a".repeat(Reason::MAX_CHARS + 1);
        let json = serde_json::to_string(&text).unwrap();
        assert!(serde_json::from_str::<Reason>(&json).is_err());
    }
}
