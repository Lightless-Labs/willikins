//! Acceptance test 12 (G1, decision (j)): gates in `plan`.
//!
//! Small, in-test tools only — no provider, no fake catalog — because the
//! mechanism under test (`Catalog::insert`'s gate validation, the skip set
//! `plan` decides from bindings, `NodeResult::Skipped`,
//! `aggregate_for_each_port`'s "any instance blocked" rule, and
//! `Plan::blocked`) belongs to `willikins-core` itself, independent of any
//! real provider. `solo` and `consumer` share one [`CountingTool`] instance
//! (`test.counted`), so a test asserting a node is `Skip` can also assert
//! its shared read counter stayed at zero — the same graph is reused by a
//! scenario where those very nodes legitimately must run, so the tool
//! itself has to behave, not merely refuse to be called.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use crate::common::{input, node, output, port, tool_name, ty, workflow_name};
use willikins_core::{
    Action, Binding, Catalog, CatalogError, Class, Ensured, Gate, GateError, InputSpec, Inputs,
    Node, Observation, Outputs, PartialInputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec, Uses, Value, Workflow, check, describe, link, plan,
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

/// A pure gate over one `EnvironmentSlug` port, passed through as its own
/// output (decision (j), point 2: "a gate passes through the key it
/// checked"). `Present` for every key in `present`, `Absent` otherwise —
/// shared so one workflow can mix blocked and satisfied instances of the
/// same tool under one `for_each`.
struct GateTool {
    spec: ToolSpec,
    present: Arc<Mutex<HashSet<String>>>,
}

impl GateTool {
    fn new(present: Arc<Mutex<HashSet<String>>>) -> Self {
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
            present,
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
        outputs.insert(port("key"), key.clone());
        if self
            .present
            .lock()
            .unwrap()
            .contains(&key.render().to_string())
        {
            Ok(Observation::Present(outputs))
        } else {
            Ok(Observation::Absent { predicted: outputs })
        }
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        unreachable!("this test never applies a plan")
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&GATE)
    }
}

/// A non-pure, keyless tool that counts every call to `read` and otherwise
/// behaves normally (always `Absent`, passing its input straight through).
/// Shared by both `solo` and `consumer` in [`workflow`]: in the "blocked"
/// scenario, both are forced `Skip` and this tool's shared counter must
/// stay zero; in the "satisfied" scenario, both run for real, so the same
/// tool must behave correctly rather than merely never being called.
/// (`plan` never applies anything, so `ensure` is never exercised here.)
struct CountingTool {
    spec: ToolSpec,
    reads: Arc<Mutex<u32>>,
}

impl CountingTool {
    fn new(name: &str, reads: Arc<Mutex<u32>>) -> Self {
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
                description: "Test tool: counts its own `read` calls.".to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
            reads,
        }
    }
}

impl Tool for CountingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        *self.reads.lock().unwrap() += 1;
        let mut outputs = Outputs::new();
        outputs.insert(
            port("value"),
            inputs.get(&port("value")).expect("always bound").clone(),
        );
        Ok(Observation::Absent { predicted: outputs })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        unreachable!("this test never applies a plan")
    }
}

/// Milestone 3g, decision (a): the same shape as [`CountingTool`], except
/// its one input port is `list<EnvironmentSlug>` -- so a test can bind it
/// with a [`Binding::List`] whose element names a blocked gate, and prove
/// the skip scan (decision (j), point 3) covers list elements the same way
/// it covers a plain `Step`/`Keyed` binding.
struct CountingListTool {
    spec: ToolSpec,
    reads: Arc<Mutex<u32>>,
}

impl CountingListTool {
    fn new(name: &str, reads: Arc<Mutex<u32>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("values"),
            PortSpec {
                ty: PortType::Exact(willikins_core::TypeRef::list_of(
                    willikins_core::TypeName::parse("EnvironmentSlug").unwrap(),
                )),
                required: true,
                derived_only: false,
            },
        );
        Self {
            spec: ToolSpec {
                name: tool_name(name),
                description: "Test tool: counts its own `read` calls, one list<T> port."
                    .to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
            reads,
        }
    }
}

impl Tool for CountingListTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        *self.reads.lock().unwrap() += 1;
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        unreachable!("this test never applies a plan")
    }
}

/// A normal, well-behaved non-pure tool: always `Absent`, passing its input
/// straight through as its output. Used for a node a test expects to plan
/// normally (not skipped) alongside a blocked gate elsewhere in the graph.
struct PassthroughTool {
    spec: ToolSpec,
}

