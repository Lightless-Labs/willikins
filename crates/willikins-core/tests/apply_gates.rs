//! Acceptance test 14 (G2, decision (j)): gates in `apply`.
//!
//! Small, in-test tools only, the same choice `plan_gates.rs` makes and for
//! the same reason: the mechanism under test (classifying `Action::Blocked`/
//! `Action::Skip` before input resolution and the `pure` branch, continuing
//! the walk past them, `Applied::blocked`, and the drift check on a gate
//! that flips between plan and apply) belongs to `willikins-core` itself,
//! independent of any real provider or the fake catalog. No gate tool
//! exists yet in the fake catalog (G1's own honest limit, carried forward
//! here): proving this through a real document and a real `Butler` binary
//! run is G3/T3's job, once Walter's own gates exist.

mod common;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use common::{input, node, port, tool_name, ty, workflow_name};
use willikins_core::{
    Action, ApplyError, ApplyEvent, Approval, Binding, Catalog, Class, Ensured, Gate, InputSpec,
    Inputs, Node, NodeStatus, Observation, Outputs, PortSpec, PortType, RecordingObserver,
    SinkToken, Tool, ToolError, ToolErrorKind, ToolSpec, Value, Workflow, apply, check, plan,
};
use willikins_types::{DomainType, EnvironmentSlug};

// ---------------------------------------------------------------------
// Test tools
// ---------------------------------------------------------------------

static GATE: Gate = Gate {
    need: "the operator makes the test condition true",
    how: "do the manual thing, then re-run this document",
    subject: &["key"],
};

/// A pure, non-`for_each` gate over one `EnvironmentSlug` port, passed
/// through as its own output. `Present` once `open` is flipped `true`,
/// `Absent` otherwise -- flipped between two `apply` calls to exercise the
/// gate-satisfied-later scenarios acceptance 14 asks for.
struct GateTool {
    spec: ToolSpec,
    open: Arc<Mutex<bool>>,
}

impl GateTool {
    fn new(open: Arc<Mutex<bool>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("key"),
            PortSpec {
                ty: PortType::Exact(ty("EnvironmentSlug")),
                required: true,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("key"), ty("EnvironmentSlug"));
        Self {
            spec: ToolSpec {
                name: tool_name("test.gate"),
                description: "Test gate over one EnvironmentSlug.".to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
            open,
        }
    }
}

impl Tool for GateTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let key = inputs
            .get(&port("key"))
            .expect("test always binds `key`")
            .clone();
        let mut outputs = Outputs::new();
        outputs.insert(port("key"), key);
        if *self.open.lock().unwrap() {
            Ok(Observation::Present(outputs))
        } else {
            Ok(Observation::Absent { predicted: outputs })
        }
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        unreachable!("a gate is pure and never ensured")
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&GATE)
    }
}

/// A non-pure, keyless tool with one required `EnvironmentSlug` input,
/// backed by an in-memory set of "created" values: `read` reports `Absent`
/// the first time a value is seen and `Present` afterwards, `ensure`
/// records it and reports `changed: true` exactly once. Used both for the
/// gate-downstream node (`downstream`, bound via `Step` to `gate`'s output)
/// and for the always-unrelated `independent` node -- the same tool
/// behaving correctly under both, so a passing test proves real
/// convergence, not merely "never called".
struct CreateOnceTool {
    spec: ToolSpec,
    created: Arc<Mutex<HashSet<String>>>,
    reads: Arc<Mutex<u32>>,
}

impl CreateOnceTool {
    fn new(name: &str, created: Arc<Mutex<HashSet<String>>>, reads: Arc<Mutex<u32>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("value"),
            PortSpec {
                ty: PortType::Exact(ty("EnvironmentSlug")),
                required: true,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), ty("EnvironmentSlug"));
        Self {
            spec: ToolSpec {
                name: tool_name(name),
                description: "Test tool: create-once, backed by a set.".to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
            created,
            reads,
        }
    }
}

