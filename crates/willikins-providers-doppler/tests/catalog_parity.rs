//! Acceptance test 1's share for this crate: the live `doppler.project.ensure`,
//! `doppler.config.ensure`, `doppler.config.inheritable.ensure`,
//! `doppler.config.inherits.ensure`, `doppler.service_token.ensure`,
//! `doppler.service_token.rotate`, `doppler.secret.get`,
//! `doppler.secret.set`, and `doppler.value.get` `ToolSpec`s equal
//! `willikins_providers_fake`'s tools of the same names, field for field.
//! `ToolSpec` derives `Serialize` but not `PartialEq` (its `class` and
//! `pure` fields aside, comparing it structurally means comparing its
//! `Serialize` form), so equality here is JSON equality; an insta
//! snapshot of all nine specs pins the exact shape besides.

use std::sync::{Arc, Mutex};

use willikins_core::{Tool, ToolSpec};
use willikins_providers_doppler::{
    DopplerClient, DopplerConfigEnsure, DopplerConfigInheritableEnsure,
    DopplerConfigInheritsEnsure, DopplerProjectEnsure, DopplerSecretGet, DopplerSecretSet,
    DopplerServiceTokenEnsure, DopplerServiceTokenRotate, DopplerValueGet,
};
use willikins_providers_http::{Credential, Http};

fn test_client() -> Arc<DopplerClient> {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new("http://127.0.0.1:0", Vec::new(), credential);
    Arc::new(DopplerClient::new(http))
}

fn fake_state() -> Arc<Mutex<willikins_providers_fake::FakeState>> {
    Arc::new(Mutex::new(willikins_providers_fake::FakeState::new()))
}

fn spec_json(spec: &ToolSpec) -> serde_json::Value {
    serde_json::to_value(spec).expect("ToolSpec serializes")
}

#[test]
fn doppler_project_ensure_spec_equals_the_fake_tools() {
    let live = DopplerProjectEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerProjectEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn doppler_config_ensure_spec_equals_the_fake_tools() {
    let live = DopplerConfigEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerConfigEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn doppler_config_inheritable_ensure_spec_equals_the_fake_tools() {
    let live = DopplerConfigInheritableEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerConfigInheritableEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn doppler_config_inherits_ensure_spec_equals_the_fake_tools() {
    let live = DopplerConfigInheritsEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerConfigInheritsEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn doppler_service_token_ensure_spec_equals_the_fake_tools() {
    let live = DopplerServiceTokenEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerServiceTokenEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn doppler_service_token_rotate_spec_equals_the_fake_tools() {
    let live = DopplerServiceTokenRotate::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerServiceTokenRotate::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn doppler_secret_get_spec_equals_the_fake_tools() {
    let live = DopplerSecretGet::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerSecretGet::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn doppler_secret_set_spec_equals_the_fake_tools() {
    let live = DopplerSecretSet::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerSecretSet::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn doppler_value_get_spec_equals_the_fake_tools() {
    let live = DopplerValueGet::new(test_client());
    let fake = willikins_providers_fake::tools::DopplerValueGet::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn every_spec_validates_against_the_type_registry() {
    for spec in [
        DopplerProjectEnsure::new(test_client()).spec(),
        DopplerConfigEnsure::new(test_client()).spec(),
        DopplerConfigInheritableEnsure::new(test_client()).spec(),
        DopplerConfigInheritsEnsure::new(test_client()).spec(),
        DopplerServiceTokenEnsure::new(test_client()).spec(),
        DopplerServiceTokenRotate::new(test_client()).spec(),
        DopplerSecretGet::new(test_client()).spec(),
        DopplerSecretSet::new(test_client()).spec(),
        DopplerValueGet::new(test_client()).spec(),
    ] {
        spec.validate(willikins_types::registry())
            .expect("valid spec");
    }
}

#[test]
fn snapshot_all_live_tool_specs() {
    insta::assert_json_snapshot!(
        "doppler_project_ensure_spec",
        spec_json(DopplerProjectEnsure::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "doppler_config_ensure_spec",
        spec_json(DopplerConfigEnsure::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "doppler_config_inheritable_ensure_spec",
        spec_json(DopplerConfigInheritableEnsure::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "doppler_config_inherits_ensure_spec",
        spec_json(DopplerConfigInheritsEnsure::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "doppler_service_token_ensure_spec",
        spec_json(DopplerServiceTokenEnsure::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "doppler_service_token_rotate_spec",
        spec_json(DopplerServiceTokenRotate::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "doppler_secret_get_spec",
        spec_json(DopplerSecretGet::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "doppler_secret_set_spec",
        spec_json(DopplerSecretSet::new(test_client()).spec())
    );
    insta::assert_json_snapshot!(
        "doppler_value_get_spec",
        spec_json(DopplerValueGet::new(test_client()).spec())
    );
}
