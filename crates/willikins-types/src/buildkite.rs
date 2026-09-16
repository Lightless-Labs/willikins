//! Buildkite domain types: organisation, pipeline slug, cluster identity,
//! and a human-written cluster lookup name.
//!
//! See `docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`
//! ("The type table") and `docs/research/2026-09-16-m3a-buildkite.md` for
//! the facts each grammar rests on.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use crate::name::is_invisible_or_bidi_control;
use crate::{DomainType, ParseError};

/// A Buildkite organisation slug.
///
/// Buildkite documents no grammar for `{org.slug}` anywhere in its REST
/// docs (research note, "Verify with a browser", item 12): this pattern
/// is chosen conservatively -- the shape Buildkite issues
/// (`willikins-test`) -- so an organisation slug with an underscore or a
/// capital letter is refused at parse time with a named error rather than
/// reaching a request line unchecked. It is a path segment in every
/// Buildkite request this crate's provider builds, and parsing at the
/// boundary is what makes it impossible for a `/`, `?` or `&` to reach
/// one -- the same argument that keeps [`crate::DopplerProject`] a type
/// rather than a `String`.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[a-z0-9]+(?:-[a-z0-9]+)*",
    max_len = 100,
    description = "A Buildkite organisation slug.",
    example = "willikins-test"
)]
pub struct BuildkiteOrg(String);

/// A Buildkite pipeline slug: the pipeline's natural key.
///
/// Buildkite's own documented regex for a slug it derives is
/// `\A[a-zA-Z0-9]+[a-zA-Z0-9\-]*\z`, maximum 100 characters (research
/// note, section 1, "Slug derivation"). This type is that grammar with
/// uppercase removed -- not a policy narrowing but natural-key
/// canonicalisation: Buildkite lowercases a slug it derives from a name,
/// and whether an explicitly supplied uppercase slug is preserved or
/// folded on the way in is undocumented, so a key that might not
/// round-trip is not a key. `naming::v1` emits only lowercase, so nothing
/// a document can express is lost by refusing uppercase here.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[a-z0-9][a-z0-9-]*",
    max_len = 100,
    description = "A Buildkite pipeline slug.",
    example = "third-thoughts"
)]
pub struct BuildkitePipelineSlug(String);

/// A Buildkite cluster's opaque UUID identity.
///
/// Lowercase hex only: a mis-cased id would compare unequal to the
/// provider's own response and report a cluster mismatch that does not
/// exist. Compared exactly on every `read` to decide
/// `Observation::Mismatch` in `buildkite.pipeline.ensure`; a free string
/// here would turn a typo into a provider error at apply time instead of
/// a parse error at `describe` time. The pattern's five hyphen-separated
/// hex groups (8-4-4-4-12) already fix the length at exactly 36
/// characters, so no separate `min_len`/`max_len` is needed.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
    description = "A Buildkite cluster's UUID.",
    example = "018e5a22-d14c-7085-bb28-db0f83f43a1c"
)]
pub struct BuildkiteClusterId(String);

/// The maximum length of a [`BuildkiteClusterName`], in characters.
const CLUSTER_NAME_MAX_LEN: usize = 255;

/// A human-written Buildkite cluster name, such as `Default cluster`.
///
/// Never a natural key: a cluster name is mutable and nowhere documented
/// unique within an organisation (research note, section 2, "A cluster
/// name is not a stable, unique natural key"). It is a lookup key a human
/// writes and reads, compared against provider-returned names and quoted
/// back in a `NotFound` message an agent reads, so -- following
/// [`crate::Description`] -- it may carry no control character other
/// than space and none of the invisible or bidirectional characters that
/// could hide or reorder what a reader sees. Hand-written, like
/// [`crate::Description`], because the no-control-except-space rule has
/// no expression in `#[derive(DomainType)]`'s pattern-only validation.
///
/// 1 to 255 characters: unlike `Description`, empty is refused, because a
/// lookup key with no text cannot name anything to look up.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BuildkiteClusterName(String);

impl BuildkiteClusterName {
    /// The cluster name text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BuildkiteClusterName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for BuildkiteClusterName {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for BuildkiteClusterName {
    const TYPE_NAME: &'static str = "BuildkiteClusterName";

    fn description() -> &'static str {
        "A human-written Buildkite cluster name, such as `Default cluster`."
    }

    fn example() -> &'static str {
        "Default cluster"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        if input.is_empty() {
            return Err(ParseError::new(Self::TYPE_NAME, "must not be empty"));
        }
        let len = input.chars().count();
        if len > CLUSTER_NAME_MAX_LEN {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("is {len} characters, the limit is {CLUSTER_NAME_MAX_LEN}"),
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

impl serde::Serialize for BuildkiteClusterName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for BuildkiteClusterName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for BuildkiteClusterName {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("BuildkiteClusterName")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": CLUSTER_NAME_MAX_LEN,
            "description": "A human-written Buildkite cluster name, such as `Default cluster`.",
            "examples": ["Default cluster"]
        })
    }
}

