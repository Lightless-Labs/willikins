//! What [`FileJournal::open`]'s replay accepts and what it refuses, one
//! hostile or damaged file per test.
//!
//! The claim under test is the one an auditor actually relies on: a
//! journal file that replays at all replays *exactly* what was appended,
//! in order, and anything else is refused loudly with the offending line
//! named. Every case here is fail-closed on purpose -- a journal that
//! skipped a line it could not understand would replay as if nothing were
//! wrong, which for an audit record is worse than refusing to open.
//!
//! What replay does **not** detect is recorded in this file's own
//! `a_hand_edited_payload_on_an_earlier_line_is_not_detected` test, which
//! pins the gap rather than hiding it.

use std::path::{Path, PathBuf};

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

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

/// Write `lines` verbatim as the whole file (each already newline-free).
fn write_lines(path: &Path, lines: &[String]) {
    let mut contents = String::new();
    for line in lines {
        contents.push_str(line);
        contents.push('\n');
    }
    std::fs::write(path, contents).unwrap();
}

fn line_of(seq: u64, at_text: &str, version: &str) -> String {
    serde_json::to_string(&Entry {
        seq,
        at: at(at_text),
        event: server_started(version),
    })
    .unwrap()
}

fn expect_corrupt(result: Result<FileJournal, JournalError>) -> (usize, String) {
    match result {
        Err(JournalError::Corrupt { line, reason }) => (line, reason),
        Err(other) => panic!("expected Corrupt, got {other:?}"),
        Ok(_) => panic!("expected Corrupt, the journal opened"),
    }
}

#[test]
fn a_duplicated_seq_is_refused_naming_the_second_line() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    write_lines(
        &path,
        &[
            line_of(1, "2026-09-13T12:00:00+00:00", "a"),
            line_of(1, "2026-09-13T12:00:01+00:00", "b"),
        ],
    );
    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 2);
    assert!(reason.contains("expected seq 2"), "{reason}");
}

#[test]
fn a_skipped_seq_is_refused_naming_the_line_that_skipped() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    write_lines(
        &path,
        &[
            line_of(1, "2026-09-13T12:00:00+00:00", "a"),
            line_of(3, "2026-09-13T12:00:01+00:00", "c"),
        ],
    );
    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 2);
    assert!(reason.contains("expected seq 2"), "{reason}");
}

/// Two entries stamped the same instant are legitimate: `append` clamps
/// its own clock read up to the previous entry's stamp rather than
/// refusing to record anything, so equal timestamps are exactly what a
/// backwards clock step produces. Only a *earlier* stamp is a refusal
/// (`src/file.rs`'s own
/// `a_non_monotonic_timestamp_makes_open_fail_naming_the_line`).
#[test]
fn two_entries_with_the_same_timestamp_replay_fine() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    write_lines(
        &path,
        &[
            line_of(1, "2026-09-13T12:00:00+00:00", "a"),
            line_of(2, "2026-09-13T12:00:00+00:00", "b"),
        ],
    );
    let journal = FileJournal::open(&path).expect("equal timestamps must be accepted");
    assert_eq!(journal.entries().len(), 2);
}

#[test]
fn a_pre_existing_empty_file_replays_as_an_empty_journal() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    std::fs::write(&path, "").unwrap();
    let journal = FileJournal::open(&path).expect("an empty file is an empty journal");
    assert!(journal.entries().is_empty());
}

#[test]
fn a_whitespace_only_file_is_refused() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    std::fs::write(&path, "   \n").unwrap();
    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 1);
    assert!(reason.contains("invalid JSON"), "{reason}");
}

#[test]
fn a_blank_line_between_entries_is_refused() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut contents = line_of(1, "2026-09-13T12:00:00+00:00", "a");
    contents.push('\n');
    contents.push('\n');
    contents.push_str(&line_of(2, "2026-09-13T12:00:01+00:00", "b"));
    contents.push('\n');
    std::fs::write(&path, contents).unwrap();
    let (line, _) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 2, "the blank line itself is the offending line");
}

