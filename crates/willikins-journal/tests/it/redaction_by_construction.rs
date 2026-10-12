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
//!
//! Milestone 3m task R7's own half of acceptance 9 ("seeds a secret into
//! a credential port of a teardown's `Created`, and into a rider
//! `secret.set`'s value port, and finds no byte of either in any line")
//! is split here: the `Created`/`Rider`-through-`PlanError::NameTaken`
//! chain is covered below, through `TeardownFinished`'s own
//! `Redacted<TeardownApplyError>` -- the one place this crate's own
//! teardown event types can carry a secret at all, since neither
//! `willikins_core::Teardown` nor `StepFingerprint` has any `Value`-bearing
//! field by construction (see `TeardownPlanned`'s own sample below). A
//! rider's own secret port never reaches *any* journal event: `Created`
//! and `Rider` are `willikins-core`-internal types an inverse's own
//! `standing`/`delete` sees in-process, at teardown apply time, and never
//! serialized anywhere -- already proven in `rollback.rs`'s own test
//! suite (`Created.riders`' secret ports are `Value::Unknown` by
//! construction, decision 3), not something this crate's own types could
//! even be asked to redact.

use indexmap::IndexMap;

use crate::common;
use crate::common::{node, port, workflow_name};
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
#[allow(clippy::too_many_lines)] // one flat list of samples; splitting it would scatter the shared setup above
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
        blocked: Vec::new(),
        replacing: Vec::new(),
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
            blocked: Vec::new(),
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
            principal: None,
            used: std::collections::BTreeMap::new(),
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
        Event::RunFinished {
            run_id,
            outcome: willikins_journal::Outcome::Blocked {
                outputs: Redacted::from(&resolved_outputs),
                blocked: vec![willikins_core::BlockedGate {
                    node: node("token"),
                    instance: None,
                    tool: willikins_core::ToolName::parse("doppler.service_token.ensure").unwrap(),
                    need: "the operator makes the test condition true".to_string(),
                    how: "do the manual thing".to_string(),
                    subject: Vec::new(),
                    holds_back: Vec::new(),
                    awaiting_inputs: Vec::new(),
                }],
            },
        },
        // The one real risk in this event's own shape: `TeardownFinished`'s
        // `Failed` outcome carries a `Redacted<TeardownApplyError>`, and
        // `TeardownApplyError::Replan` nests a `TeardownError`, which can
        // itself nest a `PlanError` -- `PlanError::NameTaken`'s own `key:
        // Inputs` is the one place a secret value could ride along this
        // chain (a key port's value need not be non-secret). Mirrors
        // `redacted.rs`'s own
        // `teardown_apply_error_redacts_a_secret_in_a_chained_plan_error`,
        // here through the real `Event`/`Entry` wire path this file checks
        // every other sample through.
        Event::TeardownFinished {
            run_id,
            outcome: willikins_journal::TeardownOutcome::Failed {
                error: Redacted::from(&willikins_core::TeardownApplyError::from(
                    willikins_core::TeardownError::from(willikins_core::PlanError::NameTaken {
                        node: node("token"),
                        tool: willikins_core::ToolName::parse("doppler.service_token.ensure")
                            .unwrap(),
                        key: inputs_with_secret.clone(),
                    }),
                )),
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

/// `TeardownPlanned`'s own half of acceptance 9, held apart from
/// [`events_carrying_the_secret`]'s loop above (milestone 3m task R7):
/// that loop also asserts every sample shows a `"REDACTED"` marker, so it
/// isn't vacuously passing, but neither `willikins_core::Teardown` nor
/// `StepFingerprint` has any `Value`-bearing field to seed one into -- see
/// this file's own module doc. This test instead proves the positive: a
/// real `Teardown`/`Vec<StepFingerprint>` serializes at all, with no
/// marker expected either way.
#[test]
fn teardown_planned_serializes_with_no_value_bearing_field_to_leak_through() {
    let entry = willikins_journal::Entry {
        seq: 1,
        at: willikins_journal::Timestamp::now(),
        event: Event::TeardownPlanned {
            plan_id: PlanId::new(),
            of_run: RunId::new(),
            workflow: workflow_name("wf"),
            document_sha256: common::document_sha256("sha"),
            teardown: Redacted::from(&willikins_core::Teardown { steps: Vec::new() }),
            fingerprint: Vec::new(),
            class: Class::Destructive,
            requires_approval: true,
            principal: willikins_journal::PrincipalId::parse("agent-0123456789ab").unwrap(),
        },
    };
    let line = serde_json::to_string(&entry).expect("Entry must serialize");
    assert!(!line.contains(&marker()), "{line}");
}
