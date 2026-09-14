//! [`Butler`]: the plan/approve/apply operations both surfaces
//! (task 10b's rmcp tools, the CLI) call. Owns the trusted workflow
//! directory, the catalog, the journal, and the single-apply lock.
//!
//! # Why `apply` keeps its own `plan_id -> resolved inputs` map
//!
//! `willikins_journal::journal::PlanRecord::inputs` is a
//! `Redacted<IndexMap<InputName, Value>>`: opaque, already-redacted JSON
//! with no way back to a live `Value` (`willikins_core::value::Value` has
//! no `Deserialize` at all -- redaction is one-way, by design). `apply`'s
//! own re-plan step needs the *real*, typed inputs to call
//! `willikins_core::plan` again. This module keeps them in an in-memory
//! map, populated by [`Butler::plan`] and read by [`Butler::apply`],
//! rather than trying to reconstruct them from the redacted JSON (every
//! workflow input is non-secret by construction -- `check` refuses a
//! secret one -- so it would be *possible* in principle, by re-parsing
//! each rendered string against its declared type, but that duplicates
//! `describe`'s own parsing logic for no benefit this task's tests need).
//!
//! **Known limitation, recorded rather than hidden**: a plan recorded in
//! a process that then restarts (a `FileJournal` reopened by a fresh
//! `Butler`) cannot be applied -- its resolved inputs are gone from
//! memory even though the journal itself replayed cleanly. `apply`
//! reports this as [`ButlerError::Journal`] rather than panicking or
//! silently using stale data. Closing this properly (durable resolved
//! inputs, or folding them back out of the redacted JSON) is a
//! `willikins-server` follow-up beyond this task's scope; the journal's
//! own durability and the `FileJournal` round-trip acceptance test are
//! unaffected because that test never restarts `Butler` mid-flow, only
//! `FileJournal` itself, after `apply` has already finished.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use indexmap::IndexMap;

use willikins_core::describe::PartialInputs;
use willikins_core::{
    Applied, ApplyError, Approval, Catalog, InputName, PlanError, ToolError, ToolErrorKind,
    ToolName, Value,
};
use willikins_journal::{
    Append, ApplyRefusedReason, ApprovalState, Clock, Entry, Event, Journal, PlanId, PlanRecord,
    PrincipalId, Reason, Redacted, RunId, RunRecord, continue_run_and_journal,
};
use willikins_types::{DomainType, WorkflowName};

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
    /// `Some(run_id)` while a run is in progress; cleared by the run
    /// thread itself when it finishes (success, failure, or a caught
    /// panic) -- never held across the run, only across each `apply`
    /// call's own synchronous checks. See [`Butler::apply`]'s doc.
    run_lock: Arc<Mutex<Option<RunId>>>,
    /// See the module docs' "Why `apply` keeps its own ... map" section.
    plan_inputs: Arc<Mutex<HashMap<PlanId, IndexMap<InputName, Value>>>>,
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
            run_lock: Arc::new(Mutex::new(None)),
            plan_inputs: Arc::new(Mutex::new(HashMap::new())),
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
    pub fn list_workflows(&self, principal: PrincipalId) -> Result<Vec<WorkflowSummary>, ButlerError> {
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
    fn load_source(&self, source: &DocumentSource) -> Result<willikins_core::Workflow, ButlerError> {
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
        source: DocumentSource,
        principal: PrincipalId,
    ) -> Result<ValidateResponse, ButlerError> {
        if let Err(retry_after_seconds) = self.read_rate_limiter.check(&principal) {
            self.record_tool_call("validate", None, principal, false);
            return Err(ButlerError::RateLimited {
                retry_after_seconds,
            });
        }
        let result = self.validate_inner(&source);
        self.record_tool_call(
            "validate",
            Self::source_workflow_name(&source),
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
        source: DocumentSource,
        partial: &PartialInputs,
        principal: PrincipalId,
    ) -> Result<willikins_core::Description, ButlerError> {
        if let Err(retry_after_seconds) = self.read_rate_limiter.check(&principal) {
            self.record_tool_call("describe", None, principal, false);
            return Err(ButlerError::RateLimited {
                retry_after_seconds,
            });
        }
        let result = self.describe_inner(&source, partial);
        self.record_tool_call(
            "describe",
            Self::source_workflow_name(&source),
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
        let result = self.propose_slug_inner(name);
        self.record_tool_call("propose_slug", None, principal, result.is_ok());
        result
    }

    fn propose_slug_inner(&self, name: &str) -> Result<ProposeSlugResponse, ButlerError> {
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
            message: error.to_string(),
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
        let now = self.clock.now();
        (*now.as_datetime() - *since.as_datetime())
            .to_std()
            .unwrap_or(Duration::ZERO)
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
        let result = self.plan_inner(&workflow, inputs);
        self.record_tool_call("plan", Some(workflow), principal, result.is_ok());
        result
    }

    fn plan_inner(
        &self,
        workflow: &WorkflowName,
        inputs: &PartialInputs,
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

        let planned = willikins_core::plan(&checked, &resolved, &self.catalog)
            .map_err(|error| ButlerError::Plan { error })?;
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
        })?;

        self.plan_inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(plan_id, resolved);

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

    fn decide(&self, plan_id: PlanId, decision: Decision) -> Result<(), ButlerError> {
        let record = self
            .journal_lock()
            .plan(&plan_id)
            .ok_or(ButlerError::UnknownPlan { plan_id })?;

        if !record.requires_approval || record.applied.is_some() {
            return Err(ButlerError::NotPendingApproval { plan_id });
        }
        if !matches!(record.approval, ApprovalState::Pending) {
            return Err(ButlerError::AlreadyDecided { plan_id });
        }
        if self.elapsed_since(record.recorded_at) > self.approval_window {
            return Err(ButlerError::PlanExpired {
                window: ExpiryWindow::Approval,
            });
        }

        match decision {
            Decision::Grant(approver) => {
                self.append(Event::ApprovalGranted { plan_id, approver })?;
            }
            Decision::Reject(approver, reason) => {
                self.append(Event::ApprovalRejected {
                    plan_id,
                    approver,
                    reason,
                })?;
            }
        }
        Ok(())
    }

    // -------------------------------------------------------------
    // apply
    // -------------------------------------------------------------

    /// Refuse or start applying `plan_id`.
    ///
    /// Every refusal below (except [`ButlerError::RunInProgress`], which
    /// answers from the single-apply lock alone -- see its own doc) is
    /// journaled as `ApplyRefused` before this returns. Once every check
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

        let mut run_guard = self.run_lock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(run_id) = *run_guard {
            // Answered from the lock alone: no journal read, no
            // `ApplyRefused` write, per the module doc.
            return Err(ButlerError::RunInProgress { run_id });
        }

        if let Some(run_id) = record.applied {
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

        let Some(resolved_inputs) = self
            .plan_inputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&plan_id)
            .cloned()
        else {
            let error = ButlerError::Journal {
                message: "this plan's resolved inputs are not available in this server process \
                          (it may have restarted since `plan`); plan again"
                    .to_string(),
            };
            self.refuse_apply(
                plan_id,
                principal,
                ApplyRefusedReason::PlanFailed {
                    error_kind: "Unavailable".to_string(),
                },
            );
            return Err(error);
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
                return Err(ButlerError::Plan { error });
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
        *run_guard = Some(run_id);
        drop(run_guard);

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
            *run_lock.lock().unwrap_or_else(PoisonError::into_inner) = None;
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
}

enum Decision {
    Grant(PrincipalId),
    Reject(PrincipalId, Reason),
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
