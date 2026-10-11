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
//! 0. [`CheckError::Unlinked`], one per `uses:` step still on
//!    [`Workflow::uses`], the first thing [`check`] looks at. A composite
//!    must go through the linker (milestone 2b, `willikins_core::compose`)
//!    before it reaches `check` at all, so a non-empty `uses` map is
//!    always a caller defect; nothing past this point is evaluated.
//! 1. Workflow input errors, one per declared input, in declaration
//!    order: [`CheckError::SecretWorkflowInput`] for a secret declared
//!    type, [`CheckError::UnregisteredInputType`] for a declared type the
//!    registry has never heard of, or else [`CheckError::DefaultTypeMismatch`]
//!    for a default value whose type is not the declared one.
//! 2. [`CheckError::UnknownTool`], one per node with an unrecognised tool,
//!    in node declaration order. A node whose tool is unknown is skipped
//!    for every later step (its ports cannot be checked against a spec
//!    that does not exist).
//! 3. Boundary errors (milestone 2b task C1, decision (d4)), one per
//!    entry of [`Workflow::boundaries`], in that order, skipping a
//!    boundary the linker left unbound (`binding` is `None`): that one
//!    became a *fixed* input instead (decision (d6)) and was already
//!    checked by stage 1, under this very same combined name, so
//!    checking it again here would only report it twice. For the rest,
//!    the same five rules stage 1 runs over a root input, over the used
//!    document's own declared [`crate::compose::Boundary::spec`] instead
//!    (`SecretWorkflowInput`, `UnregisteredInputType`,
//!    `DisallowedInputType`, `AcknowledgementDefault`,
//!    `DefaultTypeMismatch`) — reported under the combined
//!    `<uses step>/<child input>` name
//!    ([`crate::compose::prefixed_input`]), which doubles as naming the
//!    `uses:` step, since every one of these five variants' `input`
//!    field is an [`InputName`] and that type already accepts a
//!    `/`-separated path (decision (d3)); then, only when none of those
//!    five fired, an exact match between the binding `check` resolves
//!    the same way it resolves a node's own port binding and the
//!    boundary's declared type, with no conversion probe
//!    ([`CheckError::UsesInputTypeMismatch`]; decision (d4), "exact type
//!    at the boundary, no conversion").
//! 4. For every node whose tool *is* known, in node declaration order:
//!    its `for_each` binding's errors -- a secret source
//!    ([`CheckError::SecretForEachSource`]), then a scalar source
//!    ([`CheckError::ForEachOverScalar`]), then an identifier-typed source
//!    ([`CheckError::IdentifierForEachSource`]), then a colliding default
//!    ([`CheckError::DuplicateForEachDefault`]); the identifier check
//!    comes before the duplicate-default check because that error's
//!    `key` field is a rendered identifier -- then its input ports in the
//!    order the tool spec declares them (an unbound required port, or a
//!    bound port's own errors), then any `with` keys that are not one of
//!    the tool's ports, in `with` declaration order.
//! 5. Workflow outputs' errors, in declaration order.
//! 6. [`CheckError::Cycle`], one per cyclic strongly-connected component,
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
//! - An unregistered type named in an [`InputSpec`](crate::workflow::InputSpec)
//!   is now caught explicitly, at the declaration site, by
//!   [`CheckError::UnregisteredInputType`] (added by task 8, alongside
//!   `describe`). Once reported it is still treated as non-secret for the
//!   rest of `check`'s own bookkeeping (the registry has no secrecy answer
//!   for a name it does not have), which only matters for downstream node
//!   ports that happen to reference the same undeclared type name — a tool
//!   port naming an unregistered type is refused separately by
//!   [`crate::tool::ToolSpec::validate`], via [`crate::catalog::Catalog::insert`],
//!   so that avenue was already closed before this variant existed.
//! - The plan lists sixteen [`CheckError`] variants. Two more were added
//!   by the adversarial pass, because the plan has no variant for the
//!   defects they name: [`CheckError::DefaultTypeMismatch`] (an input's
//!   default value is not of its declared type -- the plan type-checks
//!   defaults at document load, which leaves a `Workflow` built any other
//!   way unchecked) and [`CheckError::NestedList`] (`Step` on a `for_each`
//!   node whose own output port is *already* list-typed, which would need
//!   a `list<list<T>>` the type model cannot represent).
//! - [`Resolver::check_boundaries`] (milestone 2b task C1) does not check
//!   a `uses:` step's own `with:` binding when it is a
//!   [`Binding::List`]: every element, literal or reference, is copied
//!   unchanged by the linker's substitution (decision (d4)) and so
//!   reaches a real node port downstream with a real expected type of
//!   its own -- *unless* the used document never references that
//!   declared input anywhere in its own body, in which case nothing
//!   ever types that particular list. A document that writes such an
//!   input only to re-export it (`outputs: x: ${{ inputs.x }}`,
//!   decision (d6)'s addendum) is exactly the pass-through case this
//!   misses; closing it needs the same per-element machinery
//!   [`Self::check_list_port`] already has, mirrored to forbid the
//!   one-hop conversion it otherwise allows. Not needed by any
//!   acceptance test this milestone pins; left as a gap rather than
//!   risked under time pressure.

use std::collections::HashSet;
use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;

use indexmap::IndexMap;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::catalog::Catalog;
use crate::class::Class;
use crate::compose::prefixed_input;
use crate::site::Site;
use crate::tool::{PortName, PortSpec, Tool, ToolName, ToolSpec};
use crate::value::{
    Conversion, ConversionMismatch, PortType, TypeRef, TypeRegistry, Value, reported_type_name_or,
};
use crate::workflow::{Binding, InputName, InputSpec, Node, NodeName, OutputName, Workflow};
use willikins_types::{Description, ParseError, WorkflowName};

/// A workflow that has passed [`check`]: its topological execution order,
/// its approval class, any non-fatal warnings, and the resolved type of
/// every binding `check` looked at.
///
/// `Debug` is hand-written, not derived (milestone 3n, decision (s3)):
/// [`CheckedNode`] holds `Arc<dyn Tool>`, and `dyn Tool` is not `Debug`.
/// See [`Checked`]'s own `impl fmt::Debug` for what it prints.
#[derive(Clone)]
pub struct Checked {
    /// The checked workflow, unchanged.
    workflow: Workflow,
    /// Every declared workflow input, in declaration order (`InputId` is
    /// the position in this slice -- SHARED VALUES of milestone 3n's
    /// decision (g1)). From G2 on, `check_input_types`, `describe` and
    /// the input-resolution path (`crate::plan::resolve_binding`) read
    /// this instead of looking a declared input up by name in the
    /// workflow and, for `describe`, the global registry a second time.
    inputs: Vec<CheckedInput>,
    /// Every node, in an order where each node comes after every node it
    /// depends on. Ties are broken by declaration order.
    order: Vec<NodeName>,
    /// Every node, in the same topological order as [`Self::order`], with
    /// the tool handle `check` resolved for it and its [`Shape`]
    /// (milestone 3n, decision (g1)). From G1 on, `plan` and `apply` walk
    /// this instead of looking a node and its tool up by name in the
    /// workflow and the catalog a second time.
    nodes: Vec<CheckedNode>,
    /// The plan's approval class: the maximum over every non-pure node's
    /// tool class, or [`Class::Reversible`] when there are none.
    class: Class,
    /// Non-fatal warnings, alongside a successful check.
    warnings: Vec<CheckWarning>,
    /// Every workflow output, classified (milestone 3n, decision (g1)), in
    /// declaration order -- the same order as [`Self::output_types`], and
    /// built from exactly the same successful resolutions. From G3 on,
    /// `plan` and `apply` read this instead of walking
    /// [`Workflow::outputs`] a second time.
    outputs: Vec<CheckedOutput>,
}

impl Checked {
    /// The checked workflow's own name.
    #[must_use]
    pub fn name(&self) -> &WorkflowName {
        &self.workflow.name
    }

    /// A read-only view of the checked workflow's own declared surface:
    /// its name, description, declared inputs and output names. Never the
    /// nodes -- see [`DocumentView`].
    #[must_use]
    pub fn document(&self) -> DocumentView<'_> {
        DocumentView {
            workflow: &self.workflow,
        }
    }

    /// The plan's approval class: see [`Self::class`]'s field doc (milestone
    /// 1).
    #[must_use]
    pub fn class(&self) -> Class {
        self.class
    }

    /// Non-fatal warnings, alongside a successful check.
    #[must_use]
    pub fn warnings(&self) -> &[CheckWarning] {
        &self.warnings
    }

    /// Every node, in an order where each node comes after every node it
    /// depends on. Ties are broken by declaration order.
    #[must_use]
    pub fn order(&self) -> Vec<NodeName> {
        self.order.clone()
    }

    /// Every node `with` binding `check` validated, keyed by node then
    /// port: the binding's own resolved type, and the conversion `check`
    /// chose to deliver it to the port's type, if any ([`Edge`]). Does not
    /// cover `for_each` bindings, and never mixes in workflow outputs
    /// (those are [`Self::output_types`]), so a node named `outputs` keeps
    /// its own entry. Public accessor, SHARED VALUES of milestone 3n's
    /// decision (s1); owned, so a caller resolving many ports should bind
    /// it once rather than call this per port. `plan` and `apply` no
    /// longer read this map at all (as of G4): every bound port's edge
    /// rides on its own [`PortEdge`] instead.
    ///
    /// From G7 on this is a view, not a copy (decision (s1)): built fresh
    /// from [`Self::nodes`]' own [`Shape::ports`](Shape::Single::ports),
    /// re-sorted into *declaration* order (`CheckedNode::declared_at`) --
    /// `self.nodes` itself is topological -- so what this prints is
    /// exactly what `plan` and `apply` consume, not a parallel copy of it
    /// `check` kept around only for this accessor.
    #[must_use]
    pub fn types(&self) -> IndexMap<NodeName, IndexMap<PortName, Edge>> {
        let mut by_declared_at: Vec<&CheckedNode> = self.nodes.iter().collect();
        by_declared_at.sort_by_key(|node| node.declared_at);
        by_declared_at
            .into_iter()
            .filter_map(|node| {
                let ports = node_port_edges(node);
                if ports.is_empty() {
                    None
                } else {
                    Some((node.name.clone(), ports))
                }
            })
            .collect()
    }

    /// One checked edge, by node and port, or `None` when that port is
    /// unbound (or `node` is not a node of this workflow). From G7 on,
    /// looked up straight off [`Self::nodes`]' own ports rather than a
    /// stored copy (same view as [`Self::types`]).
    #[must_use]
    pub fn edge(&self, node: &NodeName, port: &PortName) -> Option<&Edge> {
        let checked_node = self
            .nodes
            .iter()
            .find(|candidate| &candidate.name == node)?;
        match &checked_node.shape {
            Shape::Single { ports, .. } => find_port_edge(ports, port),
            Shape::Each { ports, .. } => find_port_edge(ports, port),
        }
    }

    /// The resolved type of every workflow output, in declaration order.
    /// From G7 on, a view over [`Self::outputs`] (decision (s1)): each
    /// [`CheckedOutput::ty`] is exactly this map's own entry for it, the
    /// same value `check_outputs` resolved before building the output's
    /// `Source` -- there is no longer a second copy of it kept only for
    /// this accessor.
    #[must_use]
    pub fn output_types(&self) -> IndexMap<OutputName, TypeRef> {
        self.outputs
            .iter()
            .map(|output| (output.name.clone(), output.ty.clone()))
            .collect()
    }

    /// Every node, in topological order (the same order as [`Self::order`]),
    /// with its already-resolved tool handle and [`Shape`]. Crate-private:
    /// ids are minted only in `check.rs` and never leave this crate
    /// (milestone 3n, decision (g1)).
    pub(crate) fn nodes(&self) -> &[CheckedNode] {
        &self.nodes
    }

    /// Every declared workflow input, in declaration order (`InputId` is
    /// the position in this slice; milestone 3n, decision (g1)).
    /// Crate-private, on the same "never leaves `willikins-core`" schedule
    /// as [`Self::nodes`]: `InputId` is minted only in `check.rs`.
    pub(crate) fn inputs(&self) -> &[CheckedInput] {
        &self.inputs
    }

    /// Every workflow output, classified, in declaration order (milestone
    /// 3n, decision (g1)). Crate-private, on the same schedule as
    /// [`Self::nodes`]: a [`CheckedOutput`]'s `Source` holds a `SingleId`/
    /// `EachId`/`InputId`, minted only in `check.rs`.
    pub(crate) fn outputs(&self) -> &[CheckedOutput] {
        &self.outputs
    }
}

/// Hand-written (decision (s3)): [`CheckedNode`] holds `Arc<dyn Tool>`,
/// which is not `Debug`, so `#[derive(Debug)]` does not compile once
/// [`Checked::nodes`] exists. Prints the workflow's own name, every node's
/// name, declared position and tool name, its [`Shape`] by kind and type,
/// the plan class and warnings, and the checked edge/output-type tables
/// (type information only -- [`Edge`] never holds a value). As `Shape`
/// gains ports and sources (G3 on), a bound literal has to print as its
/// own [`Value`]'s `Debug`, which redacts by type construction (see
/// `Value`'s own impl) -- this type never prints a literal's raw content.
impl fmt::Debug for Checked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Checked")
            .field("name", &self.workflow.name)
            .field("inputs", &self.inputs)
            .field("order", &self.order)
            .field("nodes", &self.nodes)
            .field("class", &self.class)
            .field("warnings", &self.warnings)
            .field("outputs", &self.outputs)
            .finish()
    }
}

/// One checked node: its name, its position among the workflow's declared
/// nodes, the tool handle [`check`] resolved for it, and its [`Shape`]
/// (milestone 3n, decision (g1)). Crate-private -- never leaves
/// `willikins-core` (SHARED VALUES: "Ids ... never leave willikins-core").
///
/// `Debug` is hand-written: `tool` holds `Arc<dyn Tool>`, not `Debug`, and
/// prints as its own tool name instead. `Clone` is still derived -- `Arc`
/// is `Clone` regardless of what it points to.
#[derive(Clone)]
pub(crate) struct CheckedNode {
    /// This node's name.
    pub(crate) name: NodeName,
    /// This node's index into [`Workflow::nodes`] -- declaration order,
    /// not topological order (SHARED VALUES).
    pub(crate) declared_at: usize,
    /// The tool handle [`check`] resolved against the catalog it was
    /// given. `plan` and `apply` call through this, never through a
    /// second catalog lookup by name (verify item 4; milestone 3n's
    /// decision (b2) later drops the catalog parameter itself).
    pub(crate) tool: Arc<dyn Tool>,
    /// Whether this node runs once (`Single`) or once per `for_each` item
    /// (`Each`), and the typed id that gives each kind its own index
    /// space.
    pub(crate) shape: Shape,
}

impl fmt::Debug for CheckedNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckedNode")
            .field("name", &self.name)
            .field("declared_at", &self.declared_at)
            .field("tool", &self.tool.spec().name)
            .field("shape", &self.shape)
            .finish()
    }
}

/// A node calls its tool once (`Single`) or once per item of a `for_each`
/// list (`Each`) -- milestone 3n, decision (g1). Each kind has its own
/// typed id, a separate index space from the other kind's, so a later
/// task's result tables can be `Vec<SingleResult>`/`Vec<EachResult>`
/// rather than one table either kind might index into by mistake.
///
/// G1 gave this `ids` and, for `Each`, the `for_each` source's own resolved
/// element type. G3 gives each kind its own bound **scalar** ports
/// (`ports`) and gives `Each` its own `for_each` source, classified
/// (`source`) -- a node with no bound port has an empty `ports` list,
/// exactly as [`Checked::types`]' own doc already says for the older map.
/// G4 gives a [`crate::workflow::Binding::List`] port an entry here too
/// ([`Bound::List`]), so `ports` now holds every bound port, scalar or
/// list alike.
#[derive(Clone)]
pub(crate) enum Shape {
    /// A node with no `for_each`: exactly one instance, ever.
    Single {
        /// This single node's own id, among every `Single` node, in
        /// topological order.
        id: SingleId,
        /// Every bound port of this node's tool, scalar or list alike, in
        /// the tool spec's own input order (SHARED VALUES' `types()`
        /// order).
        ports: Vec<PortEdge<Infallible>>,
    },
    /// A `for_each` node: one instance per item of its source list.
    Each {
        /// This `for_each` node's own id, among every `Each` node, in
        /// topological order.
        id: EachId,
        /// The `for_each` binding itself, classified -- never
        /// [`Source::Item`] (`check` already rejects `${{ item }}` as a
        /// `for_each` source, since it is resolved outside any `for_each`
        /// scope).
        source: Source<Infallible>,
        /// The `for_each` source's resolved element type -- `check`
        /// already rejected a secret or non-list source, so this is
        /// always the scalar type every instance's item is a value of.
        element: TypeRef,
        /// Every bound port of this node's tool, scalar or list alike, in
        /// the tool spec's own input order. A port whose binding is
        /// `${{ item }}` carries [`Source::Item`].
        ports: Vec<PortEdge<InEach>>,
    },
}

/// Hand-written, not derived: a plain `#[derive(Debug)]` would still
/// compile (`Shape` holds no `Arc<dyn Tool>`), but `rustc`'s dead-code
/// analysis explicitly ignores a derived impl's own field reads, so a
/// field read by nothing but a derive would be flagged dead under `-D
/// warnings`. Printing every field by hand reads them for real instead of
/// reaching for `#[allow(dead_code)]` -- `id` and `element` for the same
/// reason as G1, `source` and `ports` because they are new this task.
impl fmt::Debug for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single { id, ports } => f
                .debug_struct("Single")
                .field("id", id)
                .field("ports", ports)
                .finish(),
            Self::Each {
                id,
                source,
                element,
                ports,
            } => f
                .debug_struct("Each")
                .field("id", id)
                .field("source", source)
                .field("element", element)
                .field("ports", ports)
                .finish(),
        }
    }
}

/// One node's own bound ports, by name, as an [`Edge`] map -- milestone 3n,
/// decision (s1)'s `types()` view, computed once per node rather than
/// re-walked at every call site that wants it. Empty for a node with no
/// bound port, matching [`Checked::types`]' own "a node with no bound port
/// has no entry" (the caller, [`Checked::types`], is what turns an empty
/// map into a missing entry).
fn node_port_edges(node: &CheckedNode) -> IndexMap<PortName, Edge> {
    match &node.shape {
        Shape::Single { ports, .. } => ports
            .iter()
            .map(|port_edge| (port_edge.port.clone(), port_edge.bound.edge().clone()))
            .collect(),
        Shape::Each { ports, .. } => ports
            .iter()
            .map(|port_edge| (port_edge.port.clone(), port_edge.bound.edge().clone()))
            .collect(),
    }
}

/// `ports`' own entry for `port`, by name -- [`Checked::edge`]'s own
/// per-port lookup, shared between [`Shape::Single`] and [`Shape::Each`]
/// (whose `ports` differ only in `I`).
fn find_port_edge<'a, I>(ports: &'a [PortEdge<I>], port: &PortName) -> Option<&'a Edge> {
    ports
        .iter()
        .find(|port_edge| &port_edge.port == port)
        .map(|port_edge| port_edge.bound.edge())
}

/// A [`Shape::Single`] node's own id: an index into a `Vec<SingleResult>`
/// a later task gives `plan`'s and `apply`'s result tables (milestone 3n,
/// decision (g1)). Minted only by [`check`], in topological order among
/// `Single` nodes; never leaves `willikins-core`. A bounds-checked index,
/// not a proof -- see `check`'s own module doc, "Typed ids are indexes,
/// not proofs".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SingleId(pub(crate) u32);

/// A [`Shape::Each`] node's own id: an index into a `Vec<EachResult>` a
/// later task gives `plan`'s and `apply`'s result tables (milestone 3n,
/// decision (g1)). Minted only by [`check`], in topological order among
/// `Each` nodes; never leaves `willikins-core`. Also what a
/// [`crate::workflow::Binding::Keyed`] reference's `Source::Keyed` will
/// hold (G3), so "a keyed reference to a node with no `for_each`" has no
/// id to name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EachId(pub(crate) u32);

/// The item marker for a [`Source<I>`] that is **not** inside a `for_each`
/// node: milestone 3n, decision (g1)'s "Item marker" row. A unit type with
/// no values, so `Source::Item(Infallible)` can never be constructed --
/// unlike `std::convert::Infallible` itself, this crate owns it and can
/// derive whatever a generic `Source<I>` needs (`Debug`, `Clone`, `Copy`)
/// without reaching into `std`'s own impls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InEach;

/// A producing tool's output port, named by its position in that tool's own
/// `spec.outputs` rather than by [`PortName`] (milestone 3n, decision
/// (g1)): what a [`Source::Step`], [`Source::Aggregate`] or
/// [`Source::Keyed`] indexes a later task's `FilledOutputs` with, and what
/// [`crate::plan::FilledOutputs::slot`] (this crate's one other half) reads
/// back. Minted only by [`check`] (see [`output_slot`]), from the
/// producing tool's own `ToolSpec::outputs`, which is itself an `IndexMap`
/// -- so this is the same position [`Checked::types`]' SHARED VALUES row
/// already orders ports by, applied to a tool's *output* ports instead of
/// its input ones. A bounds-checked index, not a proof -- see `check`'s
/// own module doc, "Typed ids are indexes, not proofs".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OutputSlot(pub(crate) u32);

