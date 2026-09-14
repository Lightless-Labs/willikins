//! [`JournalObserver`]: an [`willikins_core::ApplyObserver`] that journals
//! `apply`'s own [`willikins_core::ApplyEvent`]s as they happen, and
//! [`run_and_journal`]/[`continue_run_and_journal`], which wrap a whole
//! `apply` call with the right events around it.
//!
//! # `Append`: why `JournalObserver` is not generic over [`Journal`] directly
//!
//! A [`Journal`]'s replay views (`entries`, `plan`, `runs`, ...) borrow
//! `&self`, which cannot be reconstructed through a `MutexGuard` that
//! itself borrows a shared `Mutex` -- so a caller that needs to *share* a
//! journal across threads (`willikins-server`'s `Butler`, task 10a, whose
//! background run thread journals every node while the main thread might
//! be polling `run(run_id)` at the same time) cannot hold one lock for a
//! whole `apply` call the way [`run_and_journal`] holds a plain `&mut J`
//! for one: that would make every other caller of the journal block for
//! as long as the run takes. [`Append`] is the narrow interface
//! `JournalObserver` actually needs -- one method, called once per event,
//! each call free to take and release its own lock. `&mut J` for any
//! [`Journal`] implements it (so every existing single-threaded caller of
//! this module is unaffected), and so does
//! `Arc<Mutex<dyn Journal + Send>>` (locking per call rather than for the
//! whole observer's lifetime), which is what `Butler` hands to
//! [`continue_run_and_journal`] before moving it into a run thread.

use std::sync::{Arc, Mutex, PoisonError};

use willikins_core::{
    Applied, ApplyError, ApplyEvent, ApplyObserver, DriftKind, NodeStatus, PlanError,
};

use crate::event::{ApplyRefusedReason, DriftReasonKind, Event, Outcome};
use crate::ids::{PlanId, RunId};
use crate::journal::Journal;
use crate::redacted::Redacted;
use crate::{Entry, JournalError, PrincipalId};

/// What [`JournalObserver`] needs from wherever it appends to: one
/// event in, one durably-recorded [`Entry`] (or a [`JournalError`]) out.
/// See the module docs for why this is narrower than [`Journal`] itself.
pub trait Append {
    /// Append `event`.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError`] if `event` could not be durably recorded.
    fn append(&mut self, event: Event) -> Result<Entry, JournalError>;
}

/// Any `&mut` reference to a [`Journal`] appends through it directly --
/// this is what every existing single-threaded caller in this crate (and
/// its tests) already passes, unaffected by `JournalObserver`'s move to
/// the narrower [`Append`] bound.
impl<J: Journal + ?Sized> Append for &mut J {
    fn append(&mut self, event: Event) -> Result<Entry, JournalError> {
        Journal::append(*self, event)
    }
}

/// A shared, lockable journal appends by locking for exactly the one
/// call -- never for a whole observer's lifetime, so a run thread holding
/// one of these never blocks a concurrent read of the same journal (see
/// the module docs). A poisoned lock (some other thread panicked while
/// holding it) is recovered rather than propagated: an audit trail that
/// stops accepting writes because of an unrelated panic elsewhere would
/// lose more than it protects.
impl Append for Arc<Mutex<dyn Journal + Send>> {
    fn append(&mut self, event: Event) -> Result<Entry, JournalError> {
        let mut guard = self.lock().unwrap_or_else(PoisonError::into_inner);
        Journal::append(&mut *guard, event)
    }
}

