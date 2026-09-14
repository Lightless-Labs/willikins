//! A tiny shared helper: an `append`'s own clock read must never go
//! backwards relative to the entry before it, or replay's own
//! non-monotonic-timestamp check ([`crate::FileJournal::open`]) would
//! refuse a journal `append` itself just wrote — e.g. after the system
//! clock steps backwards (an NTP correction). Shared by
//! [`crate::FileJournal`] and [`crate::MemoryJournal`] so both obey the
//! same rule they are each validated against.

use crate::Timestamp;

/// `now`, clamped up to `previous` if it is earlier -- the clock (real or,
/// via [`crate::Clock`], a test's own) has moved backwards since the
/// entry before it.
pub(crate) fn clamped(now: Timestamp, previous: Timestamp) -> Timestamp {
    if now < previous { previous } else { now }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_clamp_when_the_clock_moved_forward() {
        let earlier = Timestamp::parse("2020-01-01T00:00:00+00:00").unwrap();
        let now = Timestamp::now();
        assert_eq!(clamped(now, earlier), now);
    }

    #[test]
    fn clamps_to_previous_when_the_clock_moved_backward() {
        let future = Timestamp::parse("2999-01-01T00:00:00+00:00").unwrap();
        assert_eq!(clamped(Timestamp::now(), future), future);
    }
}
