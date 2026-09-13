//! One JSON-shape test per [`Event`] variant, exhaustive the same way
//! `willikins-core`'s `tests/apply_error_serde.rs` and
//! `tests/plan_error_serde.rs` are: a `variant_kinds!` macro generates a
//! wildcard-free match from a variant to its expected `kind` tag *and* the
//! variant count, so a new variant added to [`Event`] without a matching
//! entry here fails to compile rather than silently going unchecked.
//!
//! `Event` tags `snake_case` (`#[serde(tag = "kind", rename_all =
//! "snake_case")]`), unlike `willikins-core`'s untagged-by-convention
//! `PascalCase` error enums, so this copy of the macro takes an explicit
//! `variant => "tag"` pair per arm instead of deriving the tag from
//! `stringify!`.

mod common;

use std::collections::{BTreeMap, HashSet};

use common::{node, port, principal, reason, tool_name, workflow_name};
use willikins_core::{Class, InstanceFingerprint, NodeStatus, ToolError, ToolErrorKind};
use willikins_journal::{
    ApplyRefusedReason, AuthFailedReason, DriftReasonKind, Event, Outcome, PlanId, Redacted, RunId,
    Transport,
};

macro_rules! variant_kinds {
    ($fn_name:ident, $count:ident, $enum:ident, $($variant:ident => $tag:literal),+ $(,)?) => {
        fn $fn_name(value: &$enum) -> &'static str {
            match value {
                $($enum::$variant { .. } => $tag,)+
            }
        }

        const $count: usize = [$($tag),+].len();
    };
}

variant_kinds!(
    event_kind_of,
    EVENT_VARIANT_COUNT,
    Event,
    ServerStarted => "server_started",
    ToolCalled => "tool_called",
    PlanRecorded => "plan_recorded",
    ApprovalAutomatic => "approval_automatic",
    ApprovalGranted => "approval_granted",
    ApprovalRejected => "approval_rejected",
    ApplyRefused => "apply_refused",
    RunStarted => "run_started",
    NodeStarted => "node_started",
    NodeFinished => "node_finished",
    RunFinished => "run_finished",
    AuthFailed => "auth_failed",
);

fn empty_plan() -> willikins_core::Plan {
    willikins_core::Plan {
        workflow: workflow_name("wf"),
        nodes: Vec::new(),
        outputs: indexmap::IndexMap::new(),
        class: Class::Reversible,
        requires_approval: false,
    }
}

/// One instance of every [`Event`] variant.
fn event_samples() -> Vec<Event> {
    let plan_id = PlanId::new();
    let run_id = RunId::new();
    vec![
        Event::ServerStarted {
            version: "0.1.0".to_string(),
            workflows_dir: "/workflows".to_string(),
            workflow_hashes: BTreeMap::from([(
                "new-rust-service.yaml".to_string(),
                "abc".to_string(),
            )]),
        },
        Event::ToolCalled {
            principal: principal("agent"),
            tool: tool_name("plan"),
            workflow: Some(workflow_name("wf")),
            ok: true,
        },
        Event::PlanRecorded {
            plan_id,
            workflow: workflow_name("wf"),
            document_sha256: "deadbeef".to_string(),
            inputs: Redacted::from(&indexmap::IndexMap::new()),
            plan: Redacted::from(&empty_plan()),
            fingerprint: Vec::<InstanceFingerprint>::new(),
            class: Class::Reversible,
            requires_approval: false,
        },
        Event::ApprovalAutomatic {
            plan_id,
            class: Class::Reversible,
        },
        Event::ApprovalGranted {
            plan_id,
            approver: principal("approver"),
        },
        Event::ApprovalRejected {
            plan_id,
            approver: principal("approver"),
            reason: reason("not today"),
        },
        Event::ApplyRefused {
            plan_id,
            principal: principal("agent"),
            reason: ApplyRefusedReason::Drift {
                node: node("repo"),
                instance: None,
                kind: DriftReasonKind::Output { port: port("url") },
            },
        },
        Event::RunStarted {
            run_id,
            plan_id,
            principal: principal("agent"),
        },
        Event::NodeStarted {
            run_id,
            node: node("repo"),
            instance: None,
            inputs: Redacted::from(&willikins_core::Inputs::new()),
        },
        Event::NodeFinished {
            run_id,
            node: node("repo"),
            instance: None,
            status: NodeStatus::Created,
            outputs: Redacted::from(&willikins_core::Outputs::new()),
            error: None,
        },
        Event::RunFinished {
            run_id,
            outcome: Outcome::Succeeded {
                outputs: Redacted::from(&indexmap::IndexMap::new()),
            },
        },
        Event::AuthFailed {
            transport: Transport::Http,
            reason: AuthFailedReason::MissingCredential,
        },
    ]
}