/// Journals each [`willikins_core::ApplyEvent`] `apply` reports as
/// `Event::NodeStarted`/`Event::NodeFinished`, so a run that dies
/// mid-way — the process is killed, the machine loses power — leaves a
/// truthful partial record: every instance journaled before the crash
/// really was started (and, for a `NodeFinished`, really did finish) with
/// exactly the status recorded, because each event is written and
/// `sync_data`-flushed before `apply` moves on to the next instance.
///
/// `RunStarted` itself is journaled lazily, on the first event this
/// observer actually sees (see [`Self::ensure_started`]) rather than
/// unconditionally before `apply` runs at all -- *unless* it was built
/// with [`Self::for_existing_run`], for a caller (`willikins-server`'s
/// `Butler`) that already journaled `RunStarted` itself, synchronously,
/// before this observer (and the run it watches) ever existed: `apply`'s
/// rules 1 and 2 (`ApprovalRequired`, a re-plan failure, `Drift`) refuse
/// *before* minting a `SinkToken` or touching a provider, so no instance
/// is ever attempted and no `ApplyEvent` ever reaches this observer.
/// Journaling `RunStarted` in that case would be false — nothing ran —
/// and would wrongly mark the plan `applied` in
/// [`crate::journal::PlanRecord`], permanently blocking a later,
/// legitimate `apply` of the same plan with `AlreadyApplied`.
/// [`run_and_journal`] is what actually decides, from `apply`'s returned
/// error variant, whether the run reached this point at all; a caller
/// using [`Self::for_existing_run`] has already made that decision by the
/// time this observer exists.
pub struct JournalObserver<A: Append> {
    journal: A,
    run_id: RunId,
    plan_id: PlanId,
    principal: PrincipalId,
    started: bool,
    /// The first journaling failure seen, if any. `ApplyObserver::on`
    /// returns `()` — it cannot propagate a `Result` — so a failure here
    /// is stashed and surfaced by [`run_and_journal`] once the underlying
    /// `apply` call itself returns, alongside its own result, rather than
    /// silently swallowed. If this is ever set, `apply`'s run continued
    /// regardless (it has no way to know); the caller decides what a
    /// half-journaled run means for it.
    error: Option<JournalError>,
}

impl<A: Append> JournalObserver<A> {
    /// Journal the run `run_id` (of plan `plan_id`, started by
    /// `principal`) into `journal`, lazily: `RunStarted` is appended on
    /// this observer's first event, not before.
    pub fn new(journal: A, run_id: RunId, plan_id: PlanId, principal: PrincipalId) -> Self {
        Self {
            journal,
            run_id,
            plan_id,
            principal,
            started: false,
            error: None,
        }
    }

    /// Journal the run `run_id` (of plan `plan_id`, started by
    /// `principal`) into `journal`, whose `RunStarted` the caller has
    /// *already* appended: this observer's `on` never appends one, so
    /// [`Self::for_existing_run`] can never produce the duplicate
    /// `RunStarted` replay refuses (`crate::journal::fold`'s "the first
    /// record of an id is the only one" rule).
    #[must_use]
    pub fn for_existing_run(
        journal: A,
        run_id: RunId,
        plan_id: PlanId,
        principal: PrincipalId,
    ) -> Self {
        Self {
            journal,
            run_id,
            plan_id,
            principal,
            started: true,
            error: None,
        }
    }

    /// Whether `RunStarted` has been journaled (by this observer, or
    /// already true when it was built with [`Self::for_existing_run`]).
    #[must_use]
    pub fn started(&self) -> bool {
        self.started
    }

    /// Consume this observer, returning the first journaling failure it
    /// saw, if any.
    #[must_use]
    pub fn into_error(self) -> Option<JournalError> {
        self.error
    }

    /// Journal `RunStarted` if this is the first event of the run and one
    /// was not already journaled by the caller (see
    /// [`Self::for_existing_run`]).
    fn ensure_started(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let result = self.journal.append(Event::RunStarted {
            run_id: self.run_id,
            plan_id: self.plan_id,
            principal: self.principal.clone(),
        });
        if self.error.is_none() {
            self.error = result.err();
        }
    }

    fn record(&mut self, event: Event) {
        self.ensure_started();
        let result = self.journal.append(event);
        if self.error.is_none() {
            self.error = result.err();
        }
        // Once one append has failed, later events are still attempted
        // (a transient failure might not recur, and each later event is
        // still worth its own attempt) but never overwrite the first
        // failure: that is the one that first made this run's journal
        // incomplete, and the one a caller needs to see.
    }
}

