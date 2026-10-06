//! The linker: flattens a composite [`Workflow`]'s `uses:` steps into one
//! flat graph, so [`crate::check::check`] (and, downstream, `describe`,
//! `plan`, the journal, and the approvals page) see exactly one kind of
//! graph (milestone 2b, `docs/plans/2026-10-05-milestone-2b-composition.md`,
//! part L, decision (d2)).
//!
//! [`link`] replaces every `uses:` step with the used document's own
//! nodes, renamed `<step>/<node>` (decision (d3)), with the caller's
//! bindings substituted for the used document's inputs (decision (d4)).
//! An unbound defaulted input of a used document becomes a *fixed* input
//! of the flat workflow instead (decision (d6)), and a reference to one of
//! a used document's outputs resolves, at link time, to that output's own
//! binding, rewritten into the flat namespace (decision (d7)).
//!
//! # Scope (task L1: "the linker, structure")
//!
//! This module implements the *structure* of linking: flattening,
//! substitution, fixed inputs, and output aliasing, each exercised only
//! along a happy path (an in-memory resolver that always succeeds, a
//! document whose boundaries are all legitimately bound or legitimately
//! defaulted). It deliberately does **not** implement the linker's own
//! refusals (milestone 2b task L2) or the boundary's type-checking rules
//! (task C1): a site this module cannot yet handle correctly calls
//! [`unimplemented!`] instead of fabricating a [`crate::check::CheckError`]
//! variant that does not exist yet (adding one here would need a
//! `render.rs` match arm in `willikins-cli`, outside this task's scope —
//! see the plan's Gates section). Every such site is listed below so L2
//! can find them:
//!
//! - [`ResolveFailure`] returned by the caller's `resolve` closure
//!   (`UnknownWorkflow` / `UsedDocument`).
//! - A used document's required input left both unbound and undefaulted
//!   (`UnboundUsesInput`).
//! - A `uses:` step's `with:` naming an input the used document never
//!   declared (`UnknownUsesInput`) — silently ignored rather than panicking,
//!   since a document that reaches here has already gone through the DSL
//!   (or, in a unit test, is assumed well-formed); L2 adds the refusal.
//! - A reference to a used document's output it never declared
//!   (`UnknownUsesOutput`).
//! - A `Keyed` reference onto a `uses:` step (`KeyedOnUses`).
//! - An alias cycle between two `uses:` steps' outputs (`UsesOutputCycle`):
//!   [`ensure_local_subst`] recurses with no visiting set, so a genuine
//!   cycle overflows the stack instead of being refused.
//!
//! None of these are reachable from this module's own tests, which use
//! only an always-succeeding in-memory resolver over well-formed
//! documents.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::check::CheckError;
use crate::workflow::{Binding, InputName, InputSpec, Node, NodeName, OutputName, Workflow};
use willikins_types::WorkflowName;

/// Why a `uses:` step's referenced workflow could not be resolved; the
/// linker's `resolve` callback returns this instead of a `Workflow`
/// (milestone 2b task L1's signature; task L2 is what makes `link` turn
/// these into `CheckError`s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveFailure {
    /// No document by that name exists in the trusted source.
    NotFound,
    /// A document exists by that name, but resolving it is refused —
    /// deliberately not told apart from [`Self::NotFound`] at this type
    /// (a symlink and a name/file mismatch share one `CheckError` variant,
    /// milestone 2b acceptance 9): a caller probing for either case learns
    /// nothing a legitimate lookup would not already tell it.
    Refused,
    /// The named document exists and was read, but failed to parse or
    /// validate on its own terms.
    Document {
        /// Why the document itself was refused.
        message: String,
    },
}

/// One `uses:` step's input boundary: one used document's declared input,
/// and how the parent that uses it bound it (or didn't). Recorded only by
/// [`link`], across every `uses:` step in the whole tree that was linked
/// into a [`Workflow`] — see [`Workflow::boundaries`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct Boundary {
    /// The `uses:` step this boundary belongs to, as a flat node path
    /// (`org/base_gate`'s own enclosing step would be `org`; a boundary
    /// bubbled up from a step nested two levels down is `mid/leafstep`).
    pub uses: NodeName,
    /// The used document's own name.
    pub workflow: WorkflowName,
    /// The used document's own input name (always one segment: a used
    /// document's declared inputs are always authored, never themselves
    /// paths).
    pub input: InputName,
    /// The used document's own declared shape for this input.
    pub spec: InputSpec,
    /// The parent's binding for this input, already rewritten into the
    /// flat namespace — `None` when the parent left it unbound and it
    /// became a fixed input instead (decision (d6)).
    pub binding: Option<Binding>,
}

/// The result of [`link`]ing a composite [`Workflow`].
#[derive(Debug, Clone)]
pub struct Linked {
    /// The flattened workflow: `workflow.uses` is always empty.
    pub workflow: Workflow,
    /// Every workflow `link` resolved, directly or transitively, each
    /// listed once, in first-resolution order (a depth-first walk: a
    /// `uses:` step's own name is pushed before recursing into it).
    pub used: Vec<WorkflowName>,
}

