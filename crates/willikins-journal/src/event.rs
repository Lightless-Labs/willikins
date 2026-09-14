//! [`Event`] and [`Entry`]: one journal line.
//!
//! Every payload here that can hold a value carries it as a core
//! [`willikins_core::Value`]-bearing type -- but never that type directly;
//! see [`crate::redacted::Redacted`]'s module docs for why direct storage
//! is impossible (no `Deserialize`) and what stands in for it instead.
//! Nothing in this module implements `Serialize` over a secret any other
//! way: every field that could name a resource's value is either a
//! [`crate::redacted::Redacted`] wrapper, a [`willikins_core::ToolError`]
//! (documented, by its own type, to never carry one), a
//! [`willikins_core::NodeStatus`] (same guarantee, transitively, since its
//! only payload is a `ToolError`), or a
//! [`willikins_core::InstanceFingerprint`] (already collapsed to strings,
//! with every secret port fixed to one marker regardless of content).
//! `tests/redaction_by_construction.rs` seeds a distinctive secret value,
//! builds one of each event that could possibly carry it, and greps the
//! serialized line for the seeded bytes.

use std::collections::BTreeMap;

use indexmap::IndexMap;

use willikins_core::{
    Class, InputName, Inputs, InstanceFingerprint, NodeName, NodeStatus, OutputName, PortName,
    ToolError, ToolName, Value,
};
use willikins_types::WorkflowName;

use crate::document_hash::DocumentSha256;
use crate::ids::{PlanId, RunId};
use crate::reason::Reason;
use crate::redacted::Redacted;
use crate::{PrincipalId, Timestamp};

/// How a [`crate::Journal::append`]-time [`Event::ApplyRefused`] refused
/// to run a plan. Mirrors the trust boundaries section's list, plus
/// [`Self::PlanFailed`] for the one "nothing ran" refusal that list does
/// not name (see that variant's own doc);
/// `Drift` carries only the instance identity and *which kind* of drift
/// was seen, never a planned or observed value -- unlike
/// [`willikins_core::DriftKind`], which the apply executor's own error
/// carries for a caller to render immediately, and which does hold a
/// [`Value`] in its `Output` variant. The journal never gets that far: it
/// records what happened, not what almost happened to look like.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApplyRefusedReason {
    /// `apply` was called with a `plan_id` the journal has no
    /// `PlanRecorded` event for.
    UnknownPlan,
    /// The workflow document's hash no longer matches the one `plan`
    /// recorded.
    DocumentChanged,
    /// A fresh re-plan disagrees with the approved plan at one instance.
    Drift {
        /// The node whose instance drifted.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// What kind of drift, with no planned or observed value. Renamed
        /// `detail` on the wire, the same way
        /// [`willikins_core::ApplyError::Drift`]'s own `kind` field is:
        /// a field literally named `kind` collides with this enum's own
        /// internal tag key.
        #[serde(rename = "detail")]
        kind: DriftReasonKind,
    },
    /// A fresh re-plan of the approved workflow failed outright
    /// ([`willikins_core::ApplyError::Plan`]), so `apply` refused at its
    /// own rule 2 -- before minting a `SinkToken`, before any provider
    /// write, and before any [`willikins_core::ApplyEvent`] reached an
    /// observer. Nothing ran, which is why this is a refusal and not a
    /// failed run: the same plan is still legitimately applicable once
    /// whatever made the re-plan fail (an unreadable provider, a
    /// `for_each` source that momentarily resolved to nothing) is
    /// resolved, and recording it as a run would set
    /// [`crate::PlanRecord::applied`] and block that retry for good.
    ///
    /// The milestone plan's own list of `ApplyRefused` reasons does not
    /// name this case, but `willikins_core::ApplyError`'s own doc groups
    /// `Plan` with `ApprovalRequired` and `Drift` as the three refusals
    /// that carry no partial result; a seventh reason is the smaller
    /// deviation of the two available, and is recorded in this task's
    /// verification notes for a plan addendum.
    PlanFailed {
        /// The [`willikins_core::PlanError`]'s own internally tagged
        /// `kind`, e.g. `MissingInput`: a serde variant name, never a
        /// value. The full error goes back to the caller, which is where
        /// its details (some of which are [`Value`]s) belong -- the
        /// journal records which kind of planning failure refused the
        /// apply, not what the values were.
        error_kind: String,
    },
    /// The plan's approval or apply window has elapsed.
    PlanExpired,
    /// The plan requires approval and none was given.
    ApprovalRequired,
    /// This plan has already been applied once.
    AlreadyApplied,
}

