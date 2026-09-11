//! Property tests for the naming contract: acceptance test 9 (see
//! `docs/plans/2026-09-11-milestone-1-core.md`). Every valid `WordList`
//! round-trips through `kebab`, every join is non-empty and matches its
//! target character class, and no valid `ProjectSlug` within the length
//! limit is rejected.

use proptest::prelude::*;
use willikins_types::{DomainType, ProjectSlug, Word, WordList, is_reserved};

/// A single word: a letter followed by up to five letters or digits, or a
/// run of one to three digits.
fn word_string() -> impl Strategy<Value = String> {
    prop_oneof!["[a-z][a-z0-9]{0,5}", "[0-9]{1,3}"]
}

/// A valid `WordList`: one to four words, the first always letter-led so
/// the leading-letter invariant always holds by construction.
fn valid_word_list() -> impl Strategy<Value = WordList> {
    (
        "[a-z][a-z0-9]{0,5}",
        prop::collection::vec(word_string(), 0..3),
    )
        .prop_map(|(first, rest)| {
            let mut strings = vec![first];
            strings.extend(rest);
            let words: Vec<Word> = strings
                .iter()
                .map(|s| Word::parse(s).expect("generated word must parse"))
                .collect();
            WordList::new(words).expect("generated word list must satisfy the leading-letter rule")
        })
}

/// `^[a-z][a-z0-9_]*$`
fn matches_snake_class(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `^[A-Z][A-Za-z0-9]*$`
fn matches_pascal_class(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_alphanumeric())
}

proptest! {
    #[test]
    fn kebab_parses_back_to_the_same_word_list(list in valid_word_list()) {
        let parsed = WordList::parse_kebab(&list.kebab()).unwrap();
        prop_assert_eq!(parsed, list);
    }

    #[test]
    fn every_join_is_non_empty(list in valid_word_list()) {
        prop_assert!(!list.kebab().is_empty());
        prop_assert!(!list.snake().is_empty());
        prop_assert!(!list.screaming_snake().is_empty());
        prop_assert!(!list.pascal().is_empty());
        prop_assert!(!list.flat().is_empty());
    }

    #[test]
    fn snake_join_matches_its_target_class(list in valid_word_list()) {
        prop_assert!(matches_snake_class(&list.snake()));
    }

    #[test]
    fn pascal_join_matches_its_target_class(list in valid_word_list()) {
        prop_assert!(matches_pascal_class(&list.pascal()));
    }

    #[test]
    fn no_valid_project_slug_within_the_limit_is_rejected(list in valid_word_list()) {
        let kebab = list.kebab();
        prop_assume!(kebab.len() <= ProjectSlug::MAX_LEN);
        let words = list.words();
        let single_word_reserved = matches!(words, [only] if is_reserved(only.as_str()));
        prop_assume!(!single_word_reserved);
        prop_assert!(ProjectSlug::parse(&kebab).is_ok());
    }
}
