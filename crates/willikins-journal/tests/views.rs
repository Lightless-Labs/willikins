//! The replay views ([`Journal::pending_approvals`], [`Journal::plan`],
//! [`Journal::runs`], [`Journal::run`]) driven by hand-built event
//! sequences rather than a real `apply` run, so each view's own rules can
//! be exercised at states a live run cannot reach in one go: a plan
//! applied without an approval event, a run whose `RunFinished` never
//! arrived, two runs of one plan.
//!
//! `tests/acceptance_10_journal.rs` covers the happy path end to end
//! through a real run; this file covers the edges.

mod common;

use common::{node, principal, reason, workflow_name};
use indexmap::IndexMap;
use willikins_core::{Class, InstanceFingerprint, NodeStatus, OutputName, Value};
use willikins_journal::{
    ApprovalState, Event, Journal, MemoryJournal, PlanId, Redacted, RunId, RunState,
};

/// A `PlanRecorded` event for `plan_id`, requiring approval or not, whose
/// plan fingerprint names `instances` (as plain, instance-less nodes).
fn plan_recorded(plan_id: PlanId, requires_approval: bool, instances: &[&str]) -> Event {
    let fingerprint: Vec<InstanceFingerprint> = instances
        .iter()
        .map(|name| InstanceFingerprint {
            name: node(name),
            instance: None,
            action: willikins_core::Action::Create,
            outputs: Vec::new(),
        })
        .collect();
    let plan = willikins_core::Plan {
        workflow: workflow_name("wf"),
        nodes: Vec::new(),
        outputs: IndexMap::new(),
        class: if requires_approval {
            Class::Irreversible
        } else {
            Class::Reversible
        },
        requires_approval,
    };
    Event::PlanRecorded {
        plan_id,
        workflow: workflow_name("wf"),
        document_sha256: common::document_sha256("sha"),
        inputs: Redacted::from(&IndexMap::new()),
        plan: Redacted::from(&plan),
        fingerprint,
        class: plan.class,
        requires_approval,
    }
}

fn run_started(run_id: RunId, plan_id: PlanId) -> Event {
    Event::RunStarted {
        run_id,
        plan_id,
        principal: principal("agent"),
    }
}

fn node_finished(run_id: RunId, name: &str, status: NodeStatus) -> Event {
    Event::NodeFinished {
        run_id,
        node: node(name),
        instance: None,
        status,
        outputs: Redacted::from(&willikins_core::Outputs::new()),
        error: None,
    }
}

fn run_finished_ok(run_id: RunId) -> Event {
    Event::RunFinished {
        run_id,
        outcome: willikins_journal::Outcome::Succeeded {
            outputs: Redacted::from(&IndexMap::<OutputName, Value>::new()),
        },
    }
}

/// A plan whose run has already started is not waiting on a human,
/// whatever its approval events say. Journaling a `RunStarted` for a plan
/// that requires approval but carries no `ApprovalGranted` is not
/// something `run_and_journal` can produce today, but the view is a fold
/// over whatever the file holds -- including a file written by an older or
/// buggier build, or one whose `ApprovalGranted` append failed while the
/// run itself went ahead. Leaving such a plan in `pending_approvals()`
/// would invite an operator to approve something that already ran.
#[test]
fn pending_approvals_excludes_a_plan_that_has_already_been_applied() {
    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    let run_id = RunId::new();
    journal
        .append(plan_recorded(plan_id, true, &["repo"]))
        .unwrap();
    journal.append(run_started(run_id, plan_id)).unwrap();

    let record = journal.plan(&plan_id).expect("plan must be recorded");
    assert!(
        record.applied.is_some(),
        "the fold must mark the plan applied: {record:?}"
    );
    assert!(
        matches!(record.approval, ApprovalState::Pending),
        "this scenario's whole point is an applied plan whose approval never landed: {record:?}"
    );

    assert!(
        journal.pending_approvals().is_empty(),
        "an applied plan must not sit in pending_approvals: {:?}",
        journal.pending_approvals()
    );
}

