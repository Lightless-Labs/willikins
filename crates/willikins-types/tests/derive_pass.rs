//! Behavioural tests for `#[derive(DomainType)]`, one section per storage.

use willikins_types::{DomainType, SinkToken};

// ---------------------------------------------------------------------
// `String` storage
// ---------------------------------------------------------------------

#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[a-z][a-z0-9-]*",
    min_len = 2,
    max_len = 10,
    description = "A lowercase test slug",
    example = "abc"
)]
struct TestSlug(String);

#[test]
fn string_storage_accepts_a_valid_value() {
    let value = TestSlug::parse("abc-1").unwrap();
    assert_eq!(value.as_str(), "abc-1");
}

#[test]
fn string_storage_rejects_a_value_that_does_not_match_the_pattern() {
    let err = TestSlug::parse("ABC").unwrap_err();
    assert_eq!(err.type_name, "TestSlug");
    assert!(
        err.reason.contains("pattern"),
        "reason was {:?}",
        err.reason
    );
}

#[test]
fn string_storage_pattern_is_anchored_even_though_the_author_did_not_anchor_it() {
    // The written pattern is `[a-z][a-z0-9-]*`, unanchored. If the derive
    // did not anchor it, "1abc" would match on its "abc" tail.
    assert!(TestSlug::parse("1abc").is_err());
}

#[test]
fn string_storage_rejects_a_value_that_is_too_short() {
    let err = TestSlug::parse("a").unwrap_err();
    assert!(
        err.reason.contains("at least"),
        "reason was {:?}",
        err.reason
    );
}

#[test]
fn string_storage_rejects_a_value_that_is_too_long() {
    let err = TestSlug::parse("abcdefghijk").unwrap_err();
    assert!(
        err.reason.contains("at most"),
        "reason was {:?}",
        err.reason
    );
}

#[test]
fn string_storage_serde_round_trips() {
    let value = TestSlug::parse("abc-1").unwrap();
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, "\"abc-1\"");
    let back: TestSlug = serde_json::from_str(&json).unwrap();
    assert_eq!(back, value);
}

#[test]
fn string_storage_deserialize_rejects_an_invalid_value() {
    let err = serde_json::from_str::<TestSlug>("\"AB\"").unwrap_err();
    assert!(err.to_string().contains("TestSlug"), "error was {err}");
}

#[test]
fn string_storage_schema_has_the_expected_shape() {
    let schema = TestSlug::json_schema();
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("\"pattern\""), "{json}");
    assert!(json.contains("\"minLength\":2"), "{json}");
    assert!(json.contains("\"maxLength\":10"), "{json}");
    assert!(
        json.contains("\"description\":\"A lowercase test slug\""),
        "{json}"
    );
    assert!(json.contains("\"examples\":[\"abc\"]"), "{json}");
}

#[test]
fn string_storage_display_and_debug() {
    let value = TestSlug::parse("abc-1").unwrap();
    assert_eq!(value.to_string(), "abc-1");
    assert_eq!(format!("{value:?}"), "TestSlug(\"abc-1\")");
}

// ---------------------------------------------------------------------
// `secrecy::SecretString` storage
// ---------------------------------------------------------------------

#[derive(willikins_derive::DomainType)]
#[domain(
    min_len = 8,
    secret,
    description = "A test secret",
    example = "sekret-example"
)]
struct TestSecret(secrecy::SecretString);

#[test]
fn secret_storage_debug_and_display_are_redacted() {
    let value = TestSecret::parse("hunter2-hunter2").unwrap();
    assert_eq!(value.to_string(), "[REDACTED TestSecret]");
    assert_eq!(format!("{value:?}"), "[REDACTED TestSecret]");
}

#[test]
fn secret_storage_rejects_bad_input_without_echoing_it() {
    let err = TestSecret::parse("short").unwrap_err();
    assert!(
        !err.reason.contains("short"),
        "reason leaked the input: {:?}",
        err.reason
    );
    assert!(
        err.reason.contains("at least"),
        "reason was {:?}",
        err.reason
    );
}

#[test]
fn secret_storage_deserialize_works() {
    let value: TestSecret = serde_json::from_str("\"hunter2-hunter2\"").unwrap();
    let token = SinkToken::new();
    assert_eq!(value.expose(&token), "hunter2-hunter2");
}

#[test]
fn secret_storage_expose_with_sink_token_returns_the_value() {
    let value = TestSecret::parse("hunter2-hunter2").unwrap();
    let token = SinkToken::new();
    assert_eq!(value.expose(&token), "hunter2-hunter2");
}

// ---------------------------------------------------------------------
// Other (`FromStr` + `Display`) storage
// ---------------------------------------------------------------------

/// A small kebab-joined word list, local to this test, standing in for
/// `WordList` (built by a different task) to exercise the third storage.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Kebab(Vec<String>);

impl std::str::FromStr for Kebab {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err("kebab word list must not be empty".to_string());
        }
        Ok(Self(s.split('-').map(str::to_string).collect()))
    }
}

