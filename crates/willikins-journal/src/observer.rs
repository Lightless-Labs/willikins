//! [`JournalObserver`]: an [`willikins_core::ApplyObserver`] that journals
//! `apply`'s own [`willikins_core::ApplyEvent`]s as they happen, and
//! [`run_and_journal`], which wraps a whole `apply` call with the
//! `RunStarted`/`RunFinished` pair around it.

use willikins_core::{Applied, ApplyError, ApplyEvent, ApplyObserver, NodeStatus};

use crate::event::{Event, Outcome};
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
pub struct JournalObserver<'a, J: Journal> {
    journal: &'a mut J,
    run_id: RunId,
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
    /// Journal every event of the run `run_id` into `journal`.
    pub fn new(journal: &'a mut J, run_id: RunId) -> Self {
        Self {
            journal,
            run_id,
            error: None,
        }
    }

    /// Consume this observer, returning the first journaling failure it
    /// saw, if any.
    #[must_use]
    pub fn into_error(self) -> Option<JournalError> {
        self.error
    }

    fn record(&mut self, event: Event) {
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

/// Run `apply` (via `run`, a closure so this crate never depends on
/// `willikins-core::apply`'s own signature beyond what it returns),
/// journaling `RunStarted` before it and `RunFinished` after, with a
/// [`JournalObserver`] journaling every instance in between.
///
/// Returns `apply`'s own result unchanged; a journaling failure (from
/// `RunStarted`, `RunFinished`, or any per-instance event `run` reports
/// through the observer it is handed) is returned alongside it rather
/// than in place of it, since the underlying run's own outcome is never
/// less true for the journal having trouble recording it.
pub fn run_and_journal<J, F>(
    journal: &mut J,
    principal: PrincipalId,
    plan_id: PlanId,
    run: F,
) -> (Result<Applied, ApplyError>, Option<JournalError>)
where
    J: Journal,
    F: FnOnce(&mut dyn ApplyObserver) -> Result<Applied, ApplyError>,
{
    let run_id = RunId::new();
    let mut journal_error = journal
        .append(Event::RunStarted {
            run_id,
            plan_id,
            principal,
        })
        .err();

    let result = {
        let mut observer = JournalObserver::new(journal, run_id);
        let result = run(&mut observer);
        let observer_error = observer.into_error();
        if journal_error.is_none() {
            journal_error = observer_error;
        }
        result
    };

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