/// Flatten `root`'s `uses:` tree into one graph, resolving every used
/// document through `resolve`.
///
/// # Errors
///
/// Structurally returns `Result` to match the shape `check` and the rest
/// of the crate use, but this task (L1, "the linker, structure") never
/// actually produces the `Err` case — see the module docs' "Scope"
/// section for exactly which sites are deferred to task L2, which is
/// where a resolve failure or a boundary refusal turns into a real
/// [`CheckError`].
#[allow(clippy::missing_panics_doc)] // every panic site is the module docs' "Scope" list, by design
pub fn link(
    root: &Workflow,
    resolve: &mut dyn FnMut(&WorkflowName) -> Result<Workflow, ResolveFailure>,
) -> Result<Linked, Vec<CheckError>> {
    let mut used = Vec::new();
    let flat = flatten(root, resolve, &mut used);

    let mut workflow = Workflow::new(root.name.clone());
    if let Some(description) = &root.description {
        workflow = workflow.with_description(description.clone());
    }
    workflow.inputs = flat.inputs;
    workflow.nodes = flat.nodes;
    workflow.outputs = flat.outputs;
    workflow.boundaries = flat.boundaries;

    Ok(Linked { workflow, used })
}

/// `wf`, fully flattened as if it were itself the top-level document: its
/// own authored inputs and nodes, plus every fixed input and renamed node
/// its own `uses:` tree contributes, and its own outputs and boundaries,
/// each rewritten into `wf`'s own namespace. [`link`] calls this once, on
/// `root`; it calls itself recursively, once per `uses:` step, before
/// embedding the result one level up.
struct Flat {
    inputs: IndexMap<InputName, InputSpec>,
    nodes: IndexMap<NodeName, Node>,
    outputs: IndexMap<OutputName, Binding>,
    boundaries: Vec<Boundary>,
}

#[allow(clippy::too_many_lines)] // one function, one flattening pass; see `check`'s own precedent
fn flatten(
    wf: &Workflow,
    resolve: &mut dyn FnMut(&WorkflowName) -> Result<Workflow, ResolveFailure>,
    used: &mut Vec<WorkflowName>,
) -> Flat {
    // Phase 1: resolve and recursively flatten every `uses:` step's own
    // child, independent of this level's own step order -- a sibling's
    // `with:` may reference another sibling's output regardless of which
    // one is declared first (decision (d7)).
    let mut children: IndexMap<NodeName, Flat> = IndexMap::new();
    for (step, uses) in &wf.uses {
        if !used.iter().any(|w| w == &uses.workflow) {
            used.push(uses.workflow.clone());
        }
        let child_wf = match resolve(&uses.workflow) {
            Ok(child_wf) => child_wf,
            Err(_failure) => unimplemented!(
                "milestone 2b task L2: a uses: step's workflow failed to resolve \
                 (CheckError::UnknownWorkflow / UsedDocument); step `{step}`, workflow \
                 `{}`",
                uses.workflow
            ),
        };
        children.insert(step.clone(), flatten(&child_wf, resolve, used));
    }

    // Phase 2: every step's own substitution map for its child's authored
    // inputs, resolved lazily (and memoized) so a sibling reference
    // resolves regardless of declaration order.
    let mut local_substs: IndexMap<NodeName, IndexMap<InputName, Binding>> = IndexMap::new();
    for step in wf.uses.keys() {
        ensure_local_subst(step, wf, &children, &mut local_substs);
    }

    // Phase 3a: this level's own direct boundaries, plus every bubbled-up
    // boundary from each child's own tree.
    let mut boundaries = Vec::new();
    for (step, uses) in &wf.uses {
        let child_flat = &children[step];
        let subst = &local_substs[step];
        for (input, spec) in &child_flat.inputs {
            if spec.fixed_by.is_some() {
                continue; // not this child's own authored surface; see 3b.
            }
            boundaries.push(Boundary {
                uses: step.clone(),
                workflow: uses.workflow.clone(),
                input: input.clone(),
                spec: spec.clone(),
                binding: uses.with.get(input).map(|_| subst[input].clone()),
            });
        }
        for bubbled in &child_flat.boundaries {
            boundaries.push(Boundary {
                uses: prefixed_node(step, &bubbled.uses),
                workflow: bubbled.workflow.clone(),
                input: bubbled.input.clone(),
                spec: bubbled.spec.clone(),
                binding: bubbled
                    .binding
                    .as_ref()
                    .map(|binding| embed(binding, step, subst)),
            });
        }
    }

    // Phase 3b: this level's own inputs -- its authored ones, unchanged,
    // plus every fixed input a `uses:` step contributes (freshly fixed
    // here, or bubbled up already fixed deeper).
    let mut inputs = wf.inputs.clone();
    for (step, uses) in &wf.uses {
        let child_flat = &children[step];
        for (input, spec) in &child_flat.inputs {
            if let Some(deeper) = &spec.fixed_by {
                let mut fixed = spec.clone();
                fixed.fixed_by = Some(prefixed_node(step, deeper));
                inputs.insert(prefixed_input(step, input), fixed);
            } else if !uses.with.contains_key(input) {
                if spec.default.is_none() {
                    // Required, unbound, undefaulted -- the same case
                    // `ensure_local_subst` leaves out of its own
                    // substitution map; see the module docs' "Scope"
                    // list (`CheckError::UnboundUsesInput`, task L2).
                    unimplemented!(
                        "milestone 2b task L2: an unbound required child input reached the \
                         linker (CheckError::UnboundUsesInput); step `{step}`, input `{input}`"
                    );
                }
                let mut fixed = spec.clone();
                fixed.fixed_by = Some(step.clone());
                inputs.insert(prefixed_input(step, input), fixed);
            }
            // else: bound via `with:`, consumed into this step's own
            // substitution map, never exposed as an input here.
        }
    }

    // Phase 3c: nodes, in document step order (SHARED VALUES, "Step
    // order"): walk every index a `steps:` map could hold; a `uses:`
    // step's position inserts its child's own nodes, in the child's own
    // order, each renamed `<step>/<child node>`; every other index takes
    // the next tool node, in `wf.nodes`' own declaration order.
    let mut by_position: HashMap<usize, &NodeName> = HashMap::new();
    for (step, uses) in &wf.uses {
        by_position.insert(uses.position, step);
    }
    let total = wf.nodes.len() + wf.uses.len();
    let mut own_nodes = wf.nodes.iter();
    let mut nodes: IndexMap<NodeName, Node> = IndexMap::new();
    for i in 0..total {
        if let Some(&step) = by_position.get(&i) {
            let child_flat = &children[step];
            let subst = &local_substs[step];
            for (child_node, node) in &child_flat.nodes {
                let embedded = Node {
                    tool: node.tool.clone(),
                    for_each: node.for_each.as_ref().map(|b| embed(b, step, subst)),
                    with: node
                        .with
                        .iter()
                        .map(|(port, b)| (port.clone(), embed(b, step, subst)))
                        .collect(),
                };
                nodes.insert(prefixed_node(step, child_node), embedded);
            }
        } else {
            let (name, node) = own_nodes.next().unwrap_or_else(|| {
                unreachable!("the position walk visits exactly `wf.nodes.len()` non-`uses:` slots")
            });
            let rewritten = Node {
                tool: node.tool.clone(),
                for_each: node
                    .for_each
                    .as_ref()
                    .map(|b| rewrite_at_level(b, wf, &children, &mut local_substs)),
                with: node
                    .with
                    .iter()
                    .map(|(port, b)| {
                        (
                            port.clone(),
                            rewrite_at_level(b, wf, &children, &mut local_substs),
                        )
                    })
                    .collect(),
            };
            nodes.insert(name.clone(), rewritten);
        }
    }

    // Phase 3d: this level's own outputs.
    let mut outputs = IndexMap::new();
    for (name, binding) in &wf.outputs {
        outputs.insert(
            name.clone(),
            rewrite_at_level(binding, wf, &children, &mut local_substs),
        );
    }

    Flat {
        inputs,
        nodes,
        outputs,
        boundaries,
    }
}

