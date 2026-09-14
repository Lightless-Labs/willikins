//! Adversarial pass 1 over the journal and the approval gate (acceptance
//! test 19, first pass; task 9). Recorded in
//! `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`.
//!
//! The executor half of the same pass lives in `willikins-core`'s
//! `tests/apply_adversarial.rs`. This file attacks what a *journal* can be
//! made to say: a secret byte on a line, a hand-edited or spliced file
//! that replays as if nothing were wrong, and a plan whose recorded
//! identity does not pin the work that ran.
//!
//! Tests whose name begins `boundary_` pin something this crate
//! deliberately does *not* do, so the guarantee it depends on (the
//! server's, task 10a) is visible in code rather than only in a plan
//! document.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::describe::{PartialInputs, RawInput};
use willikins_core::{
    Approval, Checked, Class, Ensured, InputName, Inputs, Observation, Outputs, PortSpec, PortType,
    SinkToken, Tool, ToolError, ToolSpec, TypeName, TypeRef, Value, Workflow, apply, check, plan,
};
use willikins_journal::{
    Event, FileJournal, Journal, MemoryJournal, PlanId, Redacted, run_and_journal,
};
use willikins_providers_fake::FakeState;
use willikins_types::DomainType;

use common::{node, port, principal, tool_name, workflow_name};

/// The seeded service token every run in this file mints, and the bytes of
/// it that must never appear on a journal line.
fn distinctive_token() -> willikins_types::DopplerServiceToken {
    willikins_types::DopplerServiceToken::parse(&format!("dp.st.prd.{}", "PASS1M".repeat(7)))
        .unwrap()
}

/// The marker bytes of [`distinctive_token`].
const TOKEN_MARKER_BYTES: &str = "PASS1MPASS1MPASS1MPASS1MPASS1MPASS1MPASS1M";

/// The two constant tokens `fake.secret_list` always reports, as the
/// distinctive part of each.
const LIST_MARKER_BYTES: [&str; 2] = ["fakesecretlistone", "fakesecretlisttwo"];

/// The bytes seeded into `workflows/fixtures/state/plan-identity-secrets.json`.
const PLAN_IDENTITY_SECRET_BYTES: [&str; 2] = [
    "pass1-approved-secret-bytes-do-not-leak",
    "pass1-swapped-secret-bytes-do-not-leak",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join(name)
}

fn load(path: &Path) -> Workflow {
    willikins_dsl::load_document(path)
        .unwrap_or_else(|err| panic!("{}: failed to load: {err}", path.display()))
}

fn resolve_inputs(checked: &Checked, overrides: &[(&str, RawInput)]) -> IndexMap<InputName, Value> {
    let mut partial = PartialInputs::new();
    for (name, raw) in overrides {
        partial.insert(InputName::parse(name).unwrap(), raw.clone());
    }
    let description = willikins_core::describe(checked, &partial);
    assert!(description.errors.is_empty(), "{:?}", description.errors);
    assert!(description.missing.is_empty(), "{:?}", description.missing);
    description.resolved
}

/// Seed a fake state from one of `workflows/fixtures/state/`'s files.
fn state_from(name: &str) -> Arc<Mutex<FakeState>> {
    let path = workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join(name);
    let json = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{name}: {err}"));
    Arc::new(Mutex::new(FakeState::from_json(&json).unwrap()))
}

/// Append a `PlanRecorded` (and, for a plan that needs none, its
/// `ApprovalAutomatic`) for `approved`, returning the fresh id.
fn record_plan<J: Journal>(
    journal: &mut J,
    checked: &Checked,
    inputs: &IndexMap<InputName, Value>,
    approved: &willikins_core::Plan,
    document_sha256: &str,
) -> PlanId {
    let plan_id = PlanId::new();
    journal
        .append(Event::PlanRecorded {
            plan_id,
            workflow: checked.workflow.name.clone(),
            document_sha256: document_sha256.to_string(),
            inputs: Redacted::from(inputs),
            plan: Redacted::from(approved),
            fingerprint: approved.fingerprint(),
            class: approved.class,
            requires_approval: approved.requires_approval,
        })
        .expect("PlanRecorded must append");
    if approved.requires_approval {
        journal
            .append(Event::ApprovalGranted {
                plan_id,
                approver: principal("operator"),
            })
            .expect("ApprovalGranted must append");
    } else {
        journal
            .append(Event::ApprovalAutomatic {
                plan_id,
                class: approved.class,
            })
            .expect("ApprovalAutomatic must append");
    }
    plan_id
}

