//! Adversarial tests for the frozen naming grammar.
//!
//! The slug grammar is on the idempotence path: once a project exists, a
//! change to derivation duplicates resources. These tests pin the exact
//! boundaries — serialised length including hyphens, the first-character
//! rule, the ASCII-only character classes, the single-word reserved rule,
//! and the joins — against inputs chosen to break a plausible
//! implementation rather than to confirm a happy path.

use willikins_types::{
    ComponentSlug, DomainType, EnvironmentSlug, ProjectName, ProjectSlug, ProposeError, WordList,
    propose_slug,
};

/// `a-a-…-ab`: exactly `len` characters of alternating one-letter words,
/// so the hyphens are part of what the length limit must count.
fn hyphenated(len: usize) -> String {
    assert!(len >= 2, "need room for at least two characters");
    let mut s = String::new();
    while s.len() + 2 <= len {
        s.push('a');
        if s.len() + 2 <= len {
            s.push('-');
        }
    }
    while s.len() < len {
        s.push('b');
    }
    s
}

#[test]
fn hyphenated_helper_builds_valid_slugs_of_the_requested_length() {
    for len in 2..40 {
        let s = hyphenated(len);
        assert_eq!(s.len(), len, "`{s}` should be {len} characters");
        assert!(
            WordList::parse_kebab(&s).is_ok(),
            "`{s}` should be a valid word list"
        );
    }
}

#[test]
fn project_slug_length_counts_hyphens_at_the_boundary() {
    let at_limit = hyphenated(32);
    assert!(
        ProjectSlug::parse(&at_limit).is_ok(),
        "`{at_limit}` is 32 characters and must be accepted"
    );
    let over_limit = hyphenated(33);
    assert!(
        ProjectSlug::parse(&over_limit).is_err(),
        "`{over_limit}` is 33 characters and must be rejected"
    );
}

#[test]
fn component_slug_shares_the_thirty_two_character_bound() {
    assert!(ComponentSlug::parse(&hyphenated(32)).is_ok());
    assert!(ComponentSlug::parse(&hyphenated(33)).is_err());
}

#[test]
fn environment_slug_boundary_counts_hyphens() {
    assert!(EnvironmentSlug::parse(&hyphenated(16)).is_ok());
    assert!(EnvironmentSlug::parse(&hyphenated(17)).is_err());
}

#[test]
fn hyphen_placement_is_exact() {
    for bad in ["a-", "-a", "a--b", "-", "--", "a--b", "a- b"] {
        assert!(
            WordList::parse_kebab(bad).is_err(),
            "`{bad}` must not parse as a word list"
        );
        assert!(
            ProjectSlug::parse(bad).is_err(),
            "`{bad}` must not parse as a project slug"
        );
    }
}

#[test]
fn a_digit_word_is_allowed_only_after_the_first() {
    let ok = ProjectSlug::parse("a-1").expect("`a-1` must be accepted");
    assert_eq!(ok.to_string(), "a-1");
    assert!(
        ProjectSlug::parse("1-a").is_err(),
        "`1-a` starts with a digit word and must be rejected"
    );
    assert!(
        ProjectSlug::parse("1").is_err(),
        "`1` starts with a digit word and must be rejected"
    );
    assert!(
        ProjectSlug::parse("2fast").is_err(),
        "`2fast` is neither `[a-z][a-z0-9]*` nor `[0-9]+`"
    );
}

#[test]
fn snake_of_a_digit_word_keeps_the_separator() {
    let slug = ProjectSlug::parse("a-1").unwrap();
    assert_eq!(slug.words().snake(), "a_1");
    assert_eq!(slug.words().screaming_snake(), "A_1");
}

#[test]
fn pascal_appends_digit_words_without_a_separator() {
    let slug = ProjectSlug::parse("x-2-y").unwrap();
    assert_eq!(slug.words().pascal(), "X2Y");
}

