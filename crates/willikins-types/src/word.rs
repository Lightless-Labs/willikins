//! Words and word lists: the shared vocabulary behind every kebab-case slug.

use std::borrow::Cow;
use std::fmt;

use crate::{DomainType, ParseError};

/// A single lowercase word: `[a-z][a-z0-9]*` or `[0-9]+`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Word(String);

impl Word {
    /// Parse a single word from its ASCII text form.
    ///
    /// Accepts a lowercase letter followed by any number of lowercase
    /// letters and digits, or a run of digits on its own.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        if input.is_empty() {
            return Err(ParseError::new("Word", "a word must not be empty"));
        }
        if input.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(Self(input.to_owned()));
        }
        let mut chars = input.chars();
        let Some(first) = chars.next() else {
            return Err(ParseError::new("Word", "a word must not be empty"));
        };
        if !first.is_ascii_lowercase() {
            return Err(ParseError::new(
                "Word",
                format!("`{input}` must start with a lowercase letter, or be all digits"),
            ));
        }
        if let Some(bad) = chars.find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit())) {
            return Err(ParseError::new(
                "Word",
                format!(
                    "`{input}` contains `{bad}`, which is not a lowercase ASCII letter or digit"
                ),
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The word's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this word starts with a letter rather than a digit.
    #[must_use]
    pub fn starts_with_letter(&self) -> bool {
        self.0.as_bytes()[0].is_ascii_alphabetic()
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A non-empty, ordered list of [`Word`]s. The first word must start with a
/// letter, never a digit. Serialises as kebab-case; every join on this type
/// is total.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WordList(Vec<Word>);

impl WordList {
    /// Build a word list from already-parsed words, checking the
    /// leading-letter rule.
    pub fn new(words: Vec<Word>) -> Result<Self, ParseError> {
        let Some(first) = words.first() else {
            return Err(ParseError::new(
                "WordList",
                "must contain at least one word",
            ));
        };
        if !first.starts_with_letter() {
            return Err(ParseError::new(
                "WordList",
                format!("the first word `{first}` must start with a letter, not a digit"),
            ));
        }
        Ok(Self(words))
    }

    /// Parse a kebab-case string: single hyphens between [`Word`]s, no
    /// leading, trailing, or doubled hyphen, ASCII only.
    pub fn parse_kebab(input: &str) -> Result<Self, ParseError> {
        if input.is_empty() {
            return Err(ParseError::new("WordList", "must not be empty"));
        }
        if !input.is_ascii() {
            return Err(ParseError::new(
                "WordList",
                format!("`{input}` must be ASCII"),
            ));
        }
        if input.starts_with('-') {
            return Err(ParseError::new(
                "WordList",
                format!("`{input}` must not start with a hyphen"),
            ));
        }
        if input.ends_with('-') {
            return Err(ParseError::new(
                "WordList",
                format!("`{input}` must not end with a hyphen"),
            ));
        }
        if input.contains("--") {
            return Err(ParseError::new(
                "WordList",
                format!("`{input}` must not contain a doubled hyphen"),
            ));
        }
        let mut words = Vec::new();
        for segment in input.split('-') {
            let word = Word::parse(segment)
                .map_err(|e| ParseError::new("WordList", format!("in `{input}`: {}", e.reason)))?;
            words.push(word);
        }
        Self::new(words)
    }

    /// The words, in order.
    #[must_use]
    pub fn words(&self) -> &[Word] {
        &self.0
    }

    /// Join with single hyphens: `third-thoughts`.
    #[must_use]
    pub fn kebab(&self) -> String {
        self.join_with("-")
    }

    /// Join with single underscores: `third_thoughts`.
    #[must_use]
    pub fn snake(&self) -> String {
        self.join_with("_")
    }

    /// Upper-cased snake join: `THIRD_THOUGHTS`.
    #[must_use]
    pub fn screaming_snake(&self) -> String {
        self.snake().to_ascii_uppercase()
    }

    /// Join with nothing between words: `thirdthoughts`.
    #[must_use]
    pub fn flat(&self) -> String {
        self.join_with("")
    }

    /// Capitalise the first letter of each word and join: `ThirdThoughts`.
    /// A digit-only word is appended unchanged.
    #[must_use]
    pub fn pascal(&self) -> String {
        self.0.iter().map(|w| capitalize(w.as_str())).collect()
    }

    fn join_with(&self, sep: &str) -> String {
        self.0
            .iter()
            .map(Word::as_str)
            .collect::<Vec<_>>()
            .join(sep)
    }
}

impl fmt::Display for WordList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.kebab())
    }
}

impl DomainType for WordList {
    const TYPE_NAME: &'static str = "WordList";

    fn description() -> &'static str {
        "An ordered, non-empty list of lowercase ASCII words, serialised as kebab-case."
    }

    fn example() -> &'static str {
        "third-thoughts"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        Self::parse_kebab(input)
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl schemars::JsonSchema for WordList {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("WordList")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$",
            "description": "An ordered, non-empty list of lowercase ASCII words, serialised as kebab-case.",
            "examples": ["third-thoughts"]
        })
    }
}

