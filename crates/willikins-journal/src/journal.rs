//! The [`Journal`] trait: `append`/`entries` plus the replay views, the
//! latter implemented once as a fold over `entries()` so [`crate::MemoryJournal`]
//! and [`crate::FileJournal`] share identical behaviour rather than each
//! reimplementing the same reconstruction.

use indexmap::IndexMap;

use willikins_core::{
    Class, InstanceFingerprint, NodeName, NodeStatus, OutputName, Outputs, Value,
};
use willikins_types::WorkflowName;

use crate::event::{Entry, Event, Outcome};
use crate::ids::{PlanId, RunId};
use crate::reason::Reason;
use crate::redacted::Redacted;
use crate::{JournalError, PrincipalId, Timestamp};

/// A pending plan's approval state, folded from the events that can
/// change it.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalState {
    /// No approval decision has been recorded yet.
    Pending,
    /// The plan's class did not require approval.
    Automatic,
    /// A human approved it.
    Granted {
        /// The approving principal.
        approver: PrincipalId,
        /// When.
        at: Timestamp,
    },
    /// A human rejected it.
    Rejected {
        /// The rejecting principal.
        approver: PrincipalId,
        /// Why.
        reason: Reason,
        /// When.
        at: Timestamp,
    },
}

/// A recorded plan, replayed from a [`Event::PlanRecorded`] event and
/// whatever approval and run events followed it.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct PlanRecord {
    /// This plan's id.
    pub plan_id: PlanId,
    /// The planned workflow's name.
    pub workflow: WorkflowName,
    /// The workflow document's content hash at plan time.
    pub document_sha256: String,
    /// The resolved workflow inputs the plan was built from.
    pub inputs: Redacted<IndexMap<willikins_core::InputName, Value>>,
    /// The plan itself.
    pub plan: Redacted<willikins_core::Plan>,
    /// The plan's per-instance fingerprint.
    pub fingerprint: Vec<InstanceFingerprint>,
    /// The plan's approval class.
    pub class: Class,
    /// Whether this plan requires human approval.
    pub requires_approval: bool,
    /// When `plan` recorded it.
    pub recorded_at: Timestamp,
    /// Its current approval state.
    pub approval: ApprovalState,
    /// The run this plan was applied as, if any. Set as soon as
    /// `RunStarted` is seen -- before the run finishes -- so a run that
    /// died mid-way still makes a second `apply` of the same plan refuse
    /// as `AlreadyApplied` rather than racing a live run.
    pub applied: Option<RunId>,
}

/// Whether a run is still going, or how it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// Started; no `RunFinished` seen yet (including a run whose process
    /// died mid-way, which looks the same from the journal's own point of
    /// view -- it has no way to distinguish "still running" from "will
    /// never finish").
    Running,
    /// Finished successfully.
    Succeeded,
    /// Finished with a failure.
    Failed,
}

/// One node instance's outcome within a [`RunRecord`], folded from a
/// [`Event::NodeFinished`] event -- or synthesized as
/// [`NodeStatus::NotRun`] for a planned instance the run never reached,
/// when the plan is known (see [`RunRecord::nodes`]'s own doc).
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct RunNode {
    /// The node.
    pub node: NodeName,
    /// Its `for_each` instance key, if any.
    pub instance: Option<String>,
    /// What happened.
    pub status: NodeStatus,
    /// This instance's final outputs.
    pub outputs: Redacted<Outputs>,
}

/// A run of a plan, replayed from its `RunStarted`, `NodeStarted`,
/// `NodeFinished`, and `RunFinished` events.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct RunRecord {
    /// This run's id.
    pub run_id: RunId,
    /// The plan it applied.
    pub plan_id: PlanId,
    /// Who started it.
    pub principal: PrincipalId,
    /// When it started.
    pub started_at: Timestamp,
    /// Its current state.
    pub state: RunState,
    /// Every node instance the run reached, in the applied plan's own
    /// order when that plan's `PlanRecorded` event is in this journal
    /// (with [`NodeStatus::NotRun`] filling in any instance the run never
    /// started); in the order `NodeFinished` events arrived otherwise.
    pub nodes: Vec<RunNode>,
    /// Every workflow output's resolved value. Empty until (and unless)
    /// the run's `RunFinished` reports [`Outcome::Succeeded`].
    pub outputs: Redacted<IndexMap<OutputName, Value>>,
    /// The failure the run ended with, if any.
    pub error: Option<Redacted<willikins_core::ApplyError>>,
    /// When it finished, if it has.
    pub finished_at: Option<Timestamp>,
}

