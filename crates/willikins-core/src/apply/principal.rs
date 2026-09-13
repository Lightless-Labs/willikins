//! [`PrincipalId`]: who approved a plan, or who is calling as an agent.

use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

/// The pattern every [`PrincipalId`] must match: starts with an
/// alphanumeric character, then up to 127 more alphanumerics, `.`, `_`,
/// `@`, or `-`. Bounded the way [`crate::workflow::NodeName`] and its
/// siblings are, but a principal id is not a domain type: it never flows
/// through a tool port, so it lives here rather than in
/// `willikins-types`.
const PRINCIPAL_ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._@-]{0,127}$";

static PRINCIPAL_ID_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(PRINCIPAL_ID_PATTERN).expect("pattern is valid"));

/// The identity of a principal — an approver or an agent — recorded
/// alongside an [`crate::apply::Approval`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalId(String);

impl PrincipalId {
    /// Parse a `PrincipalId`, checking it against the required pattern.
    ///
    /// # Errors
    ///
    /// Returns [`willikins_types::ParseError`] when `input` does not match
    /// the pattern.
    pub fn parse(input: &str) -> Result<Self, willikins_types::ParseError> {
        if PRINCIPAL_ID_REGEX.is_match(input) {
            Ok(Self(input.to_string()))
        } else {
            Err(willikins_types::ParseError::new(
                "PrincipalId",
                format!(
                    "{input:?} is not a valid PrincipalId (expected to match `{PRINCIPAL_ID_PATTERN}`)"
                ),
            ))
        }
    }

    /// Borrow this id as a plain string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for PrincipalId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for PrincipalId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for PrincipalId {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PrincipalId")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": PRINCIPAL_ID_PATTERN,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_simple_id() {
        assert_eq!(PrincipalId::parse("alice").unwrap().as_str(), "alice");
    }

    #[test]
    fn accepts_the_allowed_punctuation() {
        assert!(PrincipalId::parse("alice.bob_carol@example-org").is_ok());
    }

    #[test]
    fn rejects_a_leading_punctuation_character() {
        assert!(PrincipalId::parse(".alice").is_err());
        assert!(PrincipalId::parse("-alice").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(PrincipalId::parse("").is_err());
    }

    #[test]
    fn accepts_128_characters() {
        let id = "a".repeat(128);
        assert!(PrincipalId::parse(&id).is_ok());
    }

    #[test]
    fn rejects_129_characters() {
        let id = "a".repeat(129);
        assert!(PrincipalId::parse(&id).is_err());
    }

    #[test]
    fn displays_and_serializes_as_its_string() {
        let id = PrincipalId::parse("alice").unwrap();
        assert_eq!(id.to_string(), "alice");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"alice\"");
        assert_eq!(
            serde_json::from_str::<PrincipalId>("\"alice\"").unwrap(),
            id
        );
    }

    #[test]
    fn json_schema_carries_the_pattern() {
        let schema = serde_json::to_value(schemars::schema_for!(PrincipalId)).unwrap();
        assert_eq!(schema["type"], "string");
        assert!(schema["pattern"].as_str().unwrap().contains("A-Za-z0-9"));
    }
}
