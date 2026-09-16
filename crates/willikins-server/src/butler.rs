//! [`Butler`]: the plan/approve/apply operations both surfaces
//! (task 10b's rmcp tools, the CLI) call. Owns the trusted workflow
//! directory, the catalog, the journal, and the single-apply lock.
//!
//! # A plan's resolved inputs survive a restart
//!
//! `willikins_journal::journal::PlanRecord::inputs` is a
//! `Redacted<IndexMap<InputName, Value>>`: already-redacted JSON, not a
//! live `Value` (`willikins_core::value::Value` has no `Deserialize` at
//! all -- redaction is one-way, by design). `apply`'s own re-plan step
//! needs the *real*, typed inputs to call `willikins_core::plan` again.
//!
//! An earlier revision of this module kept those inputs in an in-memory
//! `plan_id -> resolved inputs` map, populated by [`Butler::plan`] and
//! read by [`Butler::apply`] -- which meant a plan recorded before a
//! process restart (a `FileJournal` reopened by a fresh `Butler`) could
//! never be applied, even though the journal itself replayed cleanly.
//! That is exactly the case this design promises to survive: approval is
//! human-paced, and a redeploy between `plan` and a human's decision is
//! the normal case, not an edge one.
//!
//! [`Butler::apply`] now rebuilds the resolved inputs from the journal's
//! own record instead of trusting an in-memory cache: every workflow
//! input is non-secret by construction (`check` refuses a secret one),
//! so `PlanRecord.inputs`'s redacted JSON *is* the real rendered strings
//! -- one per declared input, a plain string for a scalar or an array of
//! strings for a list (`willikins_core::value::Value`'s own pinned JSON
//! shape). [`resolve_recorded_inputs`] parses each one back through the
//! type registry, against the input's declared type from the freshly
//! reloaded (and hash-checked) workflow, exactly the way
//! [`willikins_core::describe`] parses a raw value at `plan` time --
//! `Value::parse`/`Value::parse_list` round-tripping `Value::render`'s
//! own output is the invariant this depends on (see
//! `willikins-core/tests/value_render_parse_round_trip.rs`'s property
//! test). A value that no longer parses (which cannot happen for a
//! document whose hash still matches, but is checked rather than assumed)
//! refuses with [`ButlerError::RecordedInputUnreadable`], never a panic.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use indexmap::IndexMap;

use willikins_core::describe::PartialInputs;
use willikins_core::{
    Applied, ApplyError, Approval, Catalog, Checked, InputName, PlanError, ToolError,
    ToolErrorKind, ToolName, TypeRef, Value,
};
use willikins_journal::{
    Append, ApplyRefusedReason, ApprovalState, Clock, Entry, Event, Journal, PlanId, PlanRecord,
    PrincipalId, Reason, Redacted, RunId, RunRecord, continue_run_and_journal,
};
use willikins_types::{DomainType, ParseError, WorkflowName};

use crate::document;
use crate::drift::{self, DriftDetail};
use crate::error::{ButlerError, ExpiryWindow};
use crate::read_ops::{DocumentSource, ProposeSlugResponse, ValidateResponse};
use crate::startup::{self, StartupError, WorkflowSummary};
use crate::types::{ApprovalRequirement, PlanResponse, RunHandle};

/// A journal shared between `Butler` and the background thread a run
/// continues on.
pub type SharedJournal = Arc<Mutex<dyn Journal + Send>>;

/// Everything a [`Butler`] is built from.
pub struct ButlerConfig {
    /// The trusted workflow directory: every document `plan`/`apply`
    /// name is looked up here (task 10b's startup validation checks
    /// every document in it; this half does not).
    pub workflows_dir: PathBuf,
    /// The journal every operation records into.
    pub journal: SharedJournal,
    /// The tool catalog `check`/`plan`/`apply` run against.
    pub catalog: Catalog,
    /// Where "now" comes from, shared with the journal so a test's fake
    /// clock and this `Butler`'s own window math never disagree.
    pub clock: Arc<dyn Clock>,
    /// How long a plan may wait, undecided, for a human.
    pub approval_window: Duration,
    /// How long an approved (or auto-approved) plan may sit before it is
    /// applied.
    pub apply_window: Duration,
    /// `plan` calls allowed per principal per rolling minute. See
    /// `crate::rate_limit`'s module doc for why only `plan` and the
    /// combined `describe`/`validate` bucket are limited at all.
    pub plan_rate_per_minute: u32,
    /// `describe` and `validate` calls, combined, allowed per principal
    /// per rolling minute.
    pub read_rate_per_minute: u32,
}

impl ButlerConfig {
    /// The design's default approval window: 24 hours.
    pub const DEFAULT_APPROVAL_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
    /// The design's default apply window: 60 minutes.
    pub const DEFAULT_APPLY_WINDOW: Duration = Duration::from_secs(60 * 60);
    /// The design's default `plan` rate: 10 per minute.
    pub const DEFAULT_PLAN_RATE_PER_MINUTE: u32 = 10;
    /// The design's default combined `describe`/`validate` rate: 60 per
    /// minute.
    pub const DEFAULT_READ_RATE_PER_MINUTE: u32 = 60;
}

/// The plan/approve/apply library core. See the module docs.
pub struct Butler {
    workflows_dir: PathBuf,
    journal: SharedJournal,
    catalog: Arc<Catalog>,
    clock: Arc<dyn Clock>,
    approval_window: Duration,
    apply_window: Duration,
    /// What `apply` is doing, if anything: see [`RunState`]. The mutex
    /// is held only long enough to read and change that value -- never
    /// across a provider read, a file read, or the run itself.
    run_lock: Arc<Mutex<RunState>>,
    /// `plan`'s own bucket.
    plan_rate_limiter: crate::rate_limit::RateLimiter,
    /// `describe` and `validate`'s shared bucket.
    read_rate_limiter: crate::rate_limit::RateLimiter,
}

