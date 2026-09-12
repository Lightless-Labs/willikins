//! Static validation of a [`Workflow`] against a [`Catalog`].
//!
//! [`check`] is the single entry point: it returns every error it finds, in
//! a deterministic order, or a [`Checked`] workflow ready for `describe`
//! and `plan` (milestone 1 task 8).
//!
//! # Error ordering
//!
//! Errors accumulate in this fixed sequence, so a workflow with many
//! problems reports them the same way every run:
//!
//! 1. Workflow input errors, one per declared input, in declaration
//!    order: [`CheckError::SecretWorkflowInput`] for a secret declared
//!    type, or else [`CheckError::DefaultTypeMismatch`] for a default
//!    value whose type is not the declared one.
//! 2. [`CheckError::UnknownTool`], one per node with an unrecognised tool,
//!    in node declaration order. A node whose tool is unknown is skipped
//!    for every later step (its ports cannot be checked against a spec
//!    that does not exist).
//! 3. For every node whose tool *is* known, in node declaration order:
//!    its `for_each` binding's errors, then its input ports in the order
//!    the tool spec declares them (an unbound required port, or a bound
//!    port's own errors), then any `with` keys that are not one of the
//!    tool's ports, in `with` declaration order.
//! 4. Workflow outputs' errors, in declaration order.
//! 5. [`CheckError::Cycle`], one per cyclic strongly-connected component,
//!    ordered by the lowest declaration index among its nodes.
//!
//! # Cascade suppression
//!
//! Once a binding is already broken for one reason (its source node's tool
//! is unknown, a `for_each` failed to resolve, a self-reference), later
//! stages resolve it to `None` and report nothing further for it, so one
//! root cause does not multiply into a wall of errors. See `resolve` and
//! `ItemContext` below.
//!
//! # Known gaps (plan defects; not fixed here — task 7 implements the plan
//! as written)
//!
//! - [`CheckError::DuplicateNode`] can never be produced by [`check`]
//!   itself: [`Workflow::nodes`](crate::workflow::Workflow::nodes) is an
//!   `IndexMap`, which cannot hold two entries with the same key by
//!   construction. The variant exists for the future DSL parser (task 10),
//!   which will detect a duplicate node name in a YAML document *before*
//!   it collapses into the map, and report it through this same error
//!   type. Covered here by a single test that constructs the variant
//!   directly and checks its `Display`.
//! - A workflow output and the synthetic `"outputs"` node label used to
//!   report an output binding's own errors collide if a real node happens
//!   to be named `outputs`; likewise `for_each`'s synthetic port label
//!   collides with a real port named `for_each`. Neither collision changes
//!   which check fires, only which node/port name an error report for the
//!   `outputs` pseudo-scope shares with a real, identically named node.
//! - An unregistered scalar type named in an [`InputSpec`](crate::workflow::InputSpec)
//!   is treated as non-secret (the registry has no secrecy answer for it);
//!   the DSL is expected to reject an unregistered type name at load time,
//!   before `check` ever sees it.
//! - `Step` on a `for_each` node whose own output port is *already*
//!   list-typed has no `list<list<T>>` representation to promote it to;
//!   the resolved type keeps `list: true` rather than doubling up. No
//!   milestone 1 tool has a list-typed output, so this never triggers here.
//! - The plan lists sixteen [`CheckError`] variants. One more was added by
//!   the adversarial pass, because the plan has no variant for the defect
//!   it names: [`CheckError::DefaultTypeMismatch`], an input's default
//!   value that is not of its declared type. The plan type-checks defaults
//!   at document load, which leaves a `Workflow` built any other way
//!   unchecked.

use std::collections::HashSet;
use std::fmt;

use indexmap::IndexMap;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::catalog::Catalog;
use crate::class::Class;
use crate::tool::{PortName, PortSpec, ToolName, ToolSpec};
use crate::value::{PortType, TypeRef, TypeRegistry, Value};
use crate::workflow::{Binding, InputName, Node, NodeName, Workflow};
use willikins_types::ParseError;

/// A workflow that has passed [`check`]: its topological execution order,
/// its approval class, any non-fatal warnings, and the resolved type of
/// every binding `check` looked at.
#[derive(Debug, Clone)]
pub struct Checked {
    /// The checked workflow, unchanged.
    pub workflow: Workflow,
    /// Every node, in an order where each node comes after every node it
    /// depends on. Ties are broken by declaration order.
    pub order: Vec<NodeName>,
    /// The plan's approval class: the maximum over every non-pure node's
    /// tool class, or [`Class::Reversible`] when there are none.
    pub class: Class,
    /// Non-fatal warnings, alongside a successful check.
    pub warnings: Vec<CheckWarning>,
    /// The resolved type of every binding `check` validated, keyed by node
    /// then port. Covers every node's `with` entries and every workflow
    /// output (the latter under the synthetic node name `"outputs"`, see
    /// the module docs). Does not cover `for_each` bindings.
    pub types: IndexMap<NodeName, IndexMap<PortName, TypeRef>>,
}

/// A non-fatal observation returned alongside a successful [`check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckWarning {
    /// A declared workflow input that no binding anywhere references.
    UnusedInput {
        /// The unused input's name.
        input: InputName,
    },
}

impl fmt::Display for CheckWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnusedInput { input } => {
                write!(f, "input `{input}` is declared but never used")
            }
        }
    }
}

