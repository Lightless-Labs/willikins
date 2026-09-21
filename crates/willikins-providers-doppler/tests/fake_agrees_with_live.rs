//! The fake `doppler.config.inheritable.ensure` and
//! `doppler.config.inherits.ensure` agree with their live counterparts
//! *behaviourally*, not only on their `ToolSpec`s.
//!
//! `catalog_parity.rs` pins the two catalogs' specs equal — agreement by
//! construction, since both specs are hand-written. Milestone 3a's
//! `willikins-providers-buildkite/tests/fake_agrees_with_live.rs` closed
//! the same gap for Buildkite after mutation found the fake and live
//! copies of a frozen form could drift while every other test in both
//! crates stayed green; this file is this milestone's own instance of
//! that check, for the two config-inheritance tools milestone 3 added.
//!
//! Method: for each observation shape (`Present`, `Absent`, `Mismatch`,
//! and the missing-parent tolerance) seed the fake's state and serve the
//! live tool a mock body standing for *the same real world*, then assert
//! both tools answer the same way.

use std::sync::{Arc, Mutex};

use willikins_core::{Inputs, Observation, PortName, Tool, Value};
use willikins_providers_doppler::{DopplerClient, DopplerConfigInheritableEnsure};
use willikins_providers_fake::FakeState;
use willikins_providers_http::testing::{MockProvider, load_fixture};
use willikins_providers_http::{Credential, Http};
use willikins_types::{DomainType, DopplerConfig};

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(&fixtures_dir(), "doppler", name)
}

fn config() -> DopplerConfig {
    DopplerConfig::parse("third-thoughts/prd").unwrap()
}

fn base() -> DopplerConfig {
    DopplerConfig::parse("shared-apple/base").unwrap()
}

fn other_base() -> DopplerConfig {
    DopplerConfig::parse("shared-apple/other").unwrap()
}

fn config_inputs() -> Inputs {
    let mut inputs = Inputs::new();
    inputs.insert(PortName::parse("config").unwrap(), Value::known(config()));
    inputs
}

fn inherits_inputs() -> Inputs {
    let mut inputs = config_inputs();
    inputs.insert(
        PortName::parse("inherits").unwrap(),
        Value::known_list(vec![base()]),
    );
    inputs
}

/// The name of an observation, so two `Observation`s can be compared
/// without `Observation` needing `PartialEq`.
fn shape(observation: &Observation) -> String {
    match observation {
        Observation::Absent { .. } => "Absent".to_string(),
        Observation::Present(_) => "Present".to_string(),
        Observation::Foreign => "Foreign".to_string(),
        Observation::Mismatch { port } => format!("Mismatch({port})"),
    }
}

fn live_inheritable_tool(url: String) -> DopplerConfigInheritableEnsure {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new(url, Vec::new(), credential);
    DopplerConfigInheritableEnsure::new(Arc::new(DopplerClient::new(http)))
}

fn live_inherits_tool(url: String) -> willikins_providers_doppler::DopplerConfigInheritsEnsure {
    let credential = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let http = Http::new(url, Vec::new(), credential);
    willikins_providers_doppler::DopplerConfigInheritsEnsure::new(Arc::new(DopplerClient::new(
        http,
    )))
}

#[test]
fn inheritable_absent_agrees() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();

    let live = live_inheritable_tool(provider.url())
        .read(&config_inputs())
        .unwrap();
    let fake = willikins_providers_fake::tools::DopplerConfigInheritableEnsure::new(Arc::new(
        Mutex::new(FakeState::new()),
    ))
    .read(&config_inputs())
    .unwrap();
    assert_eq!(shape(&live), shape(&fake));
}

#[test]
fn inheritable_present_agrees() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inheritable_true").to_string())
        .create();

    let live = live_inheritable_tool(provider.url())
        .read(&config_inputs())
        .unwrap();
    let state = FakeState::new().with_doppler_config_inheritable(&config());
    let fake = willikins_providers_fake::tools::DopplerConfigInheritableEnsure::new(Arc::new(
        Mutex::new(state),
    ))
    .read(&config_inputs())
    .unwrap();
    assert_eq!(shape(&live), shape(&fake));
}

/// An explicit `inheritable: false` agrees too, and it is the case a
/// mutation hides: the fake records membership, so it cannot tell
/// "never marked" from "marked and unmarked", while the live tool reads
/// a real field that can say either. Weakening the live read to
/// `.is_some()` makes this test the one that disagrees.
#[test]
fn inheritable_explicitly_false_agrees() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inheritable_false").to_string())
        .create();

    let live = live_inheritable_tool(provider.url())
        .read(&config_inputs())
        .unwrap();
    let fake = willikins_providers_fake::tools::DopplerConfigInheritableEnsure::new(Arc::new(
        Mutex::new(FakeState::new()),
    ))
    .read(&config_inputs())
    .unwrap();
    assert_eq!(shape(&live), shape(&fake));
}

/// The missing-parent tolerance both crates' reads share: a `404` (fake
/// has no concept of "the parent is missing" -- an empty state already
/// answers the same way `Absent` does).
#[test]
fn inheritable_missing_parent_agrees_with_an_empty_fake_state() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(404)
        .with_body(fixture("error_404").to_string())
        .create();

    let live = live_inheritable_tool(provider.url())
        .read(&config_inputs())
        .unwrap();
    let fake = willikins_providers_fake::tools::DopplerConfigInheritableEnsure::new(Arc::new(
        Mutex::new(FakeState::new()),
    ))
    .read(&config_inputs())
    .unwrap();
    assert_eq!(shape(&live), shape(&fake));
}

#[test]
fn inherits_absent_agrees() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_present").to_string())
        .create();

    let live = live_inherits_tool(provider.url())
        .read(&inherits_inputs())
        .unwrap();
    let fake = willikins_providers_fake::tools::DopplerConfigInheritsEnsure::new(Arc::new(
        Mutex::new(FakeState::new()),
    ))
    .read(&inherits_inputs())
    .unwrap();
    assert_eq!(shape(&live), shape(&fake));
}

#[test]
fn inherits_present_agrees() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inherits_present").to_string())
        .create();

    let live = live_inherits_tool(provider.url())
        .read(&inherits_inputs())
        .unwrap();
    let state = FakeState::new().with_doppler_config_inherits(&config(), &[base()]);
    let fake = willikins_providers_fake::tools::DopplerConfigInheritsEnsure::new(Arc::new(
        Mutex::new(state),
    ))
    .read(&inherits_inputs())
    .unwrap();
    assert_eq!(shape(&live), shape(&fake));
}

/// The case a mutation could hide: an extra base already inherited reads
/// `Mismatch` on both sides, never `Present` (silently ignoring the
/// extra) or `Absent` (silently offering to overwrite it).
#[test]
fn inherits_mismatch_on_an_extra_agrees() {
    let mut provider = MockProvider::start();
    provider
        .mock(
            "GET",
            "/v3/configs/config?project=third-thoughts&config=prd",
        )
        .with_status(200)
        .with_body(fixture("config_get_inherits_extra").to_string())
        .create();

    let live = live_inherits_tool(provider.url())
        .read(&inherits_inputs())
        .unwrap();
    let state = FakeState::new().with_doppler_config_inherits(&config(), &[base(), other_base()]);
    let fake = willikins_providers_fake::tools::DopplerConfigInheritsEnsure::new(Arc::new(
        Mutex::new(state),
    ))
    .read(&inherits_inputs())
    .unwrap();
    assert_eq!(shape(&live), shape(&fake));
}