impl Butler {
    /// Build a `Butler` from `config`. Does not validate the workflow
    /// directory (task 10b's job); does not journal `ServerStarted`
    /// (task 10b's job, once it knows a version and can list the
    /// directory's documents).
    #[must_use]
    pub fn new(config: ButlerConfig) -> Self {
        let plan_rate_limiter =
            crate::rate_limit::RateLimiter::new(config.plan_rate_per_minute, config.clock.clone());
        let read_rate_limiter =
            crate::rate_limit::RateLimiter::new(config.read_rate_per_minute, config.clock.clone());
        Self {
            workflows_dir: config.workflows_dir,
            journal: config.journal,
            catalog: Arc::new(config.catalog),
            clock: config.clock,
            approval_window: config.approval_window,
            apply_window: config.apply_window,
            run_lock: Arc::new(Mutex::new(RunState::Idle)),
            plan_rate_limiter,
            read_rate_limiter,
        }
    }

    /// Build a `Butler` from `config`, validating the trusted workflow
    /// directory first: every `.yaml`/`.yml` document in it (non-recursive;
    /// see [`startup::scan_directory`]) must have a filename stem that
    /// parses as a [`WorkflowName`] and equals the document's own `name:`,
    /// must parse, and must `check` against `config.catalog` -- the first
    /// failure refuses startup, naming the file, and nothing is journaled.
    /// On success, journals `ServerStarted` once with this crate's own
    /// version, the directory, and every document's filename mapped to its
    /// content hash.
    ///
    /// `plan` and `apply` re-read and re-validate their named document on
    /// every call regardless (see their own docs): this only pins the
    /// directory's state *at startup*, so a document that stops parsing
    /// or checking afterward is caught the next time something asks for
    /// it by name, not silently kept running against its startup version.
    ///
    /// # Errors
    ///
    /// See [`StartupError`].
    pub fn start(config: ButlerConfig) -> Result<Self, StartupError> {
        let butler = Self::new(config);
        let loaded = startup::scan_directory(&butler.workflows_dir, &butler.catalog)?;

        let mut workflow_hashes = std::collections::BTreeMap::new();
        for entry in &loaded {
            let filename = entry
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            workflow_hashes.insert(filename, entry.document_sha256.to_string());
        }

        butler
            .append(Event::ServerStarted {
                version: env!("CARGO_PKG_VERSION").to_string(),
                workflows_dir: butler.workflows_dir.display().to_string(),
                workflow_hashes,
            })
            .map_err(|error| StartupError::Journal {
                message: error.to_string(),
            })?;

        Ok(butler)
    }

    /// Every document currently in the trusted workflow directory,
    /// summarised. Re-scans the directory on every call (see
    /// [`startup::scan_directory`]): a document that stopped parsing or
    /// checking since [`Self::start`] fails this call by naming it, rather
    /// than being silently omitted from the list -- the same "fail
    /// naming the file, never drop it quietly" rule `start` itself
    /// follows.
    ///
    /// # Errors
    ///
    /// [`ButlerError::Startup`] wrapping whichever [`StartupError`] the
    /// scan hit.
    pub fn list_workflows(
        &self,
        principal: PrincipalId,
    ) -> Result<Vec<WorkflowSummary>, ButlerError> {
        let result = startup::scan_directory(&self.workflows_dir, &self.catalog)
            .map(|loaded| loaded.into_iter().map(WorkflowSummary::from).collect())
            .map_err(|error| ButlerError::Startup { error });
        self.record_tool_call("list_workflows", None, principal, result.is_ok());
        result
    }

    /// Load `source`: a body is parsed with `willikins_dsl::parse_document`
    /// directly (never through the byte-cap-then-load-document path a
    /// file goes through, since a body has no path to `stat` first -- the
    /// DSL's own cap and pre-scan still apply, because `parse_document` is
    /// exactly what enforces them); a name is resolved in the trusted
    /// directory exactly as `plan` resolves one, collapsing a missing,
    /// mismatched, or symlinked file to [`ButlerError::UnknownWorkflow`].
    fn load_source(
        &self,
        source: &DocumentSource,
    ) -> Result<willikins_core::Workflow, ButlerError> {
        match source {
            DocumentSource::Body(body) => {
                willikins_dsl::parse_document(body).map_err(|error| ButlerError::Document { error })
            }
            DocumentSource::Name(name) => {
                match document::load_named_document(&self.workflows_dir, name) {
                    Ok((_sha, workflow)) => Ok(workflow),
                    Err(
                        document::LoadError::NotFound
                        | document::LoadError::NameMismatch { .. }
                        | document::LoadError::Symlink(_),
                    ) => Err(ButlerError::UnknownWorkflow {
                        workflow: name.clone(),
                    }),
                    Err(document::LoadError::Document(error)) => {
                        Err(ButlerError::Document { error })
                    }
                }
            }
        }
    }

    fn source_workflow_name(source: &DocumentSource) -> Option<WorkflowName> {
        match source {
            DocumentSource::Name(name) => Some(name.clone()),
            DocumentSource::Body(_) => None,
        }
    }

    // -------------------------------------------------------------
    // read operations: validate, describe, list_tools, propose_slug
    // -------------------------------------------------------------

    /// Parse and statically `check` `source`, with no provider call.
    ///
    /// A [`DocumentSource::Body`] over the DSL's byte cap, or carrying a
    /// YAML anchor or alias, is refused as [`ButlerError::Document`] --
    /// the DSL's own error, never folded into a fabricated `check`
    /// failure -- because `parse_document` runs those checks before a
    /// [`willikins_core::Workflow`] exists to `check` at all. A document
    /// that parses but fails `check` is reported as a normal (`ok: false`)
    /// [`ValidateResponse`], not an `Err`: `check` failures are exactly
    /// what a caller is asking to see.
    ///
    /// Rate-limited: shares the combined `describe`/`validate` bucket
    /// (see `crate::rate_limit`).
    ///
    /// # Errors
    ///
    /// [`ButlerError::Document`], [`ButlerError::UnknownWorkflow`], or
    /// [`ButlerError::RateLimited`].
    pub fn validate(
        &self,
        source: &DocumentSource,
        principal: PrincipalId,
    ) -> Result<ValidateResponse, ButlerError> {
        if let Err(retry_after_seconds) = self.read_rate_limiter.check(&principal) {
            self.record_tool_call("validate", None, principal, false);
            return Err(ButlerError::RateLimited {
                retry_after_seconds,
            });
        }
        let result = self.validate_inner(source);
        self.record_tool_call(
            "validate",
            Self::source_workflow_name(source),
            principal,
            result.is_ok(),
        );
        result
    }

