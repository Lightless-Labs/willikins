//! Milestone 3j, task C2 (decision (c5); acceptance 10):
//! `workflows/doppler-inherited-secret-gate.yaml` over the fake catalog --
//! the tracked proof of the shape the operator's own (gitignored) iOS app
//! document uses for its APNs key.
//!
//! Every tool `inherit` (`doppler.config.inherits.ensure`) and
//! `apns_key` (`doppler.secret_name.gate`) call is `Class::Reversible`
//! and pure/idempotent, so every run here uses `Approval::Auto`.
//!
//! 1. **Run 1, base seeded with the secret.** Nothing has applied yet:
//!    `inherit` plans `Create` (the branch config does not yet inherit
//!    the base), and `apns_key`'s own `read` -- at plan time, before
//!    `inherit`'s write ever applies -- reads against `inherit`'s own
//!    *predicted* config (not yet a real, existing Doppler config), so
//!    it plans `Blocked`, naming both `config` and `name` in the blocked
//!    report.
//! 2. **Apply run 1, then plan run 2.** `inherit` now reads `NoOp` (the
//!    inheritance it wrote on run 1 already matches), and `apns_key`
//!    reads the secret through that inheritance: `Compute`, nothing
//!    `Blocked`.
//! 3. **The base is not inheritable.** `base_config_gate`'s own instance
//!    is `Blocked`; `inherit` aggregates it and plans `Skip`; `apns_key`
//!    binds `inherit`'s own (unknown, because `inherit` never ran)
//!    output and plans `Skip` too -- the base config is the only blocked
//!    gate in the whole run (task C1's pass-through edge is what buys
//!    this: see the document's own header).
//! 4. **The base is inheritable but never gets the secret.** After run
//!    1 applies (the inheritance itself is still written), run 2 still
//!    finds `apns_key` `Blocked` -- a name is not a value, and this gate
//!    only ever proves a name's visibility.
//! 5. **The binding itself.** `apns_key.config` is a `Binding::Step`
//!    reading `inherit`'s own `config` output, pinning the (c2) edge
//!    decision this document exercises.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Action, Approval, Binding, InputName, NodeName, NodeStatus, PortName, RecordingObserver,
    TypeName, TypeRef, Value, apply, check, plan,
};
use willikins_providers_fake::FakeState;
use willikins_types::{DomainType, DopplerConfig, DopplerSecretValue, SecretName};

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn document() -> willikins_core::Workflow {
    let path = workspace_root().join("workflows/doppler-inherited-secret-gate.yaml");
    willikins_dsl::load_document(&path).unwrap_or_else(|err| panic!("the document loads: {err}"))
}

fn state_fixture(name: &str) -> std::path::PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join(name)
}

fn list(type_name: &str, items: &[&str]) -> Value {
    Value::parse_list(
        &TypeRef::list_of(TypeName::parse(type_name).unwrap()),
        items,
    )
    .unwrap()
}

/// Every input the document declares, with a fixed value -- `plan` never
/// backfills a default itself (`Binding::Input` fails `MissingInput` on
/// an absent input; the operator's own (gitignored) iOS app document's
/// test suite makes exactly this same choice for its own inputs).
fn base_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("base_configs").unwrap(),
        list("DopplerConfig", &["shared_keys/prd"]),
    );
    inputs
}

fn base_config() -> DopplerConfig {
    DopplerConfig::parse("shared_keys/prd").unwrap()
}

fn secret_name() -> SecretName {
    SecretName::parse("EXAMPLE_APNS_KEY").unwrap()
}

/// The document's own fake state fixture: the base config exists, is
/// inheritable, and holds the secret.
fn seeded_state() -> Arc<Mutex<FakeState>> {
    let json = std::fs::read_to_string(state_fixture("inherited-secret-name.json")).unwrap();
    Arc::new(Mutex::new(
        FakeState::from_json(&json).unwrap_or_else(|err| panic!("seeded state: {err}")),
    ))
}

/// Like [`seeded_state`], but the base config is never marked
/// inheritable -- D1's own negative case, where `base_config_gate`
/// itself blocks.
fn state_base_not_inheritable() -> Arc<Mutex<FakeState>> {
    Arc::new(Mutex::new(
        FakeState::new()
            .with_doppler_config(&base_config())
            .with_doppler_secret(
                &base_config(),
                &secret_name(),
                DopplerSecretValue::parse("willikins test secret value").unwrap(),
            ),
    ))
}

/// Like [`seeded_state`], but the base config is inheritable and exists,
/// yet never gets the secret -- acceptance 10's "base seeded without the
/// secret" case.
fn state_base_without_secret() -> Arc<Mutex<FakeState>> {
    Arc::new(Mutex::new(
        FakeState::new()
            .with_doppler_config(&base_config())
            .with_doppler_config_inheritable(&base_config()),
    ))
}

fn action_of(planned: &willikins_core::Plan, node: &str, instance: Option<&str>) -> Action {
    planned
        .nodes
        .iter()
        .find(|n| n.name.as_str() == node && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("node `{node}` (instance {instance:?}) was planned"))
        .action
}

fn status_of<'a>(
    applied: &'a willikins_core::Applied,
    node: &str,
    instance: Option<&str>,
) -> &'a NodeStatus {
    &applied
        .nodes
        .iter()
        .find(|n| n.name.as_str() == node && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("node `{node}` (instance {instance:?}) was applied"))
        .status
}

fn approval() -> Approval {
    Approval::Auto
}