/// Where one bound port's (or `for_each` source's, or workflow output's)
/// value comes from, classified by `check` instead of left as a
/// [`crate::workflow::Binding`] for `plan`/`apply` to match on a second
/// time (milestone 3n, decision (g1)).
///
/// `I` is [`InEach`] for a [`Shape::Each`] node's own ports (where
/// `${{ item }}` is valid) and [`std::convert::Infallible`] everywhere
/// else (a [`Shape::Single`] node's ports, a `for_each` source itself, and
/// a workflow output) -- SHARED VALUES' "Item marker" row. Resolving a
/// `Source<I>` never needs to inspect `I`'s own value: whichever caller
/// holds the current `for_each` item (if any) supplies it directly: see
/// `crate::plan`'s resolver.
///
/// `Literal` and `Input` carry everything `plan`/`apply` need to resolve
/// them (a parsed [`Value`], or an index into [`Checked::inputs`]) with no
/// further lookup. `Step`, `Aggregate` and `Keyed` each carry the
/// producing node's own typed id, the output port's [`OutputSlot`], and
/// this reference's own resolved type (`ty` -- the same type
/// [`Resolver::resolve`] already returns, *before* whatever conversion the
/// consuming [`Bound::Scalar`]'s own `edge` applies): used to type a
/// [`Value::unknown`] when the producing node turns out to be
/// [`crate::plan::NodeResult::Skipped`], with no catalog lookup.
#[derive(Debug, Clone)]
pub(crate) enum Source<I> {
    /// A [`crate::workflow::Binding::Literal`], already parsed by `check`
    /// (`check_literal` now returns the [`Value`], not just its
    /// [`TypeRef`]) -- `plan` and `apply` deliver this same stored value
    /// and never re-parse it.
    Literal(Value),
    /// A [`crate::workflow::Binding::Input`]: a declared workflow input,
    /// named by its position in [`Checked::inputs`]. Can only ever name
    /// one of that table's own rows -- the 3d "undeclared input bound"
    /// route closed by type, not by an `unreachable!`.
    Input(InputId),
    /// A [`crate::workflow::Binding::Item`]: the current `for_each`
    /// instance's own item. Never constructed for `I` =
    /// [`std::convert::Infallible`] -- `check` already refuses
    /// `${{ item }}` used outside a `for_each` node
    /// ([`CheckError::ItemOutsideForEach`]) before any `Source` is built.
    Item(I),
    /// A [`crate::workflow::Binding::Step`] onto a [`Shape::Single`] node:
    /// that node's own output.
    Step {
        /// The producing node's own id.
        from: SingleId,
        /// The output port, by position.
        slot: OutputSlot,
        /// This reference's own resolved type.
        ty: TypeRef,
    },
    /// A [`crate::workflow::Binding::Step`] onto a [`Shape::Each`] node:
    /// every one of that node's instances, aggregated into a list (the
    /// module docs' "`for_each` aggregation", carried over unchanged).
    Aggregate {
        /// The producing node's own id.
        from: EachId,
        /// The output port, by position.
        slot: OutputSlot,
        /// This reference's own resolved type -- always `list<T>`.
        ty: TypeRef,
    },
    /// A [`crate::workflow::Binding::Keyed`] reference: one instance of a
    /// [`Shape::Each`] node, selected by its item's own canonical string.
    Keyed {
        /// The referenced `for_each` node's own id.
        from: EachId,
        /// The instance key to match -- not validated by `check` (the
        /// binding's own doc already says so); matched against each
        /// instance's own key at plan time, exactly as today.
        key: String,
        /// The output port, by position.
        slot: OutputSlot,
        /// This reference's own resolved type.
        ty: TypeRef,
    },
}

/// One bound port's classified source and the [`Edge`] `check` chose to
/// deliver it through (milestone 3n, decision (g1)). `List` is G4's: a
/// [`crate::workflow::Binding::List`] port's own elements, each classified
/// the same way a scalar port's single source is, aligned with `edge`'s own
/// [`Edge::elements`] (same length, same order -- `check` builds both
/// together, in [`build_port_edges`]).
#[derive(Debug, Clone)]
pub(crate) enum Bound<I> {
    /// Everything but a [`crate::workflow::Binding::List`]: the classified
    /// source, and the conversion (if any) `check` chose to deliver it to
    /// the port's own declared type.
    Scalar {
        /// Where the value comes from.
        source: Source<I>,
        /// The conversion (if any) from `source`'s own resolved type to
        /// the port's declared one. Exactly [`Checked::types`]' own entry
        /// for this port -- `check` records both, so forging one without
        /// the other (as the moved-edge backstop test now does, since G3)
        /// cannot happen by accident.
        edge: Edge,
    },
    /// A [`crate::workflow::Binding::List`] port: one classified source per
    /// element, in declaration order.
    List {
        /// Each element's own classified source, aligned with `edge`'s own
        /// [`Edge::elements`] -- same length, same order.
        elements: Vec<Source<I>>,
        /// The whole port's own list edge ([`Edge::list`]): `edge.ty()` is
        /// the declared element type, and `edge.elements()` is `Some`, one
        /// entry per `elements` above, in the same order.
        edge: Edge,
    },
}

impl<I> Bound<I> {
    /// This port's own [`Edge`], whichever variant it is -- `Scalar` and
    /// `List` each carry exactly one. `check`'s own public views
    /// ([`Checked::types`], [`Checked::edge`], from G7 on) read it through
    /// here instead of a parallel map built only for them.
    fn edge(&self) -> &Edge {
        match self {
            Self::Scalar { edge, .. } | Self::List { edge, .. } => edge,
        }
    }
}

/// One of a node's bound ports, in the producing tool's own input order
/// (milestone 3n, decision (g1)).
#[derive(Clone)]
pub(crate) struct PortEdge<I> {
    /// This port's own name.
    pub(crate) port: PortName,
    /// This port's position in the node's own authored `with:` map:
    /// milestone 3n's G5 sorts [`crate::plan::BlockedGate::awaiting_inputs`]
    /// by this field (SHARED VALUES), so a gate names its awaited inputs in
    /// `with:` order rather than the tool spec's own input order `ports`
    /// itself is stored in.
    pub(crate) with_position: usize,
    /// Where this port's value comes from, and how `check` delivers it.
    pub(crate) bound: Bound<I>,
}

/// Hand-written, not derived: a plain `#[derive(Debug)]` would still
/// compile (`with_position` is read for real as of G5, by
/// [`crate::plan`]'s `mark_blocked` -- see this type's own field doc), but
/// `rustc`'s dead-code analysis explicitly ignores a derived impl's own
/// field reads (`Shape`'s own hand-written `Debug`, G1, states the same
/// rule). Printing it by hand reads every field for real instead of
/// reaching for `#[allow(dead_code)]`.
impl<I: fmt::Debug> fmt::Debug for PortEdge<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortEdge")
            .field("port", &self.port)
            .field("with_position", &self.with_position)
            .field("bound", &self.bound)
            .finish()
    }
}

/// One declared workflow output, after `check` (milestone 3n, decision
/// (g1)): its classified source and its resolved type. `plan` and `apply`
/// resolve [`Checked::outputs`] into [`crate::plan::Plan::outputs`]
/// instead of walking [`Workflow::outputs`] a second time.
///
/// `source` is never [`Source::Literal`] or [`Source::Item`] in practice --
/// a literal workflow output is refused outright
/// ([`CheckError::LiteralOutput`]) before `check_outputs` ever resolves
/// one, and `${{ item }}` is resolved outside any `for_each` node's scope,
/// which already refuses it ([`CheckError::ItemOutsideForEach`]) -- but
/// `Source<Infallible>` is the same generic shape every other scalar
/// source uses, so there is no third enum to keep in sync by hand.
#[derive(Clone)]
pub(crate) struct CheckedOutput {
    /// The output's own declared name.
    pub(crate) name: OutputName,
    /// Where this output's value comes from.
    pub(crate) source: Source<Infallible>,
    /// This output's own resolved type -- exactly [`Checked::output_types`]'
    /// own entry for this output, which reads it straight off this field
    /// from G7 on ("views, not copies"), instead of a parallel stored map.
    pub(crate) ty: TypeRef,
}

impl fmt::Debug for CheckedOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckedOutput")
            .field("name", &self.name)
            .field("source", &self.source)
            .field("ty", &self.ty)
            .finish()
    }
}

/// One declared workflow input, after `check` (milestone 3n, decision
/// (g5)): its own [`InputSpec`], the catalog's registry entry its declared
/// type resolved to, and whether a caller leaving it unsupplied means
/// *awaited* rather than *missing* -- `check_input_types`, `describe` and
/// the input-resolution path (`crate::plan::resolve_binding`) all read
/// this instead of re-deriving the same two facts from `workflow.inputs`
/// and the registry a second time. Crate-private -- `InputId` positions
/// never leave `willikins-core` (SHARED VALUES).
///
/// `entry` is never looked up again by `describe`: `check` already refused
/// an unregistered or secret declared type
/// ([`CheckError::UnregisteredInputType`], [`CheckError::SecretWorkflowInput`])
/// before a `Checked` can exist, so every declared input's type is in the
/// catalog's registry by construction.
///
/// `Debug` is hand-written, not derived: [`willikins_types::registry::TypeEntry`]
/// itself has no `Debug` (it type-erases a parser and a `TypeId` matcher to
/// function pointers), so a derive would not compile. Prints the entry's
/// own type name, never the whole entry.
#[derive(Clone)]
pub(crate) struct CheckedInput {
    /// This input's declared name.
    pub(crate) name: InputName,
    /// This input's own declared shape -- including milestone 2b's
    /// `fixed_by`, which rides here unchanged, so 2b's R1 hiding (a fixed
    /// input is never settable, and never reported as missing) keeps
    /// working on this table exactly as it did on `workflow.inputs`.
    pub(crate) spec: InputSpec,
    /// The catalog's registry entry for [`Self::spec`]'s declared type.
    pub(crate) entry: &'static willikins_types::registry::TypeEntry,
    /// Whether this input's declared type is
    /// [`willikins_types::OperatorAcknowledgement`] -- computed once here,
    /// against the catalog's own registry, rather than asked of the
    /// global registry every time `describe` or `plan` resolves this
    /// input.
    pub(crate) awaited: bool,
}

impl fmt::Debug for CheckedInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckedInput")
            .field("name", &self.name)
            .field("spec", &self.spec)
            .field("entry", &self.entry.info.name)
            .field("awaited", &self.awaited)
            .finish()
    }
}

/// A declared workflow input's own id: its position in [`Checked::inputs`],
/// in declaration order (milestone 3n, decision (g1): "`InputId` =
/// index"). Minted only by [`check`], never leaves `willikins-core`. What a
/// [`crate::workflow::Binding::Input`] reference resolves to (G3's
/// `Source::Input(InputId)`), which closes the 3d "undeclared input bound"
/// route by type: an `InputId` can only ever name one of this table's own
/// rows. A bounds-checked index, not a proof -- see `check`'s own module
/// doc, "Typed ids are indexes, not proofs".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InputId(pub(crate) u32);

/// A read-only view of a [`Checked`] workflow's own declared surface: its
/// name, description, declared inputs and output names -- never its nodes.
/// Returned by [`Checked::document`]. Milestone 3n's decision (s1): a
/// composite's used document's nodes are already flattened into the one
/// graph `check` built (milestone 2b), so there is never a second
/// document's nodes to reach from here, and nothing in this crate's own
/// front ends (`plan`, `apply`, `describe`) needs them -- only a node's own
/// declared inputs and outputs, which this view exposes.
#[derive(Debug, Clone, Copy)]
pub struct DocumentView<'a> {
    workflow: &'a Workflow,
}

impl<'a> DocumentView<'a> {
    /// The workflow's own name.
    #[must_use]
    pub fn name(&self) -> &'a WorkflowName {
        &self.workflow.name
    }

    /// The workflow's own description, if it declared one.
    #[must_use]
    pub fn description(&self) -> Option<&'a Description> {
        self.workflow.description.as_ref()
    }

    /// Every input the workflow declares, in declaration order.
    #[must_use]
    pub fn inputs(&self) -> &'a IndexMap<InputName, InputSpec> {
        &self.workflow.inputs
    }

    /// The name of every output the workflow declares, in declaration
    /// order.
    pub fn output_names(&self) -> impl Iterator<Item = &'a OutputName> {
        self.workflow.outputs.keys()
    }
}

/// One checked data-flow edge into a node's input port: the binding's own
/// resolved type, and the conversion `check` chose to deliver it to the
/// port's type, if they differ.
///
/// `check` is the only constructor ([`Self::exact`] and [`Self::converted`]
/// are `pub(crate)`), so outside `willikins-core` an `Edge` can only come
/// from a real `check` -- there is no struct-literal or `Default` way to
/// forge one. (Milestone 3n's decision (s2): since [`Checked`]'s own
/// fields are private too, even an edge *moved onto the wrong port* now
/// takes code inside `check.rs` itself.) `plan` and `apply` read the
/// conversion from here (`willikins-core::value::Conversion`) and never
/// resolve one themselves; see [`crate::value::TypeRegistry::probe_conversion`]'s
/// own doc for why it is `check`'s alone to call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    kind: EdgeKind,
}

/// [`Edge`]'s two shapes: a scalar binding (everything before milestone
/// 3g), or a [`crate::workflow::Binding::List`]'s own edge, which is one
/// [`Edge`] per element (each itself a [`Self::Scalar`]) plus the port's
/// declared element type -- a list binding can convert element 0 through a
/// registered row while element 1 matches exactly, so one `Conversion`
/// cannot describe the whole port the way it can a scalar one.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EdgeKind {
    Scalar {
        ty: TypeRef,
        conversion: Option<Conversion>,
    },
    List {
        element_ty: TypeRef,
        elements: Vec<Edge>,
    },
}

impl Edge {
    /// A binding accepted with no conversion: the ordinary case, and the
    /// only one that existed before milestone 3d.
    pub(crate) fn exact(ty: TypeRef) -> Self {
        Self {
            kind: EdgeKind::Scalar {
                ty,
                conversion: None,
            },
        }
    }

    /// A binding accepted through a registered conversion: `ty` is the
    /// binding's own resolved type (the conversion's source), and
    /// `conversion` is the row `check` probed the registry for.
    pub(crate) fn converted(ty: TypeRef, conversion: Conversion) -> Self {
        Self {
            kind: EdgeKind::Scalar {
                ty,
                conversion: Some(conversion),
            },
        }
    }

    /// A [`crate::workflow::Binding::List`]'s own edge: `element_ty` is the
    /// port's declared scalar element type `T`, and `elements` is one
    /// [`Self::exact`]/[`Self::converted`] edge per list element, in
    /// declaration order. `plan` and `apply` resolve and deliver each
    /// element through its own entry, then assemble the list — see
    /// [`Self::elements`].
    pub(crate) fn list(element_ty: TypeRef, elements: Vec<Edge>) -> Self {
        Self {
            kind: EdgeKind::List {
                element_ty,
                elements,
            },
        }
    }

    /// The binding's own resolved type: exactly what [`Checked::types`]
    /// held before milestone 3d, and the conversion's source when
    /// [`Self::conversion`] is `Some`. For a [`EdgeKind::List`] edge, its
    /// declared element type `T` — elements may individually convert or
    /// not, so no single source type describes the whole list; see
    /// [`Self::elements`] for the per-element detail.
    #[must_use]
    pub fn ty(&self) -> &TypeRef {
        match &self.kind {
            EdgeKind::Scalar { ty, .. } => ty,
            EdgeKind::List { element_ty, .. } => element_ty,
        }
    }

    /// The conversion `check` chose to deliver this binding to the port's
    /// type, or `None` when the binding's own type already matched it
    /// exactly, or when this is a [`EdgeKind::List`] edge (whose
    /// conversions, if any, are per element; see [`Self::elements`]).
    #[must_use]
    pub fn conversion(&self) -> Option<&Conversion> {
        match &self.kind {
            EdgeKind::Scalar { conversion, .. } => conversion.as_ref(),
            EdgeKind::List { .. } => None,
        }
    }

    /// The type the port actually receives: [`Self::ty`] unchanged, or the
    /// conversion's target when one applies, carrying [`Self::ty`]'s own
    /// list-ness (a conversion is defined over scalars only; see
    /// `docs/plans/2026-09-23-milestone-3d-conversions.md`, decision (e)).
    /// For a [`EdgeKind::List`] edge, `list<T>`.
    #[must_use]
    pub fn delivered(&self) -> TypeRef {
        match &self.kind {
            EdgeKind::Scalar { ty, conversion } => match conversion {
                None => ty.clone(),
                Some(conversion) => TypeRef {
                    name: conversion.to().clone(),
                    list: ty.list,
                },
            },
            EdgeKind::List { element_ty, .. } => TypeRef::list_of(element_ty.name.clone()),
        }
    }

    /// This edge's own per-element edges, when it is a
    /// [`EdgeKind::List`] edge built by [`Self::list`] — `None` for a
    /// scalar edge. `plan` and `apply` resolve a
    /// [`crate::workflow::Binding::List`] element by element through
    /// these, in order, rather than through [`Self::deliver`] (which a
    /// list edge never receives a whole list [`Value`] to call). Public
    /// for the same reason [`Self::ty`] and [`Self::conversion`] are: a
    /// test (or a future introspection caller) can read exactly what
    /// `check` recorded for each element.
    #[must_use]
    pub fn elements(&self) -> Option<&[Edge]> {
        match &self.kind {
            EdgeKind::List { elements, .. } => Some(elements),
            EdgeKind::Scalar { .. } => None,
        }
    }

    /// Apply this edge to a value resolved for it: `value` unchanged when
    /// the edge carries no conversion (every edge of every document that
    /// predates milestone 3d, which is what keeps their plans
    /// byte-identical), else [`Value::converted`] through the conversion
    /// `check` recorded. `plan` and `apply` reach this only through their
    /// shared resolution context, and never look a conversion up
    /// themselves.
    ///
    /// A [`EdgeKind::List`] edge always passes `value` through unchanged:
    /// `plan` and `apply` never call this on one — they deliver each
    /// element through [`Self::elements`] instead — so this exists only as
    /// a safe default for a hand-built [`Checked`] that did.
    ///
    /// # Errors
    ///
    /// Returns [`ConversionMismatch`] when `value` is not, in fact, of this
    /// edge's own source type -- unreachable for an edge `check` itself
    /// built and a binding `plan`/`apply` resolved normally, but reachable
    /// through a hand-built `Checked` moving a converted edge onto another
    /// port (`Checked::types`' fields are public). `plan` and `apply`
    /// surface this loudly, as [`crate::plan::PlanError::EdgeTypeMismatch`]
    /// naming the node and port, never a panic and never the value's own
    /// content (follow-up to milestone 3d, 2026-09-24).
    pub fn deliver(&self, value: Value) -> Result<Value, ConversionMismatch> {
        match &self.kind {
            EdgeKind::Scalar { conversion, .. } => match conversion {
                None => Ok(value),
                Some(conversion) => value.converted(conversion),
            },
            EdgeKind::List { .. } => Ok(value),
        }
    }
}

