//! `willikins-server`'s library core: [`Butler`], the plan/approve/apply
//! operations both surfaces (task 10b's rmcp tools, the CLI) call.
//!
//! See `docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md`'s
//! `willikins-server` section (library paragraphs) for the normative
//! shape this crate implements, and its "Plan identity" and "Approval is
//! typed at the call" trust boundaries for why plan identity and the
//! approval binding live here rather than in `willikins-core`.

mod butler;
mod catalog;
pub mod cli;
mod config;
mod document;
mod drift;
mod error;
mod http;
mod mcp;
mod rate_limit;
mod read_ops;
mod startup;
mod types;

pub use butler::{Butler, ButlerConfig, SharedJournal};
pub use catalog::{LIVE_TOOL_NAMES, LiveCredentialError, live_catalog_from_env, live_catalog_with};
pub use config::{ConfigError, ServerConfig};
pub use drift::DriftDetail;
pub use error::{ButlerError, ExpiryWindow};
pub use http::{
    HttpConfig, HttpConfigError, ServeHttpError, TokenHash, TokenHashError, router, serve_http,
    serve_http_with,
};
pub use mcp::{
    ApplyParams, ApplyStarted, DescribeParams, InputValueDto, MAX_CONCURRENT_TOOL_CALLS,
    PlanParams, ProposeSlugParams, RunStatusParams, ServeError, ValidateParams, WillikinsHandler,
    serve_stdio, serve_stdio_handler,
};
pub use read_ops::{DocumentSource, ProposeSlugResponse, ValidateResponse};
pub use startup::{InputSummary, StartupError, WorkflowSummary};
pub use types::{ApprovalRequirement, PlanResponse, RunHandle};

// Re-exported so a downstream crate (task 10b, task 11) never needs its
// own direct dependency on `willikins-journal` just to name a `PlanId`,
// `RunId`, or the journal's own recorded views when working with this
// crate's own operations.
pub use willikins_journal::{PlanId, RunId};
pub use willikins_journal::{PlanRecord, RunRecord};
