//! The 2026-09-16 smoke run's second finding, pinned at its call site.
//!
//! `ButlerError::Plan` is raised from two places — `Butler::plan`'s own
//! first plan of a workflow and `Butler::apply`'s re-plan of an approved
//! one — and its `Display` said "re-planning failed" for both, so the
//! smoke run's *initial* plan failure sent its reader looking for an
//! apply that had never started. `willikins_server`'s own unit test
//! (`error.rs`'s `plan_error_message_distinguishes_initial_planning_from_a_replan`)
//! pins that the two `PlanAttempt`s render differently, but it builds the
//! variants by hand: nothing pinned that `Butler::plan` passes
//! `PlanAttempt::Initial`, which is the half that was actually wrong.
//! This drives the real `Butler` into a real plan failure instead, so
//! swapping the attempt at the call site fails a test.

mod common;

use std::sync::{Arc, Mutex};

use willikins_providers_fake::FakeState;
use willikins_server::ButlerError;
use willikins_types::{DomainType, WorkflowName};

/// The positive fixture planned against a catalog whose GitHub
/// repository is already taken by someone else (`repo-foreign.json`):
/// `github.repo.ensure` reads `Foreign`, `willikins_core::plan` refuses
/// with `PlanError::NameTaken`, and the message must name *planning* —
/// this is the first plan, and no apply has been asked for at all.
#[test]
fn an_initial_plan_failure_says_planning_rather_than_re_planning() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let seed = std::fs::read_to_string(
        common::workspace_root()
            .join("workflows")
            .join("fixtures")
            .join("state")
            .join("repo-foreign.json"),
    )
    .unwrap();
    let state = Arc::new(Mutex::new(FakeState::from_json(&seed).unwrap()));
    let catalog = willikins_providers_fake::catalog(state);
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let err = butler
        .plan(
            WorkflowName::parse("new-rust-service").unwrap(),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect_err("a foreign repository must not plan");

    assert!(matches!(err, ButlerError::Plan { .. }), "{err:?}");
    let message = err.to_string();
    assert!(
        message.starts_with("planning failed:"),
        "an initial plan failure must not claim a re-plan: {message}"
    );
    assert!(!message.contains("re-planning"), "{message}");
}
