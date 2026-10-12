//! Milestone 3m task R8: `Butler::plan_rollback`/`apply_rollback`
//! (decisions 5, 8; acceptance 8, 10), and `Butler::decide`'s fix to
//! resolve a `plan_id` in either record kind.
//!
//! The fake catalog has no delete-failure injection hook (`FakeState`'s
//! own `fail_ensure_once` is wired into no tool's `ensure`, let alone any
//! inverse's `delete`), so "a half-finished teardown" (acceptance 8) is
//! realized here the way decision 6 itself describes the identical
//! observable case: "[the operator] deletes the thing by hand, which then
//! reads `Gone`, and plans again" -- `standing()` cannot tell an
//! operator's own out-of-band delete apart from an earlier, interrupted
//! teardown's, so the mechanism this test exercises (re-plan reads a step
//! `Gone`; a fresh teardown with no actual delete left to do still
//! succeeds and marks the run) is the same one either story would reach.

use std::time::Duration;

use crate::common;

use willikins_core::rollback::{TeardownAction, TeardownStepOutcome};
use willikins_journal::{ApplyRefusedReason, Event, PlanId, RunId};
use willikins_server::ButlerError;
use willikins_types::{DomainType, DopplerProject, WorkflowName};

const SINGLE_PROJECT_DOC: &str = "\
name: rollback-r8-single
inputs:
  project: { type: DopplerProject }
steps:
  proj:
    tool: doppler.project.ensure
    teardown: delete
    with:
      project: ${{ inputs.project }}
";

/// Same shape as [`SINGLE_PROJECT_DOC`], under a different workflow name
/// -- decision 8's own LIFO rule is scoped "per workflow", so a test of
/// `LaterRunFirst` (and `decide`'s own either-record-kind fix, which has
/// no reason to care about LIFO at all) needs two independent runs that
/// never block each other.
const OTHER_PROJECT_DOC: &str = "\
name: rollback-r8-other
inputs:
  project: { type: DopplerProject }
steps:
  proj:
    tool: doppler.project.ensure
    teardown: delete
    with:
      project: ${{ inputs.project }}
";

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

fn project(slug: &str) -> DopplerProject {
    DopplerProject::parse(slug).unwrap()
}

fn write_doc(dir: &std::path::Path, filename: &str, contents: &str) {
    std::fs::write(dir.join(filename), contents).unwrap();
}

/// Write both [`SINGLE_PROJECT_DOC`] and [`OTHER_PROJECT_DOC`] into `dir`.
/// Every test writes both, even one that only ever plans the first, so
/// `plan_rollback`'s own `later_run_first` scan (which reads every
/// recorded run's own workflow) never trips over a document the journal
/// references but the trusted directory does not hold.
fn write_docs(dir: &std::path::Path) {
    write_doc(dir, "rollback-r8-single.yaml", SINGLE_PROJECT_DOC);
    write_doc(dir, "rollback-r8-other.yaml", OTHER_PROJECT_DOC);
}

/// Plan and apply `workflow` (one of the two docs [`write_docs`] writes)
/// for `slug`, waiting for the run to finish, and return its `run_id`.
fn plan_and_apply(
    butler: &willikins_server::Butler,
    workflow: &str,
    slug: &str,
) -> willikins_journal::RunId {
    let response = butler
        .plan(
            wf(workflow),
            &common::partial_inputs(&[("project", slug)]),
            common::principal("agent"),
        )
        .expect("the single-project document plans cleanly");
    assert!(
        !response.requires_approval,
        "doppler.project.ensure is Reversible"
    );
    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect("an auto-approved plan applies immediately");
    let run = common::wait_for_run(butler, handle.run_id, 2000);
    assert_eq!(run.state, willikins_journal::RunState::Succeeded, "{run:?}");
    handle.run_id
}

/// Whether this journal holds an `ApplyRefused` for `plan_id` whose
/// reason matches `expected` -- the same pattern
/// `acceptance_8_plan_identity.rs`'s own `assert_refused` uses, as a
/// predicate here since a couple of these refusals have no `plan_id` of
/// their own to filter by (nothing was minted yet).
fn refused(
    journal: &willikins_server::SharedJournal,
    plan_id: Option<PlanId>,
    expected: impl Fn(&ApplyRefusedReason) -> bool,
) -> bool {
    journal.lock().unwrap().entries().iter().any(|entry| {
        matches!(
            &entry.event,
            Event::ApplyRefused { plan_id: p, reason, .. }
                if plan_id.is_none_or(|expected_id| *p == expected_id) && expected(reason)
        )
    })
}

