//! Acceptance test 10 (`docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`,
//! "Acceptance tests" section, item 10): a `FileJournal` records a run of
//! the positive fixture (with `next_token` seeded to a distinctive
//! marker) and a run of `workflows/fixtures/secret-get.yaml` (with a
//! seeded Doppler secret value), and:
//!
//! - the JSONL file contains the redaction markers and none of the
//!   seeded bytes, in every line;
//! - reopening the file replays to views (`pending_approvals`, `plan`,
//!   `runs`, `run`) equal to the live ones;
//! - `seq` is contiguous across both runs' worth of events;
//! - a second `FileJournal::open` on the same path fails while the first
//!   is held, and a line appended by hand with a lower `seq` makes `open`
//!   fail naming it -- both pinned in isolation, more thoroughly, by
//!   `src/file.rs`'s own unit tests (`a_second_open_on_the_same_path_is_refused_while_the_first_is_held`,
//!   `a_hand_appended_lower_seq_makes_open_fail_naming_the_line`,
//!   `a_hand_truncated_last_line_is_refused_naming_it`); this test only
//!   reconfirms the lock half against the exact file this scenario
//!   produces, since that is the file a real deployment would actually
//!   see contended.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::describe::{PartialInputs, RawInput};
use willikins_core::{Approval, InputName, Plan, Value, Workflow, apply, check, plan};
use willikins_journal::{Event, FileJournal, Journal, PlanId, Reason, Redacted, run_and_journal};
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

fn positive_fixture() -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("new-rust-service.yaml")
}

fn load(path: &Path) -> Workflow {
    willikins_dsl::load_document(path)
        .unwrap_or_else(|err| panic!("{}: failed to load: {err}", path.display()))
}

fn resolve_inputs(
    checked: &willikins_core::Checked,
    overrides: &[(&str, RawInput)],
) -> IndexMap<InputName, Value> {
    let mut partial = PartialInputs::new();
    for (name, raw) in overrides {
        partial.insert(InputName::parse(name).unwrap(), raw.clone());
    }
    let description = willikins_core::describe(checked, &partial);
    assert!(description.errors.is_empty(), "{:?}", description.errors);
    assert!(description.missing.is_empty(), "{:?}", description.missing);
    description.resolved
}

fn distinctive_token() -> willikins_types::DopplerServiceToken {
    willikins_types::DopplerServiceToken::parse(&format!("dp.st.prd.{}", "MARKER".repeat(7)))
        .unwrap()
}

const TOKEN_MARKER_BYTES: &str = "MARKERMARKERMARKERMARKERMARKERMARKERMARKER";
const SECRET_SEEDED_BYTES: &str = "acceptance-test-8b-fake-secret-bytes-do-not-leak";

/// Plan and record `workflow` against `catalog`/`inputs`, journaling
/// `PlanRecorded`, and return the fresh id and [`Plan`].
fn record_plan(
    journal: &mut FileJournal,
    checked: &willikins_core::Checked,
    inputs: &IndexMap<InputName, Value>,
    catalog: &willikins_core::Catalog,
) -> (PlanId, Plan) {
    let approved = plan(checked, inputs, catalog).expect("plan must succeed");
    let plan_id = PlanId::new();
    journal
        .append(Event::PlanRecorded {
            plan_id,
            workflow: checked.workflow.name.clone(),
            document_sha256: "test-sha256".to_string(),
            inputs: Redacted::from(inputs),
            plan: Redacted::from(&approved),
            fingerprint: approved.fingerprint(),
            class: approved.class,
            requires_approval: approved.requires_approval,
        })
        .expect("PlanRecorded must append");
    // Both fixtures this test drives are `Class::Reversible`
    // (`requires_approval == false`); a plan requiring approval would
    // instead wait for `Event::ApprovalGranted` from a human, which is
    // `willikins-server`'s job (task 7), not exercised here.
    assert!(!approved.requires_approval);
    journal
        .append(Event::ApprovalAutomatic {
            plan_id,
            class: approved.class,
        })
        .expect("ApprovalAutomatic must append");
    (plan_id, approved)
}

