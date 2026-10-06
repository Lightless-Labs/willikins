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
//! # Scope (task L1: "the linker, structure"; task L2: "the linker,
//! refusals")
//!
//! L1 implemented the *structure* of linking: flattening, substitution,
//! fixed inputs, and output aliasing, each exercised only along a happy
//! path. L2 adds the linker's own refusals, in two commits:
//!
//! - **Commit 1 (cycle, depth, and size bounds; decision (d9)).**
//!   [`flatten`] now walks with a `stack` of workflow names (the
//!   depth-first path from the root to the document currently being
//!   entered), a `depth` counter (root is depth 0), and a shared
//!   `node_count` (every linked tool node, counted once per occurrence —
//!   a diamond's second occurrence counts again). A `uses:` step whose
//!   target is already on `stack` is [`crate::check::CheckError::UsesCycle`];
//!   one found in a document already at [`MAX_USES_DEPTH`] is
//!   [`crate::check::CheckError::UsesTooDeep`]; crossing
//!   [`MAX_LINKED_NODES`] is [`crate::check::CheckError::UsesTooLarge`],
//!   raised the moment a document's own node count tips the running total
//!   over the bound — *before* that document's own `uses:` steps (if any)
//!   are resolved, so an exponential diamond is refused long before it
//!   would be fully materialised (the precedent is the YAML
//!   alias-amplification refusal,
//!   `docs/research/2026-09-12-e2e-adversarial-pass-2.md`). Each of these
//!   three is a first-error-wins short circuit: [`flatten`] returns as
//!   soon as it finds one, with no further sibling `uses:` step resolved
//!   and no accumulation of several errors at once (unlike
//!   [`crate::check::check`], which deliberately walks every node).
//! - **Commit 2 (the boundary and alias refusals, and
//!   `PathInAuthoredName`; decisions (d4), (d7)).** Every remaining site
//!   the commit-1 scope list above named now returns a real
//!   [`crate::check::CheckError`] instead of panicking:
//!   - [`ResolveFailure`] from `resolve` becomes
//!     [`crate::check::CheckError::UnknownWorkflow`] (`NotFound` /
//!     `Refused`) or [`crate::check::CheckError::UsedDocument`]
//!     (`Document`).
//!   - A `uses:` step's `with:` naming an input the used document never
//!     declared is [`crate::check::CheckError::UnknownUsesInput`],
//!     checked eagerly in [`ensure_local_subst`] (not lazily, unlike the
//!     rest of this list).
//!   - A used document's required input left both unbound and
//!     undefaulted is [`crate::check::CheckError::UnboundUsesInput`],
//!     also checked eagerly in [`ensure_local_subst`] for every declared
//!     input, whether or not the child ever actually references it.
//!   - `${{ item }}` bound to a `uses:` step's input, bare or inside a
//!     list element, is [`crate::check::CheckError::ItemInUses`].
//!   - A reference to a used document's output it never declared is
//!     [`crate::check::CheckError::UnknownUsesOutput`].
//!   - A `Keyed` reference onto a `uses:` step is
//!     [`crate::check::CheckError::KeyedOnUses`].
//!   - An alias cycle between two `uses:` steps' outputs is
//!     [`crate::check::CheckError::UsesOutputCycle`]: [`rewrite_at_level`]
//!     and [`ensure_local_subst`] thread a `visiting` stack (reset per
//!     [`flatten`] call, one entry per `uses:` step currently being
//!     resolved at *this* level), and a `Step`/`Keyed` reference onto a
//!     step already on it is the loop closing, refused before the
//!     recursion that would otherwise overflow the stack.
//!   - A `/` in any node, input, or `uses:`-step name a workflow handed
//!     to the linker authored itself (root or a resolved child) is
//!     [`crate::check::CheckError::PathInAuthoredName`], checked at the
//!     top of every [`flatten`] call.
//!
//!   `UnknownUsesOutput` and `KeyedOnUses` need to know *where* the bad
//!   reference was found, so [`rewrite_at_level`] takes a [`Site`]: a
//!   tool node's own port or `for_each` binding, a workflow output, or,
//!   when the reference sits inside another `uses:` step's own `with:`,
//!   [`Site::Port`] naming that `uses:` step as `node` and the child
//!   input (reparsed as a [`PortName`], which shares [`InputName`]'s
//!   grammar) as `port` — the grammars coincide, so no new `Site`
//!   variant is needed for it. A `Binding::List` element reuses its
//!   enclosing binding's own site rather than a per-index one: precise
//!   enough to name the right node and port, at the cost of not pointing
//!   at which element.
//!
//!   Every refusal is first-error-wins, exactly like commit 1's: the
//!   first one found anywhere in the walk aborts the whole [`link`] call
//!   immediately (via `?`), with no further sibling resolved or rewritten.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::check::CheckError;
use crate::site::Site;
use crate::tool::PortName;
use crate::workflow::{Binding, InputName, InputSpec, Node, NodeName, OutputName, Workflow};
use willikins_types::WorkflowName;

/// The greatest nesting depth [`link`] will resolve: the root document is
/// depth 0, and a `uses:` step found in a document already at this depth
/// is refused with [`CheckError::UsesTooDeep`] rather than resolved — see
/// the module docs' "Scope" section and milestone 2b decision (d9).
pub const MAX_USES_DEPTH: usize = 8;

