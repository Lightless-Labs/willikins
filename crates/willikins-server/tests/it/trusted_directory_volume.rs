//! Acceptance test 28 ("The trusted directory on a writable volume"),
//! milestone 2c, decision 25, task 21: the trusted directory can change
//! underneath a running server on a mounted volume, so every scan and
//! every named load opens it once, through a handle
//! (`O_DIRECTORY | O_NOFOLLOW`), and never re-resolves it by path.
//!
//! `acceptance_13_trusted_directory.rs` already pins the single-file
//! symlink refusal and the directory-of-real-fixtures happy path; this
//! module pins the parts that are new to milestone 2c: the *directory
//! itself* swapped to a symlink after a clean start, a negative fixture
//! added after start, exact-bytes hashing, and the startup log lines.
//!
//! The `RAILWAY_VOLUME_MOUNT_PATH` warning is **not** pinned here: it is
//! read and compared in `cli.rs`'s own `build_butler` wiring, a private
//! function this integration test cannot reach (only `willikins-server`'s
//! public API is visible from `tests/it/`) -- see
//! `crates/willikins-server/src/cli.rs`'s `tests` module,
//! `warns_by_name_when_workflows_dir_is_outside_the_mount_path` and its
//! two negative siblings, for that half.

use crate::common;

use std::sync::{Arc, Mutex};

use willikins_journal::{Clock, Event, MemoryJournal};
use willikins_server::{Butler, ButlerConfig, ButlerError, StartupError};
use willikins_types::{DomainType, WorkflowName};

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn config(
    dir: &std::path::Path,
    clock: std::sync::Arc<willikins_journal::ManualClock>,
) -> ButlerConfig {
    let (_state, catalog) = willikins_providers_fake::empty();
    ButlerConfig {
        workflows_dir: dir.to_path_buf(),
        journal: std::sync::Arc::new(std::sync::Mutex::new(MemoryJournal::with_clock(
            clock.clone() as std::sync::Arc<dyn Clock>,
        ))),
        catalog,
        clock: clock as std::sync::Arc<dyn Clock>,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    }
}

fn write(dir: &std::path::Path, filename: &str, contents: &str) {
    std::fs::write(dir.join(filename), contents).unwrap();
}

const CHILDLESS_DOC: &str = "name: foo\ndescription: a workflow\nsteps: {}\n";

// -------------------------------------------------------------
// Logging: capture `tracing` events the way `cli.rs`'s own tests do.
// -------------------------------------------------------------

#[derive(Clone)]
struct CapturingWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture_logs<T>(body: impl FnOnce() -> T) -> (T, String) {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let for_writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || CapturingWriter(for_writer.clone()))
        .with_ansi(false)
        .finish();
    let result = tracing::subscriber::with_default(subscriber, body);
    let log = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    (result, log)
}

// -------------------------------------------------------------
// The directory itself, symlinked.
// -------------------------------------------------------------

#[cfg(unix)]
#[test]
fn a_symlinked_trusted_directory_refuses_startup() {
    let real = tempfile::tempdir().unwrap();
    write(real.path(), "foo.yaml", CHILDLESS_DOC);
    let parent = tempfile::tempdir().unwrap();
    let link = parent.path().join("workflows");
    std::os::unix::fs::symlink(real.path(), &link).unwrap();

    let clock = common::manual_clock();
    let Err(err) = Butler::start(config(&link, clock)) else {
        panic!("a symlinked directory must refuse")
    };
    assert!(matches!(err, StartupError::Symlink { .. }), "{err:?}");
}

#[cfg(unix)]
#[test]
fn the_directory_swapped_to_a_symlink_after_start_refuses_list_workflows() {
    // `workflows` starts as a real directory with a valid document, so
    // `Butler::start` opens a handle on a real directory, exactly as a
    // deployment's own first start does.
    let parent = tempfile::tempdir().unwrap();
    let workflows = parent.path().join("workflows");
    std::fs::create_dir(&workflows).unwrap();
    write(&workflows, "foo.yaml", CHILDLESS_DOC);
    let clock = common::manual_clock();
    let butler =
        Butler::start(config(&workflows, clock)).expect("starts cleanly against a real directory");

    // The operator's own re-publish procedure (decision 25) never does
    // this -- it renames a *sibling* staging directory into place -- but
    // the point of this test is that even the worst case (the directory
    // itself replaced by a symlink) is refused, not silently followed.
    let elsewhere = tempfile::tempdir().unwrap();
    write(elsewhere.path(), "foo.yaml", CHILDLESS_DOC);
    std::fs::remove_dir_all(&workflows).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), &workflows).unwrap();

    let err = butler
        .list_workflows(common::principal("agent"))
        .expect_err("list_workflows must refuse once the directory itself is a symlink");
    match err {
        ButlerError::Startup {
            error: StartupError::Symlink { .. },
        } => {}
        other => panic!("expected Startup/Symlink, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn plan_by_name_after_the_directory_is_swapped_to_a_symlink_is_unknown_workflow() {
    // A plain subdirectory under a `TempDir`, not a `TempDir`'s own root:
    // its root gets swapped out for a symlink below, and a `TempDir`'s
    // `Drop` recursing through a symlink it no longer owns is exactly the
    // kind of surprise this layout avoids.
    let parent = tempfile::tempdir().unwrap();
    let workflows = parent.path().join("workflows");
    std::fs::create_dir(&workflows).unwrap();
    write(&workflows, "foo.yaml", CHILDLESS_DOC);
    let clock = common::manual_clock();
    let butler = Butler::start(config(&workflows, clock)).expect("starts cleanly");

    let real = tempfile::tempdir().unwrap();
    write(real.path(), "foo.yaml", CHILDLESS_DOC);
    std::fs::remove_dir_all(&workflows).unwrap();
    std::os::unix::fs::symlink(real.path(), &workflows).unwrap();

    let err = butler
        .plan(
            wf("foo"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect_err(
            "a symlinked directory must collapse to UnknownWorkflow, same as a missing one",
        );
    assert!(
        matches!(err, ButlerError::UnknownWorkflow { .. }),
        "{err:?}"
    );
}

// -------------------------------------------------------------
// Exact-bytes hashing: `ServerStarted` records the sha256 of exactly
// what was parsed.
// -------------------------------------------------------------

#[test]
fn server_started_hashes_exactly_the_bytes_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "foo.yaml", CHILDLESS_DOC);
    let clock = common::manual_clock();
    let cfg = config(dir.path(), clock);
    let journal = std::sync::Arc::clone(&cfg.journal);
    Butler::start(cfg).expect("starts cleanly");

    let entries = journal.lock().unwrap();
    let hashes = entries
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::ServerStarted {
                workflow_hashes, ..
            } => Some(workflow_hashes.clone()),
            _ => None,
        })
        .expect("ServerStarted must be journaled");
    let expected = sha256_hex(CHILDLESS_DOC.as_bytes());
    assert_eq!(hashes.get("foo.yaml"), Some(&expected));
}