    fn validate_inner(&self, source: &DocumentSource) -> Result<ValidateResponse, ButlerError> {
        let workflow = self.load_source(source)?;
        Ok(match willikins_core::check(&workflow, &self.catalog) {
            Ok(checked) => ValidateResponse {
                ok: true,
                errors: Vec::new(),
                warnings: checked.warnings,
            },
            Err(errors) => ValidateResponse {
                ok: false,
                errors,
                warnings: Vec::new(),
            },
        })
    }

    /// Load, `check`, and [`willikins_core::describe`] `source` against
    /// `partial`'s raw inputs, with no provider call.
    ///
    /// The returned [`willikins_core::Description`] carries its own
    /// `errors` (rejected raw values) and `missing` (undeclared inputs)
    /// fields *inside* a successful result -- `describe` itself never
    /// fails on bad inputs, only on a document that will not even parse
    /// or `check` (see `todos/2026-09-12-error-json-uniformity-gaps.md`
    /// item 1: this is the "result field, not an error" answer that todo
    /// asked task 10a to pin).
    ///
    /// Rate-limited: shares the combined `describe`/`validate` bucket.
    ///
    /// # Errors
    ///
    /// [`ButlerError::Document`], [`ButlerError::UnknownWorkflow`],
    /// [`ButlerError::Check`], or [`ButlerError::RateLimited`].
    pub fn describe(
        &self,
        source: &DocumentSource,
        partial: &PartialInputs,
        principal: PrincipalId,
    ) -> Result<willikins_core::Description, ButlerError> {
        if let Err(retry_after_seconds) = self.read_rate_limiter.check(&principal) {
            self.record_tool_call("describe", None, principal, false);
            return Err(ButlerError::RateLimited {
                retry_after_seconds,
            });
        }
        let result = self.describe_inner(source, partial);
        self.record_tool_call(
            "describe",
            Self::source_workflow_name(source),
            principal,
            result.is_ok(),
        );
        result
    }

    fn describe_inner(
        &self,
        source: &DocumentSource,
        partial: &PartialInputs,
    ) -> Result<willikins_core::Description, ButlerError> {
        let workflow = self.load_source(source)?;
        let checked = willikins_core::check(&workflow, &self.catalog)
            .map_err(|errors| ButlerError::Check { errors })?;
        Ok(willikins_core::describe(&checked, partial))
    }

    /// The full tool and type catalog, as
    /// [`willikins_core::Catalog::list_tools_json`] renders it -- the same
    /// JSON `willikins schema --catalog` prints. Never fails, and not
    /// rate-limited (see `crate::rate_limit`'s module doc: a list
    /// operation touches no provider).
    pub fn list_tools(&self, principal: PrincipalId) -> serde_json::Value {
        let json = self.catalog.list_tools_json();
        self.record_tool_call("list_tools", None, principal, true);
        json
    }

    /// Propose a project slug from a free-form display name, exactly as
    /// the CLI's `propose-slug` subcommand does: parse `name` as a
    /// [`willikins_types::ProjectName`], then run
    /// [`willikins_types::propose_slug`]. Not rate-limited (pure, no
    /// provider call, and not `plan`/`describe`/`validate`).
    ///
    /// # Errors
    ///
    /// [`ButlerError::InvalidProjectName`] when `name` does not parse;
    /// [`ButlerError::SlugProposal`] when `propose_slug` itself refuses
    /// the (valid) name.
    pub fn propose_slug(
        &self,
        name: &str,
        principal: PrincipalId,
    ) -> Result<ProposeSlugResponse, ButlerError> {
        let result = Self::propose_slug_inner(name);
        self.record_tool_call("propose_slug", None, principal, result.is_ok());
        result
    }

    fn propose_slug_inner(name: &str) -> Result<ProposeSlugResponse, ButlerError> {
        let project_name = willikins_types::ProjectName::parse(name)
            .map_err(|error| ButlerError::InvalidProjectName { error })?;
        let slug = willikins_types::propose_slug(&project_name)
            .map_err(|error| ButlerError::SlugProposal { error })?;
        Ok(ProposeSlugResponse { slug })
    }