/// The greatest number of tool nodes [`link`] will produce, counted
/// before any `for_each` expansion. Crossing it is
/// [`CheckError::UsesTooLarge`] — see the module docs' "Scope" section and
/// milestone 2b decision (d9).
pub const MAX_LINKED_NODES: usize = 2048;

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
/// A cycle, an excessive nesting depth, or an excessive linked node count
/// (task L2 commit 1; see the module docs' "Scope" section) is returned as
/// a single-element `Vec`, with no further `uses:` step resolved once
/// found. Every other refusal listed in the module docs' "Scope" section
/// (task L2 commit 2) is not implemented yet and panics instead.
#[allow(clippy::missing_panics_doc)] // every panic site is the module docs' "Scope" list, by design
pub fn link(
    root: &Workflow,
    resolve: &mut dyn FnMut(&WorkflowName) -> Result<Workflow, ResolveFailure>,
) -> Result<Linked, Vec<CheckError>> {
    let mut used = Vec::new();
    let mut stack = vec![root.name.clone()];
    let mut node_count = 0usize;
    let flat = flatten(root, resolve, &mut used, &mut stack, 0, &mut node_count)?;

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
    stack: &mut Vec<WorkflowName>,
    depth: usize,
    node_count: &mut usize,
) -> Result<Flat, Vec<CheckError>> {
    // Decision (d3): a `/` in any of `wf`'s own authored names protects a
    // hand-built `Workflow` (never checked by the DSL) from colliding
    // with a path the linker itself produces. Checked for root and every
    // resolved child alike, since this is the top of every `flatten`
    // call; see the module docs.
    if let Some(bad) = wf
        .nodes
        .keys()
        .map(NodeName::as_str)
        .chain(wf.uses.keys().map(NodeName::as_str))
        .chain(wf.inputs.keys().map(InputName::as_str))
        .find(|name| name.contains('/'))
    {
        return Err(vec![CheckError::PathInAuthoredName {
            name: bad.to_string(),
        }]);
    }

    // Size bound (decision (d9)), checked pre-order, as `wf` itself is
    // entered -- before any of `wf`'s own `uses:` steps are resolved. A
    // diamond's second occurrence of the same child is a second `flatten`
    // call (no memoization across siblings), so it adds to `node_count`
    // again, which is exactly what "counted before for_each" means: each
    // *occurrence* of a tool node in the linked graph counts once,
    // regardless of how many for_each instances it later expands into.
    // Raising this here, rather than after Phase 1 finishes resolving
    // `wf`'s own children, is what keeps an exponential diamond from
    // being materialised past the bound: the first document whose own
    // node count tips the running total over `MAX_LINKED_NODES` is
    // refused before *its* children are ever resolved.
    *node_count += wf.nodes.len();
    if *node_count > MAX_LINKED_NODES {
        return Err(vec![CheckError::UsesTooLarge { nodes: *node_count }]);
    }

    // Phase 1: resolve and recursively flatten every `uses:` step's own
    // child, independent of this level's own step order -- a sibling's
    // `with:` may reference another sibling's output regardless of which
    // one is declared first (decision (d7)).
    let mut children: IndexMap<NodeName, Flat> = IndexMap::new();
    for (step, uses) in &wf.uses {
        if !used.iter().any(|w| w == &uses.workflow) {
            used.push(uses.workflow.clone());
        }

        // Cycle bound (decision (d9)): a target already on the
        // depth-first stack -- the path of workflow names from the root
        // down to the document we are currently inside -- is a cycle
        // (one entry is a self-use). Checked before depth, and before
        // ever calling `resolve` again for the repeated name.
        if let Some(i) = stack.iter().position(|w| w == &uses.workflow) {
            let mut chain = stack[i..].to_vec();
            chain.push(uses.workflow.clone());
            return Err(vec![CheckError::UsesCycle { chain }]);
        }

        // Depth bound (decision (d9)): a `uses:` step found in a document
        // already at `MAX_USES_DEPTH` would nest one level past it.
        // `chain` is every name from the root to this document
        // (`stack`), which is why it is cloned before the step that
        // would have extended it.
        if depth >= MAX_USES_DEPTH {
            return Err(vec![CheckError::UsesTooDeep {
                chain: stack.clone(),
            }]);
        }

        let child_wf = match resolve(&uses.workflow) {
            Ok(child_wf) => child_wf,
            // `NotFound` and `Refused` deliberately share one `CheckError`
            // variant -- see `ResolveFailure`'s own docs.
            Err(ResolveFailure::NotFound | ResolveFailure::Refused) => {
                return Err(vec![CheckError::UnknownWorkflow {
                    node: step.clone(),
                    workflow: uses.workflow.clone(),
                }]);
            }
            Err(ResolveFailure::Document { message }) => {
                return Err(vec![CheckError::UsedDocument {
                    node: step.clone(),
                    workflow: uses.workflow.clone(),
                    reason: message,
                }]);
            }
        };
        stack.push(uses.workflow.clone());
        let child_flat = flatten(&child_wf, resolve, used, stack, depth + 1, node_count)?;
        stack.pop();
        children.insert(step.clone(), child_flat);
    }

    // Phase 2: every step's own substitution map for its child's authored
    // inputs, resolved lazily (and memoized) so a sibling reference
    // resolves regardless of declaration order. `visiting` is this
    // level's own alias-cycle guard (decision (d7)): one entry per
    // `uses:` step currently being resolved, reset fresh for every
    // `flatten` call -- a cycle at one level never interferes with an
    // unrelated one elsewhere in the tree.
    let mut local_substs: IndexMap<NodeName, IndexMap<InputName, Binding>> = IndexMap::new();
    let mut visiting: Vec<NodeName> = Vec::new();
    for step in wf.uses.keys() {
        ensure_local_subst(step, wf, &children, &mut local_substs, &mut visiting)?;
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
                    // Required, unbound, undefaulted -- `ensure_local_subst`
                    // (Phase 2, which already ran without error) refuses
                    // this eagerly as `CheckError::UnboundUsesInput` for
                    // every declared input, so Phase 3b can never reach
                    // this arm for one.
                    unreachable!(
                        "ensure_local_subst already refused step `{step}`, input `{input}` as \
                         UnboundUsesInput before Phase 3b could be reached"
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
            let for_each = match &node.for_each {
                Some(b) => Some(rewrite_at_level(
                    b,
                    wf,
                    &children,
                    &mut local_substs,
                    &mut visiting,
                    &Site::ForEach { node: name.clone() },
                )?),
                None => None,
            };
            let mut with = IndexMap::new();
            for (port, b) in &node.with {
                let site = Site::Port {
                    node: name.clone(),
                    port: port.clone(),
                };
                with.insert(
                    port.clone(),
                    rewrite_at_level(b, wf, &children, &mut local_substs, &mut visiting, &site)?,
                );
            }
            let rewritten = Node {
                tool: node.tool.clone(),
                for_each,
                with,
            };
            nodes.insert(name.clone(), rewritten);
        }
    }

    // Phase 3d: this level's own outputs.
    let mut outputs = IndexMap::new();
    for (name, binding) in &wf.outputs {
        let site = Site::Output { name: name.clone() };
        outputs.insert(
            name.clone(),
            rewrite_at_level(
                binding,
                wf,
                &children,
                &mut local_substs,
                &mut visiting,
                &site,
            )?,
        );
    }

    Ok(Flat {
        inputs,
        nodes,
        outputs,
        boundaries,
    })
}

