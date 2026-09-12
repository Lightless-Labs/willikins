//! Plan a [`Checked`] workflow against a [`Catalog`]: walk
//! [`Checked::order`], expand every `for_each` node into one instance per
//! source item, call [`Tool::read`] for each instance, and classify the
//! resulting action.
//!
//! # `for_each` aggregation
//!
//! A [`Binding::Step`] onto a `for_each` node's port aggregates every one
//! of that node's instances into a list, in source-list order: known when
//! every instance's value at that port is known, [`crate::value::ValueState::Unknown`]
//! (of the same `list<T>` type) the moment any instance's is not. A
//! [`Binding::Keyed`] instead picks the one instance whose item's canonical
//! string equals the given key, failing with [`PlanError::KeyNotInForEach`]
//! naming the *referencing* node, not the `for_each` node itself, when no
//! instance matches — this is what acceptance test 7 pins.
//!
//! Because that canonical string is an instance's only identity — to a
//! `Keyed` reference, and in [`PlannedNode::instance`] — a source list
//! holding two items that render to the same string is refused outright
//! with [`PlanError::DuplicateForEachKey`], before any of the node's
//! instances is read, rather than silently planning two indistinguishable
//! instances and letting `Keyed` pick the first.
//!
//! # Every declared output reaches the plan
//!
//! [`crate::check::check`] refuses a literal workflow output binding
//! (`outputs: { x: some-raw-string }`) with
//! [`crate::CheckError::LiteralOutput`], because there is no port to parse
//! a literal against outside a tool and so no honest type to record. Every
//! output a [`Checked`] workflow declares therefore has an entry in
//! [`Checked::output_types`], and `plan` resolves every one of them into
//! [`Plan::outputs`]: nothing a document declares is silently dropped
//! between `check` and `plan`. Adversarial pass 2, finding 1, pinned by
//! `willikins-cli/tests/adversarial.rs`.
//!
//! # `PlanError` is one error, not a list
//!
//! Unlike `check`, `plan` stops at the first problem it finds while
//! walking `Checked::order`: there is no cascade-suppression story to tell,
//! since planning one node can depend on another node's own plan already
//! having succeeded.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;

use crate::catalog::Catalog;
use crate::check::Checked;
use crate::class::Class;
use crate::tool::{Inputs, Observation, Outputs, PortName, Tool, ToolError, ToolName, ToolSpec};
use crate::value::{PortType, TypeName, TypeRef, Value};
use crate::workflow::{Binding, InputName, Node, NodeName, OutputName, Workflow};

/// What `plan` decided to do for one node instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// A pure tool computed its outputs; there is no external state to
    /// create or leave alone, whatever [`Observation`] its `read` reported.
    Compute,
    /// The resource does not exist yet: `ensure` would create it.
    Create,
    /// The resource already exists and is ours: `ensure` would be a no-op.
    NoOp,
}

/// One planned call to a tool: a whole node with no `for_each`, or one
/// instance of a `for_each` node.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct PlannedNode {
    /// The node this instance belongs to.
    pub name: NodeName,
    /// For a `for_each` node, the current instance's item rendered as its
    /// canonical string — the same string a [`Binding::Keyed`] matches
    /// against. `None` for a node with no `for_each`.
    pub instance: Option<String>,
    /// The tool this node calls.
    pub tool: ToolName,
    /// What `plan` decided to do for this instance.
    pub action: Action,
    /// The values bound to this instance's input ports.
    pub inputs: Inputs,
    /// Every one of this tool's declared output ports: the predicted or
    /// observed value where the tool supplied one, [`Value::unknown`]
    /// otherwise.
    pub outputs: Outputs,
}

/// A concrete, approval-classified plan.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct Plan {
    /// The planned workflow's name.
    pub workflow: String,
    /// Every planned node, in [`Checked::order`]; a `for_each` node
    /// contributes one entry per instance, in its source list's order.
    pub nodes: Vec<PlannedNode>,
    /// Every workflow output's resolved value. A literal output binding has
    /// no entry here; see the module docs.
    pub outputs: IndexMap<OutputName, Value>,
    /// The plan's approval class: [`Checked::class`], a static property of
    /// the checked workflow's tools, unaffected by anything `plan`
    /// observes at runtime.
    pub class: Class,
    /// Whether this plan should require human approval before running:
    /// `class.requires_approval()`.
    pub requires_approval: bool,
}