#[test]
fn every_event_variant_serializes_with_its_kind() {
    let samples = event_samples();
    assert_eq!(
        samples.len(),
        EVENT_VARIANT_COUNT,
        "event_samples must carry exactly one sample per Event variant"
    );
    let mut seen: HashSet<&'static str> = HashSet::new();
    for sample in &samples {
        let kind = event_kind_of(sample);
        assert!(seen.insert(kind), "duplicate sample for Event::{kind}");
        let json = serde_json::to_value(sample).expect("Event must serialize");
        assert_eq!(json["kind"], kind, "sample: {sample:?}");
    }
    assert_eq!(seen.len(), EVENT_VARIANT_COUNT);
}

#[test]
fn every_event_variant_round_trips_through_json() {
    for sample in event_samples() {
        let json = serde_json::to_string(&sample).unwrap();
        let back: Event = serde_json::from_str(&json)
            .unwrap_or_else(|err| panic!("failed to round-trip {sample:?}: {err}\njson: {json}"));
        let back_json = serde_json::to_string(&back).unwrap();
        assert_eq!(json, back_json, "round trip changed the wire shape");
    }
}

#[test]
fn a_node_finished_event_carries_a_top_level_error_mirroring_a_failed_status() {
    let error = ToolError {
        kind: ToolErrorKind::Provider,
        message: "boom".to_string(),
    };
    let event = Event::NodeFinished {
        run_id: RunId::new(),
        node: node("repo"),
        instance: None,
        status: NodeStatus::Failed {
            error: error.clone(),
        },
        outputs: Redacted::from(&willikins_core::Outputs::new()),
        error: Some(error),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["status"]["kind"], "failed");
    assert_eq!(json["error"]["kind"], "Provider");
    assert_eq!(json["error"]["message"], "boom");
}

#[test]
fn entry_wraps_seq_at_and_the_event() {
    let entry = willikins_journal::Entry {
        seq: 1,
        at: willikins_journal::Timestamp::parse("2026-09-13T00:00:00+00:00").unwrap(),
        event: Event::ServerStarted {
            version: "0.1.0".to_string(),
            workflows_dir: "/workflows".to_string(),
            workflow_hashes: BTreeMap::new(),
        },
    };
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["seq"], 1);
    assert_eq!(json["at"], "2026-09-13T00:00:00+00:00");
    assert_eq!(json["event"]["kind"], "server_started");
}

#[test]
fn an_unknown_top_level_field_on_entry_is_refused() {
    let text = r#"{"seq":1,"at":"2026-09-13T00:00:00+00:00","event":{"kind":"server_started","version":"0.1.0","workflows_dir":"/w","workflow_hashes":{}},"extra":true}"#;
    assert!(serde_json::from_str::<willikins_journal::Entry>(text).is_err());
}

#[test]
fn an_unknown_field_on_an_event_variant_is_refused() {
    let text = r#"{"kind":"approval_automatic","plan_id":"018f0000-0000-7000-8000-000000000000","class":"reversible","surprise":1}"#;
    assert!(serde_json::from_str::<Event>(text).is_err());
}
