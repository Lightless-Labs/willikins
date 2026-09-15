//! The frozen pre-pass-2 journal fixture: one file, written at the HEAD
//! that preceded adversarial pass 2's wire-format changes, carrying at
//! least one line of every [`Event`] kind and every payload enum's every
//! variant.
//!
//! **`tests/fixtures/pre-pass-2-every-event.jsonl` is frozen.** It is the
//! evidence for the additive rule the pass works under: every journal
//! written before the change replays unchanged afterwards. The tests
//! below only ever *read* it, and they must keep passing after every
//! wire-format change in this pass and in every later one. Regenerating
//! it would destroy exactly the property it exists to prove, which is why
//! [`regenerate_the_frozen_fixture`] is `#[ignore]`d, writes to a path a
//! caller names, and is documented never to be pointed at the committed
//! file again.
//!
//! What "additive" means here, precisely: a new [`Event`] variant, a new
//! variant of a payload enum, or a new field that is `Option` or
//! `#[serde(default)]`. It does **not** work in the other direction --
//! `Event` carries `#[serde(deny_unknown_fields)]`, so a journal written
//! by a *newer* binary, carrying a field or a variant an older one does
//! not know, is a replay error for that older binary. Rolling a
//! deployment back past a wire-format change therefore needs a fresh
//! journal file, not the one the newer image wrote. Recorded as a
//! decision in `docs/research/2026-09-15-e2e-http-adversarial-pass-2.md`.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use willikins_journal::{ApplyRefusedReason, Event, Journal, Outcome, Transport};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("pre-pass-2-every-event.jsonl")
}

/// Copy the frozen fixture into a fresh temporary directory, so
/// `FileJournal::open` (which takes an exclusive lock and may append)
/// never touches the committed file itself.
fn copy_to_temp() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("journal.jsonl");
    std::fs::copy(fixture_path(), &path).expect("the frozen fixture must be readable");
    (dir, path)
}

#[test]
fn the_frozen_fixture_replays_through_file_journal_open() {
    let (_dir, path) = copy_to_temp();
    let journal = willikins_journal::FileJournal::open(&path)
        .expect("the frozen pre-pass-2 journal must still open unchanged");
    assert_eq!(
        journal.entries().len(),
        FROZEN_ENTRY_COUNT,
        "the frozen fixture must not be edited"
    );
}

#[test]
fn the_frozen_fixture_replays_through_the_lock_free_reader() {
    let replayed = willikins_journal::replay(fixture_path())
        .expect("the frozen pre-pass-2 journal must still replay unchanged");
    assert_eq!(replayed.entries().len(), FROZEN_ENTRY_COUNT);
}

/// How many lines the frozen fixture holds. Named rather than inlined so
/// a diff that touches the file fails loudly here first.
const FROZEN_ENTRY_COUNT: usize = 30;

#[test]
fn every_event_kind_in_the_frozen_fixture_still_deserializes() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    for entry in replayed.entries() {
        let kind = match &entry.event {
            Event::ServerStarted { .. } => "server_started",
            Event::ToolCalled { .. } => "tool_called",
            Event::PlanRecorded { .. } => "plan_recorded",
            Event::ApprovalAutomatic { .. } => "approval_automatic",
            Event::ApprovalGranted { .. } => "approval_granted",
            Event::ApprovalRejected { .. } => "approval_rejected",
            Event::ApplyRefused { .. } => "apply_refused",
            Event::RunStarted { .. } => "run_started",
            Event::NodeStarted { .. } => "node_started",
            Event::NodeFinished { .. } => "node_finished",
            Event::RunFinished { .. } => "run_finished",
            Event::AuthFailed { .. } => "auth_failed",
        };
        *kinds.entry(kind).or_default() += 1;
    }
    for expected in [
        "server_started",
        "tool_called",
        "plan_recorded",
        "approval_automatic",
        "approval_granted",
        "approval_rejected",
        "apply_refused",
        "run_started",
        "node_started",
        "node_finished",
        "run_finished",
        "auth_failed",
    ] {
        assert!(
            kinds.contains_key(expected),
            "the frozen fixture must exercise `{expected}`, got {kinds:?}"
        );
    }
}