/// The pascal join is not injective: a digit-only word fuses with the word
/// before it, so two distinct slugs share one Swift module name. Recorded
/// here so the collision cannot be rediscovered by a user. See the
/// verification note in the task report.
#[test]
fn pascal_collides_for_a_digit_word_and_its_fused_spelling() {
    let separated = ProjectSlug::parse("foundry-2").unwrap();
    let fused = ProjectSlug::parse("foundry2").unwrap();
    assert_ne!(separated, fused);
    assert_ne!(separated.to_string(), fused.to_string());
    assert_ne!(separated.words().snake(), fused.words().snake());
    assert_eq!(separated.words().pascal(), fused.words().pascal());
}

#[test]
fn non_ascii_letters_and_digits_are_rejected() {
    for bad in [
        "café",          // Latin-1 letter
        "caf\u{e9}-bar", // same, before a hyphen
        "１２３",        // fullwidth digits
        "ａ-b",          // fullwidth letter
        "٣",             // Arabic-Indic digit
        "a-٣",           // Arabic-Indic digit in a later word
        "a-\u{0130}",    // dotted capital I
        "\u{200b}a",     // zero-width space
        "a\u{0301}-b",   // combining acute
    ] {
        assert!(
            WordList::parse_kebab(bad).is_err(),
            "`{bad}` must not parse as a word list"
        );
        assert!(
            ProjectSlug::parse(bad).is_err(),
            "`{bad}` must not parse as a project slug"
        );
    }
}

#[test]
fn ascii_non_word_characters_are_rejected() {
    for bad in [
        "a_b", "a.b", "a b", "a/b", "a:b", "A", "aB", "a\u{0}b", "a\tb", "a+b",
    ] {
        assert!(
            ProjectSlug::parse(bad).is_err(),
            "`{bad}` must not parse as a project slug"
        );
    }
}

#[test]
fn reserved_words_are_matched_case_insensitively_and_only_alone() {
    for reserved in [
        "match", "Match", "MATCH", "self", "Self", "type", "native", "default", "nul",
    ] {
        assert!(
            ProjectSlug::parse(reserved).is_err(),
            "`{reserved}` must be rejected as a single-word slug"
        );
    }
    for ok in ["match-maker", "self-hosted", "type-system", "nul-island"] {
        assert!(
            ProjectSlug::parse(ok).is_ok(),
            "`{ok}` is multi-word and must be accepted"
        );
    }
}

#[test]
fn windows_device_names_are_reserved_only_for_the_documented_numbers() {
    for reserved in ["com1", "com9", "lpt1", "lpt9", "con", "prn", "aux", "nul"] {
        assert!(
            ProjectSlug::parse(reserved).is_err(),
            "`{reserved}` is a Windows device name and must be rejected"
        );
    }
    for ok in ["com0", "com10", "lpt0", "lpt10", "com", "lpt"] {
        assert!(
            ProjectSlug::parse(ok).is_ok(),
            "`{ok}` is not a Windows device name and must be accepted"
        );
    }
}

#[test]
fn edition_2024_rust_keywords_are_reserved() {
    assert!(
        ProjectSlug::parse("gen").is_err(),
        "`gen` is reserved in Rust 2024 and must be rejected as a single-word slug"
    );
    assert!(
        ProjectSlug::parse("gen-art").is_ok(),
        "`gen-art` is multi-word and must be accepted"
    );
}

#[test]
fn the_reserved_check_applies_to_every_slug_type() {
    assert!(ComponentSlug::parse("match").is_err());
    assert!(EnvironmentSlug::parse("nul").is_err());
    assert!(EnvironmentSlug::parse("prd").is_ok());
}

#[test]
fn json_schema_carries_the_pattern_and_the_per_type_max_length() {
    let pattern = r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$";
    for (schema, max_len, name) in [
        (
            <ProjectSlug as DomainType>::json_schema(),
            32,
            "ProjectSlug",
        ),
        (
            <ComponentSlug as DomainType>::json_schema(),
            32,
            "ComponentSlug",
        ),
        (
            <EnvironmentSlug as DomainType>::json_schema(),
            16,
            "EnvironmentSlug",
        ),
    ] {
        let value = schema.as_value();
        assert_eq!(value["type"], "string", "{name} schema type");
        assert_eq!(value["pattern"], pattern, "{name} schema pattern");
        assert_eq!(value["maxLength"], max_len, "{name} schema maxLength");
    }
}