    fn journal_lock(&self) -> MutexGuard<'_, dyn Journal + Send + 'static> {
        self.journal.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn append(&self, event: Event) -> Result<Entry, ButlerError> {
        Journal::append(&mut *self.journal_lock(), event).map_err(|error| ButlerError::Journal {
            error: error.to_string(),
        })
    }

    fn record_tool_call(
        &self,
        tool: &str,
        workflow: Option<WorkflowName>,
        principal: PrincipalId,
        ok: bool,
    ) {
        let tool = ToolName::parse(tool)
            .unwrap_or_else(|error| unreachable!("`{tool}` is a valid ToolName literal: {error}"));
        let _ = self.append(Event::ToolCalled {
            principal,
            tool,
            workflow,
            ok,
        });
    }

    fn elapsed_since(&self, since: willikins_journal::Timestamp) -> Duration {
        elapsed_between(since, self.clock.now())
    }

    // -------------------------------------------------------------
    // plan
    // -------------------------------------------------------------

    /// Load, check, and plan `workflow` against the catalog, recording a
    /// `PlanRecorded` event (and, when the plan needs none, an
    /// `ApprovalAutomatic` right after it).
    ///
    /// # Errors
    ///
    /// [`ButlerError::UnknownWorkflow`] when the trusted directory holds
    /// no document named `workflow` (including one whose own internal
    /// `name:` does not match the file it would have to be found at --
    /// see `crate::document`'s module docs); [`ButlerError::Document`] on
    /// a parse failure; [`ButlerError::Check`] when the document fails
    /// `check`; [`ButlerError::Input`] when `inputs` does not resolve;
    /// [`ButlerError::Plan`] when planning fails; [`ButlerError::Journal`]
    /// if recording the plan fails.
    pub fn plan(
        &self,
        workflow: WorkflowName,
        inputs: &PartialInputs,
        principal: PrincipalId,
    ) -> Result<PlanResponse, ButlerError> {
        if let Err(retry_after_seconds) = self.plan_rate_limiter.check(&principal) {
            self.record_tool_call("plan", Some(workflow), principal, false);
            return Err(ButlerError::RateLimited {
                retry_after_seconds,
            });
        }
        let result = self.plan_inner(&workflow, inputs, &principal);
        self.record_tool_call("plan", Some(workflow), principal, result.is_ok());
        result
    }

    fn plan_inner(
        &self,
        workflow: &WorkflowName,
        inputs: &PartialInputs,
        principal: &PrincipalId,
    ) -> Result<PlanResponse, ButlerError> {
        let (document_sha256, doc_workflow) =
            match document::load_named_document(&self.workflows_dir, workflow) {
                Ok(loaded) => loaded,
                Err(
                    document::LoadError::NotFound
                    | document::LoadError::NameMismatch { .. }
                    | document::LoadError::Symlink(_),
                ) => {
                    // A symlinked document collapses to the same refusal
                    // as one that is simply not there: from a caller's
                    // point of view the trusted directory does not, in
                    // the sense that matters, hold a document named
                    // `workflow` -- see `crate::document`'s "Symlinks are
                    // refused" doc.
                    return Err(ButlerError::UnknownWorkflow {
                        workflow: workflow.clone(),
                    });
                }
                Err(document::LoadError::Document(error)) => {
                    return Err(ButlerError::Document { error });
                }
            };

        let checked = willikins_core::check(&doc_workflow, &self.catalog)
            .map_err(|errors| ButlerError::Check { errors })?;

        let description = willikins_core::describe(&checked, inputs);
        if !description.errors.is_empty() || !description.missing.is_empty() {
            return Err(ButlerError::Input {
                errors: description.errors,
                missing: description.missing,
            });
        }
        let resolved = description.resolved;

        let planned =
            willikins_core::plan(&checked, &resolved, &self.catalog).map_err(|error| {
                ButlerError::Plan {
                    error: Box::new(error),
                    attempt: crate::error::PlanAttempt::Initial,
                }
            })?;
        let fingerprint = planned.fingerprint();
        let class = planned.class;
        let requires_approval = planned.requires_approval;

        let plan_id = PlanId::new();
        let recorded = self.append(Event::PlanRecorded {
            plan_id,
            workflow: workflow.clone(),
            document_sha256,
            inputs: Redacted::from(&resolved),
            plan: Redacted::from(&planned),
            fingerprint,
            class,
            requires_approval,
            principal: Some(principal.clone()),
        })?;

        let (approval, expires_at) = if requires_approval {
            (
                ApprovalRequirement::Pending,
                expires_at(recorded.at, self.approval_window),
            )
        } else {
            self.append(Event::ApprovalAutomatic { plan_id, class })?;
            (
                ApprovalRequirement::Automatic,
                expires_at(recorded.at, self.apply_window),
            )
        };

        Ok(PlanResponse {
            plan_id,
            plan: planned,
            requires_approval,
            approval,
            expires_at,
        })
    }

    // -------------------------------------------------------------
    // approve / reject
    // -------------------------------------------------------------

    /// Grant a pending plan's approval.
    ///
    /// # Errors
    ///
    /// [`ButlerError::UnknownPlan`], [`ButlerError::NotPendingApproval`]
    /// (the plan never needed approval, or has already run),
    /// [`ButlerError::AlreadyDecided`], or
    /// [`ButlerError::PlanExpired`] (its approval window has elapsed).
    /// Refused decisions are not journaled as `ApplyRefused` -- only
    /// `apply`'s own refusals are; the `ToolCalled` this wraps is the
    /// only trace of a refused decision.
    pub fn approve(&self, plan_id: PlanId, approver: PrincipalId) -> Result<(), ButlerError> {
        let result = self.decide(plan_id, Decision::Grant(approver.clone()));
        self.record_tool_call("approve", None, approver, result.is_ok());
        result
    }

    /// Reject a pending plan.
    ///
    /// # Errors
    ///
    /// Same as [`Self::approve`].
    pub fn reject(
        &self,
        plan_id: PlanId,
        approver: PrincipalId,
        reason: Reason,
    ) -> Result<(), ButlerError> {
        let result = self.decide(plan_id, Decision::Reject(approver.clone(), reason));
        self.record_tool_call("reject", None, approver, result.is_ok());
        result
    }

    /// Record a decision for `plan_id`, atomically with the check that it
    /// is still undecided.
    ///
    /// **The journal lock is held across the whole read-check-append**,
    /// which is what makes "a decision is final" true under concurrency
    /// rather than only in a single-threaded test. Two callers deciding
    /// the same pending plan at once would otherwise both fold it as
    /// `Pending`, both append, and leave the plan carrying two decision
    /// events -- and the fold takes the *last* one, so an `approve`
    /// racing a `reject` would revive a rejected plan (adversarial pass
    /// 1, item 4, which is a journal-level fact this method is the only
    /// defence against). Pinned by
    /// `tests/adversarial_10a.rs::a_decision_landing_between_another_decisions_check_and_append_is_refused`.
    ///
    /// The clock is read *before* the lock is taken, deliberately: every
    /// other operation on this `Butler` blocks on the journal for as long
    /// as this guard is held, and a `Clock` implementation is not this
    /// module's to assume anything about. The instant it yields is at
    /// worst infinitesimally stale, which can only ever make this method
    /// more permissive by less than the time one lock acquisition takes,
    /// against a window measured in hours.
    fn decide(&self, plan_id: PlanId, decision: Decision) -> Result<(), ButlerError> {
        let now = self.clock.now();
        let mut journal = self.journal_lock();

        let record = journal
            .plan(&plan_id)
            .ok_or(ButlerError::UnknownPlan { plan_id })?;

        if !record.requires_approval || record.applied.is_some() {
            return Err(ButlerError::NotPendingApproval { plan_id });
        }
        if !matches!(record.approval, ApprovalState::Pending) {
            return Err(ButlerError::AlreadyDecided { plan_id });
        }
        if elapsed_between(record.recorded_at, now) > self.approval_window {
            return Err(ButlerError::PlanExpired {
                window: ExpiryWindow::Approval,
            });
        }

        let event = match decision {
            Decision::Grant(approver) => Event::ApprovalGranted { plan_id, approver },
            Decision::Reject(approver, reason) => Event::ApprovalRejected {
                plan_id,
                approver,
                reason,
            },
        };
        // `Journal::append` directly, not `self.append`: that helper takes
        // the journal lock itself, and `std::sync::Mutex` is not
        // reentrant, so calling it here would deadlock this thread.
        Journal::append(&mut *journal, event).map_err(|error| ButlerError::Journal {
            error: error.to_string(),
        })?;
        Ok(())
    }

    // -------------------------------------------------------------
    // apply
    // -------------------------------------------------------------

    /// Refuse or start applying `plan_id`.
    ///
    /// # Any agent principal may apply any recorded plan
    ///
    /// `principal` is recorded (`RunStarted.principal`) and never
    /// checked against the plan's own requester
    /// (`PlanRecorded.principal`, which adversarial pass 2 added). That
    /// is a decision, not an omission.
    ///
    /// What a plan is, is fixed at `plan` time: the workflow name, the
    /// document's SHA-256, the resolved inputs, the per-node actions and
    /// the class. `apply` re-verifies every one of them -- the document
    /// is reloaded and its hash compared, the inputs are rebuilt from the
    /// record, the workflow is re-planned against current provider state
    /// and refused on any drift, the approval must be a journaled
    /// decision for *that* plan id, and both windows must still be open.
    /// Nothing in that set depends on who is calling. A second agent
    /// token applying another agent's plan therefore runs exactly what
    /// the first agent's plan said and what the approver (for a plan
    /// above the threshold) actually saw.
    ///
    /// Refusing it would also invent a principal class the trust
    /// boundaries do not have: the plan names three principals -- agent,
    /// approver, operator -- with one agent *role*, and says the agent
    /// "may call every MCP tool". Agent principals are derived
    /// (`agent-<12 hex of the token hash>`), so rotating an agent token
    /// silently changes the principal; a requester check would make every
    /// token rotation strand every plan recorded before it, with no way
    /// to apply them and no way to say so. The audit trail keeps both
    /// names, which is what an operator actually needs.
    ///
    /// If a later milestone wants per-agent ownership it is an
    /// authorization feature with its own policy (who may take over a
    /// stale plan, and how), not a line in this function.
    ///
    /// Every refusal below is journaled as `ApplyRefused` before this
    /// returns -- including [`ButlerError::RunInProgress`], which is
    /// *decided* from the single-apply lock alone, with no journal read,
    /// but recorded all the same. Once every check
    /// passes, `RunStarted` is journaled, the lock is taken, and this
    /// returns a [`RunHandle`] at once: the run itself continues on a
    /// `std::thread`, calling `willikins_core::apply` with the plan this
    /// method just freshly re-planned (not the recorded, redacted one --
    /// see the module docs). `willikins_core::apply`'s own rules 1 and 2
    /// re-run the approval and drift checks internally as part of its own
    /// contract; that is redundant provider reads, never a redundant
    /// write, and the run is already committed (`RunStarted` recorded) by
    /// the time it happens, so a `Drift`/`Plan`/`ApprovalRequired` from
    /// inside the thread is treated as this run's own failure, not a new
    /// refusal (see [`continue_run_and_journal`]'s own doc).
    ///
    /// # Errors
    ///
    /// See [`ButlerError`]'s variants.
    #[allow(clippy::too_many_lines)] // one operation, the ordered refusal checks the plan spells out; splitting would scatter the sequence
    pub fn apply(&self, plan_id: PlanId, principal: PrincipalId) -> Result<RunHandle, ButlerError> {
        let result = self.apply_inner(plan_id, principal.clone());
        self.record_tool_call("apply", None, principal, result.is_ok());
        result
    }

    fn refuse_apply(&self, plan_id: PlanId, principal: PrincipalId, reason: ApplyRefusedReason) {
        let _ = self.append(Event::ApplyRefused {
            plan_id,
            principal,
            reason,
        });
    }

    #[allow(clippy::too_many_lines)]
    fn apply_inner(
        &self,
        plan_id: PlanId,
        principal: PrincipalId,
    ) -> Result<RunHandle, ButlerError> {
        let Some(record) = self.journal_lock().plan(&plan_id) else {
            self.refuse_apply(plan_id, principal, ApplyRefusedReason::UnknownPlan);
            return Err(ButlerError::UnknownPlan { plan_id });
        };

        // Claim the single-apply slot. The mutex is taken, read, changed
        // and dropped inside `claim` -- it is *not* held across the
        // checks below, which read the filesystem and call every planned
        // tool's `read`. Adversarial pass 2 found that holding it there
        // meant one slow or hung provider blocked every later `apply`
        // inside its own `spawn_blocking` thread, one thread each, until
        // the blocking pool was gone and nothing answered; and that
        // `run_in_progress()` (which the graceful-shutdown path polls
        // from async code) blocked a runtime worker on the same mutex.
        // `ApplyGuard` returns the slot to `Idle` on every path out of
        // this function, including a panic, unless it is committed to a
        // started run.
        let guard = match ApplyGuard::claim(&self.run_lock) {
            Ok(guard) => guard,
            Err(RunState::Running(run_id)) => {
                // Decided from the lock alone -- no journal *read* -- but
                // still journaled, like every other refusal: acceptance
                // test 8 lists this one and closes with "each refusal is
                // journaled", and it is the refusal that means two
                // callers reached for the same providers at once.
                self.refuse_apply(
                    plan_id,
                    principal,
                    ApplyRefusedReason::RunInProgress { run_id },
                );
                return Err(ButlerError::RunInProgress { run_id });
            }
            Err(RunState::Preparing) => {
                self.refuse_apply(plan_id, principal, ApplyRefusedReason::ApplyPreparing);
                return Err(ButlerError::ApplyPreparing);
            }
            Err(RunState::Idle) => unreachable!("claim only fails on a taken slot"),
        };

        // Re-read `applied` now that the slot is claimed, rather than
        // trusting the snapshot taken above it. `applied` is the one
        // field of a `PlanRecord` that `apply` itself can change, and
        // `RunStarted` -- the event that sets it -- is only ever appended
        // by a caller holding this same slot, so reading it here is what
        // makes "a plan is applied once" hold between two callers instead
        // of only within one. The snapshot above is a read-then-check
        // across an unclaimed slot: a caller that read `applied: None`,
        // lost the slot to a second caller, and claimed it after that
        // caller's whole run had finished would otherwise apply the plan
        // a second time. The window is a couple of instructions wide and
        // no test here reproduces it; the check is cheap and the
        // invariant is not one to leave resting on scheduling.
        let applied_now = self.journal_lock().plan(&plan_id).and_then(|r| r.applied);
        if let Some(run_id) = applied_now {
            self.refuse_apply(plan_id, principal, ApplyRefusedReason::AlreadyApplied);
            return Err(ButlerError::AlreadyApplied { plan_id, run_id });
        }

        let Ok(checked_now) = self.reload_and_check(&record) else {
            self.refuse_apply(plan_id, principal, ApplyRefusedReason::DocumentChanged);
            return Err(ButlerError::DocumentChanged {
                workflow: record.workflow.clone(),
            });
        };

        match &record.approval {
            ApprovalState::Pending => {
                if self.elapsed_since(record.recorded_at) > self.approval_window {
                    self.refuse_apply(plan_id, principal, ApplyRefusedReason::PlanExpired);
                    return Err(ButlerError::PlanExpired {
                        window: ExpiryWindow::Approval,
                    });
                }
                self.refuse_apply(plan_id, principal, ApplyRefusedReason::ApprovalRequired);
                return Err(ButlerError::ApprovalRequired {
                    class: record.class,
                });
            }
            ApprovalState::Rejected { .. } => {
                self.refuse_apply(plan_id, principal, ApplyRefusedReason::ApprovalRequired);
                return Err(ButlerError::ApprovalRequired {
                    class: record.class,
                });
            }
            ApprovalState::Automatic => {
                if self.elapsed_since(record.recorded_at) > self.apply_window {
                    self.refuse_apply(plan_id, principal, ApplyRefusedReason::PlanExpired);
                    return Err(ButlerError::PlanExpired {
                        window: ExpiryWindow::Apply,
                    });
                }
            }
            ApprovalState::Granted { at, .. } => {
                if self.elapsed_since(*at) > self.apply_window {
                    self.refuse_apply(plan_id, principal, ApplyRefusedReason::PlanExpired);
                    return Err(ButlerError::PlanExpired {
                        window: ExpiryWindow::Apply,
                    });
                }
            }
        }

        let approval = match &record.approval {
            ApprovalState::Automatic => Approval::Auto,
            ApprovalState::Granted { approver, at } => Approval::Human {
                approver: approver.clone(),
                at: *at,
            },
            ApprovalState::Pending | ApprovalState::Rejected { .. } => {
                unreachable!("both already refused above")
            }
        };

        let resolved_inputs = match resolve_recorded_inputs(&checked_now, &record.inputs) {
            Ok(inputs) => inputs,
            Err(error) => {
                // Its own reason since adversarial pass 2. It used to be
                // journaled as `PlanFailed { error_kind: "Unavailable" }`,
                // naming a `PlanError` kind that does not exist -- nothing
                // planned at all, and the fault is in the record, not the
                // provider.
                let input = match &error {
                    ButlerError::RecordedInputUnreadable { input, .. } => Some(input.clone()),
                    // `resolve_recorded_inputs`'s other failure is the
                    // whole recorded `inputs` payload being unreadable
                    // JSON, which names no single input.
                    _ => None,
                };
                self.refuse_apply(
                    plan_id,
                    principal,
                    ApplyRefusedReason::RecordedInputUnreadable { input },
                );
                return Err(error);
            }
        };

        let fresh = match willikins_core::plan(&checked_now, &resolved_inputs, &self.catalog) {
            Ok(fresh) => fresh,
            Err(error) => {
                self.refuse_apply(
                    plan_id,
                    principal,
                    ApplyRefusedReason::PlanFailed {
                        error_kind: plan_error_kind(&error),
                    },
                );
                return Err(ButlerError::Plan {
                    error: Box::new(error),
                    attempt: crate::error::PlanAttempt::RePlan,
                });
            }
        };
        let fresh_fp = fresh.fingerprint();

        if let Some(drifted) = drift::first_drift(&record.fingerprint, &fresh_fp) {
            self.refuse_apply(
                plan_id,
                principal,
                ApplyRefusedReason::Drift {
                    node: drifted.node.clone(),
                    instance: drifted.instance.clone(),
                    kind: drift_reason_kind(&drifted.detail),
                },
            );
            return Err(ButlerError::Drift {
                node: drifted.node,
                instance: drifted.instance,
                kind: Box::new(drifted.detail),
            });
        }

        let run_id = RunId::new();
        self.append(Event::RunStarted {
            run_id,
            plan_id,
            principal: principal.clone(),
        })?;
        guard.commit(run_id);

        let journal = self.journal.clone();
        let run_lock = Arc::clone(&self.run_lock);
        let catalog = Arc::clone(&self.catalog);
        let thread_principal = principal.clone();

        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                continue_run_and_journal(
                    journal.clone(),
                    thread_principal.clone(),
                    plan_id,
                    run_id,
                    |observer| {
                        willikins_core::apply(
                            &checked_now,
                            &resolved_inputs,
                            &catalog,
                            &fresh,
                            &approval,
                            observer,
                        )
                    },
                )
            }));
            if outcome.is_err() {
                let (node, instance) = fresh.nodes.first().map_or_else(
                    || {
                        unreachable!(
                            "an approved plan applied through this path always has at least one node"
                        )
                    },
                    |planned| (planned.name.clone(), planned.instance.clone()),
                );
                let error = ApplyError::Tool {
                    node,
                    instance,
                    error: ToolError {
                        kind: ToolErrorKind::Provider,
                        message: "the run thread panicked before it could finish".to_string(),
                    },
                    applied: Box::new(Applied {
                        nodes: Vec::new(),
                        outputs: IndexMap::new(),
                    }),
                };
                let mut journal = journal;
                let _ = journal.append(Event::RunFinished {
                    run_id,
                    outcome: willikins_journal::Outcome::Failed {
                        error: Redacted::from(&error),
                    },
                });
            }
            *run_lock.lock().unwrap_or_else(PoisonError::into_inner) = RunState::Idle;
        });

        Ok(RunHandle { run_id })
    }

    /// Reload the document `record.workflow` names, verify its bytes and
    /// internal name still match what was recorded, and re-`check` it.
    /// `Err(())` covers every way that can fail: `apply` reports all of
    /// them as `ButlerError::DocumentChanged`, since a document that no
    /// longer parses or checks the way it did at `plan` time is, from an
    /// approved plan's point of view, exactly as changed as one whose
    /// bytes differ.
    fn reload_and_check(&self, record: &PlanRecord) -> Result<willikins_core::Checked, ()> {
        let (document_sha256, workflow) =
            document::load_named_document(&self.workflows_dir, &record.workflow).map_err(|_| ())?;
        if document_sha256 != record.document_sha256 {
            return Err(());
        }
        willikins_core::check(&workflow, &self.catalog).map_err(|_| ())
    }

    // -------------------------------------------------------------
    // views
    // -------------------------------------------------------------

    /// One recorded run, by id, from the journal's own fold.
    #[must_use]
    pub fn run(&self, run_id: RunId) -> Option<RunRecord> {
        self.journal_lock().run(&run_id)
    }

    /// Every recorded run, from the journal's own fold.
    #[must_use]
    pub fn runs(&self) -> Vec<RunRecord> {
        self.journal_lock().runs()
    }

    /// Every plan still waiting on a human decision, from the journal's
    /// own fold.
    #[must_use]
    pub fn pending_approvals(&self) -> Vec<PlanRecord> {
        self.journal_lock().pending_approvals()
    }

    /// Who called `plan` for `plan_id`, read from the journal's own
    /// `PlanRecorded` line -- the approvals page's "requester" field.
    ///
    /// Adversarial pass 2 moved this off process-lifetime memory and onto
    /// the wire (`Event::PlanRecorded.principal`), so a plan recorded
    /// before a restart still names who asked for it: approval is
    /// human-paced by design and a redeploy between `plan` and a
    /// decision is the normal case, not an edge one. `None` now means
    /// only one thing -- the line was written before that field existed
    /// -- and the page still labels that "unknown" rather than
    /// pretending certainty.
    ///
    /// Read by the page, never by `apply`: the requester is audit, not
    /// authorization. See [`Self::apply`]'s own doc.
    #[must_use]
    pub fn requested_by(&self, plan_id: PlanId) -> Option<PrincipalId> {
        self.journal_lock()
            .plan(&plan_id)
            .and_then(|record| record.requested_by)
    }

    /// The current time, from this `Butler`'s own [`Clock`] -- so a caller
    /// computing a plan's age (task 10b's approvals page) never drifts
    /// from the clock `apply`'s own window checks use.
    #[must_use]
    pub fn now(&self) -> willikins_journal::Timestamp {
        self.clock.now()
    }

    /// This `Butler`'s configured approval window: how long a pending
    /// plan's single-use approval nonce (task 10b) stays valid, matching
    /// exactly how long [`Self::decide`] itself still accepts a decision
    /// for.
    #[must_use]
    pub fn approval_window(&self) -> Duration {
        self.approval_window
    }

    /// Record an authentication failure reached over `transport`, for
    /// `reason` -- the audit trail's only trace of a request the HTTP
    /// transport (task 10b) refused before it ever reached a `Butler`
    /// operation. Swallows a journal write failure, like
    /// [`Self::record_tool_call`]: the caller's HTTP response does not
    /// depend on whether the journal accepted the write.
    pub fn record_auth_failure(
        &self,
        transport: willikins_journal::Transport,
        reason: willikins_journal::AuthFailedReason,
    ) {
        let _ = self.append(Event::AuthFailed { transport, reason });
    }

    /// This `Butler`'s trusted workflow directory -- task 10b's
    /// approvals page reloads a pending plan's document from here (best
    /// effort, for its "document says:" section) rather than trusting
    /// anything cached from `plan` time.
    #[must_use]
    pub fn workflows_dir(&self) -> &std::path::Path {
        &self.workflows_dir
    }

    /// The run currently in progress, if any -- task 10b's `serve_http`
    /// graceful shutdown polls this (bounded) so an in-progress run's
    /// `NodeStarted`/`NodeFinished` pairs have a chance to land in the
    /// journal before the process exits.
    ///
    /// Returns `None` while an `apply` is still in its pre-run checks:
    /// nothing has run, so there is nothing to drain. The mutex behind
    /// this is never held across any I/O (see [`RunState`]), which is
    /// what makes it safe to call from async code.
    #[must_use]
    pub fn run_in_progress(&self) -> Option<RunId> {
        match *self.run_lock.lock().unwrap_or_else(PoisonError::into_inner) {
            RunState::Running(run_id) => Some(run_id),
            RunState::Idle | RunState::Preparing => None,
        }
    }
}