/// A UTF-8 BOM is not whitespace to a JSON parser, and the DSL's own
/// pre-scan already refuses one on a workflow document (see
/// `docs/HANDOFF.md`'s "Architecture Gotchas"). A journal file is not
/// hand-authored at all, so a BOM can only mean something other than
/// `append` wrote the file -- refused, naming line 1.
#[test]
fn a_utf8_bom_is_refused_naming_the_first_line() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut contents = String::from("\u{feff}");
    contents.push_str(&line_of(1, "2026-09-13T12:00:00+00:00", "a"));
    contents.push('\n');
    std::fs::write(&path, contents).unwrap();
    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 1);
    assert!(reason.contains("invalid JSON"), "{reason}");
}

/// An event kind this build has never heard of: a newer
/// `willikins-server` wrote the file, then an older binary opened it.
/// Refused, not skipped -- pinned deliberately. The cost is that a
/// rollback cannot read a journal the newer build appended to; the
/// alternative (skip what you do not understand) would let an older
/// binary silently replay an incomplete history and, worse, let anyone
/// who can write the file hide an event behind an unknown `kind`.
#[test]
fn an_unknown_event_kind_is_refused_rather_than_skipped() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut contents = line_of(1, "2026-09-13T12:00:00+00:00", "a");
    contents.push('\n');
    contents.push_str(
        r#"{"seq":2,"at":"2026-09-13T12:00:01+00:00","event":{"kind":"from_the_future","what":1}}"#,
    );
    contents.push('\n');
    std::fs::write(&path, contents).unwrap();
    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 2);
    assert!(reason.contains("invalid JSON"), "{reason}");
}

/// A very long but entirely valid line replays: nothing caps an entry's
/// size, and nothing should -- a `PlanRecorded` for a wide `for_each`
/// workflow is legitimately large, and the journal reads only its own
/// exclusively-locked file, never network input.
#[test]
fn a_ten_megabyte_valid_line_replays() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let version = "v".repeat(10 * 1024 * 1024);
    write_lines(&path, &[line_of(1, "2026-09-13T12:00:00+00:00", &version)]);
    let journal = FileJournal::open(&path).expect("a large valid line must replay");
    assert_eq!(journal.entries().len(), 1);
    match &journal.entries()[0].event {
        Event::ServerStarted { version: read, .. } => assert_eq!(read.len(), version.len()),
        other => panic!("unexpected event: {other:?}"),
    }
}

/// A 10 MB line of garbage is refused *without echoing it*: a rejected
/// oversized input quoted back in full was a real bug in this codebase
/// once (`docs/HANDOFF.md`, "Recent Context"), and an error message is
/// exactly the kind of place a journal line's bytes must not reappear.
#[test]
fn a_ten_megabyte_garbage_line_is_refused_without_echoing_it() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut contents = "x".repeat(10 * 1024 * 1024);
    contents.push('\n');
    std::fs::write(&path, contents).unwrap();
    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 1);
    assert!(
        reason.len() < 200,
        "the error echoed the offending line ({} bytes)",
        reason.len()
    );
    assert!(!reason.contains("xxxxxxxxxx"), "{reason}");
}

#[test]
fn a_path_that_is_a_directory_is_an_io_error_not_a_panic() {
    let dir = temp_dir();
    let result = FileJournal::open(dir.path());
    match result {
        Err(JournalError::Io { .. }) => {}
        other => panic!("expected Io, got {other:?}"),
    }
}

/// Every `append` grows the file by exactly the line it serialized plus
/// its newline, and nothing else: no buffering that could interleave two
/// entries, no partial line left behind. Together with `sync_data` being
/// called before `append` returns (by inspection, `src/file.rs`), this is
/// the observable half of "one durable line per append".
#[test]
fn each_append_grows_the_file_by_exactly_one_line() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut journal = FileJournal::open(&path).unwrap();
    let mut expected = 0_u64;
    for version in ["a", "bb", "ccc"] {
        let entry = journal.append(server_started(version)).unwrap();
        expected += serde_json::to_string(&entry).unwrap().len() as u64 + 1;
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            expected,
            "after appending {version}"
        );
    }
    assert_eq!(journal.entries().len(), 3);
}

