//! Pins the fix for a defect found by `advisor` review of this task: a
//! plan `apply` refuses before touching any provider
//! (`ApplyError::ApprovalRequired`, `ApplyError::Drift`) must never
//! journal `RunStarted` -- doing so would set
//! [`willikins_journal::journal::PlanRecord::applied`] even though
//! nothing ran, permanently blocking a later, legitimate `apply` of the
//! same plan with `AlreadyApplied` once the real blocker (an approval,
//! drifted state) is resolved. Acceptance test 7's own wording is exact
//! about this: "the journal has `ApplyRefused` and no `RunStarted`."
//!
//! Uses `workflows/fixtures/irreversible.yaml` against empty fake state
//! with `Approval::Auto`, the same scenario the CLI acceptance suite's
//! `milestone_2_acceptance_07_*` drives directly against `apply` --
//! this test drives it through [`run_and_journal`] instead, which is
//! the part that decides what gets journaled.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use willikins_core::describe::{PartialInputs, RawInput};
use willikins_core::{Approval, InputName, Workflow, apply, check, plan};
use willikins_journal::{Event, Journal, MemoryJournal, PlanId, Redacted, run_and_journal};
use willikins_providers_fake::FakeState;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join(name)
}

fn load(path: &Path) -> Workflow {
    willikins_dsl::load_document(path)
        .unwrap_or_else(|err| panic!("{}: failed to load: {err}", path.display()))
}

#[test]
fn approval_required_refuses_without_journaling_run_started() {
    let workflow = load(&fixture("irreversible.yaml"));
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = check(&workflow, &catalog).expect("irreversible.yaml must check cleanly");

    let mut partial = PartialInputs::new();
    partial.insert(
        InputName::parse("slug").unwrap(),
        RawInput::Scalar("third-thoughts".to_string()),
    );
    partial.insert(
        InputName::parse("org").unwrap(),
        RawInput::Scalar("lightless-labs".to_string()),
    );
    let description = willikins_core::describe(&checked, &partial);
    assert!(description.errors.is_empty());
    assert!(description.missing.is_empty());
    let inputs = description.resolved;

    let approved = plan(&checked, &inputs, &catalog).expect("plan against empty state succeeds");
    assert!(
        approved.requires_approval,
        "irreversible.yaml must require approval"
    );

    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    journal
        .append(Event::PlanRecorded {
            plan_id,
            workflow: checked.workflow.name.clone(),
            document_sha256: "test-sha256".to_string(),
            inputs: Redacted::from(&inputs),
            plan: Redacted::from(&approved),
            fingerprint: approved.fingerprint(),
            class: approved.class,
            requires_approval: approved.requires_approval,
        })
        .unwrap();

    let principal = willikins_journal::PrincipalId::parse("agent").unwrap();
    let (result, _run_id, journal_error) =
        run_and_journal(&mut journal, principal, plan_id, |observer| {
            apply(
                &checked,
                &inputs,
                &catalog,
                &approved,
                &Approval::Auto,
                observer,
            )
        });
    assert!(journal_error.is_none(), "{journal_error:?}");
    assert!(
        matches!(
            result,
            Err(willikins_core::ApplyError::ApprovalRequired { .. })
        ),
        "expected ApprovalRequired, got {result:?}"
    );

    // Exactly PlanRecorded then ApplyRefused: no RunStarted, no RunFinished.
    let kinds: Vec<&'static str> = journal
        .entries()
        .iter()
        .map(|entry| match &entry.event {
            Event::PlanRecorded { .. } => "plan_recorded",
            Event::ApplyRefused { .. } => "apply_refused",
            Event::RunStarted { .. } => "run_started",
            Event::RunFinished { .. } => "run_finished",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["plan_recorded", "apply_refused"]);

    let refused_reason = journal
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::ApplyRefused { reason, .. } => Some(reason.clone()),
            _ => None,
        });
    assert!(matches!(
        refused_reason,
        Some(willikins_journal::ApplyRefusedReason::ApprovalRequired)
    ));

    // The plan is not marked applied: a later, properly-approved `apply`
    // of this same plan must still be possible.
    let record = journal.plan(&plan_id).expect("plan must be recorded");
    assert!(
        record.applied.is_none(),
        "a refused apply must not mark the plan applied: {record:?}"
    );
    assert!(
        journal.runs().is_empty(),
        "no run was ever started: {:?}",
        journal.runs()
    );

    let locked = state.lock().unwrap();
    assert!(
        locked.github_repos.is_empty() && locked.doppler_projects.is_empty(),
        "no provider call was made"
    );
}