enum Decision {
    Grant(PrincipalId),
    Reject(PrincipalId, Reason),
}

/// The single-apply slot: what `apply` is doing, if anything.
///
/// Three states rather than task 10a's `Option<RunId>`, because there
/// are three situations and a caller deserves to be told which: nothing
/// is happening, one `apply` is running its pre-run checks (no run id
/// exists yet, and one may never exist), or a run is under way.
/// Adversarial pass 2's change; see [`Butler::apply`] and
/// [`ApplyGuard`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunState {
    /// No `apply` is in flight.
    Idle,
    /// One `apply` is between claiming the slot and journaling
    /// `RunStarted` -- reloading the document, rebuilding the recorded
    /// inputs, re-planning against providers, comparing fingerprints.
    Preparing,
    /// A run is under way; the run thread clears the slot when it ends.
    Running(RunId),
}

/// Holds the single-apply slot in [`RunState::Preparing`] for as long as
/// `apply`'s pre-run checks take, and returns it to [`RunState::Idle`] on
/// *every* way out -- an early refusal, a `?`, or a panic -- unless
/// [`Self::commit`] hands it to a started run.
///
/// The `Drop` is the point: `apply` has nine early returns between
/// claiming the slot and journaling `RunStarted`, and a slot that stayed
/// claimed after one of them would refuse every later `apply` for the
/// life of the process, with nothing running.
struct ApplyGuard {
    lock: Arc<Mutex<RunState>>,
    committed: bool,
}

