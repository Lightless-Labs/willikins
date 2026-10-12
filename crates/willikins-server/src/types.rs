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

/// What [`crate::Butler::plan_rollback`] returns (milestone 3m task R8).
/// Always requires approval and is always [`willikins_core::Class::Destructive`]
/// (SHARED VALUES: "Teardown class... always `Class::Destructive`;
/// `requires_approval` always `true`"), so unlike [`PlanResponse`] this
/// carries no `class`/`requires_approval` field of its own -- a caller
/// that needs to say so in a fixed sentence can simply say so.
///
/// No `JsonSchema`: `willikins_core::Teardown` itself has only `Serialize`
/// (deliberately, see its own doc), so a type holding one could not
/// derive `JsonSchema` either; this is server-internal today (the MCP
/// `rollback_plan` tool, the approvals page's own teardown rendering, and
/// the CLI are future work -- see the milestone plan's "Out of scope").
#[derive(Debug, Clone, serde::Serialize)]
pub struct TeardownPlanResponse {
    /// This teardown's fresh id, in the same [`PlanId`] namespace a
    /// forward plan's own id comes from.
    pub plan_id: PlanId,
    /// The teardown itself, for display.
    pub teardown: willikins_core::Teardown,
    /// When this teardown stops being approvable: the approval window's
    /// end, measured from when it was recorded.
    pub expires_at: Timestamp,
}

/// What [`crate::Butler::apply_rollback`] returns once the teardown run has
/// finished (milestone 3m task R8). Unlike [`RunHandle`], this is
/// returned only once the run is over: see that method's own doc for why
/// it runs synchronously rather than on a background thread.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TeardownRunResult {
    /// This teardown run's id, in the same [`RunId`] namespace a forward
    /// run's own id comes from -- never [`willikins_journal::journal::TeardownRecord::of_run`],
    /// which names what it undoes.
    pub run_id: RunId,
    /// Every step's own outcome, in the approved teardown's own order.
    pub steps: Vec<(
        willikins_core::InstanceRef,
        willikins_core::TeardownStepOutcome,
    )>,
    /// Whether every step ran (or was already gone) with no failure.
    pub succeeded: bool,
}