/// Every line of a journal file, as text.
fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .expect("the journal file is readable")
        .lines()
        .map(str::to_string)
        .collect()
}

/// Assert that `haystack` holds none of `needles`.
fn assert_clean(label: &str, haystack: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            !haystack.contains(needle),
            "{label} leaked `{needle}`: {haystack}"
        );
    }
}

// ---------------------------------------------------------------------
// 1. Plan identity: the approved plan does not pin the work that ran
// ---------------------------------------------------------------------

/// `workflows/fixtures/plan-identity-a.yaml` and `-b.yaml` plan to
/// identical fingerprints while writing two *different* secrets into the
/// same GitHub Actions secret. A plan recorded (and approved) for A
/// therefore applies to B: `willikins_core::apply` compares the approved
/// plan only with a fresh plan of the `Checked` workflow it is handed, and
/// the journal faithfully records a run of plan A while document B's
/// secret is what landed.
///
/// Closed by task 10a, not here: `Butler::apply` reloads the document by
/// the recorded workflow *name* from the trusted directory and refuses
/// with `DocumentChanged` when the sha256 differs, which is exactly the
/// pair of fields this test shows the journal already carries and nothing
/// yet compares. See
/// `todos/2026-09-14-plan-identity-must-cover-inputs.md`.
#[test]
fn boundary_a_plan_approved_for_one_document_applies_to_another_with_the_same_fingerprint() {
    let state = state_from("plan-identity-secrets.json");
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));

    let approved_document = load(&fixture("plan-identity-a.yaml"));
    let swapped_document = load(&fixture("plan-identity-b.yaml"));
    let approved_checked = check(&approved_document, &catalog).expect("document A checks");
    let swapped_checked = check(&swapped_document, &catalog).expect("document B checks");
    let no_inputs = IndexMap::new();

    let approved = plan(&approved_checked, &no_inputs, &catalog).expect("document A plans");
    let swapped = plan(&swapped_checked, &no_inputs, &catalog).expect("document B plans");
    assert_eq!(
        approved.fingerprint(),
        swapped.fingerprint(),
        "the two documents plan indistinguishably"
    );
    assert_ne!(approved.workflow, swapped.workflow);

    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let mut journal = FileJournal::open(&journal_path).expect("journal opens");
    let plan_id = record_plan(
        &mut journal,
        &approved_checked,
        &no_inputs,
        &approved,
        "sha256-of-document-a",
    );

    // The run the agent asks for: document B, under document A's approval.
    let (result, _run_id, journal_error) =
        run_and_journal(&mut journal, principal("agent"), plan_id, |observer| {
            apply(
                &swapped_checked,
                &no_inputs,
                &catalog,
                &approved,
                &Approval::Auto,
                observer,
            )
        });
    assert!(journal_error.is_none(), "{journal_error:?}");
    result.expect("core has nothing to compare the two documents with");

    assert!(
        state
            .lock()
            .unwrap()
            .github_actions_secrets
            .contains("lightless-labs/third-thoughts#DOPPLER_TOKEN"),
        "document B's secret was written"
    );

    // The journal is truthful about what it was told, which is the point:
    // the record names document A, and nothing on the wire says the run
    // executed B.
    let record = journal.plan(&plan_id).expect("the plan is recorded");
    assert_eq!(record.workflow.as_str(), "plan-identity-a");
    assert_eq!(record.document_sha256, "sha256-of-document-a");
    assert!(record.applied.is_some(), "and the run is recorded as its");

    // Neither secret's bytes are anywhere in the file.
    let text = std::fs::read_to_string(&journal_path).unwrap();
    assert_clean("the journal file", &text, &PLAN_IDENTITY_SECRET_BYTES);
}