impl ApplyGuard {
    /// Claim the slot, or report the state that already holds it.
    fn claim(lock: &Arc<Mutex<RunState>>) -> Result<Self, RunState> {
        let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        match *guard {
            RunState::Idle => {
                *guard = RunState::Preparing;
                drop(guard);
                Ok(Self {
                    lock: Arc::clone(lock),
                    committed: false,
                })
            }
            taken => Err(taken),
        }
    }

    /// Hand the slot to `run_id`: the run thread clears it when it ends.
    fn commit(mut self, run_id: RunId) {
        *self.lock.lock().unwrap_or_else(PoisonError::into_inner) = RunState::Running(run_id);
        self.committed = true;
    }
}

impl Drop for ApplyGuard {
    fn drop(&mut self) {
        if !self.committed {
            *self.lock.lock().unwrap_or_else(PoisonError::into_inner) = RunState::Idle;
        }
    }
}

/// How long elapsed from `since` to `now`, saturating at zero if `now` is
/// the earlier of the two (a `Clock` that stepped backwards).
fn elapsed_between(
    since: willikins_journal::Timestamp,
    now: willikins_journal::Timestamp,
) -> Duration {
    (*now.as_datetime() - *since.as_datetime())
        .to_std()
        .unwrap_or(Duration::ZERO)
}