impl Tool for CreateOnceTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        *self.reads.lock().unwrap() += 1;
        let value = inputs.get(&port("value")).expect("always bound").clone();
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), value.clone());
        if self
            .created
            .lock()
            .unwrap()
            .contains(&value.render().to_string())
        {
            Ok(Observation::Present(outputs))
        } else {
            Ok(Observation::Absent { predicted: outputs })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let value = inputs.get(&port("value")).expect("always bound").clone();
        let key = value.render().to_string();
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), value);
        let changed = self.created.lock().unwrap().insert(key);
        Ok(Ensured { outputs, changed })
    }
}

/// A non-pure, keyless, no-input tool whose `ensure` always fails. Used to
/// prove a tool failure still stops the walk with a `NotRun` tail even in a
/// run that also has earlier `Blocked`/`Skipped` instances.
struct AlwaysFailTool {
    spec: ToolSpec,
}

impl AlwaysFailTool {
    fn new(name: &str) -> Self {
        Self {
            spec: ToolSpec {
                name: tool_name(name),
                description: "Test tool: always fails ensure.".to_string(),
                inputs: IndexMap::new(),
                outputs: IndexMap::new(),
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
        }
    }
}

impl Tool for AlwaysFailTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Err(ToolError {
            kind: ToolErrorKind::Provider,
            message: "boom".to_string(),
        })
    }
}

// ---------------------------------------------------------------------
// The shared graph: gate -> downstream (Step), plus an unrelated
// `independent` node. `with_failing` additionally declares `failing`
// (`AlwaysFailTool`) between `downstream` and `independent` in declaration
// order, so `checked.order` visits gate, downstream, failing, independent
// -- `failing`'s error must stop before `independent` while leaving
// `gate`'s and `downstream`'s already-recorded statuses alone.
// ---------------------------------------------------------------------

fn workflow(with_failing: bool) -> Workflow {
    let mut wf = Workflow::new(workflow_name("gate-apply-test"))
        .input(input("key"), InputSpec::new(ty("EnvironmentSlug")))
        .node(
            node("gate"),
            Node::new(tool_name("test.gate")).port(port("key"), Binding::Input(input("key"))),
        )
        .node(
            node("downstream"),
            Node::new(tool_name("test.downstream")).port(
                port("value"),
                Binding::Step {
                    node: node("gate"),
                    port: port("key"),
                },
            ),
        );
    if with_failing {
        wf = wf.node(node("failing"), Node::new(tool_name("test.failing")));
    }
    wf.node(
        node("independent"),
        Node::new(tool_name("test.independent")).port(port("value"), Binding::Input(input("key"))),
    )
}

struct TestCatalog {
    catalog: Catalog,
    open: Arc<Mutex<bool>>,
    downstream_created: Arc<Mutex<HashSet<String>>>,
    downstream_reads: Arc<Mutex<u32>>,
    independent_created: Arc<Mutex<HashSet<String>>>,
}

fn build_catalog(with_failing: bool) -> TestCatalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    let open = Arc::new(Mutex::new(false));
    catalog
        .insert(Arc::new(GateTool::new(Arc::clone(&open))))
        .unwrap();

    let downstream_created = Arc::new(Mutex::new(HashSet::new()));
    let downstream_reads = Arc::new(Mutex::new(0));
    catalog
        .insert(Arc::new(CreateOnceTool::new(
            "test.downstream",
            Arc::clone(&downstream_created),
            Arc::clone(&downstream_reads),
        )))
        .unwrap();

    let independent_created = Arc::new(Mutex::new(HashSet::new()));
    catalog
        .insert(Arc::new(CreateOnceTool::new(
            "test.independent",
            Arc::clone(&independent_created),
            Arc::new(Mutex::new(0)),
        )))
        .unwrap();

    if with_failing {
        catalog
            .insert(Arc::new(AlwaysFailTool::new("test.failing")))
            .unwrap();
    }

    TestCatalog {
        catalog,
        open,
        downstream_created,
        downstream_reads,
        independent_created,
    }
}

fn key_inputs() -> IndexMap<willikins_core::InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("key"),
        Value::known(EnvironmentSlug::parse("dev").unwrap()),
    );
    inputs
}

