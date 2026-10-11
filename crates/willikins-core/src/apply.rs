//! The apply executor: runs a [`crate::Plan`] a human or an auto-approval
//! has already blessed, minting the one [`SinkToken`] the whole run gets.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s "Apply
//! executor" section for the six numbered rules this module implements in
//! order; the doc comment on [`apply`] restates them next to the code.
//!
//! # Resolving an instance's inputs during a run
//!
//! Milestone 3n, decision (g3): `apply` resolves **every** port of an
//! instance from the graph, through the very same
//! [`crate::plan::bind_ports`] [`crate::plan::plan`] itself calls, fed
//! this run's own accumulating typed result tables instead of `plan`'s —
//! never only the [`Source::Step`]/[`Source::Aggregate`]/[`Source::Keyed`]
//! ports layered over a [`PlannedNode`]'s own `inputs`. A
//! [`Source::Literal`] (the same stored [`Value`]), [`Source::Input`] (the
//! same `inputs` map) and [`Source::Item`] (the same value [`walk`]
//! recorded for this instance) resolve to exactly the value `plan` already
//! used, so the [`ApplyEvent::NodeStarted`] an observer sees is unchanged
//! even though nothing is reused directly; only a `Step`/`Aggregate`/
//! `Keyed` source (or a [`Bound::List`] holding one) can actually differ
//! from what `plan` saw, since only an upstream node's real output can
//! change mid-run. One resolver, fed two different result tables, rather
//! than a second copy of `plan`'s own logic.
//!
//! **Unknown attribution comes from resolution.** [`bind_ports`] returns,
//! beside the resolved [`Inputs`], where every `Unknown` port's value came
//! from ([`crate::plan::UnknownOrigin`]): an upstream node
//! ([`Source::Step`]/[`Source::Aggregate`]/[`Source::Keyed`]) or a workflow
//! input with no known value ([`Source::Input`] -- awaited or not: an
//! un-awaited input the caller simply left `Unknown` attributes exactly
//! the same way). [`first_unknown_required_input`] reads that table
//! instead of re-deriving the same answer from a raw
//! [`crate::workflow::Binding`] after the fact.

use indexmap::IndexMap;

use crate::check::{Bound, Checked, CheckedInput, PortEdge, Shape, Source};
use crate::class::Class;
use crate::plan::{Action, BlockedGate, Group, Plan, PlannedNode};
use crate::plan::{
    EachInstanceResult, EachResult, FilledOutputs, InstanceFingerprint, PlanError, ResolveCtx,
    SingleResult, UnknownOrigin, Walk, bind_ports, fill_outputs, node_names_by_shape,
    resolve_source, walk,
};
use crate::site::Site;
use crate::tool::{Ensured, Inputs, Outputs, PortName, Tool, ToolError, ToolErrorKind};
use crate::value::Value;
use crate::workflow::{InputName, NodeName, OutputName};
use crate::{Catalog, ToolName};
use willikins_types::SinkToken;

mod principal;
mod timestamp;

pub use principal::PrincipalId;
pub use timestamp::Timestamp;

// ---------------------------------------------------------------------
// Types (section A)
// ---------------------------------------------------------------------

/// Who, or what policy, approved a plan for [`apply`].
///
/// `Auto` is the caller asserting no human looked at this plan; `apply`
/// itself decides whether that is good enough, from the checked
/// workflow's own [`Class`] rather than from the plan's
/// `requires_approval` flag (see `apply`'s rule 1).
/// A server only ever constructs `Human` from a journaled
/// `ApprovalGranted` event for the plan's own id — never from a bare
/// caller claim — but that binding is `willikins-server`'s job; this type
/// only carries the two shapes.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Approval {
    /// No human approved this run.
    Auto,
    /// A human approved this run.
    Human {
        /// The approving principal.
        approver: PrincipalId,
        /// When they approved it.
        at: Timestamp,
    },
}

/// What happened to one planned node instance during [`apply`].
///
/// Derives [`serde::Deserialize`] as well as `Serialize`: unlike most
/// types in this module, a `NodeStatus` never holds a [`crate::value::Value`]
/// (its only payload, [`ToolError`], is a plain message with no secret
/// obligation of its own — see that type's doc), so round-tripping it
/// through JSON cannot resurrect anything redaction hid. `willikins-journal`
/// relies on this to store a `NodeFinished` event's `status` field
/// untouched, rather than behind its `Redacted` wrapper the way `outputs`
/// (which does carry [`Value`]s) must be.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeStatus {
    /// A pure tool computed its outputs; there was nothing to create or
    /// leave alone.
    Computed,
    /// `ensure` reported [`Ensured::changed`] `true`.
    Created,
    /// `ensure` reported [`Ensured::changed`] `false`.
    Unchanged,
    /// A required input was [`crate::value::ValueState::Unknown`] and the
    /// planned action was [`Action::NoOp`]: the resource already matches,
    /// by the plan's own observation, and nothing was called.
    Converged,
    /// `ensure` returned an error.
    Failed {
        /// The failure `ensure` reported.
        error: ToolError,
    },
    /// This instance's own planned [`Action`] was [`Action::Blocked`]: a
    /// [`crate::tool::Gate`] read [`crate::tool::Observation::Absent`]. No
    /// tool was called; the operator must act and the run must be
    /// re-planned. See decision (j),
    /// `docs/plans/2026-09-27-milestone-3e-new-ios-app.md`.
    Blocked,
    /// This instance's own planned [`Action`] was [`Action::Skip`]: it
    /// binds a [`Self::Blocked`] or [`Self::Skipped`] node, directly or
    /// transitively, so it was never read or called. Distinct from
    /// [`Self::NotRun`], which means the *run itself* stopped before
    /// reaching this instance — a `Skipped` instance was reached, and the
    /// rest of the run continued past it.
    Skipped,
    /// This instance was never attempted: an earlier instance in the same
    /// run failed, and the run stopped there.
    NotRun,
}

/// What happened to one planned node instance, alongside its final
/// outputs.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AppliedNode {
    /// The node this instance belongs to.
    pub name: NodeName,
    /// The `for_each` instance key, if any.
    pub instance: Option<String>,
    /// The tool this node called.
    pub tool: ToolName,
    /// What happened.
    pub status: NodeStatus,
    /// This instance's final output values. Empty for [`NodeStatus::NotRun`]
    /// and [`NodeStatus::Failed`].
    pub outputs: Outputs,
}

