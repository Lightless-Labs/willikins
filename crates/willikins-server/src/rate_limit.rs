//! A per-principal, clock-driven rate limiter: at most `capacity` calls in
//! any trailing 60-second window. See the plan's "Rate limits" and review
//! resolution 8: `plan` gets its own bucket (default 10/minute),
//! `describe` and `validate` share a second one (default 60/minute), each
//! keyed by [`PrincipalId`] so one principal looping never starves
//! another's budget.
//!
//! **Why not `apply`, `approve`, `reject`, `run`, `runs`, or a list
//! operation**: `apply` is already bounded by the single-apply lock (one
//! run at a time, full stop) and by the journal's own durability; a
//! decision or a view read touches no live provider. This limiter exists
//! to protect the org's shared GitHub/Doppler budgets from a looping
//! agent's `plan` calls (which read live state) and `describe`/`validate`
//! calls (cheap, but still unbounded without this) -- not to throttle
//! operations that already cannot run more than one at a time or that
//! never leave the process.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use willikins_journal::{Clock, PrincipalId};

const WINDOW: Duration = Duration::from_secs(60);

/// A sliding 60-second window of call timestamps per [`PrincipalId`],
/// bounded to `capacity` entries. "Refilling with the clock" (the plan's
/// own words) falls out naturally: an entry more than 60 seconds old is
/// dropped the next time that principal is checked, which is exactly a
/// token becoming available again.
pub struct RateLimiter {
    capacity: u32,
    clock: Arc<dyn Clock>,
    calls: Mutex<HashMap<PrincipalId, VecDeque<willikins_journal::Timestamp>>>,
}

impl RateLimiter {
    /// A limiter allowing `capacity` calls per principal per rolling
    /// minute, reading "now" from `clock`.
    #[must_use]
    pub fn new(capacity: u32, clock: Arc<dyn Clock>) -> Self {
        Self {
            capacity,
            clock,
            calls: Mutex::new(HashMap::new()),
        }
    }

    /// Record a call attempt for `principal`. `Ok(())` and the call is
    /// counted; `Err(retry_after_seconds)` and it is not (a refused call
    /// never consumes a slot -- only a call that is actually let through
    /// does), naming at least how many seconds until this principal's
    /// oldest counted call ages out of the window.
    pub fn check(&self, principal: &PrincipalId) -> Result<(), u64> {
        let now = self.clock.now();
        let mut calls = self.calls.lock().unwrap_or_else(PoisonError::into_inner);
        let window = calls.entry(principal.clone()).or_default();

        while let Some(oldest) = window.front() {
            if elapsed(*oldest, now) >= WINDOW {
                window.pop_front();
            } else {
                break;
            }
        }

        if window.len() >= self.capacity as usize {
            let oldest = *window
                .front()
                .unwrap_or_else(|| unreachable!("len >= capacity > 0 implies a front entry"));
            let waited = elapsed(oldest, now);
            let remaining = WINDOW.saturating_sub(waited);
            // Round up: a caller that retries at exactly `remaining`
            // seconds could still land inside the window by a fraction of
            // a second, so under-promising the wait is never safe.
            let retry_after_seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
            return Err(retry_after_seconds.max(1));
        }

        window.push_back(now);
        Ok(())
    }
}

fn elapsed(from: willikins_journal::Timestamp, to: willikins_journal::Timestamp) -> Duration {
    (*to.as_datetime() - *from.as_datetime())
        .to_std()
        .unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_journal::ManualClock;

    fn principal(name: &str) -> PrincipalId {
        PrincipalId::parse(name).unwrap()
    }

    fn clock() -> Arc<ManualClock> {
        Arc::new(ManualClock::new(
            willikins_journal::Timestamp::parse("2026-09-14T00:00:00+00:00").unwrap(),
        ))
    }

    #[test]
    fn the_capacity_th_call_succeeds_and_the_next_is_refused() {
        let limiter = RateLimiter::new(3, clock());
        let p = principal("agent");
        assert!(limiter.check(&p).is_ok());
        assert!(limiter.check(&p).is_ok());
        assert!(limiter.check(&p).is_ok());
        assert!(limiter.check(&p).is_err());
    }

    #[test]
    fn a_different_principal_has_its_own_bucket() {
        let limiter = RateLimiter::new(1, clock());
        assert!(limiter.check(&principal("agent-a")).is_ok());
        assert!(limiter.check(&principal("agent-b")).is_ok());
        assert!(limiter.check(&principal("agent-a")).is_err());
    }

    #[test]
    fn the_bucket_refills_once_the_window_elapses() {
        let clock = clock();
        let limiter = RateLimiter::new(1, clock.clone() as Arc<dyn Clock>);
        let p = principal("agent");
        assert!(limiter.check(&p).is_ok());
        assert!(limiter.check(&p).is_err());
        clock.advance(WINDOW + Duration::from_secs(1));
        assert!(limiter.check(&p).is_ok(), "the window elapsed; a slot should be free");
    }

    #[test]
    fn a_refused_call_reports_a_positive_retry_after() {
        let limiter = RateLimiter::new(1, clock());
        let p = principal("agent");
        assert!(limiter.check(&p).is_ok());
        let retry_after = limiter.check(&p).expect_err("the second call is over capacity");
        assert!(retry_after > 0);
        assert!(retry_after <= 60);
    }

    #[test]
    fn a_refused_call_does_not_consume_a_slot() {
        let clock = clock();
        let limiter = RateLimiter::new(1, clock.clone() as Arc<dyn Clock>);
        let p = principal("agent");
        assert!(limiter.check(&p).is_ok());
        assert!(limiter.check(&p).is_err());
        assert!(limiter.check(&p).is_err(), "still refused: the failed call above must not have freed a slot");
    }
}
