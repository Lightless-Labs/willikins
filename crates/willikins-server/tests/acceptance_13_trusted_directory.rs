//! Acceptance test 13 ("Trusted directory") and the startup half of the
//! "Startup and the trusted directory" section: `Butler::start` validates
//! every document in the directory before it will run at all, and
//! `Butler::list_workflows` lists exactly what is there.

mod common;

use willikins_journal::{Clock, Event, MemoryJournal};
use willikins_server::{Butler, ButlerConfig, ButlerError, StartupError};
use willikins_types::{DomainType, WorkflowName};

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
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

/// Starting against the real trusted directory (the three positive
/// fixtures) succeeds and journals `ServerStarted` naming all three files.
#[test]
fn starting_against_the_real_workflows_directory_succeeds_and_journals_server_started() {
    let clock = common::manual_clock();
    let dir = common::workspace_root().join("workflows");
    let cfg = config(&dir, clock);
    let journal = std::sync::Arc::clone(&cfg.journal);
    let butler = Butler::start(cfg).expect("the real trusted directory starts cleanly");

    let entries = journal.lock().unwrap();
    let started = entries
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::ServerStarted {
                workflow_hashes, ..
            } => Some(workflow_hashes.clone()),
            _ => None,
        });
    let hashes = started.expect("ServerStarted must be journaled");
    assert!(hashes.contains_key("new-rust-service.yaml"), "{hashes:?}");
    assert!(
        hashes.contains_key("new-rust-service-buildkite.yaml"),
        "{hashes:?}"
    );
    assert!(
        hashes.contains_key("rotate-service-token.yaml"),
        "{hashes:?}"
    );
    drop(entries);

    let summaries = butler
        .list_workflows(common::principal("agent"))
        .expect("list_workflows succeeds against the real directory");
    let names: Vec<&str> = summaries.iter().map(|s| s.name.as_str()).collect();
    // `scan_directory` sorts filenames, and `-` (0x2D) sorts before `.`
    // (0x2E), so `new-rust-service-buildkite.yaml` sorts before
    // `new-rust-service.yaml`.
    assert_eq!(
        names,
        [
            "new-rust-service-buildkite",
            "new-rust-service",
            "rotate-service-token"
        ]
    );
}

/// A directory holding a document that fails `check` refuses startup,
/// naming the file, and nothing is journaled.
#[test]
fn a_directory_with_a_document_that_fails_check_refuses_startup() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("bad.yaml"),
        "name: bad\ndescription: d\nsteps:\n  a:\n    tool: no.such.tool\n",
    )
    .unwrap();
    let clock = common::manual_clock();
    let cfg = config(dir.path(), clock);
    let journal = std::sync::Arc::clone(&cfg.journal);

    let Err(err) = Butler::start(cfg) else {
        panic!("a document that fails check must refuse startup")
    };
    assert!(matches!(err, StartupError::Check { .. }), "{err:?}");
    assert!(
        journal.lock().unwrap().entries().is_empty(),
        "nothing should be journaled on a refused startup"
    );
}

/// A name not in the directory is `UnknownWorkflow` from `plan`, after a
/// clean startup.
#[test]
fn a_name_not_in_the_directory_is_unknown_workflow() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let clock = common::manual_clock();
    let butler = Butler::start(config(dir.path(), clock)).unwrap();

    let err = butler
        .plan(
            wf("no-such-workflow"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect_err("an unknown workflow name must refuse");
    assert!(
        matches!(err, ButlerError::UnknownWorkflow { .. }),
        "{err:?}"
    );
}

/// `WorkflowName`'s own grammar refuses a path-traversal attempt before a
/// `Butler` ever sees one -- there is no separate check to bypass. Pinned
/// here at the server boundary because `plan`/`apply`/`list_workflows`
/// take a `WorkflowName`, never a raw string: a caller cannot even
/// construct the argument these methods need without going through
/// `WorkflowName::parse` first.
#[test]
fn a_path_traversal_attempt_is_refused_by_workflow_name_before_it_reaches_a_butler() {
    assert!(WorkflowName::parse("../x").is_err());
    assert!(WorkflowName::parse("a/b").is_err());
}

#[cfg(unix)]
#[test]
fn a_symlinked_document_in_the_trusted_directory_refuses_startup() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    common::copy_fixture_as(outside.path(), "new-rust-service.yaml", "real.yaml");
    std::os::unix::fs::symlink(
        outside.path().join("real.yaml"),
        dir.path().join("new-rust-service.yaml"),
    )
    .unwrap();
    let clock = common::manual_clock();
    let Err(err) = Butler::start(config(dir.path(), clock)) else {
        panic!("a symlinked document must refuse")
    };
    assert!(matches!(err, StartupError::Symlink { .. }), "{err:?}");
}