impl std::fmt::Display for Kebab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.join("-"))
    }
}

#[derive(willikins_derive::DomainType)]
#[domain(
    max_len = 12,
    description = "A kebab-joined word list",
    example = "abc-def"
)]
struct TestKebab(Kebab);

#[test]
fn other_storage_parses_via_from_str_and_displays_canonically() {
    let value = TestKebab::parse("abc-def").unwrap();
    assert_eq!(value.inner().0, vec!["abc".to_string(), "def".to_string()]);
    assert_eq!(value.to_string(), "abc-def");
}

#[test]
fn other_storage_serializes_as_the_display_string_not_an_array() {
    let value = TestKebab::parse("abc-def").unwrap();
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, "\"abc-def\"");
}

#[test]
fn other_storage_propagates_from_str_errors() {
    let err = TestKebab::parse("").unwrap_err();
    assert_eq!(err.type_name, "TestKebab");
    assert!(err.reason.contains("empty"), "reason was {:?}", err.reason);
}

#[test]
fn other_storage_applies_length_limits_to_the_canonical_form() {
    let err = TestKebab::parse("a-b-c-d-e-f-g-h-i-j-k").unwrap_err();
    assert!(
        err.reason.contains("at most"),
        "reason was {:?}",
        err.reason
    );
}

// ---------------------------------------------------------------------
// `assert_example_parses`
// ---------------------------------------------------------------------

#[test]
fn every_test_types_own_example_parses_as_itself() {
    willikins_types::assert_example_parses::<TestSlug>();
    willikins_types::assert_example_parses::<TestSecret>();
    willikins_types::assert_example_parses::<TestKebab>();
}

// ---------------------------------------------------------------------
// Adversarial pins: length counting, secret reasons, empty input,
// unconstrained storages, and the bare-name derive path
// ---------------------------------------------------------------------

#[derive(DomainType)]
#[domain(
    max_len = 3,
    description = "At most three characters, counted as characters",
    example = "abc"
)]
struct TestThreeChars(String);

#[test]
fn lengths_count_characters_not_bytes() {
    // Three multibyte characters: nine bytes, three chars.
    let value = TestThreeChars::parse("ééé").unwrap();
    assert_eq!(value.as_str(), "ééé");
    assert!(TestThreeChars::parse("éééé").is_err());
}

#[test]
fn type_name_is_the_struct_identifier() {
    assert_eq!(TestThreeChars::TYPE_NAME, "TestThreeChars");
    assert_eq!(TestSlug::TYPE_NAME, "TestSlug");
    assert_eq!(TestSecret::TYPE_NAME, "TestSecret");
    assert_eq!(TestKebab::TYPE_NAME, "TestKebab");
}

#[derive(DomainType)]
#[domain(
    pattern = "dp\\.st\\.[a-z0-9-]+",
    min_len = 10,
    secret,
    description = "A prefixed test secret",
    example = "dp.st.example-token"
)]
struct TestPrefixedSecret(secrecy::SecretString);

#[test]
fn secret_pattern_rejection_never_echoes_the_input() {
    // Long enough to pass min_len, so the regex is what rejects it.
    let err = TestPrefixedSecret::parse("LEAKME-LEAKME-LEAKME").unwrap_err();
    assert!(!err.reason.contains("LEAKME"), "reason leaked: {err:?}");
    assert!(!err.to_string().contains("LEAKME"), "Display leaked: {err}");
}

#[test]
fn secret_length_rejection_never_echoes_the_input() {
    let err = TestPrefixedSecret::parse("LEAKME").unwrap_err();
    assert!(!err.to_string().contains("LEAKME"), "Display leaked: {err}");
    assert!(
        err.reason.contains("at least"),
        "reason was {:?}",
        err.reason
    );
}

#[test]
fn secret_deserialize_failure_never_echoes_the_input() {
    let err = serde_json::from_str::<TestPrefixedSecret>("\"LEAKME-LEAKME-LEAKME\"").unwrap_err();
    assert!(
        !err.to_string().contains("LEAKME"),
        "serde error leaked: {err}"
    );
    assert!(err.to_string().contains("TestPrefixedSecret"), "{err}");
}

#[test]
fn secret_deserialized_from_json_is_redacted_afterwards() {
    let value: TestPrefixedSecret = serde_json::from_str("\"dp.st.hunter2-token\"").unwrap();
    assert_eq!(format!("{value:?}"), "[REDACTED TestPrefixedSecret]");
    assert_eq!(value.to_string(), "[REDACTED TestPrefixedSecret]");
    assert_eq!(value.expose(&SinkToken::new()), "dp.st.hunter2-token");
}

#[derive(DomainType)]
#[domain(
    description = "Anything at all, including nothing",
    example = "anything"
)]
struct TestUnconstrained(String);

#[derive(DomainType)]
#[domain(
    min_len = 1,
    description = "Anything but the empty string",
    example = "anything"
)]
struct TestNonEmpty(String);

