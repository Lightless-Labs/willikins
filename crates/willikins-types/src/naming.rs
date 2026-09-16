//! The naming scheme: pure, total, frozen derivations from project
//! identity to provider-specific names.
//!
//! Derivation is a table of joins over a project's [`crate::WordList`],
//! one row per target, never a parse of a target name back into words.
//! Milestone 1's rows:
//!
//! | Target | Join | Example for `third-thoughts` |
//! | --- | --- | --- |
//! | GitHub repo | kebab | `third-thoughts` |
//! | Doppler project | kebab | `third-thoughts` |
//! | Doppler root config | environment, snake | `third-thoughts/prd` (for environment `prd`) |
//! | Buildkite pipeline slug | kebab | `third-thoughts` |
//!
//! Rows for other providers (Railway, Cargo, Swift, Android, bundle IDs,
//! and the rest of the design doc's join table) are added by the
//! milestone that adds their provider, without a version bump: adding a
//! row never changes an existing one.

use std::fmt;
use std::str::FromStr;

use crate::ParseError;

/// The version of the naming scheme's derivation rules.
///
/// **Freeze rule**: derived names are the natural keys that make tools
/// idempotent. If derivation ever changed for an existing project, `read`
/// would miss it and `ensure` would create a duplicate. So a published
/// version's derivations never change: never edit `v1`. A new rule, a new
/// target, or a changed join is always a new variant, `v2`, applied only
/// to new projects; existing projects keep the version recorded at
/// creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamingScheme {
    /// The scheme implemented by [`v1`]. Frozen: see the freeze rule
    /// above.
    V1,
}

impl NamingScheme {
    /// The canonical string for this scheme.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
        }
    }
}

impl fmt::Display for NamingScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NamingScheme {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "v1" => Ok(Self::V1),
            _ => Err(ParseError::new(
                "NamingScheme",
                format!("must be `v1`, found {}", crate::quoted(input)),
            )),
        }
    }
}

impl serde::Serialize for NamingScheme {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for NamingScheme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

/// Naming scheme version 1.
///
/// Every function here is pure, total, and frozen: it never touches the
/// network, it succeeds for every valid input, and its behaviour never
/// changes once published. Never edit these functions; add `v2` instead.
pub mod v1 {
    use crate::DomainType;
    use crate::buildkite::BuildkitePipelineSlug;
    use crate::doppler::{DopplerConfig, DopplerConfigName, DopplerProject};
    use crate::github::{GitHubOrg, GitHubRepo};
    use crate::slug::{EnvironmentSlug, ProjectSlug};

    /// The GitHub repository for `slug`, owned by `org`. Join: kebab.
    #[must_use]
    pub fn github_repo(org: &GitHubOrg, slug: &ProjectSlug) -> GitHubRepo {
        GitHubRepo::new(org.clone(), slug.clone())
    }

    /// The Doppler project for `slug`. Join: kebab.
    ///
    /// # Panics
    ///
    /// Never: [`ProjectSlug`]'s kebab-case grammar and 32-character limit
    /// are both within [`DopplerProject`]'s (lowercase, digits, hyphen;
    /// 64-character limit).
    #[must_use]
    pub fn doppler_project(slug: &ProjectSlug) -> DopplerProject {
        DopplerProject::parse(&slug.words().kebab())
            .expect("a ProjectSlug's kebab form always parses as a DopplerProject")
    }

    /// The root config for `environment` within `project`. The config
    /// name is `environment`'s snake join, because Doppler config names
    /// use underscores rather than hyphens.
    ///
    /// # Panics
    ///
    /// Never: [`EnvironmentSlug`]'s snake-case grammar and 16-character
    /// limit are both within [`DopplerConfigName`]'s (lowercase, digits,
    /// underscore; 64-character limit).
    #[must_use]
    pub fn doppler_root_config(
        project: &DopplerProject,
        environment: &EnvironmentSlug,
    ) -> DopplerConfig {
        let name = DopplerConfigName::parse(&environment.words().snake())
            .expect("an EnvironmentSlug's snake form always parses as a DopplerConfigName");
        DopplerConfig::new(project.clone(), name)
    }

    /// The Buildkite pipeline slug for `slug`. Join: kebab.
    ///
    /// The Buildkite organisation is not an argument, because the slug
    /// does not depend on it -- which is why `naming.v1`'s tool port for
    /// this row needs no new input.
    ///
    /// # Panics
    ///
    /// Never: [`ProjectSlug`]'s kebab form (`[a-z][a-z0-9]*(-[a-z0-9]+)*`,
    /// at most 32 characters) is inside [`BuildkitePipelineSlug`]'s
    /// grammar (`[a-z0-9][a-z0-9-]*`) and its 100-character cap.
    #[must_use]
    pub fn buildkite_pipeline_slug(slug: &ProjectSlug) -> BuildkitePipelineSlug {
        BuildkitePipelineSlug::parse(&slug.words().kebab())
            .expect("a ProjectSlug's kebab form always parses as a BuildkitePipelineSlug")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DomainType, DopplerProject, EnvironmentSlug, GitHubOrg, ProjectSlug};

    #[test]
    fn golden_github_repo() {
        let org = GitHubOrg::parse("lightless-labs").unwrap();
        let slug = ProjectSlug::parse("third-thoughts").unwrap();
        assert_eq!(
            v1::github_repo(&org, &slug).to_string(),
            "lightless-labs/third-thoughts"
        );
    }

    #[test]
    fn golden_doppler_project() {
        let slug = ProjectSlug::parse("third-thoughts").unwrap();
        assert_eq!(v1::doppler_project(&slug).to_string(), "third-thoughts");
    }

    #[test]
    fn golden_doppler_root_config() {
        let project = DopplerProject::parse("third-thoughts").unwrap();
        let environment = EnvironmentSlug::parse("prd").unwrap();
        assert_eq!(
            v1::doppler_root_config(&project, &environment).to_string(),
            "third-thoughts/prd"
        );
    }

    #[test]
    fn golden_doppler_root_config_uses_snake_join_for_multi_word_environment() {
        let project = DopplerProject::parse("x").unwrap();
        let environment = EnvironmentSlug::parse("pre-prod").unwrap();
        assert_eq!(
            v1::doppler_root_config(&project, &environment).to_string(),
            "x/pre_prod"
        );
    }

    #[test]
    fn golden_buildkite_pipeline_slug() {
        let slug = ProjectSlug::parse("third-thoughts").unwrap();
        assert_eq!(
            v1::buildkite_pipeline_slug(&slug).to_string(),
            "third-thoughts"
        );
    }

    #[test]
    fn naming_scheme_display_is_v1() {
        assert_eq!(NamingScheme::V1.to_string(), "v1");
    }

    #[test]
    fn naming_scheme_from_str_round_trips() {
        assert_eq!("v1".parse::<NamingScheme>().unwrap(), NamingScheme::V1);
    }

    #[test]
    fn naming_scheme_from_str_rejects_unknown() {
        let err = "v2".parse::<NamingScheme>().unwrap_err();
        assert!(err.reason.contains("v1"));
    }

    #[test]
    fn naming_scheme_serde_round_trips() {
        let json = serde_json::to_string(&NamingScheme::V1).unwrap();
        assert_eq!(json, "\"v1\"");
        assert_eq!(
            serde_json::from_str::<NamingScheme>(&json).unwrap(),
            NamingScheme::V1
        );
    }

    #[test]
    fn naming_scheme_deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<NamingScheme>("\"v2\"").is_err());
    }
}
