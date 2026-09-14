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

// ---------------------------------------------------------------------
// A hand-written journal file
// ---------------------------------------------------------------------

/// Reopen `path` once its exclusive lock has been released, retrying for
/// a second (a finished run's thread may still hold the last `Arc`).
fn reopen(path: &std::path::Path, clock: &Arc<dyn Clock>) -> willikins_journal::FileJournal {
    for _ in 0..200 {
        match willikins_journal::FileJournal::open_with_clock(path, clock.clone()) {
            Ok(journal) => return journal,
            Err(willikins_journal::JournalError::Locked { .. }) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(other) => panic!("the file replays cleanly: {other}"),
        }
    }
    panic!("journal stayed locked for a full second");
}

/// Drive `plan` (and, if `approve`, an approval) over a real
/// `FileJournal` at `journal_path`, then drop everything so the file can
/// be edited by hand. Returns the recorded plan id.
fn journal_a_pending_plan(
    workflows_dir: &std::path::Path,
    journal_path: &std::path::Path,
    clock: Arc<ManualClock>,
    approve: bool,
) -> willikins_journal::PlanId {
    let (_state, catalog) = willikins_providers_fake::empty();
    let journal: willikins_server::SharedJournal = Arc::new(Mutex::new(
        willikins_journal::FileJournal::open_with_clock(
            journal_path,
            clock.clone() as Arc<dyn Clock>,
        )
        .expect("journal opens"),
    ));
    let butler = willikins_server::Butler::new(willikins_server::ButlerConfig {
        workflows_dir: workflows_dir.to_path_buf(),
        journal: journal.clone(),
        catalog,
        clock: clock as Arc<dyn Clock>,
        approval_window: willikins_server::ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: willikins_server::ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: willikins_server::ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: willikins_server::ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    });
    let response = butler
        .plan(
            wf(common::IRREVERSIBLE_NAME),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("plans cleanly");
    if approve {
        butler
            .approve(response.plan_id, common::principal("approver"))
            .expect("approval succeeds");
    }
    drop(butler);
    drop(journal);
    response.plan_id
}

/// Renumber `lines`' `seq` fields to 1..n and write them back to `path`.
fn rewrite_journal(path: &std::path::Path, lines: &[String]) {
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        let mut entry: serde_json::Value = serde_json::from_str(line).expect("a journal line");
        entry["seq"] = serde_json::json!(index + 1);
        out.push_str(&serde_json::to_string(&entry).unwrap());
        out.push('\n');
    }
    std::fs::write(path, out).unwrap();
}

/// An `ApprovalGranted` line spliced in *before* the `PlanRecorded` it
/// names is not an approval of anything: the fold only applies an
/// approval to a plan it has already seen recorded, so the plan replays
/// as `Pending` and a fresh `Butler` over the edited file still refuses
/// to apply it.
///
/// This is the shape an attacker with write access to the journal file
/// would reach for after seeing that `FileJournal::open` refuses a
/// *duplicate* `PlanRecorded` (so the plan itself cannot be rewritten):
/// pre-dating an approval is the remaining edit that replay's own rules
/// (contiguous `seq`, non-decreasing `at`) do not catch.
#[test]
fn an_approval_spliced_in_before_the_plan_it_approves_does_not_count() {
    let workflows = tempfile::tempdir().unwrap();
    common::copy_irreversible(workflows.path());
    let journal_dir = tempfile::tempdir().unwrap();
    let path = journal_dir.path().join("journal.jsonl");
    let clock = common::manual_clock();

    let plan_id = journal_a_pending_plan(workflows.path(), &path, clock.clone(), true);

    // Move the grant to the very front, before the `PlanRecorded`.
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let grant = lines
        .iter()
        .position(|line| line.contains("\"approval_granted\""))
        .expect("the grant is on the file");
    let grant_line = lines.remove(grant);
    lines.insert(0, grant_line);
    rewrite_journal(&path, &lines);

    let reopened = reopen(&path, &(clock.clone() as Arc<dyn Clock>));
    let record = willikins_journal::Journal::plan(&reopened, &plan_id).expect("the plan replays");
    assert!(
        matches!(record.approval, willikins_journal::ApprovalState::Pending),
        "a pre-dated grant must not become this plan's approval: {:?}",
        record.approval
    );

    let journal: willikins_server::SharedJournal = Arc::new(Mutex::new(reopened));
    let (_state, catalog) = willikins_providers_fake::empty();
    let butler = willikins_server::Butler::new(willikins_server::ButlerConfig {
        workflows_dir: workflows.path().to_path_buf(),
        journal,
        catalog,
        clock: clock as Arc<dyn Clock>,
        approval_window: willikins_server::ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: willikins_server::ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: willikins_server::ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: willikins_server::ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    });
    let err = butler
        .apply(plan_id, common::principal("agent"))
        .expect_err("the plan is still unapproved");
    assert!(
        matches!(err, ButlerError::ApprovalRequired { .. }),
        "{err:?}"
    );
}