#[test]
fn serde_round_trips_a_maximum_length_hyphenated_slug() {
    let at_limit = hyphenated(32);
    let slug = ProjectSlug::parse(&at_limit).unwrap();
    let json = serde_json::to_string(&slug).unwrap();
    assert_eq!(json, format!("\"{at_limit}\""));
    let back: ProjectSlug = serde_json::from_str(&json).unwrap();
    assert_eq!(back, slug);
}

#[test]
fn serde_rejects_every_shape_the_parser_rejects() {
    for bad in [
        "\"a-\"",
        "\"-a\"",
        "\"a--b\"",
        "\"1-a\"",
        "\"Match\"",
        "\"café\"",
    ] {
        let parsed: Result<ProjectSlug, _> = serde_json::from_str(bad);
        assert!(parsed.is_err(), "{bad} must not deserialize");
    }
    let over_limit = format!("\"{}\"", hyphenated(33));
    let parsed: Result<ProjectSlug, _> = serde_json::from_str(&over_limit);
    assert!(parsed.is_err(), "a 33-character slug must not deserialize");
}

fn propose(input: &str) -> Result<String, ProposeError> {
    let name = ProjectName::parse(input).expect("valid ProjectName in test fixture");
    propose_slug(&name).map(|slug| slug.to_string())
}

#[test]
fn propose_strips_diacritics_without_splitting_the_word() {
    assert_eq!(propose("ÀÉÎÕÜ").unwrap(), "aeiou");
    assert_eq!(propose("Étoile Filante").unwrap(), "etoile-filante");
}

#[test]
fn propose_on_characters_that_survive_nothing_is_an_error() {
    assert!(matches!(
        propose("🎉🎉🎉"),
        Err(ProposeError::NoWords { .. })
    ));
    assert!(matches!(
        propose("!!! ???"),
        Err(ProposeError::NoWords { .. })
    ));
    assert!(matches!(
        propose("日本語"),
        Err(ProposeError::NoWords { .. })
    ));
}

#[test]
fn propose_splits_a_single_leading_letter_as_its_own_word() {
    assert_eq!(propose("iPhone App").unwrap(), "i-phone-app");
    assert_eq!(propose("iOS Companion").unwrap(), "i-os-companion");
}

#[test]
fn propose_rejects_a_reserved_single_word_case_insensitively() {
    assert!(matches!(
        propose("Match"),
        Err(ProposeError::Reserved { .. })
    ));
    assert!(matches!(
        propose("MATCH"),
        Err(ProposeError::Reserved { .. })
    ));
    assert_eq!(propose("self-hosted").unwrap(), "self-hosted");
    assert_eq!(propose("Self Hosted").unwrap(), "self-hosted");
}

#[test]
fn propose_reports_an_over_long_name_rather_than_panicking() {
    let long_name = "a".repeat(100);
    let name = ProjectName::parse(&long_name).unwrap();
    assert!(matches!(
        propose_slug(&name),
        Err(ProposeError::TooLong { .. })
    ));
}

#[test]
fn a_two_hundred_character_name_is_not_a_project_name_at_all() {
    assert!(ProjectName::parse(&"a".repeat(200)).is_err());
    assert!(ProjectName::parse(&"word ".repeat(40)).is_err());
}

#[test]
fn propose_reports_a_leading_digit_rather_than_inventing_a_prefix() {
    assert!(matches!(
        propose("2026 Vision"),
        Err(ProposeError::LeadingDigit { .. })
    ));
}

#[test]
fn every_proposed_slug_parses_back_to_itself() {
    for input in [
        "Third Thoughts",
        "Lightless Labs' Foundry",
        "HTTPServer",
        "camelCaseName",
        "Étoile",
        "Foundry 2",
    ] {
        let proposed = propose(input).unwrap();
        let reparsed = ProjectSlug::parse(&proposed).unwrap();
        assert_eq!(reparsed.to_string(), proposed, "`{input}`");
    }
}
