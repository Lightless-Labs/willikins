//! [`ReplayedJournal`] and [`replay`]: a read-only, lock-free snapshot of
//! a journal file, for a caller that must not (or cannot) contend for
//! the exclusive lock a live [`crate::FileJournal`] holds on the same
//! path -- `willikins-cli`'s `runs`/`run` commands (task 11), which need
//! to read a running server's journal from outside the process that
//! holds it open.
//!
//! [`replay`] takes **no lock at all**, shared or exclusive: a shared
//! (`flock(LOCK_SH)`) lock would itself be refused with `WouldBlock`
//! while a [`crate::FileJournal`] holds the exclusive lock the acceptance
//! test requires this to work alongside, so "no lock" is the only choice
//! that satisfies "replay succeeds while a `FileJournal` holds the lock
//! on the same path." Safety without a lock comes from how
//! [`crate::FileJournal::append`] itself writes: one whole line, then
//! `sync_data`, before returning -- so a reader that lands mid-write
//! either sees the write in full or not at all, and the one case that
//! *can* straddle a write (this reader running concurrently with an
//! append still landing) is exactly the truncated-last-line shape
//! [`crate::file::replay`] already refuses, the same way an unclean
//! process exit would. This function is a point-in-time snapshot: it
//! never observes anything appended after it read the file.
//!
//! [`replay`] validates the file exactly as [`crate::FileJournal::open`]
//! does, because both call the same private [`crate::file::replay`]
//! helper: the same [`crate::JournalError`] variant, for the same reason,
//! naming the same line, for a malformed, truncated, or out-of-order
//! file.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use crate::JournalError;
use crate::event::Entry;
use crate::file;
use crate::ids::{PlanId, RunId};
use crate::journal::{self, PlanRecord, RunRecord};

/// A journal file's entries, replayed once by [`replay`] and held in
/// memory -- the read-only counterpart to [`crate::FileJournal`].
///
/// Implements neither [`crate::Journal`] (which requires `append`, with
/// no way to refuse it at the type level -- a `ReplayedJournal` that
/// implemented `Journal` would need some `append` body, even one that
/// always errors, which is a runtime refusal rather than the absence
/// this type is meant to guarantee) nor [`crate::Append`] (implemented
/// only for `&mut J: Journal` and `Arc<Mutex<dyn Journal + Send>>`,
/// neither of which this type is). There is no way to write through a
/// `ReplayedJournal` at all, by construction, not convention.
#[derive(Debug, Clone)]
pub struct ReplayedJournal {
    entries: Vec<Entry>,
}

impl ReplayedJournal {
    /// Every entry recorded so far, in append order, as of when
    /// [`replay`] read the file. See [`crate::Journal::entries`].
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// See [`crate::Journal::pending_approvals`].
    #[must_use]
    pub fn pending_approvals(&self) -> Vec<PlanRecord> {
        journal::pending_approvals_view(&self.entries)
    }

    /// See [`crate::Journal::plan`].
    #[must_use]
    pub fn plan(&self, plan_id: &PlanId) -> Option<PlanRecord> {
        journal::plan_view(&self.entries, plan_id)
    }

    /// See [`crate::Journal::runs`].
    #[must_use]
    pub fn runs(&self) -> Vec<RunRecord> {
        journal::runs_view(&self.entries)
    }

    /// See [`crate::Journal::run`].
    #[must_use]
    pub fn run(&self, run_id: &RunId) -> Option<RunRecord> {
        journal::run_view(&self.entries, run_id)
    }
}

/// Replay the journal file at `path` without taking any lock and without
/// creating, truncating, or otherwise modifying it -- unlike
/// [`crate::FileJournal::open`], which creates the file if it is absent
/// and holds an exclusive lock on it for its own lifetime. See the
/// module docs for why no lock is the right choice here, and what makes
/// it safe.
///
/// # Errors
///
/// [`JournalError::Io`] if `path` does not exist or cannot be opened for
/// reading; [`JournalError::Corrupt`] for exactly the reasons
/// [`crate::FileJournal::open`] reports it (invalid JSON, a
/// non-contiguous `seq`, a timestamp earlier than the entry before it, a
/// truncated final line, or a duplicate `PlanRecorded`/`RunStarted`/
/// `RunFinished`/`NodeFinished`). Never returns [`JournalError::Locked`]:
/// this function takes no lock, so it cannot be refused one.
pub fn replay(path: impl AsRef<Path>) -> Result<ReplayedJournal, JournalError> {
    let path: PathBuf = path.as_ref().to_path_buf();
    let mut handle = OpenOptions::new()
        .read(true)
        .open(&path)
        .map_err(|source| JournalError::Io {
            path: path.clone(),
            source,
        })?;
    let entries = file::replay(&mut handle, &path)?;
    Ok(ReplayedJournal { entries })
}
