//! The exclusive lock across *process* boundaries, which is the case the
//! deployment actually has: two `willikins-server` processes pointed at
//! one `WILLIKINS_JOURNAL_PATH` (a restart overlapping its predecessor, a
//! second replica on a shared volume, an operator running the CLI with
//! `--journal` against the server's own file). `src/file.rs`'s unit tests
//! cover the same-process case; an advisory `flock` is per open file
//! description, so a same-process second `open` and a second process's
//! `open` take different code paths through the kernel and a test of one
//! is not a test of the other.
//!
//! The child process is this very test binary, re-executed with
//! `--ignored --exact` so it runs exactly one otherwise-skipped test, and
//! the path it should try under an environment variable. Spawning the test
//! binary itself keeps the test self-contained: no helper crate, no
//! `cargo` invocation from inside a test, and no assumption about the
//! target directory's layout.

use std::path::PathBuf;
use std::process::Command;

use willikins_journal::{Event, FileJournal, Journal, JournalError};

/// Set to the journal path for the child role; absent for the parent.
const PATH_VAR: &str = "WILLIKINS_TEST_LOCK_PATH";
/// `refused` or `granted`: what the child must observe.
const EXPECT_VAR: &str = "WILLIKINS_TEST_LOCK_EXPECT";

/// The child role. `#[ignore]`d so a plain `cargo test` never runs it;
/// the parent invokes it by name with `--ignored --exact`, and it returns
/// without asserting anything if its environment variable is absent (so
/// that `cargo test -- --ignored`, which a human might well run, cannot
/// fail spuriously).
#[test]
#[ignore = "spawned as a child process by the cross-process lock tests"]
fn lock_probe_child() {
    let Ok(path) = std::env::var(PATH_VAR) else {
        return;
    };
    let expectation = std::env::var(EXPECT_VAR).expect("the child role needs an expectation");
    let result = FileJournal::open(PathBuf::from(path));
    match expectation.as_str() {
        "refused" => assert!(
            matches!(result, Err(JournalError::Locked { .. })),
            "another process holds the lock, so this open must be refused: {result:?}"
        ),
        "granted" => assert!(
            result.is_ok(),
            "nothing holds the lock, so this open must succeed: {result:?}"
        ),
        other => panic!("unknown expectation {other}"),
    }
}

/// Run `lock_probe_child` in a fresh process against `path`, expecting it
/// to observe `expectation`, and return whether that child passed.
fn spawn_probe(path: &std::path::Path, expectation: &str) -> bool {
    let status = Command::new(std::env::current_exe().expect("the test binary's own path"))
        .args(["--ignored", "--exact", "lock_probe_child"])
        .env(PATH_VAR, path)
        .env(EXPECT_VAR, expectation)
        .status()
        .expect("the child test process must start");
    status.success()
}

#[test]
fn a_second_process_cannot_open_a_held_journal_and_can_once_it_is_released() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");

    let held = FileJournal::open(&path).expect("the first open must succeed");
    assert!(
        spawn_probe(&path, "refused"),
        "a second process opened a journal this one holds"
    );
    drop(held);

    assert!(
        spawn_probe(&path, "granted"),
        "a second process could not open the journal after the first released it"
    );
}

/// The lock is taken at `open`, before anything is appended, so an empty
/// journal is just as exclusive as one with entries: a server that has
/// only just started still owns the file.
#[test]
fn the_lock_covers_a_journal_that_has_never_been_appended_to() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let held = FileJournal::open(&path).expect("the first open must succeed");
    assert!(held.entries().is_empty());
    assert!(spawn_probe(&path, "refused"));
}

/// The lock is an advisory flock on the file, so a journal held open and
/// then dropped mid-panic must still release it: `FileJournal`'s own
/// `Drop` runs during unwinding, and nothing in this crate leaks the
/// value past it. A journal that stayed locked after a panicking request
/// would take the whole server down with it until a restart.
#[test]
fn a_panic_while_the_journal_is_held_releases_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let panicking_path = path.clone();

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::thread::spawn(move || {
        let mut journal = FileJournal::open(&panicking_path).unwrap();
        journal
            .append(Event::ServerStarted {
                version: "0.1.0".to_string(),
                workflows_dir: "/workflows".to_string(),
                workflow_hashes: std::collections::BTreeMap::new(),
            })
            .unwrap();
        panic!("something went wrong while the journal was open");
    })
    .join();
    std::panic::set_hook(previous_hook);

    assert!(outcome.is_err(), "the thread was supposed to panic");
    let reopened = FileJournal::open(&path)
        .expect("the lock must be released when the journal is dropped during unwinding");
    assert_eq!(
        reopened.entries().len(),
        1,
        "the append that preceded the panic is still durable"
    );
}
