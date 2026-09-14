//! An append-only JSONL journal: the run ledger and audit trail for
//! `willikins-server` (task 7) and the CLI/acceptance test suites that
//! exercise it ahead of that crate landing.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-journal` section for the normative shape this crate
//! implements, and its "Plan identity" trust boundary for why a plan's
//! own record and a run's own record exist at all.
//!
//! # Redaction
//!
//! Every [`Event`] payload that can hold a [`willikins_core::Value`]
//! holds it behind [`redacted::Redacted`], never the live core type
//! directly (see that module's own docs for exactly why, and what stands
//! in for it). No type in this crate implements [`serde::Serialize`] over
//! a secret any other way -- there is no second, ad hoc redaction path
//! anywhere in this crate to remember to use correctly, because there is
//! only the one path. `tests/redaction_by_construction.rs` seeds a
//! distinctive secret value, builds one instance of every event shape
//! that could possibly carry it, and greps the appended JSONL line and
//! every replay view built from it for the seeded bytes.
//!
//! # No delete, no rewrite
//!
//! [`Journal::append`] is the only way to add anything; neither
//! [`Journal`] nor either implementation exposes a way to remove or
//! change a past entry. [`FileJournal`] additionally takes an exclusive
//! lock on its file for as long as it is open, so at most one process can
//! be appending to a given journal at a time.

mod clock;
mod document_hash;
mod event;
mod file;
mod ids;
mod journal;
mod memory;
mod observer;
mod reason;
pub mod redacted;
mod timestamp_clamp;

pub use clock::{Clock, ManualClock, SystemClock};
pub use document_hash::{DocumentSha256, DocumentSha256Error};
pub use event::{
    ApplyRefusedReason, AuthFailedReason, DriftReasonKind, Entry, Event, Outcome, Transport,
};
pub use file::{FileJournal, JournalError};
pub use ids::{PlanId, RunId};
pub use journal::{ApprovalState, Journal, PlanRecord, RunNode, RunRecord, RunState};
pub use memory::MemoryJournal;
pub use observer::{Append, JournalObserver, continue_run_and_journal, run_and_journal};
pub use reason::{Reason, ReasonError};
pub use redacted::{Redactable, Redacted};

/// Re-exported so downstream crates never need their own dependency on
/// `willikins-core` just to name a [`PrincipalId`] or a [`Timestamp`] when
/// working with this crate's own types.
pub use willikins_core::{PrincipalId, Timestamp};
