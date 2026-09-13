//! `Description`: free-text description shown to an agent.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use crate::name::is_invisible_or_bidi_control;
use crate::{DomainType, ParseError};

/// The maximum length of a [`Description`], in characters.
const MAX_LEN: usize = 1_024;

/// A free-text description: a workflow's own `description:`, an input's
/// `description:`, and (once task 1e applies this crate's types to
/// `willikins-core`) `Workflow::description` and `InputSpec::description`.
///
/// At most 1,024 characters. No control character other than space —
/// U+0000..=U+001F (tab and newline included), U+007F, and the C1 range
/// U+0080..=U+009F are all rejected, so a description is always a single
/// line. U+2028, U+2029, and the same invisible or bidirectional control
/// characters [`crate::ProjectName`] refuses are rejected too, for the
/// same reason: this text is rendered straight into an agent's prompt (see
/// `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s "Document
/// text labelling"), and none of those characters may hide extra text or
/// reorder what a reader sees.
///
/// Free text is not trimmed and an empty description is accepted: unlike
/// [`crate::ProjectName`], there is no display-label reason to demand
/// non-empty content, and the field is optional everywhere it appears.
///
/// A workflow document is privileged content, run only from a trusted
/// ref, but the *text* of a description is not: it is quoted document
/// text handed to whatever agent reads it, never an instruction to that
/// agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Description(String);

impl Description {
    /// The description text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Description {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Description {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for Description {
    const TYPE_NAME: &'static str = "Description";

    fn description() -> &'static str {
        "A free-text description shown to an agent, at most 1,024 characters."
    }

    fn example() -> &'static str {
        "Provision a GitHub repository and Doppler project for a Rust service."
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        let len = input.chars().count();
        if len > MAX_LEN {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("is {len} characters, the limit is {MAX_LEN}"),
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

impl serde::Serialize for Description {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Description {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Description {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Description")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "maxLength": MAX_LEN,
            "description": "A free-text description shown to an agent, at most 1,024 characters.",
            "examples": ["Provision a GitHub repository and Doppler project for a Rust service."]
        })
    }
}