#[test]
fn every_apply_refused_reason_in_the_frozen_fixture_still_deserializes() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let mut seen: Vec<&'static str> = Vec::new();
    for entry in replayed.entries() {
        if let Event::ApplyRefused { reason, .. } = &entry.event {
            seen.push(match reason {
                ApplyRefusedReason::UnknownPlan => "unknown_plan",
                ApplyRefusedReason::DocumentChanged => "document_changed",
                ApplyRefusedReason::Drift { kind, .. } => match kind {
                    willikins_journal::DriftReasonKind::Instance => "drift_instance",
                    willikins_journal::DriftReasonKind::Action => "drift_action",
                    willikins_journal::DriftReasonKind::Output { .. } => "drift_output",
                },
                ApplyRefusedReason::PlanFailed { .. } => "plan_failed",
                // Added by adversarial pass 2; never present in a frozen
                // pre-pass-2 file, but named so this match stays
                // exhaustive and a future variant fails to compile here.
                ApplyRefusedReason::RecordedInputUnreadable { .. } => "recorded_input_unreadable",
                ApplyRefusedReason::ApplyPreparing => "apply_preparing",
                ApplyRefusedReason::PlanExpired => "plan_expired",
                ApplyRefusedReason::ApprovalRequired => "approval_required",
                ApplyRefusedReason::AlreadyApplied => "already_applied",
                ApplyRefusedReason::RunInProgress { .. } => "run_in_progress",
            });
        }
    }
    for expected in [
        "unknown_plan",
        "document_changed",
        "drift_instance",
        "drift_action",
        "drift_output",
        "plan_failed",
        "plan_expired",
        "approval_required",
        "already_applied",
        "run_in_progress",
    ] {
        assert!(
            seen.contains(&expected),
            "the frozen fixture must exercise `{expected}`, got {seen:?}"
        );
    }
}

#[test]
fn every_pre_pass_2_auth_failed_reason_still_deserializes() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let mut seen: Vec<(Transport, String)> = Vec::new();
    for entry in replayed.entries() {
        if let Event::AuthFailed { transport, reason } = &entry.event {
            // Read the reason's own serde tag rather than matching the
            // enum: this pass (and every later one) adds variants, and a
            // frozen fixture's assertions must not have to grow an arm
            // each time.
            let label = serde_json::to_value(reason)
                .expect("an AuthFailedReason serializes")
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .expect("every AuthFailedReason is internally tagged")
                .to_string();
            seen.push((*transport, label));
        }
    }
    for expected in [
        (Transport::Http, "missing_credential"),
        (Transport::Http, "invalid_credential"),
        (Transport::Http, "wrong_role"),
        (Transport::Stdio, "invalid_credential"),
    ] {
        let expected = (expected.0, expected.1.to_string());
        assert!(
            seen.contains(&expected),
            "the frozen fixture must exercise {expected:?}, got {seen:?}"
        );
    }
}

#[test]
fn the_frozen_fixture_folds_into_the_plans_and_runs_it_recorded() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let runs = replayed.runs();
    assert_eq!(runs.len(), 2, "two runs: one succeeded, one failed");
    let outcomes: Vec<_> = runs.iter().map(|run| format!("{:?}", run.state)).collect();
    assert!(
        outcomes.iter().any(|state| state.contains("Succeeded")),
        "one run succeeded: {outcomes:?}"
    );
    assert!(
        outcomes.iter().any(|state| state.contains("Failed")),
        "one run failed: {outcomes:?}"
    );
    // Plan C was rejected and plan D is still pending: `pending_approvals`
    // reports exactly the latter.
    assert_eq!(replayed.pending_approvals().len(), 1);
}

