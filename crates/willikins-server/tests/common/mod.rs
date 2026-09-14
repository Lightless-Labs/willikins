#![allow(dead_code)]
//! Small shared constructors for `willikins-server`'s own acceptance
//! tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::describe::{PartialInputs, RawInput};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_journal::{Clock, ManualClock, MemoryJournal, PrincipalId, Reason, Timestamp};
use willikins_server::{Butler, ButlerConfig, SharedJournal};
use willikins_types::ProjectSlug;

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Copy the fixture at `workflows/<relative>` (or `workflows/fixtures/<relative>`,
/// tried second) into `dir` under `filename` -- letting a test give a
/// document a different on-disk stem than its source file
/// (`irreversible.yaml`'s own internal name is `new-rust-service-irreversible`,
/// so it is copied in as `new-rust-service-irreversible.yaml`; see
/// `willikins_server`'s `document` module docs for why the stem must
/// match).
pub fn copy_fixture_as(dir: &Path, relative: &str, filename: &str) {
    let root = workspace_root();
    let candidates = [
        root.join("workflows").join(relative),
        root.join("workflows").join("fixtures").join(relative),
    ];
    let source = candidates
        .iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            panic!("fixture `{relative}` not found under workflows/ or workflows/fixtures/")
        });
    let contents = std::fs::read(source).unwrap();
    std::fs::write(dir.join(filename), contents).unwrap();
}

pub fn principal(name: &str) -> PrincipalId {
    PrincipalId::parse(name).unwrap()
}

pub fn reason(text: &str) -> Reason {
    Reason::parse(text).unwrap()
}

pub fn partial_inputs(pairs: &[(&str, &str)]) -> PartialInputs {
    let mut inputs = PartialInputs::new();
    for (name, value) in pairs {
        inputs.insert(
            willikins_core::InputName::parse(name).unwrap(),
            RawInput::Scalar((*value).to_string()),
        );
    }
    inputs
}

/// The positive fixture's usual test inputs.
pub fn new_rust_service_inputs() -> PartialInputs {
    partial_inputs(&[("slug", "third-thoughts"), ("org", "lightless-labs")])
}

/// A `Butler` over a fresh [`MemoryJournal`] sharing `clock` with the
/// journal, `catalog`, reading documents from `dir`, and using the
/// design's default windows -- plus the same [`SharedJournal`] handle, so
/// a test can inspect raw entries (`journal.lock().unwrap().entries()`)
/// after driving `Butler` through its public API.
pub fn butler_with_journal(
    dir: &Path,
    catalog: willikins_core::Catalog,
    clock: Arc<ManualClock>,
) -> (Butler, SharedJournal) {
    let journal: SharedJournal = Arc::new(Mutex::new(MemoryJournal::with_clock(
        clock.clone() as Arc<dyn Clock>
    )));
    let config = ButlerConfig {
        workflows_dir: dir.to_path_buf(),
        journal: journal.clone(),
        catalog,
        clock: clock as Arc<dyn Clock>,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    };
    (Butler::new(config), journal)
}

/// Like [`butler_with_journal`], with custom windows.
pub fn butler_with_windows(
    dir: &Path,
    catalog: willikins_core::Catalog,
    clock: Arc<ManualClock>,
    approval_window: std::time::Duration,
    apply_window: std::time::Duration,
) -> (Butler, SharedJournal) {
    let journal: SharedJournal = Arc::new(Mutex::new(MemoryJournal::with_clock(
        clock.clone() as Arc<dyn Clock>
    )));
    let config = ButlerConfig {
        workflows_dir: dir.to_path_buf(),
        journal: journal.clone(),
        catalog,
        clock: clock as Arc<dyn Clock>,
        approval_window,
        apply_window,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    };
    (Butler::new(config), journal)
}

/// Poll `butler.run(run_id)` until it reports something other than
/// `Running`, or panic after `attempts` tries (2ms apart) -- a run
/// continues on a detached background thread, so a test that needs to
/// see it finish waits for it rather than asserting immediately.
pub fn wait_for_run(
    butler: &Butler,
    run_id: willikins_journal::RunId,
    attempts: u32,
) -> willikins_journal::RunRecord {
    for _ in 0..attempts {
        if let Some(record) = butler.run(run_id)
            && !matches!(record.state, willikins_journal::RunState::Running)
        {
            return record;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("run {run_id} did not finish within the poll budget");
}

/// A [`ManualClock`] starting at a fixed, arbitrary instant.
pub fn manual_clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(
        Timestamp::parse("2026-09-14T00:00:00+00:00").unwrap(),
    ))
}

// ---------------------------------------------------------------------
// A test-only tool that blocks on a channel until released.
// ---------------------------------------------------------------------

/// `test.blocking.ensure`: `ensure` blocks on an `mpsc` rendezvous until
/// the test sends a release signal -- how acceptance test 8's
/// `RunInProgress` scenario holds a run open long enough for a second
/// `apply` to observe it in progress.
pub struct BlockingTool {
    spec: ToolSpec,
    gate: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl BlockingTool {
    /// Build the tool and the [`std::sync::mpsc::Sender`] a test uses to
    /// release it, once per call to `ensure`.
    #[must_use]
    pub fn new() -> (Self, std::sync::mpsc::Sender<()>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut inputs = IndexMap::new();
        inputs.insert(
            willikins_core::helpers::port("key"),
            PortSpec {
                ty: willikins_core::PortType::Exact(willikins_core::helpers::scalar("ProjectSlug")),
                required: true,
            },
        );
        let tool = Self {
            spec: ToolSpec {
                name: willikins_core::helpers::tool_name("test.blocking.ensure"),
                description: "Test tool: blocks on a channel until released.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: vec![willikins_core::helpers::port("key")],
                class: Class::Reversible,
                pure: false,
            },
            gate: Mutex::new(rx),
        };
        (tool, tx)
    }
}

impl Tool for BlockingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let _key: ProjectSlug = willikins_core::helpers::get(inputs, "key")?;
        self.gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recv()
            .expect("the test releases the gate before dropping its Sender");
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
    }
}