crate::impl_domain_object_non_secret!(Description);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_text() {
        let value = Description::parse("Provision a repository.").unwrap();
        assert_eq!(value.as_str(), "Provision a repository.");
    }

    #[test]
    fn accepts_an_empty_string() {
        assert!(Description::parse("").is_ok());
    }

    #[test]
    fn does_not_trim() {
        let value = Description::parse("  padded  ").unwrap();
        assert_eq!(value.as_str(), "  padded  ");
    }

    #[test]
    fn rejects_too_long() {
        let too_long = "a".repeat(1_025);
        assert!(Description::parse(&too_long).is_err());
    }

    #[test]
    fn accepts_exactly_at_the_limit() {
        let at_limit = "a".repeat(1_024);
        assert!(Description::parse(&at_limit).is_ok());
    }

    #[test]
    fn a_length_rejection_does_not_echo_the_whole_text() {
        let too_long = format!("MARKER-{}", "a".repeat(1_025));
        let err = Description::parse(&too_long).unwrap_err();
        assert!(
            !err.reason.contains("MARKER"),
            "reason leaked: {:?}",
            err.reason
        );
        assert!(
            err.reason.len() < 100,
            "reason was {} bytes",
            err.reason.len()
        );
    }

    #[test]
    fn accepts_a_space() {
        assert!(Description::parse("a b").is_ok());
    }

    #[test]
    fn rejects_tab_and_newline() {
        assert!(Description::parse("a\tb").is_err());
        assert!(Description::parse("a\nb").is_err());
    }

    #[test]
    fn rejects_every_c0_control_character_other_than_the_ones_yaml_cannot_carry() {
        // U+0000..=U+0008, U+000B..=U+001F, and U+007F: the C0 controls
        // plus DEL, skipping the ones YAML itself treats as line breaks
        // (\n, \r) or that `char::is_control` still classifies but a raw
        // Rust string can carry directly in a test.
        for c in ('\u{0000}'..='\u{0008}')
            .chain('\u{000B}'..='\u{001F}')
            .chain(['\u{007F}'])
        {
            let input = format!("a{c}b");
            assert!(
                Description::parse(&input).is_err(),
                "U+{:04X} was accepted",
                c as u32
            );
        }
    }

    #[test]
    fn rejects_c1_control_characters() {
        for c in '\u{0080}'..='\u{009F}' {
            let input = format!("a{c}b");
            assert!(
                Description::parse(&input).is_err(),
                "U+{:04X} was accepted",
                c as u32
            );
        }
    }

    #[test]
    fn rejects_line_and_paragraph_separators() {
        assert!(Description::parse("a\u{2028}b").is_err());
        assert!(Description::parse("a\u{2029}b").is_err());
    }

    #[test]
    fn rejects_invisible_and_bidi_control_characters() {
        assert!(Description::parse("a\u{200B}b").is_err());
        assert!(Description::parse("a\u{202E}b").is_err());
        assert!(Description::parse("a\u{FEFF}b").is_err());
    }

    /// The Unicode Tags block is the plain-text channel an attacker uses
    /// to carry ASCII a human reader cannot see: U+E0041 is "A" tagged
    /// invisible, and a run of them spells out arbitrary text that this
    /// crate's own length check counts and every renderer draws as
    /// nothing. A `Description` is rendered straight into an agent's
    /// prompt, so a codepoint that can carry hidden text there is exactly
    /// the thing the type exists to refuse.
    #[test]
    fn rejects_unicode_tag_characters() {
        for c in ['\u{E0001}', '\u{E0020}', '\u{E0041}', '\u{E007F}'] {
            let input = format!("a{c}b");
            let Err(err) = Description::parse(&input) else {
                panic!("U+{:05X} was accepted", c as u32);
            };
            assert!(
                err.reason
                    .contains("invisible or bidirectional control character"),
                "U+{:05X}: reason was {:?}",
                c as u32,
                err.reason
            );
        }
    }

    /// The invisible format characters outside the ranges the type
    /// already named: the Arabic letter mark is a bidi control in its own
    /// right, the Mongolian vowel separator is a zero-width format
    /// character, and the interlinear annotation controls delimit text a
    /// renderer is meant to hide.
    #[test]
    fn rejects_the_remaining_invisible_format_characters() {
        for c in ['\u{061C}', '\u{180E}', '\u{FFF9}', '\u{FFFA}', '\u{FFFB}'] {
            let input = format!("a{c}b");
            assert!(
                Description::parse(&input).is_err(),
                "U+{:04X} was accepted",
                c as u32
            );
        }
    }

    /// A visible codepoint that merely *looks* like something else is not
    /// this type's business: it carries no hidden text and refusing it
    /// would start an unwinnable confusable-detection fight. U+FE0F in
    /// particular is load-bearing for ordinary emoji.
    #[test]
    fn accepts_a_variation_selector() {
        assert!(Description::parse("a\u{FE0F}b").is_ok());
    }

    #[test]
    fn accepts_punctuation_and_unicode() {
        assert!(Description::parse("Étoile — “quoted”, 100% done.").is_ok());
    }

    #[test]
    fn implements_domain_object_via_the_macro() {
        use crate::DomainObject;

        let value: Box<dyn DomainObject> = Box::new(Description::parse("hello").unwrap());
        assert_eq!(value.type_name(), "Description");
        assert!(!value.is_secret());
        assert_eq!(value.render(), crate::Rendered::Plain("hello".to_string()));
    }

    #[test]
    fn serde_round_trips() {
        let value = Description::parse("hello").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"hello\"");
        let back: Description = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn json_schema_carries_max_length() {
        let schema = serde_json::to_value(Description::json_schema()).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["maxLength"], 1_024);
    }

    #[test]
    fn example_parses() {
        crate::assert_example_parses::<Description>();
    }
}
