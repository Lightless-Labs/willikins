//! Acceptance test 8: plan identity, drift, the two windows, and the
//! single-apply lock, driven through `Butler`.
//!
//! The fingerprint unit test acceptance test 8 also names ("two plans
//! that differ only in a non-secret observed output are `Drift { kind:
//! Output }`, and two that differ only in a seeded `doppler.secret.get`
//! value are equal") already exists in
//! `crates/willikins-core/src/plan.rs`'s own test module
//! (`two_plans_differing_only_in_a_non_secret_observed_output_have_different_fingerprints`,
//! `two_plans_differing_only_in_a_seeded_secret_value_have_equal_fingerprints`);
//! nothing here duplicates it.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use willikins_core::{Action, PortName};
use willikins_journal::{ApplyRefusedReason, Event, Journal, PlanId, RunState};
use willikins_providers_fake::FakeState;
use willikins_server::{ButlerConfig, ButlerError, DriftDetail};
use willikins_types::{DomainType, GitHubRepo, RepoVisibility, WorkflowName};

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

fn plan_identity_state() -> Arc<Mutex<FakeState>> {
    let path = common::workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join("plan-identity-secrets.json");
    let json = std::fs::read_to_string(&path).unwrap();
    Arc::new(Mutex::new(FakeState::from_json(&json).unwrap()))
}

/// Every refusal in this file is journaled as `ApplyRefused` with the
/// matching reason.
fn assert_refused(
    journal: &willikins_server::SharedJournal,
    plan_id: PlanId,
    expected: impl Fn(&ApplyRefusedReason) -> bool,
) {
    let entries = journal.lock().unwrap();
    let found = entries.entries().iter().any(|entry| {
        matches!(
            &entry.event,
            Event::ApplyRefused { plan_id: p, reason, .. } if *p == plan_id && expected(reason)
        )
    });
    assert!(found, "no matching ApplyRefused for {plan_id}");
}

// ---------------------------------------------------------------------
// Document changed
// ---------------------------------------------------------------------

/// `plan-identity-a.yaml` and `-b.yaml` plan to identical fingerprints
/// (see their own header comments); `Butler::apply` still refuses,
/// because it compares the document's bytes and internal name, not the
/// fingerprint alone.
#[test]
fn a_document_swapped_underneath_an_approved_plan_is_refused_as_document_changed() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "plan-identity-a.yaml", "plan-identity-a.yaml");
    let state = plan_identity_state();
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("plan-identity-a"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("document A plans cleanly");
    assert!(
        !response.requires_approval,
        "both plan-identity documents are Reversible"
    );

    let ensure_calls_before = state.lock().unwrap().ensure_calls.clone();

    // Overwrite the *same file* with document B's bytes, keeping the
    // filename (and so the workflow name `plan-identity-a`) unchanged.
    let b_bytes = std::fs::read(
        common::workspace_root()
            .join("workflows")
            .join("fixtures")
            .join("plan-identity-b.yaml"),
    )
    .unwrap();
    std::fs::write(dir.path().join("plan-identity-a.yaml"), b_bytes).unwrap();

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("a swapped document must refuse");
    assert!(
        matches!(err, ButlerError::DocumentChanged { .. }),
        "{err:?}"
    );
    assert_eq!(
        state.lock().unwrap().ensure_calls,
        ensure_calls_before,
        "no provider call must have happened"
    );
    assert_refused(&journal, response.plan_id, |reason| {
        matches!(reason, ApplyRefusedReason::DocumentChanged)
    });
}

// ---------------------------------------------------------------------
// Drift
// ---------------------------------------------------------------------

/// Between `plan` and `apply`, the fake state gains the exact repository
/// `plan` predicted `repo` would create: a fresh re-plan now reads it
/// `Present` (`Action::NoOp`) instead of `Action::Create`, which is
/// `Drift { node: repo, kind: Action }`, with no `ensure` call.
#[test]
fn state_that_changed_between_plan_and_apply_is_action_drift() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("the positive fixture plans cleanly");

    let repo_node = response
        .plan
        .nodes
        .iter()
        .find(|node| node.name.as_str() == "repo")
        .expect("the plan has a `repo` node");
    assert_eq!(repo_node.action, Action::Create);
    let repo: GitHubRepo = repo_node
        .inputs
        .get(&PortName::parse("repo").unwrap())
        .and_then(|value| value.downcast::<GitHubRepo>())
        .cloned()
        .expect("the repo node's own `repo` input is known");

    {
        let mut guard = state.lock().unwrap();
        let taken = std::mem::replace(&mut *guard, FakeState::new());
        *guard = taken.with_repo(&repo, RepoVisibility::Private, true);
    }
    let ensure_calls_before = state.lock().unwrap().ensure_calls.clone();

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("state drifted between plan and apply");
    match &err {
        ButlerError::Drift { node, kind, .. } => {
            assert_eq!(node.as_str(), "repo");
            match kind.as_ref() {
                DriftDetail::Action { planned, observed } => {
                    assert_eq!(*planned, Action::Create);
                    assert_eq!(*observed, Action::NoOp);
                }
                other => panic!("expected Action drift, got {other:?}"),
            }
        }
        other => panic!("expected Drift, got {other:?}"),
    }
    assert_eq!(
        state.lock().unwrap().ensure_calls,
        ensure_calls_before,
        "no ensure call must have happened"
    );
    assert_refused(&journal, response.plan_id, |reason| {
        matches!(reason, ApplyRefusedReason::Drift { .. })
    });
}

