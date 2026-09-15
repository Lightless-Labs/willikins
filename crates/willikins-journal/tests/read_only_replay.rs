//! [`willikins_journal::replay`]: a read-only, lock-free snapshot of a
//! journal file, for a caller that must not (or cannot) contend for the
//! exclusive lock a live [`FileJournal`] holds on the same path -- the
//! CLI's `runs`/`run` commands (task 11) reading a running server's
//! journal.
//!
//! The claims under test, matching the task's pins: replay succeeds
//! alongside a live `FileJournal` on the same path and sees every entry
//! appended before the replay started; replay of a malformed, truncated,
//! or reordered file fails with the exact [`JournalError`] the exclusive
//! `FileJournal::open` gives for the same bytes; replay never creates or
//! otherwise modifies the file.

mod common;

use std::io::Write as _;

use willikins_journal::{Entry, Event, FileJournal, Journal, JournalError, Timestamp};

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn server_started(version: &str) -> Event {
    Event::ServerStarted {
        version: version.to_string(),
        workflows_dir: "/workflows".to_string(),
        workflow_hashes: std::collections::BTreeMap::new(),
    }
}

#[test]
fn replay_succeeds_while_a_file_journal_holds_the_lock_on_the_same_path() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");

    // Held for the whole test: this is exactly the scenario a plain
    // second `FileJournal::open` cannot survive (see `tests/locking.rs`).
    let mut journal = FileJournal::open(&path).unwrap();
    journal.append(server_started("a")).unwrap();
    journal.append(server_started("b")).unwrap();

    let replayed = willikins_journal::replay(&path).expect("replay must not be locked out");
    assert_eq!(replayed.entries().len(), 2);
    assert_eq!(
        serde_json::to_value(replayed.entries()).unwrap(),
        serde_json::to_value(journal.entries()).unwrap()
    );

    // The `FileJournal` is still alive and usable: replay took nothing
    // from it.
    journal.append(server_started("c")).unwrap();
    assert_eq!(journal.entries().len(), 3);
}

#[test]
fn replay_sees_every_entry_appended_before_it_started_and_none_after() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut journal = FileJournal::open(&path).unwrap();
    journal.append(server_started("first")).unwrap();

    let replayed = willikins_journal::replay(&path).unwrap();
    assert_eq!(replayed.entries().len(), 1);

    // Appended after the snapshot: not reflected in `replayed`, which
    // read the file once at construction and never touches it again.
    journal.append(server_started("second")).unwrap();
    assert_eq!(replayed.entries().len(), 1);
}

#[test]
fn replay_of_a_malformed_file_fails_with_the_same_error_the_exclusive_open_gives() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    std::fs::write(&path, "not json at all\n").unwrap();

    let via_open = FileJournal::open(&path);
    // Re-create the file: `FileJournal::open` never got to write to it
    // (it failed during replay, before any append), but be explicit
    // rather than relying on that.
    let exclusive_err = match via_open {
        Err(JournalError::Corrupt { line, reason }) => (line, reason),
        other => panic!("expected Corrupt from FileJournal::open, got {other:?}"),
    };

    let via_replay = willikins_journal::replay(&path);
    let replay_err = match via_replay {
        Err(JournalError::Corrupt { line, reason }) => (line, reason),
        other => panic!("expected Corrupt from replay, got {other:?}"),
    };

    assert_eq!(exclusive_err, replay_err);
}

#[test]
fn replay_of_a_truncated_file_fails_the_same_way_as_the_exclusive_open() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    {
        let mut journal = FileJournal::open(&path).unwrap();
        journal.append(server_started("a")).unwrap();
    }
    // Append a second, deliberately incomplete line.
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(br#"{"seq":2,"at":"2026-09-13T00:00:00+00:00","event":{"kind":"#)
            .unwrap();
    }

    let exclusive_err = match FileJournal::open(&path) {
        Err(JournalError::Corrupt { line, reason }) => (line, reason),
        other => panic!("expected Corrupt from FileJournal::open, got {other:?}"),
    };
    let replay_err = match willikins_journal::replay(&path) {
        Err(JournalError::Corrupt { line, reason }) => (line, reason),
        other => panic!("expected Corrupt from replay, got {other:?}"),
    };
    assert_eq!(exclusive_err, replay_err);
    assert!(exclusive_err.1.contains("truncated"));
}