/// The gap, pinned rather than papered over: replay validates `seq`
/// contiguity and timestamp monotonicity, so a *reordered* pair of lines
/// or a *deleted* interior line is caught, but an in-place edit of one
/// line's payload -- same `seq`, same `at`, different event -- is not, and
/// neither is truncating the file at a line boundary.
///
/// A hash of the previous line chained into each entry would catch the
/// first of those. It is deliberately not implemented: the chain would be
/// computed and verified by the same binary, from no key and no external
/// anchor, so anyone able to edit the file can recompute the chain from
/// the edited line onward with a few lines of script, and tail truncation
/// stays self-consistent either way. It would detect accidental
/// corruption and a naive hand-edit -- not tampering -- and claiming
/// otherwise in an audit record is worse than this test. Real
/// tamper-evidence needs an HMAC keyed from server configuration the
/// journal's writer cannot read back, or an external anchor (append-only
/// storage, a countersigned checkpoint); both are `willikins-server`
/// decisions with a key-management story attached, not a journal-crate
/// addition. Recorded in this task's verification notes.
#[test]
fn a_hand_edited_payload_on_an_earlier_line_is_not_detected() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    {
        let mut journal = FileJournal::open(&path).unwrap();
        journal.append(server_started("honest")).unwrap();
        journal.append(server_started("second")).unwrap();
    }
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("honest"));
    let tampered = contents.replace("honest", "forged");
    std::fs::write(&path, &tampered).unwrap();

    {
        let journal =
            FileJournal::open(&path).expect("this is the documented gap, not a bug to fix");
        match &journal.entries()[0].event {
            Event::ServerStarted { version, .. } => assert_eq!(version, "forged"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    // Truncating at a line boundary is equally undetectable.
    let first_line_end = tampered.find('\n').unwrap() + 1;
    std::fs::write(&path, &tampered[..first_line_end]).unwrap();
    let truncated = FileJournal::open(&path).expect("a boundary truncation still replays");
    assert_eq!(truncated.entries().len(), 1);
}

/// Reordering two lines *is* caught, by `seq` alone.
#[test]
fn two_swapped_lines_are_refused() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    write_lines(
        &path,
        &[
            line_of(2, "2026-09-13T12:00:01+00:00", "b"),
            line_of(1, "2026-09-13T12:00:00+00:00", "a"),
        ],
    );
    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 1);
    assert!(reason.contains("expected seq 1"), "{reason}");
}

/// Deleting an interior line is caught too: every later line's `seq` is
/// then one higher than its position.
#[test]
fn a_deleted_interior_line_is_refused() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    {
        let mut journal = FileJournal::open(&path).unwrap();
        for version in ["a", "b", "c"] {
            journal.append(server_started(version)).unwrap();
        }
    }
    let contents = std::fs::read_to_string(&path).unwrap();
    let kept: Vec<&str> = contents
        .lines()
        .enumerate()
        .filter_map(|(index, line)| (index != 1).then_some(line))
        .collect();
    let mut rewritten = kept.join("\n");
    rewritten.push('\n');
    std::fs::write(&path, rewritten).unwrap();

    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 2);
    assert!(reason.contains("expected seq 2"), "{reason}");
}

