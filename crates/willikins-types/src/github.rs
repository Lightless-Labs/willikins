//! GitHub domain types: organisation and repository identities, visibility,
//! URLs, and Actions secret names.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use crate::slug::ProjectSlug;
use crate::{DomainType, ParseError};

/// A GitHub organisation (or user) login.
///
/// GitHub login rules: alphanumeric and hyphens, no leading, trailing, or
/// doubled hyphen, at most 39 characters. Rust's `regex` crate has no
/// lookaround, so the doubled/leading/trailing-hyphen rule is folded into
/// the pattern itself rather than expressed as a negative lookahead.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = "[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*",
    max_len = 39,
    description = "A GitHub organisation or user login.",
    example = "lightless-labs"
)]
pub struct GitHubOrg(String);

/// A GitHub repository's visibility.
///
/// Hand-written rather than derived: it is a closed enum, not a validated
/// string. Its canonical strings, `"private"` and `"public"`, are exactly
/// its two variant names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepoVisibility {
    /// Visible only to the org and explicitly granted collaborators.
    Private,
    /// Visible to anyone.
    Public,
}

impl RepoVisibility {
    /// The canonical string for this visibility.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Public => "public",
        }
    }
}

impl fmt::Display for RepoVisibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RepoVisibility {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for RepoVisibility {
    const TYPE_NAME: &'static str = "RepoVisibility";

    fn description() -> &'static str {
        "A GitHub repository's visibility: private or public."
    }

    fn example() -> &'static str {
        "private"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        match input {
            "private" => Ok(Self::Private),
            "public" => Ok(Self::Public),
            _ => Err(ParseError::new(
                Self::TYPE_NAME,
                format!("must be `private` or `public`, found `{input}`"),
            )),
        }
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for RepoVisibility {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for RepoVisibility {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for RepoVisibility {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("RepoVisibility")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "enum": ["private", "public"],
            "description": "A GitHub repository's visibility: private or public.",
            "examples": ["private"]
        })
    }
}

crate::impl_domain_object_non_secret!(RepoVisibility);

/// An `https://` URL with no embedded whitespace.
#[derive(willikins_derive::DomainType)]
#[domain(
    pattern = r"https://[^\s/]+(?:/[^\s]*)?",
    max_len = 2048,
    description = "An https:// URL.",
    example = "https://github.com/lightless-labs/third-thoughts"
)]
pub struct HttpsUrl(String);

/// A GitHub repository identity: `owner/name`.
///
/// Hand-written because its canonical string is a documented join of two
/// other domain types, not a validated string in its own right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubRepo {
    owner: GitHubOrg,
    name: ProjectSlug,
}

/// `owner/name`, with `owner` bounded by [`GitHubOrg`]'s 39-character limit
/// and `name` by [`ProjectSlug`]'s 32-character limit, plus the separator.
const GITHUB_REPO_MAX_LEN: usize = 39 + 1 + 32;

/// The published schema pattern: [`GitHubOrg`]'s pattern, a literal slash,
/// then [`ProjectSlug`]'s pattern, each with its own anchors stripped so
/// they combine into one whole-string match.
const GITHUB_REPO_PATTERN: &str = r"^[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*/[a-z][a-z0-9]*(-[a-z0-9]+)*$";

impl GitHubRepo {
    /// Build a repository identity directly from its already-parsed parts.
    #[must_use]
    pub fn new(owner: GitHubOrg, name: ProjectSlug) -> Self {
        Self { owner, name }
    }

    /// The organisation that owns this repository.
    #[must_use]
    pub fn owner(&self) -> &GitHubOrg {
        &self.owner
    }

    /// The repository's name.
    #[must_use]
    pub fn name(&self) -> &ProjectSlug {
        &self.name
    }

    /// The repository's `https://github.com/<owner>/<name>` URL.
    ///
    /// # Panics
    ///
    /// Never: `owner` and `name` are already constrained to characters
    /// [`HttpsUrl`]'s pattern accepts (no whitespace, no slash inside
    /// either part), and their combined length is far under
    /// [`HttpsUrl`]'s 2048-character limit.
    #[must_use]
    pub fn url(&self) -> crate::HttpsUrl {
        crate::HttpsUrl::parse(&format!("https://github.com/{}/{}", self.owner, self.name))
            .expect("a GitHubOrg and ProjectSlug always join into a valid HttpsUrl")
    }
}

impl fmt::Display for GitHubRepo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

impl FromStr for GitHubRepo {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for GitHubRepo {
    const TYPE_NAME: &'static str = "GitHubRepo";

    fn description() -> &'static str {
        "A GitHub repository identity: owner/name."
    }

