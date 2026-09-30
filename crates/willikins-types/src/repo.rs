//! Domain types for writing files into a repository: [`RepoPath`],
//! [`GitBranchName`], [`CommitHeadline`], and [`RepoFile`].
//!
//! See `docs/plans/2026-09-30-milestone-3g-file-writing.md`, decision (g).
//! All four are hand-written rather than derived: each carries a rule
//! Rust's lookaround-free `regex` crate cannot express in one pattern (a
//! segment's *whole* value versus its characters, a case-insensitive
//! refusal, a per-component suffix rule) -- the same reason
//! [`crate::github::ActionsSecretName`] and [`crate::github::GitHubRepo`]
//! are hand-written.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use crate::{DomainType, ParseError};

/// The maximum length of a [`RepoPath`], in characters.
const REPO_PATH_MAX_LEN: usize = 1024;
/// The greatest number of `/`-separated segments a [`RepoPath`] may hold.
const REPO_PATH_MAX_SEGMENTS: usize = 32;

/// Whether `c` is a character [`RepoPath`] allows inside a segment.
fn is_repo_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '+' | '-')
}

/// A path inside a repository: `/`-separated segments, each
/// `[A-Za-z0-9._@+-]+`. No leading or trailing `/`, no empty segment, no
/// segment that is exactly `.` or `..`, at most 32 segments and 1,024
/// characters.
///
/// Refuses a `.git` segment (any case) and a path whose first two
/// segments are `.github/workflows` (any case) -- milestone 3g decision
/// (g): so a document can never write into `.git/` or a GitHub Actions
/// workflow file, and `github.scaffold.ensure`'s write token therefore
/// never needs the Workflows permission GitHub requires for that
/// directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoPath(String);

impl RepoPath {
    /// Borrow the canonical `/`-separated string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path's `/`-separated segments, in order.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RepoPath {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for RepoPath {
    const TYPE_NAME: &'static str = "RepoPath";

    fn description() -> &'static str {
        "A `/`-separated path inside a repository. Refuses `.git` segments and `.github/workflows/`."
    }

    fn example() -> &'static str {
        "apps/walter/ios/BUILD.bazel"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        if input.is_empty() {
            return Err(ParseError::new(Self::TYPE_NAME, "must not be empty"));
        }
        let len = input.chars().count();
        if len > REPO_PATH_MAX_LEN {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("is {len} characters, the limit is {REPO_PATH_MAX_LEN}"),
            ));
        }
        if input.starts_with('/') || input.ends_with('/') {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                "must not start or end with `/`",
            ));
        }
        let segments: Vec<&str> = input.split('/').collect();
        if segments.len() > REPO_PATH_MAX_SEGMENTS {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!(
                    "has {} segments, the limit is {REPO_PATH_MAX_SEGMENTS}",
                    segments.len()
                ),
            ));
        }
        for segment in &segments {
            if segment.is_empty() {
                return Err(ParseError::new(
                    Self::TYPE_NAME,
                    "must not contain an empty segment (a doubled `/`)",
                ));
            }
            if *segment == "." || *segment == ".." {
                return Err(ParseError::new(
                    Self::TYPE_NAME,
                    format!("must not contain a `{segment}` segment"),
                ));
            }
            if let Some(c) = segment.chars().find(|c| !is_repo_path_char(*c)) {
                return Err(ParseError::new(
                    Self::TYPE_NAME,
                    format!(
                        "segment {} contains a disallowed character {c:?}",
                        crate::quoted(segment)
                    ),
                ));
            }
            if segment.eq_ignore_ascii_case(".git") {
                return Err(ParseError::new(
                    Self::TYPE_NAME,
                    "must not contain a `.git` segment",
                ));
            }
        }
        if segments.len() >= 2
            && segments[0].eq_ignore_ascii_case(".github")
            && segments[1].eq_ignore_ascii_case("workflows")
        {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                "must not write under `.github/workflows/`",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for RepoPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for RepoPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for RepoPath {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("RepoPath")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "maxLength": REPO_PATH_MAX_LEN,
            "description": "A `/`-separated path inside a repository. Refuses `.git` segments and `.github/workflows/`.",
            "examples": ["apps/walter/ios/BUILD.bazel"]
        })
    }
}

crate::impl_domain_object_non_secret!(RepoPath);

/// The maximum length of a [`GitBranchName`], in characters.
const GIT_BRANCH_NAME_MAX_LEN: usize = 100;

/// Whether `c` is a character [`GitBranchName`] allows.
fn is_git_branch_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-')
}