/// A non-fatal observation returned alongside a successful [`check`].
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`): every variant is
/// struct-like or unit, so the tag merges into the variant's own fields
/// rather than failing at run time the way an internally tagged newtype
/// variant would. No variant declares a field named `kind` or `message`,
/// which would otherwise collide with the tag or with
/// [`crate::Reported`]'s own added field; see
/// `tests::every_check_warning_variant_serializes_with_its_kind`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind")]
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
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`): every variant is
/// struct-like, so the tag merges into the variant's own fields rather than
/// failing at run time the way an internally tagged newtype variant (or one
/// whose payload is not a map) would. No variant declares a field named
/// `kind` or `message`; see
/// `tests::every_check_error_variant_serializes_with_its_kind`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind")]
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
    /// its own node's tool's input ports (`site` is the binding's own
    /// location); and a `Step` or `Keyed` binding naming an output port
    /// that does not exist on the node it references (`site` then names
    /// the *referenced* node and the missing output port, not the binding's
    /// own location).
    UnknownPort {
        /// The node the missing port was looked up on, and the port name
        /// that does not exist there.
        site: Site,
        /// That node's tool.
        tool: ToolName,
    },
    /// A `Step`, `Keyed`, or `for_each` binding names a node that is not
    /// in the workflow.
    UnknownNode {
        /// The binding's own location: the node and port (or `for_each`
        /// binding, or workflow output) that contains the bad reference.
        site: Site,
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
        /// The binding's own location: the node and port (or `for_each`
        /// binding, or workflow output) that references the undeclared
        /// input.
        site: Site,
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
    /// A [`crate::tool::PortSpec::derived_only`] port was bound to
    /// anything other than the output of an earlier, non-pure node: a
    /// literal, a workflow input, a `for_each` item, or a pure node's
    /// output all report this. `doppler.secret.set`'s `config` port is
    /// the first user — see that field's own doc comment for why a pure
    /// node's output is refused too.
    UnderivedBinding {
        /// The node whose binding is not a derived one.
        node: NodeName,
        /// The `derived_only` port it was bound to.
        port: PortName,
    },
    /// A secret value, read from another node's output, was bound to a
    /// port that does not accept secrets. Takes precedence over
    /// [`Self::TypeMismatch`] for the same edge.
    SecretToNonSecretSink {
        /// The node and output port the secret value came from.
        from: (NodeName, PortName),
        /// The binding's own location that it flowed into.
        to: Site,
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
    /// A `for_each` binding resolved to a list whose element type is
    /// marked `#[domain(identifier)]` in the catalog's registry
    /// (`entry.info.identifier`).
    ///
    /// A `for_each` instance key is a rendered string, and also the exact
    /// text a [`crate::workflow::Binding::Keyed`] reference matches
    /// against, so it cannot be masked without breaking keyed references;
    /// it also cannot be printed whole without breaking milestone 3i's
    /// promise that no document route ever prints an identifier-typed
    /// value unmasked. Refusing is the only honest answer. Checked before
    /// [`Self::DuplicateForEachDefault`] -- see the module docs' "Error
    /// ordering" -- because that error's own `key` field would itself be
    /// a rendered identifier here.
    ///
    /// Not one of the plan's variants; milestone 3n, decision (b1).
    /// Replaces milestone 3i's own document-level guard test, which
    /// searched shipped documents for exactly this shape; `check` now
    /// refuses it outright.
    IdentifierForEachSource {
        /// The node whose `for_each` source is identifier-typed.
        node: NodeName,
        /// The source's resolved `list<T>` type.
        ty: TypeRef,
    },
    /// A `Binding::Item` was used outside any `for_each` node.
    ItemOutsideForEach {
        /// The binding's own location the `item` binding was found in.
        site: Site,
    },
    /// A `Binding::Keyed` referenced a node that has no `for_each`.
    KeyedOnScalarNode {
        /// The binding's own location that contains the keyed reference.
        site: Site,
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
    /// type, in name or in cardinality, or holds an object that is not a
    /// value of the Rust type the catalog's registry holds under that name
    /// (the registry entry's `TypeId` test,
    /// [`TypeRegistry::type_matches`]; a default's objects are known
    /// statically, so `check` refuses one of another Rust type sharing the
    /// declared name, not only `plan`).
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
        /// The default value's own type; for an object of another Rust
        /// type under the declared name, the name that object reports (the
        /// declared name when it is not a valid type name), so `found`
        /// can equal `expected`.
        found: TypeRef,
    },
    /// A `Step` binding on a `for_each` node whose output port is already
    /// list-typed. `Step` on a `for_each` node yields one value per
    /// instance, which would be a `list<list<T>>`; [`TypeRef`] carries a
    /// single cardinality flag and cannot represent one.
    ///
    /// Not one of the plan's sixteen variants; see the module docs.
    NestedList {
        /// The binding's own location that asks for the promotion.
        site: Site,
        /// The `for_each` node whose output port is already a list.
        referenced: NodeName,
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
    /// A workflow input's declared type is not in the type registry at
    /// all — neither secret nor non-secret, because the registry has never
    /// heard of the name. Reported alongside [`Self::SecretWorkflowInput`],
    /// in input declaration order.
    UnregisteredInputType {
        /// The offending input.
        input: InputName,
        /// Its unregistered declared type.
        ty: TypeRef,
    },
    /// A `for_each` source is a workflow input whose declared *default*
    /// holds two items with the same canonical string.
    ///
    /// `plan` expands a `for_each` node into one instance per item, keyed
    /// by that string, and refuses a collision
    /// (`PlanError::DuplicateForEachKey`) because the instances would be
    /// indistinguishable. A default is known statically, so a document
    /// whose own defaults cannot run is reported here rather than only at
    /// plan time -- the same reason [`Self::DefaultTypeMismatch`] exists,
    /// and unaffected by whether a caller could override the input.
    ///
    /// Not one of the plan's variants; see the module docs.
    DuplicateForEachDefault {
        /// The `for_each` node whose source is the offending input.
        node: NodeName,
        /// The input whose default holds the collision.
        input: InputName,
        /// The canonical string two of its default's items share.
        key: String,
    },
    /// A workflow output was bound to a literal rather than a reference.
    ///
    /// A `with` literal is parsed against the port it is bound to; a
    /// workflow output has no port, so there is nothing to parse it
    /// against and no honest type to record for it. Accepting one meant
    /// [`Checked::output_types`] — and, downstream, `Plan::outputs` —
    /// silently omitted an output the document declares, so `plan`
    /// reported a smaller output surface than the workflow has.
    ///
    /// Not one of the plan's variants; see the module docs.
    LiteralOutput {
        /// The output whose binding is a literal.
        output: OutputName,
    },
    /// A workflow input declared type
    /// [`willikins_types::OperatorAcknowledgement`] has a default value.
    /// This type is never defaulted: it exists so a gate can ask the
    /// operator, and a default would let it pass without anyone acting.
    /// See `docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, decision
    /// (j), point 6.
    ///
    /// Not one of the plan's variants; a G3 addition, recognising the type
    /// by its registry entry's `TypeId`
    /// ([`crate::value::is_operator_acknowledgement`]), never by comparing
    /// [`willikins_types::DomainType::TYPE_NAME`] strings (the milestone 3d
    /// rule).
    AcknowledgementDefault {
        /// The offending input.
        input: InputName,
    },
    /// A `Binding::Literal` bound to a port whose type is
    /// [`willikins_types::OperatorAcknowledgement`]. A document cannot
    /// supply this on its own: it must come from a workflow input the
    /// operator fills in. Reported before the literal is ever handed to a
    /// parser, exactly like [`Self::SecretLiteral`].
    ///
    /// Not one of the plan's variants; a G3 addition, see
    /// [`Self::AcknowledgementDefault`].
    AcknowledgementLiteral {
        /// The node whose binding is the literal.
        node: NodeName,
        /// The port it was bound to.
        port: PortName,
    },
    /// A [`crate::workflow::Binding::List`] (a `with:` sequence) was bound
    /// to a port whose type is not `list<T>` for any `T` -- a scalar port,
    /// or a secret-accepting [`PortType::AnySecret`] port, which never
    /// names a single element type to check a list against.
    ///
    /// Not one of the plan's variants; milestone 3g, decision (a).
    ListOnScalarPort {
        /// The binding's own location.
        site: Site,
        /// The port's declared type.
        expected: PortType,
    },
    /// One element of a [`crate::workflow::Binding::List`] did not
    /// type-check against the port's declared element type: it resolved to
    /// a list itself (no flattening — a list element is always a scalar),
    /// or its scalar type did not match and no one-hop conversion applies
    /// either.
    ///
    /// Not one of the plan's variants; milestone 3g, decision (a).
    ListElementTypeMismatch {
        /// The offending element's own location
        /// ([`Site::ListElement`]).
        site: Site,
        /// The port's declared element type.
        expected: TypeRef,
        /// The element's resolved type.
        found: TypeRef,
    },
    /// A [`crate::workflow::Binding::List`] (a `with:` sequence) was found
    /// where only a reference or a literal is valid: a `for_each` source,
    /// or a workflow output. Never produced by `willikins-dsl`, whose
    /// `for_each:` and `outputs:` fields are plain strings and so cannot
    /// hold a sequence at all; exists so `check` still refuses one reaching
    /// either site through a hand-built [`Workflow`].
    ///
    /// Not one of the plan's variants; milestone 3g, decision (a).
    SequenceNotAllowedHere {
        /// The binding's own location.
        site: Site,
    },
    /// A workflow input's declared type is
    /// [`willikins_types::TemplateSource`] or [`willikins_types::RepoFile`]
    /// -- recognised by the registry entry's own `TypeId` test
    /// ([`crate::value::is_template_source`], [`crate::value::is_repo_file`]),
    /// never by comparing [`willikins_types::DomainType::TYPE_NAME`]
    /// strings (the milestone 3d rule). Reported whether or not the input
    /// also carries a default -- checking further would only repeat this
    /// same root cause -- so this doubles as "a default of either type",
    /// milestone 3g decision (e)'s second refusal.
    ///
    /// Templates and rendered files are privileged, trusted-ref document
    /// content (the design doc's Trust model): a workflow input is exactly
    /// how a caller supplies a value, so neither type may ever be one. A
    /// `TemplateSource` is instead always a document literal, bound
    /// straight to `repo.file.render.template`; a `RepoFile` is produced
    /// only by that same tool.
    ///
    /// Not one of the plan's variants; milestone 3g, decision (e).
    DisallowedInputType {
        /// The offending input.
        input: InputName,
        /// Its disallowed declared type.
        ty: TypeRef,
    },
    /// A `Binding::Literal` bound to a port whose type is
    /// [`willikins_types::RepoFile`]. Reported before the literal is ever
    /// handed to a parser, exactly like [`Self::SecretLiteral`] and
    /// [`Self::AcknowledgementLiteral`].
    ///
    /// The only producer of a `RepoFile` is `repo.file.render`: a document
    /// literal that happens to parse in `RepoFile`'s own canonical
    /// `<path>\n<content>` form must still never reach a sink that writes
    /// it, so this is refused by type regardless of what the literal's
    /// text actually says. Unlike [`willikins_types::TemplateSource`],
    /// whose literals are the intended route into `repo.file.render`'s own
    /// `template` port, a `RepoFile` has no legitimate literal at all.
    ///
    /// Not one of the plan's variants; milestone 3g, decision (e).
    RepoFileLiteral {
        /// The node whose binding is the literal.
        node: NodeName,
        /// The port it was bound to.
        port: PortName,
    },
    /// A `uses:` step on [`Workflow::uses`] reached [`check`] without
    /// first going through the linker (milestone 2b,
    /// `willikins_core::compose::link`). The first thing [`check`] does:
    /// see the module docs' "Error ordering", step 0.
    ///
    /// Not one of the plan's variants; milestone 2b, part P.
    Unlinked {
        /// The `uses:` step that was never linked.
        node: NodeName,
    },
    /// The linker (`willikins_core::compose::link`) found a `uses:` step
    /// whose own target workflow is already on the depth-first stack of
    /// workflows being resolved -- a self-use (one entry) or a cycle
    /// across two or more documents. `chain` is the cycle itself, from
    /// the first occurrence of the repeated name to the repeat (milestone
    /// 2b decision (d9)), so `[a, b, a]` reads as "`a` uses `b`, which
    /// uses `a` again".
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2.
    UsesCycle {
        /// The cycle, from its first occurrence to its repeat.
        chain: Vec<WorkflowName>,
    },
    /// A `uses:` step was found in a document already at
    /// [`crate::compose::MAX_USES_DEPTH`] (root is depth 0): resolving it
    /// would nest a document one level deeper than the linker allows.
    /// `chain` is every workflow name from the root down to (and
    /// including) the document holding the refused step, so its length
    /// is always [`crate::compose::MAX_USES_DEPTH`] `+ 1`.
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d9).
    UsesTooDeep {
        /// The nesting chain, from the root to the document that held
        /// the refused `uses:` step.
        chain: Vec<WorkflowName>,
    },
    /// Linking produced more than [`crate::compose::MAX_LINKED_NODES`]
    /// tool nodes (counted before any `for_each` expansion). Raised as
    /// soon as the running total crosses the bound -- while the document
    /// that pushed it over is being entered, before any of *its own*
    /// `uses:` steps are resolved -- so an exponential diamond is refused
    /// long before it would be fully materialised; see the precedent in
    /// `docs/research/2026-09-12-e2e-adversarial-pass-2.md`.
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d9).
    UsesTooLarge {
        /// The running node count at the moment the bound was crossed
        /// (always greater than [`crate::compose::MAX_LINKED_NODES`]).
        nodes: usize,
    },
    /// A `uses:` step's own workflow could not be found at all: the
    /// linker's `resolve` callback returned
    /// [`crate::compose::ResolveFailure::NotFound`] or
    /// [`crate::compose::ResolveFailure::Refused`] (a symlink and a
    /// name/file mismatch deliberately share this one variant -- see
    /// that type's own docs).
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2.
    UnknownWorkflow {
        /// The `uses:` step whose target could not be found.
        node: NodeName,
        /// The workflow name it named.
        workflow: WorkflowName,
    },
    /// A `uses:` step's own workflow was found and read, but failed to
    /// parse or validate on its own terms
    /// ([`crate::compose::ResolveFailure::Document`]).
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2.
    /// The plan's SHARED VALUES table names this variant's third field
    /// `message`; it is `reason` instead, because no `CheckError`
    /// variant may have a field literally named `message` (it would
    /// collide with [`crate::Reported`]'s own added field --
    /// `tests::every_check_error_variant_serializes_with_its_kind`
    /// enforces this for every existing variant). See this plan's
    /// addendum.
    UsedDocument {
        /// The `uses:` step whose target failed.
        node: NodeName,
        /// The workflow name it named.
        workflow: WorkflowName,
        /// Why the document itself was refused.
        reason: String,
    },
    /// A `uses:` step's `with:` names an input the used document never
    /// declared.
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d4).
    UnknownUsesInput {
        /// The `uses:` step.
        node: NodeName,
        /// The undeclared input name it bound.
        input: InputName,
    },
    /// A used document's required input (no default) was left both
    /// unbound by the `uses:` step's `with:` and -- since it has no
    /// default -- could not become a fixed input either.
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d4).
    UnboundUsesInput {
        /// The `uses:` step.
        node: NodeName,
        /// The required input left unbound.
        input: InputName,
    },
    /// A reference to `${{ steps.<uses step>.<output> }}` names an
    /// output the used document never declared.
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d7).
    UnknownUsesOutput {
        /// Where the bad reference was found: the referencing node's own
        /// port, `for_each` binding, or the workflow output -- or, when
        /// the reference sits inside another `uses:` step's `with:`,
        /// [`Site::Port`] with that `uses:` step as `node` and the
        /// child input (reparsed as a [`crate::tool::PortName`], which
        /// shares [`InputName`]'s grammar) as `port`.
        site: Site,
        /// The `uses:` step the reference named.
        node: NodeName,
        /// The output name it named, which the used document never
        /// declared.
        output: OutputName,
    },
    /// A `uses:` step's `with:` bound a child input to `${{ item }}`
    /// (bare, or inside a list element): after substitution this would
    /// silently rebind to whichever `for_each` node inside the child
    /// happens to consume that input, which is never what the document
    /// meant.
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d4).
    ItemInUses {
        /// The `uses:` step.
        node: NodeName,
        /// The input bound to `item`.
        input: InputName,
    },
    /// A [`Binding::Keyed`] reference named a `uses:` step: there are no
    /// instances to key into (a `uses:` step may never have a
    /// `for_each`, decision (d12)).
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d7).
    KeyedOnUses {
        /// Where the bad reference was found (see [`Self::UnknownUsesOutput`]'s
        /// own doc for the "inside another `uses:` step's `with:`" case).
        site: Site,
        /// The `uses:` step that was keyed into.
        node: NodeName,
    },
    /// Two or more `uses:` steps' outputs alias into each other with no
    /// node in between, so the node-level cycle check never sees the
    /// loop (decision (d7)'s worked example: `a.out = inputs.x`, `x`
    /// bound to `steps.b.out`, `b.out = inputs.y`, `y` bound to
    /// `steps.a.out`). The linker resolves aliases with a visiting set
    /// and refuses the loop the moment it would re-enter a `uses:` step
    /// still being resolved.
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d7).
    UsesOutputCycle {
        /// The `uses:` step the linker was about to re-enter.
        node: NodeName,
        /// The output it was being asked to read when the loop closed.
        output: OutputName,
    },
    /// A workflow handed to the linker (root or a resolved child) has a
    /// `/` in one of its own authored names -- a node, an input, or a
    /// `uses:` step key. An authored name is always one segment; only
    /// the linker itself ever produces a `/`-separated path
    /// (`willikins-dsl` already refuses one at parse time, decision
    /// (d3)), so this protects a hand-built [`Workflow`], which never
    /// goes through the DSL.
    ///
    /// Not one of the plan's variants; milestone 2b, part L, task L2,
    /// decision (d3).
    PathInAuthoredName {
        /// The offending name, verbatim.
        name: String,
    },
    /// A `uses:` step's `with:` bound a used document's declared input to
    /// a value whose own resolved type is not *exactly* the input's
    /// declared type: `check` resolves the binding the same way it
    /// resolves a node's own port binding, then requires equality with
    /// [`crate::compose::Boundary::spec`]'s declared type, with no
    /// one-hop conversion probe (decision (d4), "exact type at the
    /// boundary, no conversion" -- substitution moves the *unconverted*
    /// binding inside, so a converted-at-the-boundary value would reach
    /// the child's own ports unconverted).
    ///
    /// Not one of the plan's variants; milestone 2b, part C, task C1,
    /// decision (d4).
    UsesInputTypeMismatch {
        /// The `uses:` step.
        node: NodeName,
        /// The used document's own input name the binding does not
        /// match.
        input: InputName,
        /// The input's declared type.
        expected: TypeRef,
        /// The binding's own resolved type.
        found: TypeRef,
    },
}

impl fmt::Display for CheckError {
    #[allow(clippy::too_many_lines)] // one arm per variant; splitting it would only move the count, not reduce it
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTool { node, tool } => {
                write!(f, "node `{node}`: unknown tool `{tool}`")
            }
            Self::UnknownPort { site, tool } => {
                write!(f, "{site} (tool `{tool}`): no such port")
            }
            Self::UnknownNode { site, referenced } => {
                write!(f, "{site}: references unknown node `{referenced}`")
            }
            Self::UnboundInput { node, port } => {
                write!(
                    f,
                    "node `{node}`, port `{port}`: required input is not bound"
                )
            }
            Self::UndeclaredInput { site, input } => {
                write!(f, "{site}: references undeclared input `{input}`")
            }
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
                "node `{node}`, port `{port}`: expected {expected}, found `{found}`"
            ),
            Self::SecretLiteral { node, port } => write!(
                f,
                "node `{node}`, port `{port}`: a literal cannot supply a secret value"
            ),
            Self::UnderivedBinding { node, port } => write!(
                f,
                "node `{node}`, port `{port}`: this port may only be bound to the output of an \
                 earlier, non-pure node — never a literal, a workflow input, a for_each item, \
                 or a pure node's output"
            ),
            Self::SecretToNonSecretSink { from, to } => write!(
                f,
                "secret value from node `{}`, port `{}` flows into non-secret sink at `{to}`",
                from.0, from.1
            ),
            Self::SecretForEachSource { node } => {
                write!(f, "node `{node}`: for_each source is secret")
            }
            Self::ForEachOverScalar { node } => {
                write!(f, "node `{node}`: for_each source is not a list")
            }
            Self::IdentifierForEachSource { node, ty } => write!(
                f,
                "node `{node}`: for_each over `{ty}`, an identifier type: its instance keys would print identifiers whole"
            ),
            Self::ItemOutsideForEach { site } => {
                write!(f, "{site}: `item` is only valid inside a for_each node")
            }
            Self::KeyedOnScalarNode { site, referenced } => write!(
                f,
                "{site}: keyed reference to node `{referenced}`, which has no for_each"
            ),
            Self::Cycle { nodes } => {
                let joined = nodes
                    .iter()
                    .map(|node| format!("`{node}`"))
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "cycle among nodes: {joined}")
            }
            Self::DuplicateForEachDefault { node, input, key } => write!(
                f,
                "node `{node}`: input `{input}`'s default has two items both keyed `{key}`; for_each instances must be distinguishable"
            ),
            Self::NestedList { site, referenced } => write!(
                f,
                "{site}: node `{referenced}` runs once per item and its port is already a list; there is no list-of-list type"
            ),
            Self::AcknowledgementLiteral { node, port } => write!(
                f,
                "node `{node}`, port `{port}`: a literal cannot supply an operator acknowledgement"
            ),
            Self::RepoFileLiteral { node, port } => write!(
                f,
                "node `{node}`, port `{port}`: a literal cannot supply a repository file"
            ),
            Self::Unlinked { node } => write!(
                f,
                "node `{node}`: a `uses:` step reached check without being linked first"
            ),
            Self::UsesCycle { chain } => {
                let joined = chain
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "uses cycle: {joined}")
            }
            Self::UsesTooDeep { chain } => {
                let joined = chain
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "uses nesting too deep: {joined}")
            }
            Self::UsesTooLarge { nodes } => {
                write!(
                    f,
                    "linked graph has {nodes} nodes, more than the allowed maximum"
                )
            }
            Self::UnknownWorkflow { node, workflow } => {
                write!(f, "node `{node}`: unknown workflow `{workflow}`")
            }
            Self::UsedDocument {
                node,
                workflow,
                reason,
            } => write!(
                f,
                "node `{node}`: workflow `{workflow}` failed to load: {reason}"
            ),
            Self::UnknownUsesInput { node, input } => {
                write!(f, "node `{node}`: `with:` names undeclared input `{input}`")
            }
            Self::UnboundUsesInput { node, input } => write!(
                f,
                "node `{node}`: required input `{input}` is not bound and has no default"
            ),
            Self::UnknownUsesOutput { site, node, output } => {
                write!(f, "{site}: node `{node}` has no declared output `{output}`")
            }
            Self::ItemInUses { node, input } => write!(
                f,
                "node `{node}`: input `{input}` cannot be bound to `item` inside a `uses:` step"
            ),
            Self::KeyedOnUses { site, node } => {
                write!(
                    f,
                    "{site}: node `{node}` is a uses: step, not a for_each node"
                )
            }
            Self::UsesOutputCycle { node, output } => write!(
                f,
                "node `{node}`, output `{output}`: uses output alias cycle"
            ),
            Self::PathInAuthoredName { name } => {
                write!(f, "name `{name}`: an authored name may not contain `/`")
            }
            Self::UsesInputTypeMismatch {
                node,
                input,
                expected,
                found,
            } => write!(
                f,
                "node `{node}`, input `{input}`: expected {expected}, found `{found}`"
            ),
            Self::ListOnScalarPort { site, expected } => write!(
                f,
                "{site}: a list binding cannot be delivered to a port of type {expected}"
            ),
            Self::ListElementTypeMismatch {
                site,
                expected,
                found,
            } => write!(f, "{site}: expected {expected}, found `{found}`"),
            Self::SequenceNotAllowedHere { site } => write!(
                f,
                "{site}: a list binding is only valid bound to a node's with-port; not here"
            ),
            // Errors about a *declaration* rather than a node's port: see
            // `fmt_declaration_error`. Listed explicitly so this match
            // stays exhaustive and a new variant is still a compile error.
            Self::SecretWorkflowInput { .. }
            | Self::DefaultTypeMismatch { .. }
            | Self::UnregisteredInputType { .. }
            | Self::DuplicateNode { .. }
            | Self::LiteralOutput { .. }
            | Self::AcknowledgementDefault { .. }
            | Self::DisallowedInputType { .. } => self.fmt_declaration_error(f),
        }
    }
}

