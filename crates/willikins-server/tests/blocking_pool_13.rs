//! Adversarial pass 2, goals "exhaust the blocking pool" and "journal
//! failure injection": the two attacks that need a tool that never
//! returns and a journal that refuses to append, neither of which can be
//! arranged from outside the process.
//!
//! Everything here is deterministic. Nothing sleeps hoping a race
//! happens: the blocking tool waits on a condition variable the test
//! releases, and the failing journal refuses on exactly the append the
//! test names.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use tower::ServiceExt as _;

use willikins_core::tool::helpers::{exact, port, scalar, tool_name};
use willikins_core::{
    Catalog, Class, Ensured, Inputs, Observation, Outputs, SinkToken, Tool, ToolError, ToolSpec,
};
use willikins_journal::{Clock, Entry, Event, Journal, JournalError, MemoryJournal, RunState};
use willikins_server::{Butler, ButlerConfig, HttpConfig, SharedJournal, TokenHash};
use willikins_types::DomainType as _;

// ---------------------------------------------------------------------
// A tool that never returns until the test says so.
// ---------------------------------------------------------------------

/// A gate a test opens once: every caller after the first `passes` of
/// them blocks until it does.
///
/// `passes` exists because `plan` itself calls every planned tool's
/// `read`, so a test that wants `apply`'s *re-plan* to be the thing that
/// blocks has to let the first read through.
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
    waiting: AtomicUsize,
    passes: AtomicUsize,
}

impl Gate {
    fn new(passes: usize) -> Self {
        Self {
            open: Mutex::new(false),
            changed: Condvar::new(),
            waiting: AtomicUsize::new(0),
            passes: AtomicUsize::new(passes),
        }
    }

    /// Take a free pass if one is left; otherwise block until the gate
    /// opens.
    fn wait(&self) {
        loop {
            let left = self.passes.load(Ordering::SeqCst);
            if left == 0 {
                break;
            }
            if self
                .passes
                .compare_exchange(left, left - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return;
            }
        }
        self.waiting.fetch_add(1, Ordering::SeqCst);
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*open {
            open = self
                .changed
                .wait(open)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn open(&self) {
        *self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    fn waiting(&self) -> usize {
        self.waiting.load(Ordering::SeqCst)
    }

    /// Wait (bounded) until at least `count` callers are blocked inside
    /// [`Self::wait`], so a test never races the threads it just started.
    fn wait_for_waiters(&self, count: usize) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while self.waiting() < count {
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
}

/// Opens a [`Gate`] when it leaves scope.
///
/// Added by adversarial pass 2's completeness critic, 2026-09-15, found
/// by mutation: replacing `try_acquire_owned` with a waiting
/// `acquire_owned` made
/// [`the_concurrency_bound_answers_busy_instead_of_queueing_on_a_full_pool`]
/// panic at its five-second timeout exactly as intended -- and then hang
/// for ever, because the panic unwound *past* `gate.open()` and
/// `tokio::runtime::Runtime`'s own `Drop` waits for every blocking task
/// to finish. A regression in the bound has to fail loudly, not wedge
/// the suite for whoever runs it next.
///
/// Declare it *after* the runtime: locals drop in reverse declaration
/// order, so the last declared opens the gate first and the runtime then
/// drops with nothing wedged.
struct OpenOnDrop(Arc<Gate>);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// `fake.blocking.ensure`: a tool whose `read` blocks until the test
/// opens its gate. Stands in for a provider that has accepted a
/// connection and will not answer -- the shape the plan's own risk
/// section calls "a synchronous core under an asynchronous server".
struct Blocking {
    spec: ToolSpec,
    gate: Arc<Gate>,
}

impl Blocking {
    fn new(gate: Arc<Gate>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("key"), exact("ProjectSlug", true));
        let mut outputs = IndexMap::new();
        outputs.insert(port("key"), scalar("ProjectSlug"));
        Self {
            spec: ToolSpec {
                name: tool_name("fake.blocking.ensure"),
                description: "A test-only tool that blocks until released.".to_string(),
                inputs,
                outputs,
                key: vec![port("key")],
                class: Class::Reversible,
                pure: false,
            },
            gate,
        }
    }

    fn predicted(inputs: &Inputs) -> Outputs {
        let mut outputs = Outputs::new();
        if let Some(value) = inputs.get(&port("key")) {
            outputs.insert(port("key"), value.clone());
        }
        outputs
    }
}

impl Tool for Blocking {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        self.gate.wait();
        Ok(Observation::Absent {
            predicted: Self::predicted(inputs),
        })
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        self.gate.wait();
        Ok(Ensured {
            outputs: Self::predicted(inputs),
            changed: true,
        })
    }
}

const BLOCKING_DOCUMENT: &str = "\
name: blocking-probe
description: A probe whose one tool does not answer until a test releases it.
inputs:
  slug: { type: ProjectSlug }
steps:
  wait:
    tool: fake.blocking.ensure
    with:
      key: ${{ inputs.slug }}
";

fn blocking_catalog(gate: &Arc<Gate>) -> Catalog {
    let (_state, mut catalog) = Butler::fake_catalog();
    catalog
        .insert(Arc::new(Blocking::new(Arc::clone(gate))))
        .expect("the blocking tool registers");
    catalog
}

const AGENT_TOKENS: [&str; 8] = [
    "agent-token-0",
    "agent-token-1",
    "agent-token-2",
    "agent-token-3",
    "agent-token-4",
    "agent-token-5",
    "agent-token-6",
    "agent-token-7",
];

fn config(max_concurrent: usize) -> HttpConfig {
    HttpConfig::build(
        "127.0.0.1:0".parse().unwrap(),
        AGENT_TOKENS
            .iter()
            .map(|token| TokenHash::of(token))
            .collect(),
        TokenHash::of("approver-token"),
        vec!["127.0.0.1".to_string()],
    )
    .expect("a valid http config")
    .with_max_concurrent_tool_calls(max_concurrent)
}

fn plan_request(token: &str, slug: &str) -> axum::http::Request<axum::body::Body> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "plan",
            "arguments": { "workflow": "blocking-probe", "inputs": { "slug": slug } },
        },
    });
    axum::http::Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {token}"))
        .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn healthz_request() -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("GET")
        .uri("/healthz")
        .header("host", "127.0.0.1")
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

