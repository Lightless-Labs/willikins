//! [`NonceStore`]: the approvals page's single-use, per-plan CSRF nonce.
//!
//! Review resolution 1: a browser re-attaches cached Basic credentials to
//! any request for the same origin, so a hostile page could `POST
//! /approvals/{plan_id}` on the approver's behalf using nothing but a
//! plain HTML form. The nonce (bound to one plan, single-use, expiring
//! with the approval window) plus the `Origin`/`Referer` check in
//! `crate::http::approvals` are the two defences acceptance test 12
//! requires together; neither alone is enough (a same-origin XSS bypasses
//! the origin check, a leaked nonce bypasses itself without the origin
//! check).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use rand::Rng;
use willikins_journal::{PlanId, Timestamp};

/// In-memory only, by design: the plan's own trust boundary says so
/// ("in-memory nonces are not [durable]; the approval page is reloaded")
/// -- a redeploy invalidates every outstanding nonce, which is correct,
/// since the page must be reloaded to see current state anyway.
pub(crate) struct NonceStore {
    entries: Mutex<HashMap<PlanId, (String, Timestamp)>>,
}

impl NonceStore {
    pub(crate) fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Issue a fresh nonce for `plan_id`, replacing any previous one.
    /// `GET /approvals` calls this every render, so the page's own forms
    /// always carry a nonce that matches what was just issued -- a stale
    /// nonce from an earlier render of the same plan stops working the
    /// moment the page is rendered again, which is the intended
    /// single-use behaviour extended to "single render" rather than
    /// merely "single POST".
    pub(crate) fn issue(&self, plan_id: PlanId, now: Timestamp) -> String {
        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes);
        let mut nonce = String::with_capacity(64);
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(nonce, "{byte:02x}");
        }
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(plan_id, (nonce.clone(), now));
        nonce
    }

    /// Consume `plan_id`'s nonce: removes it unconditionally, whether or
    /// not `presented` matches -- single-use means single-use even under
    /// a failed attempt, so a nonce a hostile page guessed wrong cannot
    /// be retried, and the legitimate one that guess collided with (if
    /// any) is burned too rather than left presentable a second time by
    /// coincidence. Returns whether `presented` matched the nonce this
    /// plan had outstanding and it was issued no more than `window` ago.
    pub(crate) fn consume(
        &self,
        plan_id: PlanId,
        presented: &str,
        now: Timestamp,
        window: Duration,
    ) -> bool {
        let removed = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&plan_id);
        match removed {
            Some((nonce, issued_at)) => nonce == presented && elapsed(issued_at, now) <= window,
            None => false,
        }
    }
}

fn elapsed(since: Timestamp, now: Timestamp) -> Duration {
    (*now.as_datetime() - *since.as_datetime())
        .to_std()
        .unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::parse("2026-09-15T00:00:00+00:00").unwrap()
    }

    #[test]
    fn a_freshly_issued_nonce_is_consumed_once_and_only_once() {
        let store = NonceStore::new();
        let plan_id = PlanId::new();
        let nonce = store.issue(plan_id, now());
        assert!(store.consume(plan_id, &nonce, now(), Duration::from_secs(60)));
        assert!(!store.consume(plan_id, &nonce, now(), Duration::from_secs(60)));
    }

    #[test]
    fn a_wrong_nonce_is_refused_and_still_burns_the_real_one() {
        let store = NonceStore::new();
        let plan_id = PlanId::new();
        let nonce = store.issue(plan_id, now());
        assert!(!store.consume(
            plan_id,
            "not-the-real-nonce",
            now(),
            Duration::from_secs(60)
        ));
        // The real nonce is gone too: a wrong guess still consumes it.
        assert!(!store.consume(plan_id, &nonce, now(), Duration::from_secs(60)));
    }

    #[test]
    fn an_unknown_plan_id_is_refused() {
        let store = NonceStore::new();
        assert!(!store.consume(PlanId::new(), "anything", now(), Duration::from_secs(60)));
    }

    #[test]
    fn a_nonce_older_than_the_window_is_refused() {
        let store = NonceStore::new();
        let plan_id = PlanId::new();
        let issued = now();
        let nonce = store.issue(plan_id, issued);
        let later = Timestamp::from_datetime(*issued.as_datetime() + chrono::Duration::hours(25));
        assert!(!store.consume(plan_id, &nonce, later, Duration::from_secs(24 * 60 * 60)));
    }

    #[test]
    fn reissuing_replaces_the_previous_nonce() {
        let store = NonceStore::new();
        let plan_id = PlanId::new();
        let first = store.issue(plan_id, now());
        let second = store.issue(plan_id, now());
        assert_ne!(first, second);
        assert!(!store.consume(plan_id, &first, now(), Duration::from_secs(60)));
        let second_again = store.issue(plan_id, now());
        assert!(store.consume(plan_id, &second_again, now(), Duration::from_secs(60)));
    }
}