// ---------------------------------------------------------------------
// 2. Forging a journal into an accepted replay
// ---------------------------------------------------------------------

/// Rewrite line `line_no` (1-based) of the journal at `path` by applying
/// `edit` to its parsed JSON, keeping every other line untouched.
fn tamper(path: &Path, line_no: usize, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut lines = lines(path);
    let mut value: serde_json::Value =
        serde_json::from_str(&lines[line_no - 1]).expect("the line is JSON");
    edit(&mut value);
    lines[line_no - 1] = serde_json::to_string(&value).unwrap();
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(path, text).unwrap();
}

/// A hand-edited `PlanRecorded` payload replays as truth: there is no hash
/// chain (a deliberate decision -- a keyless chain computed and checked by
/// the same binary is not tamper evidence), so an operator-level attacker
/// who can write the journal file can change what a later `apply` compares
/// against. `replay_integrity.rs` pins the general case
/// (`a_hand_edited_payload_on_an_earlier_line_is_not_detected`); this pins
/// the security-relevant one: the fields task 10a's plan identity is built
/// from, and the fingerprint its drift check would compare.
#[test]
fn boundary_a_hand_edited_plan_record_replays_as_truth() {
    let state = state_from("plan-identity-secrets.json");
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let document = load(&fixture("plan-identity-a.yaml"));
    let checked = check(&document, &catalog).expect("checks");
    let no_inputs = IndexMap::new();
    let approved = plan(&checked, &no_inputs, &catalog).expect("plans");

    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let plan_id = {
        let mut journal = FileJournal::open(&journal_path).expect("journal opens");
        record_plan(
            &mut journal,
            &checked,
            &no_inputs,
            &approved,
            "sha256-of-document-a",
        )
    };

    tamper(&journal_path, 1, |value| {
        value["event"]["document_sha256"] = serde_json::json!("sha256-of-something-else");
        value["event"]["requires_approval"] = serde_json::json!(false);
        value["event"]["class"] = serde_json::json!("reversible");
        value["event"]["fingerprint"][0]["action"] = serde_json::json!("noop");
    });

    let journal = FileJournal::open(&journal_path).expect("the edit replays cleanly");
    let record = journal.plan(&plan_id).expect("the plan is still there");
    assert_eq!(
        record.document_sha256, "sha256-of-something-else",
        "the journal has no way to know its own past was edited"
    );
    assert_eq!(record.fingerprint[0].action, willikins_core::Action::NoOp);
    assert!(!record.requires_approval);
}

/// A second `PlanRecorded` for a `plan_id` the journal already has is a
/// *rewrite through the append door*: the fold used to let it replace the
/// first record wholesale, which cleared `applied` (so an
/// `AlreadyApplied` check reads a spent plan as fresh), reset the approval
/// state to `Pending`, and swapped in whatever `document_sha256`,
/// `fingerprint` and `requires_approval` the second line carried. The
/// first record wins now, and `FileJournal::open` refuses the file
/// outright.
#[test]
fn a_second_plan_recorded_event_for_one_id_never_replaces_the_first() {
    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    let run_id = willikins_journal::RunId::new();

    journal
        .append(plan_recorded(plan_id, true, "sha256-original"))
        .unwrap();
    journal
        .append(Event::ApprovalGranted {
            plan_id,
            approver: principal("operator"),
        })
        .unwrap();
    journal
        .append(Event::RunStarted {
            run_id,
            plan_id,
            principal: principal("agent"),
        })
        .unwrap();

    // The forgery: the same id, recorded again.
    journal
        .append(plan_recorded(plan_id, false, "sha256-forged"))
        .unwrap();

    let record = journal.plan(&plan_id).expect("the plan is recorded");
    assert_eq!(
        record.document_sha256, "sha256-original",
        "the first record of a plan id is the only one"
    );
    assert!(
        record.requires_approval,
        "a duplicate must not clear the approval requirement"
    );
    assert!(
        record.applied.is_some(),
        "and must not un-apply a plan that has already run"
    );
    assert!(
        journal.pending_approvals().is_empty(),
        "nor put a spent plan back in front of an approver"
    );
}