impl PassthroughTool {
    fn new(name: &str) -> Self {
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
                description: "Test tool: always Absent, passes its input through.".to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
        }
    }
}

impl Tool for PassthroughTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let mut outputs = Outputs::new();
        outputs.insert(
            port("value"),
            inputs.get(&port("value")).expect("always bound").clone(),
        );
        Ok(Observation::Absent { predicted: outputs })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        unreachable!("this test never applies a plan")
    }
}

// ---------------------------------------------------------------------
// The shared graph
// ---------------------------------------------------------------------

/// `envs = [dev, stg]`; `gate` is `for_each` over it (`test.gate`, `present`
/// controls which instances read `Present`); `solo` and `solo_ok` are
/// `Keyed` onto `gate`'s `dev` and `stg` instances respectively; `consumer`
/// is a `for_each` node whose *source* aggregates every instance of `gate`
/// (`Step`); `independent` binds nothing of `gate`'s. `solo` and `consumer`
/// are both `test.counted` (one shared read counter); `solo_ok` and
/// `independent` are the well-behaved `PassthroughTool`.
fn workflow() -> Workflow {
    Workflow::new(workflow_name("gate-test"))
        .input(
            input("envs"),
            InputSpec::new(willikins_core::TypeRef::list_of(
                willikins_core::TypeName::parse("EnvironmentSlug").unwrap(),
            ))
            .with_default(Value::known_list(vec![
                EnvironmentSlug::parse("dev").unwrap(),
                EnvironmentSlug::parse("stg").unwrap(),
            ])),
        )
        .node(
            node("gate"),
            Node::new(tool_name("test.gate"))
                .for_each(Binding::Input(input("envs")))
                .port(port("key"), Binding::Item),
        )
        .node(
            node("solo"),
            Node::new(tool_name("test.counted")).port(
                port("value"),
                Binding::Keyed {
                    node: node("gate"),
                    key: "dev".to_string(),
                    port: port("key"),
                },
            ),
        )
        .node(
            node("solo_ok"),
            Node::new(tool_name("test.passthrough_ok")).port(
                port("value"),
                Binding::Keyed {
                    node: node("gate"),
                    key: "stg".to_string(),
                    port: port("key"),
                },
            ),
        )
        .node(
            node("consumer"),
            Node::new(tool_name("test.counted"))
                .for_each(Binding::Step {
                    node: node("gate"),
                    port: port("key"),
                })
                .port(port("value"), Binding::Item),
        )
        .node(
            node("independent"),
            Node::new(tool_name("test.passthrough_independent"))
                .port(port("value"), Binding::Literal("prd".to_string())),
        )
        // Proves the skip rule holds transitively: `downstream` never binds
        // `gate` at all, only `consumer` (itself skipped only because of
        // `gate`) -- `Keyed` rather than `Step`, so its own `value` port
        // stays a scalar `EnvironmentSlug` in both scenarios (the key
        // "dev" is one of `consumer`'s own instance keys once expanded,
        // since a `for_each` node's instances key by the *source* item it
        // aggregated -- here, `gate`'s own `dev`/`stg` values passed
        // straight through).
        .node(
            node("downstream"),
            Node::new(tool_name("test.counted")).port(
                port("value"),
                Binding::Keyed {
                    node: node("consumer"),
                    key: "dev".to_string(),
                    port: port("value"),
                },
            ),
        )
        .output(
            output("gate_keys"),
            Binding::Step {
                node: node("gate"),
                port: port("key"),
            },
        )
}

/// Builds a fresh catalog for [`workflow`] and the shared `test.counted`
/// read counter (starts at zero), so a caller can assert it stayed zero
/// after a `plan` that expects `solo` and `consumer` to be skipped.
fn catalog_with(present: HashSet<&str>) -> (Catalog, Arc<Mutex<u32>>) {
    let mut catalog = Catalog::new(willikins_types::registry());
    let present: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(
        present.into_iter().map(str::to_string).collect(),
    ));
    catalog.insert(Arc::new(GateTool::new(present))).unwrap();
    let reads = Arc::new(Mutex::new(0));
    catalog
        .insert(Arc::new(CountingTool::new("test.counted", reads.clone())))
        .unwrap();
    catalog
        .insert(Arc::new(PassthroughTool::new("test.passthrough_ok")))
        .unwrap();
    catalog
        .insert(Arc::new(PassthroughTool::new(
            "test.passthrough_independent",
        )))
        .unwrap();
    (catalog, reads)
}