/// Why [`plan`] could not produce a [`Plan`].
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`) as
/// `{"kind": "<Variant>", ...the variant's own fields}`: every variant is
/// struct-like, so the tag merges cleanly. No variant declares a field
/// named `kind` or `message`; see `tests/plan_error_serde.rs`, which checks
/// this at run time the same way `check.rs`'s own unit tests do for
/// [`crate::CheckError`] and [`crate::CheckWarning`].
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum PlanError {
    /// A `Binding::Input` named a workflow input that was not in the
    /// supplied resolved-inputs map.
    MissingInput {
        /// The missing input's name.
        input: InputName,
    },
    /// A `for_each` node's source resolved to
    /// [`crate::value::ValueState::Unknown`], so it cannot be expanded into
    /// instances.
    ForEachUnknown {
        /// The node whose `for_each` source is unknown.
        node: NodeName,
    },
    /// Two of a `for_each` node's items rendered to the same canonical
    /// string, so its instances would not be distinguishable: a
    /// [`Binding::Keyed`] reference could not say which one it means, and
    /// two [`PlannedNode`]s would share a `(name, instance)` pair. Reported
    /// before any of the node's instances is read.
    DuplicateForEachKey {
        /// The `for_each` node whose items collide.
        node: NodeName,
        /// The canonical string two of its items share.
        key: String,
    },
    /// A `Binding::Keyed` reference named a key none of the referenced
    /// `for_each` node's instances have.
    KeyNotInForEach {
        /// The node whose binding contains the keyed reference — the
        /// referencing node, not the `for_each` node it points at.
        node: NodeName,
        /// The key that matched no instance.
        key: String,
    },
    /// One of a tool's key ports resolved to
    /// [`crate::value::ValueState::Unknown`], so its resource cannot be
    /// looked up. Never produced for a pure tool: a pure tool's key is
    /// always empty, so this loop never finds anything to check.
    KeyUnknown {
        /// The node whose key port is unknown.
        node: NodeName,
        /// The unknown key port.
        port: PortName,
    },
    /// A resource already exists at this node's natural key, but it was
    /// not created by this tool: [`Observation::Foreign`].
    NameTaken {
        /// The node whose resource is foreign.
        node: NodeName,
        /// The tool that reported it.
        tool: ToolName,
        /// The bound values at this node's key ports only — never the full
        /// input set, so a value bound to a non-key port never appears
        /// here.
        key: Inputs,
    },
    /// A tool's `read` itself failed.
    Tool {
        /// The node whose tool failed.
        node: NodeName,
        /// The failure it reported.
        error: ToolError,
    },
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingInput { input } => {
                write!(f, "workflow input `{input}` was not supplied")
            }
            Self::ForEachUnknown { node } => {
                write!(f, "node `{node}`: for_each source is unknown")
            }
            Self::DuplicateForEachKey { node, key } => write!(
                f,
                "node `{node}`: two for_each items are both keyed `{key}`"
            ),
            Self::KeyNotInForEach { node, key } => {
                write!(f, "node `{node}`: no for_each instance is keyed `{key}`")
            }
            Self::KeyUnknown { node, port } => {
                write!(f, "node `{node}`, port `{port}`: key port is unknown")
            }
            Self::NameTaken { node, tool, .. } => write!(
                f,
                "node `{node}` (tool `{tool}`): a resource already exists at this name and is not ours"
            ),
            Self::Tool { node, error } => write!(f, "node `{node}`: {error}"),
        }
    }
}

impl std::error::Error for PlanError {}

/// One instance of a `for_each` node: its item's canonical string (the key
/// a [`Binding::Keyed`] matches against) and its planned outputs.
struct ForEachInstance {
    key: String,
    outputs: Outputs,
}

/// What a node resolved to, once planned: a single set of outputs for a
/// node with no `for_each`, or one set per instance for one that has it.
enum NodeResult {
    /// A node with no `for_each`.
    Scalar(Outputs),
    /// A `for_each` node's instances, in source-list order.
    ForEach(Vec<ForEachInstance>),
}

