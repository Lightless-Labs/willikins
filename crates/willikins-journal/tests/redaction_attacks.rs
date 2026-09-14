//! The redaction claim attacked from the places `tests/redaction_by_construction.rs`
//! does not reach: a secret planted inside an `ApplyRefused`'s drift
//! payload, a secret *list* (many elements, one marker each), a secret
//! reaching an event through `run_and_journal` rather than a hand-built
//! `Event`, and `Plan::fingerprint` -- which the journal stores typed
//! rather than behind `Redacted`, so a fingerprint that rendered a secret
//! would leak through the journal even though `Plan` itself redacts.
//!
//! One test here asserts the *opposite*: a `ToolError` message is written
//! verbatim, because "never a secret value" is a contract `willikins-core`
//! places on tool authors, not something the journal can enforce. Pinning
//! it makes the boundary visible instead of leaving a reader to assume the
//! journal scrubs text it has no way to inspect.

mod common;

use common::{node, port, principal, workflow_name};
use indexmap::IndexMap;
use willikins_core::{
    Action, Applied, ApplyError, Class, DriftKind, Inputs, InstanceRef, NodeStatus, Outputs,
    ToolError, ToolErrorKind, Value,
};
use willikins_journal::{
    ApplyRefusedReason, DriftReasonKind, Event, Journal, MemoryJournal, PlanId, Redacted, RunId,
    run_and_journal,
};
use willikins_types::DomainType;

/// A byte string that appears nowhere else in the workspace, so finding it
/// in a journal line can only mean it came from the seeded secret.
const MARKER: &str = "CANARYCANARYCANARYCANARYCANARYCANARYCANARY";

fn secret() -> willikins_types::DopplerServiceToken {
    willikins_types::DopplerServiceToken::parse(&format!("dp.st.prd.{MARKER}")).unwrap()
}

fn secret_value() -> Value {
    Value::known(secret())
}

/// Two secrets in one value: `Value::render` prints one marker per
/// element for a list, so a list is the shape most likely to leak an
/// element some per-scalar redaction path forgot.
fn secret_list_value() -> Value {
    Value::known_list(vec![secret(), secret()])
}

fn assert_line_is_clean(label: &str, line: &str) {
    assert!(
        !line.contains(MARKER),
        "{label} leaked the seeded secret: {line}"
    );
    assert!(
        line.contains("REDACTED"),
        "{label} shows no redaction marker, so it proves nothing: {line}"
    );
}

/// `Plan::fingerprint` is the one payload `Event::PlanRecorded` keeps as a
/// live core type rather than behind `Redacted`, on the grounds that it has
/// already collapsed every output to a rendered string with a fixed marker
/// for secret ports. This attacks that claim directly, with a secret
/// scalar *and* a secret list on the planned node's outputs.
#[test]
fn a_plan_fingerprint_of_secret_outputs_never_carries_the_bytes() {
    let mut outputs = Outputs::new();
    outputs.insert(port("token"), secret_value());
    outputs.insert(port("tokens"), secret_list_value());

    let plan = willikins_core::Plan {
        workflow: workflow_name("wf"),
        nodes: vec![willikins_core::PlannedNode {
            name: node("mint"),
            instance: None,
            tool: willikins_core::ToolName::parse("doppler.service_token.ensure").unwrap(),
            action: Action::Create,
            inputs: Inputs::new(),
            outputs,
        }],
        outputs: IndexMap::new(),
        class: Class::Irreversible,
        requires_approval: true,
    };

    let fingerprint = plan.fingerprint();
    let as_json = serde_json::to_string(&fingerprint).unwrap();
    assert!(
        !as_json.contains(MARKER),
        "the fingerprint leaked a secret output: {as_json}"
    );

    let mut journal = MemoryJournal::new();
    journal
        .append(Event::PlanRecorded {
            plan_id: PlanId::new(),
            workflow: workflow_name("wf"),
            document_sha256: "sha".to_string(),
            inputs: Redacted::from(&IndexMap::new()),
            plan: Redacted::from(&plan),
            fingerprint,
            class: plan.class,
            requires_approval: plan.requires_approval,
        })
        .unwrap();
    let line = serde_json::to_string(&journal.entries()[0]).unwrap();
    assert_line_is_clean("PlanRecorded", &line);
}