/// Like [`catalog_with`], but its counted tool (`test.counted_list`) takes
/// one `list<EnvironmentSlug>` port instead of a scalar one, for the
/// list-binding skip-scan test.
fn catalog_with_list(present: HashSet<&str>) -> (Catalog, Arc<Mutex<u32>>) {
    let mut catalog = Catalog::new(willikins_types::registry());
    let present: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(
        present.into_iter().map(str::to_string).collect(),
    ));
    catalog.insert(Arc::new(GateTool::new(present))).unwrap();
    let reads = Arc::new(Mutex::new(0));
    catalog
        .insert(Arc::new(CountingListTool::new(
            "test.counted_list",
            reads.clone(),
        )))
        .unwrap();
    (catalog, reads)
}

/// `plan` resolves a `Binding::Input` only from the caller's own supplied
/// map — applying a workflow input's declared default is `describe`'s job,
/// not `plan`'s (see `tests/plan.rs`'s own `new_rust_service_inputs`) — so
/// every test here supplies `envs` explicitly rather than relying on
/// `workflow()`'s declared default ever being read.
fn envs_inputs() -> IndexMap<willikins_core::InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("envs"),
        Value::known_list(vec![
            EnvironmentSlug::parse("dev").unwrap(),
            EnvironmentSlug::parse("stg").unwrap(),
        ]),
    );
    inputs
}

fn by_name<'a>(
    nodes: &'a [willikins_core::PlannedNode],
    name: &str,
    instance: Option<&str>,
) -> &'a willikins_core::PlannedNode {
    nodes
        .iter()
        .find(|n| n.name.as_str() == name && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("no planned node `{name}` (instance {instance:?})"))
}

// ---------------------------------------------------------------------
// Acceptance test 12
// ---------------------------------------------------------------------

/// With `dev` blocked and `stg` satisfied: `gate[dev]` is `Blocked`,
/// `gate[stg]` is `Compute`; `solo` (`Keyed` onto `dev`) and `consumer`
/// (`Step`-aggregating `gate`, which includes the blocked `dev`) are both
/// `Skip`, and `test.counted`'s shared read counter — which `solo` and
/// `consumer` are the only nodes bound to — stays at zero, proving neither
/// was ever read; `solo_ok` (`Keyed` onto the satisfied `stg`) and
/// `independent` (binds nothing of `gate`'s) plan exactly as they would
/// with no gate at all.
#[test]
fn a_blocked_gate_skips_its_dependents_and_leaves_every_other_branch_alone() {
    let (catalog, reads) = catalog_with(HashSet::from(["stg"]));
    let workflow = workflow();
    let checked = check(&workflow, &catalog).expect("the test graph checks cleanly");
    let inputs = envs_inputs();
    let result = plan(&checked, &inputs, &catalog).expect("a blocked gate never fails `plan`");

    assert_eq!(
        by_name(&result.nodes, "gate", Some("dev")).action,
        Action::Blocked
    );
    assert_eq!(
        by_name(&result.nodes, "gate", Some("stg")).action,
        Action::Compute
    );

    assert_eq!(by_name(&result.nodes, "solo", None).action, Action::Skip);
    assert!(
        by_name(&result.nodes, "solo", None).inputs.is_empty(),
        "a skipped node is left unbound"
    );
    let consumer_instances: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| n.name.as_str() == "consumer")
        .collect();
    assert_eq!(
        consumer_instances.len(),
        1,
        "a for_each node whose source is blocked plans as exactly one instance"
    );
    assert_eq!(consumer_instances[0].action, Action::Skip);
    assert_eq!(consumer_instances[0].instance, None);

    // `downstream` binds nothing of `gate`'s directly -- only `consumer`,
    // itself skipped only because of `gate` -- proving the skip rule holds
    // transitively, one hop beyond a direct reference.
    assert_eq!(
        by_name(&result.nodes, "downstream", None).action,
        Action::Skip
    );

    // `solo`, `consumer`, and `downstream` are the only nodes bound to
    // `test.counted`, so a zero count here is a hard proof none of them was
    // ever read, not merely an inference from their `Action`.
    assert_eq!(
        *reads.lock().unwrap(),
        0,
        "a skipped node's tool must never be read"
    );

    // Not skipped: these ran through the real tool.
    assert_eq!(
        by_name(&result.nodes, "solo_ok", None).action,
        Action::Create
    );
    assert_eq!(
        by_name(&result.nodes, "independent", None).action,
        Action::Create
    );

    // The workflow output aggregating the mixed gate is Unknown, not a
    // known two-element list.
    let gate_keys = result.outputs.get(&output("gate_keys")).unwrap();
    assert!(
        !gate_keys.is_known(),
        "an aggregate over a blocked instance must be Unknown"
    );

    // `Plan.blocked` names exactly the one blocked instance, with its
    // static need/how, its rendered subject, and every node it holds back.
    assert_eq!(result.blocked.len(), 1);
    let entry = &result.blocked[0];
    assert_eq!(entry.node.as_str(), "gate");
    assert_eq!(entry.instance.as_deref(), Some("dev"));
    assert_eq!(entry.tool.as_str(), "test.gate");
    assert_eq!(entry.need, GATE.need);
    assert_eq!(entry.how, GATE.how);
    assert_eq!(entry.subject.len(), 1);
    assert_eq!(entry.subject[0].0.as_str(), "key");
    assert_eq!(entry.subject[0].1, "dev");
    let holds_back: HashSet<&str> = entry
        .holds_back
        .iter()
        .map(willikins_core::NodeName::as_str)
        .collect();
    assert_eq!(
        holds_back,
        HashSet::from(["solo", "consumer", "downstream"])
    );

    // No secret can appear here (the catalog would have refused the gate
    // otherwise), and the rendered subject is plain text, not a marker.
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"dev\""));
    assert!(!json.contains("REDACTED"));
}

