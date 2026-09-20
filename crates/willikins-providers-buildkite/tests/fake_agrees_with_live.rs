//! The fake Buildkite tools agree with the live ones *behaviourally*, not
//! only on their `ToolSpec`s.
//!
//! `tests/catalog_parity.rs` pins the two catalogs' specs equal. That is
//! agreement by construction: both specs are hand-written, and a spec says
//! nothing about what a tool does with the ports it declares. Milestone 2's
//! first live smoke run was lost to exactly that gap, and the milestone 3a
//! plan names it again ("Does the fake genuinely agree with the live tool,
//! or do they agree by construction?").
//!
//! The concrete hole this file closes, found by mutation during milestone
//! 3a's adversarial pass: `willikins-providers-fake`'s copy of
//! [`willikins_providers_buildkite::ssh_repository_url`] is a deliberate
//! duplicate (a fake tool never depends on its live counterpart's crate),
//! and nothing pinned the two copies to the same bytes. Rewriting the
//! fake's copy to emit `https://github.com/{owner}/{name}.git` instead of
//! `git@github.com:{owner}/{name}.git` left every test in both crates
//! green, while a document planned against the fake would have predicted a
//! pipeline the live tool then reports as `Mismatch { repo }`.
//!
//! Method: for each of the five observations
//! `buildkite.pipeline.ensure` can report, seed the fake's state and serve
//! the live tool a mock body that stand for *the same real world*, then
//! assert both tools answer the same way. The two sides are built from the
//! live crate's own constants ([`MANAGED_DESCRIPTION`],
//! [`ssh_repository_url`]) on the live side and from the fake's public
//! seeding API on the fake side, so a divergence in either crate's copy of
//! a frozen form fails here.

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, Observation, PortName, Tool, Value};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkiteClusterGet, BuildkitePipelineEnsure, MANAGED_DESCRIPTION,
    ssh_repository_url,
};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::state::BuildkitePipelineRecord;
use willikins_providers_fake::tools::{FakeBuildkiteClusterGet, FakeBuildkitePipelineEnsure};
use willikins_providers_http::testing::MockProvider;
use willikins_providers_http::{Credential, Http};
use willikins_types::{
    BuildkiteClusterId, BuildkiteClusterName, BuildkiteOrg, BuildkitePipelineSlug, DomainType,
    GitHubOrg, GitHubRepo, ProjectSlug,
};

const CLUSTER_ID: &str = "018e5a22-d14c-7085-bb28-db0f83f43a1c";
const OTHER_CLUSTER_ID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

fn org() -> BuildkiteOrg {
    BuildkiteOrg::parse("willikins-test").expect("a valid Buildkite org")
}

fn slug() -> BuildkitePipelineSlug {
    BuildkitePipelineSlug::parse("third-thoughts").expect("a valid pipeline slug")
}

fn repo() -> GitHubRepo {
    GitHubRepo::new(
        GitHubOrg::parse("lightless-labs").expect("a valid GitHub org"),
        ProjectSlug::parse("third-thoughts").expect("a valid project slug"),
    )
}

fn other_repo() -> GitHubRepo {
    GitHubRepo::new(
        GitHubOrg::parse("lightless-labs").expect("a valid GitHub org"),
        ProjectSlug::parse("other-service").expect("a valid project slug"),
    )
}

fn cluster() -> BuildkiteClusterId {
    BuildkiteClusterId::parse(CLUSTER_ID).expect("a valid cluster id")
}

fn cluster_name() -> BuildkiteClusterName {
    BuildkiteClusterName::parse("Default cluster").expect("a valid cluster name")
}

fn port(name: &str) -> PortName {
    PortName::parse(name).expect("a valid port name")
}

fn pipeline_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("org"), Value::known(org()));
    inputs.insert(port("slug"), Value::known(slug()));
    inputs.insert(port("repo"), Value::known(repo()));
    inputs.insert(port("cluster"), Value::known(cluster()));
    inputs
}

fn cluster_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(port("org"), Value::known(org()));
    inputs.insert(port("name"), Value::known(cluster_name()));
    inputs
}

/// A pipeline body in the shape Buildkite returns, built from whatever
/// `description`, `repository`, and `cluster_id` the case under test wants
/// the provider to be holding. Carries `provider`, `steps`, and
/// `configuration` besides, exactly as a real response does, so the
/// response struct's field list is exercised here too.
fn pipeline_body(description: Option<&str>, repository: &str, cluster_id: &str) -> String {
    serde_json::json!({
        "id": "018e5a22-0000-0000-0000-000000000001",
        "slug": "third-thoughts",
        "web_url": "https://buildkite.com/willikins-test/third-thoughts",
        "repository": repository,
        "cluster_id": cluster_id,
        "description": description,
        "provider": {"webhook_url": "https://webhook.buildkite.com/deliver/NEVER-READ"},
        "steps": [{"type": "script", "command": "echo never read"}],
        "configuration": "steps:\n - command: \"echo never read\"",
    })
    .to_string()
}