/// Everything [`check`] can find wrong with a [`Workflow`].
///
/// Every variant names the node (and, where relevant, the port) the
/// problem concerns; see the module docs for exactly which node identifies
/// which side of a two-node reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// A node's `tool` names a tool the catalog does not have.
    UnknownTool {
        /// The node whose tool is unrecognised.
        node: NodeName,
        /// The unrecognised tool name.
        tool: ToolName,
    },
    /// A port name that does not exist on the tool it is checked against.
    ///
    /// Two situations share this variant: a `with` key that is not one of
    /// `node`'s own tool's input ports (`node` is the binding's own node);
    /// and a `Step` or `Keyed` binding naming an output port that does not
    /// exist on the node it references (`node` is then the *referenced*
    /// node, since the port that is missing is one of its outputs).
    UnknownPort {
        /// The node the missing port was looked up on.
        node: NodeName,
        /// That node's tool.
        tool: ToolName,
        /// The port name that does not exist.
        port: PortName,
    },
    /// A `Step`, `Keyed`, or `for_each` binding names a node that is not
    /// in the workflow.
    UnknownNode {
        /// The node whose binding contains the bad reference.
        node: NodeName,
        /// The port (or the synthetic `for_each` port) holding the binding.
        port: PortName,
        /// The node name it referenced, which does not exist.
        referenced: NodeName,
    },
    /// A tool's required input port has no binding.
    UnboundInput {
        /// The node with the unbound port.
        node: NodeName,
        /// The unbound required port.
        port: PortName,
    },
    /// A `Binding::Input` names a workflow input that is not declared.
    UndeclaredInput {
        /// The node whose binding references the undeclared input.
        node: NodeName,
        /// The port holding the binding.
        port: PortName,
        /// The undeclared input name.
        input: InputName,
    },
    /// A `Binding::Literal` failed to parse against its port's scalar
    /// type, or named a list-typed port (which no literal can supply).
    InvalidLiteral {
        /// The node whose binding is the bad literal.
        node: NodeName,
        /// The port holding the binding.
        port: PortName,
        /// Why the literal was rejected.
        error: ParseError,
    },
    /// A binding resolved to a type its port does not accept.
    TypeMismatch {
        /// The node whose binding does not type-check.
        node: NodeName,
        /// The port holding the binding.
        port: PortName,
        /// The port's declared type.
        expected: PortType,
        /// The binding's resolved type.
        found: TypeRef,
    },
    /// A `Binding::Literal` bound to a port whose type is secret or
    /// [`PortType::AnySecret`]. Reported before the literal is ever handed
    /// to a parser.
    SecretLiteral {
        /// The node whose binding is the literal.
        node: NodeName,
        /// The secret-accepting port it was bound to.
        port: PortName,
    },
    /// A secret value, read from another node's output, was bound to a
    /// port that does not accept secrets. Takes precedence over
    /// [`Self::TypeMismatch`] for the same edge.
    SecretToNonSecretSink {
        /// The node and output port the secret value came from.
        from: (NodeName, PortName),
        /// The node and input port it was bound to.
        to: (NodeName, PortName),
    },
    /// A workflow input's declared type is secret. A secret value must
    /// never enter a workflow this way; it can only be produced by a tool.
    SecretWorkflowInput {
        /// The offending input.
        input: InputName,
        /// Its secret type.
        ty: TypeRef,
    },
    /// A `for_each` binding resolved to a secret list.
    SecretForEachSource {
        /// The node whose `for_each` source is secret.
        node: NodeName,
    },
    /// A `for_each` binding resolved to a scalar (non-list) type.
    ForEachOverScalar {
        /// The node whose `for_each` source is not a list.
        node: NodeName,
    },
    /// A `Binding::Item` was used outside any `for_each` node.
    ItemOutsideForEach {
        /// The node the binding was found in.
        node: NodeName,
        /// The port (or synthetic port) holding the binding.
        port: PortName,
    },
    /// A `Binding::Keyed` referenced a node that has no `for_each`.
    KeyedOnScalarNode {
        /// The node whose binding contains the keyed reference.
        node: NodeName,
        /// The port holding the binding.
        port: PortName,
        /// The referenced node, which has no `for_each`.
        referenced: NodeName,
    },
    /// A dependency cycle. One variant covers a single self-referencing
    /// node (`nodes` has one entry) and a multi-node cycle alike.
    Cycle {
        /// The nodes on the cycle, in declaration order.
        nodes: Vec<NodeName>,
    },
    /// A workflow input's default value is not of the input's declared
    /// type, in name or in cardinality.
    ///
    /// Not one of the plan's sixteen variants: the plan parses a default
    /// against its declared type when a *document* is loaded, which leaves
    /// a [`Workflow`] built any other way — the builder API, a future
    /// composite — free to declare `Text` and default to a
    /// `DopplerServiceToken`. `check` resolves an input binding from the
    /// declared type, so without this the secret default would be the
    /// value a later stage actually pushed into a non-secret sink.
    DefaultTypeMismatch {
        /// The input whose default does not match its declared type.
        input: InputName,
        /// The input's declared type.
        expected: TypeRef,
        /// The default value's own type.
        found: TypeRef,
    },
    /// Two nodes share the same name.
    ///
    /// Never produced by [`check`] itself — see the "Known gaps" section
    /// of the module docs. Exists so the future DSL parser can report a
    /// duplicate `steps:` key through this same error type.
    DuplicateNode {
        /// The name two nodes shared.
        node: NodeName,
    },
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTool { node, tool } => {
                write!(f, "node `{node}`: unknown tool `{tool}`")
            }
            Self::UnknownPort { node, tool, port } => {
                write!(f, "node `{node}` (tool `{tool}`): no such port `{port}`")
            }
            Self::UnknownNode {
                node,
                port,
                referenced,
            } => write!(
                f,
                "node `{node}`, port `{port}`: references unknown node `{referenced}`"
            ),
            Self::UnboundInput { node, port } => {
                write!(
                    f,
                    "node `{node}`, port `{port}`: required input is not bound"
                )
            }
            Self::UndeclaredInput { node, port, input } => write!(
                f,
                "node `{node}`, port `{port}`: references undeclared input `{input}`"
            ),
            Self::InvalidLiteral { node, port, error } => {
                write!(f, "node `{node}`, port `{port}`: invalid literal: {error}")
            }
            Self::TypeMismatch {
                node,
                port,
                expected,
                found,
            } => write!(
                f,
                "node `{node}`, port `{port}`: expected {expected:?}, found `{found}`"
            ),
            Self::SecretLiteral { node, port } => write!(
                f,
                "node `{node}`, port `{port}`: a literal cannot supply a secret value"
            ),
            Self::SecretToNonSecretSink { from, to } => write!(
                f,
                "secret value from node `{}`, port `{}` flows into non-secret sink at node `{}`, port `{}`",
                from.0, from.1, to.0, to.1
            ),
            Self::SecretWorkflowInput { input, ty } => write!(
                f,
                "input `{input}` has secret type `{ty}`; a workflow input may not be secret"
            ),
            Self::SecretForEachSource { node } => {
                write!(f, "node `{node}`: for_each source is secret")
            }
            Self::ForEachOverScalar { node } => {
                write!(f, "node `{node}`: for_each source is not a list")
            }
            Self::ItemOutsideForEach { node, port } => write!(
                f,
                "node `{node}`, port `{port}`: `item` is only valid inside a for_each node"
            ),
            Self::KeyedOnScalarNode {
                node,
                port,
                referenced,
            } => write!(
                f,
                "node `{node}`, port `{port}`: keyed reference to node `{referenced}`, which has no for_each"
            ),
            Self::Cycle { nodes } => {
                write!(f, "cycle among nodes: ")?;
                for (index, node) in nodes.iter().enumerate() {
                    if index > 0 {
                        write!(f, " -> ")?;
                    }
                    write!(f, "`{node}`")?;
                }
                Ok(())
            }
            Self::DefaultTypeMismatch {
                input,
                expected,
                found,
            } => write!(
                f,
                "input `{input}`: default value has type `{found}`, expected `{expected}`"
            ),
            Self::DuplicateNode { node } => write!(f, "duplicate node name `{node}`"),
        }
    }
}

