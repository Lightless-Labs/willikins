//! [`FileJournal`]: an append-only JSONL file, exclusively locked for its
//! whole lifetime and replayed into memory on open.

use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::clock::{Clock, SystemClock};
use crate::event::{Entry, Event};
use crate::journal::Journal;
use crate::timestamp_clamp::clamped;

/// Why a [`FileJournal`] could not be opened, or could not append.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// The file could not be opened, read, or written for a plain I/O
    /// reason.
    #[error("{path}: {source}")]
    Io {
        /// The journal file's path.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Another live [`FileJournal`] already holds the exclusive lock on
    /// this path.
    #[error("{path}: journal is locked by another process")]
    Locked {
        /// The journal file's path.
        path: PathBuf,
    },
    /// A line failed to replay: invalid JSON, an out-of-order or
    /// non-contiguous sequence number, a timestamp earlier than the
    /// entry before it, or (only ever the very last line) one truncated
    /// mid-write. `line` is 1-based, matching what a text editor would
    /// show.
    #[error("line {line}: {reason}")]
    Corrupt {
        /// The 1-based line number.
        line: usize,
        /// What was wrong with it.
        reason: String,
    },
}

/// An append-only JSONL journal backed by a real file.
///
/// [`FileJournal::open`] takes an exclusive advisory lock
/// ([`fd_lock`]) held for as long as the value lives — a second `open` on
/// the same path fails with [`JournalError::Locked`] while the first is
/// alive, and the lock is released (by the OS, when every file
/// descriptor referencing it closes) as soon as this value is dropped.
/// Every line is replayed into memory at open time; [`Journal::entries`]
/// and the trait's own replay views never touch the file again.
/// [`Journal::append`] writes one line, calls `sync_data`, and only then
/// returns — a write is durable before its caller ever sees the
/// [`Entry`] it produced.
pub struct FileJournal {
    path: PathBuf,
    file: File,
    clock: Arc<dyn Clock>,
    // Holds the flock for this value's lifetime: opened as a `dup` of
    // `file`'s own descriptor (an flock is a property of the *open file
    // description*, which `File::try_clone` shares rather than
    // duplicating, so locking through this handle locks `file` too), with
    // its write guard immediately leaked via `mem::forget` rather than
    // stored. `fd_lock::RwLockWriteGuard` borrows its `RwLock` for a
    // named lifetime, so a struct cannot hold both the lock and the guard
    // it produced together without becoming self-referential; forgetting
    // the guard (whose `Drop` is the only thing that would ever unlock
    // early) sidesteps that, and is safe here precisely because nothing
    // else in this module ever calls `.write()`/`.read()` on `_lock`
    // again — the lock, once taken, is meant to last exactly as long as
    // `_lock`'s own file descriptor does.
    _lock: fd_lock::RwLock<File>,
    entries: Vec<Entry>,
    next_seq: u64,
}

impl std::fmt::Debug for FileJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileJournal")
            .field("path", &self.path)
            .field("entries", &self.entries)
            .field("next_seq", &self.next_seq)
            .finish_non_exhaustive()
    }
}

impl FileJournal {
    /// Open (creating if absent) the journal file at `path`, reading the
    /// system clock.
    ///
    /// # Errors
    ///
    /// Returns [`JournalError::Locked`] if another live `FileJournal`
    /// already holds this path's lock, [`JournalError::Io`] for any other
    /// failure to open or read the file, and [`JournalError::Corrupt`] if
    /// replay finds a gap or non-monotonic timestamp in `seq`, a line
    /// that is not valid JSON, or a final line truncated mid-write (see
    /// the module docs on why that last case is refused rather than
    /// silently dropped).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        Self::open_with_clock(path, Arc::new(SystemClock))
    }

    /// Open (creating if absent) the journal file at `path`, reading
    /// `clock` instead of the system clock -- what a test uses to make
    /// this journal's own timestamps agree with whatever else (a
    /// `willikins-server` `Butler`) is reading the same clock.
    ///
    /// # Errors
    ///
    /// Same as [`Self::open`].
    pub fn open_with_clock(
        path: impl AsRef<Path>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .map_err(|source| JournalError::Io {
                path: path.clone(),
                source,
            })?;

        let lock_fd = file.try_clone().map_err(|source| JournalError::Io {
            path: path.clone(),
            source,
        })?;
        let mut lock = fd_lock::RwLock::new(lock_fd);
        match lock.try_write() {
            Ok(guard) => std::mem::forget(guard),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(JournalError::Locked { path });
            }
            Err(source) => return Err(JournalError::Io { path, source }),
        }

        let entries = replay(&mut file, &path)?;
        let next_seq = entries.last().map_or(1, |entry| entry.seq + 1);

        Ok(Self {
            path,
            file,
            clock,
            _lock: lock,
            entries,
            next_seq,
        })
    }

    /// This journal's file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Journal for FileJournal {
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
        let mut line = serde_json::to_string(&entry)
            .unwrap_or_else(|err| unreachable!("an `Entry` always serializes to JSON: {err}"));
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .map_err(|source| JournalError::Io {
                path: self.path.clone(),
                source,
            })?;
        self.file.sync_data().map_err(|source| JournalError::Io {
            path: self.path.clone(),
            source,
        })?;
        self.next_seq += 1;
        self.entries.push(entry.clone());
        Ok(entry)
    }

    fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

