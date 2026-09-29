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
        blocked: Vec::new(),
        replacing: Vec::new(),
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
            document_sha256: common::document_sha256("deadbeef"),
            inputs: Redacted::from(&indexmap::IndexMap::new()),
            plan: Redacted::from(&empty_plan()),
            fingerprint: Vec::<InstanceFingerprint>::new(),
            class: Class::Reversible,
            requires_approval: false,
            principal: Some(principal("agent-0123456789ab")),
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

/// `Outcome::Blocked` (task G2, decision (j)) is additive: a new variant
/// alongside `Succeeded`/`Failed`, not exercised by [`event_samples`]'s own
/// one-per-`Event`-variant list (which only needs one `RunFinished`
/// sample), but its own wire shape and round trip still need pinning.
#[test]
fn a_run_finished_event_with_a_blocked_outcome_serializes_with_its_kind_and_round_trips() {
    let event = Event::RunFinished {
        run_id: RunId::new(),
        outcome: Outcome::Blocked {
            outputs: Redacted::from(&indexmap::IndexMap::new()),
            blocked: vec![willikins_core::BlockedGate {
                node: node("app_group"),
                instance: None,
                tool: tool_name("test.gate"),
                need: "APP_GROUPS enabled on this bundle identifier".to_string(),
                how: "register the group in the portal".to_string(),
                subject: vec![(port("identifier"), "com.example.app".to_string())],
                holds_back: vec![node("profile")],
                awaiting_inputs: Vec::new(),
            }],
        },
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["outcome"]["kind"], "blocked");
    let round_tripped: Event = serde_json::from_str(&serde_json::to_string(&event).unwrap())
        .expect("Outcome::Blocked must round-trip");
    let back_json = serde_json::to_value(&round_tripped).unwrap();
    assert_eq!(json, back_json, "round trip changed the wire shape");
}

/// `BlockedGate.awaiting_inputs` (G3) is additive over a `BlockedGate` that
/// already existed and already derived `Deserialize` (G2): a
/// `RunFinished { Blocked }` line written by a binary between G2 and G3
/// carries a `blocked` entry with no `awaiting_inputs` key at all. 2026-09-29
/// addendum, milestone 3e's finding 1 last sentence: without a default this
/// failed to deserialize outright; now it reads as empty, the same
/// direction `Plan.blocked` itself is already forgiving in.
#[test]
fn a_run_finished_blocked_line_without_awaiting_inputs_still_deserializes() {
    let pre_g3 = concat!(
        r#"{"kind":"run_finished","run_id":"018f0000-0000-7000-8000-000000000000","#,
        r#""outcome":{"kind":"blocked","outputs":{},"blocked":[{"#,
        r#""node":"app_group","instance":null,"tool":"test.gate","#,
        r#""need":"APP_GROUPS enabled on this bundle identifier","#,
        r#""how":"register the group in the portal","#,
        r#""subject":[["identifier","com.example.app"]],"#,
        r#""holds_back":["profile"]}]}}"#,
    );
    let event: Event = serde_json::from_str(pre_g3)
        .expect("a RunFinished { Blocked } line written before G3 must still replay");
    let Event::RunFinished {
        outcome: Outcome::Blocked { blocked, .. },
        ..
    } = &event
    else {
        panic!("expected RunFinished with Outcome::Blocked");
    };
    assert_eq!(blocked.len(), 1);
    assert!(
        blocked[0].awaiting_inputs.is_empty(),
        "a line with no `awaiting_inputs` key must read as empty, not fail"
    );
    // Going forward the field is still always written, never omitted:
    // this binary's own wire shape does not change.
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(
        json["outcome"]["blocked"][0]["awaiting_inputs"],
        serde_json::json!([])
    );
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

/// `PlanRecorded.principal` (adversarial pass 2) is additive in both
/// directions of a *read*: a line written before it existed carries no
/// such field and folds to `None`, and a line that carries it round-trips
/// unchanged. Absent is written as absent, never as `null`, so the wire
/// shape of a plan with no requester is byte-identical to what the
/// pre-change binary wrote.
#[test]
fn a_plan_recorded_line_without_a_principal_still_deserializes() {
    let pre_change = concat!(
        r#"{"kind":"plan_recorded","plan_id":"018f0000-0000-7000-8000-000000000000","#,
        r#""workflow":"wf","#,
        r#""document_sha256":"da493aba2ef6940ca1291c898f67e943af4d757017f62dc2eade863bda5713aa","#,
        r#""inputs":{},"#,
        r#""plan":{"workflow":"wf","nodes":[],"outputs":{},"class":"reversible","#,
        r#""requires_approval":false},"#,
        r#""fingerprint":[],"class":"reversible","requires_approval":false}"#,
    );
    let event: Event = serde_json::from_str(pre_change).expect("a pre-pass-2 line still replays");
    let Event::PlanRecorded { principal, .. } = &event else {
        panic!("expected PlanRecorded");
    };
    assert!(principal.is_none());
    let json = serde_json::to_value(&event).unwrap();
    assert!(
        json.get("principal").is_none(),
        "an absent principal is omitted, not written as null: {json}"
    );
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
