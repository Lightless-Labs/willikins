//! The live `appstore.bundle_id.ensure` and
//! `appstore.bundle_id_capability.ensure` `ToolSpec`s equal
//! `willikins_providers_fake`'s tools of the same names, field for
//! field. `ToolSpec` derives `Serialize` but not `PartialEq`, so equality
//! here is JSON equality; an insta snapshot of both specs pins the exact
//! shape besides.

use std::sync::{Arc, Mutex};

use willikins_core::{Tool, ToolSpec};
use willikins_providers_appstore::{AppstoreBundleIdCapabilityEnsure, AppstoreBundleIdEnsure};

const NOWHERE: &str = "http://127.0.0.1:1";

fn fake_state() -> Arc<Mutex<willikins_providers_fake::FakeState>> {
    Arc::new(Mutex::new(willikins_providers_fake::FakeState::new()))
}

fn spec_json(spec: &ToolSpec) -> serde_json::Value {
    serde_json::to_value(spec).expect("ToolSpec serializes")
}

#[test]
fn appstore_bundle_id_ensure_spec_equals_the_fake_tool() {
    let live = AppstoreBundleIdEnsure::new(NOWHERE);
    let fake = willikins_providers_fake::tools::FakeAppstoreBundleIdEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn appstore_bundle_id_capability_ensure_spec_equals_the_fake_tool() {
    let live = AppstoreBundleIdCapabilityEnsure::new(NOWHERE);
    let fake =
        willikins_providers_fake::tools::FakeAppstoreBundleIdCapabilityEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn every_spec_validates_against_the_type_registry() {
    for spec in [
        AppstoreBundleIdEnsure::new(NOWHERE).spec(),
        AppstoreBundleIdCapabilityEnsure::new(NOWHERE).spec(),
    ] {
        spec.validate(willikins_types::registry())
            .expect("valid spec");
    }
}

#[test]
fn snapshot_both_live_tool_specs() {
    insta::assert_json_snapshot!(
        "appstore_bundle_id_ensure_spec",
        spec_json(AppstoreBundleIdEnsure::new(NOWHERE).spec())
    );
    insta::assert_json_snapshot!(
        "appstore_bundle_id_capability_ensure_spec",
        spec_json(AppstoreBundleIdCapabilityEnsure::new(NOWHERE).spec())
    );
}