/// The read-only context every binding resolution needs: the workflow (for
/// its node and input declarations), the caller's resolved workflow
/// inputs, the catalog (to re-derive a referenced node's tool spec), and
/// every already-planned node's result.
struct ResolveCtx<'a> {
    workflow: &'a Workflow,
    inputs: &'a IndexMap<InputName, Value>,
    catalog: &'a Catalog,
    results: &'a HashMap<NodeName, NodeResult>,
}

/// The synthetic node name used to report a workflow output binding's own
/// [`PlanError::KeyNotInForEach`], mirroring `check`'s own `"outputs"`
/// sentinel for the same reason: an output is not itself a node.
fn outputs_node() -> NodeName {
    NodeName::parse("outputs").unwrap_or_else(|err| unreachable!("`outputs` is a NodeName: {err}"))
}

/// Plan `checked` against `catalog`, resolving its workflow inputs from
/// `inputs`.
///
/// Walks `checked.order`; for each node, resolves its `for_each` source (if
/// any) and every instance's `with` bindings, checks the tool's key ports
/// are known, calls [`Tool::read`], and records one [`PlannedNode`] per
/// instance. Workflow outputs are resolved last, against every node's
/// finished result.
///
/// # Errors
///
/// Returns the first [`PlanError`] found while walking the workflow; see
/// the module docs for why this is one error, not a list.
///
/// # Panics
///
/// Panics if `catalog` does not contain a tool `checked` was itself checked
/// against, or if a node, port, or output name `checked` resolved is not
/// actually present in its own workflow — both would mean `checked` was not
/// produced by [`crate::check::check`] against a catalog compatible with
/// `catalog`, which is a caller contract violation, not a value `plan`
/// deals with normally.
pub fn plan(
    checked: &Checked,
    inputs: &IndexMap<InputName, Value>,
    catalog: &Catalog,
) -> Result<Plan, PlanError> {
    let workflow = &checked.workflow;
    let mut results: HashMap<NodeName, NodeResult> = HashMap::new();
    let mut planned: Vec<PlannedNode> = Vec::new();

    for name in &checked.order {
        let node = workflow
            .nodes
            .get(name)
            .unwrap_or_else(|| unreachable!("`checked.order` lists only workflow nodes"));
        let tool = catalog
            .get(&node.tool)
            .unwrap_or_else(|| unreachable!("`checked` was checked against a compatible catalog"));
        let spec = tool.spec();
        let ctx = ResolveCtx {
            workflow,
            inputs,
            catalog,
            results: &results,
        };

        let result = match &node.for_each {
            None => {
                let bound = bind_ports(&ctx, name, node, spec, None)?;
                let node_plan = plan_one(name, None, spec, tool.as_ref(), bound)?;
                let outputs = node_plan.outputs.clone();
                planned.push(node_plan);
                NodeResult::Scalar(outputs)
            }
            Some(source) => {
                let source_value = resolve_binding(&ctx, name, source, None)?;
                let Some(items) = source_value.as_list() else {
                    return Err(PlanError::ForEachUnknown { node: name.clone() });
                };
                // Key every item up front and refuse a collision before
                // reading anything: two instances sharing a key would be
                // indistinguishable both to a `Keyed` reference and in the
                // finished plan.
                let keyed: Vec<(Value, String)> = items
                    .iter()
                    .map(|object| {
                        let value = Value::known_dyn(Arc::clone(object));
                        let key = value.render().to_string();
                        (value, key)
                    })
                    .collect();
                let mut seen: HashSet<&str> = HashSet::with_capacity(keyed.len());
                for (_, key) in &keyed {
                    if !seen.insert(key.as_str()) {
                        return Err(PlanError::DuplicateForEachKey {
                            node: name.clone(),
                            key: key.clone(),
                        });
                    }
                }
                let mut instances = Vec::with_capacity(keyed.len());
                for (item_value, key) in keyed {
                    let bound = bind_ports(&ctx, name, node, spec, Some(&item_value))?;
                    let node_plan = plan_one(name, Some(key.clone()), spec, tool.as_ref(), bound)?;
                    instances.push(ForEachInstance {
                        key,
                        outputs: node_plan.outputs.clone(),
                    });
                    planned.push(node_plan);
                }
                NodeResult::ForEach(instances)
            }
        };
        results.insert(name.clone(), result);
    }

    let mut outputs = IndexMap::new();
    let ctx = ResolveCtx {
        workflow,
        inputs,
        catalog,
        results: &results,
    };
    for (out_name, binding) in &workflow.outputs {
        // A literal output has no declared type to resolve against; see
        // the module docs' "Known gap" section.
        if matches!(binding, Binding::Literal(_)) {
            continue;
        }
        let value = resolve_binding(&ctx, &outputs_node(), binding, None)?;
        outputs.insert(out_name.clone(), value);
    }

    Ok(Plan {
        workflow: workflow.name.clone(),
        nodes: planned,
        outputs,
        class: checked.class,
        requires_approval: checked.class.requires_approval(),
    })
}