/// A git branch name: `[A-Za-z0-9._/-]+`, at most 100 characters -- a
/// **subset** of `git check-ref-format`, not the whole rule (milestone 3g
/// decision (g)). Refuses, over the whole name: `..`, `//`, a leading `/`
/// or `-`, a trailing `/`, and `@{` anywhere. Refuses, per `/`-separated
/// component (git's own rule applies these to each component, not only to
/// the whole ref, so `a/.b` and `a.lock/b` are invalid refs exactly like
/// `.b` and `a.lock` on their own): a component starting with `.`, and a
/// component ending in `.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitBranchName(String);

impl GitBranchName {
    /// Borrow the canonical string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GitBranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for GitBranchName {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for GitBranchName {
    const TYPE_NAME: &'static str = "GitBranchName";

    fn description() -> &'static str {
        "A git branch name, a subset of git's own check-ref-format rules."
    }

    fn example() -> &'static str {
        "main"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        if input.is_empty() {
            return Err(ParseError::new(Self::TYPE_NAME, "must not be empty"));
        }
        let len = input.chars().count();
        if len > GIT_BRANCH_NAME_MAX_LEN {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("is {len} characters, the limit is {GIT_BRANCH_NAME_MAX_LEN}"),
            ));
        }
        if let Some(c) = input.chars().find(|c| !is_git_branch_name_char(*c)) {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("contains a disallowed character {c:?}"),
            ));
        }
        if input.contains("..") {
            return Err(ParseError::new(Self::TYPE_NAME, "must not contain `..`"));
        }
        if input.contains("//") {
            return Err(ParseError::new(Self::TYPE_NAME, "must not contain `//`"));
        }
        if input.contains("@{") {
            return Err(ParseError::new(Self::TYPE_NAME, "must not contain `@{`"));
        }
        if input.starts_with('/') || input.starts_with('-') {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                "must not start with `/` or `-`",
            ));
        }
        if input.ends_with('/') {
            return Err(ParseError::new(Self::TYPE_NAME, "must not end with `/`"));
        }
        for component in input.split('/') {
            if component.starts_with('.') {
                return Err(ParseError::new(
                    Self::TYPE_NAME,
                    "no `/`-separated component may start with `.`",
                ));
            }
            // This is git's own literal ref-format suffix rule, not a file
            // extension check, and the rule is case-sensitive: a component
            // ending `.LOCK` is a different, valid ref name.
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            let ends_with_lock = component.ends_with(".lock");
            if ends_with_lock {
                return Err(ParseError::new(
                    Self::TYPE_NAME,
                    "no `/`-separated component may end with `.lock`",
                ));
            }
        }
        Ok(Self(input.to_owned()))
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for GitBranchName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for GitBranchName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for GitBranchName {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("GitBranchName")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "maxLength": GIT_BRANCH_NAME_MAX_LEN,
            "description": "A git branch name, a subset of git's own check-ref-format rules.",
            "examples": ["main"]
        })
    }
}

crate::impl_domain_object_non_secret!(GitBranchName);

/// The maximum length of a [`CommitHeadline`], in characters.
const COMMIT_HEADLINE_MAX_LEN: usize = 72;

/// A commit message's first line: 1 to 72 characters, no control
/// character (which also makes it exactly one line -- `char::is_control`
/// covers `\n` and `\r`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitHeadline(String);

impl CommitHeadline {
    /// Borrow the headline text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommitHeadline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for CommitHeadline {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for CommitHeadline {
    const TYPE_NAME: &'static str = "CommitHeadline";

    fn description() -> &'static str {
        "A commit message's first line: 1 to 72 characters, one line, no control character."
    }

    fn example() -> &'static str {
        "feat(walter): scaffold the iOS app, NSE, widgets and Buildkite files"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        let len = input.chars().count();
        if len == 0 {
            return Err(ParseError::new(Self::TYPE_NAME, "must not be empty"));
        }
        if len > COMMIT_HEADLINE_MAX_LEN {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("is {len} characters, the limit is {COMMIT_HEADLINE_MAX_LEN}"),
            ));
        }
        if let Some(c) = input.chars().find(|c| c.is_control()) {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                format!("must not contain control characters (found {c:?})"),
            ));
        }
        Ok(Self(input.to_owned()))
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for CommitHeadline {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for CommitHeadline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for CommitHeadline {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CommitHeadline")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": COMMIT_HEADLINE_MAX_LEN,
            "description": "A commit message's first line: 1 to 72 characters, one line, no control character.",
            "examples": ["feat(walter): scaffold the iOS app, NSE, widgets and Buildkite files"]
        })
    }
}