// ---------------------------------------------------------------------
// `Butler::decide` resolves a `plan_id` in either record kind.
// ---------------------------------------------------------------------

#[test]
fn decide_approves_one_teardown_plan_and_rejects_another() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let run_a = plan_and_apply(&butler, "rollback-r8-single", "r8-decide-a");
    let run_b = plan_and_apply(&butler, "rollback-r8-other", "r8-decide-b");

    let plan_a = butler
        .plan_rollback(run_a, common::principal("agent"))
        .expect("run_a's teardown plans cleanly");
    let plan_b = butler
        .plan_rollback(run_b, common::principal("agent"))
        .expect("run_b's teardown plans cleanly");

    butler
        .approve(plan_a.plan_id, common::principal("approver"))
        .expect("approving a teardown plan_id must work, not just a forward one");
    butler
        .reject(
            plan_b.plan_id,
            common::principal("approver"),
            common::reason("not needed"),
        )
        .expect("rejecting a teardown plan_id must work too");

    let second = butler.approve(plan_a.plan_id, common::principal("approver"));
    assert!(
        matches!(second, Err(ButlerError::AlreadyDecided { plan_id }) if plan_id == plan_a.plan_id)
    );
}

// ---------------------------------------------------------------------
// Happy path, resume, and AlreadyRolledBack (acceptance 8, 10).
// ---------------------------------------------------------------------

#[test]
fn a_full_rollback_cycle_deletes_the_project_and_marks_the_run() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let run_id = plan_and_apply(&butler, "rollback-r8-single", "r8-happy-path");
    let key = willikins_providers_fake::state::doppler_project_key(&project("r8-happy-path"));
    assert!(
        state.lock().unwrap().doppler_projects.contains_key(&key),
        "the forward run must have created the project"
    );

    let planned = butler
        .plan_rollback(run_id, common::principal("agent"))
        .expect("the run finished, so a teardown plans cleanly");
    assert_eq!(planned.teardown.steps.len(), 1);
    assert!(matches!(
        planned.teardown.steps[0].action,
        TeardownAction::Delete { uncertain: false }
    ));

    butler
        .approve(planned.plan_id, common::principal("approver"))
        .unwrap();
    let result = butler
        .apply_rollback(planned.plan_id, common::principal("agent"))
        .expect("the approved teardown applies cleanly");
    assert!(result.succeeded, "{result:?}");
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].1, TeardownStepOutcome::Deleted);

    assert!(
        !state.lock().unwrap().doppler_projects.contains_key(&key),
        "the project must actually be gone from the fake state"
    );

    let run = butler
        .run(run_id)
        .expect("the forward run is still recorded");
    assert_eq!(run.rolled_back_by, Some(result.run_id));

    // Regression: `apply_rollback` must release the single-apply slot on
    // its own, synchronous return path -- there is no run thread to do
    // it the way `Butler::apply` relies on. A forward `apply` on the same
    // `Butler`, right after `apply_rollback` returned, is the
    // discriminating check: `run_in_progress()` alone would not catch a
    // slot stuck at `Preparing` rather than `Running`.
    assert!(
        butler.run_in_progress().is_none(),
        "apply_rollback must not leave the single-apply slot claimed"
    );
    plan_and_apply(&butler, "rollback-r8-single", "r8-happy-path-again");

    // Acceptance 10: a second rollback of the same run is refused.
    let err = butler
        .plan_rollback(run_id, common::principal("agent"))
        .expect_err("a run already rolled back must refuse a second teardown");
    assert!(
        matches!(err, ButlerError::AlreadyRolledBack { teardown_run_id } if teardown_run_id == result.run_id)
    );
}