/// A scalar gate (no `for_each`) held by one `Step` binding: the
/// dependent is `Skip`, never read, and named in the gate's `holds_back`.
/// `workflow()` above only ever blocks a `for_each` instance, and its
/// transitive `downstream` binds by `Keyed`, so without this test the
/// whole-node `Step` arm of the skip rule was pinned by `apply_gates.rs`
/// alone (2026-09-29 adversarial pass, mutation m1: disabling that arm
/// left this file green).
#[test]
fn a_step_binding_on_a_blocked_scalar_gate_is_skipped_and_never_read() {
    let (catalog, reads) = catalog_with(HashSet::new());
    let workflow = Workflow::new(workflow_name("scalar-gate-test"))
        .node(
            node("gate"),
            Node::new(tool_name("test.gate"))
                .port(port("key"), Binding::Literal("dev".to_string())),
        )
        .node(
            node("dependent"),
            Node::new(tool_name("test.counted")).port(
                port("value"),
                Binding::Step {
                    node: node("gate"),
                    port: port("key"),
                },
            ),
        )
        .node(
            node("independent"),
            Node::new(tool_name("test.passthrough_independent"))
                .port(port("value"), Binding::Literal("prd".to_string())),
        );
    let checked = check(&workflow, &catalog).expect("the test graph checks cleanly");
    let result = plan(&checked, &IndexMap::new(), &catalog).expect("a blocked gate never fails");

    assert_eq!(by_name(&result.nodes, "gate", None).action, Action::Blocked);
    assert_eq!(
        by_name(&result.nodes, "dependent", None).action,
        Action::Skip
    );
    assert!(by_name(&result.nodes, "dependent", None).inputs.is_empty());
    assert_eq!(*reads.lock().unwrap(), 0, "a skipped node is never read");
    assert_eq!(
        by_name(&result.nodes, "independent", None).action,
        Action::Create
    );
    assert_eq!(result.blocked.len(), 1);
    assert_eq!(result.blocked[0].instance, None);
    assert_eq!(
        result.blocked[0]
            .holds_back
            .iter()
            .map(willikins_core::NodeName::as_str)
            .collect::<Vec<_>>(),
        vec!["dependent"]
    );
}