/// Rewrite a binding authored at `wf`'s own level (one of `wf`'s own
/// node's bindings, one of `wf`'s own outputs, or -- recursively, from
/// [`ensure_local_subst`] -- one of a `uses:` step's own `with:` values):
/// the only thing that changes here is a `Step`/`Keyed` reference onto
/// one of `wf`'s own `uses:` steps, which resolves through that child's
/// own declared output, or refuses (decision (d7)). Everything else -- a
/// reference to one of `wf`'s own tool nodes, a workflow input, an item,
/// a literal -- is `wf`'s own, and stays exactly as authored.
///
/// `site` is where `binding` itself was found, for
/// [`CheckError::UnknownUsesOutput`] and [`CheckError::KeyedOnUses`]
/// (both need to report where the bad reference *is*, not only what it
/// names); see the module docs. `visiting` is this `flatten` call's own
/// alias-cycle guard, threaded through to [`ensure_local_subst`] and
/// back.
fn rewrite_at_level(
    binding: &Binding,
    wf: &Workflow,
    children: &IndexMap<NodeName, Flat>,
    local_substs: &mut IndexMap<NodeName, IndexMap<InputName, Binding>>,
    visiting: &mut Vec<NodeName>,
    site: &Site,
) -> Result<Binding, Vec<CheckError>> {
    match binding {
        Binding::Step { node, port } => {
            if wf.uses.contains_key(node) {
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
                if visiting.contains(node) {
                    // Decision (d7): an alias cycle made only of
                    // pass-throughs, with no node on it, so the
                    // node-level cycle check (`check::find_cycles`,
                    // which runs on the flat graph) never sees it. This
                    // is the loop closing.
                    return Err(vec![CheckError::UsesOutputCycle {
                        node: node.clone(),
                        output: output_name,
                    }]);
                }
                ensure_local_subst(node, wf, children, local_substs, visiting)?;
                let subst = &local_substs[node];
                let child_flat = &children[node];
                let inner = child_flat.outputs.get(&output_name).ok_or_else(|| {
                    vec![CheckError::UnknownUsesOutput {
                        site: site.clone(),
                        node: node.clone(),
                        output: output_name.clone(),
                    }]
                })?;
                Ok(embed(inner, node, subst))
            } else {
                Ok(binding.clone())
            }
        }
        Binding::Keyed { node, .. } => {
            if wf.uses.contains_key(node) {
                // Decision (d7): there are no instances to key into -- a
                // `uses:` step may never have a `for_each` (decision
                // (d12)).
                return Err(vec![CheckError::KeyedOnUses {
                    site: site.clone(),
                    node: node.clone(),
                }]);
            }
            Ok(binding.clone())
        }
        Binding::List(items) => {
            let rewritten: Result<Vec<Binding>, Vec<CheckError>> = items
                .iter()
                .map(|item| rewrite_at_level(item, wf, children, local_substs, visiting, site))
                .collect();
            Ok(Binding::List(rewritten?))
        }
        Binding::Input(_) | Binding::Item | Binding::Literal(_) => Ok(binding.clone()),
    }
}

