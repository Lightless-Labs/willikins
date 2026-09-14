//! Adversarial verification of task 10a's `willikins-server` library.
//!
//! Each test here is an attack on one of the claims the milestone plan's
//! trust boundaries make: a plan runs only under an approval a human gave
//! for exactly that plan, inside both windows, against the document bytes
//! the human's plan was computed from and the provider state it
//! predicted; one run at a time; a decision is final; nothing outside the
//! trusted directory can be planned or applied; and no secret byte
//! reaches a response, an error, or the journal.

mod common;

use std::sync::{Arc, Mutex};

use willikins_journal::{Clock, Event, ManualClock, Timestamp};
use willikins_server::ButlerError;
use willikins_types::{DomainType, WorkflowName};

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

// ---------------------------------------------------------------------
// A clock that parks one named thread the first time it reads it.
// ---------------------------------------------------------------------

/// A [`Clock`] that, the *first* time the thread named `gate_thread`
/// reads it, announces itself on `ready` and then blocks until the test
/// releases it. Every other read (and every later read from that thread)
/// delegates straight to the inner [`ManualClock`].
///
/// This is the seam that makes a read-then-append race in `Butler`
/// deterministic instead of a stress test: `Butler::approve`/`reject`
/// read the clock between loading the plan record and appending the
/// decision, so parking one caller there lets the other caller's whole
/// decision land in between.
struct GateOnceClock {
    inner: ManualClock,
    gate_thread: String,
    gate: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
}

impl GateOnceClock {
    /// The clock, the receiver that fires once the gated thread is
    /// parked, and the sender that releases it.
    fn new(
        gate_thread: &str,
    ) -> (
        Arc<Self>,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let clock = Arc::new(Self {
            inner: ManualClock::new(Timestamp::parse("2026-09-14T00:00:00+00:00").unwrap()),
            gate_thread: gate_thread.to_string(),
            gate: Mutex::new(Some((ready_tx, release_rx))),
        });
        (clock, ready_rx, release_tx)
    }
}

impl Clock for GateOnceClock {
    fn now(&self) -> Timestamp {
        if std::thread::current().name() == Some(self.gate_thread.as_str()) {
            let taken = self
                .gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some((ready, release)) = taken {
                ready.send(()).expect("the test holds the ready receiver");
                release.recv().expect("the test releases the gate");
            }
        }
        self.inner.now()
    }
}

// ---------------------------------------------------------------------
// A decision is final, even under a concurrent decision
// ---------------------------------------------------------------------

/// Adversarial pass 1, item 4: the journal's fold takes the *last*
/// approval event for a plan, so an `ApprovalGranted` recorded after an
/// `ApprovalRejected` revives a rejected plan. `Butler::approve`/`reject`
/// is the only thing that stops that -- and it can only stop it if the
/// check ("is this plan still `Pending`?") and the append of the decision
/// happen under one journal lock.
///
/// The attack: park an `approve` between its record read and its append
/// (through [`GateOnceClock`], which `decide`'s own window math reads),
/// land a `reject` in the gap, then let the `approve` finish. If the two
/// halves are not atomic, the journal ends up holding both decisions,
/// the fold reports the last one, and the rejected plan applies.
#[test]
fn a_decision_landing_between_another_decisions_check_and_append_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_irreversible(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let (clock, ready, release) = GateOnceClock::new("gated-approver");
    let (butler, journal) =
        common::butler_with_any_clock(dir.path(), catalog, clock as Arc<dyn Clock>);
    let butler = Arc::new(butler);

    let response = butler
        .plan(
            wf(common::IRREVERSIBLE_NAME),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("the irreversible fixture plans cleanly");
    assert!(response.requires_approval);
    let plan_id = response.plan_id;

    let approving = {
        let butler = Arc::clone(&butler);
        std::thread::Builder::new()
            .name("gated-approver".to_string())
            .spawn(move || butler.approve(plan_id, common::principal("approver-a")))
            .expect("spawning the gated approver")
    };

    // The approver is now parked inside `decide`, having already seen the
    // plan as `Pending`.
    ready.recv().expect("the gated approver parks");

    let rejected = butler.reject(
        plan_id,
        common::principal("approver-b"),
        common::reason("not today"),
    );
    assert!(
        rejected.is_ok(),
        "the rejection is the first decision to land: {rejected:?}"
    );

    release.send(()).expect("releasing the gated approver");
    let approved = approving.join().expect("the approver thread finishes");

    assert!(
        matches!(approved, Err(ButlerError::AlreadyDecided { .. })),
        "an approval that raced a rejection must lose, not overwrite it: {approved:?}"
    );

    let entries = journal.lock().unwrap();
    let decisions: Vec<&'static str> = entries
        .entries()
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::ApprovalGranted { plan_id: p, .. } if *p == plan_id => Some("granted"),
            Event::ApprovalRejected { plan_id: p, .. } if *p == plan_id => Some("rejected"),
            _ => None,
        })
        .collect();
    assert_eq!(
        decisions,
        ["rejected"],
        "a plan must carry exactly one decision event"
    );
}
