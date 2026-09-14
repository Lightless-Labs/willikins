//! `willikins-server`'s library core: [`Butler`], the plan/approve/apply
//! operations both surfaces (task 10b's rmcp tools, the CLI) call.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-server` section (library paragraphs) for the normative
//! shape this crate implements, and its "Plan identity" and "Approval is
//! typed at the call" trust boundaries for why plan identity and the
//! approval binding live here rather than in `willikins-core`.

mod butler;
mod document;
mod drift;
mod error;
mod types;

pub use butler::{Butler, ButlerConfig, SharedJournal};
pub use drift::DriftDetail;
pub use error::{ButlerError, ExpiryWindow};
pub use types::{ApprovalRequirement, PlanResponse, RunHandle};

// Re-exported so a downstream crate (task 10b, task 11) never needs its
// own direct dependency on `willikins-journal` just to name a `PlanId`,
// `RunId`, or the journal's own recorded views when working with this
// crate's own operations.
pub use willikins_journal::{PlanId, RunId};
pub use willikins_journal::{PlanRecord, RunRecord};