/// The result of a successful [`apply`] run.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct Applied {
    /// Every attempted node instance, in plan order — including one whose
    /// own planned [`Action`] was [`Action::Blocked`] or [`Action::Skip`]
    /// (see [`NodeStatus::Blocked`]/[`NodeStatus::Skipped`]): the walk
    /// continues past those, unlike a tool failure. An instance the run
    /// stopped at (rejected by [`ApplyError::UnknownInput`] or
    /// [`ApplyError::UnknownRequiredInput`]) before it was ever started is
    /// absent here rather than carrying a synthetic status; see those
    /// variants' own docs.
    pub nodes: Vec<AppliedNode>,
    /// Every workflow output's resolved value.
    pub outputs: IndexMap<OutputName, Value>,
    /// Every gate this run found [`Action::Blocked`], taken from the fresh
    /// re-plan rule 2 produced — the same shape [`Plan::blocked`] carries,
    /// and empty under the same condition. `#[serde(skip_serializing_if =
    /// "Vec::is_empty")]` so an `Applied` with no blocked gate serializes
    /// byte-identically to before this field existed. `Ok(Applied)` with a
    /// non-empty `blocked` is a **blocked run**, not an error — see
    /// decision (j), `docs/plans/2026-09-27-milestone-3e-new-ios-app.md`.
    ///
    /// `default` as well, though nothing deserializes this type: the JSON
    /// Schema the MCP server publishes as a tool's `outputSchema` is built for
    /// the deserialize contract, which lists every field without a serde
    /// default as `required` -- and a skipped field that is `required`
    /// makes every result without it fail its own published schema
    /// (`crates/willikins-server/tests/mcp_output_schema_conformance.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked: Vec<BlockedGate>,
}

/// One node instance's identity — the `(node, instance)` pair a
/// [`Plan`]'s own walk is ordered by.
///
/// Carried by [`DriftKind::Instance`], which is the only place two
/// fingerprints can disagree about *which* instance sits at a position
/// rather than about that instance's content.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InstanceRef {
    /// The node.
    pub node: NodeName,
    /// Its `for_each` instance key, if any.
    pub instance: Option<String>,
}

impl std::fmt::Display for InstanceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.instance {
            Some(key) => write!(f, "{}[{key}]", self.node),
            None => write!(f, "{}", self.node),
        }
    }
}

/// Why a [`DriftKind::Action`]'s or [`DriftKind::Output`]'s planned and
/// observed values differ.
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriftKind {
    /// The two plans do not describe the same instance at this position
    /// of the walk at all: a different node or `for_each` key, a
    /// different set of output ports (so they cannot be the same tool),
    /// or an instance one side has and the other does not. Either the
    /// approved plan was not a plan of this workflow and these inputs, or
    /// a `for_each` source has changed shape; in both cases the approved
    /// plan does not describe the run that is about to happen, so nothing
    /// runs.
    Instance {
        /// The instance the approved plan has at this position, if any.
        planned: Option<InstanceRef>,
        /// The instance the fresh re-plan has there, if any.
        observed: Option<InstanceRef>,
    },
    /// The instance's planned action itself differs from what a fresh
    /// re-plan now observes.
    Action {
        /// What the approved plan said.
        planned: Action,
        /// What the fresh re-plan says now.
        observed: Action,
    },
    /// The instance's action still agrees, but a non-secret output's
    /// rendered value has changed.
    Output {
        /// The output port whose value changed.
        port: PortName,
        /// What the approved plan recorded.
        planned: Value,
        /// What the fresh re-plan observes now.
        observed: Value,
    },
}

