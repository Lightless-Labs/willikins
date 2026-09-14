//! Result types every `Butler` operation returns, `Serialize` and
//! `JsonSchema` because task 10b's rmcp tools and task 11's CLI print
//! them verbatim.

use willikins_core::Plan;
use willikins_journal::{PlanId, RunId, Timestamp};

/// Whether a plan's approval was automatic (its class did not require
/// one) or is still pending a human decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequirement {
    /// The plan's class does not require approval; it auto-approved.
    Automatic,
    /// The plan requires a human decision before it can run.
    Pending,
}

/// What [`crate::Butler::plan`] returns.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct PlanResponse {
    /// This plan's fresh id.
    pub plan_id: PlanId,
    /// The plan itself.
    pub plan: Plan,
    /// Whether this plan's class requires human approval.
    pub requires_approval: bool,
    /// Whether it auto-approved, or is waiting on one.
    pub approval: ApprovalRequirement,
    /// When this plan stops being runnable: the approval window's end
    /// for a pending plan, the apply window's end for an automatically
    /// approved one.
    pub expires_at: Timestamp,
}

/// What [`crate::Butler::apply`] returns once the run has started, before
/// it necessarily finishes: poll `Butler::run(run_id)` for its progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct RunHandle {
    /// The started run's id.
    pub run_id: RunId,
}