/// Acceptance 8's own resume behaviour, realized through the Butler layer
/// (see this file's own module doc for why): the project is removed from
/// the fake state directly, standing in for an earlier teardown attempt
/// (or the operator) having already deleted it; a fresh `plan_rollback`
/// reads that step `Gone`, and the teardown that follows still succeeds
/// and marks the run.
#[test]
fn planning_again_after_the_resource_is_already_gone_reads_it_gone_and_still_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let run_id = plan_and_apply(&butler, "rollback-r8-single", "r8-resume");
    let key = willikins_providers_fake::state::doppler_project_key(&project("r8-resume"));

    // Stand in for an interrupted teardown (or an operator's own manual
    // delete): the resource is gone, but this run is not yet marked
    // rolled back.
    state.lock().unwrap().doppler_projects.remove(&key);

    let planned = butler
        .plan_rollback(run_id, common::principal("agent"))
        .expect("re-planning after the resource is already gone must still plan cleanly");
    assert_eq!(planned.teardown.steps.len(), 1);
    assert!(
        matches!(planned.teardown.steps[0].action, TeardownAction::Gone),
        "{:?}",
        planned.teardown.steps[0].action
    );

    butler
        .approve(planned.plan_id, common::principal("approver"))
        .unwrap();
    let result = butler
        .apply_rollback(planned.plan_id, common::principal("agent"))
        .expect("a teardown with nothing left to delete still succeeds");
    assert!(result.succeeded, "{result:?}");
    // A `Gone`-actioned step is never a `Delete` step, so `apply_teardown`
    // never calls its inverse again at apply time: it is only ever
    // `Listed`, the same as `Kept`/`Retained`/`RidesWith`/`MadeByHand`/
    // `Unfinished` (`TeardownStepOutcome::Listed`'s own doc). Only a
    // `Delete`-actioned step whose own `delete()` call itself observes
    // `Gone` (a true mid-run resume) reports `AlreadyGone`.
    assert_eq!(result.steps[0].1, TeardownStepOutcome::Listed);

    let run = butler.run(run_id).unwrap();
    assert_eq!(run.rolled_back_by, Some(result.run_id));
}

// ---------------------------------------------------------------------
// Refusals, each journaled (acceptance 10).
// ---------------------------------------------------------------------

#[test]
fn an_unknown_run_id_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let unknown = RunId::new();
    let err = butler
        .plan_rollback(unknown, common::principal("agent"))
        .expect_err("an id the journal never saw at all must refuse");
    assert!(matches!(err, ButlerError::UnknownRun { run_id } if run_id == unknown));
}

#[test]
fn a_teardowns_own_run_id_is_refused_as_not_a_forward_run() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let run_id = plan_and_apply(&butler, "rollback-r8-single", "r8-not-forward");
    let planned = butler
        .plan_rollback(run_id, common::principal("agent"))
        .unwrap();
    butler
        .approve(planned.plan_id, common::principal("approver"))
        .unwrap();
    let result = butler
        .apply_rollback(planned.plan_id, common::principal("agent"))
        .unwrap();
    assert!(result.succeeded);

    let err = butler
        .plan_rollback(result.run_id, common::principal("agent"))
        .expect_err("a teardown's own run id is never itself a forward run");
    assert!(matches!(err, ButlerError::NotAForwardRun { run_id } if run_id == result.run_id));
    // Journaled under the teardown's own, already-recorded `plan_id`
    // (never an orphan id nothing can resolve back): see
    // `Butler::plan_rollback`'s own doc.
    assert!(
        refused(&journal, Some(planned.plan_id), |reason| matches!(
            reason,
            ApplyRefusedReason::NotAForwardRun
        )),
        "NotAForwardRun must be journaled under the teardown's own plan_id"
    );
}

#[test]
fn an_unfinished_run_refuses_run_unfinished() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    // Hand-append a `RunStarted` with no matching `RunFinished`: the
    // journal's own fold reads this as a run still going (or one whose
    // process died mid-way), exactly decision 8's own `RunUnfinished`
    // case -- and `plan_rollback`'s own checks never need the plan this
    // run_id names to exist at all before refusing on this basis.
    let plan_id = PlanId::new();
    let run_id = RunId::new();
    journal
        .lock()
        .unwrap()
        .append(Event::RunStarted {
            run_id,
            plan_id,
            principal: common::principal("agent"),
        })
        .unwrap();

    let err = butler
        .plan_rollback(run_id, common::principal("agent"))
        .expect_err("a run with no RunFinished must refuse");
    assert!(matches!(err, ButlerError::RunUnfinished { run_id: r } if r == run_id));
    assert!(
        refused(&journal, None, |reason| matches!(
            reason,
            ApplyRefusedReason::RunUnfinished
        )),
        "RunUnfinished must be journaled"
    );
}