impl std::error::Error for CheckError {}

/// Validate `workflow` against `catalog`.
///
/// # Errors
///
/// Returns every [`CheckError`] found, in the order documented on the
/// module.
pub fn check(workflow: &Workflow, catalog: &Catalog) -> Result<Checked, Vec<CheckError>> {
    let registry = catalog.registry();
    let mut errors = Vec::new();

    check_workflow_inputs(workflow, registry, &mut errors);

    let (mut graph, index_of) = build_graph(workflow);
    let specs = resolve_tool_specs(workflow, catalog, &mut errors);

    let mut resolver = Resolver {
        workflow,
        specs: &specs,
        registry,
        graph: &mut graph,
        index_of: &index_of,
        used_inputs: HashSet::new(),
        types: IndexMap::new(),
    };

    for (name, node) in &workflow.nodes {
        let Some(Some(spec)) = specs.get(name) else {
            continue;
        };
        resolver.check_node(name, node, spec, &mut errors);
    }
    resolver.check_outputs(&mut errors);

    let Resolver {
        used_inputs, types, ..
    } = resolver;

    errors.extend(find_cycles(&graph, workflow));

    if !errors.is_empty() {
        return Err(errors);
    }

    let order = topo_order(workflow, &graph, &index_of);
    let class = compute_class(workflow, &specs);
    let warnings = collect_warnings(workflow, &used_inputs);

    Ok(Checked {
        workflow: workflow.clone(),
        order,
        class,
        warnings,
        types,
    })
}

/// Check every declared input: [`CheckError::SecretWorkflowInput`] when
/// its type is secret, or else [`CheckError::DefaultTypeMismatch`] when
/// its default value is not of the declared type. A secret declared type
/// is the root cause and suppresses the default check, which could only
/// repeat it.
fn check_workflow_inputs(
    workflow: &Workflow,
    registry: &TypeRegistry,
    errors: &mut Vec<CheckError>,
) {
    for (name, spec) in &workflow.inputs {
        if registry.is_secret(&spec.ty.name) == Some(true) {
            errors.push(CheckError::SecretWorkflowInput {
                input: name.clone(),
                ty: spec.ty.clone(),
            });
            continue;
        }
        if let Some(default) = &spec.default
            && default.ty() != &spec.ty
        {
            errors.push(CheckError::DefaultTypeMismatch {
                input: name.clone(),
                expected: spec.ty.clone(),
                found: default.ty().clone(),
            });
        }
    }
}

/// Build one graph node per workflow node, in declaration order.
fn build_graph(workflow: &Workflow) -> (DiGraph<NodeName, ()>, IndexMap<NodeName, NodeIndex>) {
    let mut graph = DiGraph::new();
    let mut index_of = IndexMap::new();
    for name in workflow.nodes.keys() {
        let idx = graph.add_node(name.clone());
        index_of.insert(name.clone(), idx);
    }
    (graph, index_of)
}

/// Look up every node's tool spec, pushing [`CheckError::UnknownTool`] for
/// any tool the catalog does not have.
fn resolve_tool_specs<'c>(
    workflow: &Workflow,
    catalog: &'c Catalog,
    errors: &mut Vec<CheckError>,
) -> IndexMap<NodeName, Option<&'c ToolSpec>> {
    let mut specs = IndexMap::new();
    for (name, node) in &workflow.nodes {
        if let Some(tool) = catalog.get(&node.tool) {
            specs.insert(name.clone(), Some(tool.spec()));
        } else {
            errors.push(CheckError::UnknownTool {
                node: name.clone(),
                tool: node.tool.clone(),
            });
            specs.insert(name.clone(), None);
        }
    }
    specs
}

/// The synthetic port name used to report a `for_each` binding's own
/// errors, since `for_each` is not one of a node's `with` ports.
fn for_each_port() -> PortName {
    PortName::parse("for_each")
        .unwrap_or_else(|err| unreachable!("`for_each` is a PortName: {err}"))
}

/// The synthetic node name used to report a workflow output binding's own
/// errors, since an output is not itself a node. See the module docs'
/// "Known gaps" section for the collision this admits.
fn outputs_node() -> NodeName {
    NodeName::parse("outputs").unwrap_or_else(|err| unreachable!("`outputs` is a NodeName: {err}"))
}

/// Whether `ty` is a secret type, per `registry`. An unregistered type
/// name is treated as non-secret; see the module docs' "Known gaps".
fn is_secret(ty: &TypeRef, registry: &TypeRegistry) -> bool {
    registry.is_secret(&ty.name) == Some(true)
}

/// Whether `port_ty` accepts a secret value at all: [`PortType::AnySecret`]
/// always does; [`PortType::Exact`] does only when its own type is secret.
fn port_accepts_secret(port_ty: &PortType, registry: &TypeRegistry) -> bool {
    match port_ty {
        PortType::AnySecret => true,
        PortType::Exact(ty) => registry.is_secret(&ty.name) == Some(true),
    }
}

/// What `Binding::Item` resolves to inside one node, decided once per node
/// before its `with` entries are checked.
enum ItemContext {
    /// The node has no `for_each`: `Item` is always an error here.
    NotInForEach,
    /// The node has a `for_each`, but it failed to resolve; `Item` is
    /// silently unresolvable (the root cause was already reported).
    ForEachBroken,
    /// The node's `for_each` resolved to a list of this element type.
    ForEachOf(TypeRef),
}

/// Resolves bindings and accumulates the side effects of doing so: graph
/// edges, used-input tracking, and resolved binding types. Grouped into one
/// struct so the resolution methods below stay under a handful of
/// parameters each.
struct Resolver<'a> {
    workflow: &'a Workflow,
    specs: &'a IndexMap<NodeName, Option<&'a ToolSpec>>,
    registry: &'a TypeRegistry,
    graph: &'a mut DiGraph<NodeName, ()>,
    index_of: &'a IndexMap<NodeName, NodeIndex>,
    used_inputs: HashSet<InputName>,
    types: IndexMap<NodeName, IndexMap<PortName, TypeRef>>,
}

