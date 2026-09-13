//! The apply executor: runs a [`crate::Plan`] a human or an auto-approval
//! has already blessed, minting the one [`SinkToken`] the whole run gets.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s "Apply
//! executor" section for the six numbered rules this module implements in
//! order; the doc comment on [`apply`] restates them next to the code.
//!
//! # Resolving an instance's inputs during a run
//!
//! [`crate::plan::plan`] resolves every node's `with` bindings from each
//! upstream node's *planned* outputs — the values a `read`-only pass
//! observed or predicted before this run touched anything. Once the run
//! is under way, an upstream node's real outputs can differ from what was
//! planned: a freshly minted secret is `Known` here even though `read`
//! could only ever report it `Unknown`. So a [`PlannedNode`]'s own
//! `inputs` are reused for every port bound by [`Binding::Literal`],
//! [`Binding::Input`], and [`Binding::Item`] — none of which can change
//! mid-run, since none of them depend on another node's output — and only
//! a [`Binding::Step`] or [`Binding::Keyed`] port is re-resolved, against
//! this run's own accumulating results, through the very same
//! [`crate::plan::resolve_binding`] (and the [`crate::plan::ResolveCtx`],
//! [`crate::plan::NodeResult`] it walks) that `plan` itself uses: one
//! resolver, fed two different result tables, rather than a second copy of
//! `plan`'s own logic.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::check::Checked;
use crate::class::Class;
use crate::plan::{Action, Plan, PlannedNode};
use crate::plan::{
    ForEachInstance, InstanceFingerprint, NodeResult, PlanError, ResolveCtx, fill_outputs, plan,
    resolve_binding,
};
use crate::site::Site;
use crate::tool::{Ensured, Inputs, Outputs, PortName, ToolError};
use crate::value::Value;
use crate::workflow::{Binding, InputName, Node, NodeName, OutputName, Workflow};
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
/// itself decides whether that is good enough (`Plan::requires_approval`).
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
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
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
    /// This instance was never attempted: an earlier instance in the same
    /// run failed or was blocked first.
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
    /// Every attempted node instance, in plan order. An instance blocked by
    /// [`ApplyError::UnknownInput`] before it was ever started is absent
    /// here rather than carrying a synthetic status; see that variant's
    /// own doc.
    pub nodes: Vec<AppliedNode>,
    /// Every workflow output's resolved value.
    pub outputs: IndexMap<OutputName, Value>,
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
    /// A [`Binding::Step`] or [`Binding::Keyed`] port required by `node`
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
    /// `node`'s `ensure` returned an error.
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
/// 1. `approved.requires_approval && approval == Auto` refuses with
///    [`ApplyError::ApprovalRequired`], before any provider call.
/// 2. Re-plans via [`plan`]; a [`PlanError`] becomes [`ApplyError::Plan`].
///    Compares [`Plan::fingerprint`] per instance, in order; the first
///    difference is [`ApplyError::Drift`]. Nothing has been executed yet.
/// 3. Mints the run's one [`SinkToken`] — the only non-test call site in
///    the workspace.
/// 4. Walks the fresh plan in order, one node instance at a time,
///    classifying each into a [`NodeStatus`] per the table on
///    [`NodeStatus`]'s own variants; a tool failure or a blocked instance
///    stops the walk and returns the partial result.
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
#[allow(clippy::too_many_lines)] // one function, six numbered rules; splitting further would scatter the sequence the module doc walks through
pub fn apply(
    checked: &Checked,
    inputs: &IndexMap<InputName, Value>,
    catalog: &Catalog,
    approved: &Plan,
    approval: &Approval,
    observer: &mut dyn ApplyObserver,
) -> Result<Applied, ApplyError> {
    // Rule 1.
    if approved.requires_approval && matches!(approval, Approval::Auto) {
        return Err(ApplyError::ApprovalRequired {
            class: approved.class,
        });
    }

    // Rule 2.
    let fresh = plan(checked, inputs, catalog).map_err(|error| ApplyError::Plan { error })?;
    check_drift(approved, &fresh)?;

    // Rule 3: the one non-test `SinkToken::new` call site in the
    // workspace; every other is a `#[cfg(test)]` item (see
    // `tests/sink_token_guard.rs`, acceptance test 4's `SinkToken` half).
    #[allow(clippy::disallowed_methods)]
    let token = SinkToken::new();

    // Rule 4.
    let workflow = &checked.workflow;
    let mut results: HashMap<NodeName, NodeResult> = HashMap::new();
    let mut applied_nodes: Vec<AppliedNode> = Vec::new();

    let mut index = 0;
    while index < fresh.nodes.len() {
        let name = fresh.nodes[index].name.clone();
        let node = workflow
            .nodes
            .get(&name)
            .unwrap_or_else(|| unreachable!("`fresh.nodes` names only workflow nodes"));
        let tool = catalog
            .get(&node.tool)
            .unwrap_or_else(|| unreachable!("`approved` was planned against a compatible catalog"));
        let spec = tool.spec();

        let mut group_end = index;
        let mut group_outputs: Vec<(Option<String>, Outputs)> = Vec::new();

        while group_end < fresh.nodes.len() && fresh.nodes[group_end].name == name {
            let planned = &fresh.nodes[group_end];
            let resolved_inputs =
                resolve_instance_inputs(workflow, inputs, catalog, &results, node, planned)?;

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
                group_outputs.push((planned.instance.clone(), outputs));
                group_end += 1;
                continue;
            }

            match find_upstream_unknown(spec, node, &resolved_inputs) {
                None => {
                    observer.on(ApplyEvent::NodeStarted {
                        node: name.clone(),
                        instance: planned.instance.clone(),
                        inputs: resolved_inputs.clone(),
                    });
                    match tool.ensure(&resolved_inputs, &token) {
                        Ok(Ensured { outputs, changed }) => {
                            let outputs = fill_outputs(spec, &outputs);
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
                            group_outputs.push((planned.instance.clone(), outputs));
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
                            applied_nodes.extend(not_run_tail(&fresh.nodes[group_end + 1..]));
                            return Err(ApplyError::Tool {
                                node: name.clone(),
                                instance: planned.instance.clone(),
                                error,
                                applied: Box::new(Applied {
                                    nodes: applied_nodes,
                                    outputs: IndexMap::new(),
                                }),
                            });
                        }
                    }
                }
                Some((_port, _from)) if matches!(planned.action, Action::NoOp) => {
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
                    group_outputs.push((planned.instance.clone(), outputs));
                }
                Some((port, from)) => {
                    applied_nodes.extend(not_run_tail(&fresh.nodes[group_end + 1..]));
                    return Err(ApplyError::UnknownInput {
                        node: name.clone(),
                        port,
                        from,
                        applied: Box::new(Applied {
                            nodes: applied_nodes,
                            outputs: IndexMap::new(),
                        }),
                    });
                }
            }
            group_end += 1;
        }

        let node_result = if node.for_each.is_none() {
            let (_, outputs) = group_outputs
                .into_iter()
                .next()
                .unwrap_or_else(|| unreachable!("a non-for_each node has exactly one instance"));
            NodeResult::Scalar(outputs)
        } else {
            NodeResult::ForEach(
                group_outputs
                    .into_iter()
                    .map(|(key, outputs)| ForEachInstance {
                        key: key.unwrap_or_else(|| {
                            unreachable!("every for_each instance carries its own key")
                        }),
                        outputs,
                    })
                    .collect(),
            )
        };
        results.insert(name.clone(), node_result);
        index = group_end;
    }

    // Rule 5.
    let mut outputs = IndexMap::new();
    let ctx = ResolveCtx {
        workflow,
        inputs,
        catalog,
        results: &results,
    };
    for (out_name, binding) in &workflow.outputs {
        if matches!(binding, Binding::Literal(_)) {
            continue;
        }
        let site = Site::Output {
            name: out_name.clone(),
        };
        let value = resolve_binding(&ctx, &site, binding, None)
            .map_err(|error| ApplyError::Plan { error })?;
        outputs.insert(out_name.clone(), value);
    }

    Ok(Applied {
        nodes: applied_nodes,
        outputs,
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

/// Resolve `planned`'s inputs the way this run has actually gone so far:
/// start from the values [`plan`] itself resolved (correct for every port
/// bound by [`Binding::Literal`], [`Binding::Input`], or [`Binding::Item`],
/// none of which can change mid-run), then re-resolve every
/// [`Binding::Step`] or [`Binding::Keyed`] port against `results`, this
/// run's own accumulating outputs.
fn resolve_instance_inputs(
    workflow: &Workflow,
    inputs: &IndexMap<InputName, Value>,
    catalog: &Catalog,
    results: &HashMap<NodeName, NodeResult>,
    node: &Node,
    planned: &PlannedNode,
) -> Result<Inputs, ApplyError> {
    let mut resolved = planned.inputs.clone();
    let ctx = ResolveCtx {
        workflow,
        inputs,
        catalog,
        results,
    };
    for (port, binding) in &node.with {
        if matches!(binding, Binding::Step { .. } | Binding::Keyed { .. }) {
            let site = Site::Port {
                node: planned.name.clone(),
                port: port.clone(),
            };
            let value = resolve_binding(&ctx, &site, binding, None)
                .map_err(|error| ApplyError::Plan { error })?;
            resolved.insert(port.clone(), value);
        }
    }
    Ok(resolved)
}

/// The first required input port of `spec` that is
/// [`crate::value::ValueState::Unknown`] in `resolved`, alongside the
/// upstream node its binding names — or `None` when every required port is
/// known.
///
/// # Panics
///
/// Panics if an unknown required port is bound by anything other than
/// [`Binding::Step`] or [`Binding::Keyed`]: `check` guarantees a
/// [`Binding::Literal`], [`Binding::Input`], or [`Binding::Item`] port is
/// always known by the time it reaches here, so finding one unknown would
/// mean `checked` and `catalog` disagree with what `apply` was given — the
/// same caller contract [`plan`] itself relies on.
fn find_upstream_unknown(
    spec: &crate::tool::ToolSpec,
    node: &Node,
    resolved: &Inputs,
) -> Option<(PortName, NodeName)> {
    for (port, port_spec) in &spec.inputs {
        if !port_spec.required {
            continue;
        }
        let known = resolved.get(port).is_some_and(Value::is_known);
        if known {
            continue;
        }
        return match node.with.get(port) {
            Some(Binding::Step { node: upstream, .. } | Binding::Keyed { node: upstream, .. }) => {
                Some((port.clone(), upstream.clone()))
            }
            _ => unreachable!(
                "a required port can only be Unknown via a Step or Keyed binding to an \
                 upstream node whose output cannot be re-read; `check` guarantees every other \
                 binding kind is always known"
            ),
        };
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