#[test]
fn a_type_with_no_pattern_and_no_lengths_accepts_the_empty_string() {
    assert_eq!(TestUnconstrained::parse("").unwrap().as_str(), "");
    assert!(TestNonEmpty::parse("").is_err());
    assert_eq!(TestNonEmpty::parse("a").unwrap().as_str(), "a");
}

/// A `FromStr`+`Display` storage with no `pattern`, `min_len`, or
/// `max_len` at all: the generated `parse` must still compile clean under
/// `-D warnings` even though it has no check to run.
#[derive(DomainType)]
#[domain(description = "An unconstrained kebab word list", example = "abc-def")]
struct TestUnconstrainedKebab(Kebab);

#[test]
fn an_unconstrained_other_storage_parses_and_round_trips() {
    let value = TestUnconstrainedKebab::parse("abc-def").unwrap();
    assert_eq!(value.to_string(), "abc-def");
    assert!(TestUnconstrainedKebab::parse("").is_err());
}

#[test]
fn the_string_schema_carries_every_documented_keyword() {
    let schema = serde_json::to_value(TestSlug::json_schema()).unwrap();
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["pattern"], "^(?:[a-z][a-z0-9-]*)$");
    assert_eq!(schema["minLength"], 2);
    assert_eq!(schema["maxLength"], 10);
    assert_eq!(schema["description"], "A lowercase test slug");
    assert_eq!(schema["examples"], serde_json::json!(["abc"]));
}

// ---------------------------------------------------------------------
// Pattern anchoring
// ---------------------------------------------------------------------

#[derive(DomainType)]
#[domain(
    pattern = "^[a-z]+$",
    description = "A pattern the author anchored",
    example = "abc"
)]
struct TestPreAnchored(String);

#[test]
fn an_already_anchored_pattern_is_not_anchored_twice() {
    // The schema must publish the author's own pattern, unchanged.
    let schema = serde_json::to_value(TestPreAnchored::json_schema()).unwrap();
    assert_eq!(schema["pattern"], "^[a-z]+$");
    assert!(TestPreAnchored::parse("abc").is_ok());
    assert!(TestPreAnchored::parse("abc1").is_err());
}

#[derive(DomainType)]
#[domain(
    pattern = "abc|def",
    description = "A top-level alternation the author did not anchor",
    example = "abc"
)]
struct TestAlternation(String);

#[test]
fn a_top_level_alternation_is_anchored_as_a_group() {
    let schema = serde_json::to_value(TestAlternation::json_schema()).unwrap();
    assert_eq!(schema["pattern"], "^(?:abc|def)$");
    assert!(TestAlternation::parse("abc").is_ok());
    assert!(TestAlternation::parse("def").is_ok());
    // With `^abc|def$` instead of `^(?:abc|def)$`, both of these match.
    assert!(TestAlternation::parse("abcXXX").is_err());
    assert!(TestAlternation::parse("XXXdef").is_err());
}

#[derive(DomainType)]
#[domain(
    pattern = "^abc|def$",
    description = "An alternation whose branches the author anchored only at the ends",
    example = "abc"
)]
struct TestHalfAnchoredAlternation(String);

#[test]
fn an_alternation_anchored_only_at_its_ends_is_still_anchored_whole() {
    // `^abc|def$` reads as `(^abc)|(def$)`: it starts with `^` and ends
    // with `$`, yet neither branch is anchored at both ends. Treating it
    // as already anchored would accept "abcXXX" and "XXXdef".
    assert!(TestHalfAnchoredAlternation::parse("abc").is_ok());
    assert!(TestHalfAnchoredAlternation::parse("def").is_ok());
    assert!(TestHalfAnchoredAlternation::parse("abcXXX").is_err());
    assert!(TestHalfAnchoredAlternation::parse("XXXdef").is_err());
}

#[derive(DomainType)]
#[domain(
    pattern = "^price\\$",
    description = "A pattern whose trailing dollar is an escaped literal",
    example = "price$"
)]
struct TestTrailingLiteralDollar(String);

#[test]
fn a_trailing_escaped_dollar_is_not_an_end_anchor() {
    // `^price\$` ends with the character `$`, but as a literal, not an
    // anchor: without a real end anchor "price$and-more" matches.
    assert!(TestTrailingLiteralDollar::parse("price$").is_ok());
    assert!(TestTrailingLiteralDollar::parse("price$and-more").is_err());
}

#[derive(DomainType)]
#[domain(
    pattern = "^abc$|^def$",
    description = "An alternation whose every branch the author anchored",
    example = "abc"
)]
struct TestFullyAnchoredAlternation(String);

#[test]
fn an_alternation_with_every_branch_anchored_is_left_alone() {
    let schema = serde_json::to_value(TestFullyAnchoredAlternation::json_schema()).unwrap();
    assert_eq!(schema["pattern"], "^abc$|^def$");
    assert!(TestFullyAnchoredAlternation::parse("abc").is_ok());
    assert!(TestFullyAnchoredAlternation::parse("abcXXX").is_err());
}