#[test]
fn replay_of_a_reordered_file_fails_the_same_way_as_the_exclusive_open() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let first = Entry {
        seq: 1,
        at: Timestamp::parse("2026-09-13T12:00:00+00:00").unwrap(),
        event: server_started("a"),
    };
    let second = Entry {
        seq: 2,
        // Earlier than `first.at`: reordered.
        at: Timestamp::parse("2026-09-13T11:00:00+00:00").unwrap(),
        event: server_started("b"),
    };
    let mut contents = serde_json::to_string(&first).unwrap();
    contents.push('\n');
    contents.push_str(&serde_json::to_string(&second).unwrap());
    contents.push('\n');
    std::fs::write(&path, contents).unwrap();

    let exclusive_err = match FileJournal::open(&path) {
        Err(JournalError::Corrupt { line, reason }) => (line, reason),
        other => panic!("expected Corrupt from FileJournal::open, got {other:?}"),
    };
    let replay_err = match willikins_journal::replay(&path) {
        Err(JournalError::Corrupt { line, reason }) => (line, reason),
        other => panic!("expected Corrupt from replay, got {other:?}"),
    };
    assert_eq!(exclusive_err, replay_err);
}

#[test]
fn replay_never_creates_the_file() {
    let dir = temp_dir();
    let path = dir.path().join("never-created.jsonl");
    assert!(!path.exists());

    let result = willikins_journal::replay(&path);
    assert!(matches!(result, Err(JournalError::Io { .. })), "{result:?}");
    assert!(
        !path.exists(),
        "replay must never create the file it was asked to read"
    );
}

#[test]
fn replay_never_modifies_an_existing_file() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    {
        let mut journal = FileJournal::open(&path).unwrap();
        journal.append(server_started("a")).unwrap();
        journal.append(server_started("b")).unwrap();
    }
    let before = std::fs::read(&path).unwrap();

    let replayed = willikins_journal::replay(&path).unwrap();
    assert_eq!(replayed.entries().len(), 2);

    let after = std::fs::read(&path).unwrap();
    assert_eq!(before, after, "replay must never touch the file's bytes");
}

#[test]
fn replay_exposes_the_same_read_views_as_a_file_journal() {
    use indexmap::IndexMap;
    use willikins_core::{Class, InstanceFingerprint, Value};
    use willikins_journal::{PlanId, Redacted};

    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let plan_id = PlanId::new();
    let plan = willikins_core::Plan {
        workflow: common::workflow_name("wf"),
        nodes: Vec::new(),
        outputs: IndexMap::new(),
        class: Class::Reversible,
        requires_approval: true,
    };
    let fingerprint = vec![InstanceFingerprint {
        name: common::node("n"),
        instance: None,
        action: willikins_core::Action::Create,
        outputs: Vec::new(),
    }];
    {
        let mut journal = FileJournal::open(&path).unwrap();
        journal
            .append(Event::PlanRecorded {
                plan_id,
                workflow: common::workflow_name("wf"),
                document_sha256: common::document_sha256("sha"),
                inputs: Redacted::from(&IndexMap::<willikins_core::InputName, Value>::new()),
                plan: Redacted::from(&plan),
                fingerprint,
                class: plan.class,
                requires_approval: plan.requires_approval,
                principal: None,
            })
            .unwrap();
    }

    let replayed = willikins_journal::replay(&path).unwrap();
    assert_eq!(replayed.pending_approvals().len(), 1);
    let fetched = replayed.plan(&plan_id).expect("plan is recorded");
    assert_eq!(fetched.plan_id, plan_id);
    assert!(replayed.runs().is_empty());
    assert!(replayed.run(&willikins_journal::RunId::new()).is_none());
}