crate::impl_domain_object_non_secret!(BuildkiteClusterName);

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------
    // BuildkiteOrg
    // -------------------------------------------------------------

    #[test]
    fn buildkite_org_accepts_a_valid_slug() {
        assert_eq!(
            BuildkiteOrg::parse("willikins-test").unwrap().as_str(),
            "willikins-test"
        );
    }

    #[test]
    fn buildkite_org_rejects_uppercase() {
        assert!(BuildkiteOrg::parse("Willikins-Test").is_err());
    }

    #[test]
    fn buildkite_org_rejects_leading_hyphen() {
        assert!(BuildkiteOrg::parse("-willikins").is_err());
    }

    #[test]
    fn buildkite_org_rejects_slash() {
        assert!(BuildkiteOrg::parse("willikins/test").is_err());
    }

    #[test]
    fn buildkite_org_rejects_question_mark() {
        assert!(BuildkiteOrg::parse("willikins?test").is_err());
    }

    #[test]
    fn buildkite_org_rejects_ampersand() {
        assert!(BuildkiteOrg::parse("willikins&test").is_err());
    }

    #[test]
    fn buildkite_org_rejects_underscore() {
        assert!(BuildkiteOrg::parse("willikins_test").is_err());
    }

    #[test]
    fn buildkite_org_rejects_empty_string() {
        assert!(BuildkiteOrg::parse("").is_err());
    }

    #[test]
    fn buildkite_org_rejects_over_max_len() {
        let too_long = "a".repeat(101);
        assert!(BuildkiteOrg::parse(&too_long).is_err());
    }

    #[test]
    fn buildkite_org_accepts_exactly_max_len() {
        let at_limit = "a".repeat(100);
        assert!(BuildkiteOrg::parse(&at_limit).is_ok());
    }

    #[test]
    fn buildkite_org_serde_round_trips() {
        let value = BuildkiteOrg::parse("willikins-test").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"willikins-test\"");
        assert_eq!(serde_json::from_str::<BuildkiteOrg>(&json).unwrap(), value);
    }

    // -------------------------------------------------------------
    // BuildkitePipelineSlug
    // -------------------------------------------------------------

    #[test]
    fn buildkite_pipeline_slug_accepts_a_valid_slug() {
        assert_eq!(
            BuildkitePipelineSlug::parse("third-thoughts")
                .unwrap()
                .as_str(),
            "third-thoughts"
        );
    }

    #[test]
    fn buildkite_pipeline_slug_accepts_a_100_character_slug() {
        let at_limit = "a".repeat(100);
        assert!(BuildkitePipelineSlug::parse(&at_limit).is_ok());
    }

    #[test]
    fn buildkite_pipeline_slug_rejects_uppercase_naming_the_type() {
        let err = BuildkitePipelineSlug::parse("Third-Thoughts").unwrap_err();
        assert_eq!(err.type_name, "BuildkitePipelineSlug");
    }

    #[test]
    fn buildkite_pipeline_slug_rejects_leading_hyphen_naming_the_type() {
        let err = BuildkitePipelineSlug::parse("-third-thoughts").unwrap_err();
        assert_eq!(err.type_name, "BuildkitePipelineSlug");
    }

    #[test]
    fn buildkite_pipeline_slug_rejects_underscore_naming_the_type() {
        let err = BuildkitePipelineSlug::parse("third_thoughts").unwrap_err();
        assert_eq!(err.type_name, "BuildkitePipelineSlug");
    }

    #[test]
    fn buildkite_pipeline_slug_rejects_dot_naming_the_type() {
        let err = BuildkitePipelineSlug::parse("third.thoughts").unwrap_err();
        assert_eq!(err.type_name, "BuildkitePipelineSlug");
    }

    #[test]
    fn buildkite_pipeline_slug_rejects_empty_string_naming_the_type() {
        let err = BuildkitePipelineSlug::parse("").unwrap_err();
        assert_eq!(err.type_name, "BuildkitePipelineSlug");
    }

    #[test]
    fn buildkite_pipeline_slug_rejects_101_characters_naming_the_type() {
        let too_long = "a".repeat(101);
        let err = BuildkitePipelineSlug::parse(&too_long).unwrap_err();
        assert_eq!(err.type_name, "BuildkitePipelineSlug");
    }

    #[test]
    fn buildkite_pipeline_slug_serde_round_trips() {
        let value = BuildkitePipelineSlug::parse("third-thoughts").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"third-thoughts\"");
        assert_eq!(
            serde_json::from_str::<BuildkitePipelineSlug>(&json).unwrap(),
            value
        );
    }

    // -------------------------------------------------------------
    // BuildkiteClusterId
    // -------------------------------------------------------------

    #[test]
    fn buildkite_cluster_id_accepts_the_documented_example() {
        assert_eq!(
            BuildkiteClusterId::parse("018e5a22-d14c-7085-bb28-db0f83f43a1c")
                .unwrap()
                .as_str(),
            "018e5a22-d14c-7085-bb28-db0f83f43a1c"
        );
    }

    #[test]
    fn buildkite_cluster_id_rejects_uppercase_hex() {
        assert!(BuildkiteClusterId::parse("018E5A22-d14c-7085-bb28-db0f83f43a1c").is_err());
    }

    #[test]
    fn buildkite_cluster_id_rejects_a_missing_group() {
        assert!(BuildkiteClusterId::parse("018e5a22-d14c-7085-db0f83f43a1c").is_err());
    }

    #[test]
    fn buildkite_cluster_id_rejects_a_trailing_character() {
        assert!(BuildkiteClusterId::parse("018e5a22-d14c-7085-bb28-db0f83f43a1cx").is_err());
    }

    #[test]
    fn buildkite_cluster_id_serde_round_trips() {
        let value = BuildkiteClusterId::parse("018e5a22-d14c-7085-bb28-db0f83f43a1c").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"018e5a22-d14c-7085-bb28-db0f83f43a1c\"");
        assert_eq!(
            serde_json::from_str::<BuildkiteClusterId>(&json).unwrap(),
            value
        );
    }

    // -------------------------------------------------------------
    // BuildkiteClusterName
    // -------------------------------------------------------------

    #[test]
    fn buildkite_cluster_name_accepts_default_cluster() {
        assert_eq!(
            BuildkiteClusterName::parse("Default cluster")
                .unwrap()
                .as_str(),
            "Default cluster"
        );
    }

    #[test]
    fn buildkite_cluster_name_rejects_tab() {
        assert!(BuildkiteClusterName::parse("a\tb").is_err());
    }

    #[test]
    fn buildkite_cluster_name_rejects_newline() {
        assert!(BuildkiteClusterName::parse("a\nb").is_err());
    }

    #[test]
    fn buildkite_cluster_name_rejects_line_separator() {
        assert!(BuildkiteClusterName::parse("a\u{2028}b").is_err());
    }

    #[test]
    fn buildkite_cluster_name_rejects_invisible_and_bidi_characters() {
        assert!(BuildkiteClusterName::parse("a\u{200B}b").is_err());
        assert!(BuildkiteClusterName::parse("a\u{202E}b").is_err());
    }

    #[test]
    fn buildkite_cluster_name_rejects_empty_string() {
        assert!(BuildkiteClusterName::parse("").is_err());
    }

    #[test]
    fn buildkite_cluster_name_rejects_256_characters() {
        let too_long = "a".repeat(256);
        assert!(BuildkiteClusterName::parse(&too_long).is_err());
    }

    #[test]
    fn buildkite_cluster_name_accepts_exactly_255_characters() {
        let at_limit = "a".repeat(255);
        assert!(BuildkiteClusterName::parse(&at_limit).is_ok());
    }

    #[test]
    fn buildkite_cluster_name_accepts_a_space() {
        assert!(BuildkiteClusterName::parse("a b").is_ok());
    }

    #[test]
    fn buildkite_cluster_name_serde_round_trips() {
        let value = BuildkiteClusterName::parse("Default cluster").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"Default cluster\"");
        assert_eq!(
            serde_json::from_str::<BuildkiteClusterName>(&json).unwrap(),
            value
        );
    }

    #[test]
    fn buildkite_cluster_name_schema_shape() {
        let schema = serde_json::to_value(BuildkiteClusterName::json_schema()).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["minLength"], 1);
        assert_eq!(schema["maxLength"], 255);
    }

    #[test]
    fn implements_domain_object_via_the_macro() {
        use crate::DomainObject;

        let value: Box<dyn DomainObject> =
            Box::new(BuildkiteClusterName::parse("Default cluster").unwrap());
        assert_eq!(value.type_name(), "BuildkiteClusterName");
        assert!(!value.is_secret());
        assert_eq!(
            value.render(),
            crate::Rendered::Plain("Default cluster".to_string())
        );
    }

    // -------------------------------------------------------------
    // Catalog examples
    // -------------------------------------------------------------

    #[test]
    fn examples_parse_as_their_own_types() {
        crate::assert_example_parses::<BuildkiteOrg>();
        crate::assert_example_parses::<BuildkitePipelineSlug>();
        crate::assert_example_parses::<BuildkiteClusterId>();
        crate::assert_example_parses::<BuildkiteClusterName>();
    }
}