/// Milestone 3g, decision (a): the skip scan covers a `Binding::List`
/// element the same way it covers a plain `Step`/`Keyed` binding -- a node
/// with a list-bound port naming a blocked gate anywhere among its
/// elements plans `Action::Skip` as a whole and its `read` is never
/// called, proven by the shared counter staying at zero.
#[test]
fn a_list_element_binding_on_a_blocked_gate_is_skipped_and_never_read() {
    let (catalog, reads) = catalog_with_list(HashSet::new());
    let workflow = Workflow::new(workflow_name("list-element-gate-test"))
        .node(
            node("gate"),
            Node::new(tool_name("test.gate"))
                .port(port("key"), Binding::Literal("dev".to_string())),
        )
        .node(
            node("dependent"),
            Node::new(tool_name("test.counted_list")).port(
                port("values"),
                Binding::List(vec![Binding::Step {
                    node: node("gate"),
                    port: port("key"),
                }]),
            ),
        );
    let checked = check(&workflow, &catalog).expect("the test graph checks cleanly");
    let result = plan(&checked, &IndexMap::new(), &catalog).expect("a blocked gate never fails");

    assert_eq!(by_name(&result.nodes, "gate", None).action, Action::Blocked);
    assert_eq!(
        by_name(&result.nodes, "dependent", None).action,
        Action::Skip
    );
    assert_eq!(
        *reads.lock().unwrap(),
        0,
        "a node skipped because of its list element is never read"
    );
    assert_eq!(result.blocked.len(), 1);
    assert_eq!(
        result.blocked[0]
            .holds_back
            .iter()
            .map(willikins_core::NodeName::as_str)
            .collect::<Vec<_>>(),
        vec!["dependent"]
    );
}

/// With every instance satisfied: every node plans `Compute`/`Create`
/// exactly as it would with no gate at all, `Plan.blocked` is empty, and
/// the plan's JSON carries no `blocked` key whatsoever.
#[test]
fn a_satisfied_gate_blocks_nothing_and_the_plan_json_has_no_blocked_key() {
    let (catalog, _reads) = catalog_with(HashSet::from(["dev", "stg"]));
    let workflow = workflow();
    let checked = check(&workflow, &catalog).unwrap();
    let inputs = envs_inputs();
    let result = plan(&checked, &inputs, &catalog).unwrap();

    assert_eq!(
        by_name(&result.nodes, "gate", Some("dev")).action,
        Action::Compute
    );
    assert_eq!(
        by_name(&result.nodes, "gate", Some("stg")).action,
        Action::Compute
    );
    assert_eq!(by_name(&result.nodes, "solo", None).action, Action::Create);
    assert_eq!(
        by_name(&result.nodes, "solo_ok", None).action,
        Action::Create
    );
    assert_eq!(
        result
            .nodes
            .iter()
            .filter(|n| n.name.as_str() == "consumer")
            .count(),
        2,
        "an unblocked for_each source expands normally"
    );
    assert_eq!(
        by_name(&result.nodes, "independent", None).action,
        Action::Create
    );
    assert_eq!(
        by_name(&result.nodes, "downstream", None).action,
        Action::Create,
        "a Keyed reference to a satisfied for_each node's own instance resolves normally"
    );
    assert!(result.blocked.is_empty());

    let json = serde_json::to_value(&result).unwrap();
    assert!(
        json.as_object().unwrap().get("blocked").is_none(),
        "an empty `blocked` must not serialize at all: {json}"
    );
}

// ---------------------------------------------------------------------
// Catalog::insert's gate validation
// ---------------------------------------------------------------------

static BAD_SUBJECT_GATE: Gate = Gate {
    need: "n",
    how: "h",
    subject: &["nonexistent"],
};

static ANY_SECRET_SUBJECT_GATE: Gate = Gate {
    need: "n",
    how: "h",
    subject: &["secret_port"],
};

static SECRET_TYPED_SUBJECT_GATE: Gate = Gate {
    need: "n",
    how: "h",
    subject: &["secret_value"],
};

static OK_GATE: Gate = Gate {
    need: "n",
    how: "h",
    subject: &["key"],
};

/// A pure tool declaring `gate`, whose one input port's shape is
/// configurable, so each refusal test can build exactly the offending
/// shape without a new struct per case.
struct ConfigurableGateTool {
    spec: ToolSpec,
    gate: &'static Gate,
}

impl Tool for ConfigurableGateTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Present(Outputs::new()))
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        unreachable!("insert-time validation never calls a tool's own methods")
    }

    fn gate(&self) -> Option<&Gate> {
        Some(self.gate)
    }
}

fn spec_with(inputs: IndexMap<willikins_core::PortName, PortSpec>, pure: bool) -> ToolSpec {
    ToolSpec {
        name: tool_name("test.configurable_gate"),
        description: "Test tool for gate validation.".to_string(),
        inputs,
        outputs: IndexMap::new(),
        key: Vec::new(),
        // Always Reversible: the `NotPure` test needs a spec that would
        // otherwise validate cleanly (`ToolSpec::validate` only constrains
        // `class` when `pure` is true), so the gate refusal under test is
        // never confused with a `PureToolNotReversible` spec error.
        class: Class::Reversible,
        pure,
    }
}