impl<'a> Resolver<'a> {
    /// Record `ty` as the resolved type of `node`'s `port`.
    fn record_type(&mut self, node: &NodeName, port: &PortName, ty: TypeRef) {
        self.types
            .entry(node.clone())
            .or_default()
            .insert(port.clone(), ty);
    }

    /// Check one node: its `for_each` binding, every one of its tool's
    /// input ports, and any `with` key that is not one of them.
    fn check_node(
        &mut self,
        name: &NodeName,
        node: &Node,
        spec: &'a ToolSpec,
        errors: &mut Vec<CheckError>,
    ) {
        let item_ctx = self.check_for_each(name, node, errors);

        let mut seen: HashSet<PortName> = HashSet::new();
        for (port, port_spec) in &spec.inputs {
            seen.insert(port.clone());
            self.check_with_port(
                name,
                port,
                port_spec,
                node.with.get(port),
                &item_ctx,
                errors,
            );
        }
        for (port, binding) in &node.with {
            if !seen.contains(port) {
                errors.push(CheckError::UnknownPort {
                    node: name.clone(),
                    tool: node.tool.clone(),
                    port: port.clone(),
                });
                self.record_extra(binding, name);
            }
        }
    }

    /// Resolve `node`'s `for_each` binding, if any, pushing
    /// [`CheckError::ForEachOverScalar`] or [`CheckError::SecretForEachSource`]
    /// as needed.
    fn check_for_each(
        &mut self,
        name: &NodeName,
        node: &Node,
        errors: &mut Vec<CheckError>,
    ) -> ItemContext {
        let Some(binding) = &node.for_each else {
            return ItemContext::NotInForEach;
        };
        let port = for_each_port();
        if matches!(binding, Binding::Literal(_)) {
            errors.push(CheckError::ForEachOverScalar { node: name.clone() });
            return ItemContext::ForEachBroken;
        }

        let site_idx = self.index_of.get(name).copied();
        let not_in_for_each = ItemContext::NotInForEach;
        let Some((ty, _source)) =
            self.resolve(site_idx, name, &port, &not_in_for_each, binding, errors)
        else {
            return ItemContext::ForEachBroken;
        };
        if is_secret(&ty, self.registry) {
            errors.push(CheckError::SecretForEachSource { node: name.clone() });
            return ItemContext::ForEachBroken;
        }
        if !ty.list {
            errors.push(CheckError::ForEachOverScalar { node: name.clone() });
            return ItemContext::ForEachBroken;
        }
        ItemContext::ForEachOf(ty.element())
    }

    /// Check one `with` port: unbound-but-required, a literal, or a
    /// resolved binding checked for secrecy and type against `port_spec`.
    fn check_with_port(
        &mut self,
        node: &NodeName,
        port: &PortName,
        port_spec: &PortSpec,
        binding: Option<&Binding>,
        item_ctx: &ItemContext,
        errors: &mut Vec<CheckError>,
    ) {
        let Some(binding) = binding else {
            if port_spec.required {
                errors.push(CheckError::UnboundInput {
                    node: node.clone(),
                    port: port.clone(),
                });
            }
            return;
        };

        if let Binding::Literal(text) = binding {
            if let Some(found) =
                check_literal(node, port, text, &port_spec.ty, self.registry, errors)
            {
                self.record_type(node, port, found);
            }
            return;
        }

        let site_idx = self.index_of.get(node).copied();
        let Some((found, source)) = self.resolve(site_idx, node, port, item_ctx, binding, errors)
        else {
            return;
        };

        if is_secret(&found, self.registry) && !port_accepts_secret(&port_spec.ty, self.registry) {
            if let Some(from) = source {
                errors.push(CheckError::SecretToNonSecretSink {
                    from,
                    to: (node.clone(), port.clone()),
                });
            }
            return;
        }

        if port_spec.ty.accepts(&found, self.registry) {
            self.record_type(node, port, found);
        } else {
            errors.push(CheckError::TypeMismatch {
                node: node.clone(),
                port: port.clone(),
                expected: port_spec.ty.clone(),
                found,
            });
        }
    }

    /// Record the side effects of a `with` key that is not one of its
    /// node's ports, without emitting any further error for it: mark an
    /// `Input` as used, and add a graph edge for a `Step`/`Keyed` binding
    /// whose referenced node exists.
    fn record_extra(&mut self, binding: &Binding, site: &NodeName) {
        match binding {
            Binding::Input(name) => {
                self.used_inputs.insert(name.clone());
            }
            Binding::Step {
                node: referenced, ..
            }
            | Binding::Keyed {
                node: referenced, ..
            } => {
                if let (Some(&site_idx), Some(&ref_idx)) =
                    (self.index_of.get(site), self.index_of.get(referenced))
                {
                    self.graph.add_edge(ref_idx, site_idx, ());
                }
            }
            Binding::Item | Binding::Literal(_) => {}
        }
    }

    /// Check every workflow output, under the synthetic `"outputs"` node
    /// name. A literal output has no target type to check against, so it
    /// is accepted unconditionally.
    fn check_outputs(&mut self, errors: &mut Vec<CheckError>) {
        let workflow = self.workflow;
        let node = outputs_node();
        let not_in_for_each = ItemContext::NotInForEach;
        for (out_name, binding) in &workflow.outputs {
            if matches!(binding, Binding::Literal(_)) {
                continue;
            }
            let port = PortName::parse(out_name.as_str()).unwrap_or_else(|err| {
                unreachable!("OutputName and PortName share a pattern: {err}")
            });
            if let Some((found, _source)) =
                self.resolve(None, &node, &port, &not_in_for_each, binding, errors)
            {
                self.record_type(&node, &port, found);
            }
        }
    }

