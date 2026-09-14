//! [`JournalObserver`]: an [`willikins_core::ApplyObserver`] that journals
//! `apply`'s own [`willikins_core::ApplyEvent`]s as they happen, and
//! [`run_and_journal`], which wraps a whole `apply` call with the right
//! events around it.

use willikins_core::{Applied, ApplyError, ApplyEvent, ApplyObserver, DriftKind, NodeStatus};

use crate::event::{ApplyRefusedReason, DriftReasonKind, Event, Outcome};
use crate::ids::{PlanId, RunId};
use crate::journal::Journal;
use crate::redacted::Redacted;
use crate::{JournalError, PrincipalId};

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
/// unconditionally before `apply` runs at all: `apply`'s rules 1 and 2
/// (`ApprovalRequired`, a re-plan failure, `Drift`) refuse *before*
/// minting a `SinkToken` or touching a provider, so no instance is ever
/// attempted and no `ApplyEvent` ever reaches this observer. Journaling
/// `RunStarted` in that case would be false — nothing ran — and would
/// wrongly mark the plan `applied` in [`crate::journal::PlanRecord`],
/// permanently blocking a later, legitimate `apply` of the same plan
/// with `AlreadyApplied`. [`run_and_journal`] is what actually decides,
/// from `apply`'s returned error variant, whether the run reached this
/// point at all.
pub struct JournalObserver<'a, J: Journal> {
    journal: &'a mut J,
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

impl<'a, J: Journal> JournalObserver<'a, J> {
    /// Journal the run `run_id` (of plan `plan_id`, started by
    /// `principal`) into `journal`, lazily.
    pub fn new(journal: &'a mut J, run_id: RunId, plan_id: PlanId, principal: PrincipalId) -> Self {
        Self {
            journal,
            run_id,
            plan_id,
            principal,
            started: false,
            error: None,
        }
    }

    /// Whether `RunStarted` has been journaled yet.
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

    /// Journal `RunStarted` if this is the first event of the run.
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

impl<J: Journal> ApplyObserver for JournalObserver<'_, J> {
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
/// - `Err(ApplyError::ApprovalRequired { .. })` or
///   `Err(ApplyError::Drift { .. })`: `apply` refused before touching a
///   provider, so nothing ran. Journals `ApplyRefused` alone — no
///   `RunStarted`, so [`crate::journal::PlanRecord::applied`] stays
///   `None` and the plan can still be applied once its blocker is
///   resolved.
/// - Everything else (`Ok`, or any other `Err`, including a re-plan
///   failure that `willikins_core::PlanError` cannot name as one of this
///   crate's own [`ApplyRefusedReason`] variants — recorded as a plan
///   gap, not fixed here): a real run happened, or at least a `SinkToken`
///   was minted for one. Journals `RunStarted` (if the [`JournalObserver`]
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
    let mut observer = JournalObserver::new(journal, run_id, plan_id, principal.clone());
    let result = run(&mut observer);
    let started = observer.started();
    let mut journal_error = observer.into_error();

    match &result {
        Err(ApplyError::ApprovalRequired { .. }) => {
            debug_assert!(!started, "no ApplyEvent can precede ApprovalRequired");
            if let Err(err) = journal.append(Event::ApplyRefused {
                plan_id,
                principal,
                reason: ApplyRefusedReason::ApprovalRequired,
            }) && journal_error.is_none()
            {
                journal_error = Some(err);
            }
            return (result, run_id, journal_error);
        }
        Err(ApplyError::Drift {
            node,
            instance,
            kind,
        }) => {
            debug_assert!(!started, "no ApplyEvent can precede Drift");
            if let Err(err) = journal.append(Event::ApplyRefused {
                plan_id,
                principal,
                reason: ApplyRefusedReason::Drift {
                    node: node.clone(),
                    instance: instance.clone(),
                    kind: drift_reason_kind(kind),
                },
            }) && journal_error.is_none()
            {
                journal_error = Some(err);
            }
            return (result, run_id, journal_error);
        }
        _ => {}
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