/// Why [`apply`] could not run — or could not finish running — a [`Plan`].
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`) the same way
/// [`PlanError`] does. A refusal before any provider write
/// ([`Self::ApprovalRequired`], [`Self::Plan`], [`Self::Drift`]) carries no
/// partial result: nothing has been executed. A failure after the run
/// started ([`Self::UnknownInput`], [`Self::Tool`]) carries the partial
/// [`Applied`] built so far, so a journal or a CLI can show what happened
/// before the run stopped.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum ApplyError {
    /// The plan requires human approval and `approval` was
    /// [`Approval::Auto`]. Checked before any provider call.
    ApprovalRequired {
        /// The plan's approval class.
        class: Class,
    },
    /// Re-planning `checked` against `catalog` and `inputs` failed.
    Plan {
        /// The failure.
        error: PlanError,
    },
    /// The freshly re-planned instance at `node` (`instance`, if any)
    /// differs from what `approved` recorded. Nothing has been executed:
    /// drift is checked before the run's [`SinkToken`] is even minted.
    Drift {
        /// The node whose instance drifted.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// How it drifted. Renamed on the wire to `detail`: a field
        /// literally named `kind` would collide with this enum's own
        /// internal tag. Boxed (`Box<T>` serializes exactly as `T` would)
        /// so this variant does not dominate `ApplyError`'s overall size
        /// the way an inline [`DriftKind::Output`] -- two [`Value`]s wide
        /// -- otherwise would (`clippy::result_large_err`).
        #[serde(rename = "detail")]
        kind: Box<DriftKind>,
    },
    /// A [`Source::Step`], [`Source::Aggregate`] or [`Source::Keyed`] port
    /// required by `node`
    /// resolved to [`crate::value::ValueState::Unknown`] because `from`'s
    /// output cannot be re-read (its planned action was
    /// [`Action::NoOp`]/converged), and `node`'s own planned action was
    /// [`Action::Create`], so it cannot run without that value. The
    /// remedy, for a secret that was consumed by an earlier run and
    /// cannot be re-read, is `workflows/rotate-service-token.yaml`.
    UnknownInput {
        /// The blocked node.
        node: NodeName,
        /// The port whose value is unknown.
        port: PortName,
        /// The upstream node whose un-re-readable output supplies it.
        from: NodeName,
        /// The partial result: `node`'s own instance is absent (nothing
        /// was attempted for it), and every instance after it in plan
        /// order is [`NodeStatus::NotRun`].
        applied: Box<Applied>,
    },
    /// A required input port of `node` is
    /// [`crate::value::ValueState::Unknown`] and does *not* come from
    /// another node's output: it is bound to the workflow input `input`,
    /// for which the caller supplied no usable value.
    ///
    /// `plan` cannot catch this for the caller — a tool's `read` need not
    /// consult every one of its required ports (the fake
    /// `github.repo.ensure`'s `read` does not look at `visibility` when
    /// the repository is absent), so a plan can be produced without the
    /// value — and rule 4 will not call `ensure` without it. The run
    /// therefore stops here, with the same partial shape
    /// [`Self::UnknownInput`] carries: this instance was not attempted,
    /// and every instance after it in plan order is
    /// [`NodeStatus::NotRun`].
    UnknownRequiredInput {
        /// The blocked node.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// The port whose value is unknown.
        port: PortName,
        /// The workflow input that port is bound to.
        input: InputName,
        /// The partial result.
        applied: Box<Applied>,
    },
    /// `node`'s `ensure` returned an error, or (decision (f2)) its own
    /// output disagreed with its spec where it was produced -- the same
    /// position either way, since `ensure` has already run.
    Tool {
        /// The failed node.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// The failure.
        error: ToolError,
        /// The partial result: `node`'s own instance is
        /// [`NodeStatus::Failed`], and every instance after it in plan
        /// order is [`NodeStatus::NotRun`].
        applied: Box<Applied>,
    },
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApprovalRequired { class } => {
                write!(
                    f,
                    "this plan is {class:?} and requires approval before it can run"
                )
            }
            Self::Plan { error } => write!(f, "re-planning failed: {error}"),
            Self::Drift {
                node,
                instance,
                kind,
            } => {
                let where_ = match instance {
                    Some(key) => format!("{node}[{key}]"),
                    None => node.to_string(),
                };
                match kind.as_ref() {
                    DriftKind::Instance { planned, observed } => {
                        let render = |side: &Option<InstanceRef>| match side {
                            Some(reference) => reference.to_string(),
                            None => "nothing".to_string(),
                        };
                        write!(
                            f,
                            "{where_}: the approved plan and the current one describe \
                             different work here (approved {}, now {}); nothing was run",
                            render(planned),
                            render(observed)
                        )
                    }
                    DriftKind::Action { planned, observed } => write!(
                        f,
                        "{where_}: planned action {planned:?} no longer matches the current \
                         state ({observed:?}); nothing was run"
                    ),
                    DriftKind::Output { port, .. } => write!(
                        f,
                        "{where_}.{port}: the observed value has changed since this plan was \
                         approved; nothing was run"
                    ),
                }
            }
            Self::UnknownInput {
                node, port, from, ..
            } => write!(
                f,
                "{node}.{port}: this value comes from `{from}`, which cannot be re-read (its \
                 planned action was a no-op); if this is a secret that was already consumed, \
                 run `workflows/rotate-service-token.yaml` to mint and re-store a new one"
            ),
            Self::UnknownRequiredInput {
                node,
                instance,
                port,
                input,
                ..
            } => {
                let where_ = match instance {
                    Some(key) => format!("{node}[{key}]"),
                    None => node.to_string(),
                };
                write!(
                    f,
                    "{where_}.{port}: the workflow input `{input}` has no known value, so this \
                     step cannot run; supply it and plan again"
                )
            }
            Self::Tool {
                node,
                instance,
                error,
                ..
            } => {
                let where_ = match instance {
                    Some(key) => format!("{node}[{key}]"),
                    None => node.to_string(),
                };
                write!(f, "{where_}: {error}")
            }
        }
    }
}

impl std::error::Error for ApplyError {}

// ---------------------------------------------------------------------
// ApplyObserver (section B)
// ---------------------------------------------------------------------

/// One event [`apply`] reports as it runs, so a caller (a journal, a CLI
/// progress line) can see a run's shape even if the process dies mid-run.
///
/// Serializes internally tagged (`#[serde(tag = "kind", rename_all =
/// "snake_case")]`), the same convention as [`NodeStatus`] and every
/// other tagged enum in this crate: task 5's journal records these
/// events, so their JSON shape matters starting now, not only once the
/// journal lands.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApplyEvent {
    /// About to process this instance.
    NodeStarted {
        /// The node.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// The instance's resolved inputs. A secret input still prints
        /// its redaction marker: `Inputs`' own `Debug`/`Serialize` route
        /// through [`crate::value::Value::render`], so this is true by
        /// construction, not by this event's own effort.
        inputs: Inputs,
    },
    /// Finished processing this instance.
    NodeFinished {
        /// The node.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// What happened.
        status: NodeStatus,
        /// This instance's final outputs.
        outputs: Outputs,
    },
}

/// Something [`apply`] reports [`ApplyEvent`]s to as it runs.
pub trait ApplyObserver {
    /// Handle one event.
    fn on(&mut self, event: ApplyEvent);
}

/// An [`ApplyObserver`] that discards every event.
#[derive(Debug, Default)]
pub struct NoopObserver;

impl ApplyObserver for NoopObserver {
    fn on(&mut self, _event: ApplyEvent) {}
}

/// An [`ApplyObserver`] that records every event it sees, in order, for
/// tests to inspect afterwards.
#[derive(Debug, Default)]
pub struct RecordingObserver {
    /// Every event seen so far, in the order [`ApplyObserver::on`] was
    /// called.
    pub events: Vec<ApplyEvent>,
}

impl RecordingObserver {
    /// An observer with no events yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ApplyObserver for RecordingObserver {
    fn on(&mut self, event: ApplyEvent) {
        self.events.push(event);
    }
}

// ---------------------------------------------------------------------
// apply (section C)
// ---------------------------------------------------------------------