#[test]
fn insert_refuses_a_gate_whose_tool_is_not_pure() {
    let mut inputs = IndexMap::new();
    inputs.insert(
        port("key"),
        PortSpec {
            ty: PortType::Exact(ty("EnvironmentSlug")),
            required: true,
            derived_only: false,
        },
    );
    let mut catalog = Catalog::new(willikins_types::registry());
    let err = catalog
        .insert(Arc::new(ConfigurableGateTool {
            spec: spec_with(inputs, false),
            gate: &OK_GATE,
        }))
        .unwrap_err();
    assert!(matches!(
        err,
        CatalogError::Gate {
            error: GateError::NotPure,
            ..
        }
    ));
}

#[test]
fn insert_refuses_a_gate_subject_that_is_not_one_of_the_tools_own_inputs() {
    let mut inputs = IndexMap::new();
    inputs.insert(
        port("key"),
        PortSpec {
            ty: PortType::Exact(ty("EnvironmentSlug")),
            required: true,
            derived_only: false,
        },
    );
    let mut catalog = Catalog::new(willikins_types::registry());
    let err = catalog
        .insert(Arc::new(ConfigurableGateTool {
            spec: spec_with(inputs, true),
            gate: &BAD_SUBJECT_GATE,
        }))
        .unwrap_err();
    assert!(matches!(
        err,
        CatalogError::Gate {
            error: GateError::SubjectNotAnInput { .. },
            ..
        }
    ));
}

#[test]
fn insert_refuses_a_gate_subject_of_an_any_secret_port() {
    let mut inputs = IndexMap::new();
    inputs.insert(
        port("secret_port"),
        PortSpec {
            ty: PortType::AnySecret,
            required: true,
            derived_only: false,
        },
    );
    let mut catalog = Catalog::new(willikins_types::registry());
    let err = catalog
        .insert(Arc::new(ConfigurableGateTool {
            spec: spec_with(inputs, true),
            gate: &ANY_SECRET_SUBJECT_GATE,
        }))
        .unwrap_err();
    assert!(matches!(
        err,
        CatalogError::Gate {
            error: GateError::SubjectNotExact { .. },
            ..
        }
    ));
}

#[test]
fn insert_refuses_a_gate_subject_of_a_secret_type() {
    let mut inputs = IndexMap::new();
    inputs.insert(
        port("secret_value"),
        PortSpec {
            ty: PortType::Exact(ty("DopplerSecretValue")),
            required: true,
            derived_only: false,
        },
    );
    let mut catalog = Catalog::new(willikins_types::registry());
    let err = catalog
        .insert(Arc::new(ConfigurableGateTool {
            spec: spec_with(inputs, true),
            gate: &SECRET_TYPED_SUBJECT_GATE,
        }))
        .unwrap_err();
    assert!(matches!(
        err,
        CatalogError::Gate {
            error: GateError::SubjectSecret { .. },
            ..
        }
    ));
    // Sanity: `DopplerSecretValue` really is registered secret, so this
    // test is pinning the refusal this crate cares about, not an
    // unregistered-type accident.
    assert_eq!(
        willikins_types::registry()
            .is_secret(&willikins_core::TypeName::parse("DopplerSecretValue").unwrap()),
        Some(true)
    );
}

#[test]
fn insert_accepts_a_well_formed_gate() {
    let mut inputs = IndexMap::new();
    inputs.insert(
        port("key"),
        PortSpec {
            ty: PortType::Exact(ty("EnvironmentSlug")),
            required: true,
            derived_only: false,
        },
    );
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(ConfigurableGateTool {
            spec: spec_with(inputs, true),
            gate: &OK_GATE,
        }))
        .unwrap();
}

// ---------------------------------------------------------------------
// G3: operator acknowledgement. `plan`'s `Binding::Input` arm and
// `GateTracking::mark_blocked`'s `awaiting_inputs`, exercised end to end
// through a small in-test gate tool -- the mechanism `operator.acknowledge`
// (willikins-tools, commit 2) will be the one production user of.
// ---------------------------------------------------------------------

static ACK_GATE: Gate = Gate {
    need: "the operator has done the test's manual step",
    how: "do the step, then supply the awaited input",
    subject: &["step"],
};

