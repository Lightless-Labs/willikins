//! Milestone 3j, task A1 (acceptance 2): a `FakeState` JSON document
//! that seeds `doppler_configs` and `doppler_config_inheritable` with an
//! underscored project (`shared_keys/prd`, the plan's own placeholder)
//! deserializes, and the fake `doppler.config.inheritable.gate` reads it
//! `Present`. Before task A1 widened `DopplerProject`, the key's own
//! `DopplerConfig::parse` inside the gate would have refused the
//! underscore.

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, Observation, PortName, ToolName, Value};
use willikins_providers_fake::{FakeState, catalog};
use willikins_types::{DomainType, DopplerConfig};

fn port(name: &str) -> PortName {
    PortName::parse(name).expect("a test port name is valid")
}

fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).expect("a test tool name is valid")
}

fn shared_keys_prd() -> DopplerConfig {
    DopplerConfig::parse("shared_keys/prd").expect("underscored project, task A1")
}

#[test]
fn a_fake_state_json_seeding_an_underscored_base_config_deserializes_and_the_gate_reads_present() {
    let json = r#"{
        "doppler_configs": ["shared_keys/prd"],
        "doppler_config_inheritable": ["shared_keys/prd"]
    }"#;
    let state = FakeState::from_json(json).expect("the underscored keys deserialize");

    let fake_catalog = catalog(Arc::new(Mutex::new(state)));
    let gate = fake_catalog
        .get(&tool_name("doppler.config.inheritable.gate"))
        .expect("the gate is registered");

    let mut inputs = Inputs::new();
    inputs.insert(port("config"), Value::known(shared_keys_prd()));
    let observation = gate.read(&inputs).unwrap();
    assert!(
        matches!(observation, Observation::Present(_)),
        "{observation:?}"
    );
}
