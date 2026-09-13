//! [`Timestamp`]: an RFC 3339 instant, used wherever an approval or a
//! journal event needs to record when something happened.

use std::borrow::Cow;
use std::fmt;

/// A point in time, serialized as an RFC 3339 string
/// (`2026-09-13T12:00:00+00:00`) rather than through `chrono`'s own
/// `serde` feature's default representation, so the wire shape is pinned
/// here rather than left to a dependency's own choice.
///
/// The [`schemars::JsonSchema`] implementation is hand-written (a plain
/// `{"type": "string", "format": "date-time"}`) rather than derived, so
/// this type needs nothing from schemars' own `chrono04` feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(chrono::DateTime<chrono::Utc>);

impl Timestamp {
    /// The current instant.
    #[must_use]
    pub fn now() -> Self {
        Self(chrono::Utc::now())
    }

    /// Wrap an existing `chrono` instant.
    #[must_use]
    pub fn from_datetime(at: chrono::DateTime<chrono::Utc>) -> Self {
        Self(at)
    }

    /// Borrow the underlying `chrono` instant.
    #[must_use]
    pub fn as_datetime(&self) -> &chrono::DateTime<chrono::Utc> {
        &self.0
    }

    /// Parse an RFC 3339 string.
    ///
    /// # Errors
    ///
    /// Returns [`willikins_types::ParseError`] when `input` is not a valid
    /// RFC 3339 timestamp.
    pub fn parse(input: &str) -> Result<Self, willikins_types::ParseError> {
        chrono::DateTime::parse_from_rfc3339(input)
            .map(|at| Self(at.with_timezone(&chrono::Utc)))
            .map_err(|err| {
                willikins_types::ParseError::new(
                    "Timestamp",
                    format!("{input:?} is not a valid RFC 3339 timestamp: {err}"),
                )
            })
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.to_rfc3339())
    }
}

impl serde::Serialize for Timestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Timestamp {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Timestamp")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "format": "date-time",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_round_trips_through_rfc3339() {
        let now = Timestamp::now();
        let text = now.to_string();
        let back = Timestamp::parse(&text).unwrap();
        assert_eq!(now, back);
    }

    #[test]
    fn parse_rejects_a_non_rfc3339_string() {
        assert!(Timestamp::parse("not a timestamp").is_err());
    }

    #[test]
    fn serde_round_trips() {
        let at = Timestamp::parse("2026-09-13T12:00:00+00:00").unwrap();
        let json = serde_json::to_string(&at).unwrap();
        assert_eq!(json, "\"2026-09-13T12:00:00+00:00\"");
        assert_eq!(serde_json::from_str::<Timestamp>(&json).unwrap(), at);
    }

    #[test]
    fn json_schema_is_a_date_time_string() {
        let schema = serde_json::to_value(schemars::schema_for!(Timestamp)).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["format"], "date-time");
    }
}