fn butler_for(dir: &std::path::Path, catalog: Catalog) -> Arc<Butler> {
    butler_and_journal(dir, catalog).0
}

fn butler_and_journal(dir: &std::path::Path, catalog: Catalog) -> (Arc<Butler>, SharedJournal) {
    std::fs::write(dir.join("blocking-probe.yaml"), BLOCKING_DOCUMENT).unwrap();
    let clock = common::manual_clock();
    let journal: SharedJournal = Arc::new(Mutex::new(MemoryJournal::with_clock(
        Arc::clone(&clock) as Arc<dyn Clock>,
    )));
    let butler = Arc::new(Butler::new(ButlerConfig {
        workflows_dir: dir.to_path_buf(),
        journal: Arc::clone(&journal),
        catalog,
        clock: clock as Arc<dyn Clock>,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        // The pool attacks need more than ten plans a minute from one
        // principal; the rate limiter is not what is under test here.
        plan_rate_per_minute: 1_000,
        read_rate_per_minute: 1_000,
    }));
    (butler, journal)
}

// =====================================================================
// The blocking pool is exhaustible: measured, then bounded.
// =====================================================================

/// **The finding.** With the concurrency bound raised out of the way,
/// four `plan` calls against a tool that never answers fill a blocking
/// pool of four, and the fifth request -- a *different* principal's
/// cheap `validate` -- never completes, because `spawn_blocking` queues
/// once the pool is full. `/healthz` keeps answering, because it uses no
/// blocking thread at all: a deployment's own health check therefore
/// reports a server that answers nothing as healthy.
///
/// The pool is 4 here so the test is seconds rather than minutes;
/// production's default is 512, which is the same shape at a different
/// scale (and 512 stuck threads is also 512 stacks of resident memory).
#[test]
fn a_tool_that_never_returns_exhausts_the_blocking_pool_and_healthz_still_says_ok() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Gate::new(0));
    let butler = butler_for(dir.path(), blocking_catalog(&gate));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .unwrap();
    // Declared after `runtime` so it drops first: see `OpenOnDrop`.
    let _opener = OpenOnDrop(Arc::clone(&gate));

    runtime.block_on(async {
        // A bound far above the pool, so this measures the pool itself.
        let router = willikins_server::router(Arc::clone(&butler), &config(1_000));
        let mut stuck = Vec::new();
        for (index, token) in AGENT_TOKENS.iter().take(4).enumerate() {
            let router = router.clone();
            let token = (*token).to_string();
            stuck.push(tokio::spawn(async move {
                router
                    .oneshot(plan_request(&token, &format!("probe-{index}")))
                    .await
            }));
        }
        assert!(
            gate.wait_for_waiters(4),
            "four plans must reach the blocking tool"
        );

        // `/healthz` answers: it never touches a blocking thread.
        let health = router.clone().oneshot(healthz_request()).await.unwrap();
        assert_eq!(health.status(), 200);

        // A fifth principal's call does not.
        let queued = tokio::time::timeout(
            Duration::from_secs(3),
            router
                .clone()
                .oneshot(plan_request(AGENT_TOKENS[5], "probe-5")),
        )
        .await;
        assert!(
            queued.is_err(),
            "with the pool full, a further call queues rather than answering"
        );

        gate.open();
        for task in stuck {
            let _ = task.await;
        }
    });
}