    /// Resolve any non-literal binding to its type, plus (for `Step` and
    /// `Keyed`) the `(node, port)` it came from, used only to attribute a
    /// [`CheckError::SecretToNonSecretSink`]. Returns `None` when the
    /// binding is invalid (an error was pushed) or already-broken
    /// (cascade-suppressed; no new error).
    fn resolve(
        &mut self,
        site_idx: Option<NodeIndex>,
        site_node: &NodeName,
        site_port: &PortName,
        item_ctx: &ItemContext,
        binding: &Binding,
        errors: &mut Vec<CheckError>,
    ) -> Option<(TypeRef, Option<(NodeName, PortName)>)> {
        match binding {
            Binding::Literal(_) => unreachable!(
                "callers resolve Literal via check_literal, which needs the port's expected type"
            ),
            Binding::Item => match item_ctx {
                ItemContext::NotInForEach => {
                    errors.push(CheckError::ItemOutsideForEach {
                        node: site_node.clone(),
                        port: site_port.clone(),
                    });
                    None
                }
                ItemContext::ForEachBroken => None,
                ItemContext::ForEachOf(ty) => Some((ty.clone(), None)),
            },
            Binding::Input(input) => {
                self.used_inputs.insert(input.clone());
                match self.workflow.inputs.get(input) {
                    None => {
                        errors.push(CheckError::UndeclaredInput {
                            node: site_node.clone(),
                            port: site_port.clone(),
                            input: input.clone(),
                        });
                        None
                    }
                    Some(spec) => {
                        if is_secret(&spec.ty, self.registry) {
                            None
                        } else {
                            Some((spec.ty.clone(), None))
                        }
                    }
                }
            }
            Binding::Step {
                node: referenced,
                port,
            } => self.resolve_reference(
                site_idx, site_node, site_port, referenced, false, port, errors,
            ),
            Binding::Keyed {
                node: referenced,
                port,
                ..
            } => self.resolve_reference(
                site_idx, site_node, site_port, referenced, true, port, errors,
            ),
        }
    }

    /// Resolve a `Step` (`keyed == false`) or `Keyed` (`keyed == true`)
    /// reference to `referenced`'s `port`, adding the graph edge whenever
    /// `referenced` exists (even if the reference later turns out invalid),
    /// and suppressing further checks on a self-reference (left for
    /// [`find_cycles`] to report).
    #[allow(clippy::too_many_arguments)]
    fn resolve_reference(
        &mut self,
        site_idx: Option<NodeIndex>,
        site_node: &NodeName,
        site_port: &PortName,
        referenced: &NodeName,
        keyed: bool,
        port: &PortName,
        errors: &mut Vec<CheckError>,
    ) -> Option<(TypeRef, Option<(NodeName, PortName)>)> {
        let Some(&ref_idx) = self.index_of.get(referenced) else {
            errors.push(CheckError::UnknownNode {
                node: site_node.clone(),
                port: site_port.clone(),
                referenced: referenced.clone(),
            });
            return None;
        };

        if let Some(site_idx) = site_idx {
            self.graph.add_edge(ref_idx, site_idx, ());
        }

        if referenced == site_node {
            return None;
        }

        let target_node = self.workflow.nodes.get(referenced).unwrap_or_else(|| {
            unreachable!("referenced is a key of index_of, built from workflow.nodes")
        });

        if keyed && target_node.for_each.is_none() {
            errors.push(CheckError::KeyedOnScalarNode {
                node: site_node.clone(),
                port: site_port.clone(),
                referenced: referenced.clone(),
            });
            return None;
        }

        let target_spec = match self.specs.get(referenced) {
            Some(Some(spec)) => *spec,
            _ => return None,
        };

        let Some(output_ty) = target_spec.outputs.get(port) else {
            errors.push(CheckError::UnknownPort {
                node: referenced.clone(),
                tool: target_spec.name.clone(),
                port: port.clone(),
            });
            return None;
        };

        let resolved_ty = if !keyed && target_node.for_each.is_some() {
            TypeRef::list_of(output_ty.name.clone())
        } else {
            output_ty.clone()
        };

        Some((resolved_ty, Some((referenced.clone(), port.clone()))))
    }
}

/// Check a `with`-bound literal against `expected`: a secret-accepting
/// port refuses it outright; a list-typed port refuses it because no
/// literal can supply a list; otherwise it is parsed against the port's
/// scalar type.
fn check_literal(
    node: &NodeName,
    port: &PortName,
    text: &str,
    expected: &PortType,
    registry: &TypeRegistry,
    errors: &mut Vec<CheckError>,
) -> Option<TypeRef> {
    if port_accepts_secret(expected, registry) {
        errors.push(CheckError::SecretLiteral {
            node: node.clone(),
            port: port.clone(),
        });
        return None;
    }

    let PortType::Exact(ty) = expected else {
        unreachable!("AnySecret always accepts a secret and is handled above");
    };

    if ty.list {
        let error = ParseError::new("Binding", "lists cannot be literals");
        errors.push(CheckError::InvalidLiteral {
            node: node.clone(),
            port: port.clone(),
            error,
        });
        return None;
    }

    match Value::parse(ty, text) {
        Ok(value) => Some(value.ty().clone()),
        Err(error) => {
            errors.push(CheckError::InvalidLiteral {
                node: node.clone(),
                port: port.clone(),
                error,
            });
            None
        }
    }
}

/// Find every cyclic strongly-connected component (size greater than one,
/// or a single node with a self-loop), each reported as one
/// [`CheckError::Cycle`], ordered by the lowest declaration index among
/// its nodes.
fn find_cycles(graph: &DiGraph<NodeName, ()>, workflow: &Workflow) -> Vec<CheckError> {
    if petgraph::algo::toposort(graph, None).is_ok() {
        return Vec::new();
    }

    let decl_index = |idx: NodeIndex| {
        workflow
            .nodes
            .get_index_of(&graph[idx])
            .unwrap_or_else(|| unreachable!("every graph node is a workflow node"))
    };

    let mut cycles: Vec<Vec<NodeIndex>> = petgraph::algo::tarjan_scc(graph)
        .into_iter()
        .filter(|scc| scc.len() > 1 || graph.contains_edge(scc[0], scc[0]))
        .collect();
    for scc in &mut cycles {
        scc.sort_by_key(|&idx| decl_index(idx));
    }
    cycles.sort_by_key(|scc| decl_index(scc[0]));

    cycles
        .into_iter()
        .map(|scc| CheckError::Cycle {
            nodes: scc.into_iter().map(|idx| graph[idx].clone()).collect(),
        })
        .collect()
}

