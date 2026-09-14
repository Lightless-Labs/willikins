//! Types for [`crate::Butler`]'s read-only operations: `validate`,
//! `describe`, `list_tools`, `propose_slug`. The operations themselves
//! live on `Butler` in `butler.rs` (they need its private rate-limiter
//! fields); this module holds only the shapes their callers see.

use willikins_core::{CheckError, CheckWarning};
use willikins_types::ProjectSlug;

/// Where a document body comes from for `validate`/`describe`: either
/// supplied inline (the authoring loop -- pure, no provider call, no
/// trust implication) or a name resolved in the trusted workflow
/// directory. `plan`/`apply` accept only [`willikins_types::WorkflowName`]
/// directly -- never a [`Self::Body`] -- which is what makes them unable
/// to run untrusted document text at all; see the plan's "Documents"
/// trust boundary.
#[derive(Debug, Clone)]
pub enum DocumentSource {
    /// An inline document body, not yet known to be in the trusted
    /// directory (or not meant to be: the authoring loop).
    Body(String),
    /// A name to resolve in the trusted workflow directory.
    Name(willikins_types::WorkflowName),
}

/// What [`crate::Butler::validate`] returns.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct ValidateResponse {
    /// Whether the document checked cleanly (no errors; warnings are
    /// still allowed).
    pub ok: bool,
    /// Every `check` failure, when `ok` is `false`.
    pub errors: Vec<CheckError>,
    /// Every `check` warning, when `ok` is `true` (a document that fails
    /// `check` reports its failures as `errors`, not warnings, so the two
    /// lists are never both non-empty).
    pub warnings: Vec<CheckWarning>,
}

/// What [`crate::Butler::propose_slug`] returns on success.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct ProposeSlugResponse {
    /// The proposed slug.
    pub slug: ProjectSlug,
}