impl CheckError {
    /// Render the errors that name a declaration -- a workflow input, a
    /// node name, or a workflow output -- rather than a node's port.
    /// Split out of [`fmt::Display`] so neither match runs long.
    fn fmt_declaration_error(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecretWorkflowInput { input, ty } => write!(
                f,
                "input `{input}` has secret type `{ty}`; a workflow input may not be secret"
            ),
            Self::DefaultTypeMismatch {
                input,
                expected,
                found,
            } if expected == found => write!(
                f,
                "input `{input}`: default value is declared `{found}` but is a value of another \
                 Rust type"
            ),
            Self::DefaultTypeMismatch {
                input,
                expected,
                found,
            } => write!(
                f,
                "input `{input}`: default value has type `{found}`, expected `{expected}`"
            ),
            Self::UnregisteredInputType { input, ty } => write!(
                f,
                "input `{input}`: declared type `{ty}` is not a registered type"
            ),
            Self::DuplicateNode { node } => write!(f, "duplicate node name `{node}`"),
            Self::LiteralOutput { output } => write!(
                f,
                "output `{output}`: a workflow output must be a reference, not a literal; there is no port to give a literal a type"
            ),
            Self::AcknowledgementDefault { input } => write!(
                f,
                "input `{input}`: an operator acknowledgement input may not have a default"
            ),
            Self::DisallowedInputType { input, ty } => write!(
                f,
                "input `{input}`: declared type `{ty}` may never be a workflow input or a default; it is privileged, trusted-ref document content"
            ),
            other => unreachable!("not a declaration error: {other:?}"),
        }
    }

    /// This variant's name, exactly as its `Serialize` impl's `kind` tag
    /// renders it (pinned by
    /// `tests::kind_agrees_with_the_serialized_tag_for_every_variant`).
    ///
    /// A text renderer that wants the variant name on its own -- the
    /// leading `VariantName:` in `willikins-cli`'s `check_error_line`,
    /// say -- would otherwise have to either hand-maintain its own
    /// name-per-variant table (drifting from this enum the moment a
    /// variant is renamed or added, since nothing but a human keeps two
    /// such lists in step) or serialize to JSON and read `"kind"` back out
    /// just to get a string it already knows at the `match` site. This is
    /// the third option: `willikins-core`'s own enum names its own
    /// variants, once.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnknownTool { .. } => "UnknownTool",
            Self::UnknownPort { .. } => "UnknownPort",
            Self::UnknownNode { .. } => "UnknownNode",
            Self::UnboundInput { .. } => "UnboundInput",
            Self::UndeclaredInput { .. } => "UndeclaredInput",
            Self::InvalidLiteral { .. } => "InvalidLiteral",
            Self::TypeMismatch { .. } => "TypeMismatch",
            Self::SecretLiteral { .. } => "SecretLiteral",
            Self::UnderivedBinding { .. } => "UnderivedBinding",
            Self::SecretToNonSecretSink { .. } => "SecretToNonSecretSink",
            Self::SecretWorkflowInput { .. } => "SecretWorkflowInput",
            Self::SecretForEachSource { .. } => "SecretForEachSource",
            Self::ForEachOverScalar { .. } => "ForEachOverScalar",
            Self::IdentifierForEachSource { .. } => "IdentifierForEachSource",
            Self::ItemOutsideForEach { .. } => "ItemOutsideForEach",
            Self::KeyedOnScalarNode { .. } => "KeyedOnScalarNode",
            Self::Cycle { .. } => "Cycle",
            Self::DefaultTypeMismatch { .. } => "DefaultTypeMismatch",
            Self::NestedList { .. } => "NestedList",
            Self::DuplicateNode { .. } => "DuplicateNode",
            Self::UnregisteredInputType { .. } => "UnregisteredInputType",
            Self::DuplicateForEachDefault { .. } => "DuplicateForEachDefault",
            Self::LiteralOutput { .. } => "LiteralOutput",
            Self::AcknowledgementDefault { .. } => "AcknowledgementDefault",
            Self::AcknowledgementLiteral { .. } => "AcknowledgementLiteral",
            Self::ListOnScalarPort { .. } => "ListOnScalarPort",
            Self::ListElementTypeMismatch { .. } => "ListElementTypeMismatch",
            Self::SequenceNotAllowedHere { .. } => "SequenceNotAllowedHere",
            Self::DisallowedInputType { .. } => "DisallowedInputType",
            Self::RepoFileLiteral { .. } => "RepoFileLiteral",
            Self::Unlinked { .. } => "Unlinked",
            Self::UsesCycle { .. } => "UsesCycle",
            Self::UsesTooDeep { .. } => "UsesTooDeep",
            Self::UsesTooLarge { .. } => "UsesTooLarge",
            Self::UnknownWorkflow { .. } => "UnknownWorkflow",
            Self::UsedDocument { .. } => "UsedDocument",
            Self::UnknownUsesInput { .. } => "UnknownUsesInput",
            Self::UnboundUsesInput { .. } => "UnboundUsesInput",
            Self::UnknownUsesOutput { .. } => "UnknownUsesOutput",
            Self::ItemInUses { .. } => "ItemInUses",
            Self::KeyedOnUses { .. } => "KeyedOnUses",
            Self::UsesOutputCycle { .. } => "UsesOutputCycle",
            Self::PathInAuthoredName { .. } => "PathInAuthoredName",
            Self::UsesInputTypeMismatch { .. } => "UsesInputTypeMismatch",
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
    // Step 0 (module docs' "Error ordering"): a `uses:` step that reached
    // `check` unlinked is always a caller defect -- a composite must go
    // through the linker first. Short-circuit before anything else is
    // evaluated, rather than mixing `Unlinked` into a wall of errors about
    // a workflow whose nodes do not even exist yet.
    if !workflow.uses.is_empty() {
        return Err(workflow
            .uses
            .keys()
            .cloned()
            .map(|node| CheckError::Unlinked { node })
            .collect());
    }

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
        for_each_elements: IndexMap::new(),
        pending_ports: IndexMap::new(),
        pending_for_each: IndexMap::new(),
        pending_outputs: IndexMap::new(),
    };

    resolver.check_boundaries(&mut errors);

    for (name, node) in &workflow.nodes {
        let Some(Some(spec)) = specs.get(name) else {
            continue;
        };
        resolver.check_node(name, node, spec, &mut errors);
    }
    resolver.check_outputs(&mut errors);

    let Resolver {
        used_inputs,
        for_each_elements,
        pending_ports,
        pending_for_each,
        pending_outputs,
        ..
    } = resolver;

    errors.extend(find_cycles(&graph, workflow));

    if !errors.is_empty() {
        return Err(errors);
    }

    let order = topo_order(workflow, &graph, &index_of);
    let class = compute_class(workflow, &specs);
    let warnings = collect_warnings(workflow, &used_inputs);
    // G1: every node's tool was already confirmed present in `catalog`
    // above -- any `UnknownTool` would have made `errors` non-empty and
    // returned already -- so this is `check`'s own one-time lookup per
    // node, never repeated by `plan` or `apply` (verify item 4). G3 adds
    // each node's own bound scalar ports and (for an `Each` node) its
    // `for_each` source, converted from `pending_ports`/`pending_for_each`
    // in the same topological pass that mints `SingleId`/`EachId` -- see
    // `build_checked_nodes`' own doc for why a source's id is always
    // already known by the time it is needed.
    let (nodes, id_of) = build_checked_nodes(
        workflow,
        catalog,
        &specs,
        &order,
        &for_each_elements,
        &pending_ports,
        &pending_for_each,
    );
    // G2: every declared input's type was already confirmed registered and
    // non-secret by `check_workflow_inputs` above -- an unregistered or
    // secret type would have made `errors` non-empty and returned already
    // -- so this, too, is one lookup per input, never repeated by `plan`
    // or `describe`.
    let inputs = build_checked_inputs(workflow, registry);
    // G3: every workflow output, classified -- built once every node's own
    // id is known, exactly like a node's own ports.
    let outputs = build_checked_outputs(workflow, &specs, &pending_outputs, &id_of);

    Ok(Checked {
        workflow: workflow.clone(),
        inputs,
        order,
        nodes,
        class,
        warnings,
        outputs,
    })
}

/// Build [`Checked`]'s input table, in declaration order (`InputId` is the
/// index -- SHARED VALUES): each declared input's own [`InputSpec`], the
/// registry entry its declared type resolved to (already confirmed
/// present by [`check_workflow_inputs`], run before any node or this table
/// is built), and whether it is an [`willikins_types::OperatorAcknowledgement`]
/// awaited rather than missing.
fn build_checked_inputs(workflow: &Workflow, registry: &'static TypeRegistry) -> Vec<CheckedInput> {
    workflow
        .inputs
        .iter()
        .enumerate()
        .map(|(index, (name, spec))| {
            // Minted for the same reason `build_checked_nodes` mints a
            // `SingleId`/`EachId` per node: never read before G3 gives
            // `Source::Input(InputId)` a real consumer, but `InputId` is
            // always minted in declaration order, matching this table's
            // own position (SHARED VALUES). Unlike `SingleId`/`EachId`,
            // this one has nowhere to live yet (`CheckedInput` carries no
            // `id` field -- SHARED VALUES' frozen shape), so a bare
            // `InputId(index)` bound to `_id` is clippy's
            // `no_effect_underscore_binding`: the fallible cast keeps the
            // mint live for the lint without giving it a field to sit in
            // before G3.
            let _id = InputId(
                u32::try_from(index)
                    .unwrap_or_else(|_| unreachable!("more declared inputs than fit in a u32")),
            );
            let entry = registry.get(&spec.ty.name).unwrap_or_else(|| {
                unreachable!("`check_workflow_inputs` already rejected an unregistered input type")
            });
            let awaited = crate::value::is_operator_acknowledgement(registry, &spec.ty.name);
            CheckedInput {
                name: name.clone(),
                spec: spec.clone(),
                entry,
                awaited,
            }
        })
        .collect()
}

/// A node's own id, whichever kind it turned out to be -- the running map
/// [`build_checked_nodes`] and [`build_checked_outputs`] share while
/// converting [`PendingSource`]s to real [`Source`]s. Module-private: it
/// never appears in [`Checked`] itself, only in the plumbing that builds
/// it.
#[derive(Clone, Copy)]
enum NodeId {
    Single(SingleId),
    Each(EachId),
}

/// The [`OutputSlot`] of `referenced`'s own `port`: its position in that
/// node's tool's own `spec.outputs` (an `IndexMap`, so this is a real,
/// stable position -- milestone 3n, decision (g1)).
///
/// # Panics
///
/// Panics if `referenced` has no resolved spec, or `port` is not one of
/// its declared output ports: both are impossible for a
/// [`PendingSource::Reference`] or [`PendingSource::Keyed`]'s own `node`/
/// `port`, which only ever exist because [`Resolver::resolve_reference`]
/// already confirmed the output port is there (an `UnknownNode` or
/// `UnknownPort` error would have made `errors` non-empty and `check`
/// would have returned before this function is ever called).
fn output_slot(
    specs: &IndexMap<NodeName, Option<&ToolSpec>>,
    referenced: &NodeName,
    port: &PortName,
) -> OutputSlot {
    let spec = match specs.get(referenced) {
        Some(Some(spec)) => *spec,
        _ => unreachable!("a PendingSource never names a node whose spec failed to resolve"),
    };
    let index = spec
        .outputs
        .get_index_of(port)
        .unwrap_or_else(|| unreachable!("a PendingSource never names an unknown output port"));
    OutputSlot(
        u32::try_from(index)
            .unwrap_or_else(|_| unreachable!("more output ports than fit in a u32")),
    )
}

/// The [`InputId`] of `name`: its position in [`Checked::inputs`]
/// (`workflow.inputs`' own declaration order).
///
/// # Panics
///
/// Panics if `name` is not a declared input: impossible for a
/// [`PendingSource::Input`], which only ever exists because
/// [`Resolver::resolve`] already confirmed `workflow.inputs.get(input)` is
/// `Some` (an `UndeclaredInput` error would have made `errors` non-empty
/// and `check` would have returned before this function is ever called).
fn input_id(workflow: &Workflow, name: &InputName) -> InputId {
    let index = workflow
        .inputs
        .get_index_of(name)
        .unwrap_or_else(|| unreachable!("a PendingSource::Input never names an undeclared input"));
    InputId(u32::try_from(index).unwrap_or_else(|_| unreachable!("more inputs than fit in a u32")))
}

/// Convert one [`PendingSource`] to a real [`Source`], now that every
/// node's own id is known (`id_of` holds an entry for every node this
/// `PendingSource` can possibly reference -- see [`PendingSource`]'s own
/// doc for why).
fn build_source<I>(
    pending: &PendingSource,
    ty: &TypeRef,
    workflow: &Workflow,
    specs: &IndexMap<NodeName, Option<&ToolSpec>>,
    id_of: &IndexMap<NodeName, NodeId>,
) -> Source<I> {
    match pending {
        PendingSource::Literal(value) => Source::Literal(value.clone()),
        PendingSource::Input(name) => Source::Input(input_id(workflow, name)),
        PendingSource::Item => unreachable!(
            "`check` rejects `${{ item }}` used outside a for_each node \
             (CheckError::ItemOutsideForEach) before any PendingSource is built from it"
        ),
        PendingSource::Reference { node, port } => {
            let slot = output_slot(specs, node, port);
            match id_of.get(node).unwrap_or_else(|| {
                unreachable!("a PendingSource::Reference's own node is always earlier in `order`")
            }) {
                NodeId::Single(from) => Source::Step {
                    from: *from,
                    slot,
                    ty: ty.clone(),
                },
                NodeId::Each(from) => Source::Aggregate {
                    from: *from,
                    slot,
                    ty: ty.clone(),
                },
            }
        }
        PendingSource::Keyed { node, key, port } => {
            let slot = output_slot(specs, node, port);
            let NodeId::Each(from) = id_of.get(node).unwrap_or_else(|| {
                unreachable!("a PendingSource::Keyed's own node is always earlier in `order`")
            }) else {
                unreachable!(
                    "`check` rejects a Keyed reference to a node with no for_each \
                     (CheckError::KeyedOnScalarNode) before any PendingSource is built from it"
                )
            };
            Source::Keyed {
                from: *from,
                key: key.clone(),
                slot,
                ty: ty.clone(),
            }
        }
    }
}

/// [`build_source`] for a [`Shape::Each`] node's own port: the one place
/// [`PendingSource::Item`] is a real value rather than an `unreachable!`.
fn build_source_in_each(
    pending: &PendingSource,
    ty: &TypeRef,
    workflow: &Workflow,
    specs: &IndexMap<NodeName, Option<&ToolSpec>>,
    id_of: &IndexMap<NodeName, NodeId>,
) -> Source<InEach> {
    if matches!(pending, PendingSource::Item) {
        return Source::Item(InEach);
    }
    build_source(pending, ty, workflow, specs, id_of)
}

/// [`build_source`] for every [`PendingPort`] of one node, in the tool
/// spec's own input order (the order `pending_ports` was pushed in --
/// `Resolver::check_node` walks `spec.inputs`). A [`PendingBound::List`]
/// builds one [`Source`] per element, each against *that element's own*
/// resolved type (`element_edge.ty()`) -- never the list edge's own `ty()`,
/// which is the port's declared element type `T` rather than what each
/// element individually resolved to (milestone 3n, G4, the same per-source
/// rule G3 already applies to a scalar port).
fn build_port_edges<I>(
    pending: &[PendingPort],
    workflow: &Workflow,
    specs: &IndexMap<NodeName, Option<&ToolSpec>>,
    id_of: &IndexMap<NodeName, NodeId>,
    build: impl Fn(
        &PendingSource,
        &TypeRef,
        &Workflow,
        &IndexMap<NodeName, Option<&ToolSpec>>,
        &IndexMap<NodeName, NodeId>,
    ) -> Source<I>,
) -> Vec<PortEdge<I>> {
    pending
        .iter()
        .map(|p| PortEdge {
            port: p.port.clone(),
            with_position: p.with_position,
            bound: match &p.bound {
                PendingBound::Scalar(source) => Bound::Scalar {
                    source: build(source, p.edge.ty(), workflow, specs, id_of),
                    edge: p.edge.clone(),
                },
                PendingBound::List(sources) => {
                    let element_edges = p.edge.elements().unwrap_or_else(|| {
                        unreachable!("a PendingBound::List's own edge is always Edge::list")
                    });
                    let elements = sources
                        .iter()
                        .zip(element_edges)
                        .map(|(source, element_edge)| {
                            build(source, element_edge.ty(), workflow, specs, id_of)
                        })
                        .collect();
                    Bound::List {
                        elements,
                        edge: p.edge.clone(),
                    }
                }
            },
        })
        .collect()
}

/// Build [`Checked`]'s node table, in `order` (topological): each node's
/// own tool handle (already confirmed present -- see [`check`]'s only
/// caller) and its [`Shape`], minting a [`SingleId`] or [`EachId`] in
/// topological order within its own kind (SHARED VALUES). `for_each_elements`
/// holds the resolved element type of every node that has a `for_each` --
/// recorded by [`Resolver::check_for_each`] on its own success path, which
/// is the only way a node with a `for_each` reaches this function at all
/// (any failure there pushes an error, and `check` returns before building
/// this table).
///
/// G3 also converts `pending_ports` and `pending_for_each` into real
/// [`Source`]s, in the same single topological pass that mints ids: a
/// [`PendingSource::Reference`]/`Keyed` only ever names an earlier node
/// (`check`'s own module doc, "Typed ids are indexes, not proofs" --
/// `check` would have reported a cycle otherwise), so `id_of` already
/// holds that node's id by the time this node's own ports are built.
/// Returns the finished node table alongside `id_of` itself, which
/// [`build_checked_outputs`] needs too (workflow outputs are resolved only
/// after every node, so by definition after every node's own id exists).
fn build_checked_nodes(
    workflow: &Workflow,
    catalog: &Catalog,
    specs: &IndexMap<NodeName, Option<&ToolSpec>>,
    order: &[NodeName],
    for_each_elements: &IndexMap<NodeName, TypeRef>,
    pending_ports: &IndexMap<NodeName, Vec<PendingPort>>,
    pending_for_each: &IndexMap<NodeName, (PendingSource, TypeRef)>,
) -> (Vec<CheckedNode>, IndexMap<NodeName, NodeId>) {
    let mut next_single = 0u32;
    let mut next_each = 0u32;
    let mut id_of: IndexMap<NodeName, NodeId> = IndexMap::new();
    let empty: Vec<PendingPort> = Vec::new();
    let nodes = order
        .iter()
        .map(|name| {
            let declared_at = workflow.nodes.get_index_of(name).unwrap_or_else(|| {
                unreachable!("`order` names only nodes `workflow.nodes` itself declares")
            });
            let (_, node) = workflow.nodes.get_index(declared_at).unwrap_or_else(|| {
                unreachable!("`declared_at` was just read from `get_index_of` on this same map")
            });
            let tool = Arc::clone(catalog.get(&node.tool).unwrap_or_else(|| {
                unreachable!(
                    "every node's tool was confirmed present before any error was returned"
                )
            }));
            let own_pending = pending_ports.get(name).unwrap_or(&empty);
            let shape = if let Some(element) = for_each_elements.get(name) {
                let id = EachId(next_each);
                next_each += 1;
                let (pending_source, ty) = pending_for_each.get(name).unwrap_or_else(|| {
                    unreachable!(
                        "`check_for_each` records a pending source on every success path \
                         that also records `for_each_elements`"
                    )
                });
                let source =
                    build_source::<Infallible>(pending_source, ty, workflow, specs, &id_of);
                let ports =
                    build_port_edges(own_pending, workflow, specs, &id_of, build_source_in_each);
                id_of.insert(name.clone(), NodeId::Each(id));
                Shape::Each {
                    id,
                    source,
                    element: element.clone(),
                    ports,
                }
            } else {
                let id = SingleId(next_single);
                next_single += 1;
                let ports = build_port_edges(
                    own_pending,
                    workflow,
                    specs,
                    &id_of,
                    build_source::<Infallible>,
                );
                id_of.insert(name.clone(), NodeId::Single(id));
                Shape::Single { id, ports }
            };
            CheckedNode {
                name: name.clone(),
                declared_at,
                tool,
                shape,
            }
        })
        .collect();
    (nodes, id_of)
}

/// Build [`Checked`]'s output table, in declaration order, converting
/// every `pending_outputs` entry the same way [`build_checked_nodes`]
/// converts a port's own pending source -- by the time this runs, `id_of`
/// holds every node's id, since every workflow output is resolved only
/// after every node (`check_outputs` runs last).
fn build_checked_outputs(
    workflow: &Workflow,
    specs: &IndexMap<NodeName, Option<&ToolSpec>>,
    pending_outputs: &IndexMap<OutputName, (PendingSource, TypeRef)>,
    id_of: &IndexMap<NodeName, NodeId>,
) -> Vec<CheckedOutput> {
    workflow
        .outputs
        .keys()
        .filter_map(|name| {
            let (pending_source, ty) = pending_outputs.get(name)?;
            let source = build_source::<Infallible>(pending_source, ty, workflow, specs, id_of);
            Some(CheckedOutput {
                name: name.clone(),
                source,
                ty: ty.clone(),
            })
        })
        .collect()
}

/// Check every declared root input, in declaration order, through
/// [`check_input_spec`].
fn check_workflow_inputs(
    workflow: &Workflow,
    registry: &TypeRegistry,
    errors: &mut Vec<CheckError>,
) {
    for (name, spec) in &workflow.inputs {
        check_input_spec(name, spec, registry, errors);
    }
}

