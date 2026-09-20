//! Acceptance test 1's share for this crate: the live
//! `buildkite.pipeline.ensure` and `buildkite.cluster.get` `ToolSpec`s
//! equal `willikins_providers_fake`'s tools of the same names, field for
//! field. `ToolSpec` derives `Serialize` but not `PartialEq`, so equality
//! here is JSON equality; an insta snapshot of both specs pins the exact
//! shape besides.

use std::sync::{Arc, Mutex};

use willikins_core::{Tool, ToolSpec};
use willikins_providers_buildkite::{
    BuildkiteClient, BuildkiteClusterGet, BuildkitePipelineEnsure,
};
use willikins_providers_http::{Credential, Http};

fn test_client() -> Arc<BuildkiteClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken");
    let http = Http::new("http://127.0.0.1:0", Vec::new(), credential);
    Arc::new(BuildkiteClient::new(http))
}

fn fake_state() -> Arc<Mutex<willikins_providers_fake::FakeState>> {
    Arc::new(Mutex::new(willikins_providers_fake::FakeState::new()))
}

fn spec_json(spec: &ToolSpec) -> serde_json::Value {
    serde_json::to_value(spec).expect("ToolSpec serializes")
}

#[test]
fn buildkite_pipeline_ensure_spec_equals_the_fake_tools() {
    let live = BuildkitePipelineEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::FakeBuildkitePipelineEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn buildkite_cluster_get_spec_equals_the_fake_tools() {
    let live = BuildkiteClusterGet::new(test_client());
    let fake = willikins_providers_fake::tools::FakeBuildkiteClusterGet::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn every_spec_validates_against_the_type_registry() {
    for spec in [
        BuildkitePipelineEnsure::new(test_client()).spec(),
        BuildkiteClusterGet::new(test_client()).spec(),
    ] {
        spec.validate(willikins_types::registry())
            .expect("valid spec");
    }
}

#[test]
fn snapshot_both_live_tool_specs() {
    insta::assert_json_snapshot!(
        "buildkite_pipeline_ensure_spec",
        spec_json(BuildkitePipelineEnsure::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "buildkite_cluster_get_spec",
        spec_json(BuildkiteClusterGet::new(test_client()).spec())
    );
}