#[test]
fn a_later_run_of_the_same_workflow_that_created_something_must_be_rolled_back_first() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let first = plan_and_apply(&butler, "rollback-r8-single", "r8-lifo-first");
    let second = plan_and_apply(&butler, "rollback-r8-single", "r8-lifo-second");

    let err = butler
        .plan_rollback(first, common::principal("agent"))
        .expect_err("the later run must be rolled back first");
    assert!(matches!(err, ButlerError::LaterRunFirst { run_id } if run_id == second));
    assert!(
        refused(&journal, None, |reason| matches!(
            reason,
            ApplyRefusedReason::LaterRunFirst { run_id } if *run_id == second
        )),
        "LaterRunFirst must be journaled"
    );

    // Rolling the later run back first clears the way.
    let planned = butler
        .plan_rollback(second, common::principal("agent"))
        .unwrap();
    butler
        .approve(planned.plan_id, common::principal("approver"))
        .unwrap();
    let result = butler
        .apply_rollback(planned.plan_id, common::principal("agent"))
        .unwrap();
    assert!(result.succeeded);

    butler
        .plan_rollback(first, common::principal("agent"))
        .expect("once the later run is rolled back, the first run's own teardown plans cleanly");
}

#[test]
fn a_document_changed_since_the_run_refuses_at_plan_rollback() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let run_id = plan_and_apply(&butler, "rollback-r8-single", "r8-doc-changed");

    // Edit the document in place, keeping its filename (and so its
    // workflow name) unchanged.
    write_doc(
        dir.path(),
        "rollback-r8-single.yaml",
        &format!("{SINGLE_PROJECT_DOC}# edited\n"),
    );

    let err = butler
        .plan_rollback(run_id, common::principal("agent"))
        .expect_err("a changed document must refuse");
    assert!(matches!(err, ButlerError::DocumentChanged { .. }));
    assert!(
        refused(&journal, None, |reason| matches!(
            reason,
            ApplyRefusedReason::DocumentChanged
        )),
        "DocumentChanged must be journaled"
    );
}

#[test]
fn applying_before_approval_refuses_approval_required() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let run_id = plan_and_apply(&butler, "rollback-r8-single", "r8-approval-required");
    let planned = butler
        .plan_rollback(run_id, common::principal("agent"))
        .unwrap();

    let err = butler
        .apply_rollback(planned.plan_id, common::principal("agent"))
        .expect_err("a teardown always requires approval");
    assert!(matches!(err, ButlerError::ApprovalRequired { .. }));
    assert!(
        refused(&journal, Some(planned.plan_id), |reason| matches!(
            reason,
            ApplyRefusedReason::ApprovalRequired
        )),
        "ApprovalRequired must be journaled"
    );
}

#[test]
fn an_approved_teardown_applied_after_the_apply_window_elapses_refuses_plan_expired() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock.clone());

    let run_id = plan_and_apply(&butler, "rollback-r8-single", "r8-plan-expired");
    let planned = butler
        .plan_rollback(run_id, common::principal("agent"))
        .unwrap();
    butler
        .approve(planned.plan_id, common::principal("approver"))
        .unwrap();

    clock.advance(willikins_server::ButlerConfig::DEFAULT_APPLY_WINDOW + Duration::from_secs(1));

    let err = butler
        .apply_rollback(planned.plan_id, common::principal("agent"))
        .expect_err("the apply window elapsed");
    assert!(matches!(
        err,
        ButlerError::PlanExpired {
            window: willikins_server::ExpiryWindow::Apply
        }
    ));
    assert!(
        refused(&journal, Some(planned.plan_id), |reason| matches!(
            reason,
            ApplyRefusedReason::PlanExpired
        )),
        "PlanExpired must be journaled"
    );
}