// ---------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------

fn setup_irreversible(dir: &std::path::Path) {
    common::copy_fixture_as(
        dir,
        "irreversible.yaml",
        "new-rust-service-irreversible.yaml",
    );
}

#[test]
fn an_auto_approved_plan_applied_after_the_apply_window_expires() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock.clone());

    let response = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();
    clock.advance(ButlerConfig::DEFAULT_APPLY_WINDOW + Duration::from_secs(1));

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("the apply window elapsed");
    assert!(
        matches!(
            err,
            ButlerError::PlanExpired {
                window: willikins_server::ExpiryWindow::Apply
            }
        ),
        "{err:?}"
    );
    assert_refused(&journal, response.plan_id, |reason| {
        matches!(reason, ApplyRefusedReason::PlanExpired)
    });
}

#[test]
fn a_pending_plan_approved_after_the_approval_window_refuses_at_approval() {
    let dir = tempfile::tempdir().unwrap();
    setup_irreversible(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock.clone());

    let response = butler
        .plan(
            wf("new-rust-service-irreversible"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();
    clock.advance(ButlerConfig::DEFAULT_APPROVAL_WINDOW + Duration::from_secs(1));

    let err = butler
        .approve(response.plan_id, common::principal("approver"))
        .expect_err("the approval window elapsed");
    assert!(
        matches!(
            err,
            ButlerError::PlanExpired {
                window: willikins_server::ExpiryWindow::Approval
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_plan_approved_in_time_but_applied_after_the_apply_window_measured_from_approval() {
    let dir = tempfile::tempdir().unwrap();
    setup_irreversible(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock.clone());

    let response = butler
        .plan(
            wf("new-rust-service-irreversible"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();
    butler
        .approve(response.plan_id, common::principal("approver"))
        .unwrap();
    clock.advance(ButlerConfig::DEFAULT_APPLY_WINDOW + Duration::from_secs(1));

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("the apply window, measured from the grant, elapsed");
    assert!(
        matches!(
            err,
            ButlerError::PlanExpired {
                window: willikins_server::ExpiryWindow::Apply
            }
        ),
        "{err:?}"
    );
    assert_refused(&journal, response.plan_id, |reason| {
        matches!(reason, ApplyRefusedReason::PlanExpired)
    });
}

/// The success case: approved 23 hours after `plan` (inside the 24-hour
/// approval window) and applied 30 minutes after that (inside the
/// 60-minute apply window measured from the grant).
#[test]
fn a_plan_approved_23_hours_later_and_applied_30_minutes_after_that_runs() {
    let dir = tempfile::tempdir().unwrap();
    setup_irreversible(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock.clone());

    let response = butler
        .plan(
            wf("new-rust-service-irreversible"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();
    clock.advance(Duration::from_secs(23 * 60 * 60));
    butler
        .approve(response.plan_id, common::principal("approver"))
        .expect("23 hours is still inside the 24-hour approval window");
    clock.advance(Duration::from_secs(30 * 60));

    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect("30 minutes since the grant is still inside the 60-minute apply window");
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, RunState::Succeeded, "{run:?}");
}

/// Task 10b, part B: a plan recorded before a process restart still
/// applies. Same timeline as the test above (23 hours to approve, 30
/// minutes to apply), but over a real `FileJournal` with the `Butler`
/// dropped and rebuilt twice -- once between `plan` and `approve`, once
/// between `approve` and `apply` -- simulating a redeploy at each point a
/// human might be mid-decision, which is exactly the case
/// `crate::butler`'s "A plan's resolved inputs survive a restart" module
/// doc says this design must survive. The run succeeds, and its fake
/// provider call counters end up identical to an unrestarted run of the
/// same document and inputs, proving the rebuilt `Butler` resolved the
/// same inputs `plan` did -- not something reconstructed differently.
#[test]
fn a_plan_survives_a_butler_restart_between_plan_and_approve_and_again_before_apply() {
    // The unrestarted control run, over its own independent fake state.
    let control_dir = tempfile::tempdir().unwrap();
    setup_irreversible(control_dir.path());
    let (control_state, control_catalog) = willikins_providers_fake::empty();
    let control_clock = common::manual_clock();
    let (control_butler, _journal) =
        common::butler_with_journal(control_dir.path(), control_catalog, control_clock.clone());
    let control_response = control_butler
        .plan(
            wf("new-rust-service-irreversible"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .unwrap();
    control_clock.advance(Duration::from_secs(23 * 60 * 60));
    control_butler
        .approve(control_response.plan_id, common::principal("approver"))
        .expect("23 hours is still inside the 24-hour approval window");
    control_clock.advance(Duration::from_secs(30 * 60));
    let control_handle = control_butler
        .apply(control_response.plan_id, common::principal("agent"))
        .expect("30 minutes since the grant is still inside the 60-minute apply window");
    let control_run = common::wait_for_run(&control_butler, control_handle.run_id, 2000);
    assert_eq!(control_run.state, RunState::Succeeded, "{control_run:?}");
    let control_calls = control_state.lock().unwrap().ensure_calls.clone();
    assert!(
        !control_calls.is_empty(),
        "the positive fixture calls at least one ensure"
    );

    // The restarted run: same document, same inputs, same timeline, its
    // own independent fake state -- but a fresh `Butler` over the same
    // `FileJournal` at each of the two points a human might be
    // mid-decision when a redeploy happens.
    let workflows_dir = tempfile::tempdir().unwrap();
    setup_irreversible(workflows_dir.path());
    let journal_dir = tempfile::tempdir().unwrap();
    let journal_path = journal_dir.path().join("journal.jsonl");
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();

    let plan_id = {
        let butler = common::butler_over_file_journal(
            workflows_dir.path(),
            &journal_path,
            catalog,
            clock.clone(),
        );
        let response = butler
            .plan(
                wf("new-rust-service-irreversible"),
                &common::new_rust_service_inputs(),
                common::principal("agent"),
            )
            .unwrap();
        assert!(response.requires_approval);
        response.plan_id
        // `butler` (and its last `Arc` on the journal) drops here,
        // releasing the `FileJournal`'s exclusive lock -- simulating the
        // process exiting between `plan` and a human's decision.
    };

    clock.advance(Duration::from_secs(23 * 60 * 60));
    {
        let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
        let butler = common::butler_over_file_journal(
            workflows_dir.path(),
            &journal_path,
            catalog,
            clock.clone(),
        );
        butler
            .approve(plan_id, common::principal("approver"))
            .expect("23 hours is still inside the 24-hour approval window");
        // Dropped again -- a second redeploy between the grant and
        // `apply`.
    }

    clock.advance(Duration::from_secs(30 * 60));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let butler =
        common::butler_over_file_journal(workflows_dir.path(), &journal_path, catalog, clock);
    let handle = butler.apply(plan_id, common::principal("agent")).expect(
        "the plan's resolved inputs are rebuilt from the journal, not from an in-memory \
             map this fresh `Butler` never populated",
    );
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, RunState::Succeeded, "{run:?}");

    let restarted_calls = state.lock().unwrap().ensure_calls.clone();
    assert_eq!(
        restarted_calls, control_calls,
        "the restarted flow must have made exactly the same provider calls as the unrestarted one"
    );
}

/// Task 10b, part B: a recorded input that no longer parses against its
/// declared type refuses with `ButlerError::RecordedInputUnreadable`
/// naming the input, never a panic -- and touches no provider. Reaching
/// this in practice needs the journal itself hand-edited (the document
/// hash check, which `apply` runs first, already refuses any case where
/// the *document*'s declared types moved out from under a recorded
/// plan); this test manufactures that directly by editing the
/// `PlanRecorded` line's `inputs.slug.value` on disk between `plan` and
/// `apply`, the same "operator-level" trust the plan's task 10a addendum
/// already names for a hand-edited `fingerprint` line.
#[test]
fn a_recorded_input_that_no_longer_parses_refuses_without_a_panic() {
    let workflows_dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(
        workflows_dir.path(),
        "new-rust-service.yaml",
        "new-rust-service.yaml",
    );
    let journal_dir = tempfile::tempdir().unwrap();
    let journal_path = journal_dir.path().join("journal.jsonl");
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();

    let plan_id = {
        let butler = common::butler_over_file_journal(
            workflows_dir.path(),
            &journal_path,
            catalog,
            clock.clone(),
        );
        let response = butler
            .plan(
                wf("new-rust-service"),
                &common::new_rust_service_inputs(),
                common::principal("agent"),
            )
            .unwrap();
        assert!(
            !response.requires_approval,
            "the plain fixture is Reversible"
        );
        response.plan_id
    };

    // Corrupt the recorded `slug` input's rendered value in place: still
    // valid JSON, still a `PlanRecorded` line the journal replays
    // cleanly, but a string `ProjectSlug` rejects (a space is never
    // valid in the slug grammar).
    let original = std::fs::read_to_string(&journal_path).unwrap();
    let mut edited_any = false;
    let patched: String = original
        .lines()
        .map(|line| {
            let mut entry: serde_json::Value = serde_json::from_str(line).unwrap();
            if entry["event"]["kind"] == "plan_recorded" {
                assert_eq!(
                    entry["event"]["inputs"]["slug"]["value"],
                    serde_json::json!("third-thoughts"),
                    "{entry}"
                );
                entry["event"]["inputs"]["slug"]["value"] = serde_json::json!("BAD SLUG");
                edited_any = true;
            }
            serde_json::to_string(&entry).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert!(edited_any, "no plan_recorded line found in {original}");
    std::fs::write(&journal_path, patched).unwrap();

    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let butler =
        common::butler_over_file_journal(workflows_dir.path(), &journal_path, catalog, clock);
    let err = butler
        .apply(plan_id, common::principal("agent"))
        .expect_err("a recorded input that no longer parses must refuse, not panic");
    match err {
        ButlerError::RecordedInputUnreadable { input, .. } => {
            assert_eq!(input.to_string(), "slug");
        }
        other => panic!("expected RecordedInputUnreadable, got {other:?}"),
    }
    assert!(
        state.lock().unwrap().ensure_calls.is_empty(),
        "no provider call must have happened"
    );

    drop(butler);
    let reopened = common::open_file_journal_read_only(&journal_path);
    let found = reopened.entries().iter().any(|entry| {
        matches!(
            &entry.event,
            // Its own reason since adversarial pass 2: this used to be
            // journaled as `PlanFailed { error_kind: "Unavailable" }`,
            // naming a `PlanError` kind that does not exist.
            Event::ApplyRefused {
                plan_id: p,
                reason: ApplyRefusedReason::RecordedInputUnreadable { input },
                ..
            } if *p == plan_id && input.as_ref().is_some_and(|input| input.to_string() == "slug")
        )
    });
    assert!(found, "the refusal must be journaled as ApplyRefused");
}

// ---------------------------------------------------------------------
// UnknownPlan / AlreadyApplied / RunInProgress
// ---------------------------------------------------------------------

#[test]
fn an_unknown_plan_id_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let unknown = PlanId::new();
    let err = butler
        .apply(unknown, common::principal("agent"))
        .expect_err("an unrecorded plan id must refuse");
    assert!(matches!(err, ButlerError::UnknownPlan { plan_id } if plan_id == unknown));
    assert_refused(&journal, unknown, |reason| {
        matches!(reason, ApplyRefusedReason::UnknownPlan)
    });
}

#[test]
fn a_second_apply_of_an_already_applied_plan_is_refused() {
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
        .unwrap();
    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .unwrap();
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, RunState::Succeeded, "{run:?}");

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("a second apply of an already-applied plan must refuse");
    assert!(
        matches!(err, ButlerError::AlreadyApplied { plan_id, run_id } if plan_id == response.plan_id && run_id == handle.run_id)
    );
    assert_refused(&journal, response.plan_id, |reason| {
        matches!(reason, ApplyRefusedReason::AlreadyApplied)
    });
}

/// A second `apply` while a run is in progress refuses `RunInProgress`,
/// answered from the lock alone -- the second call happens synchronously
/// right after the first returns its `RunHandle`, before the blocking
/// tool is ever released, so there is no race to win.
#[test]
fn a_second_apply_while_a_run_is_in_progress_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("blocking-test.yaml"),
        "name: blocking-test\n\
         description: two nodes calling a test tool that blocks until released\n\
         steps:\n  \
           first:\n    \
             tool: test.blocking.ensure\n    \
             with:\n      \
               key: alpha\n  \
           second:\n    \
             tool: test.blocking.ensure\n    \
             with:\n      \
               key: beta\n",
    )
    .unwrap();

    let (blocking_tool, release) = common::BlockingTool::new();
    let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
    catalog.insert(Arc::new(blocking_tool)).unwrap();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("blocking-test"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("the blocking-test document plans cleanly");
    assert!(!response.requires_approval);

    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect("the first apply starts a run");

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("a run is already in progress");
    assert!(
        matches!(err, ButlerError::RunInProgress { run_id } if run_id == handle.run_id),
        "{err:?}"
    );

    // Release both blocked `ensure` calls so the run thread finishes and
    // the test's own tempdir/state can be dropped cleanly.
    release.send(()).unwrap();
    release.send(()).unwrap();
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, RunState::Succeeded, "{run:?}");

    // The lock cleared once the run finished: a third apply is now
    // refused as `AlreadyApplied`, not `RunInProgress`.
    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("the plan has already run");
    assert!(matches!(err, ButlerError::AlreadyApplied { .. }), "{err:?}");
}
