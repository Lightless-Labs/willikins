//! Whole-catalog tests: every domain type this crate defines is registered
//! in `type_infos()` exactly once, its own example parses as itself, and
//! the catalog's published JSON shape is pinned by an insta snapshot so
//! drift is visible in review.

use std::collections::HashSet;

use willikins_types::*;

/// List every domain type exactly once, run `assert_example_parses` on
/// each, and return its `TYPE_NAME`.
///
/// A type added to `type_infos()` but left out of this list fails the
/// membership check below; a type added to this list but never appended
/// to `type_infos()` fails it from the other side.
macro_rules! catalog_types {
    ($($ty:ty),+ $(,)?) => {{
        $(willikins_types::assert_example_parses::<$ty>();)+
        vec![$(<$ty as DomainType>::TYPE_NAME),+]
    }};
}

#[test]
fn every_domain_type_is_registered_exactly_once_and_its_example_parses() {
    let expected_names = catalog_types![
        WordList,
        ProjectSlug,
        ComponentSlug,
        EnvironmentSlug,
        ProjectName,
        Text,
        TemplateSource,
        GitHubOrg,
        RepoVisibility,
        GitHubRepo,
        HttpsUrl,
        ActionsSecretName,
        DopplerProject,
        DopplerConfigName,
        DopplerConfig,
        DopplerTokenName,
        SecretName,
        DopplerServiceToken,
        DopplerSecretValue,
    ];

    let infos = type_infos();
    let catalog_names: Vec<&'static str> = infos.iter().map(|info| info.name).collect();

    let catalog_set: HashSet<&str> = catalog_names.iter().copied().collect();
    assert_eq!(
        catalog_set.len(),
        catalog_names.len(),
        "type_infos() lists the same type name more than once"
    );

    let expected_set: HashSet<&str> = expected_names.iter().copied().collect();
    assert_eq!(
        expected_set.len(),
        expected_names.len(),
        "this test's own catalog list has a duplicate; fix the test"
    );

    assert_eq!(
        catalog_set, expected_set,
        "type_infos() and this test's catalog list disagree on which types exist"
    );
}

#[test]
fn catalog_json_snapshot() {
    let json = serde_json::to_string_pretty(&type_infos()).unwrap();
    insta::assert_snapshot!(json);
}