/// Whether `binding` is, or (for a list) contains, a bare `${{ item }}`
/// -- decision (d4): refused anywhere inside a `uses:` step's own
/// `with:` value ([`CheckError::ItemInUses`]), since after substitution
/// it would silently rebind to whichever `for_each` node inside the
/// child happens to consume that input.
fn contains_item(binding: &Binding) -> bool {
    match binding {
        Binding::Item => true,
        Binding::List(items) => items.iter().any(contains_item),
        Binding::Input(_) | Binding::Step { .. } | Binding::Keyed { .. } | Binding::Literal(_) => {
            false
        }
    }
}

/// Fill in `local_substs[step]` (memoized: a no-op once present), by
/// rewriting `step`'s own `with:` bindings at `wf`'s own level, and
/// recording a fresh fixed-input reference for every one of the child's
/// authored inputs that `with:` left unbound but that carries a default.
/// Eagerly refuses, before touching any binding: an unknown `with:` key
/// (decision (d4), [`CheckError::UnknownUsesInput`]) and, for every
/// declared input in turn, `${{ item }}` bound to it
/// ([`CheckError::ItemInUses`]) or a required input left both unbound
/// and undefaulted ([`CheckError::UnboundUsesInput`]) -- the latter
/// whether or not anything inside the child actually references it, so
/// [`embed`] never needs to check for it.
fn ensure_local_subst(
    step: &NodeName,
    wf: &Workflow,
    children: &IndexMap<NodeName, Flat>,
    local_substs: &mut IndexMap<NodeName, IndexMap<InputName, Binding>>,
    visiting: &mut Vec<NodeName>,
) -> Result<(), Vec<CheckError>> {
    if local_substs.contains_key(step) {
        return Ok(());
    }
    let uses = wf
        .uses
        .get(step)
        .unwrap_or_else(|| unreachable!("only ever called with one of `wf.uses`'s own keys"));
    let child_flat = &children[step];

    for key in uses.with.keys() {
        if !child_flat.inputs.contains_key(key) {
            return Err(vec![CheckError::UnknownUsesInput {
                node: step.clone(),
                input: key.clone(),
            }]);
        }
    }

    // `visiting` marks `step` as "being resolved" for the rest of this
    // function's body: a `Step`/`Keyed` reference (reached through
    // `rewrite_at_level`, recursively, for a sibling's own `with:`
    // binding) that re-enters `ensure_local_subst` for `step` while it
    // is on this list is decision (d7)'s alias cycle
    // (`CheckError::UsesOutputCycle`), checked in `rewrite_at_level`
    // itself. Left un-popped on every error path below: every one
    // aborts the whole `link` call via `?`, so nothing after an error
    // ever reads `visiting` again (the same convention `flatten`'s own
    // `stack` already uses for the same reason).
    visiting.push(step.clone());

    let mut subst = IndexMap::new();
    for (input, spec) in &child_flat.inputs {
        if spec.fixed_by.is_some() {
            // Fixed at a deeper level already; never part of this child's
            // own `with:` surface (decision (d6)).
            continue;
        }
        if let Some(binding) = uses.with.get(input) {
            if contains_item(binding) {
                return Err(vec![CheckError::ItemInUses {
                    node: step.clone(),
                    input: input.clone(),
                }]);
            }
            // The site of a reference inside this `with:` value: the
            // `uses:` step as `node`, the child input (reparsed as a
            // `PortName`, which shares `InputName`'s grammar) as `port`
            // -- see the module docs' "the grammars coincide" note.
            let site = Site::Port {
                node: step.clone(),
                port: PortName::parse(input.as_str()).unwrap_or_else(|err| {
                    unreachable!("an InputName is always a valid PortName too: {err}")
                }),
            };
            let rewritten = rewrite_at_level(binding, wf, children, local_substs, visiting, &site)?;
            subst.insert(input.clone(), rewritten);
        } else if spec.default.is_some() {
            subst.insert(input.clone(), Binding::Input(prefixed_input(step, input)));
        } else {
            // Required, unbound, undefaulted (decision (d4)).
            return Err(vec![CheckError::UnboundUsesInput {
                node: step.clone(),
                input: input.clone(),
            }]);
        }
    }
    visiting.pop();
    local_substs.insert(step.clone(), subst);
    Ok(())
}