/// The name of an observation, so two `Observation`s can be compared
/// without `Observation` needing `PartialEq` (it does not have one, and a
/// `Mismatch`'s port is the part that matters).
fn shape(observation: &Observation) -> String {
    match observation {
        Observation::Absent { .. } => "Absent".to_string(),
        Observation::Present(_) => "Present".to_string(),
        Observation::Foreign => "Foreign".to_string(),
        Observation::Mismatch { port } => format!("Mismatch({port})"),
    }
}

/// The outputs an observation carries, rendered, so predicted and
/// confirmed values can be compared between the two implementations.
fn rendered(observation: &Observation) -> Vec<(String, String)> {
    let outputs = match observation {
        Observation::Absent { predicted } => Some(predicted),
        Observation::Present(outputs) => Some(outputs),
        Observation::Foreign | Observation::Mismatch { .. } => None,
    };
    outputs
        .map(|outputs| {
            outputs
                .iter()
                .map(|(name, value)| (name.to_string(), value.render().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn live_pipeline_tool(url: String) -> BuildkitePipelineEnsure {
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    let http = Http::new(url, Vec::new(), credential);
    BuildkitePipelineEnsure::new(Arc::new(BuildkiteClient::new(http)))
}

/// One case: `state` is how the fake is seeded, `served` is what the live
/// tool's provider answers with (a `404` when `None`). Both are meant to
/// describe the same world; the assertion is that both tools say the same
/// thing about it.
fn assert_agree(case: &str, state: FakeState, served: Option<String>) {
    let mut provider = MockProvider::start();
    let mock = provider.mock(
        "GET",
        "/v2/organizations/willikins-test/pipelines/third-thoughts",
    );
    let _mock = match served {
        Some(body) => mock.with_status(200).with_body(body).create(),
        None => mock.with_status(404).create(),
    };

    let live = live_pipeline_tool(provider.url())
        .read(&pipeline_inputs())
        .unwrap_or_else(|err| panic!("{case}: the live tool failed: {err}"));
    let fake = FakeBuildkitePipelineEnsure::new(Arc::new(Mutex::new(state)))
        .read(&pipeline_inputs())
        .unwrap_or_else(|err| panic!("{case}: the fake tool failed: {err}"));

    assert_eq!(
        shape(&live),
        shape(&fake),
        "{case}: the live tool says {live:?}, the fake says {fake:?}"
    );
    assert_eq!(
        rendered(&live),
        rendered(&fake),
        "{case}: the two tools' outputs differ"
    );
}

#[test]
fn absent_agrees() {
    assert_agree("absent", FakeState::new(), None);
}

/// The case mutation found: the fake's own copy of the frozen SSH
/// repository form is seeded here from the **live** crate's
/// `ssh_repository_url`, so the two copies diverging makes this read
/// `Mismatch { repo }` on the fake side while the live side reads
/// `Present`.
#[test]
fn present_agrees_and_pins_the_frozen_repository_form() {
    let state = FakeState::new().with_buildkite_pipeline(
        &org(),
        &slug(),
        BuildkitePipelineRecord {
            repository: ssh_repository_url(&repo()),
            cluster_id: CLUSTER_ID.to_string(),
            ours: true,
        },
    );
    assert_agree(
        "present",
        state,
        Some(pipeline_body(
            Some(MANAGED_DESCRIPTION),
            &ssh_repository_url(&repo()),
            CLUSTER_ID,
        )),
    );
}

#[test]
fn foreign_agrees_and_pins_the_frozen_ownership_marker() {
    let state = FakeState::new().with_buildkite_pipeline(
        &org(),
        &slug(),
        BuildkitePipelineRecord {
            repository: ssh_repository_url(&repo()),
            cluster_id: CLUSTER_ID.to_string(),
            ours: false,
        },
    );
    assert_agree(
        "foreign",
        state,
        Some(pipeline_body(
            Some("someone else's pipeline"),
            &ssh_repository_url(&repo()),
            CLUSTER_ID,
        )),
    );
}

#[test]
fn mismatch_on_the_repository_agrees() {
    let state = FakeState::new().with_buildkite_pipeline(
        &org(),
        &slug(),
        BuildkitePipelineRecord {
            repository: ssh_repository_url(&other_repo()),
            cluster_id: CLUSTER_ID.to_string(),
            ours: true,
        },
    );
    assert_agree(
        "mismatch-repo",
        state,
        Some(pipeline_body(
            Some(MANAGED_DESCRIPTION),
            &ssh_repository_url(&other_repo()),
            CLUSTER_ID,
        )),
    );
}

/// Both sides must also agree on the *order* the two mismatch arms are
/// checked in: a pipeline whose repository and cluster are both wrong is
/// `Mismatch { repo }`, never `Mismatch { cluster }` (plan decision (d)).
#[test]
fn mismatch_on_the_cluster_agrees_and_repo_is_checked_first() {
    let matching_repo = FakeState::new().with_buildkite_pipeline(
        &org(),
        &slug(),
        BuildkitePipelineRecord {
            repository: ssh_repository_url(&repo()),
            cluster_id: OTHER_CLUSTER_ID.to_string(),
            ours: true,
        },
    );
    assert_agree(
        "mismatch-cluster",
        matching_repo,
        Some(pipeline_body(
            Some(MANAGED_DESCRIPTION),
            &ssh_repository_url(&repo()),
            OTHER_CLUSTER_ID,
        )),
    );

    let both_wrong = FakeState::new().with_buildkite_pipeline(
        &org(),
        &slug(),
        BuildkitePipelineRecord {
            repository: ssh_repository_url(&other_repo()),
            cluster_id: OTHER_CLUSTER_ID.to_string(),
            ours: true,
        },
    );
    assert_agree(
        "mismatch-both-reports-repo",
        both_wrong,
        Some(pipeline_body(
            Some(MANAGED_DESCRIPTION),
            &ssh_repository_url(&other_repo()),
            OTHER_CLUSTER_ID,
        )),
    );
}

/// `buildkite.cluster.get`'s own agreement: one seeded cluster on the fake
/// side, one cluster in the served page on the live side, same id out.
#[test]
fn cluster_get_agrees_on_a_single_match() {
    let mut provider = MockProvider::start();
    provider
        .mock("GET", "/v2/organizations/willikins-test/clusters")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(serde_json::json!([{"id": CLUSTER_ID, "name": "Default cluster"}]).to_string())
        .create();
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    let http = Http::new(provider.url(), Vec::new(), credential);
    let live = BuildkiteClusterGet::new(Arc::new(BuildkiteClient::new(http)))
        .read(&cluster_inputs())
        .expect("the live tool resolves the cluster");

    let state = FakeState::new().with_buildkite_cluster(&cluster_name(), CLUSTER_ID);
    let fake = FakeBuildkiteClusterGet::new(Arc::new(Mutex::new(state)))
        .read(&cluster_inputs())
        .expect("the fake tool resolves the cluster");

    assert_eq!(shape(&live), shape(&fake));
    assert_eq!(rendered(&live), rendered(&fake));
}

/// And on the two failure arms, by `ToolErrorKind` — the part a workflow
/// planned against the fake would meet.
#[test]
fn cluster_get_agrees_on_not_found_and_on_an_ambiguous_name() {
    for (case, served, state) in [
        (
            "not-found",
            serde_json::json!([{"id": OTHER_CLUSTER_ID, "name": "Some other cluster"}]),
            FakeState::new(),
        ),
        (
            "ambiguous",
            serde_json::json!([
                {"id": CLUSTER_ID, "name": "Default cluster"},
                {"id": OTHER_CLUSTER_ID, "name": "Default cluster"},
            ]),
            FakeState::new()
                .with_buildkite_cluster(&cluster_name(), CLUSTER_ID)
                .with_buildkite_cluster(&cluster_name(), OTHER_CLUSTER_ID),
        ),
    ] {
        let mut provider = MockProvider::start();
        provider
            .mock("GET", "/v2/organizations/willikins-test/clusters")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(served.to_string())
            .create();
        let credential =
            Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
        let http = Http::new(provider.url(), Vec::new(), credential);
        let live = BuildkiteClusterGet::new(Arc::new(BuildkiteClient::new(http)))
            .read(&cluster_inputs())
            .expect_err("the live tool refuses");
        let fake = FakeBuildkiteClusterGet::new(Arc::new(Mutex::new(state)))
            .read(&cluster_inputs())
            .expect_err("the fake tool refuses");
        assert_eq!(live.kind, fake.kind, "{case}: error kinds differ");
        assert_eq!(live.message, fake.message, "{case}: error messages differ");
    }
}