/// `Event::PlanRecorded::document_sha256` is a validated
/// [`willikins_journal::DocumentSha256`], so a hand-edited line whose
/// digest is not exactly 64 lower-case hex characters is refused at
/// replay rather than replaying as a digest no reloaded document could
/// ever match (or, worse, one that a shortened reload comparison might).
#[test]
fn a_plan_recorded_line_with_a_malformed_digest_is_refused_at_replay() {
    let workflows = tempfile::tempdir().unwrap();
    common::copy_irreversible(workflows.path());
    let journal_dir = tempfile::tempdir().unwrap();
    let path = journal_dir.path().join("journal.jsonl");
    let clock = common::manual_clock();

    let _plan_id = journal_a_pending_plan(workflows.path(), &path, clock.clone(), false);

    for bad in [
        "not-a-digest",
        // 64 characters, but upper-case hex.
        "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789",
        // 63 characters.
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef012345678",
    ] {
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<String> = text
            .lines()
            .map(|line| {
                let mut entry: serde_json::Value = serde_json::from_str(line).unwrap();
                if entry["event"]["kind"] == "plan_recorded" {
                    entry["event"]["document_sha256"] = serde_json::json!(bad);
                }
                serde_json::to_string(&entry).unwrap()
            })
            .collect();
        let edited = journal_dir.path().join("edited.jsonl");
        rewrite_journal(&edited, &lines);

        let error = willikins_journal::FileJournal::open_with_clock(
            &edited,
            clock.clone() as Arc<dyn Clock>,
        )
        .err()
        .unwrap_or_else(|| panic!("a `{bad}` digest must not replay"));
        assert!(
            matches!(error, willikins_journal::JournalError::Corrupt { .. }),
            "{error:?}"
        );
    }
}

// ---------------------------------------------------------------------
// Plan identity: the bytes, not the meaning
// ---------------------------------------------------------------------

/// Plan identity is the document's *bytes*. A change that alters nothing
/// a plan could observe -- one appended comment line, leaving the parsed
/// workflow and its whole fingerprint identical -- is still
/// `DocumentChanged`, with no provider call. Pinned deliberately: the
/// refusal is conservative by design, because "the bytes a human's plan
/// was computed from" is the only definition of sameness that does not
/// depend on the parser agreeing with the human.
#[test]
fn a_document_changed_only_in_a_comment_is_still_document_changed() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("plans cleanly");
    let fingerprint_before = response.plan.fingerprint();

    let path = dir.path().join("new-rust-service.yaml");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"# a comment, and nothing else\n");
    std::fs::write(&path, bytes).unwrap();

    let ensure_calls_before = state.lock().unwrap().ensure_calls.clone();
    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("the document's bytes changed");
    assert!(
        matches!(err, ButlerError::DocumentChanged { .. }),
        "{err:?}"
    );
    assert_eq!(
        state.lock().unwrap().ensure_calls,
        ensure_calls_before,
        "no provider call must have happened"
    );

    // The comment really did change nothing a plan can see: re-planning
    // the edited document yields the same fingerprint the refused plan
    // had, so the digest is demonstrably what caught this.
    let after = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("the edited document still plans");
    assert_eq!(
        serde_json::to_value(&fingerprint_before).unwrap(),
        serde_json::to_value(after.plan.fingerprint()).unwrap(),
        "the comment changed nothing the fingerprint sees"
    );
}