// -------------------------------------------------------------
// A negative fixture placed in the directory after `Butler::start`.
// -------------------------------------------------------------

#[test]
fn a_fixture_that_fails_to_parse_after_start_refuses_plan_as_document() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "foo.yaml", CHILDLESS_DOC);
    let clock = common::manual_clock();
    let butler = Butler::start(config(dir.path(), clock)).expect("starts cleanly");

    write(dir.path(), "bad.yaml", "not: [valid, yaml");
    let err = butler
        .plan(
            wf("bad"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect_err("a document that fails to parse must refuse plan");
    assert!(matches!(err, ButlerError::Document { .. }), "{err:?}");
}

#[test]
fn a_fixture_that_fails_check_after_start_refuses_plan_as_check() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "foo.yaml", CHILDLESS_DOC);
    let clock = common::manual_clock();
    let butler = Butler::start(config(dir.path(), clock)).expect("starts cleanly");

    write(
        dir.path(),
        "bad.yaml",
        "name: bad\ndescription: d\nsteps:\n  a:\n    tool: no.such.tool\n",
    );
    let err = butler
        .plan(
            wf("bad"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect_err("a document that fails check must refuse plan");
    assert!(matches!(err, ButlerError::Check { .. }), "{err:?}");
}

/// `list_workflows` fails closed -- `ButlerError::Startup` naming the
/// whole scan's own refusal -- never by silently dropping the broken
/// file and listing only the rest.
#[test]
fn a_broken_fixture_after_start_makes_list_workflows_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "foo.yaml", CHILDLESS_DOC);
    let clock = common::manual_clock();
    let butler = Butler::start(config(dir.path(), clock)).expect("starts cleanly");

    write(dir.path(), "bad.yaml", "not: [valid, yaml");
    let err = butler
        .list_workflows(common::principal("agent"))
        .expect_err("list_workflows must fail closed, not silently skip the broken file");
    assert!(matches!(err, ButlerError::Startup { .. }), "{err:?}");
}

// -------------------------------------------------------------
// Startup logging: one line per document (name + short sha), and a
// named warning for an empty directory.
// -------------------------------------------------------------

#[test]
fn start_logs_one_line_per_document_naming_it_and_its_short_sha() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "foo.yaml", CHILDLESS_DOC);
    let clock = common::manual_clock();
    let cfg = config(dir.path(), clock);
    let (result, log) = capture_logs(|| Butler::start(cfg));
    result.expect("starts cleanly");

    assert!(log.contains("foo.yaml"), "{log}");
    let expected_short_sha = sha256_hex(CHILDLESS_DOC.as_bytes())[..12].to_string();
    assert!(log.contains(&expected_short_sha), "{log}");
}

#[test]
fn start_warns_by_name_on_an_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let clock = common::manual_clock();
    let cfg = config(dir.path(), clock);
    let (result, log) = capture_logs(|| Butler::start(cfg));
    result.expect("an empty directory still starts cleanly");

    assert!(
        log.to_lowercase().contains("no document") || log.to_lowercase().contains("empty"),
        "{log}"
    );
}

// -------------------------------------------------------------
// A symlinked entry inside the directory, swapped in after start (the
// per-entry half of decision 25 -- the directory itself stays real;
// `acceptance_13_trusted_directory.rs` already pins the at-startup case).
// -------------------------------------------------------------

#[cfg(unix)]
#[test]
fn an_entry_swapped_to_a_symlink_after_start_refuses_list_workflows() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "foo.yaml", CHILDLESS_DOC);
    let clock = common::manual_clock();
    let butler = Butler::start(config(dir.path(), clock)).expect("starts cleanly");

    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "real.yaml", CHILDLESS_DOC);
    std::fs::remove_file(dir.path().join("foo.yaml")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("real.yaml"),
        dir.path().join("foo.yaml"),
    )
    .unwrap();

    let err = butler
        .list_workflows(common::principal("agent"))
        .expect_err("a symlinked entry swapped in after start must refuse list_workflows");
    match err {
        ButlerError::Startup {
            error: StartupError::Symlink { .. },
        } => {}
        other => panic!("expected Startup/Symlink, got {other:?}"),
    }
}