/// An append-only, replayable event log.
///
/// `append` is the only way to add an entry; there is no delete or
/// rewrite of any kind on this trait, on [`crate::MemoryJournal`], or on
/// [`crate::FileJournal`] -- an audit trail that could edit its own past
/// would not be one.
pub trait Journal {
    /// Append `event`, returning the [`Entry`] it was wrapped in (with its
    /// assigned `seq` and `at`).
    ///
    /// # Errors
    ///
    /// Returns [`JournalError`] if the event could not be durably
    /// recorded.
    fn append(&mut self, event: Event) -> Result<Entry, JournalError>;

    /// Every entry recorded so far, in append order.
    fn entries(&self) -> &[Entry];

    /// Every plan still awaiting a human decision (recorded, but neither
    /// auto-approved nor granted nor rejected).
    fn pending_approvals(&self) -> Vec<PlanRecord> {
        fold(self.entries())
            .plans
            .into_values()
            .filter(|record| matches!(record.approval, ApprovalState::Pending))
            .collect()
    }

    /// One recorded plan, by id.
    fn plan(&self, plan_id: &PlanId) -> Option<PlanRecord> {
        fold(self.entries()).plans.shift_remove(plan_id)
    }

    /// Every run recorded so far.
    fn runs(&self) -> Vec<RunRecord> {
        fold(self.entries()).runs.into_values().collect()
    }

    /// One run, by id.
    fn run(&self, run_id: &RunId) -> Option<RunRecord> {
        fold(self.entries()).runs.shift_remove(run_id)
    }
}

/// The two replay views, built together in one pass over `entries` so a
/// run's [`RunRecord::nodes`] can consult its plan's fingerprint (see
/// [`finish_runs`]) without a second, separate fold.
struct Fold {
    plans: IndexMap<PlanId, PlanRecord>,
    runs: IndexMap<RunId, RunRecord>,
}

/// Per-run finished node instances, in the order their `NodeFinished`
/// event arrived, keyed by (node, instance) so a later event for the same
/// instance (there should never be one) replaces rather than duplicates.
type FinishedNodes = IndexMap<RunId, IndexMap<(NodeName, Option<String>), RunNode>>;

fn fold(entries: &[Entry]) -> Fold {
    let mut plans: IndexMap<PlanId, PlanRecord> = IndexMap::new();
    let mut runs: IndexMap<RunId, RunRecord> = IndexMap::new();
    let mut finished: FinishedNodes = IndexMap::new();

    for entry in entries {
        match &entry.event {
            Event::PlanRecorded { .. } => fold_plan_recorded(&mut plans, entry),
            Event::ApprovalAutomatic { .. }
            | Event::ApprovalGranted { .. }
            | Event::ApprovalRejected { .. } => fold_approval(&mut plans, entry),
            Event::RunStarted { .. } => {
                fold_run_started(&mut plans, &mut runs, &mut finished, entry);
            }
            Event::NodeFinished { .. } => fold_node_finished(&mut finished, entry),
            Event::RunFinished { .. } => fold_run_finished(&mut runs, entry),
            // No replay view depends on any of these: `ApplyRefused` is an
            // audit line about an attempt, not a state transition;
            // `NodeStarted`'s progress is folded from `NodeFinished` only
            // (see `RunRecord::nodes`'s own doc); the rest carry nothing
            // any view reads.
            Event::ApplyRefused { .. }
            | Event::NodeStarted { .. }
            | Event::ServerStarted { .. }
            | Event::ToolCalled { .. }
            | Event::AuthFailed { .. } => {}
        }
    }

    finish_runs(&mut runs, &plans, finished);

    Fold { plans, runs }
}

fn fold_plan_recorded(plans: &mut IndexMap<PlanId, PlanRecord>, entry: &Entry) {
    let Event::PlanRecorded {
        plan_id,
        workflow,
        document_sha256,
        inputs,
        plan,
        fingerprint,
        class,
        requires_approval,
    } = &entry.event
    else {
        unreachable!("fold_plan_recorded is only called for Event::PlanRecorded");
    };
    plans.insert(
        *plan_id,
        PlanRecord {
            plan_id: *plan_id,
            workflow: workflow.clone(),
            document_sha256: document_sha256.clone(),
            inputs: inputs.clone(),
            plan: plan.clone(),
            fingerprint: fingerprint.clone(),
            class: *class,
            requires_approval: *requires_approval,
            recorded_at: entry.at,
            approval: ApprovalState::Pending,
            applied: None,
        },
    );
}