/// Rebuild a plan's resolved inputs from the journal's own already-redacted
/// record, against `checked`'s freshly reloaded (and hash-checked) input
/// specs. See the module docs' "A plan's resolved inputs survive a
/// restart" section.
///
/// # Errors
///
/// [`ButlerError::RecordedInputUnreadable`] naming the first input whose
/// recorded value is missing or no longer parses against its declared
/// type -- refused rather than panicking, though this cannot happen for a
/// document whose hash still matches the one `plan` recorded (the caller
/// checks that first, via `Butler::reload_and_check`).
fn resolve_recorded_inputs(
    checked: &Checked,
    inputs: &Redacted<IndexMap<InputName, Value>>,
) -> Result<IndexMap<InputName, Value>, ButlerError> {
    let raw: IndexMap<InputName, serde_json::Value> =
        serde_json::from_value(inputs.as_json().clone()).map_err(|error| ButlerError::Journal {
            error: format!("the recorded plan inputs are not valid JSON: {error}"),
        })?;

    let mut resolved = IndexMap::new();
    for (name, spec) in &checked.workflow.inputs {
        let entry = raw
            .get(name)
            .ok_or_else(|| ButlerError::RecordedInputUnreadable {
                input: name.clone(),
                error: ParseError::new(
                    "Value",
                    format!("the plan recorded no value for input `{name}`"),
                ),
            })?;
        let value = parse_recorded_value(&spec.ty, entry).map_err(|error| {
            ButlerError::RecordedInputUnreadable {
                input: name.clone(),
                error,
            }
        })?;
        resolved.insert(name.clone(), value);
    }
    Ok(resolved)
}