#[test]
#[allow(clippy::too_many_lines)] // one scenario walked start to end, matching the milestone plan's acceptance test 10 checklist item by item; splitting it would scatter the sequence
fn acceptance_10_journal_records_two_runs_without_leaking_either_secret() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let mut journal = FileJournal::open(&journal_path).expect("journal must open");

    // --- Run 1: the positive fixture, `next_token` seeded to a marker. ---
    let workflow = load(&positive_fixture());
    let state = Arc::new(Mutex::new(
        FakeState::new().with_next_token(distinctive_token()),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = check(&workflow, &catalog).expect("positive fixture must check cleanly");
    let inputs = resolve_inputs(
        &checked,
        &[
            ("slug", RawInput::Scalar("third-thoughts".to_string())),
            ("org", RawInput::Scalar("lightless-labs".to_string())),
        ],
    );
    let (plan_id_1, approved_1) = record_plan(&mut journal, &checked, &inputs, &catalog);

    let (result_1, journal_error_1) = run_and_journal(
        &mut journal,
        willikins_journal::PrincipalId::parse("agent").unwrap(),
        plan_id_1,
        |observer| {
            apply(
                &checked,
                &inputs,
                &catalog,
                &approved_1,
                &Approval::Auto,
                observer,
            )
        },
    );
    assert!(journal_error_1.is_none(), "{journal_error_1:?}");
    result_1.expect("run 1 must succeed");

    // --- Run 2: secret-get.yaml, a Doppler secret seeded in fake state. ---
    let seeded_json = std::fs::read_to_string(
        workspace_root()
            .join("workflows")
            .join("fixtures")
            .join("state")
            .join("secret-seeded.json"),
    )
    .unwrap();
    assert!(seeded_json.contains(SECRET_SEEDED_BYTES));
    let seeded_state = Arc::new(Mutex::new(FakeState::from_json(&seeded_json).unwrap()));
    let seeded_catalog = willikins_providers_fake::catalog(Arc::clone(&seeded_state));
    let secret_workflow = load(&fixture("secret-get.yaml"));
    let secret_checked =
        check(&secret_workflow, &seeded_catalog).expect("secret-get.yaml must check cleanly");
    let secret_inputs = resolve_inputs(
        &secret_checked,
        &[("project", RawInput::Scalar("widgets".to_string()))],
    );
    let (plan_id_2, approved_2) = record_plan(
        &mut journal,
        &secret_checked,
        &secret_inputs,
        &seeded_catalog,
    );

    let (result_2, journal_error_2) = run_and_journal(
        &mut journal,
        willikins_journal::PrincipalId::parse("agent").unwrap(),
        plan_id_2,
        |observer| {
            apply(
                &secret_checked,
                &secret_inputs,
                &seeded_catalog,
                &approved_2,
                &Approval::Auto,
                observer,
            )
        },
    );
    assert!(journal_error_2.is_none(), "{journal_error_2:?}");
    result_2.expect("run 2 must succeed");

    // --- The file itself: markers present, seeded bytes absent, everywhere. ---
    let contents = std::fs::read_to_string(&journal_path).unwrap();
    assert!(
        !contents.contains(TOKEN_MARKER_BYTES),
        "journal leaked the minted token: {contents}"
    );
    assert!(
        !contents.contains(SECRET_SEEDED_BYTES),
        "journal leaked the seeded Doppler secret: {contents}"
    );
    assert!(
        contents.contains("REDACTED"),
        "journal never shows a redaction marker at all: {contents}"
    );

    // --- seq is contiguous across both runs. ---
    let seqs: Vec<u64> = journal.entries().iter().map(|entry| entry.seq).collect();
    let expected: Vec<u64> = (1..=journal.entries().len() as u64).collect();
    assert_eq!(seqs, expected);

    // --- The live views, before reopening. ---
    let live_plans = journal.pending_approvals();
    assert!(
        live_plans.is_empty(),
        "both plans were auto-approved and applied: {live_plans:?}"
    );
    let live_plan_1 = journal.plan(&plan_id_1).expect("plan 1 must be recorded");
    let live_plan_2 = journal.plan(&plan_id_2).expect("plan 2 must be recorded");
    assert!(live_plan_1.applied.is_some());
    assert!(live_plan_2.applied.is_some());
    let live_runs = journal.runs();
    assert_eq!(live_runs.len(), 2);
    for run in &live_runs {
        assert_eq!(run.state, willikins_journal::RunState::Succeeded);
    }

    drop(journal);

    // --- Reopening replays to equal views. ---
    let reopened = FileJournal::open(&journal_path).expect("reopen must succeed");
    assert_eq!(reopened.entries().len(), seqs.len());
    let reopened_plan_1 = reopened.plan(&plan_id_1).expect("plan 1 must replay");
    let reopened_plan_2 = reopened.plan(&plan_id_2).expect("plan 2 must replay");
    assert_eq!(
        serde_json::to_value(&live_plan_1).unwrap(),
        serde_json::to_value(&reopened_plan_1).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&live_plan_2).unwrap(),
        serde_json::to_value(&reopened_plan_2).unwrap()
    );
    let reopened_runs = reopened.runs();
    assert_eq!(
        serde_json::to_value(&live_runs).unwrap(),
        serde_json::to_value(&reopened_runs).unwrap()
    );

    // --- A second open while the (reopened) journal is held is refused. ---
    let second = FileJournal::open(&journal_path);
    assert!(matches!(
        second,
        Err(willikins_journal::JournalError::Locked { .. })
    ));

    drop(reopened);

    // --- A hand-appended lower `seq` makes a fresh open fail, naming the line. ---
    let bogus = willikins_journal::Entry {
        seq: 1,
        at: willikins_journal::Timestamp::now(),
        event: Event::ApprovalRejected {
            plan_id: PlanId::new(),
            approver: willikins_journal::PrincipalId::parse("x").unwrap(),
            reason: Reason::parse("bogus").unwrap(),
        },
    };
    let mut line = serde_json::to_string(&bogus).unwrap();
    line.push('\n');
    {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&journal_path)
            .unwrap();
        file.write_all(line.as_bytes()).unwrap();
    }
    let corrupted = FileJournal::open(&journal_path);
    assert!(matches!(
        corrupted,
        Err(willikins_journal::JournalError::Corrupt { .. })
    ));
}
