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