/// A secret list in a node's inputs and outputs, and in the resolved
/// workflow inputs and outputs: every element must be a marker, so the
/// line carries at least as many markers as there are elements and none
/// of the bytes.
#[test]
fn a_secret_list_is_redacted_element_by_element() {
    let mut inputs = Inputs::new();
    inputs.insert(port("tokens"), secret_list_value());
    let mut outputs = Outputs::new();
    outputs.insert(port("tokens"), secret_list_value());
    let mut resolved: IndexMap<willikins_core::InputName, Value> = IndexMap::new();
    resolved.insert(
        willikins_core::InputName::parse("tokens").unwrap(),
        secret_list_value(),
    );

    let run_id = RunId::new();
    let started = Event::NodeStarted {
        run_id,
        node: node("mint"),
        instance: None,
        inputs: Redacted::from(&inputs),
    };
    let finished = Event::NodeFinished {
        run_id,
        node: node("mint"),
        instance: None,
        status: NodeStatus::Created,
        outputs: Redacted::from(&outputs),
        error: None,
    };

    for (label, event) in [("NodeStarted", started), ("NodeFinished", finished)] {
        let line = serde_json::to_string(&event).unwrap();
        assert_line_is_clean(label, &line);
        assert!(
            line.matches("REDACTED").count() >= 2,
            "{label} must carry one marker per list element: {line}"
        );
    }

    let line = serde_json::to_string(&Redacted::from(&resolved)).unwrap();
    assert!(!line.contains(MARKER), "resolved inputs leaked: {line}");
    assert!(line.matches("REDACTED").count() >= 2, "{line}");
}

/// `ApplyRefused`'s drift payload is built from `willikins_core::DriftKind`,
/// whose `Output` variant *does* carry a planned and an observed
/// [`Value`]. `run_and_journal` strips it to the value-free
/// [`DriftReasonKind`]; this feeds it two secret values (a state the
/// executor's own secret-is-not-drift rule should never produce, which is
/// exactly why the stripping must not depend on that rule holding) and
/// checks the journaled line.
#[test]
fn a_drift_refusal_strips_the_drifted_values() {
    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    let (result, _run_id, journal_error) =
        run_and_journal(&mut journal, principal("agent"), plan_id, |_observer| {
            Err(ApplyError::Drift {
                node: node("mint"),
                instance: Some("dev".to_string()),
                kind: Box::new(DriftKind::Output {
                    port: port("token"),
                    planned: secret_value(),
                    observed: secret_list_value(),
                }),
            })
        });
    assert!(journal_error.is_none(), "{journal_error:?}");
    assert!(matches!(result, Err(ApplyError::Drift { .. })));

    let entries = journal.entries();
    assert_eq!(entries.len(), 1, "only ApplyRefused: {entries:?}");
    let line = serde_json::to_string(&entries[0]).unwrap();
    assert!(
        !line.contains(MARKER),
        "the drift refusal leaked a drifted secret: {line}"
    );
    match &entries[0].event {
        Event::ApplyRefused {
            reason:
                ApplyRefusedReason::Drift {
                    node: drifted,
                    instance,
                    kind,
                },
            ..
        } => {
            assert_eq!(drifted, &node("mint"));
            assert_eq!(instance.as_deref(), Some("dev"));
            assert_eq!(
                kind,
                &DriftReasonKind::Output {
                    port: port("token")
                }
            );
        }
        other => panic!("expected an ApplyRefused Drift, got {other:?}"),
    }
}