#[test]
fn the_document_checks_cleanly_against_the_fake_catalog() {
    let (_state, catalog) = willikins_providers_fake::empty();
    check(&document(), &catalog).expect("the document checks cleanly");
}

/// Acceptance 10, run 1: `inherit` plans `Create`, `apns_key` plans
/// `Blocked`, and the blocked report names both `config` and `name`.
#[test]
fn run_1_creates_inherit_and_blocks_the_key_naming_config_and_name() {
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&document(), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = base_inputs();
    let planned =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));

    assert_eq!(action_of(&planned, "inherit", None), Action::Create);
    assert_eq!(action_of(&planned, "apns_key", None), Action::Blocked);

    let entry = planned
        .blocked
        .iter()
        .find(|b| b.node.as_str() == "apns_key")
        .unwrap_or_else(|| panic!("`apns_key` is blocked: {:?}", planned.blocked));
    let subject_ports: Vec<&str> = entry
        .subject
        .iter()
        .map(|(port, _)| port.as_str())
        .collect();
    assert_eq!(
        subject_ports,
        vec!["config", "name"],
        "the blocked report must name both config and name"
    );
}

/// Acceptance 10, run 2: applying run 1 resolves `inherit` (`NoOp`) and
/// lets `apns_key` read the secret through the now-real inheritance
/// (`Compute`) -- nothing is `Blocked`.
#[test]
fn apply_run_1_then_run_2_resolves_inherit_and_computes_the_key() {
    let state = seeded_state();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&document(), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = base_inputs();

    let planned1 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));
    let mut observer = RecordingObserver::new();
    let applied1 = apply(
        &checked,
        &inputs,
        &catalog,
        &planned1,
        &approval(),
        &mut observer,
    )
    .expect("a blocked run is Ok, not an error");
    assert!(matches!(
        status_of(&applied1, "inherit", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        status_of(&applied1, "apns_key", None),
        NodeStatus::Blocked
    ));

    let planned2 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 2 plans: {err}"));
    assert_eq!(action_of(&planned2, "inherit", None), Action::NoOp);
    assert_eq!(action_of(&planned2, "apns_key", None), Action::Compute);
    assert!(
        planned2.blocked.is_empty(),
        "run 2 must report nothing blocked: {:?}",
        planned2.blocked
    );
}

/// Acceptance 10: the base config is never marked inheritable.
/// `base_config_gate` itself blocks; `inherit` aggregates all its
/// instances and plans `Skip`; `apns_key` binds `inherit`'s own output
/// and plans `Skip` too -- the base config is the only blocked gate in
/// the whole run. This is C1's pass-through edge paying off: without it,
/// `apns_key` would read its own (unrelated) absence and report a
/// second, misleading blocked gate.
#[test]
fn a_non_inheritable_base_blocks_its_gate_and_skips_inherit_and_the_key() {
    let state = state_base_not_inheritable();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&document(), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = base_inputs();
    let planned =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("the document plans: {err}"));

    assert_eq!(
        action_of(&planned, "base_config_gate", Some("shared_keys/prd")),
        Action::Blocked
    );
    assert_eq!(action_of(&planned, "inherit", None), Action::Skip);
    assert_eq!(action_of(&planned, "apns_key", None), Action::Skip);

    assert_eq!(
        planned.blocked.len(),
        1,
        "the base config must be the only blocked gate: {:?}",
        planned.blocked
    );
    let entry = &planned.blocked[0];
    assert_eq!(entry.node.as_str(), "base_config_gate");
    let holds_back: std::collections::BTreeSet<&str> =
        entry.holds_back.iter().map(NodeName::as_str).collect();
    assert_eq!(
        holds_back,
        std::collections::BTreeSet::from(["inherit", "apns_key"]),
        "the missing base config must hold back both inherit and apns_key"
    );
}

/// Acceptance 10: the base config is inheritable and exists, but never
/// gets the secret. After run 1 applies (the inheritance itself is
/// still written for real -- `inherit`'s own `ensure` never reads
/// `apns_key`'s name), run 2 still finds `apns_key` `Blocked`: a name is
/// not a value, and this gate only ever proves a name's visibility.
#[test]
fn a_base_without_the_secret_leaves_the_key_blocked_after_run_1_applies() {
    let state = state_base_without_secret();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&document(), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = base_inputs();

    let planned1 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));
    let mut observer = RecordingObserver::new();
    apply(
        &checked,
        &inputs,
        &catalog,
        &planned1,
        &approval(),
        &mut observer,
    )
    .expect("a blocked run is Ok, not an error");

    let planned2 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 2 plans: {err}"));
    assert_eq!(action_of(&planned2, "inherit", None), Action::NoOp);
    assert_eq!(
        action_of(&planned2, "apns_key", None),
        Action::Blocked,
        "the secret was never seeded in the base, so the gate must still block"
    );
}

/// Acceptance 10's last clause, and decision (c2): `apns_key.config`
/// binds `inherit`'s own `config` output, never `prd_config`'s directly
/// -- the edge that orders the gate after inheritance is set, bought by
/// task C1's pass-through.
#[test]
fn apns_key_config_binds_inherit_config() {
    let workflow = document();
    let node = &workflow.nodes[&NodeName::parse("apns_key").unwrap()];
    match node.with.get(&PortName::parse("config").unwrap()) {
        Some(Binding::Step { node: n, port }) => {
            assert_eq!(n.as_str(), "inherit");
            assert_eq!(port.as_str(), "config");
        }
        other => panic!("expected a Binding::Step onto inherit.config, got {other:?}"),
    }
}
