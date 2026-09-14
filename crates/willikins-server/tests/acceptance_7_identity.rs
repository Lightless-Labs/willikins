//! Acceptance test 7 (identity half): the approval gate, driven through
//! `Butler` rather than `willikins_core::apply` directly. The HTTP half
//! (the approval page, nonce/origin checks) is task 10b's.

mod common;

use willikins_core::Class;
use willikins_journal::{ApplyRefusedReason, Event, RunState};
use willikins_server::{ApprovalRequirement, ButlerError};
use willikins_types::{DomainType, WorkflowName};

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

/// The positive fixture's class does not require approval: `plan` reports
/// `Automatic`, `apply` with no decision at all runs it, and the journal
/// has `ApprovalAutomatic { class: Reversible }`.
#[test]
fn the_positive_fixture_auto_approves_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("the positive fixture plans cleanly");
    assert!(!response.requires_approval);
    assert_eq!(response.approval, ApprovalRequirement::Automatic);

    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect("an automatic plan runs without approval");
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, RunState::Succeeded, "{run:?}");

    let entries = journal.lock().unwrap();
    let has_approval_automatic = entries.entries().iter().any(|entry| {
        matches!(
            &entry.event,
            Event::ApprovalAutomatic { plan_id, class }
                if *plan_id == response.plan_id && *class == Class::Reversible
        )
    });
    assert!(
        has_approval_automatic,
        "no ApprovalAutomatic{{class: Reversible}} recorded"
    );
}

/// `workflows/fixtures/irreversible.yaml`'s own internal name is
/// `new-rust-service-irreversible`; copied in under that name so the
/// filename-stem lookup finds it (see `willikins_server::document`'s
/// module docs).
fn irreversible_workflow_name() -> WorkflowName {
    wf("new-rust-service-irreversible")
}

fn setup_irreversible() -> (
    tempfile::TempDir,
    willikins_server::Butler,
    willikins_server::SharedJournal,
) {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(
        dir.path(),
        "irreversible.yaml",
        "new-rust-service-irreversible.yaml",
    );
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);
    (dir, butler, journal)
}

/// Unapproved: `apply` refuses `ApprovalRequired`, the journal has
/// `ApplyRefused` and no `RunStarted`.
#[test]
fn an_unapproved_irreversible_plan_refuses_and_journals_no_run_started() {
    let (_dir, butler, journal) = setup_irreversible();
    let response = butler
        .plan(
            irreversible_workflow_name(),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("the irreversible fixture plans cleanly");
    assert!(response.requires_approval);
    assert_eq!(response.approval, ApprovalRequirement::Pending);

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("an unapproved irreversible plan must refuse");
    assert!(
        matches!(
            err,
            ButlerError::ApprovalRequired {
                class: Class::Irreversible
            }
        ),
        "{err:?}"
    );

    let entries = journal.lock().unwrap();
    let refused = entries.entries().iter().any(|entry| {
        matches!(
            &entry.event,
            Event::ApplyRefused { plan_id, reason: ApplyRefusedReason::ApprovalRequired, .. }
                if *plan_id == response.plan_id
        )
    });
    assert!(refused, "no ApplyRefused{{ApprovalRequired}} recorded");
    let started_a_run = entries
        .entries()
        .iter()
        .any(|entry| matches!(&entry.event, Event::RunStarted { .. }));
    assert!(!started_a_run, "nothing should have started a run");
}

/// After approval, the same plan runs.
#[test]
fn an_approved_irreversible_plan_runs() {
    let (_dir, butler, _journal) = setup_irreversible();
    let response = butler
        .plan(
            irreversible_workflow_name(),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();

    butler
        .approve(response.plan_id, common::principal("approver"))
        .expect("approval succeeds");

    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect("an approved plan runs");
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, RunState::Succeeded, "{run:?}");
}

/// After rejection, a later `apply` refuses `ApprovalRequired`.
#[test]
fn a_rejected_plan_still_refuses_apply() {
    let (_dir, butler, _journal) = setup_irreversible();
    let response = butler
        .plan(
            irreversible_workflow_name(),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();

    butler
        .reject(
            response.plan_id,
            common::principal("approver"),
            common::reason("no"),
        )
        .expect("rejection succeeds");

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("a rejected plan must still refuse apply");
    assert!(
        matches!(
            err,
            ButlerError::ApprovalRequired {
                class: Class::Irreversible
            }
        ),
        "{err:?}"
    );
}

/// Adversarial pass 1, item 2
/// (`docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`):
/// the journal's own fold lets a later `ApprovalGranted` overwrite an
/// earlier `ApprovalRejected` (an append-only log has to fold what it is
/// given). `Butler::approve`/`reject` is what refuses a second decision
/// outright, so this attack never reaches the journal at all through the
/// public API: a `PlanRecorded`'s decision is final the moment the first
/// `approve` or `reject` for it succeeds.
#[test]
fn a_grant_after_a_rejection_is_refused_as_already_decided() {
    let (_dir, butler, _journal) = setup_irreversible();
    let response = butler
        .plan(
            irreversible_workflow_name(),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();

    butler
        .reject(
            response.plan_id,
            common::principal("approver"),
            common::reason("no"),
        )
        .expect("the first decision succeeds");

    let err = butler
        .approve(response.plan_id, common::principal("approver"))
        .expect_err("a plan that already has a decision must refuse a second one");
    assert!(matches!(err, ButlerError::AlreadyDecided { plan_id } if plan_id == response.plan_id));
}

/// The mirror direction: a rejection after a grant is refused the same
/// way.
#[test]
fn a_rejection_after_a_grant_is_refused_as_already_decided() {
    let (_dir, butler, _journal) = setup_irreversible();
    let response = butler
        .plan(
            irreversible_workflow_name(),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();

    butler
        .approve(response.plan_id, common::principal("approver"))
        .expect("the first decision succeeds");

    let err = butler
        .reject(
            response.plan_id,
            common::principal("approver"),
            common::reason("changed my mind"),
        )
        .expect_err("a plan that already has a decision must refuse a second one");
    assert!(matches!(err, ButlerError::AlreadyDecided { plan_id } if plan_id == response.plan_id));
}
