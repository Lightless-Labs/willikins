//! The live `signoz.ingestion_key.ensure` `ToolSpec` equals
//! `willikins_providers_fake`'s tool of the same name, field for field.
//! `ToolSpec` derives `Serialize` but not `PartialEq`, so equality here
//! is JSON equality; an insta snapshot pins the exact shape besides.

use std::sync::{Arc, Mutex};

use willikins_core::{Tool, ToolSpec};
use willikins_providers_http::Credential;
use willikins_providers_signoz::{SigNozClient, SigNozIngestionKeyEnsure};

fn test_client() -> Arc<SigNozClient> {
    let credential = Credential::for_testing(
        "WILLIKINS_TEST_SIGNOZ_API_KEY",
        "test-signoz-api-key-000000",
    );
    let http = willikins_providers_signoz::http_client("http://127.0.0.1:0", credential);
    Arc::new(SigNozClient::new(http))
}

fn fake_state() -> Arc<Mutex<willikins_providers_fake::FakeState>> {
    Arc::new(Mutex::new(willikins_providers_fake::FakeState::new()))
}

fn spec_json(spec: &ToolSpec) -> serde_json::Value {
    serde_json::to_value(spec).expect("ToolSpec serializes")
}

#[test]
fn signoz_ingestion_key_ensure_spec_equals_the_fake_tool() {
    let live = SigNozIngestionKeyEnsure::new(test_client());
    let fake = willikins_providers_fake::tools::SigNozIngestionKeyEnsure::new(fake_state());
    assert_eq!(spec_json(live.spec()), spec_json(fake.spec()));
}

#[test]
fn the_spec_validates_against_the_type_registry() {
    SigNozIngestionKeyEnsure::new(test_client())
        .spec()
        .validate(willikins_types::registry())
        .expect("valid spec");
}

#[test]
fn snapshot_the_live_tool_spec() {
    insta::assert_json_snapshot!(
        "signoz_ingestion_key_ensure_spec",
        spec_json(SigNozIngestionKeyEnsure::new(test_client()).spec())
    );
}