/// Run `approved` (a [`Plan`] for `checked` against `inputs` and
/// `catalog`), minting the one [`SinkToken`] the whole run gets.
///
/// Implements the design's six numbered rules in order:
///
/// 1. `approval == Auto` on work that needs a human refuses with
///    [`ApplyError::ApprovalRequired`], before any provider call. Whether
///    it needs one is read from `checked`'s own
///    [`Class`] — a property of the workflow and the tools it uses, which
///    a caller cannot forge — as well as from `approved.requires_approval`,
///    so neither a cleared flag nor a weaker workflow's plan can open the
///    gate.
/// 2. Re-plans via [`plan`]; a [`PlanError`] becomes [`ApplyError::Plan`].
///    Compares [`Plan::fingerprint`] per instance, in order; the first
///    difference is [`ApplyError::Drift`]. Nothing has been executed yet.
/// 3. Mints the run's one [`SinkToken`] — the only non-test call site in
///    the workspace.
/// 4. Walks the fresh plan in order, one node instance at a time,
///    classifying each into a [`NodeStatus`] per the table on
///    [`NodeStatus`]'s own variants, **by its planned [`Action`] first**:
///    [`Action::Blocked`] and [`Action::Skip`] are recorded as
///    [`NodeStatus::Blocked`]/[`NodeStatus::Skipped`] with no tool call and
///    no input resolution, and the walk **continues** past them; only a
///    tool failure stops the walk and returns the partial result, with a
///    [`NodeStatus::NotRun`] tail. `Applied::blocked` carries the fresh
///    plan's own `blocked` list, so a run that ends with every instance
///    classified and no tool failure but a non-empty `blocked` is a
///    **blocked run** (see decision (j),
///    `docs/plans/2026-09-27-milestone-3e-new-ios-app.md`), `Ok`, not an
///    error.
///
///    4a. Just before [`Tool::ensure`], an instance whose planned action
///    is not [`Action::Replace`] asks [`Tool::replaces`]; `true` fails that
///    instance with [`ToolErrorKind::Conflict`] before `ensure` runs, so a
///    delete the approved plan did not show never happens -- see
///    [`refuse_unplanned_replacement`].
/// 5. Resolves workflow outputs from the run's own instance outputs, the
///    same way [`plan`] resolves them from planned ones (see the module
///    docs).
/// 6. Calls `observer` with [`ApplyEvent::NodeStarted`] before and
///    [`ApplyEvent::NodeFinished`] after every attempted instance.
///
/// # Errors
///
/// Returns [`ApplyError`]; see its own variants.
///
/// # Panics
///
/// Panics if `checked`, `approved`, or `catalog` are mutually
/// inconsistent (a node or tool `approved` names that `checked` or
/// `catalog` does not have) — a caller contract violation, the same way
/// [`plan`] panics on one; see `plan`'s own doc.
#[allow(clippy::too_many_lines)]
// one function, six numbered rules; splitting further would scatter the sequence the module doc walks through
pub fn apply(
    checked: &Checked,
    inputs: &IndexMap<InputName, Value>,
    catalog: &Catalog,
    approved: &Plan,
    approval: &Approval,
    observer: &mut dyn ApplyObserver,
) -> Result<Applied, ApplyError> {
    // Rule 1. The class of the work about to run is read from `checked`,
    // not from `approved`: `Plan`'s fields are public, so its
    // `requires_approval` can arrive cleared (rebuilt from a journal line,
    // handed across a process boundary) or belong to another, weaker
    // workflow whose instances fingerprint identically — and rule 2's
    // drift check compares actions and outputs, never a class. `checked`
    // is what the run actually executes, and its class is derived from the
    // tools it uses. The approved plan's own flag still refuses on its
    // own, so a plan is never treated as less privileged than it claims.
    if (checked.class().requires_approval() || approved.requires_approval)
        && matches!(approval, Approval::Auto)
    {
        return Err(ApplyError::ApprovalRequired {
            class: checked.class(),
        });
    }

    // Rule 2.
    let fresh = walk(checked, inputs, catalog).map_err(|error| ApplyError::Plan { error })?;
    check_drift(approved, &fresh.plan)?;

    // Rule 3: the one non-test `SinkToken::new` call site in the
    // workspace; every other is a `#[cfg(test)]` item (see
    // `tests/sink_token_guard.rs`, acceptance test 4's `SinkToken` half).
    #[allow(clippy::disallowed_methods)]
    let token = SinkToken::new();

    // Rule 4. Walks `checked.nodes()`, zipped with `fresh.groups` (milestone
    // 3n, decision (g6)): a `for_each` node whose source has zero items
    // contributes no entry at all to `fresh.plan.nodes` (`plan` never
    // expands it), so driving the walk from `fresh.plan.nodes` would skip
    // such a node outright -- decision (f1), point 1. Walking
    // `checked.nodes()` instead visits every node; its own `Group` already
    // says whether it was wholly skipped, planned as one instance, or
    // expanded into several, with no need to scan `fresh.plan.nodes` for a
    // run of the same name.
    //
    // Pushed in lockstep with `checked.nodes()`, which mints
    // `SingleId`/`EachId` in exactly this order within each kind
    // (milestone 3n, decision (g1)).
    let mut single_results: Vec<SingleResult> = Vec::new();
    let mut each_results: Vec<EachResult> = Vec::new();
    let mut applied_nodes: Vec<AppliedNode> = Vec::new();

    // Milestone 3n, G6: translates a `Source::Step`/`Aggregate`/`Keyed`'s
    // own typed id back to its producing node's name, for
    // `first_unknown_required_input`'s own attribution -- the same shape
    // `plan`'s `walk` already builds for `GateTracking` (G5), shared here
    // via `node_names_by_shape` rather than re-derived.
    let (single_names, each_names) = node_names_by_shape(checked);

    for (checked_node, group) in checked.nodes().iter().zip(&fresh.groups) {
        let name = &checked_node.name;
        // G1: the tool handle `check` already resolved, never a second
        // catalog lookup by name (verify item 4).
        let tool = checked_node.tool.as_ref();
        let spec = tool.spec();

        // Milestone 3n, G3: `ports`' own `Source<I>` is type-erased to
        // `Source<()>` (`erase_ports`) right here, the one place that
        // still needs to match `Shape`'s two variants to get a concrete
        // type at all. Erasing loses nothing `resolve_instance_inputs`
        // needs -- `I` only ever appears in `Source::Item`'s own payload,
        // whose *value* it never reads (this instance's own `item`
        // parameter supplies that directly) -- and it means the `Group`
        // match just below never has to reconcile against `Shape` a
        // second time: every `Group` variant uses the same `ports`.
        let ports = match &checked_node.shape {
            Shape::Single { ports, .. } => erase_ports(ports),
            Shape::Each { ports, .. } => erase_ports(ports),
        };
        let is_each = matches!(&checked_node.shape, Shape::Each { .. });

        match group {
            // A whole-node `Action::Skip` (decision (j), point 3): plans
            // as a single instance, whatever `node.for_each` says, and
            // `apply_instance` reports it `Skipped` with no tool call --
            // see its own `Action::Blocked | Action::Skip` arm.
            Group::Skipped { planned } => {
                apply_instance(
                    checked.inputs(),
                    inputs,
                    catalog,
                    &single_results,
                    &each_results,
                    &single_names,
                    &each_names,
                    tool,
                    spec,
                    &ports,
                    &token,
                    name,
                    &fresh,
                    *planned,
                    None,
                    observer,
                    &mut applied_nodes,
                )?;
                if is_each {
                    each_results.push(EachResult::Skipped);
                } else {
                    single_results.push(SingleResult::Skipped);
                }
            }
            Group::Single { planned } => {
                let (outputs, _blocked) = apply_instance(
                    checked.inputs(),
                    inputs,
                    catalog,
                    &single_results,
                    &each_results,
                    &single_names,
                    &each_names,
                    tool,
                    spec,
                    &ports,
                    &token,
                    name,
                    &fresh,
                    *planned,
                    None,
                    observer,
                    &mut applied_nodes,
                )?;
                single_results.push(SingleResult::Outputs(FilledOutputs::from(outputs)));
            }
            Group::Each { instances } => {
                let mut each_instance_results = Vec::with_capacity(instances.len());
                for instance in instances {
                    let (outputs, blocked) = apply_instance(
                        checked.inputs(),
                        inputs,
                        catalog,
                        &single_results,
                        &each_results,
                        &single_names,
                        &each_names,
                        tool,
                        spec,
                        &ports,
                        &token,
                        name,
                        &fresh,
                        instance.planned,
                        Some(&instance.item),
                        observer,
                        &mut applied_nodes,
                    )?;
                    each_instance_results.push(EachInstanceResult {
                        key: instance.key.clone(),
                        outputs: FilledOutputs::from(outputs),
                        blocked,
                    });
                }
                each_results.push(EachResult::Instances(each_instance_results));
            }
        }
    }

    // Rule 5.
    let mut outputs = IndexMap::new();
    let ctx = ResolveCtx {
        checked_inputs: checked.inputs(),
        inputs,
        single: &single_results,
        each: &each_results,
        single_names: &single_names,
        each_names: &each_names,
    };
    for checked_output in checked.outputs() {
        let site = Site::Output {
            name: checked_output.name.clone(),
        };
        let value = resolve_source(&ctx, &checked_output.source, &site, None)
            .map_err(|error| ApplyError::Plan { error })?;
        outputs.insert(checked_output.name.clone(), value);
    }

    Ok(Applied {
        nodes: applied_nodes,
        outputs,
        blocked: fresh.plan.blocked,
    })
}