/// A topological order over `graph`'s nodes: repeatedly pick the
/// smallest-declaration-index node with no unprocessed dependency. `O(n^2)`
/// in the node count, which is fine for a workflow-sized graph; simple and
/// exactly reproducible, unlike relying on a general toposort's internal
/// traversal order.
fn topo_order(
    workflow: &Workflow,
    graph: &DiGraph<NodeName, ()>,
    index_of: &IndexMap<NodeName, NodeIndex>,
) -> Vec<NodeName> {
    use petgraph::visit::EdgeRef;

    let mut indegree: IndexMap<NodeIndex, usize> =
        graph.node_indices().map(|idx| (idx, 0)).collect();
    for edge in graph.edge_references() {
        *indegree.get_mut(&edge.target()).unwrap() += 1;
    }

    let mut done: HashSet<NodeIndex> = HashSet::new();
    let mut order = Vec::with_capacity(workflow.nodes.len());

    while order.len() < workflow.nodes.len() {
        let next = workflow
            .nodes
            .keys()
            .find(|name| {
                let idx = index_of[*name];
                !done.contains(&idx) && indegree[&idx] == 0
            })
            .unwrap_or_else(|| unreachable!("an acyclic graph always has a ready node"));
        let idx = index_of[next];
        done.insert(idx);
        order.push(next.clone());
        for edge in graph.edges(idx) {
            *indegree.get_mut(&edge.target()).unwrap() -= 1;
        }
    }

    order
}

/// The plan's approval class: the maximum over every non-pure node's tool
/// class. Only called once every node's spec is known to resolve (`check`
/// has already returned on any `UnknownTool`).
fn compute_class(workflow: &Workflow, specs: &IndexMap<NodeName, Option<&ToolSpec>>) -> Class {
    Class::max_of(workflow.nodes.keys().filter_map(|name| {
        let spec = specs[name].unwrap_or_else(|| unreachable!("no UnknownTool errors remain"));
        (!spec.pure).then_some(spec.class)
    }))
}