/// The same forgery in a file is refused at replay, naming the line: a
/// duplicate `plan_id` can never be produced by `append` (every
/// `PlanId::new` is a fresh uuid v7), so a file holding one was edited.
#[test]
fn a_file_with_a_duplicate_plan_id_is_refused_at_replay() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let plan_id = PlanId::new();
    {
        let mut journal = FileJournal::open(&journal_path).unwrap();
        journal
            .append(plan_recorded(plan_id, true, "sha256-original"))
            .unwrap();
        journal
            .append(plan_recorded(plan_id, false, "sha256-forged"))
            .unwrap();
    }
    let err = FileJournal::open(&journal_path).expect_err("a duplicate plan id is corruption");
    let message = err.to_string();
    assert!(message.contains("line 2"), "{message}");
    assert!(message.contains("plan"), "{message}");
}

/// The same rewrite one event further on: a second `RunFinished` for a
/// run the journal has already seen finish. Letting it through would turn
/// a failed run into a successful one -- or empty its recorded outputs --
/// from one appended line.
#[test]
fn a_second_run_finished_event_for_one_run_never_replaces_the_first() {
    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    let run_id = willikins_journal::RunId::new();
    journal
        .append(plan_recorded(plan_id, false, "sha256-original"))
        .unwrap();
    journal
        .append(Event::RunStarted {
            run_id,
            plan_id,
            principal: principal("agent"),
        })
        .unwrap();
    journal
        .append(Event::RunFinished {
            run_id,
            outcome: willikins_journal::Outcome::Failed {
                error: Redacted::from(&willikins_core::ApplyError::ApprovalRequired {
                    class: Class::Destructive,
                }),
            },
        })
        .unwrap();

    // The forgery: the same run, finishing again, successfully.
    journal
        .append(Event::RunFinished {
            run_id,
            outcome: willikins_journal::Outcome::Succeeded {
                outputs: Redacted::from(&IndexMap::<willikins_core::OutputName, Value>::new()),
            },
        })
        .unwrap();

    let run = journal.run(&run_id).expect("the run is recorded");
    assert_eq!(
        run.state,
        willikins_journal::RunState::Failed,
        "a run finishes once: {run:?}"
    );
    assert!(
        run.error.is_some(),
        "and keeps the failure it finished with"
    );
}

/// And one event further still: a second `NodeFinished` for one instance
/// of one run. The fold used to let it replace the first, so a `Failed`
/// instance could be appended over as `Created` -- the finest-grained
/// rewrite of the four, and the one an operator reading a run record is
/// least likely to question.
#[test]
fn a_second_node_finished_event_for_one_instance_never_replaces_the_first() {
    let mut journal = MemoryJournal::new();
    // No `PlanRecorded` here on purpose: with no plan to fold against, a
    // `RunRecord`'s `nodes` are exactly the instances that finished, in
    // arrival order, which is what this attack is about.
    let plan_id = PlanId::new();
    let run_id = willikins_journal::RunId::new();
    journal
        .append(Event::RunStarted {
            run_id,
            plan_id,
            principal: principal("agent"),
        })
        .unwrap();
    let failed = willikins_core::NodeStatus::Failed {
        error: willikins_core::ToolError {
            kind: willikins_core::ToolErrorKind::Provider,
            message: "the provider said no".to_string(),
        },
    };
    journal
        .append(Event::NodeFinished {
            run_id,
            node: node("repo"),
            instance: None,
            status: failed,
            outputs: Redacted::from(&Outputs::new()),
            error: None,
        })
        .unwrap();
    journal
        .append(Event::NodeFinished {
            run_id,
            node: node("repo"),
            instance: None,
            status: willikins_core::NodeStatus::Created,
            outputs: Redacted::from(&Outputs::new()),
            error: None,
        })
        .unwrap();

    let run = journal.run(&run_id).expect("the run is recorded");
    assert_eq!(run.nodes.len(), 1, "{run:?}");
    assert!(
        matches!(
            run.nodes[0].status,
            willikins_core::NodeStatus::Failed { .. }
        ),
        "an instance finishes once: {:?}",
        run.nodes[0].status
    );
}

