//! [`ButlerError`]: everything a [`crate::Butler`] operation can refuse
//! with. Kind-tagged the same way every other error in this workspace is
//! (`{"kind": "...", "message": "...", ...fields}` through
//! [`willikins_core::Reported`]), so `willikins-server`'s later MCP tools
//! (task 10b) and the CLI print it exactly like a `CheckError` or a
//! `PlanError`.

use std::fmt;

use willikins_core::describe::{InputError, MissingInput};
use willikins_core::{CheckError, Class, InputName, NodeName, PlanError};
use willikins_dsl::DocumentError;
use willikins_journal::{PlanId, RunId};
use willikins_types::{ParseError, ProposeError, WorkflowName};

use crate::drift::DriftDetail;
use crate::startup::StartupError;

/// Which window a plan overran. See the plan's "Plan identity" trust
/// boundary: the *approval* window bounds how long a plan may wait,
/// undecided, for a human; the *apply* window bounds how stale an
/// approved (or auto-approved) plan may be by the time it actually runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpiryWindow {
    /// The plan sat undecided past its approval window.
    Approval,
    /// The plan was applied too long after it became runnable (approval,
    /// or `plan` itself for an automatically-approved one).
    Apply,
}

impl fmt::Display for ExpiryWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Approval => write!(f, "approval"),
            Self::Apply => write!(f, "apply"),
        }
    }
}

