//! Values, tool contract, catalog, workflow graph, checker, describe, plan, ledger.
//!
//! See `docs/plans/2026-09-11-milestone-1-core.md` for the crate contract.
//!
//! This crate re-exports [`SinkToken`] from `willikins-types` with that
//! crate's `executor` cargo feature enabled: `willikins-core` is where the
//! milestone-2 apply executor lives, and minting a token is the one thing
//! that feature exists to allow. Feature unification means the constructor
//! becomes reachable from every crate in the same build once this one
//! enables it; the workspace `clippy.toml` `disallowed-methods` entry is
//! what actually keeps every other call site honest.

pub mod apply;
pub mod catalog;
pub mod check;
pub mod class;
pub mod describe;
pub mod plan;
pub mod reported;
pub mod site;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod tool;
pub mod value;
pub mod workflow;

pub use apply::{
    Applied, AppliedNode, ApplyError, ApplyEvent, ApplyObserver, Approval, DriftKind, InstanceRef,
    NodeStatus, NoopObserver, PrincipalId, RecordingObserver, Timestamp, apply,
};
pub use catalog::{Catalog, CatalogError};
pub use check::{CheckError, CheckWarning, Checked, check};
pub use class::Class;
pub use describe::{
    Description, InputArg, InputError, MissingInput, PartialInputs, RawInput, describe,
};
pub use plan::{Action, InstanceFingerprint, Plan, PlanError, PlannedNode, plan};
pub use reported::Reported;
pub use site::Site;
/// Generic tool-authoring helpers (port/type construction, input checks,
/// `ToolError` constructors) shared by every tool crate downstream of
/// `willikins-core`; see [`tool::helpers`] for the full set.
pub use tool::helpers;
pub use tool::{
    Ensured, Inputs, Observation, Outputs, PortName, PortSpec, SpecError, Tool, ToolError,
    ToolErrorKind, ToolName, ToolSpec,
};
pub use value::{Known, PortType, TypeName, TypeRef, TypeRegistry, Value, ValueState};
pub use workflow::{Binding, InputName, InputSpec, Node, NodeName, OutputName, Workflow};

/// Capability token gating access to secret values; see `willikins_types::sink::SinkToken`.
pub use willikins_types::SinkToken;
