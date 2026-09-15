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
use willikins_types::DomainType;

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
            document_sha256: common::document_sha256("test-sha256"),
            inputs: Redacted::from(&inputs),
            plan: Redacted::from(&approved),
            fingerprint: approved.fingerprint(),
            class: approved.class,
            requires_approval: approved.requires_approval,
            principal: None,
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

/// The same defect class, one variant further along: `apply`'s rule 2
/// fails a fresh re-plan (`ApplyError::Plan`) *before* minting a
/// `SinkToken` and before touching any provider -- core's own `ApplyError`
/// doc groups it with `ApprovalRequired` and `Drift` as the three refusals
/// that carry no partial result. Journaling `RunStarted` for it would set
/// `PlanRecord::applied` and permanently refuse every later `apply` of
/// that plan as `AlreadyApplied`, even though the cause (a provider that
/// could not be read at that moment, a `for_each` source that momentarily
/// resolved to nothing) is transient and the plan is still perfectly
/// applicable. The run closure returns the error directly rather than
/// arranging a real re-plan failure: `run_and_journal`'s own routing is
/// what is under test, and a synthetic error pins it at every variant
/// without a fixture per cause.
#[test]
fn a_replan_failure_refuses_without_journaling_a_run() {
    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    journal
        .append(Event::PlanRecorded {
            plan_id,
            workflow: willikins_types::WorkflowName::parse("wf").unwrap(),
            document_sha256: common::document_sha256("test-sha256"),
            inputs: Redacted::from(&indexmap::IndexMap::new()),
            plan: Redacted::from(&willikins_core::Plan {
                workflow: willikins_types::WorkflowName::parse("wf").unwrap(),
                nodes: Vec::new(),
                outputs: indexmap::IndexMap::new(),
                class: willikins_core::Class::Reversible,
                requires_approval: false,
            }),
            fingerprint: Vec::new(),
            class: willikins_core::Class::Reversible,
            requires_approval: false,
            principal: None,
        })
        .unwrap();

    let (result, _run_id, journal_error) = run_and_journal(
        &mut journal,
        willikins_journal::PrincipalId::parse("agent").unwrap(),
        plan_id,
        |_observer| {
            Err(willikins_core::ApplyError::Plan {
                error: willikins_core::PlanError::MissingInput {
                    input: InputName::parse("slug").unwrap(),
                },
            })
        },
    );
    assert!(journal_error.is_none(), "{journal_error:?}");
    assert!(matches!(
        result,
        Err(willikins_core::ApplyError::Plan { .. })
    ));

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

    // The refusal names which kind of planning failure it was, and nothing
    // else of the error: `PlanError`'s sibling fields can hold values.
    match &journal.entries()[1].event {
        Event::ApplyRefused { reason, .. } => assert_eq!(
            reason,
            &willikins_journal::ApplyRefusedReason::PlanFailed {
                error_kind: "MissingInput".to_string(),
            }
        ),
        other => panic!("expected ApplyRefused, got {other:?}"),
    }

    let record = journal.plan(&plan_id).expect("plan must be recorded");
    assert!(
        record.applied.is_none(),
        "a re-plan failure must leave the plan applicable: {record:?}"
    );
    assert!(journal.runs().is_empty(), "{:?}", journal.runs());
}

/// The same `PlanFailed` refusal, over a real file: journal follow-ups
/// (`todos/2026-09-14-journal-follow-ups.md`) note this event only had a
/// `MemoryJournal` round-trip pin, never one through `FileJournal`'s own
/// serialize-then-replay path.
#[test]
fn an_apply_refused_plan_failed_event_round_trips_through_a_file_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let plan_id = PlanId::new();
    {
        let mut journal = willikins_journal::FileJournal::open(&path).unwrap();
        journal
            .append(Event::PlanRecorded {
                plan_id,
                workflow: willikins_types::WorkflowName::parse("wf").unwrap(),
                document_sha256: common::document_sha256("test-sha256"),
                inputs: Redacted::from(&indexmap::IndexMap::new()),
                plan: Redacted::from(&willikins_core::Plan {
                    workflow: willikins_types::WorkflowName::parse("wf").unwrap(),
                    nodes: Vec::new(),
                    outputs: indexmap::IndexMap::new(),
                    class: willikins_core::Class::Reversible,
                    requires_approval: false,
                }),
                fingerprint: Vec::new(),
                class: willikins_core::Class::Reversible,
                requires_approval: false,
                principal: None,
            })
            .unwrap();
        let (result, _run_id, journal_error) = run_and_journal(
            &mut journal,
            willikins_journal::PrincipalId::parse("agent").unwrap(),
            plan_id,
            |_observer| {
                Err(willikins_core::ApplyError::Plan {
                    error: willikins_core::PlanError::MissingInput {
                        input: InputName::parse("slug").unwrap(),
                    },
                })
            },
        );
        assert!(journal_error.is_none(), "{journal_error:?}");
        assert!(matches!(
            result,
            Err(willikins_core::ApplyError::Plan { .. })
        ));
    }

    let reopened = willikins_journal::FileJournal::open(&path).expect("the file replays cleanly");
    match &reopened.entries()[1].event {
        Event::ApplyRefused { reason, .. } => assert_eq!(
            reason,
            &willikins_journal::ApplyRefusedReason::PlanFailed {
                error_kind: "MissingInput".to_string(),
            }
        ),
        other => panic!("expected ApplyRefused, got {other:?}"),
    }
    let record = reopened.plan(&plan_id).expect("plan must replay");
    assert!(record.applied.is_none());
    assert!(reopened.runs().is_empty());
}
