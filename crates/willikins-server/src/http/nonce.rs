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

    /// Consume `plan_id`'s nonce: returns whether `presented` matched the
    /// nonce this plan had outstanding *and* it was issued no more than
    /// `window` ago, removing it only when it matched.
    ///
    /// **Compare, then remove** -- changed by adversarial pass 2, which
    /// found that removing unconditionally let one request destroy a
    /// nonce it had not proved it knew. A nonce posted against the wrong
    /// plan (`POST /approvals/{B}` carrying plan A's nonce) burned B's,
    /// so the approver's own pending decision for B stopped working until
    /// they reloaded the page. Nothing was bought by that: the nonce is
    /// 32 bytes from the system CSPRNG, so there is no guessing attack to
    /// slow down, and the case where burning would matter -- a forged
    /// cross-site POST -- never reaches this function, because the
    /// `Origin`/`Referer` check in `crate::http::approvals` refuses it
    /// first. What it cost was real: the approval page is the only
    /// out-of-band decision channel this milestone has, and a stray or
    /// stale same-origin POST could deny it.
    ///
    /// Single-use is unchanged: a nonce that *does* match is removed, so
    /// replaying it fails, and a fresh `GET /approvals` reissues (see
    /// [`Self::issue`]).
    pub(crate) fn consume(
        &self,
        plan_id: PlanId,
        presented: &str,
        now: Timestamp,
        window: Duration,
    ) -> bool {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some((nonce, issued_at)) = entries.get(&plan_id) else {
            return false;
        };
        // Constant-time only in the sense that matters here: the nonce is
        // not a long-lived secret and this comparison is behind Basic
        // authentication and the origin check, so a plain equality is
        // enough -- but the entry is left in place unless it matched.
        if nonce != presented {
            return false;
        }
        let fresh = elapsed(*issued_at, now) <= window;
        entries.remove(&plan_id);
        fresh
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

    /// Adversarial pass 2's decision, pinned: a wrong guess is refused
    /// and leaves the real nonce usable. See [`NonceStore::consume`]'s
    /// own doc for why burning it bought nothing and cost the approval
    /// path.
    #[test]
    fn a_wrong_nonce_is_refused_and_leaves_the_real_one_usable() {
        let store = NonceStore::new();
        let plan_id = PlanId::new();
        let nonce = store.issue(plan_id, now());
        assert!(!store.consume(
            plan_id,
            "not-the-real-nonce",
            now(),
            Duration::from_secs(60)
        ));
        assert!(store.consume(plan_id, &nonce, now(), Duration::from_secs(60)));
        // Still single-use.
        assert!(!store.consume(plan_id, &nonce, now(), Duration::from_secs(60)));
    }

    /// A nonce issued for one plan, presented against another, refuses
    /// and leaves *both* plans' nonces alone.
    #[test]
    fn a_nonce_presented_against_another_plan_burns_neither() {
        let store = NonceStore::new();
        let plan_a = PlanId::new();
        let plan_b = PlanId::new();
        let nonce_a = store.issue(plan_a, now());
        let nonce_b = store.issue(plan_b, now());
        assert!(!store.consume(plan_b, &nonce_a, now(), Duration::from_secs(60)));
        assert!(store.consume(plan_a, &nonce_a, now(), Duration::from_secs(60)));
        assert!(store.consume(plan_b, &nonce_b, now(), Duration::from_secs(60)));
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
        // An expired nonce is consumed even so: it matched, and leaving
        // it in place would keep an unusable entry alive for the life of
        // the process.
        assert!(!store.consume(plan_id, &nonce, issued, Duration::from_secs(24 * 60 * 60)));
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
