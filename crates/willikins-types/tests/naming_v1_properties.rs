//! Property tests for `naming::v1`: every function is total over valid
//! inputs, and every result parses back as its own type with `Display`
//! round-tripping through `parse`.

use proptest::prelude::*;
use willikins_types::naming::v1;
use willikins_types::{DomainType, EnvironmentSlug, GitHubOrg, GitHubRepo, ProjectSlug};

/// A single lowercase word: a letter followed by up to five letters or
/// digits, or a run of one to three digits.
fn word_str() -> impl Strategy<Value = String> {
    prop_oneof!["[a-z][a-z0-9]{0,5}", "[0-9]{1,3}"]
}

/// A kebab-case string of one to three words, the first always
/// letter-led, so it is a candidate for any of the word-list-backed slug
/// types below.
fn kebab_words(max_extra: usize) -> impl Strategy<Value = String> {
    (
        "[a-z][a-z0-9]{0,5}",
        prop::collection::vec(word_str(), 0..max_extra),
    )
        .prop_map(|(first, rest)| {
            let mut words = vec![first];
            words.extend(rest);
            words.join("-")
        })
}

fn valid_project_slug() -> impl Strategy<Value = ProjectSlug> {
    kebab_words(3).prop_filter_map("must parse as ProjectSlug and not be reserved", |s| {
        ProjectSlug::parse(&s).ok()
    })
}

fn valid_environment_slug() -> impl Strategy<Value = EnvironmentSlug> {
    kebab_words(2).prop_filter_map("must parse as EnvironmentSlug and not be reserved", |s| {
        EnvironmentSlug::parse(&s).ok()
    })
}

fn valid_github_org() -> impl Strategy<Value = GitHubOrg> {
    "[A-Za-z0-9]{1,5}(-[A-Za-z0-9]{1,5}){0,2}"
        .prop_filter_map("must parse as GitHubOrg", |s| GitHubOrg::parse(&s).ok())
}

proptest! {
    #[test]
    fn github_repo_is_total_and_round_trips(org in valid_github_org(), slug in valid_project_slug()) {
        let repo = v1::github_repo(&org, &slug);
        let text = repo.to_string();
        prop_assert_eq!(GitHubRepo::parse(&text).unwrap(), repo);
    }

    #[test]
    fn github_repo_name_equals_the_slug(org in valid_github_org(), slug in valid_project_slug()) {
        let repo = v1::github_repo(&org, &slug);
        prop_assert_eq!(repo.name(), &slug);
    }

    #[test]
    fn doppler_project_is_total_and_round_trips(slug in valid_project_slug()) {
        use willikins_types::DopplerProject;

        let project = v1::doppler_project(&slug);
        let text = project.to_string();
        prop_assert_eq!(DopplerProject::parse(&text).unwrap(), project);
    }

    #[test]
    fn doppler_root_config_is_total_and_round_trips(
        slug in valid_project_slug(),
        environment in valid_environment_slug(),
    ) {
        use willikins_types::{DopplerConfig, DopplerProject};

        let project = v1::doppler_project(&slug);
        let config = v1::doppler_root_config(&project, &environment);
        let text = config.to_string();
        prop_assert_eq!(config.project(), &project);
        prop_assert_eq!(DopplerConfig::parse(&text).unwrap(), config);
        let _: DopplerProject = project;
    }

    #[test]
    fn buildkite_pipeline_slug_is_total_and_round_trips(slug in valid_project_slug()) {
        use willikins_types::BuildkitePipelineSlug;

        let pipeline_slug = v1::buildkite_pipeline_slug(&slug);
        let text = pipeline_slug.to_string();
        prop_assert_eq!(BuildkitePipelineSlug::parse(&text).unwrap(), pipeline_slug);
    }
}