/// Run one planned instance of `node`, at `planned_idx` within
/// `fresh.plan.nodes`, and return its outputs and whether it was
/// `Action::Blocked` -- `apply`'s own rules 4 and 4a. Shared between
/// [`Group::Single`]'s and [`Group::Skipped`]'s one instance and each of
/// [`Group::Each`]'s, so the classification below (blocked/skipped, pure,
/// converged, `ensure`'d, or failed) is written once rather than per
/// `Group` variant.
///
/// `item` is this instance's own `for_each` item, exactly as [`walk`]
/// recorded it on [`EachInstance`] (`None` for a node with no `for_each`
/// -- decision (f1), point 2; decision (g6)).
///
/// # Errors
///
/// Propagates [`ApplyError::Tool`], [`ApplyError::UnknownInput`] or
/// [`ApplyError::UnknownRequiredInput`], each carrying every
/// [`AppliedNode`] pushed to `applied_nodes` so far plus a `NotRun` tail
/// for every planned instance after `planned_idx`.
/// Type-erase `ports`' own `Source<I>` to `Source<()>` (milestone 3n,
/// G3): `I` only ever appears in `Source::Item`'s own payload, and
/// resolving one never reads that payload's value (the caller's own
/// `item: Option<&Value>` supplies it directly, by convention -- see
/// [`resolve_source`]), so erasing it loses nothing. Lets `apply`'s main
/// loop pick `ports` once per node, from `Shape`'s two variants, with no
/// further generic parameter for the `Group` match just below to
/// reconcile against a second time.
fn erase_ports<I>(ports: &[PortEdge<I>]) -> Vec<PortEdge<()>> {
    ports
        .iter()
        .map(|port_edge| PortEdge {
            port: port_edge.port.clone(),
            with_position: port_edge.with_position,
            bound: match &port_edge.bound {
                Bound::Scalar { source, edge } => Bound::Scalar {
                    source: erase_source(source),
                    edge: edge.clone(),
                },
                Bound::List { elements, edge } => Bound::List {
                    elements: elements.iter().map(erase_source).collect(),
                    edge: edge.clone(),
                },
            },
        })
        .collect()
}

/// [`erase_ports`]'s own per-source conversion.
fn erase_source<I>(source: &Source<I>) -> Source<()> {
    match source {
        Source::Literal(value) => Source::Literal(value.clone()),
        Source::Input(id) => Source::Input(*id),
        Source::Item(_) => Source::Item(()),
        Source::Step { from, slot, ty } => Source::Step {
            from: *from,
            slot: *slot,
            ty: ty.clone(),
        },
        Source::Aggregate { from, slot, ty } => Source::Aggregate {
            from: *from,
            slot: *slot,
            ty: ty.clone(),
        },
        Source::Keyed {
            from,
            key,
            slot,
            ty,
        } => Source::Keyed {
            from: *from,
            key: key.clone(),
            slot: *slot,
            ty: ty.clone(),
        },
    }
}

