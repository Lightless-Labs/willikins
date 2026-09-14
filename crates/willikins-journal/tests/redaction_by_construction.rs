//! Pins the crate's central redaction claim directly, independent of any
//! fixture or `apply` run: seed one distinctive secret value, build every
//! [`Event`] shape that could possibly carry it (through
//! [`willikins_journal::Redacted`], through a plan's own fingerprint,
//! through a `NodeFinished`'s outputs), serialize each to the exact JSON
//! line [`willikins_journal::Journal::append`] would write, and grep for
//! the seeded bytes. `tests/acceptance_10_journal.rs` proves the same
//! thing again end to end through real fixtures and a real `apply` run;
//! this test isolates the claim to the journal crate's own construction
//! rules, per this crate's module docs.

mod common;

use indexmap::IndexMap;

use common::{node, port, workflow_name};
use willikins_core::{Class, Inputs, Outputs, Value};
use willikins_journal::{Event, PlanId, Redacted, RunId};
use willikins_types::DomainType;

fn marker() -> String {
    "CANARY".repeat(7)
}

fn secret_value() -> Value {
    Value::known(
        willikins_types::DopplerServiceToken::parse(&format!("dp.st.prd.{}", marker())).unwrap(),
    )
}

/// Every event shape whose payload could hold [`secret_value`]'s bytes,
/// with it planted in every place that type's own fields allow.
fn events_carrying_the_secret() -> Vec<Event> {
    let plan_id = PlanId::new();
    let run_id = RunId::new();

    let mut inputs_with_secret = Inputs::new();
    inputs_with_secret.insert(port("token"), secret_value());

    let mut outputs_with_secret = Outputs::new();
    outputs_with_secret.insert(port("token"), secret_value());

    let mut resolved_inputs: IndexMap<willikins_core::InputName, Value> = IndexMap::new();
    resolved_inputs.insert(
        willikins_core::InputName::parse("token").unwrap(),
        secret_value(),
    );

    let mut resolved_outputs: IndexMap<willikins_core::OutputName, Value> = IndexMap::new();
    resolved_outputs.insert(
        willikins_core::OutputName::parse("token").unwrap(),
        secret_value(),
    );

    let plan_with_secret = willikins_core::Plan {
        workflow: workflow_name("wf"),
        nodes: vec![willikins_core::PlannedNode {
            name: node("token"),
            instance: None,
            tool: willikins_core::ToolName::parse("doppler.service_token.ensure").unwrap(),
            action: willikins_core::Action::Create,
            inputs: Inputs::new(),
            outputs: outputs_with_secret.clone(),
        }],
        outputs: resolved_outputs.clone(),
        class: Class::Irreversible,
        requires_approval: true,
    };

    let applied_error = willikins_core::ApplyError::UnknownRequiredInput {
        node: node("token"),
        instance: None,
        port: port("token"),
        input: willikins_core::InputName::parse("token").unwrap(),
        applied: Box::new(willikins_core::Applied {
            nodes: vec![willikins_core::AppliedNode {
                name: node("token"),
                instance: None,
                tool: willikins_core::ToolName::parse("doppler.service_token.ensure").unwrap(),
                status: willikins_core::NodeStatus::Created,
                outputs: outputs_with_secret.clone(),
            }],
            outputs: resolved_outputs.clone(),
        }),
    };

    vec![
        Event::PlanRecorded {
            plan_id,
            workflow: workflow_name("wf"),
            document_sha256: common::document_sha256("sha"),
            inputs: Redacted::from(&resolved_inputs),
            plan: Redacted::from(&plan_with_secret),
            fingerprint: plan_with_secret.fingerprint(),
            class: Class::Irreversible,
            requires_approval: true,
        },
        Event::NodeStarted {
            run_id,
            node: node("token"),
            instance: None,
            inputs: Redacted::from(&inputs_with_secret),
        },
        Event::NodeFinished {
            run_id,
            node: node("token"),
            instance: None,
            status: willikins_core::NodeStatus::Created,
            outputs: Redacted::from(&outputs_with_secret),
            error: None,
        },
        Event::RunFinished {
            run_id,
            outcome: willikins_journal::Outcome::Succeeded {
                outputs: Redacted::from(&resolved_outputs),
            },
        },
        Event::RunFinished {
            run_id,
            outcome: willikins_journal::Outcome::Failed {
                error: Redacted::from(&applied_error),
            },
        },
    ]
}

#[test]
fn the_marker_type_actually_redacts_by_itself() {
    // Sanity: prove the marker really is a secret domain type before
    // trusting the rest of this test's absence checks.
    assert!(secret_value().is_secret());
    let rendered = secret_value().render().to_string();
    assert!(!rendered.contains(&marker()));
}

#[test]
fn no_event_that_could_carry_the_secret_leaks_it() {
    for event in events_carrying_the_secret() {
        let entry = willikins_journal::Entry {
            seq: 1,
            at: willikins_journal::Timestamp::now(),
            event,
        };
        let line = serde_json::to_string(&entry).expect("Entry must serialize");
        assert!(
            !line.contains(&marker()),
            "journal line leaked the seeded secret: {line}"
        );
        assert!(
            line.contains("REDACTED"),
            "journal line never shows a redaction marker, so it isn't proving anything: {line}"
        );
        // Debug must redact too: a journal that only redacted on the wire
        // but leaked through a `tracing`/`{:?}` log line elsewhere would
        // still be a leak.
        let debug = format!("{entry:?}");
        assert!(!debug.contains(&marker()), "Debug leaked: {debug}");
    }
}