    fn example() -> &'static str {
        "lightless-labs/third-thoughts"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        if input.matches('/').count() != 1 {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                "must be exactly `owner/name`, with a single `/`",
            ));
        }
        let (owner, name) = input
            .split_once('/')
            .expect("checked above: input contains exactly one `/`");
        let owner = GitHubOrg::parse(owner)
            .map_err(|err| ParseError::new(Self::TYPE_NAME, format!("owner: {}", err.reason)))?;
        let name = ProjectSlug::parse(name)
            .map_err(|err| ParseError::new(Self::TYPE_NAME, format!("name: {}", err.reason)))?;
        Ok(Self { owner, name })
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for GitHubRepo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for GitHubRepo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for GitHubRepo {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("GitHubRepo")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": GITHUB_REPO_PATTERN,
            "maxLength": GITHUB_REPO_MAX_LEN,
            "description": "A GitHub repository identity: owner/name.",
            "examples": ["lightless-labs/third-thoughts"]
        })
    }
}

crate::impl_domain_object_non_secret!(GitHubRepo);

/// A GitHub Actions repository secret name.
///
/// Hand-written because the `GITHUB_`-prefix rejection has no expression
/// in Rust's lookaround-free regex; it is a separate, named check instead.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionsSecretName(String);

/// GitHub reserves this prefix for its own automatically supplied secrets.
const RESERVED_PREFIX: &str = "GITHUB_";

impl ActionsSecretName {
    /// The maximum length, in characters.
    pub const MAX_LEN: usize = 200;

    /// Borrow the canonical string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ActionsSecretName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ActionsSecretName {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for ActionsSecretName {
    const TYPE_NAME: &'static str = "ActionsSecretName";

    fn description() -> &'static str {
        "A GitHub Actions repository secret name. Must not start with GITHUB_, which GitHub reserves for its own automatically supplied secrets."
    }

    fn example() -> &'static str {
        "DOPPLER_TOKEN"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        let mut chars = input.chars();
        let Some(first) = chars.next() else {
            return Err(ParseError::new(Self::TYPE_NAME, "must not be empty"));
        };
        if !(first.is_ascii_uppercase() || first == '_') {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("must start with an uppercase ASCII letter or `_`, found `{first}`"),
            ));
        }
        if let Some(bad) =
            chars.find(|c| !(c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_'))
        {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!(
                    "must contain only uppercase ASCII letters, digits, and `_`, found `{bad}`"
                ),
            ));
        }
        let len = input.chars().count();
        if len > Self::MAX_LEN {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("is {len} characters, the limit is {}", Self::MAX_LEN),
            ));
        }
        if input.starts_with(RESERVED_PREFIX) {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("must not start with `{RESERVED_PREFIX}`, which GitHub reserves"),
            ));
        }
        Ok(Self(input.to_string()))
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for ActionsSecretName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for ActionsSecretName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for ActionsSecretName {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ActionsSecretName")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": r"^[A-Z_][A-Z0-9_]*$",
            "maxLength": ActionsSecretName::MAX_LEN,
            "description": "A GitHub Actions repository secret name. Must not start with GITHUB_, which GitHub reserves for its own automatically supplied secrets.",
            "examples": ["DOPPLER_TOKEN"]
        })
    }
}