/// A pure gate over `step: Text` (the subject, passed through as its own
/// output) and `acknowledged: OperatorAcknowledgement`. `Present` iff
/// `acknowledged` is known; `Absent` when it is
/// [`willikins_core::Value::unknown`] -- never reads it through
/// `helpers::get`, which would fail on an `Unknown` value instead of
/// reporting the gate unmet. Models `operator.acknowledge`'s own `read`.
struct AckGateTool {
    spec: ToolSpec,
}

impl AckGateTool {
    fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("step"),
            PortSpec {
                ty: PortType::Exact(ty("Text")),
                required: true,
                derived_only: false,
            },
        );
        inputs.insert(
            port("acknowledged"),
            PortSpec {
                ty: PortType::Exact(ty("OperatorAcknowledgement")),
                required: true,
                derived_only: false,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("step"), ty("Text"));
        Self {
            spec: ToolSpec {
                name: tool_name("test.acknowledge"),
                description: "Test gate over one OperatorAcknowledgement.".to_string(),
                inputs,
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
        }
    }
}

impl Tool for AckGateTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let step = inputs
            .get(&port("step"))
            .expect("test always binds `step`")
            .clone();
        let mut outputs = Outputs::new();
        outputs.insert(port("step"), step);
        let acknowledged_known = inputs
            .get(&port("acknowledged"))
            .is_some_and(Value::is_known);
        if acknowledged_known {
            Ok(Observation::Present(outputs))
        } else {
            Ok(Observation::Absent { predicted: outputs })
        }
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        unreachable!("this test never applies a plan")
    }

    fn gate(&self) -> Option<&Gate> {
        Some(&ACK_GATE)
    }
}

/// A workflow with one declared input (`ack_done: OperatorAcknowledgement`,
/// no default) and one node, `gate`, binding `step` to a literal and
/// `acknowledged` to that input.
fn ack_workflow() -> Workflow {
    Workflow::new(workflow_name("w"))
        .input(
            input("ack_done"),
            InputSpec::new(ty("OperatorAcknowledgement")),
        )
        .node(
            node("gate"),
            Node::new(tool_name("test.acknowledge"))
                .port(port("step"), Binding::Literal("do the thing".to_string()))
                .port(port("acknowledged"), Binding::Input(input("ack_done"))),
        )
}

fn ack_catalog() -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog.insert(Arc::new(AckGateTool::new())).unwrap();
    catalog
}

/// Acceptance 16's core plan-time claim: an `OperatorAcknowledgement`
/// input the caller never supplied is absent from the resolved inputs map
/// (`describe` put it under `awaiting`, never `resolved`) yet `plan` still
/// succeeds -- `Binding::Input` resolves it to `Value::unknown` rather than
/// `PlanError::MissingInput` -- and the gate reads `Absent`, so the node is
/// `Action::Blocked` and names the awaited input in `awaiting_inputs`.
#[test]
fn an_unsupplied_acknowledgement_input_blocks_the_gate_and_is_named_in_awaiting_inputs() {
    let workflow = ack_workflow();
    let catalog = ack_catalog();
    let checked = check(&workflow, &catalog).expect("the test graph checks cleanly");

    // No entry for `ack_done` at all -- exactly what `describe` leaves
    // behind for an awaited input (never added to the resolved map).
    let inputs: IndexMap<willikins_core::InputName, Value> = IndexMap::new();
    let result = plan(&checked, &inputs, &catalog)
        .expect("an unsupplied OperatorAcknowledgement input must never fail `plan`");

    assert_eq!(by_name(&result.nodes, "gate", None).action, Action::Blocked);
    assert_eq!(result.blocked.len(), 1);
    let entry = &result.blocked[0];
    assert_eq!(entry.tool.as_str(), "test.acknowledge");
    assert_eq!(entry.subject[0].0.as_str(), "step");
    assert_eq!(entry.subject[0].1, "do the thing");
    assert_eq!(
        entry.awaiting_inputs,
        vec![input("ack_done")],
        "the report must name exactly the input a --input flag would supply"
    );

    // The caller's own map is untouched: `plan` never adds the unsupplied
    // input to it.
    assert!(!inputs.contains_key(&input("ack_done")));
}

