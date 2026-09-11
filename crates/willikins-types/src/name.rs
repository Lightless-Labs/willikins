//! `ProjectName`: the free-form display name, distinct from [`crate::ProjectSlug`].

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use crate::{DomainType, ParseError};

/// The maximum length of a [`ProjectName`], in characters.
const MAX_LEN: usize = 100;

/// Whether `c` is an invisible Unicode format character or a bidirectional
/// control, the kind that can hide extra text or reorder what a human
/// reads without changing what a naive length or equality check sees.
/// [`ProjectName`] ends up in rendered templates such as `CLAUDE.md`, so
/// these are rejected outright rather than merely trimmed.
///
/// Covers soft hyphen, the zero-width space family, the explicit
/// bidirectional embedding/override controls, the word-joiner family, the
/// isolate controls, the byte-order mark / zero-width no-break space, and
/// the Unicode line and paragraph separators. Not an exhaustive Unicode
/// category-Cf sweep, but every codepoint this crate is asked to reject by
/// name.
///
/// U+2028 and U+2029 are categories Zl and Zp rather than Cc, so
/// [`char::is_control`] does not see them, yet they break a line in every
/// renderer a `ProjectName` reaches — a rendered `CLAUDE.md` included. A
/// display name is a single line by definition, so they are rejected with
/// the invisibles rather than left to the control-character check.
fn is_invisible_or_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// A project's free-form, human-readable display name.
///
/// Trimmed, non-empty, at most 100 characters, and free of control
/// characters. Mutable and used only for labels and template text; every
/// provider name is derived from [`crate::ProjectSlug`], never from this.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectName(String);

impl ProjectName {
    /// The trimmed display text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ProjectName {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for ProjectName {
    const TYPE_NAME: &'static str = "ProjectName";

    fn description() -> &'static str {
        "A project's free-form, human-readable display name."
    }

    fn example() -> &'static str {
        "Third Thoughts"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        // U+00A0 (no-break space) looks like a space and is often typed as
        // one via a dead key or a pasted document; fold it before trimming
        // so "Third\u{A0}Thoughts " trims and displays like plain text
        // instead of silently keeping a byte a human cannot see is there.
        let folded = input.replace('\u{00A0}', " ");
        let trimmed = folded.trim();
        if trimmed.is_empty() {
            return Err(ParseError::new(Self::TYPE_NAME, "must not be empty"));
        }
        let len = trimmed.chars().count();
        if len > MAX_LEN {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("is {len} characters, the limit is {MAX_LEN}"),
            ));
        }
        if let Some(c) = trimmed.chars().find(|c| c.is_control()) {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("must not contain control characters (found {c:?})"),
            ));
        }
        if let Some(c) = trimmed.chars().find(|&c| is_invisible_or_bidi_control(c)) {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!(
                    "must not contain invisible or bidirectional control character (found {c:?})"
                ),
            ));
        }
        Ok(Self(trimmed.to_owned()))
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for ProjectName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ProjectName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for ProjectName {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ProjectName")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": MAX_LEN,
            "description": "A project's free-form, human-readable display name.",
            "examples": ["Third Thoughts"]
        })
    }
}

crate::impl_domain_object_non_secret!(ProjectName);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_whitespace() {
        let name = ProjectName::parse("  Third Thoughts  ").unwrap();
        assert_eq!(name.as_str(), "Third Thoughts");
    }

    #[test]
    fn rejects_empty() {
        assert!(ProjectName::parse("").is_err());
    }

    #[test]
    fn rejects_all_whitespace() {
        assert!(ProjectName::parse("   ").is_err());
    }

    #[test]
    fn rejects_too_long() {
        let too_long = "a".repeat(101);
        assert!(ProjectName::parse(&too_long).is_err());
    }

    #[test]
    fn accepts_exactly_at_the_limit() {
        let at_limit = "a".repeat(100);
        assert!(ProjectName::parse(&at_limit).is_ok());
    }

    #[test]
    fn rejects_control_characters() {
        assert!(ProjectName::parse("Third\tThoughts").is_err());
        assert!(ProjectName::parse("Third\nThoughts").is_err());
    }

    #[test]
    fn accepts_punctuation_and_unicode() {
        assert!(ProjectName::parse("Étoile").is_ok());
        assert!(ProjectName::parse("Lightless Labs' Foundry").is_ok());
        assert!(ProjectName::parse("!!!").is_ok());
    }

    #[test]
    fn folds_a_no_break_space_to_a_plain_space_before_trimming() {
        let name = ProjectName::parse("Third\u{A0}Thoughts").unwrap();
        assert_eq!(name.as_str(), "Third Thoughts");
    }

    #[test]
    fn folding_a_no_break_space_can_still_leave_the_input_empty() {
        assert!(ProjectName::parse("\u{A0}").is_err());
    }

    #[test]
    fn rejects_every_listed_invisible_or_bidi_control_character() {
        let codepoints: Vec<char> = ['\u{00AD}']
            .into_iter()
            .chain('\u{200B}'..='\u{200F}')
            .chain('\u{202A}'..='\u{202E}')
            .chain('\u{2060}'..='\u{2064}')
            .chain('\u{2066}'..='\u{2069}')
            .chain(['\u{FEFF}'])
            .collect();
        assert_eq!(
            codepoints.len(),
            1 + 5 + 5 + 5 + 4 + 1,
            "the ranges in this test no longer match the ones the reason names"
        );
        for c in codepoints {
            let input = format!("Third{c}Thoughts");
            let result = ProjectName::parse(&input);
            let Err(err) = result else {
                panic!("U+{:04X} was accepted", c as u32);
            };
            assert!(
                err.reason
                    .contains("invisible or bidirectional control character"),
                "U+{:04X}: reason was {:?}",
                c as u32,
                err.reason
            );
        }
    }

    #[test]
    fn implements_domain_object_via_the_macro() {
        use crate::DomainObject;

        let value: Box<dyn DomainObject> = Box::new(ProjectName::parse("Third Thoughts").unwrap());
        assert_eq!(value.type_name(), "ProjectName");
        assert!(!value.is_secret());
        assert_eq!(
            value.render(),
            crate::Rendered::Plain("Third Thoughts".to_string())
        );
        let cloned = value.clone_box();
        assert!(value.dyn_eq(cloned.as_ref()));
    }

    #[test]
    fn serde_round_trips() {
        let name = ProjectName::parse("Third Thoughts").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"Third Thoughts\"");
        let back: ProjectName = serde_json::from_str(&json).unwrap();
        assert_eq!(back, name);
    }
}