/// Decision 8's LIFO rule holds at apply, not only at plan (the
/// 2026-10-12 independent adversarial pass): a later run of the same
/// workflow that created something between this teardown's plan and its
/// apply must be rolled back first. The two runs here touch independent
/// projects, so no container drift check can see the later one -- only
/// the LIFO rule does -- and without a re-check at apply the earlier
/// run's project was deleted while the later run stood.
#[test]
fn a_later_run_between_teardown_plan_and_apply_refuses_later_run_first() {
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let first = plan_and_apply(&butler, "rollback-r8-single", "r8-lifo-apply-first");
    let planned = butler
        .plan_rollback(first, common::principal("agent"))
        .expect("no later run exists yet, so the teardown plans cleanly");
    let second = plan_and_apply(&butler, "rollback-r8-single", "r8-lifo-apply-second");
    butler
        .approve(planned.plan_id, common::principal("approver"))
        .unwrap();

    let err = butler
        .apply_rollback(planned.plan_id, common::principal("agent"))
        .expect_err("the later run must be rolled back first, at apply too");
    assert!(
        matches!(err, ButlerError::LaterRunFirst { run_id } if run_id == second),
        "{err:?}"
    );
    assert!(
        refused(&journal, Some(planned.plan_id), |reason| matches!(
            reason,
            ApplyRefusedReason::LaterRunFirst { run_id } if *run_id == second
        )),
        "LaterRunFirst must be journaled against the teardown's own plan_id"
    );
    let key = willikins_providers_fake::state::doppler_project_key(&project("r8-lifo-apply-first"));
    assert!(
        state.lock().unwrap().doppler_projects.contains_key(&key),
        "nothing may be deleted"
    );
    assert!(
        !journal
            .lock()
            .unwrap()
            .entries()
            .iter()
            .any(|entry| matches!(entry.event, Event::TeardownStarted { .. })),
        "the refusal precedes TeardownStarted"
    );
}

/// Trust boundary 8 through `uses:` (the 2026-10-12 independent
/// adversarial pass): a used document's own node keeps its policy, and
/// flipping it to `teardown: delete` after the run -- leaving the root's
/// own bytes untouched -- refuses `DocumentChanged`, never a teardown
/// that deletes what the document kept when the run was approved.
#[test]
fn a_used_documents_policy_flipped_after_the_run_refuses_document_changed() {
    const CHILD_KEEPS: &str = "\
name: rollback-r8-child
inputs:
  project: { type: DopplerProject }
steps:
  proj:
    tool: doppler.project.ensure
    with:
      project: ${{ inputs.project }}
";
    const ROOT: &str = "\
name: rollback-r8-root
inputs:
  project: { type: DopplerProject }
steps:
  child:
    uses: rollback-r8-child
    with:
      project: ${{ inputs.project }}
";
    let dir = tempfile::tempdir().unwrap();
    write_docs(dir.path());
    write_doc(dir.path(), "rollback-r8-child.yaml", CHILD_KEEPS);
    write_doc(dir.path(), "rollback-r8-root.yaml", ROOT);
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let run_id = plan_and_apply(&butler, "rollback-r8-root", "r8-composite-kept");
    let planned = butler
        .plan_rollback(run_id, common::principal("agent"))
        .expect("the unchanged composite plans a teardown");
    assert_eq!(planned.teardown.steps.len(), 1);
    assert!(
        matches!(
            planned.teardown.steps[0].action,
            TeardownAction::Kept { .. }
        ),
        "the used document's own node keeps its default policy: {:?}",
        planned.teardown.steps
    );

    write_doc(
        dir.path(),
        "rollback-r8-child.yaml",
        &CHILD_KEEPS.replace(
            "    tool: doppler.project.ensure\n",
            "    tool: doppler.project.ensure\n    teardown: delete\n",
        ),
    );
    let err = butler
        .plan_rollback(run_id, common::principal("agent"))
        .expect_err("a used document's policy changed after the run must refuse");
    assert!(
        matches!(err, ButlerError::DocumentChanged { .. }),
        "{err:?}"
    );
    assert!(
        refused(&journal, None, |reason| matches!(
            reason,
            ApplyRefusedReason::DocumentChanged
        )),
        "DocumentChanged must be journaled"
    );
    let key = willikins_providers_fake::state::doppler_project_key(&project("r8-composite-kept"));
    assert!(state.lock().unwrap().doppler_projects.contains_key(&key));
}