crate::impl_domain_object_non_secret!(CommitHeadline);

/// The maximum length of a [`RepoFile`]'s content, in characters -- the
/// same bound as [`crate::Text`].
const REPO_FILE_CONTENT_MAX_LEN: usize = 65_536;

/// Validate a [`RepoFile`]'s content on its own: at most 65,536 characters,
/// no NUL byte. Shared by [`RepoFile::new`] and [`RepoFile::parse`].
fn validate_repo_file_content(content: &str) -> Result<(), String> {
    let len = content.chars().count();
    if len > REPO_FILE_CONTENT_MAX_LEN {
        return Err(format!(
            "content is {len} characters, the limit is {REPO_FILE_CONTENT_MAX_LEN}"
        ));
    }
    if content.contains('\0') {
        return Err("content must not contain a NUL byte".to_string());
    }
    Ok(())
}

/// A file to write into a repository: a [`RepoPath`] plus its content.
/// Content is at most 65,536 characters and contains no NUL byte.
///
/// Canonical string form: `<path>\n<content>` -- the path, exactly one
/// newline, then the content verbatim (which may itself hold further
/// newlines; [`Self::parse`] splits on the *first* one only, and
/// [`RepoPath`]'s own grammar admits no newline, so the split is
/// unambiguous). Content bytes are never normalized -- no trailing
/// newline is added or stripped -- because `G1`'s locally computed git
/// blob sha must match the content byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFile {
    path: RepoPath,
    content: String,
}

impl RepoFile {
    /// Build a file from an already-parsed path and content, validating
    /// only the content (the path is already a [`RepoPath`]).
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when `content` is over
    /// [`REPO_FILE_CONTENT_MAX_LEN`] characters or contains a NUL byte.
    pub fn new(path: RepoPath, content: impl Into<String>) -> Result<Self, ParseError> {
        let content = content.into();
        validate_repo_file_content(&content)
            .map_err(|reason| ParseError::new(<Self as DomainType>::TYPE_NAME, reason))?;
        Ok(Self { path, content })
    }

    /// The file's path.
    #[must_use]
    pub fn path(&self) -> &RepoPath {
        &self.path
    }

    /// The file's content, verbatim.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

impl fmt::Display for RepoFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n{}", self.path, self.content)
    }
}

impl FromStr for RepoFile {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl DomainType for RepoFile {
    const TYPE_NAME: &'static str = "RepoFile";

    fn description() -> &'static str {
        "A file to write into a repository: `<path>\\n<content>`."
    }

    fn example() -> &'static str {
        "apps/walter/BUILD.bazel\n# Reserve the app name.\n"
    }

    fn parse(input: &str) -> Result<Self, ParseError> {
        let Some((path_str, content)) = input.split_once('\n') else {
            return Err(ParseError::new(
                Self::TYPE_NAME,
                "must be `<path>\\n<content>`, with a newline separating them",
            ));
        };
        let path = RepoPath::parse(path_str)
            .map_err(|err| ParseError::new(Self::TYPE_NAME, format!("path: {}", err.reason)))?;
        validate_repo_file_content(content)
            .map_err(|reason| ParseError::new(Self::TYPE_NAME, reason))?;
        Ok(Self {
            path,
            content: content.to_owned(),
        })
    }

    fn json_schema() -> schemars::Schema {
        schemars::schema_for!(Self)
    }
}

impl serde::Serialize for RepoFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for RepoFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for RepoFile {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("RepoFile")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "A file to write into a repository: `<path>\\n<content>`.",
            "examples": ["apps/walter/BUILD.bazel\n# Reserve the app name.\n"]
        })
    }
}

