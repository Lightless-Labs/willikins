//! Milestone 3h, task E1 (decision (b); acceptance 2): `Action::Update`.
//!
//! Mirrors `apply_replaces.rs`'s shape for `Tool::replaces`: small in-test
//! tools only, no provider, no fake catalog, because the mechanism under
//! test (`plan_one`'s `Absent` branch, `refuse_unplanned_replacement`'s
//! lack of a counterpart, and `check_drift`'s existing `Action` comparison)
//! belongs to `willikins-core` itself.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use crate::common::{input, node, port, tool_name, ty, workflow_name};
use willikins_core::{
    Action, ApplyError, Approval, Binding, Catalog, Class, DriftKind, Ensured, Gate, InputSpec,
    Inputs, Node, Observation, Outputs, PlanError, PortSpec, PortType, RecordingObserver,
    SinkToken, Tool, ToolError, ToolErrorKind, ToolSpec, Value, Workflow, apply, check, plan,
};
use willikins_types::{DomainType, EnvironmentSlug};

// ---------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------

fn value_port_spec() -> IndexMap<willikins_core::PortName, PortSpec> {
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

fn spec(name: &str, pure: bool) -> ToolSpec {
    let mut outputs = IndexMap::new();
    outputs.insert(port("value"), ty("EnvironmentSlug"));
    ToolSpec {
        name: tool_name(name),
        description: "Test tool.".to_string(),
        inputs: value_port_spec(),
        outputs,
        // `Catalog::insert` refuses a pure tool with a key
        // (`CatalogError::PureToolWithKey`): a pure tool has no external
        // resource to key, so only `Toggleable` (never pure) declares one.
        key: if pure {
            Vec::new()
        } else {
            vec![port("value")]
        },
        class: Class::Reversible,
        pure,
    }
}

fn workflow(tool: &str) -> Workflow {
    Workflow::new(workflow_name("update-test"))
        .input(input("key"), InputSpec::new(ty("EnvironmentSlug")))
        .node(
            node("subject"),
            Node::new(tool_name(tool)).port(port("value"), Binding::Input(input("key"))),
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

fn action_of(result: &willikins_core::Plan, name: &str) -> Action {
    result
        .nodes
        .iter()
        .find(|n| n.name.as_str() == name)
        .unwrap_or_else(|| panic!("no planned node `{name}`"))
        .action
}

// ---------------------------------------------------------------------
// Toggleable: a non-pure, non-gate tool whose `read`, `replaces` and
// `updates` are all driven by shared, mutable state, with call counters
// so a test can assert a hook was (or was never) reached.
// ---------------------------------------------------------------------

#[derive(Default)]
struct World {
    present: bool,
    replaces: bool,
    updates: bool,
    updates_err: Option<String>,
    update_calls: u32,
    ensure_calls: u32,
}

struct Toggleable {
    spec: ToolSpec,
    world: Arc<Mutex<World>>,
}

impl Toggleable {
    fn new(world: Arc<Mutex<World>>) -> Self {
        Self {
            spec: spec("test.toggleable", false),
            world,
        }
    }
}

impl Tool for Toggleable {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let outputs = value_outputs(inputs);
        if self.world.lock().unwrap().present {
            Ok(Observation::Present(outputs))
        } else {
            Ok(Observation::Absent { predicted: outputs })
        }
    }

    fn replaces(&self, _inputs: &Inputs) -> Result<bool, ToolError> {
        Ok(self.world.lock().unwrap().replaces)
    }

    fn updates(&self, _inputs: &Inputs) -> Result<bool, ToolError> {
        let mut world = self.world.lock().unwrap();
        world.update_calls += 1;
        if let Some(message) = world.updates_err.clone() {
            return Err(ToolError {
                kind: ToolErrorKind::Provider,
                message,
            });
        }
        Ok(world.updates)
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        self.world.lock().unwrap().ensure_calls += 1;
        Ok(Ensured {
            outputs: value_outputs(inputs),
            changed: true,
        })
    }
}

fn toggleable_catalog(world: &Arc<Mutex<World>>) -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(Toggleable::new(Arc::clone(world))))
        .unwrap();
    catalog
}

// ---------------------------------------------------------------------
// NeverUpdates: a tool whose `updates` panics -- proof it is never
// called for a pure tool or a gate, exactly as the task asks.
// ---------------------------------------------------------------------

static GATE: Gate = Gate {
    need: "the operator makes the test condition true",
    how: "do the manual thing, then re-run this document",
    subject: &["value"],
};

struct NeverUpdates {
    spec: ToolSpec,
    is_gate: bool,
}

impl Tool for NeverUpdates {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: value_outputs(inputs),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        unreachable!("this test never applies a plan")
    }

    fn gate(&self) -> Option<&Gate> {
        if self.is_gate { Some(&GATE) } else { None }
    }

    fn updates(&self, _inputs: &Inputs) -> Result<bool, ToolError> {
        unreachable!("a pure tool or a gate must never be asked `updates`")
    }
}

fn never_updates_catalog(is_gate: bool) -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(NeverUpdates {
            spec: spec("test.never_updates", true),
            is_gate,
        }))
        .unwrap();
    catalog
}