/// Which of [`willikins_core::DriftKind`]'s three shapes an
/// [`ApplyRefusedReason::Drift`] reports, with the values themselves
/// stripped: `Output` keeps the port name (not a secret; ports are
/// declared in the workflow document) but never the planned or observed
/// value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DriftReasonKind {
    /// The two plans disagree about which instance sits at this position
    /// of the walk.
    Instance,
    /// The instance's planned action itself differs from what a fresh
    /// re-plan now observes.
    Action,
    /// The instance's action still agrees, but a non-secret output's
    /// rendered value has changed.
    Output {
        /// The output port whose value changed.
        port: PortName,
    },
}

/// How a run finished: [`crate::Event::RunFinished`]'s payload.
///
/// `Succeeded` carries the run's resolved workflow outputs -- a field the
/// milestone plan's own wording for `RunFinished` does not list alongside
/// `outcome`, added here because [`crate::journal::RunRecord`] (which the
/// plan does specify as carrying `outputs`) has nowhere else to get them
/// from: `willikins_core::apply::apply`'s successful return is exactly an
/// `Applied`, whose own `outputs` field this mirrors. Recorded as a
/// deliberate, reasoned deviation rather than a silent addition; see the
/// crate's top-level docs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// The run finished; every attempted instance succeeded.
    Succeeded {
        /// Every workflow output's resolved value.
        outputs: Redacted<IndexMap<OutputName, Value>>,
    },
    /// The run stopped on a failure or a blocked instance.
    Failed {
        /// The failure `apply` returned, partial result included.
        error: Redacted<willikins_core::ApplyError>,
    },
}

/// Which surface a caller reached the butler through, for
/// [`Event::AuthFailed`]. `willikins-server` (task 7) is the only source
/// of this event; kept minimal and provisional here since neither
/// transport's own request shape exists yet at this task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// The stdio MCP transport.
    Stdio,
    /// The HTTP MCP transport.
    Http,
}

/// Why an authentication attempt failed, for [`Event::AuthFailed`]. Never
/// the presented token or its hash -- see that event's own doc.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthFailedReason {
    /// No credential was presented at all.
    MissingCredential,
    /// A credential was presented but matched no known principal.
    InvalidCredential,
    /// The credential was valid but for the wrong role (an approver
    /// credential on `/mcp`, an agent credential on `/approvals`).
    WrongRole,
}

