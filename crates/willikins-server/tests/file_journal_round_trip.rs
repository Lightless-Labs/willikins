//! The whole approve-then-apply flow, driven through `Butler` over a real
//! `FileJournal` in a tempdir, then reopened independently (not through
//! `Butler`) and compared for equal views.

mod common;

use std::sync::{Arc, Mutex};

use willikins_journal::{Clock, FileJournal, Journal};
use willikins_server::{Butler, ButlerConfig, SharedJournal};
use willikins_types::{DomainType, WorkflowName};

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

/// `FileJournal::open` retried for up to a second: the successful run's
/// background thread has already appended `RunFinished` (what
/// `wait_for_run` observed) but may not yet have unwound and dropped its
/// own `Arc` clone of the journal handle -- the last strong reference,
/// whichever holder drops it, is what releases the exclusive `flock` an
/// immediate re-open would otherwise race.
fn open_once_unlocked(path: &std::path::Path) -> FileJournal {
    for _ in 0..200 {
        match FileJournal::open(path) {
            Ok(journal) => return journal,
            Err(willikins_journal::JournalError::Locked { .. }) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(other) => panic!("the file replays cleanly: {other}"),
        }
    }
    panic!(
        "journal at {} stayed locked for a full second",
        path.display()
    );
}

#[test]
fn approve_then_apply_over_a_file_journal_reopens_to_equal_views() {
    let workflows_dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(
        workflows_dir.path(),
        "irreversible.yaml",
        "new-rust-service-irreversible.yaml",
    );
    let journal_dir = tempfile::tempdir().unwrap();
    let journal_path = journal_dir.path().join("journal.jsonl");

    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();

    let (plan_id, run_id, original_plan_json, original_run_json) = {
        let journal: SharedJournal = Arc::new(Mutex::new(
            FileJournal::open_with_clock(&journal_path, clock.clone() as Arc<dyn Clock>)
                .expect("journal opens"),
        ));
        let butler = Butler::new(ButlerConfig {
            workflows_dir: workflows_dir.path().to_path_buf(),
            journal: journal.clone(),
            catalog,
            clock: clock as Arc<dyn Clock>,
            approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
            apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        });

        let response = butler
            .plan(
                wf("new-rust-service-irreversible"),
                &common::new_rust_service_inputs(),
                common::principal("agent"),
            )
            .expect("plans cleanly");
        assert!(response.requires_approval);

        butler
            .approve(response.plan_id, common::principal("approver"))
            .expect("approval succeeds");

        let handle = butler
            .apply(response.plan_id, common::principal("agent"))
            .expect("an approved plan applies");
        let run = common::wait_for_run(&butler, handle.run_id, 2000);
        assert_eq!(run.state, willikins_journal::RunState::Succeeded, "{run:?}");

        let plan_json = serde_json::to_value(
            journal
                .lock()
                .unwrap()
                .plan(&response.plan_id)
                .expect("the plan is recorded"),
        )
        .unwrap();
        let run_json = serde_json::to_value(
            journal
                .lock()
                .unwrap()
                .run(&handle.run_id)
                .expect("the run is recorded"),
        )
        .unwrap();

        // Drop every strong reference to the journal so its exclusive
        // lock releases before the next `FileJournal::open` below.
        drop(butler);
        drop(journal);

        (response.plan_id, handle.run_id, plan_json, run_json)
    };

    let reopened = open_once_unlocked(&journal_path);
    let reopened_plan_json =
        serde_json::to_value(reopened.plan(&plan_id).expect("the plan replays")).unwrap();
    let reopened_run_json =
        serde_json::to_value(reopened.run(&run_id).expect("the run replays")).unwrap();

    assert_eq!(original_plan_json, reopened_plan_json);
    assert_eq!(original_run_json, reopened_run_json);
}
