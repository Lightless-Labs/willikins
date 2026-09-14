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
use willikins_journal::{
    Clock, FileJournal, ManualClock, MemoryJournal, PrincipalId, Reason, Timestamp,
};
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

/// A `Butler` over a real [`FileJournal`] at `journal_path`, sharing
/// `clock` with it -- for a test that needs the journal to survive a
/// `Butler` being dropped and rebuilt (a simulated process restart),
/// unlike [`butler_with_journal`]'s [`MemoryJournal`], which holds its
/// entries only in memory and cannot be reopened by a fresh `Butler`.
///
/// Opens with a short retry (matching
/// `tests/file_journal_round_trip.rs`'s own `open_once_unlocked`):
/// dropping the previous `Butler` drops its last strong `Arc` reference
/// to the journal, which is what releases the exclusive `flock` an
/// immediate reopen would otherwise race.
pub fn butler_over_file_journal(
    dir: &Path,
    journal_path: &Path,
    catalog: willikins_core::Catalog,
    clock: Arc<ManualClock>,
) -> Butler {
    let mut journal = None;
    for _ in 0..200 {
        match FileJournal::open_with_clock(journal_path, clock.clone() as Arc<dyn Clock>) {
            Ok(opened) => {
                journal = Some(opened);
                break;
            }
            Err(willikins_journal::JournalError::Locked { .. }) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(other) => panic!(
                "journal at {} does not open: {other}",
                journal_path.display()
            ),
        }
    }
    let journal: SharedJournal =
        Arc::new(Mutex::new(journal.unwrap_or_else(|| {
            panic!("journal at {} stayed locked", journal_path.display())
        })));
    let config = ButlerConfig {
        workflows_dir: dir.to_path_buf(),
        journal,
        catalog,
        clock: clock as Arc<dyn Clock>,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    };
    Butler::new(config)
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

/// Like [`butler_with_journal`], over any [`Clock`] rather than a
/// [`ManualClock`] specifically -- what an adversarial test that needs a
/// clock which *blocks* (see `tests/adversarial_10a.rs`'s
/// `GateOnceClock`) hands a `Butler`.
pub fn butler_with_any_clock(
    dir: &Path,
    catalog: willikins_core::Catalog,
    clock: Arc<dyn Clock>,
) -> (Butler, SharedJournal) {
    let journal: SharedJournal = Arc::new(Mutex::new(MemoryJournal::with_clock(clock.clone())));
    let config = ButlerConfig {
        workflows_dir: dir.to_path_buf(),
        journal: journal.clone(),
        catalog,
        clock,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    };
    (Butler::new(config), journal)
}

/// `workflows/fixtures/irreversible.yaml`'s own internal name: the
/// filename stem it must be copied in under (see `willikins_server`'s
/// `document` module docs).
pub const IRREVERSIBLE_NAME: &str = "new-rust-service-irreversible";

/// Copy `workflows/fixtures/irreversible.yaml` into `dir` under the stem
/// its own internal `name:` requires.
pub fn copy_irreversible(dir: &Path) {
    copy_fixture_as(
        dir,
        "irreversible.yaml",
        "new-rust-service-irreversible.yaml",
    );
}

// ---------------------------------------------------------------------
// A test-only tool whose `ensure` panics.
// ---------------------------------------------------------------------

/// `test.panicking.ensure`: `read` answers `Absent`, `ensure` panics.
/// How an adversarial test drives a run thread into an unwind, to check
/// that the single-apply lock is released, `RunFinished { Failed }` is
/// journaled, and `Butler::run` reports the run as failed rather than
/// leaving it `Running` for ever.
pub struct PanickingTool {
    spec: ToolSpec,
}

impl PanickingTool {
    /// The tool, named `test.panicking.ensure`.
    #[must_use]
    pub fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            willikins_core::helpers::port("key"),
            PortSpec {
                ty: willikins_core::PortType::Exact(willikins_core::helpers::scalar("ProjectSlug")),
                required: true,
            },
        );
        Self {
            spec: ToolSpec {
                name: willikins_core::helpers::tool_name("test.panicking.ensure"),
                description: "Test tool: panics from `ensure`.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: vec![willikins_core::helpers::port("key")],
                class: Class::Reversible,
                pure: false,
            },
        }
    }
}

impl Tool for PanickingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        panic!("test.panicking.ensure always panics");
    }
}

// ---------------------------------------------------------------------
// A test-only pure tool whose list output a test can change.
// ---------------------------------------------------------------------

/// `test.list.source`: a pure tool whose one output, `items:
/// list<EnvironmentSlug>`, is whatever the test's shared `Vec` currently
/// holds. A `for_each` over it expands to one instance per item, so a
/// test can change the size of a plan's `for_each` expansion between
/// `plan` and `apply` without touching the document.
pub struct ListSourceTool {
    spec: ToolSpec,
    items: Arc<Mutex<Vec<willikins_types::EnvironmentSlug>>>,
}

impl ListSourceTool {
    /// The tool and the shared list a test mutates.
    #[must_use]
    pub fn new(
        initial: Vec<willikins_types::EnvironmentSlug>,
    ) -> (Self, Arc<Mutex<Vec<willikins_types::EnvironmentSlug>>>) {
        let items = Arc::new(Mutex::new(initial));
        let mut inputs = IndexMap::new();
        inputs.insert(
            willikins_core::helpers::port("key"),
            PortSpec {
                ty: willikins_core::PortType::Exact(willikins_core::helpers::scalar("ProjectSlug")),
                required: true,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(
            willikins_core::helpers::port("items"),
            willikins_core::helpers::list("EnvironmentSlug"),
        );
        let tool = Self {
            spec: ToolSpec {
                name: willikins_core::helpers::tool_name("test.list.source"),
                description: "Test tool: a list the test controls.".to_string(),
                inputs,
                outputs,
                // A pure tool declares no key: it names no resource.
                key: Vec::new(),
                class: Class::Reversible,
                pure: true,
            },
            items: Arc::clone(&items),
        };
        (tool, items)
    }

    fn compute(&self) -> Outputs {
        let items = self
            .items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut outputs = Outputs::new();
        outputs.insert(
            willikins_core::helpers::port("items"),
            willikins_core::Value::known_list(items),
        );
        outputs
    }
}

impl Tool for ListSourceTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Present(self.compute()))
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: self.compute(),
            changed: false,
        })
    }
}

/// `test.counting.ensure`: keyed on an `EnvironmentSlug`, `read` answers
/// `Absent` and `ensure` counts its calls. The downstream half of a
/// `for_each` over [`ListSourceTool`], and the way a drift test proves no
/// provider write happened.
pub struct CountingTool {
    spec: ToolSpec,
    calls: Arc<Mutex<u32>>,
}

impl CountingTool {
    /// The tool and the shared call counter.
    #[must_use]
    pub fn new() -> (Self, Arc<Mutex<u32>>) {
        let calls = Arc::new(Mutex::new(0));
        let mut inputs = IndexMap::new();
        inputs.insert(
            willikins_core::helpers::port("key"),
            PortSpec {
                ty: willikins_core::PortType::Exact(willikins_core::helpers::scalar(
                    "EnvironmentSlug",
                )),
                required: true,
            },
        );
        let tool = Self {
            spec: ToolSpec {
                name: willikins_core::helpers::tool_name("test.counting.ensure"),
                description: "Test tool: counts its own ensure calls.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: vec![willikins_core::helpers::port("key")],
                class: Class::Reversible,
                pure: false,
            },
            calls: Arc::clone(&calls),
        };
        (tool, calls)
    }
}

impl Tool for CountingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        *self
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
    }
}