/// Rewrite a binding authored at `wf`'s own level (one of `wf`'s own
/// node's bindings, or one of `wf`'s own outputs): the only thing that
/// changes here is a `Step`/`Keyed` reference onto one of `wf`'s own
/// `uses:` steps, which resolves through that child's own declared
/// output. Everything else -- a reference to one of `wf`'s own tool
/// nodes, a workflow input, an item, a literal -- is `wf`'s own, and
/// stays exactly as authored.
fn rewrite_at_level(
    binding: &Binding,
    wf: &Workflow,
    children: &IndexMap<NodeName, Flat>,
    local_substs: &mut IndexMap<NodeName, IndexMap<InputName, Binding>>,
) -> Binding {
    match binding {
        Binding::Step { node, port } => {
            if wf.uses.contains_key(node) {
                ensure_local_subst(node, wf, children, local_substs);
                let subst = &local_substs[node];
                let child_flat = &children[node];
                // `${{ steps.<uses step>.<output> }}` parses with the
                // same grammar as `${{ steps.<node>.<port> }}`, so the
                // reference's own binding carries a `PortName` either
                // way; here it actually names one of the child's
                // declared *outputs* (an `OutputName`), which shares
                // `PortName`'s grammar exactly (`tool.rs`'s
                // `PORT_NAME_PATTERN` and `workflow.rs`'s
                // `SEGMENT_NAME_PATTERN` are the same pattern), so this
                // reparse never fails.
                let output_name = OutputName::parse(port.as_str()).unwrap_or_else(|err| {
                    unreachable!("a PortName is always a valid OutputName too: {err}")
                });
                let inner = child_flat.outputs.get(&output_name).unwrap_or_else(|| {
                    unimplemented!(
                        "milestone 2b task L2: unknown uses output (CheckError::UnknownUsesOutput); \
                         step `{node}`, output `{port}`"
                    )
                });
                embed(inner, node, subst)
            } else {
                binding.clone()
            }
        }
        Binding::Keyed { node, .. } => {
            if wf.uses.contains_key(node) {
                unimplemented!(
                    "milestone 2b task L2: a keyed reference onto a uses: step \
                     (CheckError::KeyedOnUses); step `{node}`"
                );
            }
            binding.clone()
        }
        Binding::List(items) => Binding::List(
            items
                .iter()
                .map(|item| rewrite_at_level(item, wf, children, local_substs))
                .collect(),
        ),
        Binding::Input(_) | Binding::Item | Binding::Literal(_) => binding.clone(),
    }
}