/// One thing that happened, recorded by [`crate::Journal::append`].
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`, `snake_case`
/// variant names) with `#[serde(deny_unknown_fields)]` on every variant,
/// so a hand-edited or truncated-then-patched line with a stray field is
/// a replay error rather than a silently ignored one. See the module docs
/// for the redaction rule every payload here follows.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// The server started. Emitted once, first, by `willikins-server`
    /// (task 7).
    ServerStarted {
        /// The running `willikins-server` binary's own version.
        version: String,
        /// The trusted workflow directory it was started with.
        workflows_dir: String,
        /// Every workflow document's file name and its content's SHA-256,
        /// hex-encoded, as found at startup.
        workflow_hashes: BTreeMap<String, String>,
    },
    /// One MCP tool call. Never a document body or an input value: only
    /// names.
    ToolCalled {
        /// Who called it.
        principal: PrincipalId,
        /// The tool name (an MCP tool such as `plan`, not a
        /// `willikins_core::Tool`).
        tool: ToolName,
        /// The workflow it concerned, if the tool takes one.
        workflow: Option<WorkflowName>,
        /// Whether the call succeeded.
        ok: bool,
    },
    /// A plan was produced and recorded.
    PlanRecorded {
        /// This plan's fresh id.
        plan_id: PlanId,
        /// The planned workflow's name.
        workflow: WorkflowName,
        /// The workflow document's content hash at plan time.
        document_sha256: DocumentSha256,
        /// The resolved workflow inputs the plan was built from.
        inputs: Redacted<IndexMap<InputName, Value>>,
        /// The plan itself.
        plan: Redacted<willikins_core::Plan>,
        /// The plan's per-instance fingerprint, for a later drift check
        /// even once the plan itself is gone from memory (kept typed,
        /// unlike `plan` above: see
        /// [`willikins_core::InstanceFingerprint`]'s own doc).
        fingerprint: Vec<InstanceFingerprint>,
        /// The plan's approval class.
        class: Class,
        /// Whether this plan requires human approval.
        requires_approval: bool,
    },
    /// A plan below the approval threshold was auto-approved.
    ApprovalAutomatic {
        /// The plan.
        plan_id: PlanId,
        /// Its class, for the record.
        class: Class,
    },
    /// A human approved a pending plan.
    ApprovalGranted {
        /// The plan.
        plan_id: PlanId,
        /// The approving principal.
        approver: PrincipalId,
    },
    /// A human rejected a pending plan.
    ApprovalRejected {
        /// The plan.
        plan_id: PlanId,
        /// The rejecting principal.
        approver: PrincipalId,
        /// Why, bounded to [`crate::Reason::MAX_CHARS`] characters.
        reason: Reason,
    },
    /// An `apply` call was refused before (or, for `AlreadyApplied`,
    /// instead of) running anything.
    ApplyRefused {
        /// The plan.
        plan_id: PlanId,
        /// Who tried to apply it.
        principal: PrincipalId,
        /// Why.
        reason: ApplyRefusedReason,
    },
    /// A run began.
    RunStarted {
        /// This run's fresh id.
        run_id: RunId,
        /// The plan being run.
        plan_id: PlanId,
        /// Who started it.
        principal: PrincipalId,
    },
    /// About to process one planned node instance.
    NodeStarted {
        /// The run.
        run_id: RunId,
        /// The node.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// The instance's resolved inputs.
        inputs: Redacted<Inputs>,
    },
    /// Finished processing one planned node instance.
    NodeFinished {
        /// The run.
        run_id: RunId,
        /// The node.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// What happened. Carries its own [`ToolError`] on
        /// [`NodeStatus::Failed`]; `error` below mirrors it (see that
        /// field's own doc).
        status: NodeStatus,
        /// This instance's final outputs.
        outputs: Redacted<willikins_core::Outputs>,
        /// The same failure `status` carries under
        /// [`NodeStatus::Failed`], if any, surfaced as its own field so a
        /// reader of this event does not need to match on `status` to
        /// find it. `None` for every other status.
        error: Option<ToolError>,
    },
    /// A run finished, one way or the other.
    RunFinished {
        /// The run.
        run_id: RunId,
        /// How it finished.
        outcome: Outcome,
    },
    /// An authentication attempt failed. Never the presented token or its
    /// hash.
    AuthFailed {
        /// Which transport was reached.
        transport: Transport,
        /// Why.
        reason: AuthFailedReason,
    },
}

/// One journal line: a sequence number, a timestamp, and the [`Event`]
/// itself.
///
/// `seq` starts at 1 and is contiguous; `at` never goes backwards between
/// consecutive entries -- both are validated by
/// [`crate::FileJournal::open`] on replay, and `append` clamps its own
/// clock read so it can never violate the second rule itself (see that
/// function's doc). `#[serde(deny_unknown_fields)]` here too: a stray
/// top-level field is as much a malformed line as an unknown `Event`
/// field.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// This entry's 1-based sequence number within the journal.
    pub seq: u64,
    /// When this entry was appended.
    pub at: Timestamp,
    /// The event itself.
    pub event: Event,
}