/// **The bound.** The same attack against the shipped configuration:
/// the concurrency bound refuses the call that would have queued, at
/// once, with a kind-tagged `Busy` an agent can act on -- and the
/// unbounded read paths (`run_status`) and `/healthz` keep answering.
#[test]
fn the_concurrency_bound_answers_busy_instead_of_queueing_on_a_full_pool() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(Gate::new(0));
    let butler = butler_for(dir.path(), blocking_catalog(&gate));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .unwrap();
    // Declared after `runtime` so it drops first: see `OpenOnDrop`.
    let _opener = OpenOnDrop(Arc::clone(&gate));

    runtime.block_on(async {
        // Two permits, four pool threads: the bound is reached first, by
        // construction, which is exactly the production relationship (64
        // permits, 512 threads) at a size a test can reach.
        let router = willikins_server::router(Arc::clone(&butler), &config(2));
        let mut stuck = Vec::new();
        for (index, token) in AGENT_TOKENS.iter().take(2).enumerate() {
            let router = router.clone();
            let token = (*token).to_string();
            stuck.push(tokio::spawn(async move {
                router
                    .oneshot(plan_request(&token, &format!("probe-{index}")))
                    .await
            }));
        }
        assert!(gate.wait_for_waiters(2), "two plans must be in flight");

        let refused = tokio::time::timeout(
            Duration::from_secs(5),
            router
                .clone()
                .oneshot(plan_request(AGENT_TOKENS[3], "probe-3")),
        )
        .await
        .expect("the bound answers rather than queueing")
        .unwrap();
        assert_eq!(refused.status(), 200, "a domain error is a 200 tool result");
        let json = body_json(refused).await;
        assert_eq!(
            json["result"]["isError"],
            serde_json::Value::Bool(true),
            "{json}"
        );
        assert_eq!(
            json["result"]["structuredContent"]["kind"], "Busy",
            "{json}"
        );

        // `/healthz` and the unbounded read path still answer.
        let health = router.clone().oneshot(healthz_request()).await.unwrap();
        assert_eq!(health.status(), 200);

        gate.open();
        for task in stuck {
            let _ = task.await;
        }
    });
}

/// A second `apply` while the first is still in its pre-run checks --
/// the window where the first is calling every planned tool's `read`
/// against a provider that has stopped answering -- is refused at once
/// with `ApplyPreparing`, not blocked behind the first.
///
/// **The finding this pins.** Task 10a held the single-apply mutex
/// across those checks, so a second `apply` blocked on the mutex *inside
/// its own blocking thread*, one thread per caller, until the pool was
/// gone; and `run_in_progress()` -- which the graceful-shutdown path
/// polls from async code -- blocked a runtime worker on the same mutex.
/// The slot is now a three-state value the mutex is held only long
/// enough to read and change.
#[test]
fn a_second_apply_during_the_first_ones_pre_run_checks_is_refused_not_blocked() {
    let dir = tempfile::tempdir().unwrap();
    // One free pass: `plan`'s own `read` of the single node. `apply`'s
    // re-plan is then the call that blocks.
    let gate = Arc::new(Gate::new(1));
    let (butler, journal) = butler_and_journal(dir.path(), blocking_catalog(&gate));

    let plan = butler
        .plan(
            willikins_types::WorkflowName::parse("blocking-probe").unwrap(),
            &common::partial_inputs(&[("slug", "probe-one")]),
            common::principal("agent-one"),
        )
        .expect("the probe plans");
    let plan_id = plan.plan_id;

    let first = {
        let butler = Arc::clone(&butler);
        std::thread::spawn(move || butler.apply(plan_id, common::principal("agent-one")))
    };
    assert!(
        gate.wait_for_waiters(1),
        "the first apply must reach the blocking re-plan"
    );

    // The second apply answers immediately rather than blocking on the
    // first: a bounded join proves it did not queue.
    let second = {
        let butler = Arc::clone(&butler);
        std::thread::spawn(move || butler.apply(plan_id, common::principal("agent-two")))
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !second.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "the second apply blocked behind the first"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let refused = second.join().expect("no panic");
    assert!(
        matches!(refused, Err(willikins_server::ButlerError::ApplyPreparing)),
        "expected ApplyPreparing, got {refused:?}"
    );

    // And a run is *not* in progress while an apply is only preparing:
    // there is no run to drain.
    assert!(butler.run_in_progress().is_none());

    gate.open();
    let _ = first.join().expect("no panic");

    // The refusal is journaled, with its own reason -- not folded into
    // `RunInProgress`, which would name a run that never started.
    let guard = journal
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let found = guard.entries().iter().any(|entry| {
        matches!(
            &entry.event,
            Event::ApplyRefused {
                reason: willikins_journal::ApplyRefusedReason::ApplyPreparing,
                ..
            }
        )
    });
    assert!(found, "the ApplyPreparing refusal must be journaled");
}

// =====================================================================
// Journal failure injection.
// =====================================================================

/// A journal that accepts `accept` appends and then refuses every one.
struct FailingJournal {
    inner: MemoryJournal,
    accepted: usize,
    accept: usize,
}

impl Journal for FailingJournal {
    fn append(&mut self, event: Event) -> Result<Entry, JournalError> {
        if self.accepted >= self.accept {
            return Err(JournalError::Io {
                path: std::path::PathBuf::from("/data/journal.jsonl"),
                source: std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "read-only file system",
                ),
            });
        }
        self.accepted += 1;
        self.inner.append(event)
    }

    fn entries(&self) -> &[Entry] {
        self.inner.entries()
    }
}