/// Fill in `local_substs[step]` (memoized: a no-op once present), by
/// rewriting `step`'s own `with:` bindings at `wf`'s own level, and
/// recording a fresh fixed-input reference for every one of the child's
/// authored inputs that `with:` left unbound but that carries a default.
/// An authored input with neither is left out of the map entirely: the
/// module docs' "Scope" list covers what happens if anything inside the
/// child still references it (`CheckError::UnboundUsesInput`, task L2).
fn ensure_local_subst(
    step: &NodeName,
    wf: &Workflow,
    children: &IndexMap<NodeName, Flat>,
    local_substs: &mut IndexMap<NodeName, IndexMap<InputName, Binding>>,
) {
    if local_substs.contains_key(step) {
        return;
    }
    let uses = wf
        .uses
        .get(step)
        .unwrap_or_else(|| unreachable!("only ever called with one of `wf.uses`'s own keys"));
    let child_flat = &children[step];

    let mut subst = IndexMap::new();
    for (input, spec) in &child_flat.inputs {
        if spec.fixed_by.is_some() {
            // Fixed at a deeper level already; never part of this child's
            // own `with:` surface (decision (d6)).
            continue;
        }
        if let Some(binding) = uses.with.get(input) {
            // `rewrite_at_level` may itself recurse into
            // `ensure_local_subst` for a *different* sibling step (one
            // this `with:` binding references), never this one -- `step`
            // is not yet a key of `local_substs` at this point (the
            // `contains_key` guard above returned early otherwise), so
            // there is no cycle through this call; a genuine cycle
            // between two steps' own outputs is `CheckError::UsesOutputCycle`
            // (task L2), with no guard against it here.
            let rewritten = rewrite_at_level(binding, wf, children, local_substs);
            subst.insert(input.clone(), rewritten);
        } else if spec.default.is_some() {
            subst.insert(input.clone(), Binding::Input(prefixed_input(step, input)));
        }
        // else: required, unbound, undefaulted -- `embed` panics with
        // `unimplemented!` if anything inside the child still references
        // it (`CheckError::UnboundUsesInput`, task L2).
    }
    local_substs.insert(step.clone(), subst);
}

/// Rewrite a binding found *inside* a child already being embedded under
/// `step`: one of its own nodes' bindings, one of its own outputs, or one
/// of its own boundaries' bindings. Every reference in here is in the
/// child's own flat namespace (already fully resolved by that child's own
/// [`flatten`] call -- no further `uses:`-step indirection remains inside
/// it), so the only two things that change are a node/input reference,
/// which gets `step`'s own prefix, and an authored input reference, which
/// `subst` (that step's own [`ensure_local_subst`] result) replaces with
/// the parent's own binding for it.
fn embed(binding: &Binding, step: &NodeName, subst: &IndexMap<InputName, Binding>) -> Binding {
    match binding {
        Binding::Input(name) => {
            if let Some(replacement) = subst.get(name) {
                replacement.clone()
            } else if name.as_str().contains('/') {
                // Already fixed at a deeper level; bubble it up under
                // this step's own prefix too (decision (d3), applied to
                // input paths the same way it applies to node paths).
                Binding::Input(prefixed_input(step, name))
            } else {
                unimplemented!(
                    "milestone 2b task L2: an unbound required child input reached the \
                     linker (CheckError::UnboundUsesInput); step `{step}`, input `{name}`"
                )
            }
        }
        Binding::Step { node, port } => Binding::Step {
            node: prefixed_node(step, node),
            port: port.clone(),
        },
        Binding::Keyed { node, key, port } => Binding::Keyed {
            node: prefixed_node(step, node),
            key: key.clone(),
            port: port.clone(),
        },
        Binding::Item => Binding::Item,
        Binding::Literal(value) => Binding::Literal(value.clone()),
        Binding::List(items) => {
            Binding::List(items.iter().map(|item| embed(item, step, subst)).collect())
        }
    }
}

/// `<step>/<name>`, as a [`NodeName`] -- always valid, because `step` is
/// an authored name (one segment, no `/`; the DSL refuses one) and `name`
/// is already a valid [`NodeName`] of either shape.
fn prefixed_node(step: &NodeName, name: &NodeName) -> NodeName {
    NodeName::parse(&format!("{step}/{name}")).unwrap_or_else(|err| {
        unreachable!("a NodeName prefixed with a valid step name is always valid: {err}")
    })
}