/// Everything a [`crate::Butler`] operation can refuse with.
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`), the same
/// convention `CheckError`/`PlanError`/`ApplyError` use; no variant
/// declares a field named `kind` or `message` (pinned by
/// `tests::every_butler_error_variant_serializes_with_its_kind`, in the
/// same `variant_kinds!` style `willikins-core`'s own error enums use).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum ButlerError {
    /// `plan_id` names no `PlanRecorded` event this journal has.
    UnknownPlan {
        /// The unrecognised id.
        plan_id: PlanId,
    },
    /// The trusted workflow directory holds no document named `workflow`.
    UnknownWorkflow {
        /// The requested name.
        workflow: WorkflowName,
    },
    /// The named document's bytes (or its own internal name) no longer
    /// match what `plan` recorded.
    DocumentChanged {
        /// The workflow whose document changed.
        workflow: WorkflowName,
    },
    /// The plan overran `window`.
    PlanExpired {
        /// Which window.
        window: ExpiryWindow,
    },
    /// The plan requires human approval and none has been given (or it
    /// was rejected).
    ApprovalRequired {
        /// The plan's approval class.
        class: Class,
    },
    /// `plan_id` already has an approval decision recorded; a second
    /// `approve` or `reject` is refused rather than silently overwriting
    /// it (adversarial pass 1, item 2 --
    /// `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`).
    AlreadyDecided {
        /// The already-decided plan.
        plan_id: PlanId,
    },
    /// `approve` or `reject` was called for a plan that never needed a
    /// decision (its class did not require approval), or one that has
    /// already run.
    NotPendingApproval {
        /// The plan.
        plan_id: PlanId,
    },
    /// `plan_id` has already been applied.
    AlreadyApplied {
        /// The already-applied plan.
        plan_id: PlanId,
        /// The run it was applied as.
        run_id: RunId,
    },
    /// A run is already in progress; `apply` refuses rather than queuing
    /// a second one. Decided from the single-apply lock alone, without
    /// consulting the journal's own fold -- but still journaled as an
    /// `ApplyRefused`, like every other refusal.
    RunInProgress {
        /// The run in progress.
        run_id: RunId,
    },
    /// A fresh re-plan disagrees with the plan that was approved, at one
    /// instance. Nothing was executed.
    Drift {
        /// The node whose instance drifted.
        node: NodeName,
        /// Its `for_each` instance key, if any.
        instance: Option<String>,
        /// How it drifted. Boxed (`Box<T>` serializes exactly as `T`
        /// would) so this variant does not dominate `ButlerError`'s
        /// overall size (`clippy::result_large_err`), the same reason
        /// `willikins_core::ApplyError::Drift`'s own `kind` is boxed.
        #[serde(rename = "detail")]
        kind: Box<DriftDetail>,
    },
    /// `apply`'s fold of the journal's own `PlanRecorded.inputs` (rebuilt
    /// after a possible process restart -- see `crate::butler`'s module
    /// docs) found a value that is missing or no longer parses against
    /// the reloaded document's declared type for that input. Cannot
    /// happen for a document whose hash still matches the one `plan`
    /// recorded, which `apply` checks first, but refused rather than
    /// panicked on.
    RecordedInputUnreadable {
        /// The input whose recorded value could not be read back.
        input: InputName,
        /// Why.
        error: ParseError,
    },
    /// Re-planning the approved workflow, against the catalog's current
    /// state, failed outright.
    Plan {
        /// The failure.
        error: PlanError,
    },
    /// The reloaded document failed `check` against the catalog.
    Check {
        /// Every failure. Each element carries its own `kind` (`CheckError`
        /// is itself internally tagged) and, on the wire, `message` too --
        /// wrapped through [`willikins_core::Reported`] the same way
        /// `willikins-server`'s `ValidateResponse.errors` already is (see
        /// `todos/2026-09-12-error-json-uniformity-gaps.md` item 5), via
        /// [`crate::read_ops::serialize_reported`].
        #[serde(serialize_with = "crate::read_ops::serialize_reported")]
        errors: Vec<CheckError>,
    },
    /// The named document could not be loaded or parsed.
    Document {
        /// The failure.
        error: DocumentError,
    },
    /// The supplied inputs did not resolve: some were rejected, or some
    /// declared input has neither a value nor a default.
    Input {
        /// Every rejected raw value or unrecognised input name. Each
        /// element carries `message` too, the same way [`Self::Check`]'s
        /// `errors` does (`InputError` itself has no internal `kind` tag --
        /// it is a plain struct, not an enum -- so, unlike `CheckError`,
        /// only `message` is added; see the field's own `pub(crate)`
        /// helper for the reasoning this reuses).
        #[serde(serialize_with = "crate::read_ops::serialize_reported")]
        errors: Vec<InputError>,
        /// Every declared input with no value at all. Left as `describe`
        /// produces it (a *successful*-response shape elsewhere; see
        /// `todos/2026-09-12-error-json-uniformity-gaps.md` item 1) --
        /// task 11's own instructions name only `errors` for wrapping.
        missing: Vec<MissingInput>,
    },
    /// The journal itself could not record or be read.
    Journal {
        /// What went wrong. Named `error`, not `message`: this type is
        /// serialized through [`Reported`](willikins_core::Reported),
        /// which adds a `message` of its own by `serde(flatten)`, and
        /// flatten resolves a collision by writing *both* keys -- which
        /// every JSON reader then folds back to one, silently, with no
        /// rule saying which. See the enum's own doc.
        error: String,
    },
    /// Another `apply` call is in its pre-run checks (reload, rebuild,
    /// re-plan, drift) and only one does that at a time. No run has
    /// started, and the plan is still applicable: the caller retries.
    ///
    /// Distinct from [`Self::RunInProgress`], which has a run to name.
    /// See `crate::Butler::apply`'s own doc.
    ApplyPreparing,
    /// A directory scan (`start`, `list_workflows`) failed. See
    /// [`StartupError`] for what it names.
    Startup {
        /// The underlying failure.
        error: StartupError,
    },
    /// A principal exceeded its call rate for `plan`, or the combined
    /// `describe`/`validate` bucket. See the "Rate limits" section of the
    /// `willikins-server` plan section: `plan` at most 10/minute,
    /// `describe`+`validate` together at most 60/minute by default, per
    /// principal, refilled from the shared [`willikins_journal::Clock`].
    RateLimited {
        /// How long, at minimum, before this principal's bucket has room
        /// for another call in this class.
        retry_after_seconds: u64,
    },
    /// `propose_slug`'s own `name` argument is not a valid
    /// `willikins_types::ProjectName`.
    InvalidProjectName {
        /// The parse failure.
        error: ParseError,
    },
    /// `willikins_types::propose_slug` itself refused the (valid)
    /// project name.
    SlugProposal {
        /// The refusal.
        error: ProposeError,
    },
}

impl fmt::Display for ButlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPlan { plan_id } => write!(f, "no plan `{plan_id}` is recorded"),
            Self::UnknownWorkflow { workflow } => {
                write!(
                    f,
                    "no workflow named `{workflow}` is in the trusted directory"
                )
            }
            Self::DocumentChanged { workflow } => write!(
                f,
                "the document for `{workflow}` has changed since this plan was recorded"
            ),
            Self::PlanExpired { window } => write!(f, "the plan's {window} window has elapsed"),
            Self::ApprovalRequired { class } => write!(
                f,
                "this plan is {class:?} and requires approval before it can run"
            ),
            Self::AlreadyDecided { plan_id } => {
                write!(f, "plan `{plan_id}` already has an approval decision")
            }
            Self::NotPendingApproval { plan_id } => {
                write!(f, "plan `{plan_id}` is not waiting on an approval decision")
            }
            Self::AlreadyApplied { plan_id, run_id } => {
                write!(f, "plan `{plan_id}` was already applied as run `{run_id}`")
            }
            Self::RunInProgress { run_id } => {
                write!(
                    f,
                    "run `{run_id}` is already in progress; try again once it finishes"
                )
            }
            Self::Drift { node, instance, .. } => {
                let where_ = match instance {
                    Some(key) => format!("{node}[{key}]"),
                    None => node.to_string(),
                };
                write!(
                    f,
                    "{where_}: the current state no longer matches the approved plan; nothing was run"
                )
            }
            Self::RecordedInputUnreadable { input, error } => {
                write!(
                    f,
                    "recorded input `{input}` could not be read back: {error}"
                )
            }
            Self::ApplyPreparing => write!(
                f,
                "another apply is running its pre-run checks; retry in a moment"
            ),
            Self::Plan { error } => write!(f, "re-planning failed: {error}"),
            Self::Check { errors } => write!(
                f,
                "the document no longer checks: {} error(s)",
                errors.len()
            ),
            Self::Document { error } => write!(f, "{error}"),
            Self::Input { errors, missing } => write!(
                f,
                "{} input error(s), {} missing input(s)",
                errors.len(),
                missing.len()
            ),
            Self::Journal { error } => write!(f, "journal: {error}"),
            Self::Startup { error } => write!(f, "{error}"),
            Self::RateLimited {
                retry_after_seconds,
            } => write!(
                f,
                "rate limit exceeded; try again in {retry_after_seconds} second(s)"
            ),
            Self::InvalidProjectName { error } => write!(f, "{error}"),
            Self::SlugProposal { error } => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ButlerError {}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! variant_kinds {
        ($fn_name:ident, $count:ident, $enum:ident, $($variant:ident),+ $(,)?) => {
            fn $fn_name(value: &$enum) -> &'static str {
                match value {
                    $($enum::$variant { .. } => stringify!($variant),)+
                }
            }

            const $count: usize = [$(stringify!($variant)),+].len();
        };
    }

    variant_kinds!(
        kind_of,
        VARIANT_COUNT,
        ButlerError,
        UnknownPlan,
        UnknownWorkflow,
        DocumentChanged,
        PlanExpired,
        ApprovalRequired,
        AlreadyDecided,
        NotPendingApproval,
        AlreadyApplied,
        RunInProgress,
        ApplyPreparing,
        Drift,
        RecordedInputUnreadable,
        Plan,
        Check,
        Document,
        Input,
        Journal,
        Startup,
        RateLimited,
        InvalidProjectName,
        SlugProposal,
    );

    fn plan_id() -> PlanId {
        PlanId::new()
    }

    fn run_id() -> RunId {
        RunId::new()
    }

    fn node_name() -> NodeName {
        NodeName::parse("n").unwrap()
    }

    fn workflow_name() -> WorkflowName {
        use willikins_types::DomainType;
        WorkflowName::parse("wf").unwrap()
    }

    fn samples() -> Vec<ButlerError> {
        vec![
            ButlerError::UnknownPlan { plan_id: plan_id() },
            ButlerError::UnknownWorkflow {
                workflow: workflow_name(),
            },
            ButlerError::DocumentChanged {
                workflow: workflow_name(),
            },
            ButlerError::PlanExpired {
                window: ExpiryWindow::Approval,
            },
            ButlerError::ApprovalRequired {
                class: Class::Irreversible,
            },
            ButlerError::AlreadyDecided { plan_id: plan_id() },
            ButlerError::NotPendingApproval { plan_id: plan_id() },
            ButlerError::AlreadyApplied {
                plan_id: plan_id(),
                run_id: run_id(),
            },
            ButlerError::RunInProgress { run_id: run_id() },
            ButlerError::ApplyPreparing,
            ButlerError::Drift {
                node: node_name(),
                instance: None,
                kind: Box::new(DriftDetail::Action {
                    planned: willikins_core::Action::Create,
                    observed: willikins_core::Action::NoOp,
                }),
            },
            ButlerError::RecordedInputUnreadable {
                input: willikins_core::InputName::parse("x").unwrap(),
                error: ParseError::new("Value", "boom"),
            },
            ButlerError::Plan {
                error: PlanError::MissingInput {
                    input: willikins_core::InputName::parse("x").unwrap(),
                },
            },
            ButlerError::Check { errors: Vec::new() },
            ButlerError::Document {
                error: willikins_dsl::parse_document("").unwrap_err(),
            },
            ButlerError::Input {
                errors: Vec::new(),
                missing: Vec::new(),
            },
            ButlerError::Journal {
                error: "boom".to_string(),
            },
            ButlerError::Startup {
                error: StartupError::Symlink {
                    path: std::path::PathBuf::from("x.yaml"),
                },
            },
            ButlerError::RateLimited {
                retry_after_seconds: 5,
            },
            ButlerError::InvalidProjectName {
                error: ParseError::new("ProjectName", "bad"),
            },
            ButlerError::SlugProposal {
                error: ProposeError::NoWords {
                    input: "!!!".to_string(),
                },
            },
        ]
    }

    /// Closes `todos/2026-09-12-error-json-uniformity-gaps.md`'s ask, for
    /// `ButlerError`: every variant, not just some, serializes through
    /// [`willikins_core::Reported`] with both `kind` (its own internal tag)
    /// and `message` (its own `Display`), never colliding (no variant
    /// declares a field named `kind` or `message` -- see the enum's own
    /// doc).
    #[test]
    fn every_variant_is_represented_and_serializes_with_its_kind_and_message() {
        let samples = samples();
        assert_eq!(
            samples.len(),
            VARIANT_COUNT,
            "add a sample for every new ButlerError variant"
        );
        for sample in &samples {
            let expected_kind = kind_of(sample);

            let plain = serde_json::to_value(sample).unwrap();
            assert_eq!(plain["kind"], expected_kind, "{sample:?}");

            // No variant may declare a field named `message`: `Reported`
            // adds one with `serde(flatten)`, and flatten resolves a
            // collision by writing *both* keys rather than refusing. This
            // assertion runs before `Reported` is involved at all, so a
            // colliding field shows up here on its own -- which the
            // `json["message"]` check below cannot do, since parsing into
            // a `serde_json::Value` folds a duplicate key silently. A
            // field literally named `kind` is a compile error under
            // `#[serde(tag = "kind")]`, so that half has a backstop
            // already. `ButlerError::Journal` did carry a `message` field
            // until this check was written; it is `error` now.
            assert!(
                plain.as_object().unwrap().get("message").is_none(),
                "ButlerError::{expected_kind} must not have a field named `message`: {plain}"
            );

            let reported = willikins_core::Reported::new(sample);
            let json = serde_json::to_value(reported).unwrap();
            assert_eq!(json["kind"], expected_kind, "{sample:?}");
            let message = json["message"]
                .as_str()
                .unwrap_or_else(|| panic!("{sample:?} has no `message` field"));
            assert!(!message.is_empty(), "{sample:?}");
        }
    }

    #[test]
    fn every_variant_displays_and_is_a_std_error() {
        for sample in samples() {
            let _: &dyn std::error::Error = &sample;
            assert!(!sample.to_string().is_empty());
        }
    }

    #[test]
    fn reported_carries_kind_and_message() {
        let error = ButlerError::UnknownPlan { plan_id: plan_id() };
        let reported = willikins_core::Reported::new(&error);
        let json = serde_json::to_value(reported).unwrap();
        assert_eq!(json["kind"], "UnknownPlan");
        assert!(json["message"].as_str().unwrap().contains("no plan"));
    }

    /// Closes `todos/2026-09-12-error-json-uniformity-gaps.md` item 2 for
    /// `ButlerError::Check`: each element of `errors`, not just the outer
    /// `ButlerError`, carries `kind` (its own internal tag, unaffected)
    /// and `message` (new), the same shape
    /// `willikins_server::ValidateResponse.errors` already carries (item
    /// 5) -- so an agent reading either surface's `errors` array sees the
    /// identical per-element shape.
    #[test]
    fn check_errors_each_carry_kind_and_message() {
        let error = ButlerError::Check {
            errors: vec![CheckError::Cycle {
                nodes: vec![node_name()],
            }],
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "Check");
        let element = &json["errors"][0];
        assert_eq!(element["kind"], "Cycle");
        assert!(
            element["message"].as_str().is_some_and(|m| !m.is_empty()),
            "{json}"
        );
    }

    /// The same for `ButlerError::Input`'s `errors` (not `missing`, which
    /// task 11's own instructions leave as `describe` produces it --
    /// see the field's own doc). `InputError` is a plain struct, not a
    /// `#[serde(tag = "kind")]` enum, so wrapping it through `Reported`
    /// adds `message` but no `kind` -- there is no per-variant tag to add
    /// one from. The outer `ButlerError::Input` object still carries its
    /// own `kind` (`"Input"`), same as every other variant.
    #[test]
    fn input_errors_each_carry_message_and_the_outer_object_still_carries_kind() {
        let error = ButlerError::Input {
            errors: vec![InputError {
                input: willikins_core::InputName::parse("x").unwrap(),
                error: ParseError::new("Value", "boom"),
            }],
            missing: Vec::new(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "Input");
        let element = &json["errors"][0];
        assert_eq!(element["input"], "x");
        assert_eq!(element["message"], "input `x`: Value: boom");
        assert!(element.get("kind").is_none(), "{json}");
    }
}
