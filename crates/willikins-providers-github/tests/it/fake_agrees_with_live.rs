//! `github.repo.get`'s fake tool agrees with the live one *behaviourally*,
//! not only on their `ToolSpec`s.
//!
//! `catalog_parity.rs` pins the two specs equal — agreement by
//! construction, since both are hand-written. This file is milestone 3e's
//! own instance of the check `willikins-providers-buildkite/tests/fake_agrees_with_live.rs`
//! introduced for `buildkite.cluster.get`: for each observation
//! `github.repo.get` can report, seed the fake's state and serve the live
//! tool a mock body standing for *the same real world*, then assert both
//! tools answer the same way.

use std::sync::{Arc, Mutex};

use willikins_core::rollback::{CreationWindow, Standing};
use willikins_core::{Created, Inputs, Observation, PortName, Timestamp, Tool, Value};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::tools::FakeGitHubRepoGet;
use willikins_providers_github::{GitHubClient, GitHubRepoEnsure, GitHubRepoGet};
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, GitHubRepo, RepoVisibility};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "github", name)
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("example-org/monorepo").unwrap()
}

fn inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
    inputs
}

fn live_against(url: String) -> GitHubRepoGet {
    let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let http = Http::new(url, Vec::new(), credential);
    GitHubRepoGet::new(Arc::new(GitHubClient::new(http)))
}

fn shape(observation: &Observation) -> &'static str {
    match observation {
        Observation::Present(_) => "present",
        Observation::Absent { .. } => "absent",
        Observation::Foreign => "foreign",
        Observation::Mismatch { .. } => "mismatch",
    }
}

/// A present, unarchived repository — whether or not it carries the
/// `managed-by-willikins` topic (decision (g): this tool ignores
/// ownership) — agrees between the fake and the live tool.
#[test]
fn agrees_on_present_regardless_of_ownership() {
    for (case, live_fixture, ours) in [
        ("owned", "repo_get_present", true),
        ("foreign", "repo_get_foreign", false),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/repos/example-org/monorepo")
            .with_status(200)
            .with_body(fixture(live_fixture).to_string())
            .create();
        let live = live_against(provider.url())
            .read(&inputs())
            .unwrap_or_else(|err| panic!("{case}: live tool errored: {err:?}"));

        let state = FakeState::new().with_repo(&repo(), RepoVisibility::Private, ours);
        let fake = FakeGitHubRepoGet::new(Arc::new(Mutex::new(state)))
            .read(&inputs())
            .unwrap_or_else(|err| panic!("{case}: fake tool errored: {err:?}"));

        assert_eq!(shape(&live), shape(&fake), "{case}");
        let Observation::Present(live_outputs) = live else {
            panic!("{case}: expected Present");
        };
        let Observation::Present(fake_outputs) = fake else {
            panic!("{case}: expected Present");
        };
        assert_eq!(
            live_outputs
                .get(&PortName::parse("repo").unwrap())
                .unwrap()
                .render()
                .to_string(),
            fake_outputs
                .get(&PortName::parse("repo").unwrap())
                .unwrap()
                .render()
                .to_string(),
            "{case}"
        );
    }
}

/// Not found (404) agrees by `ToolErrorKind`; the message itself is
/// sourced from GitHub's own error body on the live side and is not
/// expected to match the fake's self-generated one.
#[test]
fn agrees_on_not_found() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/example-org/monorepo")
        .with_status(404)
        .with_body(fixture("error_basic_404").to_string())
        .create();
    let live = live_against(provider.url())
        .read(&inputs())
        .expect_err("the live tool refuses");

    let fake = FakeGitHubRepoGet::new(Arc::new(Mutex::new(FakeState::new())))
        .read(&inputs())
        .expect_err("the fake tool refuses");

    assert_eq!(live.kind, fake.kind);
}

/// Archived agrees on both `ToolErrorKind` and the exact message: both
/// sides compute this refusal themselves rather than sourcing it from a
/// provider body, so the two copies of the format string are pinned to
/// stay byte-identical.
#[test]
fn agrees_on_archived() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/example-org/monorepo")
        .with_status(200)
        .with_body(fixture("repo_get_archived").to_string())
        .create();
    let live = live_against(provider.url())
        .read(&inputs())
        .expect_err("the live tool refuses");

    let state = FakeState::new().with_archived_repo(&repo());
    let fake = FakeGitHubRepoGet::new(Arc::new(Mutex::new(state)))
        .read(&inputs())
        .expect_err("the fake tool refuses");

    assert_eq!(live.kind, fake.kind);
    assert_eq!(live.message, fake.message);
}