crate::impl_domain_object_non_secret!(RepoFile);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_example_parses;

    // -------------------------------------------------------------
    // RepoPath
    // -------------------------------------------------------------

    #[test]
    fn repo_path_accepts_a_plain_path() {
        let path = RepoPath::parse("apps/walter/ios/BUILD.bazel").unwrap();
        assert_eq!(path.as_str(), "apps/walter/ios/BUILD.bazel");
        assert_eq!(
            path.segments().collect::<Vec<_>>(),
            vec!["apps", "walter", "ios", "BUILD.bazel"]
        );
    }

    #[test]
    fn repo_path_rejects_empty() {
        assert!(RepoPath::parse("").is_err());
    }

    #[test]
    fn repo_path_rejects_dot_segment() {
        assert!(RepoPath::parse("a/./b").is_err());
    }

    #[test]
    fn repo_path_rejects_dot_dot_segment() {
        assert!(RepoPath::parse("a/../b").is_err());
    }

    #[test]
    fn repo_path_rejects_empty_segment() {
        assert!(RepoPath::parse("a//b").is_err());
    }

    #[test]
    fn repo_path_rejects_leading_slash() {
        assert!(RepoPath::parse("/a/b").is_err());
    }

    #[test]
    fn repo_path_rejects_trailing_slash() {
        assert!(RepoPath::parse("a/b/").is_err());
    }

    #[test]
    fn repo_path_rejects_dot_git_segment_first() {
        assert!(RepoPath::parse(".git/x").is_err());
    }

    #[test]
    fn repo_path_rejects_dot_git_segment_any_case_anywhere() {
        assert!(RepoPath::parse("a/.GIT/b").is_err());
    }

    #[test]
    fn repo_path_rejects_github_workflows() {
        assert!(RepoPath::parse(".github/workflows/ci.yml").is_err());
    }

    #[test]
    fn repo_path_rejects_github_workflows_any_case() {
        assert!(RepoPath::parse(".GitHub/Workflows/x").is_err());
    }

    #[test]
    fn repo_path_allows_a_dotgithub_path_that_is_not_workflows() {
        // Only the exact `.github/workflows/...` prefix is refused; other
        // `.github/` content is not this crate's concern.
        assert!(RepoPath::parse(".github/ISSUE_TEMPLATE/bug.md").is_ok());
    }

    #[test]
    fn repo_path_rejects_backslash() {
        assert!(RepoPath::parse(r"a\b").is_err());
    }

    #[test]
    fn repo_path_rejects_control_character() {
        assert!(RepoPath::parse("a/b\tc").is_err());
    }

    #[test]
    fn repo_path_rejects_over_the_segment_limit() {
        let segments: Vec<String> = (0..33).map(|i| format!("s{i}")).collect();
        assert!(RepoPath::parse(&segments.join("/")).is_err());
    }

    #[test]
    fn repo_path_accepts_at_exactly_the_segment_limit() {
        let segments: Vec<String> = (0..32).map(|i| format!("s{i}")).collect();
        assert!(RepoPath::parse(&segments.join("/")).is_ok());
    }

    #[test]
    fn repo_path_rejects_over_the_length_limit() {
        let too_long = format!("{}/x", "a".repeat(REPO_PATH_MAX_LEN));
        assert!(RepoPath::parse(&too_long).is_err());
    }

    #[test]
    fn repo_path_example_parses() {
        assert_example_parses::<RepoPath>();
    }

    // -------------------------------------------------------------
    // GitBranchName
    // -------------------------------------------------------------

    #[test]
    fn git_branch_name_accepts_main() {
        assert_eq!(GitBranchName::parse("main").unwrap().as_str(), "main");
    }

    #[test]
    fn git_branch_name_rejects_double_dot() {
        assert!(GitBranchName::parse("a..b").is_err());
    }

    #[test]
    fn git_branch_name_rejects_leading_hyphen() {
        assert!(GitBranchName::parse("-x").is_err());
    }

    #[test]
    fn git_branch_name_rejects_trailing_dot_lock() {
        assert!(GitBranchName::parse("x.lock").is_err());
    }

    #[test]
    fn git_branch_name_rejects_double_slash() {
        assert!(GitBranchName::parse("a//b").is_err());
    }

    #[test]
    fn git_branch_name_rejects_at_brace() {
        assert!(GitBranchName::parse("@{").is_err());
    }

    #[test]
    fn git_branch_name_rejects_leading_dot_component() {
        assert!(GitBranchName::parse("a/.b").is_err());
    }

    #[test]
    fn git_branch_name_rejects_dot_lock_component_not_only_whole_name() {
        assert!(GitBranchName::parse("a.lock/b").is_err());
    }

    #[test]
    fn git_branch_name_rejects_leading_slash() {
        assert!(GitBranchName::parse("/main").is_err());
    }

    #[test]
    fn git_branch_name_rejects_trailing_slash() {
        assert!(GitBranchName::parse("main/").is_err());
    }

    #[test]
    fn git_branch_name_rejects_over_the_length_limit() {
        let too_long = "a".repeat(GIT_BRANCH_NAME_MAX_LEN + 1);
        assert!(GitBranchName::parse(&too_long).is_err());
    }

    #[test]
    fn git_branch_name_accepts_a_slashed_name() {
        assert!(GitBranchName::parse("release/1.0").is_ok());
    }

    #[test]
    fn git_branch_name_example_parses() {
        assert_example_parses::<GitBranchName>();
    }

    // -------------------------------------------------------------
    // CommitHeadline
    // -------------------------------------------------------------

    #[test]
    fn commit_headline_accepts_ordinary_text() {
        let headline = CommitHeadline::parse("feat(walter): scaffold the app").unwrap();
        assert_eq!(headline.as_str(), "feat(walter): scaffold the app");
    }

    #[test]
    fn commit_headline_rejects_empty() {
        assert!(CommitHeadline::parse("").is_err());
    }

    #[test]
    fn commit_headline_accepts_a_single_character() {
        assert!(CommitHeadline::parse("x").is_ok());
    }

    #[test]
    fn commit_headline_rejects_over_72_characters() {
        let too_long = "a".repeat(73);
        assert!(CommitHeadline::parse(&too_long).is_err());
    }

    #[test]
    fn commit_headline_accepts_exactly_72_characters() {
        let at_limit = "a".repeat(72);
        assert!(CommitHeadline::parse(&at_limit).is_ok());
    }

    #[test]
    fn commit_headline_rejects_a_newline() {
        assert!(CommitHeadline::parse("first line\nsecond line").is_err());
    }

    #[test]
    fn commit_headline_rejects_a_tab() {
        assert!(CommitHeadline::parse("a\tb").is_err());
    }

    #[test]
    fn commit_headline_example_parses() {
        assert_example_parses::<CommitHeadline>();
    }

    // -------------------------------------------------------------
    // RepoFile
    // -------------------------------------------------------------

    #[test]
    fn repo_file_round_trips() {
        let path = RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
        let file = RepoFile::new(path.clone(), "line one\nline two\n").unwrap();
        assert_eq!(file.path(), &path);
        assert_eq!(file.content(), "line one\nline two\n");
        let canonical = file.to_string();
        assert_eq!(canonical, "apps/walter/BUILD.bazel\nline one\nline two\n");
        let parsed = RepoFile::parse(&canonical).unwrap();
        assert_eq!(parsed, file);
    }

    #[test]
    fn repo_file_round_trips_with_empty_content() {
        let path = RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
        let file = RepoFile::new(path, "").unwrap();
        let canonical = file.to_string();
        assert_eq!(canonical, "apps/walter/BUILD.bazel\n");
        let parsed = RepoFile::parse(&canonical).unwrap();
        assert_eq!(parsed, file);
    }

    #[test]
    fn repo_file_parse_rejects_no_newline() {
        assert!(RepoFile::parse("apps/walter/BUILD.bazel").is_err());
    }

    #[test]
    fn repo_file_parse_rejects_a_bad_path() {
        assert!(RepoFile::parse(".git/x\ncontent").is_err());
    }

    #[test]
    fn repo_file_new_rejects_a_nul_byte() {
        let path = RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
        assert!(RepoFile::new(path, "a\0b").is_err());
    }

    #[test]
    fn repo_file_parse_rejects_a_nul_byte_in_content() {
        assert!(RepoFile::parse("apps/walter/BUILD.bazel\na\0b").is_err());
    }

    #[test]
    fn repo_file_rejects_over_long_content() {
        let path = RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
        let too_long = "a".repeat(REPO_FILE_CONTENT_MAX_LEN + 1);
        assert!(RepoFile::new(path, too_long).is_err());
    }

    #[test]
    fn repo_file_accepts_content_at_exactly_the_length_limit() {
        let path = RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
        let at_limit = "a".repeat(REPO_FILE_CONTENT_MAX_LEN);
        assert!(RepoFile::new(path, at_limit).is_ok());
    }

    #[test]
    fn repo_file_content_is_never_normalized() {
        // No trailing newline is added or stripped: `G1`'s locally
        // computed git blob sha must match the content byte for byte.
        let path = RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
        let file = RepoFile::new(path, "no trailing newline").unwrap();
        assert_eq!(file.content(), "no trailing newline");
    }

    #[test]
    fn repo_file_example_parses() {
        assert_example_parses::<RepoFile>();
    }

    #[test]
    fn repo_file_implements_domain_object_via_the_macro() {
        use crate::DomainObject;

        let path = RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
        let file = RepoFile::new(path, "content\n").unwrap();
        let object: Box<dyn DomainObject> = Box::new(file.clone());
        assert_eq!(object.type_name(), "RepoFile");
        assert!(!object.is_secret());
        assert_eq!(
            object.render(),
            crate::Rendered::Plain("apps/walter/BUILD.bazel\ncontent\n".to_string())
        );
    }
}