#[allow(clippy::too_many_arguments)] // one instance's worth of classification; see the doc above
#[allow(clippy::too_many_lines)] // one instance's five outcomes (blocked/skip, pure, ensure's two, converged, two unknowns), not split further
fn apply_instance(
    checked_inputs: &[CheckedInput],
    inputs: &IndexMap<InputName, Value>,
    catalog: &Catalog,
    single: &[SingleResult],
    each: &[EachResult],
    single_names: &[NodeName],
    each_names: &[NodeName],
    tool: &dyn Tool,
    spec: &crate::tool::ToolSpec,
    ports: &[PortEdge<()>],
    token: &SinkToken,
    name: &NodeName,
    fresh: &Walk,
    planned_idx: usize,
    item: Option<&Value>,
    observer: &mut dyn ApplyObserver,
    applied_nodes: &mut Vec<AppliedNode>,
) -> Result<(Outputs, bool), ApplyError> {
    let planned = &fresh.plan.nodes[planned_idx];

    // Classify by `planned.action` first, before any input resolution and
    // before the `pure` branch: a `Blocked` gate (always pure) must not be
    // reported `Computed`, and a `Skip` instance's `inputs` are empty by
    // construction (`plan`, point 3), so resolving its bindings -- or even
    // checking a non-pure tool's required ports against them -- would
    // panic or deliver an `Unknown` through a conversion edge that expects
    // a known value. Neither tool is called (decision (j), point 4).
    if matches!(planned.action, Action::Blocked | Action::Skip) {
        let outputs = planned.outputs.clone();
        let blocked = planned.action == Action::Blocked;
        let status = if blocked {
            NodeStatus::Blocked
        } else {
            NodeStatus::Skipped
        };
        observer.on(ApplyEvent::NodeStarted {
            node: name.clone(),
            instance: planned.instance.clone(),
            inputs: planned.inputs.clone(),
        });
        observer.on(ApplyEvent::NodeFinished {
            node: name.clone(),
            instance: planned.instance.clone(),
            status: status.clone(),
            outputs: outputs.clone(),
        });
        applied_nodes.push(AppliedNode {
            name: name.clone(),
            instance: planned.instance.clone(),
            tool: spec.name.clone(),
            status,
            outputs: outputs.clone(),
        });
        return Ok((outputs, blocked));
    }

    let (resolved_inputs, origins) = resolve_instance_inputs(
        checked_inputs,
        inputs,
        single,
        each,
        single_names,
        each_names,
        name,
        spec,
        ports,
        item,
    )?;

    if spec.pure {
        let outputs = planned.outputs.clone();
        observer.on(ApplyEvent::NodeStarted {
            node: name.clone(),
            instance: planned.instance.clone(),
            inputs: resolved_inputs.clone(),
        });
        observer.on(ApplyEvent::NodeFinished {
            node: name.clone(),
            instance: planned.instance.clone(),
            status: NodeStatus::Computed,
            outputs: outputs.clone(),
        });
        applied_nodes.push(AppliedNode {
            name: name.clone(),
            instance: planned.instance.clone(),
            tool: spec.name.clone(),
            status: NodeStatus::Computed,
            outputs: outputs.clone(),
        });
        return Ok((outputs, false));
    }

    match first_unknown_required_input(spec, &origins) {
        None => {
            observer.on(ApplyEvent::NodeStarted {
                node: name.clone(),
                instance: planned.instance.clone(),
                inputs: resolved_inputs.clone(),
            });
            // Rule 4a: never run a replacement the approved plan did not
            // show.
            //
            // Decision (f2): `ensure`'s own outputs are checked against
            // `spec` here, in the same chain as `ensure` itself failing --
            // the position is the same either way, `ensure` has already
            // run, so a refused output is reported exactly like a tool
            // that failed outright (never a panic, and `applied`'s
            // partial result is built the same way).
            match refuse_unplanned_replacement(tool, planned, &resolved_inputs)
                .and_then(|()| tool.ensure(&resolved_inputs, token))
                .and_then(|Ensured { outputs, changed }| {
                    fill_outputs(catalog.registry(), spec, &outputs)
                        .map(|outputs| (outputs, changed))
                }) {
                Ok((outputs, changed)) => {
                    let status = if changed {
                        NodeStatus::Created
                    } else {
                        NodeStatus::Unchanged
                    };
                    observer.on(ApplyEvent::NodeFinished {
                        node: name.clone(),
                        instance: planned.instance.clone(),
                        status: status.clone(),
                        outputs: outputs.clone(),
                    });
                    applied_nodes.push(AppliedNode {
                        name: name.clone(),
                        instance: planned.instance.clone(),
                        tool: spec.name.clone(),
                        status,
                        outputs: outputs.clone(),
                    });
                    Ok((outputs, false))
                }
                Err(error) => {
                    observer.on(ApplyEvent::NodeFinished {
                        node: name.clone(),
                        instance: planned.instance.clone(),
                        status: NodeStatus::Failed {
                            error: error.clone(),
                        },
                        outputs: Outputs::new(),
                    });
                    applied_nodes.push(AppliedNode {
                        name: name.clone(),
                        instance: planned.instance.clone(),
                        tool: spec.name.clone(),
                        status: NodeStatus::Failed {
                            error: error.clone(),
                        },
                        outputs: Outputs::new(),
                    });
                    applied_nodes.extend(not_run_tail(&fresh.plan.nodes[planned_idx + 1..]));
                    Err(ApplyError::Tool {
                        node: name.clone(),
                        instance: planned.instance.clone(),
                        error,
                        applied: Box::new(Applied {
                            nodes: std::mem::take(applied_nodes),
                            outputs: IndexMap::new(),
                            blocked: Vec::new(),
                        }),
                    })
                }
            }
        }
        Some(_) if matches!(planned.action, Action::NoOp) => {
            let outputs = planned.outputs.clone();
            observer.on(ApplyEvent::NodeStarted {
                node: name.clone(),
                instance: planned.instance.clone(),
                inputs: resolved_inputs.clone(),
            });
            observer.on(ApplyEvent::NodeFinished {
                node: name.clone(),
                instance: planned.instance.clone(),
                status: NodeStatus::Converged,
                outputs: outputs.clone(),
            });
            applied_nodes.push(AppliedNode {
                name: name.clone(),
                instance: planned.instance.clone(),
                tool: spec.name.clone(),
                status: NodeStatus::Converged,
                outputs: outputs.clone(),
            });
            Ok((outputs, false))
        }
        Some(UnknownRequired::Upstream { port, from }) => {
            applied_nodes.extend(not_run_tail(&fresh.plan.nodes[planned_idx + 1..]));
            Err(ApplyError::UnknownInput {
                node: name.clone(),
                port,
                from,
                applied: Box::new(Applied {
                    nodes: std::mem::take(applied_nodes),
                    outputs: IndexMap::new(),
                    blocked: Vec::new(),
                }),
            })
        }
        Some(UnknownRequired::WorkflowInput { port, input }) => {
            applied_nodes.extend(not_run_tail(&fresh.plan.nodes[planned_idx + 1..]));
            Err(ApplyError::UnknownRequiredInput {
                node: name.clone(),
                instance: planned.instance.clone(),
                port,
                input,
                applied: Box::new(Applied {
                    nodes: std::mem::take(applied_nodes),
                    outputs: IndexMap::new(),
                    blocked: Vec::new(),
                }),
            })
        }
    }
}