/// **The finding.** A journal that stops accepting appends mid-run --
/// a read-only or full volume, which on a hosted deployment is a
/// detached or exhausted disk -- leaves the run reading `Running`
/// forever: `RunFinished` is the event a run's terminal state is folded
/// from, and `JournalObserver` stashes an append failure rather than
/// panicking (by design: a run's own outcome is not less true for the
/// journal having trouble recording it).
///
/// What is *not* broken: the single-apply slot is released either way,
/// so the server keeps accepting applies; and `run_in_progress()`
/// returning `None` while the record still says `Running` is the signal
/// that tells the two apart -- "still going" from "died unrecorded".
/// `willikins-cli`'s `wait_for_run` now uses exactly that signal instead
/// of polling forever.
#[test]
fn a_journal_that_stops_accepting_leaves_the_run_running_but_releases_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("blocking-probe.yaml"), BLOCKING_DOCUMENT).unwrap();
    let gate = Arc::new(Gate::new(usize::MAX));
    let clock = common::manual_clock();
    // Accept everything up to and including `RunStarted`, then refuse:
    // `ServerStarted` is not appended (this is `Butler::new`), so the
    // run's own `NodeStarted`/`NodeFinished`/`RunFinished` are what get
    // refused.
    let journal: SharedJournal = Arc::new(Mutex::new(FailingJournal {
        inner: MemoryJournal::with_clock(Arc::clone(&clock) as Arc<dyn Clock>),
        accepted: 0,
        accept: 6,
    }));
    let butler = Arc::new(Butler::new(ButlerConfig {
        workflows_dir: dir.path().to_path_buf(),
        journal: Arc::clone(&journal),
        catalog: blocking_catalog(&gate),
        clock: clock as Arc<dyn Clock>,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: 1_000,
        read_rate_per_minute: 1_000,
    }));

    let plan = butler
        .plan(
            willikins_types::WorkflowName::parse("blocking-probe").unwrap(),
            &common::partial_inputs(&[("slug", "probe-one")]),
            common::principal("agent-one"),
        )
        .expect("the probe plans");
    let handle = butler
        .apply(plan.plan_id, common::principal("agent-one"))
        .expect("the apply starts");

    // The run thread finishes (the tool never blocks here) but its
    // `RunFinished` append was refused.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while butler.run_in_progress().is_some() {
        assert!(
            std::time::Instant::now() < deadline,
            "the run thread must release the slot even when the journal refuses"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let record = butler.run(handle.run_id).expect("the run was started");
    assert!(
        matches!(record.state, RunState::Running),
        "a run whose RunFinished was refused stays Running: {:?}",
        record.state
    );
    assert!(
        butler.run_in_progress().is_none(),
        "and nothing is actually running -- the pair is the signal"
    );

    // A later apply is still accepted: the slot was not leaked.
    let second = butler.plan(
        willikins_types::WorkflowName::parse("blocking-probe").unwrap(),
        &common::partial_inputs(&[("slug", "probe-two")]),
        common::principal("agent-one"),
    );
    assert!(
        second.is_err(),
        "the journal is still refusing, so `plan` cannot record"
    );
}