/// Every one of the four is refused outright in a file, naming the line:
/// `append` mints each id once and emits each of these events once per
/// id, so a file holding a second one was edited.
#[test]
fn a_file_with_any_duplicated_record_is_refused_at_replay() {
    let plan_id = PlanId::new();
    let run_id = willikins_journal::RunId::new();
    let node_finished = || Event::NodeFinished {
        run_id,
        node: node("repo"),
        instance: None,
        status: willikins_core::NodeStatus::Created,
        outputs: Redacted::from(&Outputs::new()),
        error: None,
    };
    let run_finished = || Event::RunFinished {
        run_id,
        outcome: willikins_journal::Outcome::Succeeded {
            outputs: Redacted::from(&IndexMap::<willikins_core::OutputName, Value>::new()),
        },
    };
    let run_started = || Event::RunStarted {
        run_id,
        plan_id,
        principal: principal("agent"),
    };

    for (label, events) in [
        ("run_started", vec![run_started(), run_started()]),
        (
            "node_finished",
            vec![run_started(), node_finished(), node_finished()],
        ),
        (
            "run_finished",
            vec![run_started(), run_finished(), run_finished()],
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let journal_path = dir.path().join("journal.jsonl");
        let expected_line = events.len();
        {
            let mut journal = FileJournal::open(&journal_path).unwrap();
            for event in events {
                journal.append(event).unwrap();
            }
        }
        let err = FileJournal::open(&journal_path)
            .expect_err(&format!("a duplicated {label} is corruption"));
        let message = err.to_string();
        assert!(
            message.contains(&format!("line {expected_line}")),
            "{label}: {message}"
        );
        assert!(message.contains(label), "{label}: {message}");
    }
}

/// A minimal `PlanRecorded` event for `plan_id`.
fn plan_recorded(plan_id: PlanId, requires_approval: bool, sha: &str) -> Event {
    let plan = empty_plan();
    Event::PlanRecorded {
        plan_id,
        workflow: workflow_name("forged"),
        document_sha256: sha.to_string(),
        inputs: Redacted::from(&IndexMap::<InputName, Value>::new()),
        plan: Redacted::from(&plan),
        fingerprint: plan.fingerprint(),
        class: if requires_approval {
            Class::Destructive
        } else {
            Class::Reversible
        },
        requires_approval,
    }
}

/// A plan with no nodes and no outputs.
fn empty_plan() -> willikins_core::Plan {
    willikins_core::Plan {
        workflow: workflow_name("forged"),
        nodes: Vec::new(),
        outputs: IndexMap::new(),
        class: Class::Reversible,
        requires_approval: false,
    }
}

/// A run spliced into the file with no `RunStarted` of its own -- node
/// events and a `RunFinished` for an id the journal never started -- is
/// invisible to every view rather than half-materialising a run record.
#[test]
fn a_spliced_run_with_no_run_started_is_invisible_to_the_views() {
    let mut journal = MemoryJournal::new();
    let run_id = willikins_journal::RunId::new();
    journal
        .append(Event::NodeFinished {
            run_id,
            node: node("ghost"),
            instance: None,
            status: willikins_core::NodeStatus::Created,
            outputs: Redacted::from(&Outputs::new()),
            error: None,
        })
        .unwrap();
    journal
        .append(Event::RunFinished {
            run_id,
            outcome: willikins_journal::Outcome::Succeeded {
                outputs: Redacted::from(&IndexMap::<willikins_core::OutputName, Value>::new()),
            },
        })
        .unwrap();

    assert!(journal.run(&run_id).is_none(), "no run was ever started");
    assert!(journal.runs().is_empty());
}

/// A `RunStarted` naming a `plan_id` the journal has no record of does
/// materialise a run -- a run really did start -- but with no plan to fold
/// against, its `nodes` are only what actually finished, in arrival order,
/// and no `PlanRecord` is invented for the unknown id.
#[test]
fn boundary_a_run_started_for_an_unknown_plan_replays_without_inventing_a_plan() {
    let mut journal = MemoryJournal::new();
    let run_id = willikins_journal::RunId::new();
    let plan_id = PlanId::new();
    journal
        .append(Event::RunStarted {
            run_id,
            plan_id,
            principal: principal("agent"),
        })
        .unwrap();
    journal
        .append(Event::NodeFinished {
            run_id,
            node: node("ghost"),
            instance: None,
            status: willikins_core::NodeStatus::Created,
            outputs: Redacted::from(&Outputs::new()),
            error: None,
        })
        .unwrap();

    let run = journal.run(&run_id).expect("the run is recorded");
    assert_eq!(run.plan_id, plan_id);
    assert_eq!(run.nodes.len(), 1);
    assert!(
        journal.plan(&plan_id).is_none(),
        "no plan record is invented for an id nothing recorded"
    );
}

/// An approval decision is *not* final in the fold: a later
/// `ApprovalGranted` overwrites an earlier `ApprovalRejected`, so a plan a
/// human refused reads as approved. Nothing in this crate can prevent it
/// -- an append-only log's fold has to fold everything it is given, and
/// two contradictory decisions are a fact about the file -- so the refusal
/// belongs to whoever accepts the second decision: `willikins-server`
/// (task 10a) must refuse to record an approval or rejection for a plan
/// that already has one, exactly as it refuses a second `apply`. Pinned so
/// that the day the fold is made first-decision-wins instead, it is a
/// deliberate change with a test to update rather than a silent one.
#[test]
fn boundary_a_grant_after_a_rejection_is_the_decision_the_views_report() {
    let mut journal = MemoryJournal::new();
    let plan_id = PlanId::new();
    journal
        .append(plan_recorded(plan_id, true, "sha256-original"))
        .unwrap();
    journal
        .append(Event::ApprovalRejected {
            plan_id,
            approver: principal("operator"),
            reason: willikins_journal::Reason::parse("not this one").unwrap(),
        })
        .unwrap();
    journal
        .append(Event::ApprovalGranted {
            plan_id,
            approver: principal("agent-pretending-to-be-an-approver"),
        })
        .unwrap();

    let record = journal.plan(&plan_id).expect("the plan is recorded");
    assert!(
        matches!(
            record.approval,
            willikins_journal::ApprovalState::Granted { .. }
        ),
        "the last decision is the one the views report: {:?}",
        record.approval
    );
}

// ---------------------------------------------------------------------
// 3. Secret bytes into the journal
// ---------------------------------------------------------------------

/// The rotation workflow is the one that mints a secret, hands it
/// straight to a sink, and is `Destructive` (so it runs only under
/// `Approval::Human`). Run it with the mint seeded to a distinctive
/// marker and with an injected failure at the sink, so the journal holds
/// a `NodeStarted` carrying the secret, a `NodeFinished` for the node
/// that minted it, a hostile-shaped `ToolError` message, and a
/// `RunFinished` whose `Outcome::Failed` carries the whole partial
/// `Applied`. None of it may hold the token's bytes.
#[test]
fn the_rotation_workflow_journals_a_minted_secret_and_a_tool_failure_without_leaking() {
    let config = willikins_types::DopplerConfig::parse("widgets/prd").unwrap();
    let state = Arc::new(Mutex::new(
        FakeState::new()
            .with_next_token(distinctive_token())
            .with_doppler_config(&config)
            .with_fail_ensure_once(
                "github.actions_secret.ensure",
                "lightless-labs/third-thoughts#DOPPLER_TOKEN",
            ),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let workflow = load(
        &workspace_root()
            .join("workflows")
            .join("rotate-service-token.yaml"),
    );
    let checked = check(&workflow, &catalog).expect("the rotation fixture checks");
    assert_eq!(checked.class, Class::Destructive);
    let inputs = resolve_inputs(
        &checked,
        &[
            ("project", RawInput::Scalar("widgets".to_string())),
            (
                "repo",
                RawInput::Scalar("lightless-labs/third-thoughts".to_string()),
            ),
        ],
    );
    let approved = plan(&checked, &inputs, &catalog).expect("plans");
    assert!(approved.requires_approval);

    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let mut journal = FileJournal::open(&journal_path).expect("journal opens");
    let plan_id = record_plan(&mut journal, &checked, &inputs, &approved, "sha256-rotate");

    let approval = Approval::Human {
        approver: principal("operator"),
        at: willikins_core::Timestamp::now(),
    };
    let (result, run_id, journal_error) =
        run_and_journal(&mut journal, principal("agent"), plan_id, |observer| {
            apply(&checked, &inputs, &catalog, &approved, &approval, observer)
        });
    assert!(journal_error.is_none(), "{journal_error:?}");
    let err = result.expect_err("the injected failure stops the run at the sink");
    assert!(
        format!("{err}").contains("injected failure"),
        "the tool's own message reaches the caller: {err}"
    );

    // Every line of the file, every view, and every rendering of the
    // error itself.
    let text = std::fs::read_to_string(&journal_path).unwrap();
    assert!(text.contains("REDACTED"), "the marker's stand-in is there");
    assert_clean("the journal file", &text, &[TOKEN_MARKER_BYTES]);
    for line in lines(&journal_path) {
        assert_clean("a journal line", &line, &[TOKEN_MARKER_BYTES]);
    }
    let record = journal.run(&run_id).expect("the run is recorded");
    assert_clean(
        "the run record",
        &serde_json::to_string(&record).unwrap(),
        &[TOKEN_MARKER_BYTES],
    );
    assert_clean(
        "the run record's Debug",
        &format!("{record:?}"),
        &[TOKEN_MARKER_BYTES],
    );
    let plan_record = journal.plan(&plan_id).expect("the plan is recorded");
    assert_clean(
        "the plan record",
        &serde_json::to_string(&plan_record).unwrap(),
        &[TOKEN_MARKER_BYTES],
    );
    assert_clean(
        "the error",
        &format!("{err:?} {err}"),
        &[TOKEN_MARKER_BYTES],
    );

    // And a reopen replays the same thing.
    drop(journal);
    let reopened = FileJournal::open(&journal_path).expect("replays");
    assert_clean(
        "the replayed run record",
        &serde_json::to_string(&reopened.run(&run_id).unwrap()).unwrap(),
        &[TOKEN_MARKER_BYTES],
    );
}

/// A sink whose one required port takes a *list* of service tokens: the
/// only shape in which a `NodeStarted` event can carry a secret list.
struct SecretListSink {
    spec: ToolSpec,
}

impl SecretListSink {
    fn new() -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("values"),
            PortSpec {
                ty: PortType::Exact(TypeRef::list_of(
                    TypeName::parse("DopplerServiceToken").unwrap(),
                )),
                required: true,
            },
        );
        Self {
            spec: ToolSpec {
                name: tool_name("test.secret_list_sink"),
                description: "Test tool: accepts a list of secret tokens.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
        }
    }
}

impl Tool for SecretListSink {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
    }
}

/// A `NodeStarted` whose inputs hold a secret *list* prints one redaction
/// marker per element and none of the elements' bytes -- the list case of
/// the same by-construction redaction the scalar case relies on.
#[test]
fn node_started_inputs_holding_a_secret_list_are_redacted_element_by_element() {
    let (_state, mut catalog) = willikins_providers_fake::empty();
    catalog
        .insert(Arc::new(SecretListSink::new()))
        .expect("the sink registers");

    let workflow = Workflow::new(workflow_name("secret-list-sink"))
        .node(
            node("tokens"),
            willikins_core::Node::new(tool_name("fake.secret_list")).port(
                port("config"),
                willikins_core::Binding::Literal("widgets/prd".to_string()),
            ),
        )
        .node(
            node("sink"),
            willikins_core::Node::new(tool_name("test.secret_list_sink")).port(
                port("values"),
                willikins_core::Binding::Step {
                    node: node("tokens"),
                    port: port("tokens"),
                },
            ),
        );
    let checked = check(&workflow, &catalog).expect("a secret list into a secret list port checks");
    let no_inputs = IndexMap::new();
    let approved = plan(&checked, &no_inputs, &catalog).expect("plans");

    let mut journal = MemoryJournal::new();
    let plan_id = record_plan(&mut journal, &checked, &no_inputs, &approved, "sha256-list");
    let (result, _run_id, journal_error) =
        run_and_journal(&mut journal, principal("agent"), plan_id, |observer| {
            apply(
                &checked,
                &no_inputs,
                &catalog,
                &approved,
                &Approval::Auto,
                observer,
            )
        });
    assert!(journal_error.is_none(), "{journal_error:?}");
    result.expect("runs");

    let started = journal
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::NodeStarted { node, inputs, .. } if node.as_str() == "sink" => Some(inputs),
            _ => None,
        })
        .expect("the sink's NodeStarted is journaled");
    let json = serde_json::to_string(started).unwrap();
    assert_eq!(
        json.matches("[REDACTED DopplerServiceToken]").count(),
        2,
        "one marker per element: {json}"
    );
    assert_clean("the sink's inputs", &json, &LIST_MARKER_BYTES);
    assert_clean(
        "the whole journal",
        &serde_json::to_string(journal.entries()).unwrap(),
        &LIST_MARKER_BYTES,
    );
}