/// Every `plan_recorded` line in the frozen fixture predates
/// `principal`, so it folds to `requested_by: None` -- which is exactly
/// what the approvals page renders as "unknown". A missing optional
/// field reads as absent, not as a replay error.
#[test]
fn a_frozen_plan_recorded_line_folds_to_an_unknown_requester() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let pending = replayed.pending_approvals();
    assert!(!pending.is_empty());
    for record in &pending {
        assert!(
            record.requested_by.is_none(),
            "a pre-pass-2 line names no requester"
        );
    }
    for entry in replayed.entries() {
        if let Event::PlanRecorded { principal, .. } = &entry.event {
            assert!(principal.is_none());
        }
    }
    // And the field really is absent from the bytes, not written as null:
    // a pre-change reader (which refuses an unknown field) still sees the
    // exact lines it wrote.
    let text = std::fs::read_to_string(fixture_path()).unwrap();
    assert!(
        !text.contains("\"principal\":null"),
        "the frozen fixture must not have been rewritten"
    );
}

#[test]
fn a_node_finished_in_the_frozen_fixture_still_carries_its_tool_error() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let failed = replayed
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::NodeFinished {
                error: Some(error), ..
            } => Some(error.clone()),
            _ => None,
        })
        .expect("the frozen fixture must carry a failed node");
    assert!(
        !failed.message.is_empty(),
        "a recorded ToolError keeps its message"
    );
}

#[test]
fn the_frozen_fixture_still_carries_both_run_outcomes() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let mut succeeded = 0;
    let mut failed = 0;
    for entry in replayed.entries() {
        if let Event::RunFinished { outcome, .. } = &entry.event {
            match outcome {
                Outcome::Succeeded { .. } => succeeded += 1,
                Outcome::Failed { .. } => failed += 1,
            }
        }
    }
    assert_eq!((succeeded, failed), (1, 1));
}

// ---------------------------------------------------------------------
// The generator. `#[ignore]`d, and never to be pointed at the committed
// fixture again -- see this file's own module doc.
// ---------------------------------------------------------------------