/// The four approval states across four plans: only the one that requires
/// approval and has no decision recorded is pending, and the view is the
/// same after a reopen of the same file.
#[test]
fn pending_approvals_keeps_only_the_undecided_plan_and_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");

    let awaiting = PlanId::new();
    let automatic = PlanId::new();
    let granted = PlanId::new();
    let rejected = PlanId::new();

    {
        let mut journal = willikins_journal::FileJournal::open(&path).unwrap();
        journal.append(plan_recorded(awaiting, true, &[])).unwrap();
        journal
            .append(plan_recorded(automatic, false, &[]))
            .unwrap();
        journal
            .append(Event::ApprovalAutomatic {
                plan_id: automatic,
                class: Class::Reversible,
            })
            .unwrap();
        journal.append(plan_recorded(granted, true, &[])).unwrap();
        journal
            .append(Event::ApprovalGranted {
                plan_id: granted,
                approver: principal("approver"),
            })
            .unwrap();
        journal.append(plan_recorded(rejected, true, &[])).unwrap();
        journal
            .append(Event::ApprovalRejected {
                plan_id: rejected,
                approver: principal("approver"),
                reason: reason("not today"),
            })
            .unwrap();

        let pending = journal.pending_approvals();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].plan_id, awaiting);
    }

    let reopened = willikins_journal::FileJournal::open(&path).unwrap();
    let pending = reopened.pending_approvals();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0].plan_id, awaiting);
}

/// A run whose `RunFinished` never arrived -- the process died, or is
/// still going: `Running`, the instances it did finish, and every other
/// instance the recorded plan named filled in as `NotRun`.
#[test]
fn a_run_without_its_run_finished_reads_running_with_not_run_instances() {
    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    let run_id = RunId::new();
    journal
        .append(plan_recorded(plan_id, false, &["repo", "project"]))
        .unwrap();
    journal.append(run_started(run_id, plan_id)).unwrap();
    journal
        .append(node_finished(run_id, "repo", NodeStatus::Created))
        .unwrap();

    let run = journal.run(&run_id).expect("the run must replay");
    assert_eq!(run.state, RunState::Running);
    assert!(run.finished_at.is_none());
    assert!(run.error.is_none());
    assert_eq!(run.nodes.len(), 2, "{:?}", run.nodes);
    assert_eq!(run.nodes[0].node, node("repo"));
    assert!(matches!(run.nodes[0].status, NodeStatus::Created));
    assert_eq!(
        run.nodes[1].node,
        node("project"),
        "the plan's own order must drive the list: {:?}",
        run.nodes
    );
    assert!(
        matches!(run.nodes[1].status, NodeStatus::NotRun),
        "an instance the run never reached must read NotRun: {:?}",
        run.nodes[1]
    );
}

/// Two `RunStarted` events for one plan. `willikins-server` forbids this
/// (a second `apply` while a run is in progress is `RunInProgress`, and
/// after one finishes `AlreadyApplied`), so this can only come from a
/// hand-written or corrupted file -- the views must still be total, with
/// both runs visible and the plan naming the later one.
#[test]
fn two_runs_of_one_plan_replay_without_panicking() {
    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    let first = RunId::new();
    let second = RunId::new();
    journal
        .append(plan_recorded(plan_id, false, &["repo"]))
        .unwrap();
    journal.append(run_started(first, plan_id)).unwrap();
    journal
        .append(node_finished(first, "repo", NodeStatus::Created))
        .unwrap();
    journal.append(run_finished_ok(first)).unwrap();
    journal.append(run_started(second, plan_id)).unwrap();
    journal
        .append(node_finished(second, "repo", NodeStatus::Unchanged))
        .unwrap();
    journal.append(run_finished_ok(second)).unwrap();

    let runs = journal.runs();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs[0].run_id, first);
    assert_eq!(runs[1].run_id, second);
    for run in &runs {
        assert_eq!(run.state, RunState::Succeeded);
        assert_eq!(run.nodes.len(), 1);
    }
    assert!(
        matches!(runs[0].nodes[0].status, NodeStatus::Created),
        "each run keeps its own node statuses: {:?}",
        runs[0].nodes
    );
    assert!(
        matches!(runs[1].nodes[0].status, NodeStatus::Unchanged),
        "each run keeps its own node statuses: {:?}",
        runs[1].nodes
    );

    let record = journal.plan(&plan_id).expect("plan must replay");
    assert_eq!(
        record.applied,
        Some(second),
        "the last RunStarted wins: {record:?}"
    );
}

/// An unknown id is `None`, not a panic or an empty record.
#[test]
fn an_unknown_plan_or_run_id_is_none() {
    let journal = MemoryJournal::new();
    assert!(journal.plan(&PlanId::new()).is_none());
    assert!(journal.run(&RunId::new()).is_none());
}