/// `<step>/<name>`, as an [`InputName`] -- see [`prefixed_node`].
fn prefixed_input(step: &NodeName, name: &InputName) -> InputName {
    InputName::parse(&format!("{step}/{name}")).unwrap_or_else(|err| {
        unreachable!("an InputName prefixed with a valid step name is always valid: {err}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::catalog::Catalog;
    use crate::check::{self, Checked};
    use crate::class::Class;
    use crate::describe::{self, PartialInputs, RawInput};
    use crate::plan;
    use crate::tool::{
        Ensured, Inputs, Observation, Outputs, PortName, PortSpec, Tool, ToolError, ToolName,
        ToolSpec,
    };
    use crate::value::{PortType, TypeName, TypeRef, Value};
    use crate::workflow::Uses;
    use willikins_types::{DomainType, SinkToken};

    fn wf_name(name: &str) -> WorkflowName {
        WorkflowName::parse(name).unwrap()
    }

    fn node(name: &str) -> NodeName {
        NodeName::parse(name).unwrap()
    }

    fn input(name: &str) -> InputName {
        InputName::parse(name).unwrap()
    }

    fn output(name: &str) -> OutputName {
        OutputName::parse(name).unwrap()
    }

    fn port(name: &str) -> PortName {
        PortName::parse(name).unwrap()
    }

    fn tool(name: &str) -> ToolName {
        ToolName::parse(name).unwrap()
    }

    fn ty(name: &str) -> TypeRef {
        TypeRef::scalar(TypeName::parse(name).unwrap())
    }

    fn list_ty(name: &str) -> TypeRef {
        TypeRef::list_of(TypeName::parse(name).unwrap())
    }

    /// `text.join`'s test double: its spec is fixed at construction, and
    /// `read` always predicts nothing -- no test here inspects a `combo`
    /// node's actual output *value*, only that it resolved at all.
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

    /// `text.identity`'s test double: `read` predicts its own `value`
    /// output as whatever `value` input it was bound to -- an actual
    /// echo, not an empty prediction, so a `for_each` node built on this
    /// tool (`cfg`) produces *known* per-instance values and its
    /// aggregate is a real `Known::List`, not
    /// [`crate::value::ValueState::Unknown`] (verify item 3's first
    /// half needs a real list to assert its length against).
    struct EchoTool {
        spec: ToolSpec,
    }

    impl Tool for EchoTool {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
            let mut predicted = Outputs::new();
            if let Some(value) = inputs.get(&port("value")) {
                predicted.insert(port("value"), value.clone());
            }
            Ok(Observation::Absent { predicted })
        }

        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            Ok(Ensured {
                outputs: Outputs::new(),
                changed: true,
            })
        }
    }

    /// `text.identity`: one required `Text` input port `value`, one
    /// `Text` output port `value` ([`EchoTool`]). `text.join`: one
    /// required `list<Text>` input port `items`, one `Text` output port
    /// `joined` ([`DummyTool`]). Both pure and reversible -- this test
    /// suite only exercises `check`/`describe`/`plan`, never `apply`, so
    /// what `ensure` would actually do never matters.
    fn test_catalog() -> Catalog {
        let mut catalog = Catalog::new(willikins_types::registry());

        let mut identity_in = IndexMap::new();
        identity_in.insert(
            port("value"),
            PortSpec {
                ty: PortType::Exact(ty("Text")),
                required: true,
                derived_only: false,
            },
        );
        let mut identity_out = IndexMap::new();
        identity_out.insert(port("value"), ty("Text"));
        catalog
            .insert(Arc::new(EchoTool {
                spec: ToolSpec {
                    name: tool("text.identity"),
                    description: "Test double: echoes its `value` port.".to_string(),
                    inputs: identity_in,
                    outputs: identity_out,
                    key: vec![],
                    class: Class::Reversible,
                    pure: true,
                },
            }))
            .unwrap();

        let mut join_in = IndexMap::new();
        join_in.insert(
            port("items"),
            PortSpec {
                ty: PortType::Exact(list_ty("Text")),
                required: true,
                derived_only: false,
            },
        );
        let mut join_out = IndexMap::new();
        join_out.insert(port("joined"), ty("Text"));
        catalog
            .insert(Arc::new(DummyTool {
                spec: ToolSpec {
                    name: tool("text.join"),
                    description: "Test double: joins its `items` port.".to_string(),
                    inputs: join_in,
                    outputs: join_out,
                    key: vec![],
                    class: Class::Reversible,
                    pure: true,
                },
            }))
            .unwrap();

        catalog
    }

    /// `leaf`: `name` (required `Text`), `tags` (`list<Text>`, default
    /// `["a", "b"]`). Node `greet` echoes `name`; node `cfg` is a
    /// `for_each` over `tags` that echoes each item. Outputs: `greeting`
    /// (`greet`'s output), `cfg_by_key` (a `Keyed` reference into `cfg`,
    /// key `"a"`), `cfg_all` (a plain `Step` onto the `for_each` node
    /// `cfg`, aggregated -- verify item 3's first half).
    fn leaf() -> Workflow {
        Workflow::new(wf_name("leaf"))
            .input(input("name"), InputSpec::new(ty("Text")))
            .input(
                input("tags"),
                InputSpec::new(list_ty("Text"))
                    .with_default(Value::parse_list(&list_ty("Text"), &["a", "b"]).unwrap()),
            )
            .node(
                node("greet"),
                Node::new(tool("text.identity")).port(port("value"), Binding::Input(input("name"))),
            )
            .node(
                node("cfg"),
                Node::new(tool("text.identity"))
                    .for_each(Binding::Input(input("tags")))
                    .port(port("value"), Binding::Item),
            )
            .output(
                output("greeting"),
                Binding::Step {
                    node: node("greet"),
                    port: port("value"),
                },
            )
            .output(
                output("cfg_by_key"),
                Binding::Keyed {
                    node: node("cfg"),
                    key: "a".to_string(),
                    port: port("value"),
                },
            )
            .output(
                output("cfg_all"),
                Binding::Step {
                    node: node("cfg"),
                    port: port("value"),
                },
            )
    }

    /// `middle`: `who` (required `Text`). `uses: leaf` at position 0,
    /// binding `name` to `who` and leaving `tags` unbound (so it becomes
    /// the fixed input `leafstep/tags`). Its own node `combo` (`text.join`)
    /// joins `[steps.leafstep.greeting, "extra"]`. Outputs re-export
    /// `leafstep`'s `greeting` and `cfg_by_key`, plus `combo`'s own
    /// `joined`.
    fn middle() -> Workflow {
        let mut leafstep_with = IndexMap::new();
        leafstep_with.insert(input("name"), Binding::Input(input("who")));

        Workflow::new(wf_name("middle"))
            .input(input("who"), InputSpec::new(ty("Text")))
            .uses(
                node("leafstep"),
                Uses {
                    workflow: wf_name("leaf"),
                    with: leafstep_with,
                    position: 0,
                },
            )
            .node(
                node("combo"),
                Node::new(tool("text.join")).port(
                    port("items"),
                    Binding::List(vec![
                        Binding::Step {
                            node: node("leafstep"),
                            port: port("greeting"),
                        },
                        Binding::Literal("extra".to_string()),
                    ]),
                ),
            )
            .output(
                output("echoed_greeting"),
                Binding::Step {
                    node: node("leafstep"),
                    port: port("greeting"),
                },
            )
            .output(
                output("leaf_cfg_key"),
                Binding::Step {
                    node: node("leafstep"),
                    port: port("cfg_by_key"),
                },
            )
            .output(
                output("combo_result"),
                Binding::Step {
                    node: node("combo"),
                    port: port("joined"),
                },
            )
    }

    /// `root`: `name` (required `Text`). Its own node `footer` echoes
    /// `name`. `uses: middle` at position 0, binding `who` to the literal
    /// `"Ada"` (so `footer` ends up at the *end* of the flat node order,
    /// proving the position walk inserts a used document's nodes at its
    /// own `uses:` step's slot rather than always first or always last).
    /// Outputs re-export `midstep`'s `echoed_greeting` (a leaf output,
    /// re-exported through middle -- acceptance 3's required case),
    /// `leaf_cfg_key`, and `combo_result`, plus `footer`'s own output.
    fn root() -> Workflow {
        Workflow::new(wf_name("root"))
            .input(input("name"), InputSpec::new(ty("Text")))
            .node(
                node("footer"),
                Node::new(tool("text.identity")).port(port("value"), Binding::Input(input("name"))),
            )
            .uses(
                node("midstep"),
                Uses {
                    workflow: wf_name("middle"),
                    with: {
                        let mut with = IndexMap::new();
                        with.insert(input("who"), Binding::Literal("Ada".to_string()));
                        with
                    },
                    position: 0,
                },
            )
            .output(
                output("final_greeting"),
                Binding::Step {
                    node: node("midstep"),
                    port: port("echoed_greeting"),
                },
            )
            .output(
                output("final_key"),
                Binding::Step {
                    node: node("midstep"),
                    port: port("leaf_cfg_key"),
                },
            )
            .output(
                output("final_combo"),
                Binding::Step {
                    node: node("midstep"),
                    port: port("combo_result"),
                },
            )
            .output(
                output("footer_value"),
                Binding::Step {
                    node: node("footer"),
                    port: port("value"),
                },
            )
    }

    /// An in-memory resolver over `leaf`/`middle`, over a well-formed
    /// `root` that never asks for anything else -- this task's happy
    /// path, with no refusal or call-counting behaviour (task L2's
    /// scope; see the module docs).
    fn resolver() -> impl FnMut(&WorkflowName) -> Result<Workflow, ResolveFailure> {
        move |name: &WorkflowName| match name.as_str() {
            "leaf" => Ok(leaf()),
            "middle" => Ok(middle()),
            other => panic!("resolver asked for an unexpected workflow: {other}"),
        }
    }

    fn link_root() -> Linked {
        link(&root(), &mut resolver()).unwrap()
    }

    /// Acceptance 3: a two-level composite (root -> middle -> leaf) links
    /// to the hand-written flat workflow below -- covering prefixed nodes
    /// in position order, every substituted binding form (`Input`,
    /// `Step`, `Keyed`, `Literal`, `List`), one fixed input with
    /// `fixed_by`, and a root output re-exporting a leaf output through
    /// middle -- and `used` is `[middle, leaf]`.
    #[test]
    #[allow(clippy::too_many_lines)] // one scenario, exercised end to end; see `check`'s own precedent
    fn two_level_composite_links_to_the_hand_written_flat_workflow() {
        let linked = link_root();

        assert_eq!(
            linked.used,
            vec![wf_name("middle"), wf_name("leaf")],
            "each used document is listed once, in first-resolution order"
        );

        // Node order: the position walk inserts `midstep`'s children
        // (position 0) before `root`'s own `footer` (the only slot left,
        // position 1) -- not in `root.nodes`' own declaration order,
        // which lists `footer` first.
        assert_eq!(
            linked
                .workflow
                .nodes
                .keys()
                .map(NodeName::as_str)
                .collect::<Vec<_>>(),
            vec![
                "midstep/leafstep/greet",
                "midstep/leafstep/cfg",
                "midstep/combo",
                "footer",
            ],
            "a uses: step's children are inserted at its own position, not appended"
        );

        // Input substitution, two levels deep:
        // `greet.value = Input(name)` in `leaf`, substituted to
        // `Input(who)` when `middle` embeds `leaf` (`with: {name: who}`),
        // then substituted again to `Literal("Ada")` when `root` embeds
        // `middle` (`with: {who: "Ada"}`).
        let greet = &linked.workflow.nodes[&node("midstep/leafstep/greet")];
        assert_eq!(
            greet.with.get(&port("value")),
            Some(&Binding::Literal("Ada".to_string())),
            "a chain of Input substitutions resolves to the literal bound at the top"
        );

        // The fixed input: `leaf`'s `tags` is never bound by `middle`'s
        // `leafstep.with`, so it becomes `leafstep/tags`, fixed by
        // `leafstep`, inside `middle`'s own flattening; `root` then
        // bubbles that up to `midstep/leafstep/tags`, fixed by
        // `midstep/leafstep`.
        let fixed = linked
            .workflow
            .inputs
            .get(&input("midstep/leafstep/tags"))
            .expect("the bubbled-up fixed input must be present");
        assert_eq!(
            fixed.fixed_by,
            Some(node("midstep/leafstep")),
            "fixed_by names the full path to the step that fixed it"
        );
        assert_eq!(fixed.ty, list_ty("Text"));
        let expected_default = Value::parse_list(&list_ty("Text"), &["a", "b"]).unwrap();
        assert_eq!(
            fixed
                .default
                .as_ref()
                .map(|value| serde_json::to_value(value).unwrap()),
            Some(serde_json::to_value(&expected_default).unwrap()),
            "the fixed input keeps the used document's own default"
        );
        assert!(
            !linked.workflow.inputs.contains_key(&input("who")),
            "an input bound via `with:` is consumed, never exposed"
        );
        assert_eq!(
            linked.workflow.inputs.keys().collect::<Vec<_>>(),
            vec![&input("name"), &input("midstep/leafstep/tags")],
            "root's own authored input, then the one fixed input"
        );

        // `for_each` substitution: the `cfg` node's `for_each` source,
        // `Input(tags)` in `leaf`, ends up pointing at the same fixed
        // input the output above does.
        let cfg = &linked.workflow.nodes[&node("midstep/leafstep/cfg")];
        assert_eq!(
            cfg.for_each,
            Some(Binding::Input(input("midstep/leafstep/tags")))
        );

        // `List` substitution: `combo`'s `items` port holds a `Step` onto
        // `leafstep` (prefixed to the flat node name) and an unchanged
        // `Literal`.
        let combo = &linked.workflow.nodes[&node("midstep/combo")];
        assert_eq!(
            combo.with.get(&port("items")),
            Some(&Binding::List(vec![
                Binding::Step {
                    node: node("midstep/leafstep/greet"),
                    port: port("value"),
                },
                Binding::Literal("extra".to_string()),
            ]))
        );

        // Output aliasing: a root output re-exporting a leaf output
        // *through* middle (the required acceptance-3 case), and a
        // `Keyed` form surviving the same two-level embedding.
        assert_eq!(
            linked.workflow.outputs.get(&output("final_greeting")),
            Some(&Binding::Step {
                node: node("midstep/leafstep/greet"),
                port: port("value"),
            })
        );
        assert_eq!(
            linked.workflow.outputs.get(&output("final_key")),
            Some(&Binding::Keyed {
                node: node("midstep/leafstep/cfg"),
                key: "a".to_string(),
                port: port("value"),
            })
        );
        assert_eq!(
            linked.workflow.outputs.get(&output("final_combo")),
            Some(&Binding::Step {
                node: node("midstep/combo"),
                port: port("joined"),
            })
        );
        assert_eq!(
            linked.workflow.outputs.get(&output("footer_value")),
            Some(&Binding::Step {
                node: node("footer"),
                port: port("value"),
            }),
            "root's own, unprefixed node reference is untouched"
        );

        // Boundaries: root's own direct boundary on `who`, plus `middle`'s
        // two bubbled boundaries on `leaf`'s `name` and `tags`, each
        // re-prefixed and with its binding embedded.
        assert_eq!(linked.workflow.boundaries.len(), 3);
        let who_boundary = &linked.workflow.boundaries[0];
        assert_eq!(who_boundary.uses, node("midstep"));
        assert_eq!(who_boundary.workflow, wf_name("middle"));
        assert_eq!(who_boundary.input, input("who"));
        assert_eq!(
            who_boundary.binding,
            Some(Binding::Literal("Ada".to_string()))
        );

        let name_boundary = &linked.workflow.boundaries[1];
        assert_eq!(name_boundary.uses, node("midstep/leafstep"));
        assert_eq!(name_boundary.workflow, wf_name("leaf"));
        assert_eq!(name_boundary.input, input("name"));
        assert_eq!(
            name_boundary.binding,
            Some(Binding::Literal("Ada".to_string())),
            "bubbled boundary bindings are embedded too, not left in middle's own terms"
        );

        let tags_boundary = &linked.workflow.boundaries[2];
        assert_eq!(tags_boundary.uses, node("midstep/leafstep"));
        assert_eq!(tags_boundary.workflow, wf_name("leaf"));
        assert_eq!(tags_boundary.input, input("tags"));
        assert_eq!(
            tags_boundary.binding, None,
            "an unbound (fixed) input's boundary binding is None"
        );
    }

    /// Acceptance 3's second claim: the flat graph's `check` gives the
    /// same `order` a hand-written flat document would -- in particular,
    /// `midstep/combo` (which reads `midstep/leafstep/greet`'s output)
    /// comes after it, even though the position walk placed `footer`
    /// after both. Verify item 5: ties are broken by declaration index,
    /// so a composite written in dependency order keeps that order once
    /// linked.
    #[test]
    fn the_linked_graphs_check_order_matches_its_own_node_declaration_order() {
        let linked = link_root();
        let catalog = test_catalog();
        let checked = check::check(&linked.workflow, &catalog).unwrap();
        assert_eq!(
            checked.order,
            linked.workflow.nodes.keys().cloned().collect::<Vec<_>>(),
        );
    }

    /// Verify item 3: a `Binding::Step` onto a `for_each` node is
    /// accepted as a workflow output and resolves to the aggregated list
    /// (here, `cfg`'s `for_each` over `tags`, re-exported all the way up to
    /// a hypothetical root output would carry every instance's `value`);
    /// and a list-typed `Binding::Input` output resolves its default at
    /// plan time -- the fixed input `midstep/leafstep/tags` is exactly
    /// such an output's binding one level removed (`cfg`'s `for_each`
    /// source), so this exercises both halves on the one linked graph.
    #[test]
    fn describe_and_plan_resolve_the_fixed_list_input_and_its_for_each_aggregation() {
        let linked = link_root();
        let catalog = test_catalog();
        let checked: Checked = check::check(&linked.workflow, &catalog).unwrap();

        let mut partial = PartialInputs::new();
        partial.insert(input("name"), RawInput::Scalar("Root".to_string()));
        let description = describe::describe(&checked, &partial);
        assert!(
            description.missing.is_empty(),
            "the fixed input's default covers it; only `name` needed a caller value: {:?}",
            description.missing
        );
        let expected_tags = Value::parse_list(&list_ty("Text"), &["a", "b"]).unwrap();
        assert_eq!(
            description
                .resolved
                .get(&input("midstep/leafstep/tags"))
                .map(|value| serde_json::to_value(value).unwrap()),
            Some(serde_json::to_value(&expected_tags).unwrap()),
            "describe resolves a fixed input's default the same as any other"
        );

        let result = plan::plan(&checked, &description.resolved, &catalog).unwrap();

        // `cfg`'s `for_each` source is the very same fixed input; its
        // aggregated output is not a declared workflow output on this
        // fixture, but the plan still ran every instance.
        let cfg_instances: Vec<_> = result
            .nodes
            .iter()
            .filter(|n| n.name == node("midstep/leafstep/cfg"))
            .collect();
        assert_eq!(
            cfg_instances.len(),
            2,
            "the for_each node ran once per item of the fixed list input's default"
        );

        // Every declared output resolved (no `PlanError`), including the
        // ones that alias straight through to a node two levels down.
        for name in ["final_greeting", "final_key", "final_combo", "footer_value"] {
            assert!(
                result.outputs.contains_key(&output(name)),
                "output `{name}` should have resolved"
            );
        }
    }

    /// Verify item 3's first half, isolated: a `Binding::Step` onto a
    /// `for_each` node (`cfg`, over the default `tags` list) is accepted
    /// as a workflow output (`cfg_all`) and resolves to the aggregated
    /// list, one value per item -- on `leaf` alone, with no linking
    /// needed, since this is existing `check`/`plan` behaviour the
    /// linker's fixed-input pass-through (decision (d6)) relies on.
    #[test]
    fn a_step_onto_a_for_each_node_is_a_valid_output_and_aggregates() {
        let catalog = test_catalog();
        let checked = check::check(&leaf(), &catalog).unwrap();

        let mut partial = PartialInputs::new();
        partial.insert(input("name"), RawInput::Scalar("Ada".to_string()));
        let description = describe::describe(&checked, &partial);
        assert!(description.missing.is_empty(), "{:?}", description.missing);

        let result = plan::plan(&checked, &description.resolved, &catalog).unwrap();
        let aggregated = result
            .outputs
            .get(&output("cfg_all"))
            .expect("cfg_all should have resolved");
        assert_eq!(
            aggregated.as_list().map(<[_]>::len),
            Some(2),
            "the aggregate holds one value per for_each item"
        );
    }

    /// `rewrite_at_level`'s own no-op case: a reference to a `uses:` step
    /// from a sibling that was never asked to touch it is untouched too
    /// -- `footer_value`'s `Step { node: footer, .. }` is never rewritten
    /// because `footer` is not in `root.uses`.
    #[test]
    fn a_reference_to_an_ordinary_tool_node_is_never_treated_as_a_uses_step() {
        let linked = link_root();
        assert_eq!(
            linked.workflow.outputs[&output("footer_value")],
            Binding::Step {
                node: node("footer"),
                port: port("value"),
            }
        );
    }
}
