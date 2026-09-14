//! [`MemoryJournal`]: the same [`Journal`] trait over a plain `Vec`, for
//! tests that do not need a real file.

use std::sync::Arc;

use crate::JournalError;
use crate::clock::{Clock, SystemClock};
use crate::event::{Entry, Event};
use crate::journal::Journal;
use crate::timestamp_clamp::clamped;

/// An in-memory [`Journal`]. Never persists anything; every entry is
/// gone once the value is dropped. Shares `append`'s clock-clamping rule
/// and every replay view with [`crate::FileJournal`] (the views are the
/// trait's own default methods).
pub struct MemoryJournal {
    entries: Vec<Entry>,
    next_seq: u64,
    clock: Arc<dyn Clock>,
}

impl MemoryJournal {
    /// An empty journal reading the system clock, `seq` starting at 1
    /// (never 0: a derived `#[derive(Default)]` would have given
    /// `next_seq: 0`, one lower than every real journal's first entry, so
    /// `Default` is implemented by hand below instead, in terms of this).
    #[must_use]
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }

    /// An empty journal reading `clock` instead of the system clock --
    /// what a test uses to make this journal's own timestamps agree with
    /// whatever else (a `willikins-server` `Butler`) is reading the same
    /// clock.
    #[must_use]
    pub fn with_clock(clock: Arc<dyn Clock>) -> Self {
        Self {
            entries: Vec::new(),
            next_seq: 1,
            clock,
        }
    }
}

impl Default for MemoryJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MemoryJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryJournal")
            .field("entries", &self.entries)
            .field("next_seq", &self.next_seq)
            .finish_non_exhaustive()
    }
}

impl Journal for MemoryJournal {
    fn append(&mut self, event: Event) -> Result<Entry, JournalError> {
        let now = self.clock.now();
        let at = match self.entries.last() {
            Some(last) => clamped(now, last.at),
            None => now,
        };
        let entry = Entry {
            seq: self.next_seq,
            at,
            event,
        };
        self.next_seq += 1;
        self.entries.push(entry.clone());
        Ok(entry)
    }

    fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;

    fn server_started() -> Event {
        Event::ServerStarted {
            version: "0.1.0".to_string(),
            workflows_dir: "/workflows".to_string(),
            workflow_hashes: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn append_assigns_contiguous_seq() {
        let mut journal = MemoryJournal::new();
        assert_eq!(journal.append(server_started()).unwrap().seq, 1);
        assert_eq!(journal.append(server_started()).unwrap().seq, 2);
        assert_eq!(journal.entries().len(), 2);
    }

    #[test]
    fn with_clock_stamps_entries_from_the_given_clock() {
        let start = crate::Timestamp::parse("2026-09-13T00:00:00+00:00").unwrap();
        let clock = Arc::new(crate::clock::ManualClock::new(start));
        let mut journal = MemoryJournal::with_clock(clock.clone());
        let entry = journal.append(server_started()).unwrap();
        assert_eq!(entry.at, start);
        clock.advance(std::time::Duration::from_secs(3600));
        let second = journal.append(server_started()).unwrap();
        assert_eq!(
            second.at,
            crate::Timestamp::parse("2026-09-13T01:00:00+00:00").unwrap()
        );
    }
}
