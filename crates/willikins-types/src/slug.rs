//! Canonical, immutable slug types: ordered ASCII word lists serialised as
//! kebab-case, each with its own maximum length and the shared reserved-word
//! check. See the "Naming" section of the design doc.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use crate::reserved::is_reserved;
use crate::word::WordList;
use crate::{DomainType, ParseError};

/// Define a slug newtype over [`WordList`] with a hand-written
/// [`DomainType`], `Display`, `FromStr`, `Serialize`, `Deserialize`, and
/// [`schemars::JsonSchema`]. The derive macro is not ready yet, so every
/// slug type shares this shape via a local macro instead of duplicating it.
macro_rules! define_slug {
    (
        $(#[$meta:meta])+
        $name:ident,
        max_len = $max_len:expr,
        description = $description:expr,
        example = $example:expr $(,)?
    ) => {
        $(#[$meta])+
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(WordList);

        impl $name {
            /// The maximum length of the kebab-case serialised form.
            pub const MAX_LEN: usize = $max_len;

            /// The words that make up this slug.
            #[must_use]
            pub fn words(&self) -> &WordList {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0.kebab())
            }
        }

        impl FromStr for $name {
            type Err = ParseError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                Self::parse(input)
            }
        }

        impl DomainType for $name {
            const TYPE_NAME: &'static str = stringify!($name);

            fn description() -> &'static str {
                $description
            }

            fn example() -> &'static str {
                $example
            }

            fn parse(input: &str) -> Result<Self, ParseError> {
                let words = WordList::parse_kebab(input)
                    .map_err(|e| ParseError::new(Self::TYPE_NAME, e.reason))?;
                let kebab = words.kebab();
                if kebab.len() > Self::MAX_LEN {
                    return Err(ParseError::new(
                        Self::TYPE_NAME,
                        format!(
                            "`{kebab}` is {} characters, the limit is {}",
                            kebab.len(),
                            Self::MAX_LEN
                        ),
                    ));
                }
                if let [only] = words.words()
                    && is_reserved(only.as_str())
                {
                    return Err(ParseError::new(
                        Self::TYPE_NAME,
                        format!("`{only}` is a reserved word"),
                    ));
                }
                Ok(Self(words))
            }

            fn json_schema() -> schemars::Schema {
                schemars::schema_for!(Self)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                Self::parse(&raw).map_err(serde::de::Error::custom)
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "string",
                    "pattern": r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$",
                    "maxLength": Self::MAX_LEN,
                    "description": $description,
                    "examples": [$example]
                })
            }
        }
    };
}

define_slug! {
    /// The canonical, immutable identifier for a project. Every provider
    /// name is derived from this, never from the free-form display name.
    /// Maximum 32 characters: a single-service project uses the slug as
    /// its Railway service name, and Railway caps service names at 32.
    ProjectSlug,
    max_len = 32,
    description = "The canonical, immutable slug identifying a project.",
    example = "third-thoughts",
}

define_slug! {
    /// A component within a project, such as a Railway service or an iOS
    /// app extension. Maximum 32 characters, the same Railway-derived
    /// bound as [`ProjectSlug`].
    ComponentSlug,
    max_len = 32,
    description = "A slug identifying one component within a project.",
    example = "api",
}

define_slug! {
    /// A deployment environment, such as `dev`, `stg`, or `prd`. Maximum
    /// 16 characters.
    EnvironmentSlug,
    max_len = 16,
    description = "A slug identifying a deployment environment.",
    example = "prd",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_kebab() {
        let slug = ProjectSlug::parse("third-thoughts").unwrap();
        assert_eq!(slug.to_string(), "third-thoughts");
        assert_eq!(slug.words().kebab(), "third-thoughts");
    }

    #[test]
    fn rejects_bad_grammar() {
        assert!(ProjectSlug::parse("Third-Thoughts").is_err());
        assert!(ProjectSlug::parse("-third-thoughts").is_err());
        assert!(ProjectSlug::parse("third--thoughts").is_err());
        assert!(ProjectSlug::parse("").is_err());
    }

    #[test]
    fn rejects_too_long() {
        let too_long = "a".repeat(33);
        let err = ProjectSlug::parse(&too_long).unwrap_err();
        assert!(err.reason.contains("33"));
        assert!(err.reason.contains("32"));
    }

    #[test]
    fn accepts_exactly_at_the_limit() {
        let at_limit = "a".repeat(32);
        assert!(ProjectSlug::parse(&at_limit).is_ok());
    }

    #[test]
    fn environment_slug_has_a_tighter_limit() {
        let too_long = "a".repeat(17);
        assert!(EnvironmentSlug::parse(&too_long).is_err());
        let at_limit = "a".repeat(16);
        assert!(EnvironmentSlug::parse(&at_limit).is_ok());
    }

    #[test]
    fn single_word_reserved_is_rejected_naming_the_word() {
        let err = ProjectSlug::parse("self").unwrap_err();
        assert!(err.reason.contains("self"));
        assert!(err.reason.contains("reserved"));
    }

    #[test]
    fn multi_word_slug_is_never_reserved() {
        assert!(ProjectSlug::parse("type-system").is_ok());
        assert!(ProjectSlug::parse("self-hosted").is_ok());
    }

    #[test]
    fn acceptance_test_10_reserved_single_words_rejected() {
        for word in ["native", "default", "type", "match", "self", "nul"] {
            assert!(
                ProjectSlug::parse(word).is_err(),
                "`{word}` should be rejected as a reserved word"
            );
        }
        assert!(ProjectSlug::parse("type-system").is_ok());
    }

    #[test]
    fn serde_round_trips_through_kebab_string() {
        let slug = ProjectSlug::parse("third-thoughts").unwrap();
        let json = serde_json::to_string(&slug).unwrap();
        assert_eq!(json, "\"third-thoughts\"");
        let back: ProjectSlug = serde_json::from_str(&json).unwrap();
        assert_eq!(back, slug);
    }

    #[test]
    fn serde_deserialize_rejects_invalid() {
        let result: Result<ProjectSlug, _> = serde_json::from_str("\"Not Valid\"");
        assert!(result.is_err());
    }

    #[test]
    fn from_str_matches_parse() {
        let a: ProjectSlug = "third-thoughts".parse().unwrap();
        let b = ProjectSlug::parse("third-thoughts").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn json_schema_has_expected_shape() {
        let schema = <ProjectSlug as DomainType>::json_schema();
        let value = schema.as_value();
        assert_eq!(value["type"], "string");
        assert_eq!(value["maxLength"], 32);
        assert_eq!(value["pattern"], r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$");
    }

    #[test]
    fn max_len_constants_match_the_grammar() {
        assert_eq!(ProjectSlug::MAX_LEN, 32);
        assert_eq!(ComponentSlug::MAX_LEN, 32);
        assert_eq!(EnvironmentSlug::MAX_LEN, 16);
    }
}