/// A document whose input *default* is literally the redaction marker
/// string is not a secret and must not read as one: its JSON carries no
/// `redacted` key, which is the only structural difference between a value
/// that *is* redacted and one that merely spells the marker out. (A text
/// renderer cannot tell them apart, which is a rendering concern, handed
/// to pass 2.)
#[test]
fn a_document_default_spelling_the_redaction_marker_is_not_marked_redacted() {
    let (_state, catalog) = willikins_providers_fake::empty();
    let document = load(&fixture("redaction-marker-default.yaml"));
    let checked = check(&document, &catalog).expect("the fixture checks");
    let inputs = resolve_inputs(&checked, &[]);
    let approved = plan(&checked, &inputs, &catalog).expect("plans");

    let mut journal = MemoryJournal::new();
    let plan_id = record_plan(&mut journal, &checked, &inputs, &approved, "sha256-marker");
    let (result, _run_id, journal_error) =
        run_and_journal(&mut journal, principal("agent"), plan_id, |observer| {
            apply(
                &checked,
                &inputs,
                &catalog,
                &approved,
                &Approval::Auto,
                observer,
            )
        });
    assert!(journal_error.is_none(), "{journal_error:?}");
    result.expect("runs");

    let started = journal
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::NodeStarted { node, inputs, .. } if node.as_str() == "readme" => Some(inputs),
            _ => None,
        })
        .expect("the rendering node's NodeStarted is journaled");
    let json: serde_json::Value = serde_json::to_value(started).unwrap();
    let value = &json["value"];
    assert_eq!(
        value["value"],
        serde_json::json!("[REDACTED DopplerServiceToken]"),
        "the forged marker is carried as the plain text it is: {json}"
    );
    assert!(
        value.get("redacted").is_none(),
        "and is not marked redacted: {json}"
    );

    // For contrast: a real secret's JSON does carry the key.
    let secret = Value::known(distinctive_token());
    let secret_json: serde_json::Value = serde_json::to_value(&secret).unwrap();
    assert_eq!(secret_json["redacted"], serde_json::json!(true));
}