/// A document renamed out from under an approved plan is
/// `DocumentChanged`, not `UnknownWorkflow`: `apply` asks for the
/// document *this plan* was computed from, and it is gone. Pinned
/// because both answers are defensible and a caller has to be able to
/// tell the two apart (`UnknownWorkflow` means "never ask me for this
/// again"; `DocumentChanged` means "plan again").
#[test]
fn a_document_renamed_away_between_plan_and_apply_is_document_changed() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("plans cleanly");

    std::fs::rename(
        dir.path().join("new-rust-service.yaml"),
        dir.path().join("something-else.yaml"),
    )
    .unwrap();

    let ensure_calls_before = state.lock().unwrap().ensure_calls.clone();
    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("the document is gone");
    assert!(
        matches!(err, ButlerError::DocumentChanged { .. }),
        "{err:?}"
    );
    assert_eq!(state.lock().unwrap().ensure_calls, ensure_calls_before);
}

/// A file sitting at `<name>.yaml` whose own `name:` is something else is
/// not a document named `<name>`: `plan` refuses it as
/// `UnknownWorkflow`, so `PlanRecord::workflow` and the document's own
/// internal name are always equal for an untampered file -- which is what
/// makes `apply`'s later re-check of that equality mean "the file changed
/// underfoot" and nothing else.
#[test]
fn a_document_whose_internal_name_differs_from_its_filename_cannot_be_planned() {
    let dir = tempfile::tempdir().unwrap();
    // `new-rust-service.yaml`'s bytes, under the wrong filename.
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "impostor.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let err = butler
        .plan(
            wf("impostor"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect_err("`impostor.yaml` does not declare itself `impostor`");
    assert!(
        matches!(&err, ButlerError::UnknownWorkflow { workflow } if workflow.as_str() == "impostor"),
        "{err:?}"
    );
}

/// A symlink inside the trusted directory pointing at a document outside
/// it is refused, never followed -- at `plan` as well as at startup, so a
/// link planted after a clean startup cannot be planned either.
#[cfg(unix)]
#[test]
fn a_symlink_to_a_document_outside_the_trusted_directory_cannot_be_planned() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    common::copy_fixture_as(
        outside.path(),
        "new-rust-service.yaml",
        "new-rust-service.yaml",
    );
    std::os::unix::fs::symlink(
        outside.path().join("new-rust-service.yaml"),
        dir.path().join("new-rust-service.yaml"),
    )
    .unwrap();

    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let err = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect_err("a symlinked document is not in the trusted directory");
    assert!(
        matches!(err, ButlerError::UnknownWorkflow { .. }),
        "{err:?}"
    );

    // And the same link refuses `validate` by name, which resolves a name
    // through the same door.
    let err = butler
        .validate(
            &willikins_server::DocumentSource::Name(wf("new-rust-service")),
            common::principal("agent"),
        )
        .expect_err("validate resolves a name the same way");
    assert!(
        matches!(err, ButlerError::UnknownWorkflow { .. }),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------
// One run at a time
// ---------------------------------------------------------------------

/// Write a one-node document at `<dir>/<name>.yaml` calling `tool` with
/// the literal key `key`.
fn write_one_node_document(dir: &std::path::Path, name: &str, tool: &str, key: &str) {
    std::fs::write(
        dir.join(format!("{name}.yaml")),
        format!(
            "name: {name}\n\
             description: one node calling a test tool\n\
             steps:\n  \
               only:\n    \
                 tool: {tool}\n    \
                 with:\n      \
                   key: {key}\n"
        ),
    )
    .unwrap();
}

/// Two *different* approved plans applied from two threads at once:
/// exactly one starts a run, the other is refused `RunInProgress` naming
/// the winner's run, and once that run finishes the loser applies
/// cleanly. The winner's own `ensure` blocks until this test releases it,
/// so the refusal is observed while a run genuinely is in progress rather
/// than after it quietly finished.
#[test]
fn two_applies_racing_on_two_threads_leave_exactly_one_run() {
    let dir = tempfile::tempdir().unwrap();
    write_one_node_document(dir.path(), "race-a", "test.blocking.ensure", "alpha");
    write_one_node_document(dir.path(), "race-b", "test.blocking.ensure", "beta");

    let (blocking_tool, release) = common::BlockingTool::new();
    let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
    catalog.insert(Arc::new(blocking_tool)).unwrap();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);
    let butler = Arc::new(butler);

    let plan_a = butler
        .plan(
            wf("race-a"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("race-a plans");
    let plan_b = butler
        .plan(
            wf("race-b"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("race-b plans");
    assert!(!plan_a.requires_approval && !plan_b.requires_approval);

    let start = Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = [plan_a.plan_id, plan_b.plan_id]
        .into_iter()
        .map(|plan_id| {
            let butler = Arc::clone(&butler);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                (plan_id, butler.apply(plan_id, common::principal("agent")))
            })
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|handle| handle.join().expect("the apply thread finishes"))
        .collect();

    let winners: Vec<_> = results
        .iter()
        .filter_map(|(plan_id, result)| {
            result.as_ref().ok().map(|handle| (*plan_id, handle.run_id))
        })
        .collect();
    assert_eq!(
        winners.len(),
        1,
        "exactly one apply may start a run: {results:?}"
    );
    let winning_run = winners[0].1;
    let (loser_plan, loser) = results
        .iter()
        .find_map(|(plan_id, result)| result.as_ref().err().map(|error| (*plan_id, error)))
        .expect("the other apply was refused");
    assert!(
        matches!(loser, ButlerError::RunInProgress { run_id } if *run_id == winning_run),
        "the loser must name the winner's run: {loser:?}"
    );

    release.send(()).expect("releasing the winner's ensure");
    let run = common::wait_for_run(&butler, winning_run, 2000);
    assert_eq!(run.state, willikins_journal::RunState::Succeeded, "{run:?}");

    // Exactly one `RunStarted` so far, and the loser can now apply.
    let started = |journal: &willikins_server::SharedJournal| {
        journal
            .lock()
            .unwrap()
            .entries()
            .iter()
            .filter(|entry| matches!(&entry.event, Event::RunStarted { .. }))
            .count()
    };
    assert_eq!(started(&journal), 1);

    let second = butler
        .apply(loser_plan, common::principal("agent"))
        .expect("the lock released with the finished run");
    release.send(()).expect("releasing the second run's ensure");
    let run = common::wait_for_run(&butler, second.run_id, 2000);
    assert_eq!(run.state, willikins_journal::RunState::Succeeded, "{run:?}");
    assert_eq!(started(&journal), 2);
}

/// A run thread whose tool panics must not leave the single-apply lock
/// held for the life of the process: the panic is caught, the run is
/// journaled as `RunFinished { Failed }`, `Butler::run` reports it
/// `Failed` rather than for ever `Running`, and a following `apply` of a
/// different plan starts normally instead of being told a run is still in
/// progress.
#[test]
fn a_panicking_run_thread_releases_the_lock_and_records_a_failed_run() {
    let dir = tempfile::tempdir().unwrap();
    write_one_node_document(dir.path(), "panic-a", "test.panicking.ensure", "alpha");
    write_one_node_document(dir.path(), "panic-b", "test.panicking.ensure", "beta");

    let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(common::PanickingTool::new()))
        .unwrap();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let plan_a = butler
        .plan(
            wf("panic-a"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("panic-a plans");
    let handle = butler
        .apply(plan_a.plan_id, common::principal("agent"))
        .expect("the run starts");
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, willikins_journal::RunState::Failed, "{run:?}");
    assert!(run.error.is_some(), "a failed run records its error");

    let plan_b = butler
        .plan(
            wf("panic-b"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("panic-b plans");
    let second = butler
        .apply(plan_b.plan_id, common::principal("agent"))
        .expect("the lock was released by the panicking run");
    let run = common::wait_for_run(&butler, second.run_id, 2000);
    assert_eq!(run.state, willikins_journal::RunState::Failed, "{run:?}");
}

/// A run outlives the `Butler` that started it: every handle it needs
/// (the journal, the catalog, the lock) is an `Arc` it owns a clone of,
/// so dropping the `Butler` mid-run neither kills the run nor loses its
/// outcome. Pinned because the alternative -- a run silently abandoned
/// when its server value goes away -- would leave a `RunStarted` with no
/// `RunFinished` on the audit trail and a plan permanently `applied`.
#[test]
fn a_butler_dropped_mid_run_still_lets_the_run_finish_and_journal_its_outcome() {
    let dir = tempfile::tempdir().unwrap();
    write_one_node_document(dir.path(), "outlive", "test.blocking.ensure", "alpha");

    let (blocking_tool, release) = common::BlockingTool::new();
    let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
    catalog.insert(Arc::new(blocking_tool)).unwrap();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("outlive"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("plans cleanly");
    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect("the run starts");

    drop(butler);
    release.send(()).expect("releasing the ensure");

    for _ in 0..2000 {
        let finished = willikins_journal::Journal::run(&*journal.lock().unwrap(), &handle.run_id)
            .is_some_and(|run| !matches!(run.state, willikins_journal::RunState::Running));
        if finished {
            let run =
                willikins_journal::Journal::run(&*journal.lock().unwrap(), &handle.run_id).unwrap();
            assert_eq!(run.state, willikins_journal::RunState::Succeeded, "{run:?}");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the run never finished after its Butler was dropped");
}

/// Every `apply` refusal is journaled -- acceptance test 8's own closing
/// sentence, over a list that names `RunInProgress`. Refusing from the
/// single-apply lock without a journal line would leave the one refusal
/// an operator most wants to see (two agents reaching for the same
/// provider at once) invisible on the audit trail.
#[test]
fn a_run_in_progress_refusal_is_journaled_like_every_other_refusal() {
    let dir = tempfile::tempdir().unwrap();
    write_one_node_document(dir.path(), "busy-a", "test.blocking.ensure", "alpha");
    write_one_node_document(dir.path(), "busy-b", "test.blocking.ensure", "beta");

    let (blocking_tool, release) = common::BlockingTool::new();
    let mut catalog = willikins_core::Catalog::new(willikins_types::registry());
    catalog.insert(Arc::new(blocking_tool)).unwrap();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let first = butler
        .plan(
            wf("busy-a"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("busy-a plans");
    let second = butler
        .plan(
            wf("busy-b"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("busy-b plans");

    let handle = butler
        .apply(first.plan_id, common::principal("agent"))
        .expect("the first run starts");
    let err = butler
        .apply(second.plan_id, common::principal("agent"))
        .expect_err("a run is in progress");
    assert!(matches!(err, ButlerError::RunInProgress { .. }), "{err:?}");

    let refused = journal.lock().unwrap().entries().iter().any(|entry| {
        matches!(
            &entry.event,
            Event::ApplyRefused { plan_id, reason, .. }
                if *plan_id == second.plan_id
                    && matches!(
                        reason,
                        willikins_journal::ApplyRefusedReason::RunInProgress { run_id }
                            if *run_id == handle.run_id
                    )
        )
    });
    assert!(
        refused,
        "a RunInProgress refusal must be journaled as ApplyRefused"
    );

    release.send(()).expect("releasing the first run");
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, willikins_journal::RunState::Succeeded, "{run:?}");
}
