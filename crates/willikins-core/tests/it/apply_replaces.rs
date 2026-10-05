//! Milestone 3e, adversarial pass 3 (2026-09-29): a replacement `apply`
//! was never shown.
//!
//! `apply` calls `Tool::ensure` for every non-pure instance whose inputs
//! are known, whatever its planned `Action` -- a `NoOp` included. A tool
//! whose `ensure` deletes before creating (`appstore.profile.ensure`'s
//! replace-when-INVALID) therefore deletes whenever its resource is
//! invalidated *during* the run, by an earlier node in the same `apply`
//! (Apple invalidates a profile when a capability is enabled on its App
//! ID). Both `plan` and `apply`'s own re-plan saw the resource valid, so the
//! approved plan said `NoOp` and `Plan::replacing` was empty -- exactly the
//! "an approved plan shows only `Create`/`NoOp` while a delete runs" case F1
//! was meant to close.
//!
//! The rule these tests pin: `ensure` never runs a replacement the
//! approved plan did not show. Just before `ensure`, an instance whose
//! planned action is not `Action::Replace` asks `Tool::replaces`; `true`
//! fails that instance (`Conflict`, nothing deleted), and a re-run plans
//! the `Replace` for an approver to see.
//!
//! In-test tools only, as in `apply_gates.rs`: the mechanism belongs to
//! `willikins-core`, and the fake catalog does not model Apple's
//! invalidation.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use crate::common::{input, node, port, tool_name, ty, workflow_name};
use willikins_core::{
    Action, ApplyError, Approval, Binding, Catalog, Class, Ensured, InputSpec, Inputs, Node,
    NodeStatus, Observation, Outputs, PortSpec, PortType, RecordingObserver, SinkToken, Tool,
    ToolError, ToolErrorKind, ToolSpec, Value, Workflow, apply, check, plan,
};
use willikins_types::{DomainType, EnvironmentSlug};

/// The shared provider state: one replaceable resource that exists and is
/// valid until the invalidator runs.
#[derive(Default)]
struct World {
    /// The invalidator's own resource exists.
    invalidator_done: bool,
    /// The replaceable resource is invalid (Apple's `INVALID`).
    invalid: bool,
    /// How many times the replaceable tool's `ensure` deleted.
    deletes: u32,
}

fn one_slug_port() -> IndexMap<willikins_core::PortName, PortSpec> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        port("value"),
        PortSpec {
            ty: PortType::Exact(ty("EnvironmentSlug")),
            required: true,
            derived_only: false,
        },
    );
    inputs
}

fn value_outputs(inputs: &Inputs) -> Outputs {
    let mut outputs = Outputs::new();
    outputs.insert(
        port("value"),
        inputs.get(&port("value")).expect("always bound").clone(),
    );
    outputs
}

/// Creates its own resource once; doing so invalidates the replaceable
/// resource, the way enabling a capability invalidates a profile.
struct Invalidator {
    spec: ToolSpec,
    world: Arc<Mutex<World>>,
}

impl Tool for Invalidator {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let outputs = value_outputs(inputs);
        if self.world.lock().unwrap().invalidator_done {
            Ok(Observation::Present(outputs))
        } else {
            Ok(Observation::Absent { predicted: outputs })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let mut world = self.world.lock().unwrap();
        let changed = !world.invalidator_done;
        if changed {
            world.invalidator_done = true;
            world.invalid = true;
        }
        Ok(Ensured {
            outputs: value_outputs(inputs),
            changed,
        })
    }
}

/// `appstore.profile.ensure`'s shape: `read` is `Present` while valid and
/// `Absent` once invalid; `replaces` is `true` exactly when invalid;
/// `ensure` deletes the invalid resource, then creates a fresh one.
struct Replaceable {
    spec: ToolSpec,
    world: Arc<Mutex<World>>,
}

impl Tool for Replaceable {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let outputs = value_outputs(inputs);
        if self.world.lock().unwrap().invalid {
            Ok(Observation::Absent { predicted: outputs })
        } else {
            Ok(Observation::Present(outputs))
        }
    }

    fn replaces(&self, _inputs: &Inputs) -> Result<bool, ToolError> {
        Ok(self.world.lock().unwrap().invalid)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let mut world = self.world.lock().unwrap();
        let changed = if world.invalid {
            world.deletes += 1;
            world.invalid = false;
            true
        } else {
            false
        };
        Ok(Ensured {
            outputs: value_outputs(inputs),
            changed,
        })
    }
}

fn spec(name: &str, key: bool) -> ToolSpec {
    let mut outputs = IndexMap::new();
    outputs.insert(port("value"), ty("EnvironmentSlug"));
    ToolSpec {
        name: tool_name(name),
        description: "Test tool.".to_string(),
        inputs: one_slug_port(),
        outputs,
        key: if key { vec![port("value")] } else { Vec::new() },
        class: Class::Reversible,
        pure: false,
    }
}