// ---------------------------------------------------------------------
// Acceptance test 2
// ---------------------------------------------------------------------

/// `Absent` with `updates` true plans `Update`.
#[test]
fn absent_with_updates_true_plans_update() {
    let world = Arc::new(Mutex::new(World {
        updates: true,
        ..World::default()
    }));
    let catalog = toggleable_catalog(&world);
    let checked = check(&workflow("test.toggleable"), &catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let result = plan(&checked, &inputs, &catalog).expect("plans cleanly");

    assert_eq!(action_of(&result, "subject"), Action::Update);
    assert_eq!(world.lock().unwrap().update_calls, 1);
}

/// `replaces` true wins over `updates` true: the plan shows `Replace`, and
/// `updates` is never even called (short-circuited).
#[test]
fn replaces_wins_over_updates() {
    let world = Arc::new(Mutex::new(World {
        replaces: true,
        updates: true,
        ..World::default()
    }));
    let catalog = toggleable_catalog(&world);
    let checked = check(&workflow("test.toggleable"), &catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let result = plan(&checked, &inputs, &catalog).expect("plans cleanly");

    assert_eq!(action_of(&result, "subject"), Action::Replace);
    assert_eq!(
        world.lock().unwrap().update_calls,
        0,
        "`updates` must not be called once `replaces` already said `true`"
    );
}

/// `Present` plans `NoOp`, and `updates` is never called: the hook is only
/// ever consulted from the `Absent` branch.
#[test]
fn present_plans_no_op_and_never_calls_updates() {
    let world = Arc::new(Mutex::new(World {
        present: true,
        updates: true,
        ..World::default()
    }));
    let catalog = toggleable_catalog(&world);
    let checked = check(&workflow("test.toggleable"), &catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let result = plan(&checked, &inputs, &catalog).expect("plans cleanly");

    assert_eq!(action_of(&result, "subject"), Action::NoOp);
    assert_eq!(world.lock().unwrap().update_calls, 0);
}

/// A pure tool never has `updates` called, whatever its `read` reports --
/// a panicking implementation proves it.
#[test]
fn pure_tool_never_calls_updates() {
    let catalog = never_updates_catalog(false);
    let checked = check(&workflow("test.never_updates"), &catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let result = plan(&checked, &inputs, &catalog).expect("plans cleanly, never panics");

    assert_eq!(action_of(&result, "subject"), Action::Compute);
}

/// A gate never has `updates` called, whatever its `read` reports -- a
/// panicking implementation proves it.
#[test]
fn gate_never_calls_updates() {
    let catalog = never_updates_catalog(true);
    let checked = check(&workflow("test.never_updates"), &catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let result = plan(&checked, &inputs, &catalog).expect("plans cleanly, never panics");

    assert_eq!(action_of(&result, "subject"), Action::Blocked);
}

/// A `ToolError` from `updates` maps to `PlanError::Tool`, exactly as
/// `replaces`' own error does.
#[test]
fn updates_error_maps_to_plan_error_tool() {
    let world = Arc::new(Mutex::new(World {
        updates_err: Some("provider unavailable".to_string()),
        ..World::default()
    }));
    let catalog = toggleable_catalog(&world);
    let checked = check(&workflow("test.toggleable"), &catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let result = plan(&checked, &inputs, &catalog);

    let Err(PlanError::Tool { node, error }) = result else {
        panic!("expected PlanError::Tool, got {result:?}");
    };
    assert_eq!(node.as_str(), "subject");
    assert_eq!(error.kind, ToolErrorKind::Provider);
    assert_eq!(error.message, "provider unavailable");
}

/// An approved `Create` that re-plans as `Update` (the resource's state
/// changed between planning and applying) fails `apply` with
/// `DriftKind::Action`, before `ensure` ever runs -- the pre-run re-plan
/// already catches this; milestone 3h adds no new mid-run guard for it.
#[test]
fn approved_create_that_replans_update_fails_apply_with_action_drift() {
    let world = Arc::new(Mutex::new(World::default()));
    let catalog = toggleable_catalog(&world);
    let checked = check(&workflow("test.toggleable"), &catalog).expect("checks cleanly");
    let inputs = key_inputs();

    let approved = plan(&checked, &inputs, &catalog).unwrap();
    assert_eq!(action_of(&approved, "subject"), Action::Create);

    // The resource's state changes between planning and applying: the
    // tool would now report `updates: true`.
    world.lock().unwrap().updates = true;

    let mut observer = RecordingObserver::new();
    let result = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    );

    let Err(ApplyError::Drift { node, kind, .. }) = result else {
        panic!("expected ApplyError::Drift, got {result:?}");
    };
    assert_eq!(node.as_str(), "subject");
    assert!(
        matches!(
            *kind,
            DriftKind::Action {
                planned: Action::Create,
                observed: Action::Update,
            }
        ),
        "expected DriftKind::Action{{Create, Update}}, got {kind:?}"
    );
    assert_eq!(
        world.lock().unwrap().ensure_calls,
        0,
        "ensure must never run once drift is detected"
    );
}