impl<A: Append> ApplyObserver for JournalObserver<A> {
    fn on(&mut self, event: ApplyEvent) {
        let event = match event {
            ApplyEvent::NodeStarted {
                node,
                instance,
                inputs,
            } => Event::NodeStarted {
                run_id: self.run_id,
                node,
                instance,
                inputs: Redacted::from(&inputs),
            },
            ApplyEvent::NodeFinished {
                node,
                instance,
                status,
                outputs,
            } => {
                let error = match &status {
                    NodeStatus::Failed { error } => Some(error.clone()),
                    _ => None,
                };
                Event::NodeFinished {
                    run_id: self.run_id,
                    node,
                    instance,
                    status,
                    outputs: Redacted::from(&outputs),
                    error,
                }
            }
        };
        self.record(event);
    }
}

/// A [`PlanError`]'s own internally tagged `kind`, and nothing else of
/// it.
///
/// `PlanError` serializes as `{"kind": "<Variant>", ...the variant's own
/// fields}` and declares no variant field named `kind` (pinned by
/// `willikins-core`'s `tests/plan_error_serde.rs`), so the `kind` member
/// is always a serde variant name -- a fixed identifier from a closed
/// set, never a value. Some of the sibling fields *are*
/// [`willikins_core::Value`]s, which is exactly why only the tag is taken
/// and the error itself goes back to the caller instead.
fn plan_error_kind(error: &PlanError) -> String {
    serde_json::to_value(error)
        .ok()
        .and_then(|json| {
            json.get("kind")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        // Unreachable while `PlanError` stays internally tagged; a
        // placeholder rather than a panic, because refusing to journal a
        // refusal would be the worse failure of the two.
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Strip [`DriftKind`] down to [`DriftReasonKind`]: same shape, no
/// planned or observed value.
fn drift_reason_kind(kind: &DriftKind) -> DriftReasonKind {
    match kind {
        DriftKind::Instance { .. } => DriftReasonKind::Instance,
        DriftKind::Action { .. } => DriftReasonKind::Action,
        DriftKind::Output { port, .. } => DriftReasonKind::Output { port: port.clone() },
    }
}

/// Run `apply` (via `run`, a closure so this crate never depends on
/// `willikins-core::apply`'s own signature beyond what it returns) and
/// journal the outcome.
///
/// Two shapes, matching `apply`'s own contract:
///
/// - `Err(ApplyError::ApprovalRequired { .. })`,
///   `Err(ApplyError::Plan { .. })` or `Err(ApplyError::Drift { .. })`:
///   `apply` refused at its own rule 1 or rule 2, before minting a
///   `SinkToken` and before touching a provider, so nothing ran. Journals
///   `ApplyRefused` alone — no `RunStarted`, so
///   [`crate::journal::PlanRecord::applied`] stays `None` and the plan
///   can still be applied once its blocker is resolved. These are exactly
///   the three variants `willikins_core::ApplyError`'s own doc names as
///   carrying no partial result.
/// - Everything else (`Ok`, or any `Err` that carries a partial
///   `Applied`): a real run happened, or at least a `SinkToken` was
///   minted for one. Journals `RunStarted` (if the [`JournalObserver`]
///   handed to `run` did not already journal it lazily on its first
///   event) then `RunFinished`.
///
/// Returns `apply`'s own result unchanged, the run's id (minted here,
/// needed by a caller building a `RunHandle`), and any journaling
/// failure (from `RunStarted`, `ApplyRefused`, `RunFinished`, or a
/// per-instance event) alongside it rather than in place of it, since
/// the underlying run's own outcome is never less true for the journal
/// having trouble recording it.
pub fn run_and_journal<J, F>(
    journal: &mut J,
    principal: PrincipalId,
    plan_id: PlanId,
    run: F,
) -> (Result<Applied, ApplyError>, RunId, Option<JournalError>)
where
    J: Journal,
    F: FnOnce(&mut dyn ApplyObserver) -> Result<Applied, ApplyError>,
{
    let run_id = RunId::new();
    // An explicit reborrow (`&mut *journal`, not `journal`): passing the
    // `&mut J` parameter by value into a generic `A: Append` position
    // moves it outright (unlike a concrete `&mut J` parameter type,
    // Rust's implicit-reborrow rule does not kick in through generic
    // inference), which would leave nothing for this function's own
    // later `journal.append(...)` calls below.
    let mut observer = JournalObserver::new(&mut *journal, run_id, plan_id, principal.clone());
    let result = run(&mut observer);
    let started = observer.started();
    let mut journal_error = observer.into_error();

    // The three errors `apply` reports without having run anything (its
    // own rules 1 and 2, both before the `SinkToken` is minted): each is
    // journaled as an `ApplyRefused` on its own, and no `RunStarted`, so
    // the plan stays applicable.
    let refusal = match &result {
        Err(ApplyError::ApprovalRequired { .. }) => Some(ApplyRefusedReason::ApprovalRequired),
        Err(ApplyError::Plan { error }) => Some(ApplyRefusedReason::PlanFailed {
            error_kind: plan_error_kind(error),
        }),
        Err(ApplyError::Drift {
            node,
            instance,
            kind,
        }) => Some(ApplyRefusedReason::Drift {
            node: node.clone(),
            instance: instance.clone(),
            kind: drift_reason_kind(kind),
        }),
        Ok(_) | Err(_) => None,
    };
    if let Some(reason) = refusal {
        debug_assert!(
            !started,
            "no ApplyEvent can precede a refusal that ran nothing"
        );
        if let Err(err) = journal.append(Event::ApplyRefused {
            plan_id,
            principal,
            reason,
        }) && journal_error.is_none()
        {
            journal_error = Some(err);
        }
        return (result, run_id, journal_error);
    }

    if !started
        && let Err(err) = journal.append(Event::RunStarted {
            run_id,
            plan_id,
            principal: principal.clone(),
        })
        && journal_error.is_none()
    {
        journal_error = Some(err);
    }

    let outcome = match &result {
        Ok(applied) => Outcome::Succeeded {
            outputs: Redacted::from(&applied.outputs),
        },
        Err(error) => Outcome::Failed {
            error: Redacted::from(error),
        },
    };
    if let Err(err) = journal.append(Event::RunFinished { run_id, outcome })
        && journal_error.is_none()
    {
        journal_error = Some(err);
    }

    (result, run_id, journal_error)
}

/// Like [`run_and_journal`], but for a run whose `RunStarted` the caller
/// has *already* appended (with this exact `run_id`), synchronously,
/// before deciding to run anything at all -- `willikins-server`'s
/// `Butler` (task 10a), which performs its own approval, window, and
/// drift checks before ever spawning the thread that calls this.
///
/// Because `RunStarted` is already recorded, every one of `apply`'s
/// outcomes here is a genuine run outcome, not a pre-write refusal: even
/// `ApprovalRequired`, `Plan`, or `Drift` (which [`run_and_journal`]
/// treats as "nothing ran" and journals as `ApplyRefused`) means this run
/// failed, because a plan whose `RunStarted` is on record is already
/// [`crate::journal::PlanRecord::applied`] and cannot be un-applied by a
/// later, cleaner attempt -- there is no refusal branch here at all,
/// unlike [`run_and_journal`]'s.
///
/// `journal` must be cheaply [`Clone`] (an `Arc<Mutex<dyn Journal +
/// Send>>` is): one clone is moved into the [`JournalObserver`] this
/// builds, appending each per-instance event as it happens, and the
/// original is kept to append the final `RunFinished` once `run`
/// returns.
///
/// Returns `apply`'s own result unchanged and any journaling failure
/// (from a per-instance event or the final `RunFinished`) alongside it,
/// the same way [`run_and_journal`] does.
pub fn continue_run_and_journal<A, F>(
    mut journal: A,
    principal: PrincipalId,
    plan_id: PlanId,
    run_id: RunId,
    run: F,
) -> (Result<Applied, ApplyError>, Option<JournalError>)
where
    A: Append + Clone,
    F: FnOnce(&mut dyn ApplyObserver) -> Result<Applied, ApplyError>,
{
    let observer = JournalObserver::for_existing_run(journal.clone(), run_id, plan_id, principal);
    let mut observer = observer;
    let result = run(&mut observer);
    let mut journal_error = observer.into_error();

    let outcome = match &result {
        Ok(applied) => Outcome::Succeeded {
            outputs: Redacted::from(&applied.outputs),
        },
        Err(error) => Outcome::Failed {
            error: Redacted::from(error),
        },
    };
    if let Err(err) = journal.append(Event::RunFinished { run_id, outcome })
        && journal_error.is_none()
    {
        journal_error = Some(err);
    }

    (result, journal_error)
}