/// Write a journal exercising every event kind to
/// `$WILLIKINS_FIXTURE_OUT`, for the one-shot generation of the frozen
/// fixture at the pre-pass-2 HEAD. Kept in the tree as the fixture's
/// provenance, not as a way to refresh it: regenerating the committed
/// file would erase the only evidence that a pre-change journal still
/// replays.
#[test]
#[ignore = "one-shot fixture generation; the committed fixture is frozen"]
#[allow(clippy::too_many_lines)] // one literal list of every event kind; splitting it hides what it covers
fn regenerate_the_frozen_fixture() {
    use common::{document_sha256, node, port, principal, reason, tool_name, workflow_name};
    use indexmap::IndexMap;
    use willikins_core::{Class, InstanceFingerprint, NodeStatus, ToolError, ToolErrorKind};
    use willikins_journal::{
        AuthFailedReason, Clock, DriftReasonKind, ManualClock, PlanId, Redacted, RunId, Timestamp,
    };

    let out = std::env::var("WILLIKINS_FIXTURE_OUT")
        .expect("set WILLIKINS_FIXTURE_OUT to the path to write");
    let _ = std::fs::remove_file(&out);

    let clock = std::sync::Arc::new(ManualClock::new(
        Timestamp::parse("2026-09-15T00:00:00+00:00").unwrap(),
    ));
    let clock_for_journal: std::sync::Arc<dyn Clock> = clock.clone();
    let mut journal = willikins_journal::FileJournal::open_with_clock(&out, clock_for_journal)
        .expect("a fresh journal file");

    let empty_plan = |name: &str| willikins_core::Plan {
        workflow: workflow_name(name),
        nodes: Vec::new(),
        outputs: IndexMap::new(),
        class: Class::Reversible,
        requires_approval: false,
    };

    let plan_a = PlanId::new();
    let plan_b = PlanId::new();
    let plan_c = PlanId::new();
    let plan_d = PlanId::new();
    let run_one = RunId::new();
    let run_two = RunId::new();
    let other_run = RunId::new();

    let recorded = |plan_id: PlanId, name: &str, requires_approval: bool| Event::PlanRecorded {
        plan_id,
        workflow: workflow_name(name),
        document_sha256: document_sha256(name),
        inputs: Redacted::from(&IndexMap::new()),
        plan: Redacted::from(&empty_plan(name)),
        fingerprint: Vec::<InstanceFingerprint>::new(),
        class: Class::Reversible,
        requires_approval,
        principal: None,
    };

    let tool_error = ToolError {
        kind: ToolErrorKind::Provider,
        message: "provider says: the repository already exists".to_string(),
    };

    let events = vec![
        Event::ServerStarted {
            version: "0.1.0".to_string(),
            workflows_dir: "/app/workflows".to_string(),
            workflow_hashes: BTreeMap::from([(
                "new-rust-service.yaml".to_string(),
                document_sha256("new-rust-service").to_string(),
            )]),
        },
        Event::ToolCalled {
            principal: principal("agent-0123456789ab"),
            tool: tool_name("plan"),
            workflow: Some(workflow_name("new-rust-service")),
            ok: true,
        },
        recorded(plan_a, "new-rust-service", false),
        Event::ApprovalAutomatic {
            plan_id: plan_a,
            class: Class::Reversible,
        },
        recorded(plan_b, "rotate-token", true),
        Event::ApprovalGranted {
            plan_id: plan_b,
            approver: principal("operator"),
        },
        recorded(plan_c, "rotate-token-b", true),
        Event::ApprovalRejected {
            plan_id: plan_c,
            approver: principal("operator"),
            reason: reason("not this week"),
        },
        recorded(plan_d, "rotate-token-c", true),
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::UnknownPlan,
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::DocumentChanged,
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::Drift {
                node: node("repo"),
                instance: None,
                kind: DriftReasonKind::Instance,
            },
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::Drift {
                node: node("repo"),
                instance: Some("dev".to_string()),
                kind: DriftReasonKind::Action,
            },
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::Drift {
                node: node("repo"),
                instance: None,
                kind: DriftReasonKind::Output { port: port("url") },
            },
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::PlanFailed {
                error_kind: "MissingInput".to_string(),
            },
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::PlanExpired,
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::ApprovalRequired,
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::AlreadyApplied,
        },
        Event::ApplyRefused {
            plan_id: plan_c,
            principal: principal("agent-0123456789ab"),
            reason: ApplyRefusedReason::RunInProgress { run_id: other_run },
        },
        Event::RunStarted {
            run_id: run_one,
            plan_id: plan_a,
            principal: principal("agent-0123456789ab"),
        },
        Event::NodeStarted {
            run_id: run_one,
            node: node("repo"),
            instance: None,
            inputs: Redacted::from(&willikins_core::Inputs::new()),
        },
        Event::NodeFinished {
            run_id: run_one,
            node: node("repo"),
            instance: None,
            status: NodeStatus::Created,
            outputs: Redacted::from(&willikins_core::Outputs::new()),
            error: None,
        },
        Event::RunFinished {
            run_id: run_one,
            outcome: Outcome::Succeeded {
                outputs: Redacted::from(&IndexMap::new()),
            },
        },
        Event::RunStarted {
            run_id: run_two,
            plan_id: plan_b,
            principal: principal("agent-0123456789ab"),
        },
        Event::NodeFinished {
            run_id: run_two,
            node: node("secret"),
            instance: Some("prd".to_string()),
            status: NodeStatus::Failed {
                error: tool_error.clone(),
            },
            outputs: Redacted::from(&willikins_core::Outputs::new()),
            error: Some(tool_error.clone()),
        },
        Event::RunFinished {
            run_id: run_two,
            outcome: Outcome::Failed {
                error: Redacted::from(&willikins_core::ApplyError::Tool {
                    node: node("secret"),
                    instance: Some("prd".to_string()),
                    error: tool_error,
                    applied: Box::new(willikins_core::Applied {
                        nodes: Vec::new(),
                        outputs: IndexMap::new(),
                    }),
                }),
            },
        },
        Event::AuthFailed {
            transport: Transport::Http,
            reason: AuthFailedReason::MissingCredential,
        },
        Event::AuthFailed {
            transport: Transport::Http,
            reason: AuthFailedReason::InvalidCredential,
        },
        Event::AuthFailed {
            transport: Transport::Http,
            reason: AuthFailedReason::WrongRole,
        },
        Event::AuthFailed {
            transport: Transport::Stdio,
            reason: AuthFailedReason::InvalidCredential,
        },
    ];

    for event in events {
        clock.advance(std::time::Duration::from_secs(1));
        journal.append(event).expect("every event appends");
    }
}