/// Resolve every one of `spec`'s input ports for `node` into an [`Inputs`]
/// map: a [`Binding::Literal`] is parsed against the port's own scalar type
/// (already validated by `check`, so this can never fail); anything else
/// goes through [`resolve_binding`]. A port `check` did not require and
/// `node` did not bind is simply absent from the result.
fn bind_ports(
    ctx: &ResolveCtx,
    node_name: &NodeName,
    node: &Node,
    spec: &ToolSpec,
    item: Option<&Value>,
) -> Result<Inputs, PlanError> {
    let mut inputs = Inputs::new();
    for port in spec.inputs.keys() {
        let Some(binding) = node.with.get(port) else {
            continue;
        };
        let value = if let Binding::Literal(text) = binding {
            let port_spec = &spec.inputs[port];
            let PortType::Exact(ty) = &port_spec.ty else {
                unreachable!("`check` rejects a literal bound to an AnySecret port");
            };
            Value::parse(ty, text).unwrap_or_else(|err| {
                unreachable!("`check` already validated this literal against its port type: {err}")
            })
        } else {
            resolve_binding(ctx, node_name, binding, item)?
        };
        inputs.insert(port.clone(), value);
    }
    Ok(inputs)
}

/// Resolve one non-literal binding to its [`Value`].
///
/// `site_node` is only used to attribute [`PlanError::KeyNotInForEach`] to
/// the node whose binding contains the keyed reference, never to the
/// `for_each` node it points at.
fn resolve_binding(
    ctx: &ResolveCtx,
    site_node: &NodeName,
    binding: &Binding,
    item: Option<&Value>,
) -> Result<Value, PlanError> {
    match binding {
        Binding::Literal(_) => unreachable!(
            "callers resolve a Literal directly, with the port's expected type in hand"
        ),
        Binding::Item => Ok(item.cloned().unwrap_or_else(|| {
            unreachable!("`check` rejects `item` used outside a for_each node")
        })),
        Binding::Input(name) => {
            ctx.inputs
                .get(name)
                .cloned()
                .ok_or_else(|| PlanError::MissingInput {
                    input: name.clone(),
                })
        }
        Binding::Step { node, port } => Ok(resolve_step(ctx, node, port)),
        Binding::Keyed { node, key, port } => resolve_keyed(ctx, site_node, node, key, port),
    }
}

/// Resolve a `Step` reference to `node`'s `port`: that node's own output
/// value when it has no `for_each`, or the aggregation across every
/// instance described in the module docs when it does.
fn resolve_step(ctx: &ResolveCtx, node: &NodeName, port: &PortName) -> Value {
    match ctx
        .results
        .get(node)
        .unwrap_or_else(|| unreachable!("`checked.order` plans every node before its dependents"))
    {
        NodeResult::Scalar(outputs) => outputs
            .get(port)
            .cloned()
            .unwrap_or_else(|| unreachable!("`check` validated that this output port exists")),
        NodeResult::ForEach(instances) => aggregate_for_each_port(ctx, node, port, instances),
    }
}