crate::impl_domain_object_non_secret!(ActionsSecretName);

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------
    // GitHubOrg
    // -------------------------------------------------------------

    #[test]
    fn github_org_accepts_a_valid_login() {
        assert_eq!(
            GitHubOrg::parse("lightless-labs").unwrap().as_str(),
            "lightless-labs"
        );
        assert!(GitHubOrg::parse("a").is_ok());
    }

    #[test]
    fn github_org_rejects_leading_hyphen() {
        assert!(GitHubOrg::parse("-lightless").is_err());
    }

    #[test]
    fn github_org_rejects_trailing_hyphen() {
        assert!(GitHubOrg::parse("lightless-").is_err());
    }

    #[test]
    fn github_org_rejects_doubled_hyphen() {
        assert!(GitHubOrg::parse("light--less").is_err());
    }

    #[test]
    fn github_org_rejects_over_max_len() {
        let too_long = "a".repeat(40);
        assert!(GitHubOrg::parse(&too_long).is_err());
    }

    #[test]
    fn github_org_accepts_exactly_max_len() {
        let at_limit = "a".repeat(39);
        assert!(GitHubOrg::parse(&at_limit).is_ok());
    }

    #[test]
    fn github_org_serde_round_trips() {
        let value = GitHubOrg::parse("lightless-labs").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"lightless-labs\"");
        assert_eq!(serde_json::from_str::<GitHubOrg>(&json).unwrap(), value);
    }

    // -------------------------------------------------------------
    // RepoVisibility
    // -------------------------------------------------------------

    #[test]
    fn repo_visibility_parses_both_variants() {
        assert_eq!(
            RepoVisibility::parse("private").unwrap(),
            RepoVisibility::Private
        );
        assert_eq!(
            RepoVisibility::parse("public").unwrap(),
            RepoVisibility::Public
        );
    }

    #[test]
    fn repo_visibility_rejects_anything_else_naming_the_rule() {
        let err = RepoVisibility::parse("internal").unwrap_err();
        assert!(err.reason.contains("private"));
        assert!(err.reason.contains("public"));
    }

    #[test]
    fn repo_visibility_displays_its_canonical_string() {
        assert_eq!(RepoVisibility::Private.to_string(), "private");
        assert_eq!(RepoVisibility::Public.to_string(), "public");
    }

    #[test]
    fn repo_visibility_serde_round_trips() {
        let json = serde_json::to_string(&RepoVisibility::Private).unwrap();
        assert_eq!(json, "\"private\"");
        assert_eq!(
            serde_json::from_str::<RepoVisibility>(&json).unwrap(),
            RepoVisibility::Private
        );
    }

    #[test]
    fn repo_visibility_deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<RepoVisibility>("\"internal\"").is_err());
    }

    #[test]
    fn repo_visibility_schema_is_a_string_enum() {
        let schema = serde_json::to_value(RepoVisibility::json_schema()).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["enum"], serde_json::json!(["private", "public"]));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn repo_visibility_domain_object_view() {
        use crate::DomainObject;
        let value: Box<dyn DomainObject> = Box::new(RepoVisibility::Public);
        assert_eq!(value.type_name(), "RepoVisibility");
        assert!(!value.is_secret());
        assert_eq!(value.expose(&crate::SinkToken::new()), "public");
    }

    // -------------------------------------------------------------
    // HttpsUrl
    // -------------------------------------------------------------

    #[test]
    fn https_url_accepts_a_valid_url() {
        assert!(HttpsUrl::parse("https://github.com/lightless-labs/third-thoughts").is_ok());
    }

    #[test]
    fn https_url_rejects_http() {
        assert!(HttpsUrl::parse("http://github.com/org/repo").is_err());
    }

    #[test]
    fn https_url_rejects_embedded_whitespace() {
        assert!(HttpsUrl::parse("https://github.com/lightless labs/repo").is_err());
    }

    #[test]
    fn https_url_rejects_over_max_len() {
        let too_long = format!("https://example.com/{}", "a".repeat(2048));
        assert!(HttpsUrl::parse(&too_long).is_err());
    }

    #[test]
    fn https_url_accepts_no_path() {
        assert!(HttpsUrl::parse("https://example.com").is_ok());
    }

    #[test]
    fn https_url_serde_round_trips() {
        let value = HttpsUrl::parse("https://github.com/lightless-labs/third-thoughts").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"https://github.com/lightless-labs/third-thoughts\"");
        assert_eq!(serde_json::from_str::<HttpsUrl>(&json).unwrap(), value);
    }

    // -------------------------------------------------------------
    // GitHubRepo
    // -------------------------------------------------------------

    #[test]
    fn github_repo_parses_owner_slash_name() {
        let repo = GitHubRepo::parse("lightless-labs/third-thoughts").unwrap();
        assert_eq!(repo.owner().as_str(), "lightless-labs");
        assert_eq!(repo.name().to_string(), "third-thoughts");
        assert_eq!(repo.to_string(), "lightless-labs/third-thoughts");
    }

    #[test]
    fn github_repo_rejects_missing_slash() {
        let err = GitHubRepo::parse("lightless-labs").unwrap_err();
        assert_eq!(err.type_name, "GitHubRepo");
        assert!(err.reason.contains('/'));
    }

    #[test]
    fn github_repo_rejects_more_than_one_slash() {
        assert!(GitHubRepo::parse("lightless-labs/third/thoughts").is_err());
    }

    #[test]
    fn github_repo_propagates_the_owner_error_naming_which_part_failed() {
        let err = GitHubRepo::parse("-bad-owner/third-thoughts").unwrap_err();
        assert_eq!(err.type_name, "GitHubRepo");
        assert!(
            err.reason.starts_with("owner:"),
            "reason was {:?}",
            err.reason
        );
    }

    #[test]
    fn github_repo_propagates_the_name_error_naming_which_part_failed() {
        let err = GitHubRepo::parse("lightless-labs/Not-A-Slug").unwrap_err();
        assert_eq!(err.type_name, "GitHubRepo");
        assert!(
            err.reason.starts_with("name:"),
            "reason was {:?}",
            err.reason
        );
    }

    #[test]
    fn github_repo_url_is_the_expected_https_url() {
        let repo = GitHubRepo::parse("lightless-labs/third-thoughts").unwrap();
        assert_eq!(
            repo.url().to_string(),
            "https://github.com/lightless-labs/third-thoughts"
        );
    }

    #[test]
    fn github_repo_url_never_panics_at_the_length_extremes() {
        let owner = GitHubOrg::parse(&"a".repeat(39)).unwrap();
        let name = ProjectSlug::parse(&"a".repeat(32)).unwrap();
        let repo = GitHubRepo::new(owner, name);
        // Must not panic.
        let _ = repo.url();
    }

    #[test]
    fn github_repo_serde_round_trips() {
        let repo = GitHubRepo::parse("lightless-labs/third-thoughts").unwrap();
        let json = serde_json::to_string(&repo).unwrap();
        assert_eq!(json, "\"lightless-labs/third-thoughts\"");
        assert_eq!(serde_json::from_str::<GitHubRepo>(&json).unwrap(), repo);
    }

    #[test]
    fn github_repo_schema_shape() {
        let schema = serde_json::to_value(GitHubRepo::json_schema()).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["maxLength"], 72);
        let repo = GitHubRepo::parse("lightless-labs/third-thoughts").unwrap();
        let pattern = regex::Regex::new(schema["pattern"].as_str().unwrap()).unwrap();
        assert!(pattern.is_match(&repo.to_string()));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn github_repo_domain_object_view() {
        use crate::DomainObject;
        let repo = GitHubRepo::parse("lightless-labs/third-thoughts").unwrap();
        let value: Box<dyn DomainObject> = Box::new(repo.clone());
        assert_eq!(value.type_name(), "GitHubRepo");
        assert_eq!(value.expose(&crate::SinkToken::new()), repo.to_string());
    }

    // -------------------------------------------------------------
    // ActionsSecretName
    // -------------------------------------------------------------

    #[test]
    fn actions_secret_name_accepts_a_valid_name() {
        assert_eq!(
            ActionsSecretName::parse("DOPPLER_TOKEN").unwrap().as_str(),
            "DOPPLER_TOKEN"
        );
    }

    #[test]
    fn actions_secret_name_rejects_lowercase() {
        assert!(ActionsSecretName::parse("doppler_token").is_err());
    }

    #[test]
    fn actions_secret_name_rejects_leading_digit() {
        assert!(ActionsSecretName::parse("1TOKEN").is_err());
    }

    #[test]
    fn actions_secret_name_accepts_leading_underscore() {
        assert!(ActionsSecretName::parse("_TOKEN").is_ok());
    }

    #[test]
    fn actions_secret_name_rejects_the_reserved_github_prefix_naming_the_rule() {
        let err = ActionsSecretName::parse("GITHUB_TOKEN").unwrap_err();
        assert!(err.reason.contains("GITHUB_"));
        assert!(err.reason.contains("reserves"));
    }

    #[test]
    fn actions_secret_name_rejects_over_max_len() {
        let too_long = format!("A{}", "A".repeat(200));
        assert!(ActionsSecretName::parse(&too_long).is_err());
    }

    #[test]
    fn actions_secret_name_accepts_exactly_max_len() {
        let at_limit = format!("A{}", "A".repeat(199));
        assert_eq!(at_limit.len(), 200);
        assert!(ActionsSecretName::parse(&at_limit).is_ok());
    }

    #[test]
    fn actions_secret_name_serde_round_trips() {
        let value = ActionsSecretName::parse("DOPPLER_TOKEN").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"DOPPLER_TOKEN\"");
        assert_eq!(
            serde_json::from_str::<ActionsSecretName>(&json).unwrap(),
            value
        );
    }

    #[test]
    fn actions_secret_name_deserialize_rejects_the_github_prefix() {
        assert!(serde_json::from_str::<ActionsSecretName>("\"GITHUB_TOKEN\"").is_err());
    }

    #[test]
    fn actions_secret_name_schema_shape() {
        let schema = serde_json::to_value(ActionsSecretName::json_schema()).unwrap();
        assert_eq!(schema["type"], "string");
        assert_eq!(schema["pattern"], "^[A-Z_][A-Z0-9_]*$");
        assert_eq!(schema["maxLength"], 200);
    }

    // -------------------------------------------------------------
    // Catalog examples
    // -------------------------------------------------------------

    #[test]
    fn examples_parse_as_their_own_types() {
        crate::assert_example_parses::<GitHubOrg>();
        crate::assert_example_parses::<HttpsUrl>();
        crate::assert_example_parses::<GitHubRepo>();
        crate::assert_example_parses::<ActionsSecretName>();
        assert!(RepoVisibility::parse(RepoVisibility::example()).is_ok());
    }
}