/// Parse one input's recorded JSON (`willikins_core::value::Value`'s own
/// pinned shape: `{"type", "list", "state", "value", ...}`) back into a
/// live [`Value`], against its declared type `ty`. Every workflow input
/// is non-secret by construction (`check` refuses a secret one), so
/// `entry["state"]` is always `"known"` and `entry["value"]` is always
/// present -- a scalar string, or an array of strings for a list -- but
/// this checks rather than assumes, so a hand-edited or otherwise
/// malformed record is refused, not panicked on.
fn parse_recorded_value(ty: &TypeRef, entry: &serde_json::Value) -> Result<Value, ParseError> {
    let bad_shape = || {
        ParseError::new(
            "Value",
            format!("recorded input value is not the expected shape: {entry}"),
        )
    };
    if entry.get("state").and_then(serde_json::Value::as_str) != Some("known") {
        return Err(bad_shape());
    }
    let value = entry.get("value").ok_or_else(bad_shape)?;
    if ty.list {
        let items = value.as_array().ok_or_else(bad_shape)?;
        let strings = items
            .iter()
            .map(|item| item.as_str().ok_or_else(bad_shape))
            .collect::<Result<Vec<_>, _>>()?;
        Value::parse_list(ty, &strings)
    } else {
        let text = value.as_str().ok_or_else(bad_shape)?;
        Value::parse(ty, text)
    }
}

/// `from` plus `window`, as a [`willikins_journal::Timestamp`].
fn expires_at(
    from: willikins_journal::Timestamp,
    window: Duration,
) -> willikins_journal::Timestamp {
    let delta = chrono::Duration::from_std(window)
        .unwrap_or_else(|_| unreachable!("a configured window fits in a chrono::Duration"));
    willikins_journal::Timestamp::from_datetime(*from.as_datetime() + delta)
}

/// A [`PlanError`]'s own internally tagged `kind`, mirroring
/// `willikins-journal`'s private helper of the same shape (not exported,
/// so this crate keeps its own copy rather than depending on an
/// implementation detail).
fn plan_error_kind(error: &PlanError) -> String {
    serde_json::to_value(error)
        .ok()
        .and_then(|json| {
            json.get("kind")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Strip [`DriftDetail`] down to [`willikins_journal::DriftReasonKind`]:
/// same shape, no planned or observed value -- the journal never records
/// what almost happened to look like, only that it did.
fn drift_reason_kind(detail: &DriftDetail) -> willikins_journal::DriftReasonKind {
    match detail {
        DriftDetail::Instance { .. } => willikins_journal::DriftReasonKind::Instance,
        DriftDetail::Action { .. } => willikins_journal::DriftReasonKind::Action,
        DriftDetail::Output { port, .. } => {
            willikins_journal::DriftReasonKind::Output { port: port.clone() }
        }
    }
}