/// Rule 4a (milestone 3e, adversarial pass 3, 2026-09-29): `Ok(())` unless
/// `ensure` would now delete an existing resource that the approved plan
/// did not show as [`Action::Replace`].
///
/// Rule 2's drift check runs once, before any node executes, so it cannot
/// see a resource invalidated *during* the run by an earlier node -- Apple
/// invalidates a profile when a capability is enabled on its App ID, and
/// Sample's capability nodes run before its profile nodes. Such a profile
/// planned `NoOp` (it was `ACTIVE`), and `apply` calls `ensure` on a `NoOp`
/// instance too, so `appstore.profile.ensure` would delete and recreate it
/// with `Plan::replacing` empty. Asking [`Tool::replaces`] here closes that:
/// the instance fails with [`ToolErrorKind::Conflict`], nothing is deleted,
/// and a re-run plans the `Replace` for an approver to see. A tool that
/// never replaces anything keeps [`Tool::replaces`]' default `false`, so
/// this costs it nothing; `appstore.profile.ensure` pays one more
/// read-only resolve per instance. What remains is the window between this
/// call and `ensure`'s own re-resolve, the same one every read-then-write
/// has.
///
/// Has no counterpart for [`Tool::updates`] (milestone 3h): an update
/// never deletes anything, so there is nothing here for an unplanned one
/// to make safe. A node planned [`Action::Update`] whose `replaces` turns
/// `true` mid-run still hits this guard exactly as a `NoOp` or `Create`
/// does -- `planned.action == Action::Replace` is the only exemption --
/// and an unplanned `Create`-to-`Update` (or `Update`-to-`Create`) change
/// mid-run is caught earlier still, by `check_drift`'s
/// [`DriftKind::Action`], before any node in the run executes.
///
/// # Errors
///
/// [`ToolErrorKind::Conflict`] for an unplanned replacement, or whatever
/// [`Tool::replaces`] itself returns.
fn refuse_unplanned_replacement(
    tool: &dyn Tool,
    planned: &PlannedNode,
    inputs: &Inputs,
) -> Result<(), ToolError> {
    if planned.action == Action::Replace || !tool.replaces(inputs)? {
        return Ok(());
    }
    Err(ToolError {
        kind: ToolErrorKind::Conflict,
        message: format!(
            "ensure would now delete the existing resource at this node's key and create a \
             fresh one, but the approved plan planned {:?} here, not Replace: it changed \
             during this run, most likely through an earlier node. Nothing was deleted; \
             re-run this document to plan the replacement and approve it",
            planned.action
        ),
    })
}

/// Every [`AppliedNode`] `remaining` should become, all
/// [`NodeStatus::NotRun`] with empty outputs — the tail of a run stopped
/// partway through.
fn not_run_tail(remaining: &[PlannedNode]) -> Vec<AppliedNode> {
    remaining
        .iter()
        .map(|planned| AppliedNode {
            name: planned.name.clone(),
            instance: planned.instance.clone(),
            tool: planned.tool.clone(),
            status: NodeStatus::NotRun,
            outputs: Outputs::new(),
        })
        .collect()
}

/// Resolve every one of `name`'s bound `ports` fresh, from the graph
/// (milestone 3n, decision (g3)): `crate::plan::bind_ports`, the exact
/// function `plan` itself calls, fed this run's own accumulating typed
/// result tables (`single`/`each`) instead of `plan`'s. A
/// [`Source::Literal`] (the same stored [`Value`]), [`Source::Input`]
/// (the same `inputs` map) and [`Source::Item`] (`item`, the same value
/// `crate::plan::walk` recorded for this instance) therefore resolve to
/// exactly what `plan` already used; only a `Step`, `Aggregate` or
/// `Keyed` source (or a [`Bound::List`] holding one) can actually differ,
/// since only an upstream node's real output can change mid-run --
/// `bind_ports` resolves every port alike, with nothing left here to
/// decide which ones might.
///
/// Also returns, beside the resolved [`Inputs`], where every port whose
/// delivered value turned out [`crate::value::ValueState::Unknown`] came
/// from ([`UnknownOrigin`]) -- [`first_unknown_required_input`]'s own
/// source, read instead of re-derived from a
/// [`crate::workflow::Binding`].
#[allow(clippy::too_many_arguments)] // single/each/single_names/each_names (milestone 3n, G3/G6) join the existing shape
fn resolve_instance_inputs(
    checked_inputs: &[CheckedInput],
    inputs: &IndexMap<InputName, Value>,
    single: &[SingleResult],
    each: &[EachResult],
    single_names: &[NodeName],
    each_names: &[NodeName],
    name: &NodeName,
    spec: &crate::tool::ToolSpec,
    ports: &[PortEdge<()>],
    item: Option<&Value>,
) -> Result<(Inputs, IndexMap<PortName, UnknownOrigin>), ApplyError> {
    let ctx = ResolveCtx {
        checked_inputs,
        inputs,
        single,
        each,
        single_names,
        each_names,
    };
    bind_ports(&ctx, name, spec, ports, item).map_err(|error| ApplyError::Plan { error })
}

/// Where an unknown required input's value was supposed to come from.
enum UnknownRequired {
    /// From an upstream node's output, which could not be re-read (the
    /// design's own convergence gap).
    Upstream {
        /// The unknown port.
        port: PortName,
        /// The upstream node that supplies it.
        from: NodeName,
    },
    /// From a workflow input the caller supplied no known value for.
    WorkflowInput {
        /// The unknown port.
        port: PortName,
        /// The workflow input it is bound to.
        input: InputName,
    },
}

