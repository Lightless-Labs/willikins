//! [`Clock`]: where [`crate::MemoryJournal`] and [`crate::FileJournal`]
//! get "now" from, shared with whatever else needs to agree with the
//! journal about the time -- `willikins-server`'s `Butler` (task 10a),
//! specifically, which computes approval and apply windows from the same
//! instants the journal stamped its own entries with. A production
//! journal uses [`SystemClock`]; a test that wants to approve a plan 23
//! hours after `plan` and apply it 30 minutes later, without an actual
//! 23-hour wait, builds a [`ManualClock`] and hands the *same* one to
//! both the journal and the code computing elapsed time -- if they read
//! two different clocks, the windows and the journal's own timestamps can
//! disagree about what "now" is.

use std::sync::Mutex;

use crate::Timestamp;

/// A source of "now", so a journal (and anything that must agree with it
/// about elapsed time) can be handed a fake one in a test.
pub trait Clock: Send + Sync {
    /// The current instant, as this clock sees it.
    fn now(&self) -> Timestamp;
}

/// The real clock: [`Timestamp::now`].
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// A clock a test drives by hand, starting at a given instant and moving
/// only forward: [`Self::advance`] and [`Self::set_at_least`] both
/// saturate to the current reading rather than ever going backwards, the
/// same guarantee [`crate::timestamp_clamp::clamped`] gives the real
/// clock against a stepped-back system clock. This is what lets a test
/// advance the clock by, say, 23 hours and then 30 minutes and have every
/// journal entry appended in between agree with the elapsed time a
/// caller computes from [`Self::now`].
#[derive(Debug)]
pub struct ManualClock(Mutex<Timestamp>);

impl ManualClock {
    /// A clock reading `start`.
    #[must_use]
    pub fn new(start: Timestamp) -> Self {
        Self(Mutex::new(start))
    }

    /// Move this clock forward by `by`. A no-op if `by` would somehow not
    /// advance it (it never can, in practice: [`std::time::Duration`]
    /// cannot be negative).
    pub fn advance(&self, by: std::time::Duration) {
        let mut guard = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let delta = chrono::Duration::from_std(by)
            .unwrap_or_else(|_| unreachable!("a test's advance() is never near i64::MAX millis"));
        let candidate = Timestamp::from_datetime(*guard.as_datetime() + delta);
        if candidate > *guard {
            *guard = candidate;
        }
    }

    /// Set this clock to `at`, unless it already reads a later instant.
    pub fn set_at_least(&self, at: Timestamp) {
        let mut guard = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if at > *guard {
            *guard = at;
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_reads_something_close_to_now() {
        let clock = SystemClock;
        let before = Timestamp::now();
        let read = clock.now();
        let after = Timestamp::now();
        assert!(before <= read && read <= after);
    }

    #[test]
    fn manual_clock_reads_back_its_start() {
        let start = Timestamp::parse("2026-09-13T00:00:00+00:00").unwrap();
        let clock = ManualClock::new(start);
        assert_eq!(clock.now(), start);
    }

    #[test]
    fn manual_clock_advances_by_the_given_amount() {
        let start = Timestamp::parse("2026-09-13T00:00:00+00:00").unwrap();
        let clock = ManualClock::new(start);
        clock.advance(std::time::Duration::from_secs(3600));
        assert_eq!(
            clock.now(),
            Timestamp::parse("2026-09-13T01:00:00+00:00").unwrap()
        );
    }

    #[test]
    fn manual_clock_never_moves_backward() {
        let start = Timestamp::parse("2026-09-13T12:00:00+00:00").unwrap();
        let clock = ManualClock::new(start);
        clock.set_at_least(Timestamp::parse("2026-09-13T00:00:00+00:00").unwrap());
        assert_eq!(clock.now(), start);
    }
}