/// A journal appended to after a reopen keeps one contiguous `seq` run
/// across the whole file, not two runs restarting at 1.
#[test]
fn appending_after_a_reopen_continues_the_sequence() {
    let dir = temp_dir();
    let path: PathBuf = dir.path().join("journal.jsonl");
    {
        let mut journal = FileJournal::open(&path).unwrap();
        journal.append(server_started("a")).unwrap();
    }
    {
        let mut journal = FileJournal::open(&path).unwrap();
        assert_eq!(journal.append(server_started("b")).unwrap().seq, 2);
    }
    let reopened = FileJournal::open(&path).unwrap();
    let seqs: Vec<u64> = reopened.entries().iter().map(|entry| entry.seq).collect();
    assert_eq!(seqs, vec![1, 2]);
}

/// A file whose final line was cut mid-write is refused, and the refusal
/// names the partial line -- `src/file.rs` pins this too; repeated here
/// because the byte-level shape matters (a cut that lands *after* a
/// complete JSON object but before its newline must still be refused, not
/// parsed as a whole line).
#[test]
fn a_cut_after_a_complete_object_but_before_its_newline_is_still_refused() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut contents = line_of(1, "2026-09-13T12:00:00+00:00", "a");
    contents.push('\n');
    contents.push_str(&line_of(2, "2026-09-13T12:00:01+00:00", "b"));
    std::fs::write(&path, contents).unwrap();
    let (line, reason) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 2);
    assert!(reason.contains("truncated"), "{reason}");
}

/// An entry written by hand with a stray extra top-level field is
/// refused: `deny_unknown_fields` is what makes "replay reproduces
/// exactly what was appended" true even for a line that parses.
#[test]
fn a_line_with_a_stray_field_is_refused() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut value: serde_json::Value =
        serde_json::from_str(&line_of(1, "2026-09-13T12:00:00+00:00", "a")).unwrap();
    value["surprise"] = serde_json::Value::Bool(true);
    let mut contents = serde_json::to_string(&value).unwrap();
    contents.push('\n');
    std::fs::write(&path, contents).unwrap();
    let (line, _) = expect_corrupt(FileJournal::open(&path));
    assert_eq!(line, 1);
}

/// A second `open` in the same process is refused, then succeeds once the
/// first is dropped -- `src/file.rs` pins both; repeated here alongside
/// the cross-process case in `tests/locking.rs` so the three live
/// together in the reader's mind.
#[test]
fn a_second_open_in_this_process_is_refused_then_allowed() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let first = FileJournal::open(&path).unwrap();
    assert!(matches!(
        FileJournal::open(&path),
        Err(JournalError::Locked { .. })
    ));
    drop(first);
    assert!(FileJournal::open(&path).is_ok());
}

/// A journal whose *path* is replaced by a rename while it is open keeps
/// appending into the now-unlinked original inode, and those appends are
/// invisible to anyone who later opens the path. Replay reads the path
/// (`std::fs::read_to_string`) rather than the exclusively-locked
/// descriptor, so the lock protects the inode it took, not the name --
/// pinned here as the known consequence, and reported as a cheap
/// hardening (`seek(0)` on the held descriptor instead of a fresh
/// path-based read) if `willikins-server` ever wants the name protected
/// too. It needs local write access to the journal's directory, which is
/// already enough to delete the file outright.
#[test]
fn a_rename_over_the_journal_path_orphans_later_appends() {
    let dir = temp_dir();
    let path = dir.path().join("journal.jsonl");
    let mut journal = FileJournal::open(&path).unwrap();
    journal.append(server_started("first")).unwrap();

    let decoy = dir.path().join("decoy.jsonl");
    write_lines(&decoy, &[line_of(1, "2026-09-13T12:00:00+00:00", "decoy")]);
    std::fs::rename(&decoy, &path).unwrap();

    // The live journal happily appends -- into the orphaned inode.
    journal.append(server_started("second")).unwrap();
    assert_eq!(journal.entries().len(), 2);
    drop(journal);

    let reopened = FileJournal::open(&path).expect("the replacement replays on its own terms");
    assert_eq!(reopened.entries().len(), 1);
    match &reopened.entries()[0].event {
        Event::ServerStarted { version, .. } => assert_eq!(version, "decoy"),
        other => panic!("unexpected event: {other:?}"),
    }
}