/// The first required input port of `spec` with an entry in `origins` --
/// i.e. the first required port whose resolved, delivered value turned out
/// [`crate::value::ValueState::Unknown`] -- and where it came from, or
/// `None` when every required port is known (milestone 3n, decision
/// (g3)). `origins` (`resolve_instance_inputs`'s own [`UnknownOrigin`]
/// table) already names it the moment it is produced, so there is
/// nothing left to re-derive from a [`crate::workflow::Binding`] here,
/// and no required-and-unknown port can be missing from it: a
/// [`Source::Literal`] or [`Source::Item`] is always known, so every
/// other kind that resolves `Unknown` always has a name to blame.
fn first_unknown_required_input(
    spec: &crate::tool::ToolSpec,
    origins: &IndexMap<PortName, UnknownOrigin>,
) -> Option<UnknownRequired> {
    for (port, port_spec) in &spec.inputs {
        if !port_spec.required {
            continue;
        }
        let Some(origin) = origins.get(port) else {
            continue;
        };
        return Some(match origin {
            UnknownOrigin::Upstream(from) => UnknownRequired::Upstream {
                port: port.clone(),
                from: from.clone(),
            },
            UnknownOrigin::WorkflowInput(input) => UnknownRequired::WorkflowInput {
                port: port.clone(),
                input: input.clone(),
            },
        });
    }
    None
}

/// Compare `approved`'s and `fresh`'s fingerprints, instance by instance in
/// order, and return the first [`ApplyError::Drift`] found, if any.
///
/// Two fingerprints at the same position are compared in three steps,
/// widest first: *identity* (the `(node, instance)` pair, and the output
/// ports that pair declares), then the planned *action*, then each
/// non-secret output's rendered *value*. An identity mismatch —  a
/// different node, a different `for_each` key, a different set of output
/// ports, or an instance one side has and the other does not —  is
/// [`DriftKind::Instance`]: the approved plan does not describe the run
/// that is about to happen at all, which is not the same claim as "this
/// instance's action changed" and must not be reported as one. Comparing
/// by position alone would let an approved plan whose instances were
/// re-ordered (or which belongs to another workflow entirely) through
/// whenever the actions and rendered values happened to line up, and
/// would look the approved side's output port up in the fresh side's
/// outputs, which need not have it.
fn check_drift(approved: &Plan, fresh: &Plan) -> Result<(), ApplyError> {
    let approved_fp = approved.fingerprint();
    let fresh_fp = fresh.fingerprint();
    let len = approved_fp.len().max(fresh_fp.len());

    for idx in 0..len {
        match (approved_fp.get(idx), fresh_fp.get(idx)) {
            (Some(a), Some(b)) => {
                if a.name != b.name || a.instance != b.instance {
                    return Err(instance_drift(Some(a), Some(b)));
                }
                if a.action != b.action {
                    return Err(ApplyError::Drift {
                        node: a.name.clone(),
                        instance: a.instance.clone(),
                        kind: Box::new(DriftKind::Action {
                            planned: a.action,
                            observed: b.action,
                        }),
                    });
                }
                match first_output_difference(a, b) {
                    None => {}
                    Some(OutputDifference::Shape) => {
                        return Err(instance_drift(Some(a), Some(b)));
                    }
                    Some(OutputDifference::Value(port)) => {
                        // Both sides declare this port (`Shape` above
                        // covers every other case), so both `PlannedNode`s
                        // have a value for it: a fingerprint entry is
                        // built from its own `PlannedNode`'s outputs, in
                        // that map's order. A missing one would mean the
                        // two are not the same instance after all, which
                        // is `Instance` drift rather than a panic.
                        let planned = approved.nodes[idx].outputs.get(&port).cloned();
                        let observed = fresh.nodes[idx].outputs.get(&port).cloned();
                        let (Some(planned), Some(observed)) = (planned, observed) else {
                            return Err(instance_drift(Some(a), Some(b)));
                        };
                        return Err(ApplyError::Drift {
                            node: a.name.clone(),
                            instance: a.instance.clone(),
                            kind: Box::new(DriftKind::Output {
                                port,
                                planned,
                                observed,
                            }),
                        });
                    }
                }
            }
            (Some(a), None) => return Err(instance_drift(Some(a), None)),
            (None, Some(b)) => return Err(instance_drift(None, Some(b))),
            (None, None) => unreachable!("idx < len = max(approved_fp.len(), fresh_fp.len())"),
        }
    }
    Ok(())
}

/// The [`ApplyError::Drift`] for a position whose two sides do not
/// describe the same instance, attributed to whichever side has one (the
/// approved plan's, when both do).
///
/// # Panics
///
/// Panics if both sides are `None`, which its only caller never does.
fn instance_drift(
    planned: Option<&InstanceFingerprint>,
    observed: Option<&InstanceFingerprint>,
) -> ApplyError {
    fn reference(fingerprint: &InstanceFingerprint) -> InstanceRef {
        InstanceRef {
            node: fingerprint.name.clone(),
            instance: fingerprint.instance.clone(),
        }
    }
    let anchor = planned
        .or(observed)
        .unwrap_or_else(|| unreachable!("a drift always has at least one side"));
    ApplyError::Drift {
        node: anchor.name.clone(),
        instance: anchor.instance.clone(),
        kind: Box::new(DriftKind::Instance {
            planned: planned.map(reference),
            observed: observed.map(reference),
        }),
    }
}

/// How two [`InstanceFingerprint`]s' output lists differ.
enum OutputDifference {
    /// They do not declare the same output ports, in the same order: the
    /// two entries cannot describe the same tool's instance at all.
    Shape,
    /// They declare the same ports, and this one's rendered value differs.
    Value(PortName),
}

/// The first difference between `a`'s and `b`'s output lists, if any.
fn first_output_difference(
    a: &InstanceFingerprint,
    b: &InstanceFingerprint,
) -> Option<OutputDifference> {
    if a.outputs.len() != b.outputs.len() {
        return Some(OutputDifference::Shape);
    }
    for ((a_port, a_rendered), (b_port, b_rendered)) in a.outputs.iter().zip(&b.outputs) {
        if a_port != b_port {
            return Some(OutputDifference::Shape);
        }
        if a_rendered != b_rendered {
            return Some(OutputDifference::Value(a_port.clone()));
        }
    }
    None
}