/// Read `file` -- the exact descriptor [`FileJournal`] holds open and
/// locked, seeked back to the start, *not* a fresh handle opened on
/// `path` -- and validate its contents line by line: contiguous `seq`
/// from 1, `at` never going backwards, every line valid JSON matching
/// [`Entry`]'s shape. `path` is carried only for error messages.
///
/// Reading through the held descriptor rather than re-opening `path` is
/// what makes this replay immune to a rename over the path between the
/// lock being taken and this read happening: a `File` and the inode it
/// refers to stay linked once opened, on every platform this workspace
/// targets, regardless of what a later `rename(2)` (or `MoveFileEx` on
/// Windows) does to the name -- unlike a second `open(path)`, which would
/// follow the rename to whatever now sits at that name (see
/// `tests::a_rename_over_the_path_after_the_lock_is_taken_does_not_affect_replay`).
/// `append(true)` is safe to seek on: `O_APPEND` fixes where a *write*
/// lands (always the current end of file) independent of the read/write
/// cursor a `seek` moves, so leaving that cursor at EOF once replay
/// finishes has no effect on `FileJournal::append`'s own writes.
///
/// A file that does not end in `\n` has its final line refused as
/// truncated rather than parsed: `append` only ever writes a line
/// followed by `\n` and then `sync_data`s before returning, so a missing
/// trailing newline can only mean the process died mid-`write_all` —
/// there is no way to tell that case apart from a deliberately truncated
/// (and possibly still "valid-looking" up to the cut) forged line, and
/// silently dropping the tail either way would let a corrupted journal
/// replay as if nothing were wrong. An operator recovering from a crash
/// inspects the file and trims the partial line by hand.
pub(crate) fn replay(file: &mut File, path: &Path) -> Result<Vec<Entry>, JournalError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| JournalError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|source| JournalError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if contents.is_empty() {
        return Ok(Vec::new());
    }
    if !contents.ends_with('\n') {
        let line = contents.matches('\n').count() + 1;
        return Err(JournalError::Corrupt {
            line,
            reason: "truncated: the final line has no trailing newline (a write was likely \
                     interrupted mid-append); trim the partial line by hand to recover"
                .to_string(),
        });
    }

    let mut entries = Vec::new();
    let mut last_at: Option<crate::Timestamp> = None;
    // Every id this file has already recorded. A `PlanId`/`RunId` is
    // minted once (`uuid::Uuid::now_v7`) by whoever calls `plan` or starts
    // a run, so a second `PlanRecorded` or `RunStarted` for one id cannot
    // come from `append`: the file was edited. Refusing here is what makes
    // "no delete, no rewrite" true of the file and not only of the API --
    // a duplicate `PlanRecorded` would otherwise be a rewrite of a plan's
    // recorded identity (its document hash, fingerprint, approval
    // requirement and whether it has already run), and a duplicate
    // `RunStarted` a rewrite of a finished run's outcome.
    // The same holds one event further on: a run finishes once
    // (`RunFinished`) and each of its instances finishes once
    // (`NodeFinished`), so a second of either rewrites an outcome that was
    // already recorded. `crate::journal::fold` independently keeps the
    // first of all four, so a `Journal` that is not this one refuses to be
    // rewritten too.
    let mut plan_ids: std::collections::HashSet<crate::PlanId> = std::collections::HashSet::new();
    let mut run_ids: std::collections::HashSet<crate::RunId> = std::collections::HashSet::new();
    let mut finished_runs: std::collections::HashSet<crate::RunId> =
        std::collections::HashSet::new();
    let mut finished_nodes: std::collections::HashSet<(crate::RunId, String, Option<String>)> =
        std::collections::HashSet::new();
    for (index, line) in contents.lines().enumerate() {
        // 1-based, matching both a line number and the `seq` a
        // contiguous, gap-free journal must carry at this position --
        // the two coincide exactly, so there is no separate counter to
        // keep in step with the loop.
        let line_no = index + 1;
        let expected_seq = line_no as u64;
        let entry: Entry = serde_json::from_str(line).map_err(|err| JournalError::Corrupt {
            line: line_no,
            reason: format!("invalid JSON: {err}"),
        })?;
        if entry.seq != expected_seq {
            return Err(JournalError::Corrupt {
                line: line_no,
                reason: format!("expected seq {expected_seq}, found {}", entry.seq),
            });
        }
        if let Some(previous) = last_at
            && entry.at < previous
        {
            return Err(JournalError::Corrupt {
                line: line_no,
                reason: format!(
                    "timestamp {} is before the previous entry's {previous}",
                    entry.at
                ),
            });
        }
        let duplicate = match &entry.event {
            Event::PlanRecorded { plan_id, .. } if !plan_ids.insert(*plan_id) => {
                Some(format!("a second `plan_recorded` event for plan {plan_id}"))
            }
            Event::RunStarted { run_id, .. } if !run_ids.insert(*run_id) => {
                Some(format!("a second `run_started` event for run {run_id}"))
            }
            Event::RunFinished { run_id, .. } if !finished_runs.insert(*run_id) => {
                Some(format!("a second `run_finished` event for run {run_id}"))
            }
            Event::NodeFinished {
                run_id,
                node,
                instance,
                ..
            } if !finished_nodes.insert((*run_id, node.to_string(), instance.clone())) => Some(
                format!("a second `node_finished` event for {node} in run {run_id}"),
            ),
            _ => None,
        };
        if let Some(what) = duplicate {
            return Err(JournalError::Corrupt {
                line: line_no,
                reason: format!(
                    "{what}: each of these is recorded once per id, so this line was not \
                     written by `append`"
                ),
            });
        }
        last_at = Some(entry.at);
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;

    fn temp_path() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        // Leak the tempdir so the path outlives this function; each test
        // gets its own directory (from a fresh `tempdir()` call) so this
        // never grows unbounded across the process's lifetime in a way
        // that matters for a test binary.
        std::mem::forget(dir);
        path
    }

    fn server_started() -> Event {
        Event::ServerStarted {
            version: "0.1.0".to_string(),
            workflows_dir: "/workflows".to_string(),
            workflow_hashes: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn creates_the_file_if_absent_and_replays_empty() {
        let path = temp_path();
        assert!(!path.exists());
        let journal = FileJournal::open(&path).unwrap();
        assert!(path.exists());
        assert!(journal.entries().is_empty());
    }

    #[test]
    fn append_assigns_contiguous_seq_starting_at_one() {
        let path = temp_path();
        let mut journal = FileJournal::open(&path).unwrap();
        let first = journal.append(server_started()).unwrap();
        let second = journal.append(server_started()).unwrap();
        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
    }

    #[test]
    fn reopening_replays_to_the_same_entries() {
        let path = temp_path();
        {
            let mut journal = FileJournal::open(&path).unwrap();
            journal.append(server_started()).unwrap();
            journal.append(server_started()).unwrap();
        }
        let reopened = FileJournal::open(&path).unwrap();
        assert_eq!(reopened.entries().len(), 2);
        assert_eq!(reopened.entries()[0].seq, 1);
        assert_eq!(reopened.entries()[1].seq, 2);
    }

    #[test]
    fn a_second_open_on_the_same_path_is_refused_while_the_first_is_held() {
        let path = temp_path();
        let _first = FileJournal::open(&path).unwrap();
        let second = FileJournal::open(&path);
        assert!(matches!(second, Err(JournalError::Locked { .. })));
    }

    #[test]
    fn the_lock_releases_when_the_journal_is_dropped() {
        let path = temp_path();
        {
            let _first = FileJournal::open(&path).unwrap();
        }
        assert!(FileJournal::open(&path).is_ok());
    }

    #[test]
    fn a_hand_appended_lower_seq_makes_open_fail_naming_the_line() {
        let path = temp_path();
        {
            let mut journal = FileJournal::open(&path).unwrap();
            journal.append(server_started()).unwrap();
            journal.append(server_started()).unwrap();
        }
        let entry = Entry {
            seq: 1,
            at: crate::Timestamp::now(),
            event: server_started(),
        };
        let mut line = serde_json::to_string(&entry).unwrap();
        line.push('\n');
        {
            use std::io::Write as _;
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(line.as_bytes()).unwrap();
        }
        let result = FileJournal::open(&path);
        match result {
            Err(JournalError::Corrupt { line, reason }) => {
                assert_eq!(line, 3);
                assert!(reason.contains("expected seq 3"));
            }
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn a_hand_truncated_last_line_is_refused_naming_it() {
        let path = temp_path();
        {
            let mut journal = FileJournal::open(&path).unwrap();
            journal.append(server_started()).unwrap();
        }
        // Append a second, deliberately incomplete line (no trailing
        // newline), simulating a crash mid-`write_all`.
        {
            use std::io::Write as _;
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(br#"{"seq":2,"at":"2026-09-13T00:00:00+00:00","event":{"kind":"#)
                .unwrap();
        }
        let result = FileJournal::open(&path);
        match result {
            Err(JournalError::Corrupt { line, reason }) => {
                assert_eq!(line, 2);
                assert!(reason.contains("truncated"));
            }
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn a_malformed_line_is_refused_naming_it() {
        let path = temp_path();
        std::fs::write(&path, "not json at all\n").unwrap();
        let result = FileJournal::open(&path);
        match result {
            Err(JournalError::Corrupt { line, reason }) => {
                assert_eq!(line, 1);
                assert!(reason.contains("invalid JSON"));
            }
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn a_non_monotonic_timestamp_makes_open_fail_naming_the_line() {
        let path = temp_path();
        let first = Entry {
            seq: 1,
            at: crate::Timestamp::parse("2026-09-13T12:00:00+00:00").unwrap(),
            event: server_started(),
        };
        let second = Entry {
            seq: 2,
            // Earlier than `first.at`: the clock stepped backwards.
            at: crate::Timestamp::parse("2026-09-13T11:00:00+00:00").unwrap(),
            event: server_started(),
        };
        let mut contents = serde_json::to_string(&first).unwrap();
        contents.push('\n');
        contents.push_str(&serde_json::to_string(&second).unwrap());
        contents.push('\n');
        std::fs::write(&path, contents).unwrap();

        let result = FileJournal::open(&path);
        match result {
            Err(JournalError::Corrupt { line, reason }) => {
                assert_eq!(line, 2);
                assert!(reason.contains("before"), "{reason}");
            }
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn append_clamps_a_backwards_clock_so_a_reopen_still_succeeds() {
        let path = temp_path();
        // A single entry stamped far in the future: `append`'s own next
        // `Timestamp::now()` read will be *earlier* than this, exactly
        // the clock-stepped-backwards case `clamped_now` exists for.
        let future = crate::Timestamp::parse("2999-01-01T00:00:00+00:00").unwrap();
        let seeded = Entry {
            seq: 1,
            at: future,
            event: server_started(),
        };
        let mut line = serde_json::to_string(&seeded).unwrap();
        line.push('\n');
        std::fs::write(&path, line).unwrap();

        let mut journal = FileJournal::open(&path).unwrap();
        let appended = journal.append(server_started()).unwrap();
        assert_eq!(appended.seq, 2);
        assert!(
            appended.at >= future,
            "append must clamp to at least the previous entry's timestamp"
        );
        drop(journal);

        // If `append` had used a bare `Timestamp::now()` instead, this
        // reopen would refuse with a non-monotonic-timestamp `Corrupt`.
        let reopened = FileJournal::open(&path).unwrap();
        assert_eq!(reopened.entries().len(), 2);
    }

    #[test]
    fn append_is_durable_before_returning_and_survives_a_reopen() {
        let path = temp_path();
        let mut journal = FileJournal::open(&path).unwrap();
        journal.append(server_started()).unwrap();
        drop(journal);
        let reopened = FileJournal::open(&path).unwrap();
        assert_eq!(reopened.entries().len(), 1);
    }

    /// Journal follow-up (`todos/2026-09-14-journal-follow-ups.md`):
    /// replay must read the descriptor [`FileJournal`] already holds
    /// open, not re-open `path` fresh, or a rename over the path between
    /// the lock being taken and replay running would silently replay
    /// whatever now sits at that name instead of what was locked.
    #[test]
    fn replay_reads_the_held_descriptor_not_the_path_so_a_rename_over_it_has_no_effect() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let other_path = dir.path().join("other.jsonl");

        {
            let mut journal = FileJournal::open(&path).unwrap();
            journal
                .append(Event::ServerStarted {
                    version: "0.1.0".to_string(),
                    workflows_dir: "original".to_string(),
                    workflow_hashes: std::collections::BTreeMap::new(),
                })
                .unwrap();
        }
        {
            let mut journal = FileJournal::open(&other_path).unwrap();
            journal
                .append(Event::ServerStarted {
                    version: "0.1.0".to_string(),
                    workflows_dir: "replaced".to_string(),
                    workflow_hashes: std::collections::BTreeMap::new(),
                })
                .unwrap();
        }

        // Both `FileJournal`s above have been dropped (their locks
        // released); open a plain read handle on `path`.
        let mut file = OpenOptions::new().read(true).open(&path).unwrap();

        // Rename the *other* file's content over `path`. `file`'s
        // already-open descriptor keeps referring to the inode it was
        // opened on, not whatever the name `path` now resolves to.
        std::fs::rename(&other_path, &path).unwrap();

        let entries = replay(&mut file, &path).expect("replay reads the held descriptor");
        assert_eq!(entries.len(), 1);
        match &entries[0].event {
            Event::ServerStarted { workflows_dir, .. } => {
                assert_eq!(
                    workflows_dir, "original",
                    "replay must not follow the rename"
                );
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
