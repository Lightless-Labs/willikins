//! Acceptance 16 (G3), the positive fixture: a small workflow calling the
//! real `operator.acknowledge` tool, through `willikins_providers_fake`'s
//! catalog (the same catalog a document runs against), checks cleanly,
//! plans `Action::Blocked` when its acknowledgement input is unsupplied,
//! and plans `Action::Compute` once `done` is supplied. Complements
//! `crates/willikins-core/tests/plan_gates.rs`'s small in-test gate tool,
//! which proves the engine mechanism in the abstract; this proves the one
//! production tool that uses it.

use indexmap::IndexMap;

use willikins_core::{
    Action, Binding, InputName, InputSpec, Node, NodeName, PortName, ToolName, Value, Workflow,
    check, plan,
};
use willikins_types::{DomainType, WorkflowName};

fn workflow() -> Workflow {
    Workflow::new(WorkflowName::parse("acknowledge-fixture").unwrap())
        .input(
            InputName::parse("m7_bootstrap_done").unwrap(),
            InputSpec::new(willikins_core::TypeRef::scalar(
                willikins_core::TypeName::parse("OperatorAcknowledgement").unwrap(),
            )),
        )
        .node(
            NodeName::parse("m7_bootstrap").unwrap(),
            Node::new(ToolName::parse("operator.acknowledge").unwrap())
                .port(
                    PortName::parse("step").unwrap(),
                    Binding::Literal("Replace the walter pipeline's stored bootstrap".to_string()),
                )
                .port(
                    PortName::parse("acknowledged").unwrap(),
                    Binding::Input(InputName::parse("m7_bootstrap_done").unwrap()),
                ),
        )
}

#[test]
fn checks_cleanly_against_the_real_fake_catalog() {
    let (_state, catalog) = willikins_providers_fake::empty();
    check(&workflow(), &catalog).expect("a well-formed operator.acknowledge node checks cleanly");
}

#[test]
fn plans_blocked_without_the_acknowledgement_input() {
    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = check(&workflow(), &catalog).unwrap();

    let inputs: IndexMap<InputName, Value> = IndexMap::new();
    let planned = plan(&checked, &inputs, &catalog)
        .expect("an unsupplied OperatorAcknowledgement input never fails `plan`");

    let node = planned
        .nodes
        .iter()
        .find(|n| n.name.as_str() == "m7_bootstrap")
        .expect("the node was planned");
    assert_eq!(node.action, Action::Blocked);

    assert_eq!(planned.blocked.len(), 1);
    let entry = &planned.blocked[0];
    assert_eq!(entry.tool.as_str(), "operator.acknowledge");
    assert_eq!(
        entry.awaiting_inputs,
        vec![InputName::parse("m7_bootstrap_done").unwrap()]
    );
    assert_eq!(entry.subject[0].0.as_str(), "step");
    assert_eq!(
        entry.subject[0].1,
        "Replace the walter pipeline's stored bootstrap"
    );
}

#[test]
fn plans_compute_once_done_is_supplied() {
    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = check(&workflow(), &catalog).unwrap();

    let mut inputs: IndexMap<InputName, Value> = IndexMap::new();
    inputs.insert(
        InputName::parse("m7_bootstrap_done").unwrap(),
        Value::known(willikins_types::OperatorAcknowledgement::parse("done").unwrap()),
    );
    let planned = plan(&checked, &inputs, &catalog).expect("a supplied acknowledgement plans");

    let node = planned
        .nodes
        .iter()
        .find(|n| n.name.as_str() == "m7_bootstrap")
        .expect("the node was planned");
    assert_eq!(node.action, Action::Compute);
    assert!(planned.blocked.is_empty());
}