/// [`CheckWarning::UnusedInput`] for every declared input `used` does not
/// contain, in declaration order.
fn collect_warnings(workflow: &Workflow, used: &HashSet<InputName>) -> Vec<CheckWarning> {
    workflow
        .inputs
        .keys()
        .filter(|name| !used.contains(*name))
        .map(|name| CheckWarning::UnusedInput {
            input: name.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::tool::{Inputs, Observation, Outputs, Tool, ToolError};
    use crate::value::TypeName;
    use crate::workflow::InputSpec;
    use willikins_types::SinkToken;

    fn ty(name: &str) -> TypeRef {
        TypeRef::scalar(TypeName::parse(name).unwrap())
    }

    fn list_ty(name: &str) -> TypeRef {
        TypeRef::list_of(TypeName::parse(name).unwrap())
    }

    fn exact(name: &str) -> PortType {
        PortType::Exact(ty(name))
    }

    fn port(name: &str) -> PortName {
        PortName::parse(name).unwrap()
    }

    fn node_name(name: &str) -> NodeName {
        NodeName::parse(name).unwrap()
    }

    fn input_name(name: &str) -> InputName {
        InputName::parse(name).unwrap()
    }

    fn tool_name(name: &str) -> ToolName {
        ToolName::parse(name).unwrap()
    }

    /// A tool whose spec is fixed at construction; `read` always reports
    /// `Absent` with no predicted outputs.
    struct DummyTool {
        spec: ToolSpec,
    }

    impl Tool for DummyTool {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }

        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
            Ok(Outputs::new())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn spec_of(
        name: &str,
        inputs: &[(&str, PortType, bool)],
        outputs: &[(&str, TypeRef)],
        key: &[&str],
        class: Class,
        pure: bool,
    ) -> ToolSpec {
        let mut in_map = IndexMap::new();
        for (port_name, port_ty, required) in inputs {
            in_map.insert(
                port(port_name),
                PortSpec {
                    ty: port_ty.clone(),
                    required: *required,
                },
            );
        }
        let mut out_map = IndexMap::new();
        for (port_name, port_ty) in outputs {
            out_map.insert(port(port_name), port_ty.clone());
        }
        ToolSpec {
            name: tool_name(name),
            description: format!("Test double for `{name}`."),
            inputs: in_map,
            outputs: out_map,
            key: key.iter().map(|p| port(p)).collect(),
            class,
            pure,
        }
    }

    /// A catalog whose tools mirror the plan's fake-provider port table
    /// exactly, one [`DummyTool`] per row. Duplicated (rather than shared)
    /// with `tests/check.rs`, which cannot see this module's private
    /// items; see the module docs.
    #[allow(clippy::too_many_lines)]
    fn test_catalog() -> Catalog {
        let mut catalog = Catalog::new(willikins_types::registry());
        let specs = vec![
            spec_of(
                "naming.v1",
                &[
                    ("org", exact("GitHubOrg"), true),
                    ("slug", exact("ProjectSlug"), true),
                ],
                &[
                    ("github_repo", ty("GitHubRepo")),
                    ("doppler_project", ty("DopplerProject")),
                ],
                &[],
                Class::Reversible,
                true,
            ),
            spec_of(
                "github.repo.ensure",
                &[
                    ("repo", exact("GitHubRepo"), true),
                    ("visibility", exact("RepoVisibility"), true),
                ],
                &[("repo", ty("GitHubRepo")), ("url", ty("HttpsUrl"))],
                &["repo"],
                Class::Reversible,
                false,
            ),
            spec_of(
                "github.actions_secret.ensure",
                &[
                    ("repo", exact("GitHubRepo"), true),
                    ("name", exact("ActionsSecretName"), true),
                    ("value", PortType::AnySecret, true),
                ],
                &[],
                &["repo", "name"],
                Class::Reversible,
                false,
            ),
            spec_of(
                "doppler.project.ensure",
                &[("project", exact("DopplerProject"), true)],
                &[("project", ty("DopplerProject"))],
                &["project"],
                Class::Reversible,
                false,
            ),
            spec_of(
                "doppler.config.ensure",
                &[
                    ("project", exact("DopplerProject"), true),
                    ("environment", exact("EnvironmentSlug"), true),
                ],
                &[("config", ty("DopplerConfig"))],
                &["project", "environment"],
                Class::Reversible,
                false,
            ),
            spec_of(
                "doppler.service_token.ensure",
                &[
                    ("config", exact("DopplerConfig"), true),
                    ("name", exact("DopplerTokenName"), true),
                ],
                &[("token", ty("DopplerServiceToken"))],
                &["config", "name"],
                Class::Reversible,
                false,
            ),
            spec_of(
                "doppler.secret.get",
                &[
                    ("config", exact("DopplerConfig"), true),
                    ("name", exact("SecretName"), true),
                ],
                &[("value", ty("DopplerSecretValue"))],
                &[],
                Class::Reversible,
                true,
            ),
            spec_of(
                "fake.secret_list",
                &[("config", exact("DopplerConfig"), true)],
                &[("tokens", list_ty("DopplerServiceToken"))],
                &[],
                Class::Reversible,
                true,
            ),
            spec_of(
                "fake.irreversible.ensure",
                &[("key", exact("ProjectSlug"), true)],
                &[],
                &["key"],
                Class::Irreversible,
                false,
            ),
            spec_of(
                "template.render",
                &[
                    ("template", exact("TemplateSource"), true),
                    ("value", exact("Text"), true),
                ],
                &[("rendered", ty("Text"))],
                &[],
                Class::Reversible,
                true,
            ),
        ];
        for spec in specs {
            catalog.insert(Arc::new(DummyTool { spec })).unwrap();
        }
        catalog
    }

    #[test]
    fn unknown_tool_names_the_node_and_tool() {
        let workflow =
            Workflow::new("w").node(node_name("mystery"), Node::new(tool_name("no.such.tool")));
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UnknownTool {
                node: node_name("mystery"),
                tool: tool_name("no.such.tool"),
            }]
        );
        assert_eq!(
            errors[0].to_string(),
            "node `mystery`: unknown tool `no.such.tool`"
        );
    }

    #[test]
    fn unknown_port_for_a_with_key_that_is_not_one_of_the_tools_ports() {
        let workflow = Workflow::new("w").node(
            node_name("names"),
            Node::new(tool_name("naming.v1"))
                .port(port("org"), Binding::Literal("lightless-labs".to_string()))
                .port(port("slug"), Binding::Literal("demo".to_string()))
                .port(port("bogus"), Binding::Literal("x".to_string())),
        );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UnknownPort {
                node: node_name("names"),
                tool: tool_name("naming.v1"),
                port: port("bogus"),
            }]
        );
    }

    #[test]
    fn unknown_port_for_a_step_reference_to_a_missing_output_port() {
        let workflow = Workflow::new("w")
            .node(
                node_name("names"),
                Node::new(tool_name("naming.v1"))
                    .port(port("org"), Binding::Literal("lightless-labs".to_string()))
                    .port(port("slug"), Binding::Literal("demo".to_string())),
            )
            .node(
                node_name("repo"),
                Node::new(tool_name("github.repo.ensure"))
                    .port(
                        port("repo"),
                        Binding::Step {
                            node: node_name("names"),
                            port: port("no_such_output"),
                        },
                    )
                    .port(port("visibility"), Binding::Literal("private".to_string())),
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UnknownPort {
                node: node_name("names"),
                tool: tool_name("naming.v1"),
                port: port("no_such_output"),
            }]
        );
    }

    #[test]
    fn unknown_node_for_a_step_reference_to_a_nonexistent_node() {
        let workflow = Workflow::new("w").node(
            node_name("repo"),
            Node::new(tool_name("github.repo.ensure"))
                .port(
                    port("repo"),
                    Binding::Step {
                        node: node_name("ghost"),
                        port: port("repo"),
                    },
                )
                .port(port("visibility"), Binding::Literal("private".to_string())),
        );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UnknownNode {
                node: node_name("repo"),
                port: port("repo"),
                referenced: node_name("ghost"),
            }]
        );
    }

    #[test]
    fn unbound_input_for_every_missing_required_port() {
        let workflow =
            Workflow::new("w").node(node_name("names"), Node::new(tool_name("naming.v1")));
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![
                CheckError::UnboundInput {
                    node: node_name("names"),
                    port: port("org"),
                },
                CheckError::UnboundInput {
                    node: node_name("names"),
                    port: port("slug"),
                },
            ]
        );
    }

    #[test]
    fn undeclared_input_for_an_input_binding_with_no_matching_declaration() {
        let workflow = Workflow::new("w").node(
            node_name("names"),
            Node::new(tool_name("naming.v1"))
                .port(port("org"), Binding::Input(input_name("org")))
                .port(port("slug"), Binding::Literal("demo".to_string())),
        );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UndeclaredInput {
                node: node_name("names"),
                port: port("org"),
                input: input_name("org"),
            }]
        );
    }

    #[test]
    fn invalid_literal_for_a_bad_scalar_literal() {
        let workflow = Workflow::new("w")
            .node(
                node_name("names"),
                Node::new(tool_name("naming.v1"))
                    .port(port("org"), Binding::Literal("lightless-labs".to_string()))
                    .port(port("slug"), Binding::Literal("demo".to_string())),
            )
            .node(
                node_name("repo"),
                Node::new(tool_name("github.repo.ensure"))
                    .port(
                        port("repo"),
                        Binding::Step {
                            node: node_name("names"),
                            port: port("github_repo"),
                        },
                    )
                    .port(port("visibility"), Binding::Literal("internal".to_string())),
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(errors.len(), 1);
        let CheckError::InvalidLiteral {
            node,
            port: p,
            error,
        } = &errors[0]
        else {
            panic!("expected InvalidLiteral, got {:?}", errors[0]);
        };
        assert_eq!(node, &node_name("repo"));
        assert_eq!(p, &port("visibility"));
        assert!(error.reason.contains("private"), "{}", error.reason);
    }

    #[test]
    fn invalid_literal_for_a_literal_on_a_list_typed_port() {
        // No milestone-1 tool has a list-typed *input* port, so this test
        // uses a synthetic one to exercise the rule directly.
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("items"),
            PortSpec {
                ty: PortType::Exact(list_ty("GitHubOrg")),
                required: true,
            },
        );
        let spec = ToolSpec {
            name: tool_name("test.list_port"),
            description: "A tool with a list-typed input.".to_string(),
            inputs,
            outputs: IndexMap::new(),
            key: Vec::new(),
            class: Class::Reversible,
            pure: true,
        };
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog.insert(Arc::new(DummyTool { spec })).unwrap();

        let workflow = Workflow::new("w").node(
            node_name("n"),
            Node::new(tool_name("test.list_port"))
                .port(port("items"), Binding::Literal("a,b".to_string())),
        );
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::InvalidLiteral {
                node: node_name("n"),
                port: port("items"),
                error: ParseError::new("Binding", "lists cannot be literals"),
            }]
        );
    }

    #[test]
    fn type_mismatch_for_a_binding_of_the_wrong_type() {
        let workflow = Workflow::new("w")
            .input(input_name("slug"), InputSpec::new(ty("ProjectSlug")))
            .node(
                node_name("repo"),
                Node::new(tool_name("github.repo.ensure"))
                    .port(port("repo"), Binding::Input(input_name("slug")))
                    .port(port("visibility"), Binding::Literal("private".to_string())),
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::TypeMismatch {
                node: node_name("repo"),
                port: port("repo"),
                expected: exact("GitHubRepo"),
                found: ty("ProjectSlug"),
            }]
        );
    }

    #[test]
    fn secret_literal_for_a_literal_bound_to_an_any_secret_port() {
        let workflow = Workflow::new("w")
            .node(
                node_name("names"),
                Node::new(tool_name("naming.v1"))
                    .port(port("org"), Binding::Literal("lightless-labs".to_string()))
                    .port(port("slug"), Binding::Literal("demo".to_string())),
            )
            .node(
                node_name("repo"),
                Node::new(tool_name("github.repo.ensure"))
                    .port(
                        port("repo"),
                        Binding::Step {
                            node: node_name("names"),
                            port: port("github_repo"),
                        },
                    )
                    .port(port("visibility"), Binding::Literal("private".to_string())),
            )
            .node(
                node_name("ci_secret"),
                Node::new(tool_name("github.actions_secret.ensure"))
                    .port(
                        port("repo"),
                        Binding::Step {
                            node: node_name("repo"),
                            port: port("repo"),
                        },
                    )
                    .port(port("name"), Binding::Literal("DOPPLER_TOKEN".to_string()))
                    .port(
                        port("value"),
                        Binding::Literal("dp.st.prd.hunter2hunter2".to_string()),
                    ),
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::SecretLiteral {
                node: node_name("ci_secret"),
                port: port("value"),
            }]
        );
    }

    #[test]
    fn cycle_from_a_node_referencing_itself() {
        let workflow = Workflow::new("w").node(
            node_name("a"),
            Node::new(tool_name("doppler.project.ensure")).port(
                port("project"),
                Binding::Step {
                    node: node_name("a"),
                    port: port("project"),
                },
            ),
        );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::Cycle {
                nodes: vec![node_name("a")],
            }]
        );
        assert_eq!(errors[0].to_string(), "cycle among nodes: `a`");
    }

    #[test]
    fn cycle_from_two_nodes_referencing_each_other() {
        let workflow = Workflow::new("w")
            .node(
                node_name("a"),
                Node::new(tool_name("doppler.project.ensure")).port(
                    port("project"),
                    Binding::Step {
                        node: node_name("b"),
                        port: port("project"),
                    },
                ),
            )
            .node(
                node_name("b"),
                Node::new(tool_name("doppler.project.ensure")).port(
                    port("project"),
                    Binding::Step {
                        node: node_name("a"),
                        port: port("project"),
                    },
                ),
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::Cycle {
                nodes: vec![node_name("a"), node_name("b")],
            }]
        );
    }

    #[test]
    fn duplicate_node_is_not_produced_by_check_but_exists_for_the_future_dsl_parser() {
        // `Workflow::nodes` is an `IndexMap`, which cannot hold two entries
        // under the same key, so `check` itself can never emit this
        // variant. It exists so a future DSL parser (task 10) can report a
        // duplicate `steps:` key through the same error type; this test
        // only pins its `Display`.
        let error = CheckError::DuplicateNode {
            node: node_name("repo"),
        };
        assert_eq!(error.to_string(), "duplicate node name `repo`");
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn every_variants_display_names_its_node_and_port() {
        let cases: Vec<(CheckError, &str, Option<&str>)> = vec![
            (
                CheckError::UnknownTool {
                    node: node_name("n"),
                    tool: tool_name("t.t"),
                },
                "n",
                None,
            ),
            (
                CheckError::UnknownPort {
                    node: node_name("n"),
                    tool: tool_name("t.t"),
                    port: port("p"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::UnknownNode {
                    node: node_name("n"),
                    port: port("p"),
                    referenced: node_name("ghost"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::UnboundInput {
                    node: node_name("n"),
                    port: port("p"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::UndeclaredInput {
                    node: node_name("n"),
                    port: port("p"),
                    input: input_name("x"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::InvalidLiteral {
                    node: node_name("n"),
                    port: port("p"),
                    error: ParseError::new("Binding", "bad"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::TypeMismatch {
                    node: node_name("n"),
                    port: port("p"),
                    expected: exact("GitHubOrg"),
                    found: ty("ProjectSlug"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::SecretLiteral {
                    node: node_name("n"),
                    port: port("p"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::SecretToNonSecretSink {
                    from: (node_name("src"), port("out")),
                    to: (node_name("n"), port("p")),
                },
                "src",
                Some("out"),
            ),
            (
                CheckError::SecretWorkflowInput {
                    input: input_name("x"),
                    ty: ty("DopplerServiceToken"),
                },
                "x",
                None,
            ),
            (
                CheckError::SecretForEachSource {
                    node: node_name("n"),
                },
                "n",
                None,
            ),
            (
                CheckError::ForEachOverScalar {
                    node: node_name("n"),
                },
                "n",
                None,
            ),
            (
                CheckError::ItemOutsideForEach {
                    node: node_name("n"),
                    port: port("p"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::KeyedOnScalarNode {
                    node: node_name("n"),
                    port: port("p"),
                    referenced: node_name("ghost"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::Cycle {
                    nodes: vec![node_name("n")],
                },
                "n",
                None,
            ),
            (
                CheckError::DuplicateNode {
                    node: node_name("n"),
                },
                "n",
                None,
            ),
        ];
        // `DefaultTypeMismatch` names an input rather than a node, so it
        // is checked on its own rather than through the node/port loop.
        assert_eq!(
            CheckError::DefaultTypeMismatch {
                input: input_name("visibility"),
                expected: ty("RepoVisibility"),
                found: ty("EnvironmentSlug"),
            }
            .to_string(),
            "input `visibility`: default value has type `EnvironmentSlug`, expected `RepoVisibility`"
        );
        for (error, node_needle, port_needle) in cases {
            let message = error.to_string();
            assert!(
                message.contains(node_needle),
                "{error:?} display {message:?} does not name node `{node_needle}`"
            );
            if let Some(port_needle) = port_needle {
                assert!(
                    message.contains(port_needle),
                    "{error:?} display {message:?} does not name port `{port_needle}`"
                );
            }
        }
    }

    #[test]
    fn check_warning_display_names_the_input() {
        let warning = CheckWarning::UnusedInput {
            input: input_name("slug"),
        };
        assert_eq!(
            warning.to_string(),
            "input `slug` is declared but never used"
        );
    }
}