fn fold_approval(plans: &mut IndexMap<PlanId, PlanRecord>, entry: &Entry) {
    let (plan_id, approval) = match &entry.event {
        Event::ApprovalAutomatic { plan_id, .. } => (plan_id, ApprovalState::Automatic),
        Event::ApprovalGranted { plan_id, approver } => (
            plan_id,
            ApprovalState::Granted {
                approver: approver.clone(),
                at: entry.at,
            },
        ),
        Event::ApprovalRejected {
            plan_id,
            approver,
            reason,
        } => (
            plan_id,
            ApprovalState::Rejected {
                approver: approver.clone(),
                reason: reason.clone(),
                at: entry.at,
            },
        ),
        _ => unreachable!("fold_approval is only called for the three approval events"),
    };
    if let Some(record) = plans.get_mut(plan_id) {
        record.approval = approval;
    }
}

fn fold_run_started(
    plans: &mut IndexMap<PlanId, PlanRecord>,
    runs: &mut IndexMap<RunId, RunRecord>,
    finished: &mut FinishedNodes,
    entry: &Entry,
) {
    let Event::RunStarted {
        run_id,
        plan_id,
        principal,
    } = &entry.event
    else {
        unreachable!("fold_run_started is only called for Event::RunStarted");
    };
    if let Some(record) = plans.get_mut(plan_id) {
        record.applied = Some(*run_id);
    }
    runs.insert(
        *run_id,
        RunRecord {
            run_id: *run_id,
            plan_id: *plan_id,
            principal: principal.clone(),
            started_at: entry.at,
            state: RunState::Running,
            nodes: Vec::new(),
            outputs: Redacted::from(&IndexMap::<OutputName, Value>::new()),
            error: None,
            finished_at: None,
        },
    );
    finished.insert(*run_id, IndexMap::new());
}

fn fold_node_finished(finished: &mut FinishedNodes, entry: &Entry) {
    let Event::NodeFinished {
        run_id,
        node,
        instance,
        status,
        outputs,
        ..
    } = &entry.event
    else {
        unreachable!("fold_node_finished is only called for Event::NodeFinished");
    };
    if let Some(map) = finished.get_mut(run_id) {
        map.insert(
            (node.clone(), instance.clone()),
            RunNode {
                node: node.clone(),
                instance: instance.clone(),
                status: status.clone(),
                outputs: outputs.clone(),
            },
        );
    }
}

fn fold_run_finished(runs: &mut IndexMap<RunId, RunRecord>, entry: &Entry) {
    let Event::RunFinished { run_id, outcome } = &entry.event else {
        unreachable!("fold_run_finished is only called for Event::RunFinished");
    };
    if let Some(run) = runs.get_mut(run_id) {
        match outcome {
            Outcome::Succeeded { outputs } => {
                run.state = RunState::Succeeded;
                run.outputs = outputs.clone();
            }
            Outcome::Failed { error } => {
                run.state = RunState::Failed;
                run.error = Some(error.clone());
            }
        }
        run.finished_at = Some(entry.at);
    }
}

/// Fill in each [`RunRecord`]'s `nodes`: the plan's own instance order
/// with [`NodeStatus::NotRun`] for anything never finished, when the plan
/// is known; otherwise just the finished instances, in the order they
/// finished.
fn finish_runs(
    runs: &mut IndexMap<RunId, RunRecord>,
    plans: &IndexMap<PlanId, PlanRecord>,
    mut finished: FinishedNodes,
) {
    for (run_id, run) in runs.iter_mut() {
        let observed = finished.shift_remove(run_id).unwrap_or_default();
        run.nodes = match plans.get(&run.plan_id) {
            Some(plan_record) => plan_record
                .fingerprint
                .iter()
                .map(|planned| {
                    observed
                        .get(&(planned.name.clone(), planned.instance.clone()))
                        .cloned()
                        .unwrap_or_else(|| RunNode {
                            node: planned.name.clone(),
                            instance: planned.instance.clone(),
                            status: NodeStatus::NotRun,
                            outputs: Redacted::from(&Outputs::new()),
                        })
                })
                .collect(),
            None => observed.into_values().collect(),
        };
    }
}
