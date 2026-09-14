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

/// An `ApprovalGranted` for plan B is not an approval of plan A. The
/// binding lives in the journal's own fold (the event carries a
/// `plan_id`), and this pins that `Butler::apply` reads it that way:
/// approving B leaves A exactly as unapproved as it was.
#[test]
fn an_approval_granted_for_one_plan_does_not_let_another_apply() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_irreversible(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let plan_a = butler
        .plan(
            wf(common::IRREVERSIBLE_NAME),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("plan A");
    let plan_b = butler
        .plan(
            wf(common::IRREVERSIBLE_NAME),
            &common::partial_inputs(&[("slug", "other-thoughts"), ("org", "lightless-labs")]),
            common::principal("agent"),
        )
        .expect("plan B");
    assert_ne!(plan_a.plan_id, plan_b.plan_id);

    butler
        .approve(plan_b.plan_id, common::principal("approver"))
        .expect("plan B is approved");

    let err = butler
        .apply(plan_a.plan_id, common::principal("agent"))
        .expect_err("plan A was never approved");
    assert!(
        matches!(err, ButlerError::ApprovalRequired { .. }),
        "{err:?}"
    );

    let entries = journal.lock().unwrap();
    let started = entries
        .entries()
        .iter()
        .any(|entry| matches!(&entry.event, Event::RunStarted { .. }));
    assert!(!started, "nothing must have run");
}

/// A second `approve` of an already-approved plan is `AlreadyDecided`,
/// not a duplicate `ApprovalGranted` event.
#[test]
fn a_second_approve_of_an_approved_plan_is_already_decided() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_irreversible(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf(common::IRREVERSIBLE_NAME),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("plans cleanly");
    butler
        .approve(response.plan_id, common::principal("approver-a"))
        .expect("the first approval succeeds");
    let second = butler.approve(response.plan_id, common::principal("approver-b"));
    assert!(
        matches!(second, Err(ButlerError::AlreadyDecided { .. })),
        "{second:?}"
    );

    let entries = journal.lock().unwrap();
    let grants = entries
        .entries()
        .iter()
        .filter(|entry| matches!(&entry.event, Event::ApprovalGranted { .. }))
        .count();
    assert_eq!(grants, 1, "a second grant must not be journaled");
}

/// `approve` of a plan whose class never needed one is
/// `NotPendingApproval`, not a no-op that quietly records a human
/// decision for a plan no human was asked about.
#[test]
fn approving_an_automatically_approved_plan_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("plans cleanly");
    assert!(!response.requires_approval);

    let approved = butler.approve(response.plan_id, common::principal("approver"));
    assert!(
        matches!(approved, Err(ButlerError::NotPendingApproval { .. })),
        "{approved:?}"
    );
    let rejected = butler.reject(
        response.plan_id,
        common::principal("approver"),
        common::reason("no"),
    );
    assert!(
        matches!(rejected, Err(ButlerError::NotPendingApproval { .. })),
        "{rejected:?}"
    );

    let entries = journal.lock().unwrap();
    let human_decisions = entries
        .entries()
        .iter()
        .filter(|entry| {
            matches!(
                &entry.event,
                Event::ApprovalGranted { .. } | Event::ApprovalRejected { .. }
            )
        })
        .count();
    assert_eq!(human_decisions, 0);
}

// ---------------------------------------------------------------------
// Window edges, pinned
// ---------------------------------------------------------------------

/// Both windows are checked with `elapsed > window`, so the instant the
/// window's own length has elapsed -- exactly, to the microsecond a
/// `Timestamp` records -- is still inside it. Pinned in all three places
/// that check a window, and stated as the deliberate reading of
/// `PlanResponse::expires_at`: the plan is runnable *at* `expires_at`,
/// and refused after it.
#[test]
fn a_decision_and_an_apply_exactly_at_each_windows_edge_are_still_accepted() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_irreversible(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock.clone());

    let response = butler
        .plan(
            wf(common::IRREVERSIBLE_NAME),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("plans cleanly");

    // Exactly the approval window, to the instant.
    clock.advance(willikins_server::ButlerConfig::DEFAULT_APPROVAL_WINDOW);
    butler
        .approve(response.plan_id, common::principal("approver"))
        .expect("an approval exactly at the approval window's edge is inside it");

    // Exactly the apply window after the grant, to the instant.
    clock.advance(willikins_server::ButlerConfig::DEFAULT_APPLY_WINDOW);
    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect("an apply exactly at the apply window's edge is inside it");
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, willikins_journal::RunState::Succeeded, "{run:?}");
}

/// One microsecond past each edge is outside it -- the other half of the
/// pin above, so the boundary is nailed down from both sides rather than
/// just "somewhere around a day".
#[test]
fn a_decision_one_microsecond_past_the_approval_window_is_expired() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_irreversible(dir.path());
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock.clone());

    let response = butler
        .plan(
            wf(common::IRREVERSIBLE_NAME),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("plans cleanly");
    clock.advance(
        willikins_server::ButlerConfig::DEFAULT_APPROVAL_WINDOW
            + std::time::Duration::from_micros(1),
    );
    let approved = butler.approve(response.plan_id, common::principal("approver"));
    assert!(
        matches!(
            approved,
            Err(ButlerError::PlanExpired {
                window: willikins_server::ExpiryWindow::Approval
            })
        ),
        "{approved:?}"
    );
}
