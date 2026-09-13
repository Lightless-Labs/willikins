//! [`MemoryJournal`]: the same [`Journal`] trait over a plain `Vec`, for
//! tests that do not need a real file.

use crate::event::{Entry, Event};
use crate::journal::Journal;
use crate::timestamp_clamp::clamped_now;
use crate::{JournalError, Timestamp};

/// An in-memory [`Journal`]. Never persists anything; every entry is
/// gone once the value is dropped. Shares `append`'s clock-clamping rule
/// and every replay view with [`crate::FileJournal`] (the views are the
/// trait's own default methods).
#[derive(Debug)]
pub struct MemoryJournal {
    entries: Vec<Entry>,
    next_seq: u64,
}

impl MemoryJournal {
    /// An empty journal, `seq` starting at 1 (never 0: a derived
    /// `#[derive(Default)]` would have given `next_seq: 0`, one lower
    /// than every real journal's first entry, so `Default` is
    /// implemented by hand below instead, in terms of this).
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_seq: 1,
        }
    }
}

impl Default for MemoryJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl Journal for MemoryJournal {
    fn append(&mut self, event: Event) -> Result<Entry, JournalError> {
        let at = match self.entries.last() {
            Some(last) => clamped_now(last.at),
            None => Timestamp::now(),
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
}
