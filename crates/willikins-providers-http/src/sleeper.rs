//! An injectable delay, so retry tests never actually wait.

use std::sync::Arc;
use std::time::Duration;

/// Something that can wait `duration` before returning. The production
/// implementation ([`RealSleeper`]) really sleeps; a test can inject one
/// that records the requested durations and returns immediately.
pub trait Sleeper: Send + Sync {
    /// Wait for `duration`.
    fn sleep(&self, duration: Duration);
}

/// [`Sleeper`] that really sleeps, via [`std::thread::sleep`].
#[derive(Debug, Default, Clone, Copy)]
pub struct RealSleeper;

impl Sleeper for RealSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// The default sleeper every [`crate::http::Http`] is built with, shared
/// rather than reallocated per client.
#[must_use]
pub fn real() -> Arc<dyn Sleeper> {
    Arc::new(RealSleeper)
}