fn applied_status(applied: &willikins_core::Applied, name: &str) -> NodeStatus {
    applied
        .nodes
        .iter()
        .find(|n| n.name.as_str() == name)
        .unwrap_or_else(|| panic!("no applied node `{name}`"))
        .status
        .clone()
}

// ---------------------------------------------------------------------
// Acceptance test 14
// ---------------------------------------------------------------------

/// A blocked run: `gate` reads `Absent` (closed), so `downstream` (bound to
/// it by `Step`) is `Skip`; `independent` (unrelated) still runs and
/// creates its resource for real. `Applied::blocked` names the one blocked
/// gate; the observer sees `NodeStarted`/`NodeFinished` for both `gate`
/// (`Blocked`) and `downstream` (`Skipped`), never a tool call for either.
#[test]
fn a_blocked_run_still_creates_every_independent_resource() {
    let tc = build_catalog(false);
    let workflow = workflow(false);
    let checked = check(&workflow, &tc.catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let approved = plan(&checked, &inputs, &tc.catalog).expect("plan never fails on a gate");
    assert_eq!(approved.blocked.len(), 1);

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &tc.catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("a blocked run is Ok, not an error");

    assert!(matches!(
        applied_status(&applied, "gate"),
        NodeStatus::Blocked
    ));
    assert!(matches!(
        applied_status(&applied, "downstream"),
        NodeStatus::Skipped
    ));
    assert!(matches!(
        applied_status(&applied, "independent"),
        NodeStatus::Created
    ));
    assert_eq!(applied.blocked.len(), 1);
    assert_eq!(applied.blocked[0].node.as_str(), "gate");

    // `downstream`'s tool was never read: a blocked run's own Skip must not
    // call through to the tool at all.
    assert_eq!(*tc.downstream_reads.lock().unwrap(), 0);
    assert!(tc.downstream_created.lock().unwrap().is_empty());
    // `independent` really ran, not merely planned to.
    assert!(tc.independent_created.lock().unwrap().contains("dev"));

    // NodeStarted/NodeFinished pairs exist for the blocked gate and the
    // skipped node -- the walk did not stop before reaching them.
    let mut saw_gate_finished = false;
    let mut saw_downstream_finished = false;
    for event in &observer.events {
        if let ApplyEvent::NodeFinished { node, status, .. } = event {
            if node.as_str() == "gate" {
                assert!(matches!(status, NodeStatus::Blocked));
                saw_gate_finished = true;
            }
            if node.as_str() == "downstream" {
                assert!(matches!(status, NodeStatus::Skipped));
                saw_downstream_finished = true;
            }
        }
    }
    assert!(saw_gate_finished && saw_downstream_finished);
}

/// The full resume cycle acceptance 14 asks for: blocked, then (once the
/// gate opens) a second `apply` against a *fresh* plan runs the
/// previously-skipped node and converges, and a third reads every node
/// `Unchanged`/`Computed`.
#[test]
fn a_second_apply_after_the_gate_opens_runs_the_skipped_node_and_a_third_converges() {
    let tc = build_catalog(false);
    let workflow = workflow(false);
    let checked = check(&workflow, &tc.catalog).expect("checks cleanly");
    let inputs = key_inputs();

    // Run 1: blocked.
    let plan1 = plan(&checked, &inputs, &tc.catalog).unwrap();
    let mut obs1 = RecordingObserver::new();
    let applied1 = apply(
        &checked,
        &inputs,
        &tc.catalog,
        &plan1,
        &Approval::Auto,
        &mut obs1,
    )
    .expect("blocked run is Ok");
    assert!(matches!(
        applied_status(&applied1, "downstream"),
        NodeStatus::Skipped
    ));
    assert_eq!(applied1.blocked.len(), 1);

    // The gate opens; a fresh plan is required (the plan's own actions
    // changed), which is what "re-run this document" means in practice.
    *tc.open.lock().unwrap() = true;
    let plan2 = plan(&checked, &inputs, &tc.catalog).unwrap();
    assert!(plan2.blocked.is_empty());
    let mut obs2 = RecordingObserver::new();
    let applied2 = apply(
        &checked,
        &inputs,
        &tc.catalog,
        &plan2,
        &Approval::Auto,
        &mut obs2,
    )
    .expect("the resumed run succeeds");
    assert!(applied2.blocked.is_empty());
    assert!(matches!(
        applied_status(&applied2, "gate"),
        NodeStatus::Computed
    ));
    assert!(matches!(
        applied_status(&applied2, "downstream"),
        NodeStatus::Created
    ));
    assert!(matches!(
        applied_status(&applied2, "independent"),
        NodeStatus::Unchanged
    ));

    // A third apply, re-planned again, converges everything.
    let plan3 = plan(&checked, &inputs, &tc.catalog).unwrap();
    let mut obs3 = RecordingObserver::new();
    let applied3 = apply(
        &checked,
        &inputs,
        &tc.catalog,
        &plan3,
        &Approval::Auto,
        &mut obs3,
    )
    .expect("the third run converges");
    assert!(matches!(
        applied_status(&applied3, "gate"),
        NodeStatus::Computed
    ));
    assert!(matches!(
        applied_status(&applied3, "downstream"),
        NodeStatus::Unchanged
    ));
    assert!(matches!(
        applied_status(&applied3, "independent"),
        NodeStatus::Unchanged
    ));
}

/// A gate that flips between the approved plan and `apply`'s own re-plan
/// refuses as `Action` drift -- whichever direction it flips, since
/// `apply` never trusts a stale plan's classification of a gate.
#[test]
fn a_gate_flipped_between_plan_and_apply_refuses_as_action_drift() {
    let tc = build_catalog(false);
    let workflow = workflow(false);
    let checked = check(&workflow, &tc.catalog).unwrap();
    let inputs = key_inputs();

    let approved = plan(&checked, &inputs, &tc.catalog).unwrap();
    assert_eq!(
        approved
            .nodes
            .iter()
            .find(|n| n.name.as_str() == "gate")
            .unwrap()
            .action,
        Action::Blocked
    );

    // The gate opens after approval but before apply: the approved plan no
    // longer describes what would actually happen.
    *tc.open.lock().unwrap() = true;
    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &tc.catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .unwrap_err();
    match err {
        ApplyError::Drift { node, kind, .. } => {
            assert_eq!(node.as_str(), "gate");
            assert!(matches!(
                *kind,
                willikins_core::DriftKind::Action {
                    planned: Action::Blocked,
                    observed: Action::Compute,
                }
            ));
        }
        other => panic!("expected Drift, got {other:?}"),
    }
    // Drift is caught before the run's own SinkToken is minted (rule 2):
    // nothing was attempted.
    assert!(observer.events.is_empty());
}

/// A tool failure elsewhere in the same run still stops the walk with a
/// `NotRun` tail and `ApplyError::Tool`, and the partial `Applied` it
/// carries keeps the earlier `Blocked`/`Skipped` statuses intact rather
/// than overwriting them.
#[test]
fn a_tool_failure_still_stops_the_walk_leaving_earlier_blocked_and_skipped_statuses() {
    let tc = build_catalog(true);
    let workflow = workflow(true);
    let checked = check(&workflow, &tc.catalog).expect("checks cleanly");
    let inputs = key_inputs();
    let approved = plan(&checked, &inputs, &tc.catalog).unwrap();

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &tc.catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .unwrap_err();

    let ApplyError::Tool { node, applied, .. } = err else {
        panic!("expected ApplyError::Tool");
    };
    assert_eq!(node.as_str(), "failing");

    let status_of = |name: &str| {
        applied
            .nodes
            .iter()
            .find(|n| n.name.as_str() == name)
            .unwrap_or_else(|| panic!("no node `{name}` in the partial result"))
            .status
            .clone()
    };
    assert!(matches!(status_of("gate"), NodeStatus::Blocked));
    assert!(matches!(status_of("downstream"), NodeStatus::Skipped));
    assert!(matches!(status_of("failing"), NodeStatus::Failed { .. }));
    assert!(matches!(status_of("independent"), NodeStatus::NotRun));
}