/// Milestone 3e, task R2: a document that binds `token` still agrees
/// between the fake and the live tool. The fake tool ignores the port's
/// value entirely (it has no real credential to check), so this proves
/// the *shape* of the two sides' answers still matches with the port
/// bound -- the live-side authorization proof itself lives in
/// `repo_get_mock.rs`'s `read_authorizes_with_the_bound_token_port_not_the_default_credential`.
#[test]
fn agrees_on_present_with_the_token_port_bound() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/repos/example-org/monorepo")
        .with_status(200)
        .with_body(fixture("repo_get_present").to_string())
        .create();

    let mut bound_inputs = inputs();
    bound_inputs.insert(
        PortName::parse("token").unwrap(),
        Value::known(willikins_types::GitHubToken::parse("ghp_theboundtoken").unwrap()),
    );

    let live = live_against(provider.url())
        .read(&bound_inputs)
        .unwrap_or_else(|err| panic!("live tool errored: {err:?}"));

    let state = FakeState::new().with_repo(&repo(), RepoVisibility::Private, true);
    let fake = FakeGitHubRepoGet::new(Arc::new(Mutex::new(state)))
        .read(&bound_inputs)
        .unwrap_or_else(|err| panic!("fake tool errored: {err:?}"));

    assert_eq!(shape(&live), shape(&fake));
    assert!(matches!(live, Observation::Present(_)));
}

/// Milestone 3m task P6: `github.repo.ensure`'s inverse, the live
/// `GitHubRepoInverse` against mocks and the fake `GitHubRepoInverse`
/// against `FakeState`, agree on [`Standing`]'s *variant* for the same
/// scenario -- never on the exact `why`/`what` text, which each crate
/// writes for its own reader. The live inverse's own window check has no
/// fake counterpart (this crate's own `tests/it/repo_inverse_mock.rs`
/// pins that half); every case here holds inside both inverses' own
/// window regardless.
mod inverse_standing_parity {
    use super::*;

    fn created_inputs() -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        inputs
    }

    fn window() -> CreationWindow {
        CreationWindow {
            start: Timestamp::parse("2026-01-01T00:00:00Z").unwrap(),
            end: Timestamp::parse("2026-01-01T00:10:00Z").unwrap(),
        }
    }

    fn created(inputs: &Inputs) -> Created<'_> {
        Created {
            inputs,
            outputs: Box::leak(Box::new(willikins_core::Outputs::new())),
            window: window(),
            settled_by: window().end,
            accept: &[],
            riders: &[],
        }
    }

    fn live_standing(provider: &mut MockProvider) -> Standing {
        let credential = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
        let http = Http::new(provider.url(), Vec::new(), credential);
        let tool = GitHubRepoEnsure::new(Arc::new(GitHubClient::new(http)));
        Tool::inverse(&tool)
            .expect("github.repo.ensure declares Delete")
            .standing(&created(&created_inputs()))
            .expect("standing never fails against a well-formed mock response")
    }

    fn fake_standing(state: FakeState) -> Standing {
        let tool =
            willikins_providers_fake::tools::GitHubRepoEnsure::new(Arc::new(Mutex::new(state)));
        Tool::inverse(&tool)
            .expect("github.repo.ensure declares Delete")
            .standing(&created(&created_inputs()))
            .expect("standing never fails against a well-formed fake state")
    }

    #[test]
    fn both_report_gone_when_absent() {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/repos/example-org/monorepo")
            .with_status(404)
            .with_body(fixture("error_basic_404").to_string())
            .create();
        assert_eq!(live_standing(&mut provider), Standing::Gone);
        assert_eq!(fake_standing(FakeState::new()), Standing::Gone);
    }

    #[test]
    fn both_report_as_created_when_present_and_ours_with_no_foreign_contents() {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/repos/example-org/monorepo")
            .with_status(200)
            .with_body(
                serde_json::json!({
                    "visibility": "private",
                    "topics": ["managed-by-willikins"],
                    "full_name": "example-org/monorepo",
                    "created_at": "2026-01-01T00:05:00Z",
                    "pushed_at": "2026-01-01T00:05:00Z",
                    "open_issues_count": 0,
                    "forks_count": 0,
                })
                .to_string(),
            )
            .create();
        provider
            .mock(
                "GET",
                "/repos/example-org/monorepo/actions/secrets?per_page=100&page=1",
            )
            .with_status(200)
            .with_body(serde_json::json!({"total_count": 0, "secrets": []}).to_string())
            .create();
        assert_eq!(
            live_standing(&mut provider),
            Standing::AsCreated { losing: Vec::new() }
        );
        let state = FakeState::new().with_repo(&repo(), RepoVisibility::Private, true);
        assert_eq!(
            fake_standing(state),
            Standing::AsCreated { losing: Vec::new() }
        );
    }

    #[test]
    fn both_report_not_ours_when_foreign() {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/repos/example-org/monorepo")
            .with_status(200)
            .with_body(fixture("repo_get_foreign").to_string())
            .create();
        assert!(matches!(
            live_standing(&mut provider),
            Standing::NotOurs { .. }
        ));
        let state = FakeState::new().with_repo(&repo(), RepoVisibility::Private, false);
        assert!(matches!(fake_standing(state), Standing::NotOurs { .. }));
    }
}