fn capitalize(word: &str) -> String {
    if word.bytes().all(|b| b.is_ascii_digit()) {
        return word.to_owned();
    }
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_uppercase().to_string() + chars.as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_rejects_empty() {
        assert!(Word::parse("").is_err());
    }

    #[test]
    fn word_rejects_uppercase() {
        assert!(Word::parse("Foo").is_err());
    }

    #[test]
    fn word_rejects_leading_digit_with_letters() {
        assert!(Word::parse("2fast").is_err());
    }

    #[test]
    fn word_accepts_all_digits() {
        assert_eq!(Word::parse("2").unwrap().as_str(), "2");
        assert_eq!(Word::parse("42").unwrap().as_str(), "42");
    }

    #[test]
    fn word_accepts_letter_then_digits() {
        assert_eq!(Word::parse("b2b").unwrap().as_str(), "b2b");
    }

    #[test]
    fn word_rejects_non_ascii() {
        assert!(Word::parse("café").is_err());
    }

    #[test]
    fn word_rejects_hyphen() {
        assert!(Word::parse("foo-bar").is_err());
    }

    fn words(strs: &[&str]) -> Vec<Word> {
        strs.iter().map(|s| Word::parse(s).unwrap()).collect()
    }

    #[test]
    fn word_list_rejects_empty() {
        assert!(WordList::new(Vec::new()).is_err());
    }

    #[test]
    fn word_list_rejects_leading_digit_word() {
        assert!(WordList::new(words(&["2", "fast"])).is_err());
    }

    #[test]
    fn word_list_accepts_digit_after_first() {
        assert!(WordList::new(words(&["foundry", "2"])).is_ok());
    }

    #[test]
    fn parse_kebab_rejects_empty() {
        assert!(WordList::parse_kebab("").is_err());
    }

    #[test]
    fn parse_kebab_rejects_leading_hyphen() {
        assert!(WordList::parse_kebab("-foo").is_err());
    }

    #[test]
    fn parse_kebab_rejects_trailing_hyphen() {
        assert!(WordList::parse_kebab("foo-").is_err());
    }

    #[test]
    fn parse_kebab_rejects_double_hyphen() {
        assert!(WordList::parse_kebab("foo--bar").is_err());
    }

    #[test]
    fn parse_kebab_rejects_uppercase() {
        assert!(WordList::parse_kebab("Foo-bar").is_err());
    }

    #[test]
    fn parse_kebab_rejects_non_ascii() {
        assert!(WordList::parse_kebab("café-bar").is_err());
    }

    #[test]
    fn parse_kebab_round_trips_kebab() {
        let list = WordList::parse_kebab("third-thoughts").unwrap();
        assert_eq!(list.kebab(), "third-thoughts");
    }

    #[test]
    fn joins_match_design_golden_table() {
        let list = WordList::parse_kebab("third-thoughts").unwrap();
        assert_eq!(list.kebab(), "third-thoughts");
        assert_eq!(list.snake(), "third_thoughts");
        assert_eq!(list.screaming_snake(), "THIRD_THOUGHTS");
        assert_eq!(list.pascal(), "ThirdThoughts");
    }

    #[test]
    fn pascal_appends_digit_only_word_unchanged() {
        let list = WordList::parse_kebab("foundry-2").unwrap();
        assert_eq!(list.pascal(), "Foundry2");
    }

    #[test]
    fn flat_join_has_no_separators() {
        let list = WordList::parse_kebab("third-thoughts").unwrap();
        assert_eq!(list.flat(), "thirdthoughts");
    }
}
