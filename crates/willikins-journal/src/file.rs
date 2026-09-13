//! [`FileJournal`]: an append-only JSONL file, exclusively locked for its
//! whole lifetime and replayed into memory on open.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::event::{Entry, Event};
use crate::journal::Journal;
use crate::timestamp_clamp::clamped_now;

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
#[derive(Debug)]
pub struct FileJournal {
    path: PathBuf,
    file: File,
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

impl FileJournal {
    /// Open (creating if absent) the journal file at `path`.
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
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
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

        let entries = replay(&path)?;
        let next_seq = entries.last().map_or(1, |entry| entry.seq + 1);

        Ok(Self {
            path,
            file,
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
        let at = match self.entries.last() {
            Some(last) => clamped_now(last.at),
            None => crate::Timestamp::now(),
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

/// Read `path` fresh (a separate handle from the one `FileJournal` writes
/// through) and validate it line by line: contiguous `seq` from 1, `at`
/// never going backwards, every line valid JSON matching [`Entry`]'s
/// shape. A file that does not end in `\n` has its final line refused as
/// truncated rather than parsed: `append` only ever writes a line
/// followed by `\n` and then `sync_data`s before returning, so a missing
/// trailing newline can only mean the process died mid-`write_all` —
/// there is no way to tell that case apart from a deliberately truncated
/// (and possibly still "valid-looking" up to the cut) forged line, and
/// silently dropping the tail either way would let a corrupted journal
/// replay as if nothing were wrong. An operator recovering from a crash
/// inspects the file and trims the partial line by hand.
fn replay(path: &Path) -> Result<Vec<Entry>, JournalError> {
    let contents = std::fs::read_to_string(path).map_err(|source| JournalError::Io {
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
    fn append_is_durable_before_returning_and_survives_a_reopen() {
        let path = temp_path();
        let mut journal = FileJournal::open(&path).unwrap();
        journal.append(server_started()).unwrap();
        drop(journal);
        let reopened = FileJournal::open(&path).unwrap();
        assert_eq!(reopened.entries().len(), 1);
    }
}