/// Aggregate every instance of a `for_each` node's `port` into one list
/// value: known when every instance's value there is known, unknown (at
/// `list<element>`) the moment one is not.
fn aggregate_for_each_port(
    ctx: &ResolveCtx,
    node: &NodeName,
    port: &PortName,
    instances: &[ForEachInstance],
) -> Value {
    let target = ctx
        .workflow
        .nodes
        .get(node)
        .unwrap_or_else(|| unreachable!("referenced node exists per `check`"));
    let target_spec = ctx
        .catalog
        .get(&target.tool)
        .unwrap_or_else(|| unreachable!("`checked` was checked against a compatible catalog"))
        .spec();
    let element: TypeName = target_spec
        .outputs
        .get(port)
        .unwrap_or_else(|| unreachable!("`check` validated that this output port exists"))
        .name
        .clone();

    let mut items = Vec::with_capacity(instances.len());
    for instance in instances {
        let value = instance
            .outputs
            .get(port)
            .unwrap_or_else(|| unreachable!("every planned instance fills every output port"));
        match value.as_scalar_arc() {
            Some(object) => items.push(object),
            None => return Value::unknown(TypeRef::list_of(element)),
        }
    }
    Value::known_dyn_list(element, items)
}

/// Resolve a `Keyed` reference: the instance of `target`'s `for_each` node
/// whose item renders to `key`, or [`PlanError::KeyNotInForEach`] attributed
/// to `site_node` when none matches.
fn resolve_keyed(
    ctx: &ResolveCtx,
    site_node: &NodeName,
    target: &NodeName,
    key: &str,
    port: &PortName,
) -> Result<Value, PlanError> {
    let NodeResult::ForEach(instances) = ctx
        .results
        .get(target)
        .unwrap_or_else(|| unreachable!("`checked.order` plans every node before its dependents"))
    else {
        unreachable!("`check` rejects a Keyed reference to a node with no for_each");
    };
    instances
        .iter()
        .find(|instance| instance.key == key)
        .map(|instance| {
            instance
                .outputs
                .get(port)
                .cloned()
                .unwrap_or_else(|| unreachable!("`check` validated that this output port exists"))
        })
        .ok_or_else(|| PlanError::KeyNotInForEach {
            node: site_node.clone(),
            key: key.to_string(),
        })
}

/// Plan one call to `tool`: check its key ports are known, call
/// [`Tool::read`], and classify the resulting [`Action`].
fn plan_one(
    name: &NodeName,
    instance: Option<String>,
    spec: &ToolSpec,
    tool: &dyn Tool,
    inputs: Inputs,
) -> Result<PlannedNode, PlanError> {
    for key_port in &spec.key {
        if !inputs.get(key_port).is_some_and(Value::is_known) {
            return Err(PlanError::KeyUnknown {
                node: name.clone(),
                port: key_port.clone(),
            });
        }
    }

    let observation = tool.read(&inputs).map_err(|error| PlanError::Tool {
        node: name.clone(),
        error,
    })?;

    if matches!(observation, Observation::Foreign) {
        return Err(PlanError::NameTaken {
            node: name.clone(),
            tool: spec.name.clone(),
            key: restrict_to_key(&inputs, &spec.key),
        });
    }

    let action = if spec.pure {
        Action::Compute
    } else if matches!(observation, Observation::Absent { .. }) {
        Action::Create
    } else {
        Action::NoOp
    };

    let outputs = match &observation {
        Observation::Absent { predicted } => fill_outputs(spec, predicted),
        Observation::Present(present) => fill_outputs(spec, present),
        Observation::Foreign => unreachable!("handled above"),
    };

    Ok(PlannedNode {
        name: name.clone(),
        instance,
        tool: spec.name.clone(),
        action,
        inputs,
        outputs,
    })
}

/// `inputs` restricted to `key`'s ports only, in `key`'s own order.
fn restrict_to_key(inputs: &Inputs, key: &[PortName]) -> Inputs {
    let mut restricted = Inputs::new();
    for port in key {
        if let Some(value) = inputs.get(port) {
            restricted.insert(port.clone(), value.clone());
        }
    }
    restricted
}

/// Every one of `spec`'s declared output ports: `provided`'s value where it
/// has one, [`Value::unknown`] otherwise.
fn fill_outputs(spec: &ToolSpec, provided: &Outputs) -> Outputs {
    let mut outputs = Outputs::new();
    for (port, ty) in &spec.outputs {
        let value = provided
            .get(port)
            .cloned()
            .unwrap_or_else(|| Value::unknown(ty.clone()));
        outputs.insert(port.clone(), value);
    }
    outputs
}
