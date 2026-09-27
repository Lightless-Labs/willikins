//! Acceptance test 1's share for this crate: the live `github.repo.ensure`
//! and `github.actions_secret.ensure` `ToolSpec`s equal
//! `willikins_providers_fake`'s tools of the same names, field for field.
//! `ToolSpec` derives `Serialize` but not `PartialEq` (its `class` and
//! `pure` fields aside, comparing it structurally means comparing its
//! `Serialize` form), so equality here is JSON equality; an insta snapshot
//! of both specs pins the exact shape besides. Milestone 3e task 2 adds
//! `github.repo.get` alongside them, same method.

use std::sync::{Arc, Mutex};

use willikins_core::{Tool, ToolSpec};
use willikins_providers_github::{
    GitHubActionsSecretEnsure, GitHubClient, GitHubRepoEnsure, GitHubRepoGet,
};
use willikins_providers_http::{Credential, Http};

fn test_client() -> Arc<GitHubClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new("http://127.0.0.1:0", Vec::new(), credential);
    Arc::new(GitHubClient::new(http))
}

fn spec_json(spec: &ToolSpec) -> serde_json::Value {
    serde_json::to_value(spec).expect("ToolSpec serializes")
}

#[test]
fn github_repo_ensure_spec_equals_the_fake_tools() {
    let live = GitHubRepoEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::GitHubRepoEnsure::new(Arc::new(Mutex::new(
        willikins_providers_fake::FakeState::new(),
    )));
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn github_actions_secret_ensure_spec_equals_the_fake_tools() {
    let live = GitHubActionsSecretEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::GitHubActionsSecretEnsure::new(Arc::new(
        Mutex::new(willikins_providers_fake::FakeState::new()),
    ));
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn github_repo_get_spec_equals_the_fake_tool() {
    let live = GitHubRepoGet::new(test_client());
    let fake = willikins_providers_fake::tools::FakeGitHubRepoGet::new(Arc::new(Mutex::new(
        willikins_providers_fake::FakeState::new(),
    )));
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn both_specs_validate_against_the_type_registry() {
    GitHubRepoEnsure::new(test_client())
        .spec()
        .validate(willikins_types::registry())
        .expect("valid spec");
    GitHubActionsSecretEnsure::new(test_client())
        .spec()
        .validate(willikins_types::registry())
        .expect("valid spec");
    GitHubRepoGet::new(test_client())
        .spec()
        .validate(willikins_types::registry())
        .expect("valid spec");
}

#[test]
fn snapshot_both_live_tool_specs() {
    let repo = spec_json(GitHubRepoEnsure::new(test_client()).spec());
    let secret = spec_json(GitHubActionsSecretEnsure::new(test_client()).spec());
    let repo_get = spec_json(GitHubRepoGet::new(test_client()).spec());
    insta::assert_json_snapshot!("github_repo_ensure_spec", repo);
    insta::assert_json_snapshot!("github_actions_secret_ensure_spec", secret);
    insta::assert_json_snapshot!("github_repo_get_spec", repo_get);
}