/// The same for an `Instance` drift, whose core variant carries
/// [`InstanceRef`]s: nothing of either side survives into the journal but
/// the discriminant.
#[test]
fn an_instance_drift_refusal_keeps_only_its_discriminant() {
    let mut journal = MemoryJournal::new();
    let (_result, _run_id, journal_error) = run_and_journal(
        &mut journal,
        principal("agent"),
        PlanId::new(),
        |_observer| {
            Err(ApplyError::Drift {
                node: node("mint"),
                instance: None,
                kind: Box::new(DriftKind::Instance {
                    planned: Some(InstanceRef {
                        node: node("mint"),
                        instance: Some(MARKER.to_string()),
                    }),
                    observed: None,
                }),
            })
        },
    );
    assert!(journal_error.is_none(), "{journal_error:?}");
    let line = serde_json::to_string(&journal.entries()[0]).unwrap();
    assert!(
        !line.contains(MARKER),
        "an instance key from the drifted side reached the journal: {line}"
    );
    assert!(line.contains(r#""detail":{"kind":"instance"}"#), "{line}");
}

/// A run that fails carries `apply`'s own `ApplyError` -- partial
/// `Applied` result and all -- into `RunFinished`, through `Redacted`.
/// Drives it through `run_and_journal` rather than building the event by
/// hand, so the wiring is under test alongside the redaction.
#[test]
fn a_failed_run_journals_its_error_redacted() {
    let mut outputs = Outputs::new();
    outputs.insert(port("token"), secret_value());
    let mut resolved: IndexMap<willikins_core::OutputName, Value> = IndexMap::new();
    resolved.insert(
        willikins_core::OutputName::parse("token").unwrap(),
        secret_list_value(),
    );

    let mut journal = MemoryJournal::new();
    let (_result, run_id, journal_error) = run_and_journal(
        &mut journal,
        principal("agent"),
        PlanId::new(),
        |_observer| {
            Err(ApplyError::Tool {
                node: node("mint"),
                instance: None,
                error: ToolError {
                    kind: ToolErrorKind::Provider,
                    message: "the provider said no".to_string(),
                },
                applied: Box::new(Applied {
                    nodes: vec![willikins_core::AppliedNode {
                        name: node("mint"),
                        instance: None,
                        tool: willikins_core::ToolName::parse("doppler.service_token.ensure")
                            .unwrap(),
                        status: NodeStatus::Created,
                        outputs: outputs.clone(),
                    }],
                    outputs: resolved.clone(),
                }),
            })
        },
    );
    assert!(journal_error.is_none(), "{journal_error:?}");

    // RunStarted (backfilled, since no ApplyEvent ever reached the
    // observer) then RunFinished.
    let kinds: Vec<&str> = journal
        .entries()
        .iter()
        .map(|entry| match &entry.event {
            Event::RunStarted { .. } => "run_started",
            Event::RunFinished { .. } => "run_finished",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["run_started", "run_finished"]);

    for entry in journal.entries() {
        let line = serde_json::to_string(entry).unwrap();
        assert!(!line.contains(MARKER), "a journal line leaked: {line}");
    }
    let run = journal.run(&run_id).expect("the run must replay");
    assert_eq!(run.state, willikins_journal::RunState::Failed);
    let rendered = serde_json::to_string(&run).unwrap();
    assert!(
        !rendered.contains(MARKER),
        "the run view leaked: {rendered}"
    );
    assert!(rendered.contains("REDACTED"), "{rendered}");
}

/// The boundary, stated out loud: a `ToolError`'s `message` is a plain
/// `String` that `willikins_core` documents as never carrying a secret
/// value ("an implementation must build `message` from the tool's own
/// state and provider response, not from the `Inputs` it was given"). The
/// journal writes it verbatim and has no way not to -- it cannot tell a
/// provider's own error text from a leaked token. So this asserts the
/// *presence* of the marker: the guarantee lives in every `Tool`
/// implementation, and a tool that interpolates an input into an error
/// message is a bug in that tool, invisible here. Recorded in this task's
/// verification notes as an inherited trust boundary, not a journal
/// defect.
#[test]
fn a_tool_error_message_is_written_verbatim_because_only_the_tool_can_keep_it_clean() {
    let error = ToolError {
        kind: ToolErrorKind::Provider,
        message: format!("a badly written tool interpolated {MARKER} into its message"),
    };
    let event = Event::NodeFinished {
        run_id: RunId::new(),
        node: node("mint"),
        instance: None,
        status: NodeStatus::Failed {
            error: error.clone(),
        },
        outputs: Redacted::from(&Outputs::new()),
        error: Some(error),
    };
    let line = serde_json::to_string(&event).unwrap();
    assert!(
        line.contains(MARKER),
        "if this ever stops holding, the journal grew a scrubber and this test's \
         reasoning needs rewriting: {line}"
    );
    assert_eq!(
        line.matches(MARKER).count(),
        2,
        "once in `status`, once in `error`"
    );
}