/// Supplying `done` makes the gate `Compute`, `Plan.blocked` empty, and
/// `awaiting_inputs` moot -- the acknowledgement flows exactly like any
/// other known input.
#[test]
fn supplying_done_makes_the_acknowledgement_gate_compute() {
    let workflow = ack_workflow();
    let catalog = ack_catalog();
    let checked = check(&workflow, &catalog).expect("the test graph checks cleanly");

    let mut inputs: IndexMap<willikins_core::InputName, Value> = IndexMap::new();
    inputs.insert(
        input("ack_done"),
        Value::known(willikins_types::OperatorAcknowledgement::parse("done").unwrap()),
    );
    let result = plan(&checked, &inputs, &catalog).expect("a supplied acknowledgement plans fine");

    assert_eq!(by_name(&result.nodes, "gate", None).action, Action::Compute);
    assert!(result.blocked.is_empty());
}

// ---------------------------------------------------------------------
// R1 (milestone 2b, decision (d6), "Acknowledgements"): after
// substitution, `BlockedGate.awaiting_inputs` names the root's own input,
// never the child's, since an `OperatorAcknowledgement` input can never
// carry a default (`AcknowledgementDefault`) and so is always bound by
// the parent -- exercised end to end through the linker, not merely
// asserted.
// ---------------------------------------------------------------------

/// A one-node child document: the same `test.acknowledge` gate as
/// [`ack_workflow`], but declaring its own input (`ack`, no default --
/// `check` refuses one, and this document is never checked on its own
/// here anyway) rather than a root's.
fn ack_child_workflow() -> Workflow {
    Workflow::new(workflow_name("ack-child"))
        .input(input("ack"), InputSpec::new(ty("OperatorAcknowledgement")))
        .node(
            node("gate"),
            Node::new(tool_name("test.acknowledge"))
                .port(port("step"), Binding::Literal("do the thing".to_string()))
                .port(port("acknowledged"), Binding::Input(input("ack"))),
        )
}

/// A root with one declared input (`m_done`, the same shape as
/// [`ack_workflow`]'s own `ack_done`) and one `uses:` step, `org`, binding
/// the child's `ack` input straight to it -- decision (d6)'s normal case,
/// "normally to the parent's own input".
fn ack_composite_root() -> Workflow {
    let mut with = IndexMap::new();
    with.insert(input("ack"), Binding::Input(input("m_done")));
    Workflow::new(workflow_name("ack-composite-root"))
        .input(
            input("m_done"),
            InputSpec::new(ty("OperatorAcknowledgement")),
        )
        .uses(
            node("org"),
            Uses {
                workflow: workflow_name("ack-child"),
                with,
                position: 0,
            },
        )
}

/// Acceptance 7: a child gate whose acknowledgement input is bound to the
/// root's input `m_done` plans `Blocked` with `awaiting_inputs ==
/// [m_done]` -- the root's own name, not the child's `ack`, because
/// linking substitutes the binding before `plan` ever sees the node.
#[test]
fn acceptance_7_a_linked_acknowledgement_gate_awaits_the_roots_own_input() {
    let mut resolve = |name: &willikins_types::WorkflowName| match name.as_str() {
        "ack-child" => Ok(ack_child_workflow()),
        other => panic!("resolver asked for an unexpected workflow: {other}"),
    };
    let linked = link(&ack_composite_root(), &mut resolve).expect("a well-formed composite links");
    assert_eq!(
        linked
            .workflow
            .inputs
            .keys()
            .map(willikins_core::InputName::as_str)
            .collect::<Vec<_>>(),
        vec!["m_done"],
        "the child's `ack` input is bound via `with:`, never exposed as its own fixed input"
    );

    let catalog = ack_catalog();
    let checked = check(&linked.workflow, &catalog).expect("the linked graph checks cleanly");

    // Empty partial: `m_done` is `awaiting`, never `resolved` -- the same
    // shape `describe` leaves an unsupplied acknowledgement input in at
    // the root (acceptance 16), now reached through a linked composite.
    let description = describe(&checked, &PartialInputs::new());
    assert_eq!(description.awaiting.len(), 1);
    assert_eq!(description.awaiting[0].name, input("m_done"));
    assert!(!description.resolved.contains_key(&input("m_done")));

    let result = plan(&checked, &description.resolved, &catalog)
        .expect("an unsupplied OperatorAcknowledgement input must never fail `plan`");

    assert_eq!(
        by_name(&result.nodes, "org/gate", None).action,
        Action::Blocked
    );
    assert_eq!(result.blocked.len(), 1);
    assert_eq!(
        result.blocked[0].awaiting_inputs,
        vec![input("m_done")],
        "the report must name the root's own input, the one a --input flag would supply"
    );
}