fn catalog(world: &Arc<Mutex<World>>) -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(Invalidator {
            spec: spec("test.invalidator", false),
            world: Arc::clone(world),
        }))
        .unwrap();
    catalog
        .insert(Arc::new(Replaceable {
            spec: spec("test.replaceable", true),
            world: Arc::clone(world),
        }))
        .unwrap();
    catalog
}

/// `invalidator` then `profile`, independent of each other and ordered by
/// declaration -- Sample's shape: its capability nodes are declared before,
/// and never feed, its profile nodes.
fn workflow() -> Workflow {
    Workflow::new(workflow_name("replace-in-run-test"))
        .input(input("key"), InputSpec::new(ty("EnvironmentSlug")))
        .node(
            node("invalidator"),
            Node::new(tool_name("test.invalidator"))
                .port(port("value"), Binding::Input(input("key"))),
        )
        .node(
            node("profile"),
            Node::new(tool_name("test.replaceable"))
                .port(port("value"), Binding::Input(input("key"))),
        )
}

fn key_inputs() -> IndexMap<willikins_core::InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("key"),
        Value::known(EnvironmentSlug::parse("dev").unwrap()),
    );
    inputs
}

fn action_of(plan: &willikins_core::Plan, name: &str) -> Action {
    plan.nodes
        .iter()
        .find(|n| n.name.as_str() == name)
        .unwrap_or_else(|| panic!("no planned node `{name}`"))
        .action
}

/// The attack: the approved plan says `profile: NoOp` and names nothing in
/// `replacing`; an earlier node in the same run invalidates it. `apply`
/// must not delete it. The instance fails with `Conflict`, and the next
/// plan shows the `Replace` and names it.
#[test]
fn apply_never_deletes_what_the_approved_plan_showed_as_no_op() {
    let world = Arc::new(Mutex::new(World::default()));
    let catalog = catalog(&world);
    let checked = check(&workflow(), &catalog).expect("checks cleanly");
    let inputs = key_inputs();

    let approved = plan(&checked, &inputs, &catalog).unwrap();
    assert_eq!(action_of(&approved, "invalidator"), Action::Create);
    assert_eq!(action_of(&approved, "profile"), Action::NoOp);
    assert!(approved.replacing.is_empty());

    let mut observer = RecordingObserver::new();
    let result = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    );

    assert_eq!(
        world.lock().unwrap().deletes,
        0,
        "a delete ran that the approved plan never showed"
    );
    let Err(ApplyError::Tool {
        node,
        error,
        applied,
        ..
    }) = result
    else {
        panic!("expected the profile instance to fail, got {result:?}");
    };
    assert_eq!(node.as_str(), "profile");
    assert_eq!(error.kind, ToolErrorKind::Conflict);
    assert!(
        error.message.contains("re-run"),
        "the error says how to proceed: {}",
        error.message
    );
    assert!(matches!(
        applied
            .nodes
            .iter()
            .find(|n| n.name.as_str() == "invalidator")
            .unwrap()
            .status,
        NodeStatus::Created
    ));

    // The re-run: the plan now shows the replacement and names it.
    let replan = plan(&checked, &inputs, &catalog).unwrap();
    assert_eq!(action_of(&replan, "profile"), Action::Replace);
    assert_eq!(replan.replacing.len(), 1);
    assert_eq!(replan.replacing[0].node.as_str(), "profile");

    // Approving that plan runs the replacement, exactly once.
    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &replan,
        &Approval::Auto,
        &mut observer,
    )
    .expect("a shown replacement runs");
    assert_eq!(world.lock().unwrap().deletes, 1);
    assert!(matches!(
        applied
            .nodes
            .iter()
            .find(|n| n.name.as_str() == "profile")
            .unwrap()
            .status,
        NodeStatus::Created
    ));
}

/// The mirror: nothing invalidates the resource during the run, so the
/// check costs nothing observable -- `profile` converges `Unchanged`.
#[test]
fn a_no_op_that_stays_valid_still_converges() {
    let world = Arc::new(Mutex::new(World {
        invalidator_done: true,
        ..World::default()
    }));
    let catalog = catalog(&world);
    let checked = check(&workflow(), &catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let approved = plan(&checked, &inputs, &catalog).unwrap();
    assert_eq!(action_of(&approved, "profile"), Action::NoOp);

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("converges");
    assert_eq!(world.lock().unwrap().deletes, 0);
    assert!(matches!(
        applied
            .nodes
            .iter()
            .find(|n| n.name.as_str() == "profile")
            .unwrap()
            .status,
        NodeStatus::Unchanged
    ));
}