/// Rewrite a binding found *inside* a child already being embedded under
/// `step`: one of its own nodes' bindings, one of its own outputs, or one
/// of its own boundaries' bindings. Every reference in here is in the
/// child's own flat namespace (already fully resolved by that child's own
/// [`flatten`] call -- no further `uses:`-step indirection remains inside
/// it), so the only two things that change are a node/input reference,
/// which gets `step`'s own prefix, and an authored input reference, which
/// `subst` (that step's own [`ensure_local_subst`] result) replaces with
/// the parent's own binding for it. Infallible: by the time anything
/// calls this, [`ensure_local_subst`] has already run for `step` without
/// error, which is what rules out the one case that would otherwise need
/// to fail here (see the `Input` arm below).
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
                // `ensure_local_subst` builds `subst` with one entry for
                // every declared, non-fixed input of the child -- a
                // `with:` binding (rewritten) or a fixed-input reference
                // for a defaulted one left unbound -- and refuses
                // (`CheckError::UnboundUsesInput`) the one case that
                // would otherwise leave a gap: required, unbound, and
                // undefaulted. `embed` is only ever reached for a `step`
                // whose `ensure_local_subst` call already succeeded, so
                // a non-path `Input` missing from `subst` here is a bug
                // in this module, not a document defect.
                unreachable!(
                    "ensure_local_subst guarantees a subst entry for every non-path input of \
                     step `{step}`; `{name}` has none"
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

    // -----------------------------------------------------------------
    // Task L2 commit 1: cycle, depth, and size bounds (decision (d9)).
    // -----------------------------------------------------------------

    /// `loop`: one `uses:` step, `again`, naming itself.
    fn self_using() -> Workflow {
        Workflow::new(wf_name("loop")).uses(
            node("again"),
            Uses {
                workflow: wf_name("loop"),
                with: IndexMap::new(),
                position: 0,
            },
        )
    }

    /// Acceptance 4: a self-use is `CheckError::UsesCycle` with a
    /// two-entry chain (the name, then its repeat) -- and `resolve` is
    /// never called for it, because the cycle is caught against the
    /// stack before any further resolution is attempted.
    #[test]
    fn a_self_use_is_refused_as_a_one_entry_cycle() {
        let err = link(&self_using(), &mut |name| {
            panic!("resolve must not be called for a self-use, got `{name}`")
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UsesCycle {
                chain: vec![wf_name("loop"), wf_name("loop")],
            }]
        );
    }

    /// `a` uses `b`; `b` uses `a`.
    fn cyclic_a() -> Workflow {
        Workflow::new(wf_name("a")).uses(
            node("to_b"),
            Uses {
                workflow: wf_name("b"),
                with: IndexMap::new(),
                position: 0,
            },
        )
    }

    fn cyclic_b() -> Workflow {
        Workflow::new(wf_name("b")).uses(
            node("to_a"),
            Uses {
                workflow: wf_name("a"),
                with: IndexMap::new(),
                position: 0,
            },
        )
    }

    /// Acceptance 4: a two-document cycle is `UsesCycle` with the chain
    /// `[a, b, a]`, and `resolve` is called exactly once (for `b`) --
    /// never again for `a`, since that is the name already on the stack
    /// that completes the cycle.
    #[test]
    fn a_two_document_cycle_is_refused_with_its_chain() {
        let mut calls = 0;
        let err = link(&cyclic_a(), &mut |name| {
            calls += 1;
            match name.as_str() {
                "b" => Ok(cyclic_b()),
                other => panic!("resolver asked for an unexpected workflow: {other}"),
            }
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UsesCycle {
                chain: vec![wf_name("a"), wf_name("b"), wf_name("a")],
            }]
        );
        assert_eq!(
            calls, 1,
            "resolve must not be called again for the name that completes the cycle"
        );
    }

    /// `root` at index 0, `w1` at index 1, ... `w<i>` at index `i`.
    fn chain_name(i: usize) -> String {
        if i == 0 {
            "root".to_string()
        } else {
            format!("w{i}")
        }
    }

    /// A linear chain of `len` workflows, `root -> w1 -> w2 -> ... ->
    /// w<len-1>`: the last one has no `uses:` step when `leaf` is `true`,
    /// or one more (naming a workflow absent from the returned map, which
    /// must therefore never be resolved) when it is `false`. Shared by
    /// the `MAX_USES_DEPTH` boundary test and its positive neighbour.
    fn linear_chain(len: usize, leaf: bool) -> (Workflow, IndexMap<String, Workflow>) {
        let mut docs = IndexMap::new();
        for i in 0..len {
            let is_last = i == len - 1;
            let mut wf = Workflow::new(wf_name(&chain_name(i)));
            if !is_last || !leaf {
                let next = chain_name(i + 1);
                wf = wf.uses(
                    node("next"),
                    Uses {
                        workflow: wf_name(&next),
                        with: IndexMap::new(),
                        position: 0,
                    },
                );
            }
            docs.insert(chain_name(i), wf);
        }
        let root = docs.get(&chain_name(0)).unwrap().clone();
        (root, docs)
    }

    fn resolve_from(
        docs: &IndexMap<String, Workflow>,
    ) -> impl FnMut(&WorkflowName) -> Result<Workflow, ResolveFailure> + '_ {
        move |name: &WorkflowName| {
            docs.get(name.as_str())
                .cloned()
                .ok_or(ResolveFailure::NotFound)
        }
    }

    /// Acceptance 4: a chain nine documents deep (root at depth 0,
    /// through the ninth document at depth 8) whose own `uses:` step
    /// would nest a tenth is refused as `UsesTooDeep`, naming all nine
    /// in the chain, and never resolves the tenth.
    #[test]
    fn a_nine_deep_chain_is_refused_as_uses_too_deep() {
        let (root, docs) = linear_chain(9, false);
        let err = link(&root, &mut resolve_from(&docs)).unwrap_err();
        let expected_chain: Vec<WorkflowName> = (0..9).map(|i| wf_name(&chain_name(i))).collect();
        assert_eq!(
            err,
            vec![CheckError::UsesTooDeep {
                chain: expected_chain,
            }]
        );
    }

    /// The boundary's other side: the same nine documents, but the ninth
    /// (depth 8) has no further `uses:` step -- it links cleanly, which
    /// is what proves the refusal above is about depth 8's own `uses:`
    /// step, not about merely reaching depth 8.
    #[test]
    fn a_chain_with_a_leaf_at_depth_eight_links_cleanly() {
        let (root, docs) = linear_chain(9, true);
        let linked = link(&root, &mut resolve_from(&docs)).unwrap();
        assert_eq!(linked.used.len(), 8, "w1 through w8, each used once");
    }

    /// `leaf`: `count` plain tool nodes, no `uses:` steps.
    fn counting_leaf(count: usize) -> Workflow {
        let mut wf = Workflow::new(wf_name("leaf"));
        for i in 0..count {
            wf = wf.node(node(&format!("n{i}")), Node::new(tool("noop.tool")));
        }
        wf
    }

    /// A workflow with `fanout` `uses:` steps, `u0..u<fanout-1>`, each
    /// naming `child`.
    fn fan(name: &str, child: &str, fanout: usize) -> Workflow {
        let mut wf = Workflow::new(wf_name(name));
        for i in 0..fanout {
            wf = wf.uses(
                node(&format!("u{i}")),
                Uses {
                    workflow: wf_name(child),
                    with: IndexMap::new(),
                    position: i,
                },
            );
        }
        wf
    }

    /// Acceptance 4: a diamond whose full expansion would be `1 * 8 * 8 *
    /// 64 = 4096` nodes -- `leaf` (64 nodes), `mid1` (8 `uses: leaf`
    /// steps), `mid2` (8 `uses: mid1` steps), `root` (one `uses: mid2`
    /// step) -- is refused as `UsesTooLarge` the moment the running count
    /// first crosses `MAX_LINKED_NODES`, well before `resolve` has been
    /// called the 73 times (1 for `mid2`, 8 for `mid1`, 64 for `leaf`)
    /// a full expansion would need: the bound is caught mid-expansion,
    /// not after the whole diamond is materialised.
    #[test]
    fn an_exponential_diamond_is_refused_before_full_expansion() {
        let leaf = counting_leaf(64);
        let mid1 = fan("mid1", "leaf", 8);
        let mid2 = fan("mid2", "mid1", 8);
        let root = fan("root", "mid2", 1);

        let mut calls = 0usize;
        let err = link(&root, &mut |name| {
            calls += 1;
            match name.as_str() {
                "mid2" => Ok(mid2.clone()),
                "mid1" => Ok(mid1.clone()),
                "leaf" => Ok(leaf.clone()),
                other => panic!("resolver asked for an unexpected workflow: {other}"),
            }
        })
        .unwrap_err();

        match err.as_slice() {
            [CheckError::UsesTooLarge { nodes }] => {
                assert!(
                    *nodes > MAX_LINKED_NODES,
                    "the reported count must be the one that actually crossed the bound: \
                     got {nodes}"
                );
                assert!(
                    *nodes <= MAX_LINKED_NODES + 64,
                    "the overshoot is bounded by the one document that tipped it over \
                     (leaf, 64 nodes), not by however much more a full expansion would add: \
                     got {nodes}"
                );
            }
            other => panic!("expected exactly one UsesTooLarge, got {other:?}"),
        }
        assert!(
            calls < 73,
            "a full expansion would call resolve 73 times (1 + 8 + 64); the bound must be \
             caught before that: got {calls}"
        );
    }

    // -----------------------------------------------------------------
    // Task L2 commit 2: the boundary and alias refusals, and
    // PathInAuthoredName (decisions (d4), (d7)).
    // -----------------------------------------------------------------

    /// `root`, with one `uses:` step named `step`, naming `target`, bound
    /// to nothing.
    fn uses_one(root_name: &str, step: &str, target: &str) -> Workflow {
        Workflow::new(wf_name(root_name)).uses(
            node(step),
            Uses {
                workflow: wf_name(target),
                with: IndexMap::new(),
                position: 0,
            },
        )
    }

    /// A workflow with one declared input `known` (`Text`), defaulted
    /// when `default` is `true`.
    fn child_with_one_input(name: &str, default: bool) -> Workflow {
        let mut spec = InputSpec::new(ty("Text"));
        if default {
            spec = spec.with_default(Value::parse(&ty("Text"), "x").unwrap());
        }
        Workflow::new(wf_name(name)).input(input("known"), spec)
    }

    /// Acceptance 4: an unresolvable child's `resolve` failure
    /// (`NotFound` or `Refused`) is `CheckError::UnknownWorkflow`.
    #[test]
    fn an_unresolvable_child_is_refused_as_unknown_workflow() {
        let root = uses_one("root", "child", "ghost");
        let err = link(&root, &mut |_| Err(ResolveFailure::NotFound)).unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UnknownWorkflow {
                node: node("child"),
                workflow: wf_name("ghost"),
            }]
        );
    }

    /// Acceptance 4: a child that fails to parse or validate on its own
    /// terms (`ResolveFailure::Document`) is `CheckError::UsedDocument`,
    /// naming the child.
    #[test]
    fn a_child_that_fails_to_parse_is_refused_as_used_document() {
        let root = uses_one("root", "child", "bad");
        let err = link(&root, &mut |_| {
            Err(ResolveFailure::Document {
                message: "boom".to_string(),
            })
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UsedDocument {
                node: node("child"),
                workflow: wf_name("bad"),
                reason: "boom".to_string(),
            }]
        );
    }

    /// Acceptance 4: a `with:` key the used document never declared is
    /// `CheckError::UnknownUsesInput`.
    #[test]
    fn an_unknown_with_key_is_refused_as_unknown_uses_input() {
        let mut with = IndexMap::new();
        with.insert(input("mystery"), Binding::Literal("x".to_string()));
        let root = Workflow::new(wf_name("root")).uses(
            node("child"),
            Uses {
                workflow: wf_name("leafchild"),
                with,
                position: 0,
            },
        );
        let err = link(&root, &mut |name| match name.as_str() {
            "leafchild" => Ok(child_with_one_input("leafchild", true)),
            other => panic!("resolver asked for an unexpected workflow: {other}"),
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UnknownUsesInput {
                node: node("child"),
                input: input("mystery"),
            }]
        );
    }

    /// Acceptance 4: a required child input left both unbound and
    /// undefaulted is `CheckError::UnboundUsesInput`, whether or not
    /// anything inside the child references it (this fixture's child
    /// has no nodes at all).
    #[test]
    fn a_required_unbound_undefaulted_input_is_refused_as_unbound_uses_input() {
        let root = uses_one("root", "child", "needy");
        let err = link(&root, &mut |name| match name.as_str() {
            "needy" => Ok(child_with_one_input("needy", false)),
            other => panic!("resolver asked for an unexpected workflow: {other}"),
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UnboundUsesInput {
                node: node("child"),
                input: input("known"),
            }]
        );
    }

    /// Acceptance 4: `${{ item }}` bound to a `uses:` step's input is
    /// `CheckError::ItemInUses`.
    #[test]
    fn item_bound_to_a_uses_input_is_refused_as_item_in_uses() {
        let mut with = IndexMap::new();
        with.insert(input("known"), Binding::Item);
        let root = Workflow::new(wf_name("root")).uses(
            node("child"),
            Uses {
                workflow: wf_name("leafchild"),
                with,
                position: 0,
            },
        );
        let err = link(&root, &mut |name| match name.as_str() {
            "leafchild" => Ok(child_with_one_input("leafchild", true)),
            other => panic!("resolver asked for an unexpected workflow: {other}"),
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::ItemInUses {
                node: node("child"),
                input: input("known"),
            }]
        );
    }

    /// Acceptance 4: `${{ steps.<uses step>.<output> }}` naming an
    /// output the child never declared is `CheckError::UnknownUsesOutput`,
    /// sited at the reference (here, a root output).
    #[test]
    fn an_unknown_uses_output_reference_is_refused() {
        let root = Workflow::new(wf_name("root"))
            .uses(
                node("child"),
                Uses {
                    workflow: wf_name("leafchild"),
                    with: IndexMap::new(),
                    position: 0,
                },
            )
            .output(
                output("x"),
                Binding::Step {
                    node: node("child"),
                    port: port("missing"),
                },
            );
        let err = link(&root, &mut |name| match name.as_str() {
            "leafchild" => Ok(child_with_one_input("leafchild", true)),
            other => panic!("resolver asked for an unexpected workflow: {other}"),
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UnknownUsesOutput {
                site: Site::Output { name: output("x") },
                node: node("child"),
                output: output("missing"),
            }]
        );
    }

    /// Acceptance 4: a `Keyed` reference onto a `uses:` step is
    /// `CheckError::KeyedOnUses` -- there are no instances to key into.
    #[test]
    fn a_keyed_reference_onto_a_uses_step_is_refused() {
        let root = Workflow::new(wf_name("root"))
            .uses(
                node("child"),
                Uses {
                    workflow: wf_name("leafchild"),
                    with: IndexMap::new(),
                    position: 0,
                },
            )
            .output(
                output("x"),
                Binding::Keyed {
                    node: node("child"),
                    key: "a".to_string(),
                    port: port("out"),
                },
            );
        let err = link(&root, &mut |name| match name.as_str() {
            "leafchild" => Ok(child_with_one_input("leafchild", true)),
            other => panic!("resolver asked for an unexpected workflow: {other}"),
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::KeyedOnUses {
                site: Site::Output { name: output("x") },
                node: node("child"),
            }]
        );
    }

    /// `alias-child`: one input `x`, one output `out` that is a bare
    /// pass-through of it (`${{ inputs.x }}`) -- the unit decision (d7)'s
    /// worked example multiplies into a cycle.
    fn alias_child() -> Workflow {
        Workflow::new(wf_name("alias-child"))
            .input(input("x"), InputSpec::new(ty("Text")))
            .output(output("out"), Binding::Input(input("x")))
    }

    /// Acceptance 4, decision (d7)'s worked example: `a`'s input is bound
    /// to `steps.b.out`, `b`'s input is bound to `steps.a.out` -- a loop
    /// made only of pass-throughs, with no node on it, refused as
    /// `CheckError::UsesOutputCycle` before the recursion that would
    /// otherwise overflow the stack.
    #[test]
    fn an_alias_cycle_between_two_uses_steps_outputs_is_refused() {
        let mut a_with = IndexMap::new();
        a_with.insert(
            input("x"),
            Binding::Step {
                node: node("b"),
                port: port("out"),
            },
        );
        let mut b_with = IndexMap::new();
        b_with.insert(
            input("x"),
            Binding::Step {
                node: node("a"),
                port: port("out"),
            },
        );
        let root = Workflow::new(wf_name("root"))
            .uses(
                node("a"),
                Uses {
                    workflow: wf_name("alias-child"),
                    with: a_with,
                    position: 0,
                },
            )
            .uses(
                node("b"),
                Uses {
                    workflow: wf_name("alias-child"),
                    with: b_with,
                    position: 1,
                },
            );
        let err = link(&root, &mut |name| match name.as_str() {
            "alias-child" => Ok(alias_child()),
            other => panic!("resolver asked for an unexpected workflow: {other}"),
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UsesOutputCycle {
                node: node("a"),
                output: output("out"),
            }]
        );
    }

    /// Acceptance 4: a hand-built `Workflow` (never checked by the DSL)
    /// with a `/` in an authored node name is
    /// `CheckError::PathInAuthoredName` -- the linker's own protection,
    /// since the type itself (`NodeName`) accepts a path.
    #[test]
    fn a_slash_in_an_authored_node_name_is_refused() {
        let root =
            Workflow::new(wf_name("root")).node(node("bad/name"), Node::new(tool("noop.tool")));
        let err = link(&root, &mut |name| {
            panic!("resolve should not be called, got `{name}`")
        })
        .unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::PathInAuthoredName {
                name: "bad/name".to_string(),
            }]
        );
    }

    /// A workflow with one declared input `x` (`Text`, defaulted).
    fn child_with_x_input(name: &str) -> Workflow {
        Workflow::new(wf_name(name)).input(
            input("x"),
            InputSpec::new(ty("Text")).with_default(Value::parse(&ty("Text"), "x").unwrap()),
        )
    }

    /// `root` with two `uses:` steps, `a` and `b`: `a`'s `with:` binds
    /// its own child's declared input `x` to `reference` (a reference
    /// onto `b`) -- the "sits inside another `uses:` step's `with:`"
    /// case the module docs' `Site` note covers, for either
    /// `UnknownUsesOutput` or `KeyedOnUses`.
    fn root_referencing_b_from_as_with(reference: Binding) -> Workflow {
        let mut a_with = IndexMap::new();
        a_with.insert(input("x"), reference);
        Workflow::new(wf_name("root"))
            .uses(
                node("a"),
                Uses {
                    workflow: wf_name("achild"),
                    with: a_with,
                    position: 0,
                },
            )
            .uses(
                node("b"),
                Uses {
                    workflow: wf_name("bchild"),
                    with: IndexMap::new(),
                    position: 1,
                },
            )
    }

    // `Result` is never actually `Err` here -- the signature matches
    // `link`'s own `resolve` parameter type, which this is passed as.
    #[allow(clippy::unnecessary_wraps)]
    fn resolve_a_and_b(name: &WorkflowName) -> Result<Workflow, ResolveFailure> {
        match name.as_str() {
            "achild" => Ok(child_with_x_input("achild")),
            "bchild" => Ok(child_with_one_input("bchild", true)),
            other => panic!("resolver asked for an unexpected workflow: {other}"),
        }
    }

    /// Acceptance 4, the module docs' `Site` note: when the bad
    /// reference sits inside another `uses:` step's own `with:`,
    /// `UnknownUsesOutput`'s `site` is `Site::Port` naming that `uses:`
    /// step (`a`) as `node` and the child input (`x`) as `port` -- not
    /// `b`, which is the *referenced* step (reported separately as
    /// `node`).
    #[test]
    fn an_unknown_uses_output_referenced_from_inside_another_uses_steps_with_is_sited_there() {
        let root = root_referencing_b_from_as_with(Binding::Step {
            node: node("b"),
            port: port("missing"),
        });
        let err = link(&root, &mut resolve_a_and_b).unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::UnknownUsesOutput {
                site: Site::Port {
                    node: node("a"),
                    port: port("x"),
                },
                node: node("b"),
                output: output("missing"),
            }]
        );
    }

    /// Acceptance 4, the module docs' `Site` note: the same "inside
    /// another `uses:` step's `with:`" site, for `KeyedOnUses`.
    #[test]
    fn a_keyed_reference_from_inside_another_uses_steps_with_is_sited_there() {
        let root = root_referencing_b_from_as_with(Binding::Keyed {
            node: node("b"),
            key: "k".to_string(),
            port: port("out"),
        });
        let err = link(&root, &mut resolve_a_and_b).unwrap_err();
        assert_eq!(
            err,
            vec![CheckError::KeyedOnUses {
                site: Site::Port {
                    node: node("a"),
                    port: port("x"),
                },
                node: node("b"),
            }]
        );
    }
}