/// Check one declared input's own shape, independent of anything bound
/// to it: [`CheckError::SecretWorkflowInput`] when its type is secret,
/// [`CheckError::UnregisteredInputType`] when its type is not in the
/// registry at all, [`CheckError::DisallowedInputType`] when its type is
/// [`willikins_types::TemplateSource`] or [`willikins_types::RepoFile`]
/// (milestone 3g decision (e) -- whether or not a default is also present),
/// [`CheckError::AcknowledgementDefault`] when its type is
/// [`willikins_types::OperatorAcknowledgement`] and it carries a default, or
/// else [`CheckError::DefaultTypeMismatch`] when its default value is not
/// of the declared type: its own `TypeRef` differs, or one of its known
/// objects fails the registry entry's `TypeId` test
/// ([`TypeRegistry::type_matches`]), never a name comparison. Any of the
/// first three is a root cause that suppresses the default check for that
/// input, which could only repeat it (a secret type cannot have a valid
/// default at all; an unregistered type has nothing to check the default
/// against; `TemplateSource` and `RepoFile` may never be defaulted either,
/// same as they may never be the type at all).
///
/// `name` is the name every pushed error is reported under:
/// [`check_workflow_inputs`] passes a root input's own authored name;
/// [`Resolver::check_boundaries`] (milestone 2b task C1, decision (d4))
/// passes a used document's declared input instead, reported as
/// `<uses step>/<child input>` ([`crate::compose::prefixed_input`]) --
/// [`InputName`] already accepts that `/`-separated form (decision
/// (d3)), so this same type of error names the `uses:` step and the
/// child input with no new field on any of the five variants above.
/// Returns whether an error was pushed, so a caller resolving a *bound*
/// boundary's own binding can skip that check once this declared shape
/// is already broken -- the same cascade-suppression rule the module
/// docs describe elsewhere, applied here across these five rules rather
/// than within one of them.
fn check_input_spec(
    name: &InputName,
    spec: &InputSpec,
    registry: &TypeRegistry,
    errors: &mut Vec<CheckError>,
) -> bool {
    match registry.is_secret(&spec.ty.name) {
        Some(true) => {
            errors.push(CheckError::SecretWorkflowInput {
                input: name.clone(),
                ty: spec.ty.clone(),
            });
            return true;
        }
        None => {
            errors.push(CheckError::UnregisteredInputType {
                input: name.clone(),
                ty: spec.ty.clone(),
            });
            return true;
        }
        Some(false) => {}
    }
    if crate::value::is_operator_acknowledgement(registry, &spec.ty.name) {
        if spec.default.is_some() {
            errors.push(CheckError::AcknowledgementDefault {
                input: name.clone(),
            });
            return true;
        }
        return false;
    }
    if crate::value::is_template_source(registry, &spec.ty.name)
        || crate::value::is_repo_file(registry, &spec.ty.name)
    {
        // One error whether or not `spec.default` is set: this is the
        // root cause, and checking the default further (below) could
        // only repeat it. Milestone 3g decision (e)'s two refusals
        // ("as a workflow input type" and "as an input default")
        // collapse into this one branch.
        errors.push(CheckError::DisallowedInputType {
            input: name.clone(),
            ty: spec.ty.clone(),
        });
        return true;
    }
    let Some(default) = &spec.default else {
        return false;
    };
    let found = if default.ty() == &spec.ty {
        let scalar = default.as_scalar().into_iter();
        let items = default
            .as_list()
            .unwrap_or_default()
            .iter()
            .map(Arc::as_ref);
        scalar
            .chain(items)
            .find(|object| registry.type_matches(&spec.ty.name, *object) != Some(true))
            .map(|object| TypeRef {
                name: reported_type_name_or(object, &spec.ty.name),
                list: spec.ty.list,
            })
    } else {
        Some(default.ty().clone())
    };
    if let Some(found) = found {
        errors.push(CheckError::DefaultTypeMismatch {
            input: name.clone(),
            expected: spec.ty.clone(),
            found,
        });
        return true;
    }
    false
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

/// Whether `ty` is a secret type, per `registry`. An unregistered type
/// name is treated as non-secret; see the module docs' "Known gaps".
fn is_secret(ty: &TypeRef, registry: &TypeRegistry) -> bool {
    registry.is_secret(&ty.name) == Some(true)
}

/// Whether `ty` is an identifier type (`#[domain(identifier)]`), per
/// `registry`. `ty.name` is the element type whether or not `ty` itself is
/// list-typed (milestone 3n, decision (b1)), the same convention
/// [`is_secret`] already relies on. An unregistered type name is treated
/// as non-identifier.
fn is_identifier(ty: &TypeRef, registry: &TypeRegistry) -> bool {
    registry
        .get(&ty.name)
        .is_some_and(|entry| entry.info.identifier)
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

/// A bound port's, `for_each` source's, or workflow output's own
/// [`Source`], named by the `NodeName`s it refers to rather than by the
/// `SingleId`/`EachId`/`InputId` a real [`Source`] carries -- milestone
/// 3n, decision (g1)'s two-phase build. `SingleId`/`EachId` are minted by
/// [`build_checked_nodes`] in topological order, which is *after*
/// [`Resolver`] has finished resolving every binding, so a resolution
/// method records this name-based stand-in instead; [`build_source`]
/// converts it to a real `Source` once the id each name was given is
/// known (always true for a reference's own target by the time its
/// consumer is reached -- "a source's id is always lower than its
/// consumer's", `check`'s own module doc).
///
/// One [`Binding::List`] element's own pending source is a `PendingSource`
/// too (milestone 3n, G4) -- a list binding carries no single stand-in of
/// its own; [`PendingBound::List`] holds one of these per element instead.
#[derive(Clone)]
enum PendingSource {
    /// A [`Binding::Literal`], already parsed: the [`Value`] itself, with
    /// no name to defer resolving.
    Literal(Value),
    /// A [`Binding::Input`]'s own input name.
    Input(InputName),
    /// A [`Binding::Item`].
    Item,
    /// A [`Binding::Step`]: resolves to [`Source::Step`] or
    /// [`Source::Aggregate`] depending on whether `node`'s own [`Shape`]
    /// turns out to be `Single` or `Each`.
    Reference { node: NodeName, port: PortName },
    /// A [`Binding::Keyed`]: always resolves to [`Source::Keyed`] (`check`
    /// already refused one naming a node with no `for_each`,
    /// [`CheckError::KeyedOnScalarNode`]).
    Keyed {
        node: NodeName,
        key: String,
        port: PortName,
    },
}

/// [`PendingSource`] for every [`Binding`] kind [`Resolver::resolve`]
/// itself resolves (everything but [`Binding::Literal`] and
/// [`Binding::List`], which never reach [`Resolver::resolve`] at all --
/// see that method's own `unreachable!`s). Also the one [`check_list_port`]
/// calls once per element of a [`Binding::List`], never on the list
/// binding itself (its own `Literal`/nested-`List` elements are handled
/// before this is ever reached, by [`Resolver::check_list_element`]).
fn pending_source_of(binding: &Binding) -> PendingSource {
    match binding {
        Binding::Input(name) => PendingSource::Input(name.clone()),
        Binding::Item => PendingSource::Item,
        Binding::Step { node, port } => PendingSource::Reference {
            node: node.clone(),
            port: port.clone(),
        },
        Binding::Keyed { node, key, port } => PendingSource::Keyed {
            node: node.clone(),
            key: key.clone(),
            port: port.clone(),
        },
        Binding::Literal(_) | Binding::List(_) => unreachable!(
            "callers of pending_source_of never hold a Literal or List binding here -- \
             a literal has its own Value in hand already, and a list's own elements are \
             classified one at a time by check_list_element, never by passing the whole \
             List binding to pending_source_of"
        ),
    }
}

/// One bound port's own pending classification (milestone 3n, G4): a
/// single [`PendingSource`] for a scalar port, or one per element for a
/// [`Binding::List`] port, aligned with [`PendingPort::edge`]'s own
/// [`Edge::elements`].
#[derive(Clone)]
enum PendingBound {
    Scalar(PendingSource),
    List(Vec<PendingSource>),
}

/// One bound port's pending data, by name (milestone 3n, decision
/// (g1)'s two-phase build -- see [`PendingSource`]'s own doc): everything
/// [`build_checked_nodes`] needs to build a real [`PortEdge`] once ids
/// exist, recorded by [`Resolver::check_with_port`]/[`Resolver::check_list_port`]
/// at exactly the point each one resolves this port's own [`Edge`].
#[derive(Clone)]
struct PendingPort {
    port: PortName,
    with_position: usize,
    bound: PendingBound,
    edge: Edge,
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
    /// Every `for_each` node's own resolved element type, recorded by
    /// [`Self::check_for_each`] on its own success path only (milestone
    /// 3n, decision (g1)): `check`'s node table reads this once every
    /// node has checked clean, to tell a [`Shape::Each`] from a
    /// [`Shape::Single`] and give the former its `element`.
    for_each_elements: IndexMap<NodeName, TypeRef>,
    /// Every node's own bound ports, scalar or list alike, by name,
    /// pending [`build_checked_nodes`]' id-aware conversion to real
    /// [`PortEdge`]s (milestone 3n, decision (g1), G3/G4).
    pending_ports: IndexMap<NodeName, Vec<PendingPort>>,
    /// Every `for_each` node's own source, by name, and its resolved type
    /// (the full `list<T>`, before `.element()`) -- pending the same
    /// conversion as [`Self::pending_ports`].
    pending_for_each: IndexMap<NodeName, (PendingSource, TypeRef)>,
    /// Every workflow output's own source, by name, and its resolved
    /// type, in declaration order -- pending the same conversion.
    pending_outputs: IndexMap<OutputName, (PendingSource, TypeRef)>,
}

impl<'a> Resolver<'a> {
    /// Whether `source` — a resolved binding's `(node, port)` origin, as
    /// [`Self::resolve`] returns it — is one [`PortSpec::derived_only`]
    /// accepts: the output of an earlier node whose own tool is **not**
    /// pure.
    ///
    /// `None` (a literal, a workflow input, or a `for_each` `item`, none
    /// of which carries a source) is never accepted — a `derived_only`
    /// port polices provenance, and none of those three has any. A pure
    /// node's output is refused too, deliberately: a pure tool is
    /// evaluated from its own inputs alone (`plan` runs it during
    /// planning, from whatever literals or inputs feed it), so accepting
    /// one here would let a document launder a literal through a
    /// passthrough node and defeat the whole restriction — see
    /// `PortSpec::derived_only`'s own doc comment. A node whose spec
    /// could not be resolved (an unknown tool, already reported
    /// elsewhere) is conservatively refused rather than assumed derived.
    fn is_derived_binding(&self, source: Option<&(NodeName, PortName)>) -> bool {
        let Some((from_node, _)) = source else {
            return false;
        };
        matches!(self.specs.get(from_node), Some(Some(spec)) if !spec.pure)
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
            // `with_position` is only ever read once `binding` is `Some`
            // (a [`PendingPort`]'s own field); for an unbound port
            // `get_index_of` is `None` too, so `unwrap_or(0)` is never
            // actually consulted.
            let with_position = node.with.get_index_of(port).unwrap_or(0);
            self.check_with_port(
                name,
                port,
                with_position,
                port_spec,
                node.with.get(port),
                &item_ctx,
                errors,
            );
        }
        for (port, binding) in &node.with {
            if !seen.contains(port) {
                errors.push(CheckError::UnknownPort {
                    site: Site::Port {
                        node: name.clone(),
                        port: port.clone(),
                    },
                    tool: node.tool.clone(),
                });
                self.record_extra(binding, name);
            }
        }
    }

    /// Resolve `node`'s `for_each` binding, if any, pushing
    /// [`CheckError::SecretForEachSource`], [`CheckError::ForEachOverScalar`],
    /// [`CheckError::IdentifierForEachSource`] or
    /// [`CheckError::DuplicateForEachDefault`] as needed, in that order (the
    /// module docs' "Error ordering").
    fn check_for_each(
        &mut self,
        name: &NodeName,
        node: &Node,
        errors: &mut Vec<CheckError>,
    ) -> ItemContext {
        let Some(binding) = &node.for_each else {
            return ItemContext::NotInForEach;
        };
        let site = Site::ForEach { node: name.clone() };
        if matches!(binding, Binding::Literal(_)) {
            errors.push(CheckError::ForEachOverScalar { node: name.clone() });
            return ItemContext::ForEachBroken;
        }

        let site_idx = self.index_of.get(name).copied();
        let not_in_for_each = ItemContext::NotInForEach;
        let Some((ty, _source)) = self.resolve(site_idx, &site, &not_in_for_each, binding, errors)
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
        if is_identifier(&ty, self.registry) {
            errors.push(CheckError::IdentifierForEachSource {
                node: name.clone(),
                ty: ty.clone(),
            });
            return ItemContext::ForEachBroken;
        }
        if let Some(error) = self.colliding_for_each_default(name, binding) {
            errors.push(error);
            return ItemContext::ForEachBroken;
        }
        let element = ty.element();
        // Recorded only on this success path: `check`'s node table (G1)
        // reads this once `errors` is confirmed empty, so a node that
        // reaches that point with a `for_each` always has an entry here.
        self.for_each_elements.insert(name.clone(), element.clone());
        // G3: the `for_each` source itself, classified, pending the same
        // id-aware conversion as a bound port (see `PendingSource`'s own
        // doc). `ty` is the whole resolved `list<T>`, matching
        // `Source::Aggregate`'s/`Source::Step`'s own `ty` field.
        self.pending_for_each
            .insert(name.clone(), (pending_source_of(binding), ty));
        ItemContext::ForEachOf(element)
    }

    /// When `binding` is a workflow input carrying a known list default,
    /// report [`CheckError::DuplicateForEachDefault`] if two of that
    /// default's items render to the same canonical string -- the key
    /// `plan` gives a `for_each` instance, and the key a `Binding::Keyed`
    /// reference matches against. Any other binding resolves to a value
    /// `check` cannot see, which is why `plan` keeps its own guard.
    fn colliding_for_each_default(&self, name: &NodeName, binding: &Binding) -> Option<CheckError> {
        let Binding::Input(input) = binding else {
            return None;
        };
        let items = self
            .workflow
            .inputs
            .get(input)?
            .default
            .as_ref()?
            .as_list()?;
        let mut seen: HashSet<String> = HashSet::with_capacity(items.len());
        for object in items {
            let key = object.render().to_string();
            if !seen.insert(key.clone()) {
                return Some(CheckError::DuplicateForEachDefault {
                    node: name.clone(),
                    input: input.clone(),
                    key,
                });
            }
        }
        None
    }

    /// Record `node`.`port`'s pending classification and `edge` (milestone
    /// 3n, decision (g1)): everything [`build_checked_nodes`] needs to
    /// build this port's real [`PortEdge`] once ids exist. From G7 on
    /// this is the *only* place this data is recorded -- `Checked::types`
    /// used to keep a second, parallel copy of just `edge` via a sibling
    /// `record_edge` call at each of this method's own call sites; that
    /// map, and the method, are gone, and `Checked::types`/`Checked::edge`
    /// now read this same `edge` back through `PortEdge`/`Bound` instead.
    fn record_pending_port(
        &mut self,
        node: &NodeName,
        port: &PortName,
        with_position: usize,
        bound: PendingBound,
        edge: Edge,
    ) {
        self.pending_ports
            .entry(node.clone())
            .or_default()
            .push(PendingPort {
                port: port.clone(),
                with_position,
                bound,
                edge,
            });
    }

    /// Check one `with` port: unbound-but-required, a literal, or a
    /// resolved binding checked for secrecy and type against `port_spec`.
    #[allow(clippy::too_many_arguments)] // with_position (G3) joins the existing shape, one per fact the caller already has in hand
    #[allow(clippy::too_many_lines)] // one port's five outcomes (unbound/literal/list/secret-or-type-mismatch/exact-or-converted), not split further
    fn check_with_port(
        &mut self,
        node: &NodeName,
        port: &PortName,
        with_position: usize,
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
            if port_spec.derived_only {
                errors.push(CheckError::UnderivedBinding {
                    node: node.clone(),
                    port: port.clone(),
                });
                return;
            }
            if let Some(value) =
                check_literal(node, port, text, &port_spec.ty, self.registry, errors)
            {
                // A literal is never converted: it has no source type of
                // its own, and it already parsed as the port's own type
                // directly (`check_literal`, above). See decision (e)'s
                // step 2.
                let edge = Edge::exact(value.ty().clone());
                self.record_pending_port(
                    node,
                    port,
                    with_position,
                    PendingBound::Scalar(PendingSource::Literal(value)),
                    edge,
                );
            }
            return;
        }

        if let Binding::List(elements) = binding {
            self.check_list_port(
                node,
                port,
                with_position,
                port_spec,
                elements,
                item_ctx,
                errors,
            );
            return;
        }

        let site_idx = self.index_of.get(node).copied();
        let site = Site::Port {
            node: node.clone(),
            port: port.clone(),
        };
        let Some((found, source)) = self.resolve(site_idx, &site, item_ctx, binding, errors) else {
            return;
        };

        if port_spec.derived_only && !self.is_derived_binding(source.as_ref()) {
            errors.push(CheckError::UnderivedBinding {
                node: node.clone(),
                port: port.clone(),
            });
            return;
        }

        if is_secret(&found, self.registry)
            && !port_accepts_secret(&port_spec.ty, self.registry)
            && let Some(from) = source
        {
            // A secret with an attributable source is a taint violation,
            // reported in preference to the type mismatch it also is. A
            // secret with *no* source (today only `Item`, whose element
            // type a secret `for_each` source is already refused for)
            // falls through to the type check below, which cannot accept
            // it either -- so no binding is ever dropped without an error.
            errors.push(CheckError::SecretToNonSecretSink { from, to: site });
            return;
        }

        if port_spec.ty.accepts(&found, self.registry) {
            let edge = Edge::exact(found);
            self.record_pending_port(
                node,
                port,
                with_position,
                PendingBound::Scalar(pending_source_of(binding)),
                edge,
            );
            return;
        }

        // Milestone 3d, decision (e), step 7: the binding's own type does
        // not match the port's exactly, but a registered conversion might
        // still deliver it. Scalars only ("one hop, no chains" -- the
        // probe itself has no transitive search either, but a list edge
        // is refused before ever reaching the table: `list<A>` into a
        // `list<B>` port stays a `TypeMismatch`, the same way `Vec<A>` is
        // not `Into<Vec<B>>` in Rust). This can only ever turn a
        // `TypeMismatch` into an accepted edge, never the reverse, and
        // only for a pair `AnySecret` never presents (it names no single
        // target type to probe).
        if let PortType::Exact(to) = &port_spec.ty
            && !found.list
            && !to.list
        {
            // `willikins-core` is `TypeRegistry::probe_conversion`'s one
            // sanctioned caller: `clippy.toml` disallows every other call,
            // and a tripwire test greps `plan.rs`, `apply.rs` and
            // `describe.rs` for the method's name so they can never reach
            // the table even through an `#[allow]` smuggled in there.
            #[allow(clippy::disallowed_methods)]
            let row = self.registry.probe_conversion(&found.name, &to.name);
            if let Some(row) = row {
                let edge = Edge::converted(found, row);
                self.record_pending_port(
                    node,
                    port,
                    with_position,
                    PendingBound::Scalar(pending_source_of(binding)),
                    edge,
                );
                return;
            }
        }

        errors.push(CheckError::TypeMismatch {
            node: node.clone(),
            port: port.clone(),
            expected: port_spec.ty.clone(),
            found,
        });
    }

    /// Check a [`Binding::List`] bound to `port`: milestone 3g, decision
    /// (a). Refuses outright when `port_spec` is not `Exact(list<T>)` (a
    /// scalar port, or `AnySecret`, which names no single element type to
    /// check a list against) or is `derived_only` (no list binding can ever
    /// be "the output of one earlier, non-pure node" — each element has its
    /// own, possibly different, source). Otherwise checks every element in
    /// order against the element type `T`: a literal parses directly
    /// against `T` (never converted, exactly like a scalar literal); a
    /// resolved reference that is itself list-typed is
    /// [`CheckError::ListElementTypeMismatch`] (no flattening); a secret
    /// element with an attributable source is
    /// [`CheckError::SecretToNonSecretSink`], reported in preference to the
    /// type mismatch it also is, exactly like the scalar case; otherwise an
    /// exact match records a plain [`Edge::exact`], and a one-hop
    /// conversion (the milestone 3d rule, scalars only) records
    /// [`Edge::converted`]. One error per bad element, never stopping at
    /// the first; on any element error, no edge is recorded for the whole
    /// port at all (matching the scalar case, where a failed binding
    /// records nothing either).
    ///
    /// On success (milestone 3n, G4), also records this port's own pending
    /// classification: one [`PendingSource`] per element, in the same
    /// order as the [`Edge::list`] just recorded -- [`build_port_edges`]
    /// zips the two to build this port's real [`Bound::List`].
    #[allow(clippy::too_many_arguments)] // with_position (G4) joins check_with_port's own parameter shape
    fn check_list_port(
        &mut self,
        node: &NodeName,
        port: &PortName,
        with_position: usize,
        port_spec: &PortSpec,
        elements: &[Binding],
        item_ctx: &ItemContext,
        errors: &mut Vec<CheckError>,
    ) {
        let site = Site::Port {
            node: node.clone(),
            port: port.clone(),
        };

        if port_spec.derived_only {
            errors.push(CheckError::UnderivedBinding {
                node: node.clone(),
                port: port.clone(),
            });
            return;
        }

        let PortType::Exact(list_ty) = &port_spec.ty else {
            errors.push(CheckError::ListOnScalarPort {
                site,
                expected: port_spec.ty.clone(),
            });
            return;
        };
        if !list_ty.list {
            errors.push(CheckError::ListOnScalarPort {
                site,
                expected: port_spec.ty.clone(),
            });
            return;
        }
        let element_ty = list_ty.element();

        let mut element_edges: Vec<Edge> = Vec::with_capacity(elements.len());
        let mut pending_sources: Vec<PendingSource> = Vec::with_capacity(elements.len());
        let mut ok = true;
        for (index, element) in elements.iter().enumerate() {
            let elem_site = Site::list_element(node.clone(), port.clone(), index);
            match self.check_list_element(
                node,
                port,
                &element_ty,
                element,
                &elem_site,
                item_ctx,
                errors,
            ) {
                Some((source, edge)) => {
                    pending_sources.push(source);
                    element_edges.push(edge);
                }
                None => ok = false,
            }
        }

        if ok {
            let edge = Edge::list(element_ty, element_edges);
            self.record_pending_port(
                node,
                port,
                with_position,
                PendingBound::List(pending_sources),
                edge,
            );
        }
    }

    /// Check one element of a [`Binding::List`] against `element_ty` (`T`),
    /// pushing an error and returning `None` on any failure, or the
    /// element's own classified [`PendingSource`] and [`Edge`] (never
    /// converted for a literal; otherwise exact or one-hop converted) on
    /// success. Split out of [`Self::check_list_port`] to keep that
    /// method's own line count down.
    #[allow(clippy::too_many_arguments)] // mirrors check_with_port's own parameter shape, one per fact the caller already has in hand
    fn check_list_element(
        &mut self,
        node: &NodeName,
        port: &PortName,
        element_ty: &TypeRef,
        element: &Binding,
        elem_site: &Site,
        item_ctx: &ItemContext,
        errors: &mut Vec<CheckError>,
    ) -> Option<(PendingSource, Edge)> {
        if let Binding::Literal(text) = element {
            return check_list_literal(node, port, text, element_ty, self.registry, errors).map(
                |value| {
                    let edge = Edge::exact(value.ty().clone());
                    (PendingSource::Literal(value), edge)
                },
            );
        }

        // A nested list (never produced by `willikins-dsl`, which refuses
        // one at parse time) is refused defensively here too, rather than
        // recursing: `Resolver::resolve` is not exhaustive over it, since
        // resolving a list has no single type to report through that
        // generic path.
        if matches!(element, Binding::List(_)) {
            errors.push(CheckError::ListElementTypeMismatch {
                site: elem_site.clone(),
                expected: element_ty.clone(),
                found: TypeRef::list_of(element_ty.name.clone()),
            });
            return None;
        }

        let site_idx = self.index_of.get(node).copied();
        let (found, source) = self.resolve(site_idx, elem_site, item_ctx, element, errors)?;

        if is_secret(&found, self.registry) {
            match source {
                Some(from) => errors.push(CheckError::SecretToNonSecretSink {
                    from,
                    to: elem_site.clone(),
                }),
                None => errors.push(CheckError::ListElementTypeMismatch {
                    site: elem_site.clone(),
                    expected: element_ty.clone(),
                    found,
                }),
            }
            return None;
        }

        if found.list {
            errors.push(CheckError::ListElementTypeMismatch {
                site: elem_site.clone(),
                expected: element_ty.clone(),
                found,
            });
            return None;
        }

        if found == *element_ty {
            return Some((pending_source_of(element), Edge::exact(found)));
        }

        // One hop, scalars only -- the same rule `check_with_port` applies;
        // see its own comment for why this can only ever turn a mismatch
        // into an accepted edge, never the reverse.
        #[allow(clippy::disallowed_methods)]
        let row = self
            .registry
            .probe_conversion(&found.name, &element_ty.name);
        if let Some(row) = row {
            return Some((pending_source_of(element), Edge::converted(found, row)));
        }

        errors.push(CheckError::ListElementTypeMismatch {
            site: elem_site.clone(),
            expected: element_ty.clone(),
            found,
        });
        None
    }

    /// Record the side effects of a `with` key that is not one of its
    /// node's ports, without emitting any further error for it: mark an
    /// `Input` as used, and add a graph edge for a `Step`/`Keyed` binding
    /// whose referenced node exists -- recursively, for every element of a
    /// [`Binding::List`].
    fn record_extra(&mut self, binding: &Binding, site: &NodeName) {
        match binding {
            Binding::Input(name) => {
                self.used_inputs.insert(name.clone());
            }
            Binding::List(elements) => {
                for element in elements {
                    self.record_extra(element, site);
                }
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

    /// Check every `uses:` step's input boundary (milestone 2b task C1,
    /// decision (d4); module docs' "Error ordering", stage 3), in
    /// [`Workflow::boundaries`]'s own order. A boundary the linker left
    /// unbound (`binding` is `None`) is skipped: it became a *fixed*
    /// input instead (decision (d6)), already inserted into
    /// `workflow.inputs` under this exact combined name, so
    /// [`check_workflow_inputs`] already checked it -- checking it again
    /// here would only report the same declaration twice.
    ///
    /// For a *bound* boundary: first, the same five rules
    /// [`check_input_spec`] runs over a root input, over the used
    /// document's own declared [`crate::compose::Boundary::spec`] instead, reported
    /// under the combined `<uses step>/<child input>` name
    /// ([`prefixed_input`]) -- this is the only place any of those five
    /// errors can ever come from for an input the parent actually bound,
    /// because substitution erased it from `workflow.inputs` entirely
    /// (it was never a fixed input to begin with). When that declared
    /// shape is already broken, resolving the binding below could only
    /// cascade from it, so this is skipped the same way a broken
    /// declaration suppresses its own default check.
    ///
    /// Otherwise: an exact match, with no conversion probe, between the
    /// binding's own type and [`crate::compose::Boundary::spec`]'s declared one
    /// (decision (d4), "exact type at the boundary, no conversion"):
    ///
    /// - [`Binding::Literal`] is parsed directly against the declared
    ///   type by [`check_literal`] (the same function a node's own
    ///   literal binding goes through), which can only ever report
    ///   [`CheckError::InvalidLiteral`] or one of its secret/acknowledgement/
    ///   repo-file refusals -- never a type *mismatch*, since a literal
    ///   that parses at all parses as the very type it was parsed
    ///   against.
    /// - [`Binding::List`] is left unchecked here: milestone 2b's own
    ///   known gap (see the module docs). Every element -- literal or
    ///   reference alike -- is copied unchanged by substitution
    ///   (decision (d4)) and so reaches a real port downstream with a
    ///   real expected type of its own, *unless* the used document never
    ///   references this input anywhere, in which case nothing types it
    ///   at all. [`Binding::Item`] can never reach here: the linker
    ///   refuses `${{ item }}` in a `uses:` step's `with:` before `link`
    ///   ever returns ([`CheckError::ItemInUses`]).
    /// - [`Binding::Input`], [`Binding::Step`], and [`Binding::Keyed`] are
    ///   resolved exactly as [`Self::check_with_port`] resolves a node's
    ///   own binding ([`Self::resolve`], with `site_idx: None` so no
    ///   graph edge is added here -- the real one, if any, was already
    ///   added wherever this same binding was substituted into an actual
    ///   node port or `for_each` source), then compared to
    ///   [`crate::compose::Boundary::spec`]'s declared type by [`TypeRef`] equality,
    ///   never [`PortType::accepts`] and never
    ///   [`TypeRegistry::probe_conversion`]: [`CheckError::UsesInputTypeMismatch`]
    ///   on a mismatch.
    fn check_boundaries(&mut self, errors: &mut Vec<CheckError>) {
        let workflow = self.workflow;
        let not_in_for_each = ItemContext::NotInForEach;
        for boundary in &workflow.boundaries {
            let Some(binding) = &boundary.binding else {
                continue;
            };
            let name = prefixed_input(&boundary.uses, &boundary.input);
            if check_input_spec(&name, &boundary.spec, self.registry, errors) {
                continue;
            }
            let port = PortName::parse(boundary.input.as_str()).unwrap_or_else(|err| {
                unreachable!(
                    "InputName and PortName share one grammar, so a valid InputName is \
                     always a valid PortName: {err}"
                )
            });
            match binding {
                Binding::Literal(text) => {
                    check_literal(
                        &boundary.uses,
                        &port,
                        text,
                        &PortType::Exact(boundary.spec.ty.clone()),
                        self.registry,
                        errors,
                    );
                }
                Binding::List(_) => {}
                Binding::Item => unreachable!(
                    "the linker refuses `${{ item }}` in a `uses:` step's `with:` \
                     (CheckError::ItemInUses) before `link` ever returns a boundary"
                ),
                Binding::Input(_) | Binding::Step { .. } | Binding::Keyed { .. } => {
                    let site = Site::Port {
                        node: boundary.uses.clone(),
                        port,
                    };
                    if let Some((found, _source)) =
                        self.resolve(None, &site, &not_in_for_each, binding, errors)
                        && found != boundary.spec.ty
                    {
                        errors.push(CheckError::UsesInputTypeMismatch {
                            node: boundary.uses.clone(),
                            input: boundary.input.clone(),
                            expected: boundary.spec.ty.clone(),
                            found,
                        });
                    }
                }
            }
        }
    }

    /// Check every workflow output. Errors are reported under
    /// [`Site::Output`] (an output is not a node); resolved types feed
    /// [`Self::pending_outputs`], keyed by output name, so a real node
    /// named `outputs` cannot have its port types overwritten --
    /// [`build_checked_outputs`] reads them from there into
    /// [`CheckedOutput::ty`], which [`Checked::output_types`] (from G7 on,
    /// a view) reads in turn. A literal output has no target type to
    /// check against, so it is refused ([`CheckError::LiteralOutput`])
    /// rather than accepted and dropped; a secret output is accepted, by
    /// decision -- see `tests/check_adversarial.rs`. Every output a
    /// successful `check` returns therefore has an entry in
    /// [`Checked::output_types`].
    fn check_outputs(&mut self, errors: &mut Vec<CheckError>) {
        let workflow = self.workflow;
        let not_in_for_each = ItemContext::NotInForEach;
        for (out_name, binding) in &workflow.outputs {
            if matches!(binding, Binding::Literal(_)) {
                errors.push(CheckError::LiteralOutput {
                    output: out_name.clone(),
                });
                continue;
            }
            let site = Site::Output {
                name: out_name.clone(),
            };
            if let Some((found, _source)) =
                self.resolve(None, &site, &not_in_for_each, binding, errors)
            {
                self.pending_outputs
                    .insert(out_name.clone(), (pending_source_of(binding), found));
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
        site: &Site,
        item_ctx: &ItemContext,
        binding: &Binding,
        errors: &mut Vec<CheckError>,
    ) -> Option<(TypeRef, Option<(NodeName, PortName)>)> {
        match binding {
            Binding::Literal(_) => unreachable!(
                "callers resolve Literal via check_literal, which needs the port's expected type"
            ),
            Binding::List(_) => {
                // Reached only for a `for_each` source or a workflow
                // output: `check_with_port` intercepts a with-bound
                // `Binding::List` itself, via `check_list_port`, before
                // ever calling `resolve`. Neither site names a single
                // expected type for `resolve` to check a list binding
                // against, and `willikins-dsl` never produces one there
                // (its `for_each:` and `outputs:` fields are plain
                // strings) -- this exists so a hand-built `Workflow` still
                // gets a real error instead of a panic.
                errors.push(CheckError::SequenceNotAllowedHere { site: site.clone() });
                None
            }
            Binding::Item => match item_ctx {
                ItemContext::NotInForEach => {
                    errors.push(CheckError::ItemOutsideForEach { site: site.clone() });
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
                            site: site.clone(),
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
            } => self.resolve_reference(site_idx, site, referenced, false, port, errors),
            Binding::Keyed {
                node: referenced,
                port,
                ..
            } => self.resolve_reference(site_idx, site, referenced, true, port, errors),
        }
    }

    /// Resolve a `Step` (`keyed == false`) or `Keyed` (`keyed == true`)
    /// reference to `referenced`'s `port`, adding the graph edge whenever
    /// `referenced` exists (even if the reference later turns out invalid),
    /// and suppressing further checks on a self-reference (left for
    /// [`find_cycles`] to report).
    fn resolve_reference(
        &mut self,
        site_idx: Option<NodeIndex>,
        site: &Site,
        referenced: &NodeName,
        keyed: bool,
        port: &PortName,
        errors: &mut Vec<CheckError>,
    ) -> Option<(TypeRef, Option<(NodeName, PortName)>)> {
        let Some(&ref_idx) = self.index_of.get(referenced) else {
            errors.push(CheckError::UnknownNode {
                site: site.clone(),
                referenced: referenced.clone(),
            });
            return None;
        };

        if let Some(site_idx) = site_idx {
            self.graph.add_edge(ref_idx, site_idx, ());
        }

        // A node that references itself is left for `find_cycles` to
        // report. Only a real `Port` or `ForEach` site can be a self
        // reference: a workflow output's site is [`Site::Output`], which
        // names no node at all, so comparing it against a real node
        // literally named `outputs` would mistake an ordinary reference
        // for a self-reference and drop it with no error and no recorded
        // type (adversarial pass 2, finding 2). `site.node()` is `Some`
        // exactly for a real node site, matching `site_idx`.
        if site.node() == Some(referenced) {
            return None;
        }

        let target_node = self.workflow.nodes.get(referenced).unwrap_or_else(|| {
            unreachable!("referenced is a key of index_of, built from workflow.nodes")
        });

        if keyed && target_node.for_each.is_none() {
            errors.push(CheckError::KeyedOnScalarNode {
                site: site.clone(),
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
                site: Site::Port {
                    node: referenced.clone(),
                    port: port.clone(),
                },
                tool: target_spec.name.clone(),
            });
            return None;
        };

        let resolved_ty = if !keyed && target_node.for_each.is_some() {
            if output_ty.list {
                errors.push(CheckError::NestedList {
                    site: site.clone(),
                    referenced: referenced.clone(),
                });
                return None;
            }
            TypeRef::list_of(output_ty.name.clone())
        } else {
            output_ty.clone()
        };

        Some((resolved_ty, Some((referenced.clone(), port.clone()))))
    }
}

/// Check a `with`-bound literal against `expected`: a secret-accepting
/// port refuses it outright; an operator-acknowledgement or
/// [`willikins_types::RepoFile`] port refuses it too, before ever handing
/// the text to a parser (milestone 3g decision (e): the only producer of a
/// `RepoFile` is `repo.file.render`, so no literal may supply one); a
/// list-typed port refuses it because no literal can supply a list;
/// otherwise it is parsed against the port's scalar type.
///
/// Returns the parsed [`Value`] itself, not just its [`TypeRef`] (milestone
/// 3n, decision (g1)): `check_with_port` needs the value both to record
/// the edge (`value.ty()`) and to build a [`Source::Literal`], and
/// `check_boundaries` -- the other caller -- only ever needed the type, so
/// it drops the value.
fn check_literal(
    node: &NodeName,
    port: &PortName,
    text: &str,
    expected: &PortType,
    registry: &TypeRegistry,
    errors: &mut Vec<CheckError>,
) -> Option<Value> {
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

    if crate::value::is_operator_acknowledgement(registry, &ty.name) {
        errors.push(CheckError::AcknowledgementLiteral {
            node: node.clone(),
            port: port.clone(),
        });
        return None;
    }

    if crate::value::is_repo_file(registry, &ty.name) {
        errors.push(CheckError::RepoFileLiteral {
            node: node.clone(),
            port: port.clone(),
        });
        return None;
    }

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
        Ok(value) => Some(value),
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

/// Check one literal element of a [`Binding::List`] against `element_ty`
/// (the port's declared, always-scalar element type `T`): a secret `T`
/// refuses it outright, exactly like [`check_literal`] does for a
/// secret-accepting scalar port (a list element is never `AnySecret` --
/// [`check_list_port`] only reaches here once `element_ty` is already known
/// to be `T`, a concrete type); an operator-acknowledgement or
/// [`willikins_types::RepoFile`] `T` refuses it too, for the same reason
/// [`check_literal`] does (milestone 3g decision (e): `files: list<RepoFile>`
/// is exactly `github.scaffold.ensure`'s own port shape); otherwise it is
/// parsed directly against `T`, never converted (decision (e), step 2, the
/// same rule a scalar literal follows).
///
/// Returns the parsed [`Value`] itself, not just its [`TypeRef`] (milestone
/// 3n, G4, mirroring [`check_literal`]'s own G3 change): [`check_list_element`]
/// needs the value to build this element's own [`Source::Literal`], and
/// `plan`/`apply` then deliver it without re-parsing, exactly like a
/// scalar literal.
fn check_list_literal(
    node: &NodeName,
    port: &PortName,
    text: &str,
    element_ty: &TypeRef,
    registry: &TypeRegistry,
    errors: &mut Vec<CheckError>,
) -> Option<Value> {
    if registry.is_secret(&element_ty.name) == Some(true) {
        errors.push(CheckError::SecretLiteral {
            node: node.clone(),
            port: port.clone(),
        });
        return None;
    }

    if crate::value::is_operator_acknowledgement(registry, &element_ty.name) {
        errors.push(CheckError::AcknowledgementLiteral {
            node: node.clone(),
            port: port.clone(),
        });
        return None;
    }

    if crate::value::is_repo_file(registry, &element_ty.name) {
        errors.push(CheckError::RepoFileLiteral {
            node: node.clone(),
            port: port.clone(),
        });
        return None;
    }

    match Value::parse(element_ty, text) {
        Ok(value) => Some(value),
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

    use crate::tool::{Ensured, Inputs, Observation, Outputs, Tool, ToolError};
    use crate::value::TypeName;
    use crate::workflow::{InputSpec, Uses};
    use willikins_types::{DomainType, SinkToken};

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

    fn workflow_name(name: &str) -> willikins_types::WorkflowName {
        willikins_types::WorkflowName::parse(name).unwrap()
    }

    /// A [`Site::Port`] from two raw names, so a sample error stays on one
    /// line where it used to carry a bare `node`/`port` pair.
    fn port_site(node: &str, port_name: &str) -> Site {
        Site::Port {
            node: node_name(node),
            port: port(port_name),
        }
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

        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            Ok(Ensured {
                outputs: Outputs::new(),
                changed: true,
            })
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
                    derived_only: false,
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
            // A pure passthrough, not a mirror of any real tool: the
            // boundary-pass tests (milestone 2b task C1) need an
            // `AppleBundleIdentifier`-typed step output, to exercise the
            // registered `AppleBundleIdentifier => TemplateValue`
            // conversion row (`willikins_types::conversion_rows`)
            // against the boundary's own "no conversion, ever" rule,
            // without a real ASC tool's credential inputs.
            spec_of(
                "fake.identifier.get",
                &[("identifier", exact("AppleBundleIdentifier"), true)],
                &[("identifier", ty("AppleBundleIdentifier"))],
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

    /// A linked composite's own `Boundary`, built directly rather than
    /// through [`crate::compose::link`] (`Workflow::boundaries` and
    /// `Boundary`'s own fields are all `pub`, exactly like a hand-built
    /// `Workflow` itself): the mechanism these tests pin is `check`'s
    /// own boundary pass, not the linker's output, and these cases need
    /// no used-document resolution at all.
    fn boundary(
        uses: NodeName,
        input: InputName,
        spec: InputSpec,
        binding: Option<Binding>,
    ) -> crate::compose::Boundary {
        crate::compose::Boundary {
            uses,
            workflow: workflow_name("child-doc"),
            input,
            spec,
            binding,
        }
    }

    /// Milestone 2b task C1, decision (d4), acceptance 5: a parent
    /// binding a `GitHubRepo` step output to a child's `DopplerProject`
    /// input gives `UsesInputTypeMismatch`, naming the `uses:` step and
    /// the child input -- `check` never probes the conversion table for
    /// this edge (there is no row between these two types anyway, but
    /// the boundary pass does not even try).
    #[test]
    fn a_boundary_bound_to_the_wrong_type_gives_uses_input_type_mismatch() {
        let mut workflow = Workflow::new(workflow_name("w")).node(
            node_name("names"),
            Node::new(tool_name("naming.v1"))
                .port(port("org"), Binding::Literal("example-org".to_string()))
                .port(port("slug"), Binding::Literal("example-repo".to_string())),
        );
        workflow.boundaries = vec![boundary(
            node_name("child"),
            input_name("project"),
            InputSpec::new(ty("DopplerProject")),
            Some(Binding::Step {
                node: node_name("names"),
                port: port("github_repo"),
            }),
        )];
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UsesInputTypeMismatch {
                node: node_name("child"),
                input: input_name("project"),
                expected: ty("DopplerProject"),
                found: ty("GitHubRepo"),
            }]
        );
        assert_eq!(
            errors[0].to_string(),
            "node `child`, input `project`: expected DopplerProject, found `GitHubRepo`"
        );
    }

    /// Acceptance 5: a parent binding a type that only *converts* to a
    /// child's declared type -- here, the registered
    /// `AppleBundleIdentifier => TemplateValue` row -- still gives
    /// `UsesInputTypeMismatch`. Substitution moves the *unconverted*
    /// binding inside (decision (d4)): a node that actually consumes
    /// `value` would see the conversion applied at its own port, exactly
    /// as it would for any other edge, but the boundary itself never
    /// probes the table.
    #[test]
    fn a_boundary_binding_that_only_converts_to_the_declared_type_still_mismatches() {
        let mut workflow = Workflow::new(workflow_name("w")).node(
            node_name("app"),
            Node::new(tool_name("fake.identifier.get")).port(
                port("identifier"),
                Binding::Literal("com.example.app".to_string()),
            ),
        );
        workflow.boundaries = vec![boundary(
            node_name("child"),
            input_name("value"),
            InputSpec::new(ty("TemplateValue")),
            Some(Binding::Step {
                node: node_name("app"),
                port: port("identifier"),
            }),
        )];
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UsesInputTypeMismatch {
                node: node_name("child"),
                input: input_name("value"),
                expected: ty("TemplateValue"),
                found: ty("AppleBundleIdentifier"),
            }]
        );
    }

    /// Acceptance 5: a child declaring a secret input that the parent
    /// binds to a secret step output of the same type gives
    /// `SecretWorkflowInput` (the existing signature rule, reported
    /// under the combined `<uses step>/<child input>` name), and exactly
    /// that one error -- not also `UsesInputTypeMismatch`, even though
    /// nothing here would have mismatched anyway: the declared shape
    /// being broken already suppresses the type check (the same cascade
    /// [`check_input_spec`]'s own doc describes).
    #[test]
    fn a_secret_boundary_input_gives_exactly_one_secret_workflow_input_error() {
        let mut workflow = Workflow::new(workflow_name("w")).node(
            node_name("secret"),
            Node::new(tool_name("doppler.secret.get"))
                .port(
                    port("config"),
                    Binding::Literal("example-org/prd".to_string()),
                )
                .port(port("name"), Binding::Literal("EXAMPLE_SECRET".to_string())),
        );
        workflow.boundaries = vec![boundary(
            node_name("child"),
            input_name("token"),
            InputSpec::new(ty("DopplerSecretValue")),
            Some(Binding::Step {
                node: node_name("secret"),
                port: port("value"),
            }),
        )];
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::SecretWorkflowInput {
                input: input_name("child/token"),
                ty: ty("DopplerSecretValue"),
            }]
        );
    }

    /// Acceptance 5: an unbound (fixed) boundary is never re-checked by
    /// the boundary pass -- it is already a fixed input of the flat
    /// `workflow.inputs`, under this exact combined name, so
    /// [`check_workflow_inputs`] already covers it there. A boundary
    /// whose `spec` is itself broken (here, a secret type) but whose
    /// `binding` is `None` must therefore give exactly one error, not
    /// two.
    #[test]
    fn an_unbound_boundary_is_not_double_checked_against_its_already_fixed_input() {
        let mut workflow = Workflow::new(workflow_name("w")).input(
            input_name("child/token"),
            InputSpec::new(ty("DopplerSecretValue")),
        );
        workflow.boundaries = vec![boundary(
            node_name("child"),
            input_name("token"),
            InputSpec::new(ty("DopplerSecretValue")),
            None,
        )];
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::SecretWorkflowInput {
                input: input_name("child/token"),
                ty: ty("DopplerSecretValue"),
            }]
        );
    }

    /// Acceptance 5: a secret child output bound to a parent port that
    /// does not accept secrets gives the existing
    /// `SecretToNonSecretSink`, on the flat edge, exactly as it would for
    /// two nodes authored directly in the root -- composition changes
    /// nothing about this rule (decision (d5), "outwards... the existing
    /// sink check decides that on the flat edge").
    #[test]
    fn a_secret_child_output_into_a_non_secret_port_gives_the_existing_taint_error() {
        let workflow = Workflow::new(workflow_name("w"))
            .node(
                node_name("child/secret"),
                Node::new(tool_name("doppler.secret.get"))
                    .port(
                        port("config"),
                        Binding::Literal("example-org/prd".to_string()),
                    )
                    .port(port("name"), Binding::Literal("EXAMPLE_SECRET".to_string())),
            )
            .node(
                node_name("render"),
                Node::new(tool_name("template.render"))
                    .port(port("template"), Binding::Literal("tpl".to_string()))
                    .port(
                        port("value"),
                        Binding::Step {
                            node: node_name("child/secret"),
                            port: port("value"),
                        },
                    ),
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::SecretToNonSecretSink {
                from: (node_name("child/secret"), port("value")),
                to: port_site("render", "value"),
            }]
        );
    }

    /// Acceptance 5's last case: a secret child output bound to a
    /// secret-accepting parent port checks clean -- composition changes
    /// nothing here either.
    #[test]
    fn a_secret_child_output_into_a_secret_accepting_port_checks_clean() {
        let workflow = Workflow::new(workflow_name("w"))
            .node(
                node_name("child/secret"),
                Node::new(tool_name("doppler.secret.get"))
                    .port(
                        port("config"),
                        Binding::Literal("example-org/prd".to_string()),
                    )
                    .port(port("name"), Binding::Literal("EXAMPLE_SECRET".to_string())),
            )
            .node(
                node_name("secret_out"),
                Node::new(tool_name("github.actions_secret.ensure"))
                    .port(
                        port("repo"),
                        Binding::Literal("example-org/example-repo".to_string()),
                    )
                    .port(port("name"), Binding::Literal("EXAMPLE_SECRET".to_string()))
                    .port(
                        port("value"),
                        Binding::Step {
                            node: node_name("child/secret"),
                            port: port("value"),
                        },
                    ),
            );
        let catalog = test_catalog();
        check(&workflow, &catalog).expect("a secret flowing into a secret-accepting port is clean");
    }

    #[test]
    fn unknown_tool_names_the_node_and_tool() {
        let workflow = Workflow::new(workflow_name("w"))
            .node(node_name("mystery"), Node::new(tool_name("no.such.tool")));
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

    /// Milestone 2b, decision (d1) and acceptance 2: a [`Workflow`] whose
    /// `uses` map is non-empty reaches `check` unlinked and returns
    /// exactly [`CheckError::Unlinked`], naming the step -- before
    /// anything else is evaluated. Two unknown-tool nodes sit alongside
    /// the `uses:` step to prove it short-circuits rather than reporting
    /// `Unlinked` mixed in with every other error the workflow has.
    #[test]
    fn a_workflow_reaching_check_with_an_unlinked_uses_step_returns_exactly_unlinked() {
        let workflow = Workflow::new(workflow_name("w"))
            .node(node_name("mystery"), Node::new(tool_name("no.such.tool")))
            .uses(
                node_name("org"),
                Uses {
                    workflow: workflow_name("example-org"),
                    with: IndexMap::new(),
                    position: 0,
                },
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::Unlinked {
                node: node_name("org"),
            }]
        );
        assert_eq!(
            errors[0].to_string(),
            "node `org`: a `uses:` step reached check without being linked first"
        );
    }

    /// Two `uses:` steps reaching `check` unlinked each get their own
    /// [`CheckError::Unlinked`], in declaration order -- not just the
    /// first.
    #[test]
    fn two_unlinked_uses_steps_each_get_their_own_unlinked_error() {
        let workflow = Workflow::new(workflow_name("w"))
            .uses(
                node_name("org"),
                Uses {
                    workflow: workflow_name("example-org"),
                    with: IndexMap::new(),
                    position: 0,
                },
            )
            .uses(
                node_name("app"),
                Uses {
                    workflow: workflow_name("ios-app"),
                    with: IndexMap::new(),
                    position: 1,
                },
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![
                CheckError::Unlinked {
                    node: node_name("org"),
                },
                CheckError::Unlinked {
                    node: node_name("app"),
                },
            ]
        );
    }

    #[test]
    fn unknown_port_for_a_with_key_that_is_not_one_of_the_tools_ports() {
        let workflow = Workflow::new(workflow_name("w")).node(
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
                site: port_site("names", "bogus"),
                tool: tool_name("naming.v1"),
            }]
        );
    }

    #[test]
    fn unknown_port_for_a_step_reference_to_a_missing_output_port() {
        let workflow = Workflow::new(workflow_name("w"))
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
                site: port_site("names", "no_such_output"),
                tool: tool_name("naming.v1"),
            }]
        );
    }

    #[test]
    fn unknown_node_for_a_step_reference_to_a_nonexistent_node() {
        let workflow = Workflow::new(workflow_name("w")).node(
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
                site: port_site("repo", "repo"),
                referenced: node_name("ghost"),
            }]
        );
    }

    #[test]
    fn unbound_input_for_every_missing_required_port() {
        let workflow = Workflow::new(workflow_name("w"))
            .node(node_name("names"), Node::new(tool_name("naming.v1")));
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
        let workflow = Workflow::new(workflow_name("w")).node(
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
                site: port_site("names", "org"),
                input: input_name("org"),
            }]
        );
    }

    #[test]
    fn invalid_literal_for_a_bad_scalar_literal() {
        let workflow = Workflow::new(workflow_name("w"))
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
                derived_only: false,
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

        let workflow = Workflow::new(workflow_name("w")).node(
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
        let workflow = Workflow::new(workflow_name("w"))
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
        let workflow = Workflow::new(workflow_name("w"))
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
                        // The literal's shape does not matter here: `check`
                        // rejects any literal bound to a secret-accepting
                        // port before looking at its text. `concat!`-split
                        // anyway, so this file holds nothing a scanner reads
                        // as a real Doppler token.
                        Binding::Literal(
                            concat!("dp.st.prd.", "hunter2hunter2hunter2hunter2hunter2hunter2")
                                .to_string(),
                        ),
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
        let workflow = Workflow::new(workflow_name("w")).node(
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
        let workflow = Workflow::new(workflow_name("w"))
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
    fn identifier_for_each_source_names_the_node_and_the_identifier_type() {
        let error = CheckError::IdentifierForEachSource {
            node: node_name("loop"),
            ty: list_ty("AppleCertificateId"),
        };
        assert_eq!(
            error.to_string(),
            "node `loop`: for_each over `list<AppleCertificateId>`, an identifier type: its instance keys would print identifiers whole"
        );
        assert_eq!(error.kind(), "IdentifierForEachSource");
        let json = serde_json::to_value(&error).expect("CheckError must serialize");
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "IdentifierForEachSource",
                "node": "loop",
                "ty": "list<AppleCertificateId>",
            })
        );
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
                    site: port_site("n", "p"),
                    tool: tool_name("t.t"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::UnknownNode {
                    site: port_site("n", "p"),
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
                    site: port_site("n", "p"),
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
                    to: port_site("n", "p"),
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
                CheckError::IdentifierForEachSource {
                    node: node_name("n"),
                    ty: list_ty("AppleCertificateId"),
                },
                "n",
                None,
            ),
            (
                CheckError::ItemOutsideForEach {
                    site: port_site("n", "p"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::KeyedOnScalarNode {
                    site: port_site("n", "p"),
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
                CheckError::NestedList {
                    site: port_site("n", "p"),
                    referenced: node_name("each"),
                },
                "n",
                Some("p"),
            ),
            (
                CheckError::DuplicateNode {
                    node: node_name("n"),
                },
                "n",
                None,
            ),
            (
                CheckError::UnregisteredInputType {
                    input: input_name("x"),
                    ty: ty("Bogus"),
                },
                "x",
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
    fn unregistered_input_type_for_a_scalar_input() {
        let workflow = Workflow::new(workflow_name("w"))
            .input(input_name("mystery"), InputSpec::new(ty("NoSuchType")));
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UnregisteredInputType {
                input: input_name("mystery"),
                ty: ty("NoSuchType"),
            }]
        );
        assert_eq!(
            errors[0].to_string(),
            "input `mystery`: declared type `NoSuchType` is not a registered type"
        );
    }

    #[test]
    fn unregistered_input_type_for_a_list_input() {
        let workflow = Workflow::new(workflow_name("w"))
            .input(input_name("mystery"), InputSpec::new(list_ty("NoSuchType")));
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UnregisteredInputType {
                input: input_name("mystery"),
                ty: list_ty("NoSuchType"),
            }]
        );
    }

    #[test]
    fn unregistered_input_type_suppresses_the_default_type_mismatch_check() {
        // An unregistered type has nothing to check a default against, so
        // only one error is reported for this input, same as the secret
        // case already covers for `SecretWorkflowInput`.
        let workflow = Workflow::new(workflow_name("w")).input(
            input_name("mystery"),
            InputSpec::new(ty("NoSuchType")).with_default(Value::known(
                willikins_types::GitHubOrg::parse("lightless-labs").unwrap(),
            )),
        );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::UnregisteredInputType {
                input: input_name("mystery"),
                ty: ty("NoSuchType"),
            }]
        );
    }

    #[test]
    fn acknowledgement_default_refuses_a_default_on_an_operator_acknowledgement_input() {
        // Negative fixture (acceptance 16): a workflow input declared
        // `OperatorAcknowledgement` may never carry a default -- see
        // `docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, decision (j),
        // point 6.
        let workflow = Workflow::new(workflow_name("w")).input(
            input_name("done_ack"),
            InputSpec::new(ty("OperatorAcknowledgement")).with_default(Value::known(
                willikins_types::OperatorAcknowledgement::parse("done").unwrap(),
            )),
        );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::AcknowledgementDefault {
                input: input_name("done_ack"),
            }]
        );
        assert_eq!(
            errors[0].to_string(),
            "input `done_ack`: an operator acknowledgement input may not have a default"
        );
    }

    #[test]
    fn acknowledgement_literal_refuses_a_literal_bound_to_an_operator_acknowledgement_port() {
        // Negative fixture (acceptance 16): a literal cannot supply an
        // operator's acknowledgement -- it must come from a workflow input
        // the operator fills in themselves.
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(Arc::new(DummyTool {
                spec: spec_of(
                    "test.acknowledge",
                    &[
                        ("step", exact("Text"), true),
                        ("acknowledged", exact("OperatorAcknowledgement"), true),
                    ],
                    &[],
                    &[],
                    Class::Reversible,
                    true,
                ),
            }))
            .unwrap();
        let workflow = Workflow::new(workflow_name("w")).node(
            node_name("gate"),
            Node::new(tool_name("test.acknowledge"))
                .port(port("step"), Binding::Literal("do the thing".to_string()))
                .port(port("acknowledged"), Binding::Literal("done".to_string())),
        );
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![CheckError::AcknowledgementLiteral {
                node: node_name("gate"),
                port: port("acknowledged"),
            }]
        );
        assert_eq!(
            errors[0].to_string(),
            "node `gate`, port `acknowledged`: a literal cannot supply an operator acknowledgement"
        );
    }

    #[test]
    fn unregistered_input_type_is_reported_alongside_secret_workflow_input_in_declaration_order() {
        let workflow = Workflow::new(workflow_name("w"))
            .input(input_name("mystery"), InputSpec::new(ty("NoSuchType")))
            .input(
                input_name("token"),
                InputSpec::new(ty("DopplerServiceToken")),
            );
        let catalog = test_catalog();
        let errors = check(&workflow, &catalog).unwrap_err();
        assert_eq!(
            errors,
            vec![
                CheckError::UnregisteredInputType {
                    input: input_name("mystery"),
                    ty: ty("NoSuchType"),
                },
                CheckError::SecretWorkflowInput {
                    input: input_name("token"),
                    ty: ty("DopplerServiceToken"),
                },
            ]
        );
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

    // -------------------------------------------------------------
    // Serialization: internally tagged `{"kind": "<Variant>", ...}`
    // -------------------------------------------------------------

    /// Generates a wildcard-free `match` from a variant name to its `kind`
    /// tag *and* the variant count, from one list of names.
    ///
    /// Both halves matter, and neither closes the gap alone. The
    /// `match` makes a newly added variant a compile error (it is not
    /// exhaustive until the name is listed here), and listing the name
    /// bumps the count, which then fails the length assertion in the test
    /// below until a sample is added too. A hand-written count could not
    /// close that second half: adding a variant left the old count and the
    /// old sample list agreeing with each other, and the new variant
    /// escaped the test silently.
    ///
    /// `plan_error_serde.rs` carries its own copy for [`crate::PlanError`];
    /// an integration test cannot see a `#[cfg(test)]` macro in the lib.
    macro_rules! variant_kinds {
        ($fn_name:ident, $count:ident, $enum:ident, $($variant:ident),+ $(,)?) => {
            fn $fn_name(value: &$enum) -> &'static str {
                match value {
                    $($enum::$variant { .. } => stringify!($variant),)+
                }
            }

            const $count: usize = [$(stringify!($variant)),+].len();
        };
    }

    /// One instance of every [`CheckError`] variant. Kept in lockstep with
    /// the enum by `variant_kinds!` above and the length assertion in
    /// [`every_check_error_variant_serializes_with_its_kind`].
    #[allow(clippy::too_many_lines)] // one literal per variant; splitting it would only move the count, not reduce it
    fn check_error_samples() -> Vec<CheckError> {
        vec![
            CheckError::UnknownTool {
                node: node_name("n"),
                tool: tool_name("bogus.tool"),
            },
            CheckError::UnknownPort {
                site: port_site("n", "p"),
                tool: tool_name("bogus.tool"),
            },
            CheckError::UnknownNode {
                site: port_site("n", "p"),
                referenced: node_name("ghost"),
            },
            CheckError::UnboundInput {
                node: node_name("n"),
                port: port("p"),
            },
            CheckError::UndeclaredInput {
                site: port_site("n", "p"),
                input: input_name("i"),
            },
            CheckError::InvalidLiteral {
                node: node_name("n"),
                port: port("p"),
                error: ParseError::new("Test", "boom"),
            },
            CheckError::TypeMismatch {
                node: node_name("n"),
                port: port("p"),
                expected: exact("GitHubOrg"),
                found: ty("HttpsUrl"),
            },
            CheckError::SecretLiteral {
                node: node_name("n"),
                port: port("p"),
            },
            CheckError::UnderivedBinding {
                node: node_name("n"),
                port: port("p"),
            },
            CheckError::SecretToNonSecretSink {
                from: (node_name("a"), port("out")),
                to: port_site("b", "in"),
            },
            CheckError::SecretWorkflowInput {
                input: input_name("i"),
                ty: ty("DopplerServiceToken"),
            },
            CheckError::SecretForEachSource {
                node: node_name("n"),
            },
            CheckError::ForEachOverScalar {
                node: node_name("n"),
            },
            CheckError::IdentifierForEachSource {
                node: node_name("n"),
                ty: list_ty("AppleCertificateId"),
            },
            CheckError::ItemOutsideForEach {
                site: port_site("n", "p"),
            },
            CheckError::KeyedOnScalarNode {
                site: port_site("n", "p"),
                referenced: node_name("ref"),
            },
            CheckError::Cycle {
                nodes: vec![node_name("n")],
            },
            CheckError::DefaultTypeMismatch {
                input: input_name("i"),
                expected: ty("GitHubOrg"),
                found: ty("HttpsUrl"),
            },
            CheckError::NestedList {
                site: port_site("n", "p"),
                referenced: node_name("ref"),
            },
            CheckError::DuplicateNode {
                node: node_name("n"),
            },
            CheckError::UnregisteredInputType {
                input: input_name("i"),
                ty: ty("NoSuchType"),
            },
            CheckError::DuplicateForEachDefault {
                node: node_name("n"),
                input: input_name("i"),
                key: "k".to_string(),
            },
            CheckError::LiteralOutput {
                output: OutputName::parse("o").unwrap(),
            },
            CheckError::AcknowledgementDefault {
                input: input_name("i"),
            },
            CheckError::AcknowledgementLiteral {
                node: node_name("n"),
                port: port("p"),
            },
            CheckError::ListOnScalarPort {
                site: port_site("n", "p"),
                expected: exact("GitHubOrg"),
            },
            CheckError::ListElementTypeMismatch {
                site: Site::list_element(node_name("n"), port("p"), 0),
                expected: ty("GitHubOrg"),
                found: ty("HttpsUrl"),
            },
            CheckError::SequenceNotAllowedHere {
                site: Site::ForEach {
                    node: node_name("n"),
                },
            },
            CheckError::DisallowedInputType {
                input: input_name("i"),
                ty: ty("TemplateSource"),
            },
            CheckError::RepoFileLiteral {
                node: node_name("n"),
                port: port("p"),
            },
            CheckError::Unlinked {
                node: node_name("n"),
            },
            CheckError::UsesCycle {
                chain: vec![workflow_name("a"), workflow_name("b"), workflow_name("a")],
            },
            CheckError::UsesTooDeep {
                chain: vec![workflow_name("a"), workflow_name("b")],
            },
            CheckError::UsesTooLarge { nodes: 2049 },
            CheckError::UnknownWorkflow {
                node: node_name("n"),
                workflow: workflow_name("ghost"),
            },
            CheckError::UsedDocument {
                node: node_name("n"),
                workflow: workflow_name("ghost"),
                reason: "boom".to_string(),
            },
            CheckError::UnknownUsesInput {
                node: node_name("n"),
                input: input_name("i"),
            },
            CheckError::UnboundUsesInput {
                node: node_name("n"),
                input: input_name("i"),
            },
            CheckError::UnknownUsesOutput {
                site: port_site("n", "p"),
                node: node_name("n"),
                output: OutputName::parse("o").unwrap(),
            },
            CheckError::ItemInUses {
                node: node_name("n"),
                input: input_name("i"),
            },
            CheckError::KeyedOnUses {
                site: port_site("n", "p"),
                node: node_name("n"),
            },
            CheckError::UsesOutputCycle {
                node: node_name("n"),
                output: OutputName::parse("o").unwrap(),
            },
            CheckError::PathInAuthoredName {
                name: "a/b".to_string(),
            },
            CheckError::UsesInputTypeMismatch {
                node: node_name("n"),
                input: input_name("i"),
                expected: ty("DopplerProject"),
                found: ty("GitHubRepo"),
            },
        ]
    }

    variant_kinds!(
        check_error_kind_of,
        CHECK_ERROR_VARIANT_COUNT,
        CheckError,
        UnknownTool,
        UnknownPort,
        UnknownNode,
        UnboundInput,
        UndeclaredInput,
        InvalidLiteral,
        TypeMismatch,
        SecretLiteral,
        UnderivedBinding,
        SecretToNonSecretSink,
        SecretWorkflowInput,
        SecretForEachSource,
        ForEachOverScalar,
        IdentifierForEachSource,
        ItemOutsideForEach,
        KeyedOnScalarNode,
        Cycle,
        DefaultTypeMismatch,
        NestedList,
        DuplicateNode,
        UnregisteredInputType,
        DuplicateForEachDefault,
        LiteralOutput,
        AcknowledgementDefault,
        AcknowledgementLiteral,
        ListOnScalarPort,
        ListElementTypeMismatch,
        SequenceNotAllowedHere,
        DisallowedInputType,
        RepoFileLiteral,
        Unlinked,
        UsesCycle,
        UsesTooDeep,
        UsesTooLarge,
        UnknownWorkflow,
        UsedDocument,
        UnknownUsesInput,
        UnboundUsesInput,
        UnknownUsesOutput,
        ItemInUses,
        KeyedOnUses,
        UsesOutputCycle,
        PathInAuthoredName,
        UsesInputTypeMismatch,
    );

    #[test]
    fn every_check_error_variant_serializes_with_its_kind() {
        let samples = check_error_samples();
        assert_eq!(
            samples.len(),
            CHECK_ERROR_VARIANT_COUNT,
            "check_error_samples must carry exactly one sample per CheckError variant"
        );
        let mut seen_kinds: HashSet<&'static str> = HashSet::new();
        for sample in &samples {
            let kind = check_error_kind_of(sample);
            assert!(
                seen_kinds.insert(kind),
                "duplicate sample for CheckError::{kind}"
            );
            let json = serde_json::to_value(sample).expect("CheckError must serialize");
            assert_eq!(json["kind"], kind, "sample: {sample:?}");
            // No variant declares a field named `message`: if one did, this
            // assertion -- run over every variant -- would catch it, since
            // such a field would show up here even though `Reported` (which
            // adds its own `message`) is not involved yet. A field literally
            // named `kind` is refused at compile time by serde's derive
            // under `#[serde(tag = "kind")]` (duplicate field), so that half
            // of the invariant has a compiler backstop; this is the runtime
            // half, for `message`.
            assert!(
                json.as_object().unwrap().get("message").is_none(),
                "CheckError::{kind} must not have a field named `message`: {json}"
            );
        }
        assert_eq!(seen_kinds.len(), CHECK_ERROR_VARIANT_COUNT);
    }

    /// [`CheckError::kind`] (the library-side answer to
    /// `todos/2026-09-12-error-json-uniformity-gaps.md` item 3: a caller
    /// that wants the variant name as a string, such as
    /// `willikins-cli`'s text renderer, should be able to ask the enum
    /// rather than hand-maintain its own copy) agrees with both the
    /// `kind` this test's own `variant_kinds!`-generated `check_error_kind_of`
    /// names and the tag `Serialize` actually puts on the wire, for every
    /// variant.
    #[test]
    fn kind_agrees_with_the_serialized_tag_for_every_variant() {
        for sample in check_error_samples() {
            let expected = check_error_kind_of(&sample);
            assert_eq!(sample.kind(), expected, "sample: {sample:?}");
            let json = serde_json::to_value(&sample).expect("CheckError must serialize");
            assert_eq!(json["kind"], sample.kind(), "sample: {sample:?}");
        }
    }

    variant_kinds!(
        check_warning_kind_of,
        CHECK_WARNING_VARIANT_COUNT,
        CheckWarning,
        UnusedInput,
    );

    /// One instance of every [`CheckWarning`] variant. One today; guarded
    /// the same way [`check_error_samples`] is, so a second warning cannot
    /// reach an agent without this test covering it.
    fn check_warning_samples() -> Vec<CheckWarning> {
        vec![CheckWarning::UnusedInput {
            input: input_name("slug"),
        }]
    }

    #[test]
    fn every_check_warning_variant_serializes_with_its_kind() {
        let samples = check_warning_samples();
        assert_eq!(
            samples.len(),
            CHECK_WARNING_VARIANT_COUNT,
            "check_warning_samples must carry exactly one sample per CheckWarning variant"
        );
        let mut seen_kinds: HashSet<&'static str> = HashSet::new();
        for sample in &samples {
            let kind = check_warning_kind_of(sample);
            assert!(
                seen_kinds.insert(kind),
                "duplicate sample for CheckWarning::{kind}"
            );
            let json = serde_json::to_value(sample).expect("CheckWarning must serialize");
            assert_eq!(json["kind"], kind, "sample: {sample:?}");
            assert!(
                json.as_object().unwrap().get("message").is_none(),
                "CheckWarning::{kind} must not have a field named `message`: {json}"
            );
        }
        assert_eq!(seen_kinds.len(), CHECK_WARNING_VARIANT_COUNT);
    }

    // -------------------------------------------------------------
    // Milestone 3d, decision (c): the tripwire. `probe_conversion` is
    // `willikins-core`'s own `check` module's one sanctioned caller,
    // enforced by `clippy.toml`'s `disallowed-methods`. This is the
    // second, independent enforcement: even an `#[allow]` smuggled into
    // `plan.rs`, `apply.rs` or `describe.rs` to silence clippy there would
    // still fail this test, because it does not run clippy at all -- it
    // greps the method's own name out of the source text.
    // -------------------------------------------------------------

    #[test]
    fn plan_apply_and_describe_never_name_probe_conversion() {
        // Assembled, not spelled out, so this test does not trip on its
        // own source when `cargo test` includes this file's text in
        // anything that greps the workspace (it does not today, but the
        // needle is assembled on principle, matching the plan).
        let needle = concat!("probe_", "conversion");
        for (path, source) in [
            ("plan.rs", include_str!("plan.rs")),
            ("apply.rs", include_str!("apply.rs")),
            ("describe.rs", include_str!("describe.rs")),
        ] {
            assert!(
                !source.contains(needle),
                "{path} names `{needle}`: only check.rs may resolve a conversion; plan and \
                 apply must apply the one check already recorded on a Checked edge"
            );
        }
    }

    /// `text`'s own body, up to (not including) its first top-level
    /// `#[cfg(test)]` module line, or the whole text when it has none --
    /// the same split `tests/it/ratchet.rs`'s own `non_test_body` uses for
    /// the unreachable ratchet, duplicated here (a different compiled
    /// unit) so a file's own test fixtures -- which legitimately build a
    /// `Binding` to feed `check` itself -- never count against the
    /// tripwire below.
    fn non_test_body(text: &str) -> String {
        match text.lines().position(|line| line.trim() == "#[cfg(test)]") {
            Some(index) => text.lines().take(index).collect::<Vec<_>>().join("\n"),
            None => text.to_string(),
        }
    }

    /// Strip `//` line comments and `/* ... */` block comments from
    /// `text`, leaving double-quoted string literals untouched -- the
    /// same algorithm `no_certificate_writes_guard.rs`'s own
    /// `strip_comments` uses, duplicated here for the same reason as
    /// [`non_test_body`]. Needed because plan.rs's and apply.rs's own
    /// module docs name `Binding` by the dozen in prose (milestone 3n,
    /// task G6's addendum, point 8): a plain substring search over raw
    /// text would false-positive on every one of those.
    fn strip_comments(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        let mut in_string = false;
        while let Some(c) = chars.next() {
            if in_string {
                out.push(c);
                if c == '\\' {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                    continue;
                }
                if c == '"' {
                    in_string = false;
                }
                continue;
            }
            if c == '"' {
                in_string = true;
                out.push(c);
                continue;
            }
            if c == '/' && chars.peek() == Some(&'/') {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
                continue;
            }
            if c == '/' && chars.peek() == Some(&'*') {
                chars.next(); // consume the `*`
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                continue;
            }
            out.push(c);
        }
        out
    }

    /// Milestone 3n, acceptance 7 (task G7): the `probe_conversion`
    /// tripwire above, extended. From G7 on, "no second lookup" is a type
    /// boundary -- `DocumentView` has no nodes, and `plan.rs`/`apply.rs`
    /// hold no `Binding` import at all -- but this stays belt-and-braces
    /// the same way `probe_conversion`'s own grep does: even an import
    /// smuggled back in would still fail this test. Checked against each
    /// file's own *non-test* body (fixtures legitimately build a
    /// `Binding` to feed `check`), comment-stripped (plan.rs's and
    /// apply.rs's own doc comments both still name `Binding` in prose).
    /// `describe.rs`'s `parse_raw` is excluded from the `Value::parse(`
    /// check -- the plan's own "one sanctioned place" (acceptance 7).
    #[test]
    fn plan_apply_and_describe_never_name_a_binding_or_a_second_lookup() {
        for (path, raw) in [
            ("plan.rs", include_str!("plan.rs")),
            ("apply.rs", include_str!("apply.rs")),
            ("describe.rs", include_str!("describe.rs")),
        ] {
            let body = strip_comments(&non_test_body(raw));
            assert!(
                !body.contains("Binding"),
                "{path}'s non-test code still names `Binding`: from G7 on, plan/apply/describe \
                 see only `check`'s own typed graph"
            );
            assert!(
                !body.contains("catalog.get("),
                "{path}'s non-test code still looks a tool up in a catalog by name"
            );
            assert!(
                !body.contains(".workflow("),
                "{path}'s non-test code still reads `Checked::workflow()`, the crate-private \
                 bridge G7 deletes"
            );
        }
        for (path, raw) in [
            ("plan.rs", include_str!("plan.rs")),
            ("apply.rs", include_str!("apply.rs")),
        ] {
            let body = strip_comments(&non_test_body(raw));
            assert!(
                !body.contains("Value::parse("),
                "{path}'s non-test code still parses a Value directly: `describe.rs`'s \
                 `parse_raw` is the one sanctioned place a caller's text becomes a Value"
            );
        }
    }

    /// The `EdgeTypeMismatch` backstop's own reason to exist: a `Checked`
    /// `check` never built. Moved here from `tests/it/apply.rs` by
    /// milestone 3n's decision (s2) -- once `Checked`'s fields are
    /// private (this task's own second half), this module is the only
    /// one left that can still move a real, `check`-built converting edge
    /// onto a different port than `check` chose it for. Real production
    /// types stand in for the integration test's own private `ConvA`/
    /// `ConvB` fixture, which this module cannot see: `AppleBundleIdentifier`
    /// converts to `Text` by one row the production registry already
    /// carries (`willikins_types::lib`'s `conversion_rows`), so
    /// `test_catalog`'s own global registry is enough -- no bespoke
    /// conversion table needed. `plan`'s own input parsing cannot see
    /// this -- both `a` and `c` are exactly the types their declared
    /// inputs say -- so only `Edge::deliver`'s own backstop catches it, at
    /// both `plan` and `apply`.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_converted_edge_moved_onto_another_port_errors_at_plan_and_apply() {
        let spec = spec_of(
            "test.sealed_sink",
            &[("b", exact("Text"), true)],
            &[],
            &[],
            Class::Reversible,
            false,
        );
        let mut catalog = Catalog::new(willikins_types::registry());
        catalog
            .insert(Arc::new(DummyTool { spec }))
            .expect("one tool, one insert");

        let workflow = Workflow::new(workflow_name("conv-forged"))
            .input(input_name("a"), InputSpec::new(ty("AppleBundleIdentifier")))
            .input(input_name("c"), InputSpec::new(ty("Text")))
            .node(
                node_name("sink1"),
                Node::new(tool_name("test.sealed_sink"))
                    .port(port("b"), Binding::Input(input_name("a"))),
            )
            .node(
                node_name("sink2"),
                Node::new(tool_name("test.sealed_sink"))
                    .port(port("b"), Binding::Input(input_name("c"))),
            );
        let checked = check(&workflow, &catalog)
            .expect("AppleBundleIdentifier converts to Text; Text binds to Text exactly");

        let converting_edge = checked
            .edge(&node_name("sink1"), &port("b"))
            .unwrap()
            .clone();
        assert!(converting_edge.conversion().is_some());
        assert!(
            checked
                .edge(&node_name("sink2"), &port("b"))
                .unwrap()
                .conversion()
                .is_none()
        );

        // Milestone 3n, G3: `plan` no longer delivers a scalar port
        // through `Checked::types` at all -- it reads the edge straight
        // off `sink2`'s own `PortEdge` (`Shape::Single::ports`), which is
        // what must be forged for this to still exercise the backstop.
        // `Checked::types` itself is left untouched (still correct), so
        // this is the moved edge *moving onto the wrong port* in the only
        // place that still matters.
        let mut forged = checked.clone();
        let sink2 = forged
            .nodes
            .iter_mut()
            .find(|node| node.name == node_name("sink2"))
            .expect("sink2 is one of this workflow's checked nodes");
        let Shape::Single { ports, .. } = &mut sink2.shape else {
            unreachable!("sink2 has no for_each")
        };
        let port_edge = ports
            .iter_mut()
            .find(|port_edge| port_edge.port == port("b"))
            .expect("sink2 has a bound scalar port b");
        let Bound::Scalar { edge, .. } = &mut port_edge.bound else {
            unreachable!("sink2's port b is a scalar port")
        };
        *edge = converting_edge;

        let mut inputs = IndexMap::new();
        inputs.insert(
            input_name("a"),
            Value::known(willikins_types::AppleBundleIdentifier::parse("com.example.app").unwrap()),
        );
        inputs.insert(
            input_name("c"),
            Value::known(willikins_types::Text::parse("hello").unwrap()),
        );

        let plan_err = crate::plan::plan(&forged, &inputs, &catalog).expect_err(
            "sink2's real Text does not match the moved edge's AppleBundleIdentifier source",
        );
        match plan_err {
            crate::plan::PlanError::EdgeTypeMismatch {
                site,
                expected,
                found,
            } => {
                assert_eq!(
                    site,
                    Site::Port {
                        node: node_name("sink2"),
                        port: port("b"),
                    }
                );
                assert_eq!(expected, ty("AppleBundleIdentifier"));
                assert_eq!(found, ty("Text"));
            }
            other => panic!("expected EdgeTypeMismatch, got {other:?}"),
        }

        // A genuine plan against the real (unforged) `Checked`, so `apply`
        // has an `approved: &Plan` to diff against -- its own opening
        // replan against the forged `Checked` is what surfaces the same
        // refusal for `apply`, before any drift check or `ensure` call.
        let genuine_planned =
            crate::plan::plan(&checked, &inputs, &catalog).expect("the real Checked plans");
        let mut observer = crate::apply::RecordingObserver::new();
        let apply_err = crate::apply::apply(
            &forged,
            &inputs,
            &catalog,
            &genuine_planned,
            &crate::apply::Approval::Auto,
            &mut observer,
        )
        .expect_err("apply's own opening replan hits the same forged edge");
        match apply_err {
            crate::apply::ApplyError::Plan {
                error:
                    crate::plan::PlanError::EdgeTypeMismatch {
                        site,
                        expected,
                        found,
                    },
            } => {
                assert_eq!(
                    site,
                    Site::Port {
                        node: node_name("sink2"),
                        port: port("b"),
                    }
                );
                assert_eq!(expected, ty("AppleBundleIdentifier"));
                assert_eq!(found, ty("Text"));
            }
            other => panic!("expected ApplyError::Plan{{EdgeTypeMismatch}}, got {other:?}"),
        }
        assert!(observer.events.is_empty(), "nothing was ever ensured");
    }

    // ---------------------------------------------------------------------
    // Milestone 3n, task G1: the node table (`CheckedNode`, `Shape`, typed
    // ids). `CheckedNode`, `Shape`, `SingleId` and `EachId` are
    // `pub(crate)`, so this test lives here rather than under `tests/it/`
    // (an integration test compiles as a separate crate and cannot see
    // crate-private items -- see the advisor's own note, and `check`'s
    // module doc).
    // ---------------------------------------------------------------------

    /// Acceptance 13's groundwork and verify item 7's own shape: a node
    /// with no `for_each` gets `Shape::Single`, a `for_each` node gets
    /// `Shape::Each` carrying its resolved element type, both carry the
    /// tool handle `check` resolved (never read back through the
    /// catalog), and `declared_at` is each node's own position in
    /// `workflow.nodes` (checked directly, this test being inside
    /// `check.rs` itself -- `Checked::declared_node`, the one bridge that
    /// read `declared_at` back into a `Node`, lost its last caller at G6
    /// and is gone). Ids are assigned in topological order within their own kind
    /// (SHARED VALUES); with no edge between these two nodes, topological
    /// order ties on declaration order, so `single` is id 0 among
    /// `Single` nodes and `each` is id 0 among `Each` nodes.
    #[test]
    fn check_builds_a_single_shape_and_an_each_shape_with_the_right_element_and_tool() {
        let catalog = {
            let mut catalog = Catalog::new(willikins_types::registry());
            catalog
                .insert(Arc::new(DummyTool {
                    spec: spec_of("test.single", &[], &[], &[], Class::Reversible, true),
                }))
                .unwrap();
            catalog
                .insert(Arc::new(DummyTool {
                    spec: spec_of("test.each", &[], &[], &[], Class::Reversible, true),
                }))
                .unwrap();
            catalog
        };
        let workflow = Workflow::new(workflow_name("node-table"))
            .input(input_name("ids"), InputSpec::new(list_ty("Text")))
            .node(node_name("single"), Node::new(tool_name("test.single")))
            .node(
                node_name("each"),
                Node::new(tool_name("test.each")).for_each(Binding::Input(input_name("ids"))),
            );

        let checked = check(&workflow, &catalog).expect("two independent nodes check cleanly");
        let nodes = checked.nodes();
        assert_eq!(
            nodes.len(),
            2,
            "one CheckedNode per workflow node, no more, no fewer"
        );

        let single = nodes
            .iter()
            .find(|n| n.name == node_name("single"))
            .expect("the single node has its own CheckedNode");
        assert_eq!(single.tool.spec().name, tool_name("test.single"));
        assert_eq!(single.declared_at, 0, "single is workflow.nodes' entry 0");
        match &single.shape {
            Shape::Single { id, ports } => {
                assert_eq!(id.0, 0, "the first Single node is id 0");
                assert!(
                    ports.is_empty(),
                    "test.single declares no input ports at all"
                );
            }
            Shape::Each { .. } => panic!("a node with no for_each must be Shape::Single"),
        }
        assert!(
            checked
                .workflow
                .nodes
                .get_index(single.declared_at)
                .expect("declared_at indexes workflow.nodes")
                .1
                .for_each
                .is_none(),
            "declared_at is the inverse of workflow.nodes' own position"
        );

        let each = nodes
            .iter()
            .find(|n| n.name == node_name("each"))
            .expect("the each node has its own CheckedNode");
        assert_eq!(each.tool.spec().name, tool_name("test.each"));
        assert_eq!(each.declared_at, 1, "each is workflow.nodes' entry 1");
        match &each.shape {
            Shape::Each {
                id,
                source,
                element,
                ports,
            } => {
                assert_eq!(id.0, 0, "the first Each node is id 0");
                assert_eq!(*element, ty("Text"), "the for_each source's element type");
                assert!(
                    matches!(source, Source::Input(_)),
                    "the for_each source is bound to a declared input"
                );
                assert!(ports.is_empty(), "test.each declares no input ports at all");
            }
            Shape::Single { .. } => panic!("a for_each node must be Shape::Each"),
        }
        assert!(
            checked
                .workflow
                .nodes
                .get_index(each.declared_at)
                .expect("declared_at indexes workflow.nodes")
                .1
                .for_each
                .is_some(),
            "declared_at is the inverse of workflow.nodes' own position"
        );
    }

    /// Acceptance 13: `format!("{checked:?}")` must compile at all --
    /// `CheckedNode` holds `Arc<dyn Tool>`, which is not `Debug`, so this
    /// is only true because `Checked`'s `Debug` is hand-written (decision
    /// (s3)) -- and it names every node and its tool. From G3 on, `Shape`'s
    /// own `ports` carry a real `Source::Literal(Value)` for each of these
    /// two literal-bound ports, so this now exercises that path for real:
    /// `Source<I>`'s derived `Debug` only ever forwards to `Value`'s own
    /// (which redacts by type, never by content), so a non-secret literal
    /// like `DB_PASSWORD` still prints -- `check` already refuses binding
    /// any secret-accepting port to a literal at all
    /// ([`CheckError::SecretLiteral`]), so `Source::Literal` can never
    /// hold a secret value to leak in the first place. The genuinely
    /// secret cases (a tool's secret *output* reaching `Source::Step`) are
    /// `check_adversarial.rs`'s own redaction tests, unaffected by this
    /// change and still green.
    #[test]
    fn debug_of_a_checked_workflow_compiles_and_names_every_node_and_its_tool() {
        let catalog = test_catalog();
        let workflow = Workflow::new(workflow_name("debug-shape")).node(
            node_name("secret"),
            Node::new(tool_name("doppler.secret.get"))
                .port(
                    port("config"),
                    Binding::Literal("example-org/prd".to_string()),
                )
                .port(port("name"), Binding::Literal("DB_PASSWORD".to_string())),
        );

        let checked = check(&workflow, &catalog).expect("one pure node checks cleanly");
        let rendered = format!("{checked:?}");
        assert!(
            rendered.contains("secret") && rendered.contains("doppler.secret.get"),
            "node and tool names must still print: {rendered}"
        );
        assert!(
            rendered.contains("DB_PASSWORD"),
            "a non-secret literal's own Source::Literal(Value) still prints through Value's own \
             Debug, unredacted (only a secret-*typed* value is redacted, and a literal can never \
             be one): {rendered}"
        );
    }

    // ---------------------------------------------------------------------
    // Milestone 3n, task G3: `Source`, `PortEdge`, `Bound::Scalar`,
    // `CheckedOutput` and the `for_each` source.
    // ---------------------------------------------------------------------

    /// One scalar port of each `Source` kind G3 builds, plus a
    /// `for_each` source and a workflow output -- every case the task
    /// touches, pinned together: a literal (`Source::Literal`, holding
    /// the parsed `Value`), a declared input (`Source::Input`), a `Step`
    /// onto a `Shape::Single` node (`Source::Step`), a `Step` onto a
    /// `Shape::Each` node (`Source::Aggregate`), a `Keyed` reference
    /// (`Source::Keyed`), the `for_each` source itself (also
    /// `Source::Input` here), the `${{ item }}` port inside the `for_each`
    /// node (`Source::Item`), and a workflow output resolving to
    /// `Source::Step` (`CheckedOutput`). `with_position` is also pinned,
    /// at the ports' own `with:` declaration order.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn check_builds_every_source_kind_from_g1s_representation() {
        let catalog = {
            let mut catalog = Catalog::new(willikins_types::registry());
            catalog
                .insert(Arc::new(DummyTool {
                    spec: spec_of(
                        "test.producer",
                        &[],
                        &[("out", ty("Text"))],
                        &[],
                        Class::Reversible,
                        true,
                    ),
                }))
                .unwrap();
            catalog
                .insert(Arc::new(DummyTool {
                    spec: spec_of(
                        "test.looped",
                        &[("item_val", exact("Text"), true)],
                        &[("val", ty("Text"))],
                        &[],
                        Class::Reversible,
                        true,
                    ),
                }))
                .unwrap();
            catalog
                .insert(Arc::new(DummyTool {
                    spec: spec_of(
                        "test.sink",
                        &[
                            ("lit", exact("Text"), true),
                            ("from_input", exact("Text"), true),
                            ("from_step", exact("Text"), true),
                            ("agg", PortType::Exact(list_ty("Text")), true),
                            ("keyed", exact("Text"), true),
                        ],
                        &[("done", ty("Text"))],
                        &[],
                        Class::Reversible,
                        true,
                    ),
                }))
                .unwrap();
            catalog
        };

        let workflow = Workflow::new(workflow_name("g3-sources"))
            .input(input_name("name_input"), InputSpec::new(ty("Text")))
            .input(input_name("items"), InputSpec::new(list_ty("Text")))
            .node(node_name("producer"), Node::new(tool_name("test.producer")))
            .node(
                node_name("looped"),
                Node::new(tool_name("test.looped"))
                    .for_each(Binding::Input(input_name("items")))
                    .port(port("item_val"), Binding::Item),
            )
            .node(
                node_name("sink"),
                Node::new(tool_name("test.sink"))
                    .port(port("lit"), Binding::Literal("hello".to_string()))
                    .port(port("from_input"), Binding::Input(input_name("name_input")))
                    .port(
                        port("from_step"),
                        Binding::Step {
                            node: node_name("producer"),
                            port: port("out"),
                        },
                    )
                    .port(
                        port("agg"),
                        Binding::Step {
                            node: node_name("looped"),
                            port: port("val"),
                        },
                    )
                    .port(
                        port("keyed"),
                        Binding::Keyed {
                            node: node_name("looped"),
                            key: "k".to_string(),
                            port: port("val"),
                        },
                    ),
            )
            .output(
                OutputName::parse("final_output").unwrap(),
                Binding::Step {
                    node: node_name("producer"),
                    port: port("out"),
                },
            );

        let checked = check(&workflow, &catalog).expect("every binding above type-checks");

        let producer = checked
            .nodes()
            .iter()
            .find(|n| n.name == node_name("producer"))
            .expect("producer has its own CheckedNode");
        let Shape::Single {
            id: producer_id, ..
        } = producer.shape
        else {
            unreachable!("producer has no for_each")
        };

        let looped = checked
            .nodes()
            .iter()
            .find(|n| n.name == node_name("looped"))
            .expect("looped has its own CheckedNode");
        let Shape::Each {
            id: looped_id,
            source: looped_source,
            ports: looped_ports,
            ..
        } = &looped.shape
        else {
            unreachable!("looped has a for_each")
        };
        assert!(
            matches!(looped_source, Source::Input(_)),
            "looped's for_each source is the declared input `items`"
        );
        assert_eq!(looped_ports.len(), 1, "looped binds exactly one port");
        assert_eq!(looped_ports[0].port, port("item_val"));
        assert_eq!(looped_ports[0].with_position, 0);
        assert!(
            matches!(
                looped_ports[0].bound,
                Bound::Scalar {
                    source: Source::Item(_),
                    ..
                }
            ),
            "item_val is bound to `${{{{ item }}}}`"
        );

        let sink = checked
            .nodes()
            .iter()
            .find(|n| n.name == node_name("sink"))
            .expect("sink has its own CheckedNode");
        let Shape::Single {
            ports: sink_ports, ..
        } = &sink.shape
        else {
            unreachable!("sink has no for_each")
        };
        assert_eq!(
            sink_ports.len(),
            5,
            "sink binds every one of its five declared ports"
        );
        for (index, port_name) in ["lit", "from_input", "from_step", "agg", "keyed"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                sink_ports[index].port,
                port(port_name),
                "ports() keeps the tool spec's own input order"
            );
            assert_eq!(
                sink_ports[index].with_position, index,
                "with_position matches this port's `with:` declaration order"
            );
        }

        let Bound::Scalar {
            source: lit_source, ..
        } = &sink_ports[0].bound
        else {
            panic!("expected Bound::Scalar, got {:?}", sink_ports[0].bound);
        };
        match lit_source {
            Source::Literal(value) => assert_eq!(
                value.render().to_string(),
                "hello",
                "the literal's own parsed Value is stored, not re-derived"
            ),
            other => panic!("expected Source::Literal, got {other:?}"),
        }

        let Bound::Scalar {
            source: input_source,
            ..
        } = &sink_ports[1].bound
        else {
            panic!("expected Bound::Scalar, got {:?}", sink_ports[1].bound);
        };
        assert!(
            matches!(input_source, Source::Input(_)),
            "from_input is bound to the declared input `name_input`"
        );

        let Bound::Scalar {
            source: step_source,
            ..
        } = &sink_ports[2].bound
        else {
            panic!("expected Bound::Scalar, got {:?}", sink_ports[2].bound);
        };
        match step_source {
            Source::Step { from, .. } => assert_eq!(
                *from, producer_id,
                "from_step names producer's own SingleId"
            ),
            other => panic!("expected Source::Step, got {other:?}"),
        }

        let Bound::Scalar {
            source: agg_source, ..
        } = &sink_ports[3].bound
        else {
            panic!("expected Bound::Scalar, got {:?}", sink_ports[3].bound);
        };
        match agg_source {
            Source::Aggregate { from, .. } => {
                assert_eq!(*from, *looped_id, "agg names looped's own EachId");
            }
            other => panic!("expected Source::Aggregate, got {other:?}"),
        }

        let Bound::Scalar {
            source: keyed_source,
            ..
        } = &sink_ports[4].bound
        else {
            panic!("expected Bound::Scalar, got {:?}", sink_ports[4].bound);
        };
        match keyed_source {
            Source::Keyed { from, key, .. } => {
                assert_eq!(*from, *looped_id, "keyed names looped's own EachId");
                assert_eq!(key, "k", "the instance key rides unvalidated, as authored");
            }
            other => panic!("expected Source::Keyed, got {other:?}"),
        }

        assert_eq!(checked.outputs().len(), 1, "one declared output");
        let output = &checked.outputs()[0];
        assert_eq!(output.name, OutputName::parse("final_output").unwrap());
        assert_eq!(output.ty, ty("Text"));
        match &output.source {
            Source::Step { from, .. } => {
                assert_eq!(
                    *from, producer_id,
                    "the output names producer's own SingleId"
                );
            }
            other => panic!("expected Source::Step, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------------
    // Milestone 3n, task G4: `Bound::List`.
    // ---------------------------------------------------------------------

    /// A list-bound port gets its own [`PortEdge`], `Bound::List`,
    /// with one classified [`Source`] per element -- a literal
    /// (`Source::Literal`, holding the parsed `Value`, not re-parsed), a
    /// declared input (`Source::Input`) and a `Step` reference
    /// (`Source::Step`) -- aligned with the recorded [`Edge::list`]'s own
    /// [`Edge::elements`], in declaration order.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn check_builds_bound_list_with_one_source_per_element() {
        let catalog = {
            let mut catalog = Catalog::new(willikins_types::registry());
            catalog
                .insert(Arc::new(DummyTool {
                    spec: spec_of(
                        "test.producer",
                        &[],
                        &[("out", ty("Text"))],
                        &[],
                        Class::Reversible,
                        true,
                    ),
                }))
                .unwrap();
            catalog
                .insert(Arc::new(DummyTool {
                    spec: spec_of(
                        "test.sink",
                        &[("items", PortType::Exact(list_ty("Text")), true)],
                        &[],
                        &[],
                        Class::Reversible,
                        true,
                    ),
                }))
                .unwrap();
            catalog
        };

        let workflow = Workflow::new(workflow_name("w"))
            .input(input_name("name_input"), InputSpec::new(ty("Text")))
            .node(node_name("producer"), Node::new(tool_name("test.producer")))
            .node(
                node_name("sink"),
                Node::new(tool_name("test.sink")).port(
                    port("items"),
                    Binding::List(vec![
                        Binding::Literal("hello".to_string()),
                        Binding::Input(input_name("name_input")),
                        Binding::Step {
                            node: node_name("producer"),
                            port: port("out"),
                        },
                    ]),
                ),
            );

        let checked = check(&workflow, &catalog).expect("every list element type-checks");

        let producer = checked
            .nodes()
            .iter()
            .find(|n| n.name == node_name("producer"))
            .expect("producer has its own CheckedNode");
        let Shape::Single {
            id: producer_id, ..
        } = producer.shape
        else {
            unreachable!("producer has no for_each")
        };

        let sink = checked
            .nodes()
            .iter()
            .find(|n| n.name == node_name("sink"))
            .expect("sink has its own CheckedNode");
        let Shape::Single { ports, .. } = &sink.shape else {
            unreachable!("sink has no for_each")
        };
        assert_eq!(ports.len(), 1, "sink binds its one list port");
        assert_eq!(ports[0].port, port("items"));
        assert_eq!(ports[0].with_position, 0);

        let Bound::List { elements, edge } = &ports[0].bound else {
            panic!("expected Bound::List, got {:?}", ports[0].bound);
        };
        assert_eq!(
            edge.ty(),
            &ty("Text"),
            "the edge's own type is the declared element type"
        );
        let element_edges = edge
            .elements()
            .expect("a Bound::List's own edge is always Edge::list");
        assert_eq!(
            elements.len(),
            element_edges.len(),
            "one Source per element edge"
        );
        assert_eq!(elements.len(), 3, "the port's own three elements");

        match &elements[0] {
            Source::Literal(value) => assert_eq!(
                value.render().to_string(),
                "hello",
                "a list literal's own parsed Value is stored, not re-derived"
            ),
            other => panic!("expected Source::Literal, got {other:?}"),
        }
        assert!(
            matches!(elements[1], Source::Input(_)),
            "the second element is bound to the declared input `name_input`"
        );
        match &elements[2] {
            Source::Step { from, .. } => {
                assert_eq!(
                    *from, producer_id,
                    "the third element names producer's own SingleId"
                );
            }
            other => panic!("expected Source::Step, got {other:?}"),
        }
    }
}
