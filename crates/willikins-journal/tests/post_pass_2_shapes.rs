//! The frozen **post**-pass-2 journal fixture: the shapes adversarial
//! pass 2 *added*, written once at the HEAD that added them.
//!
//! Added by adversarial pass 2's completeness critic, 2026-09-15.
//!
//! # Why a second fixture
//!
//! `pre_pass_2_replay.rs` freezes a journal written *before* the pass's
//! wire-format changes, and proves that every line an older binary wrote
//! still replays. That is the backward direction, and it is the one that
//! protects a deployment mid-upgrade.
//!
//! It cannot, by construction, say anything about the new shapes: a
//! pre-change journal has no `InvalidNonce`, no `ForeignOrigin`, no
//! `MalformedUsername`, no `RecordedInputUnreadable`, no
//! `ApplyPreparing`, and no `PlanRecorded` carrying a principal --
//! because none of them existed when it was written. The pass's own
//! claim ("every event kind, every `ApplyRefusedReason`, every
//! `DriftReasonKind`, both `Outcome`s, both transports and every
//! `AuthFailedReason`") is true of the pre-change enums and silent about
//! the post-change ones, so nothing in the tree pinned the *spelling* of
//! a single new variant on the wire. A later pass that renamed
//! `invalid_nonce` to `bad_nonce`, or dropped `input` from
//! `recorded_input_unreadable`, would have broken every journal written
//! by this milestone's binary and no test would have said so.
//!
//! This file is that pin, and it is the next pass's *pre*-change
//! baseline: freeze it, read it, never regenerate it. It also carries
//! the four `NodeStatus` kinds the pre-change fixture happens not to
//! exercise (`Computed`, `Unchanged`, `Converged`, `NotRun`) -- not an
//! overclaim in the note, which never mentions `NodeStatus`, but a gap
//! all the same.

mod common;

use std::path::{Path, PathBuf};

use willikins_journal::{ApplyRefusedReason, AuthFailedReason, Event, Transport};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("post-pass-2-new-shapes.jsonl")
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
fn the_new_shapes_replay_through_file_journal_open() {
    let (_dir, path) = copy_to_temp();
    let journal = willikins_journal::FileJournal::open(&path).expect("the new shapes replay");
    assert!(
        !willikins_journal::Journal::entries(&journal).is_empty(),
        "the fixture must not be empty"
    );
}

#[test]
fn the_new_shapes_replay_through_the_lock_free_reader() {
    let replayed = willikins_journal::replay(fixture_path()).expect("the new shapes replay");
    assert!(!replayed.entries().is_empty());
}

/// Every [`AuthFailedReason`] adversarial pass 2 added, by its spelling
/// on the wire. Finding 2's whole point was that an operator reading the
/// audit trail is told which of four different things happened; a
/// renamed or dropped variant would silently take that back.
#[test]
fn every_new_auth_failed_reason_replays_by_name() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let mut seen: Vec<&'static str> = Vec::new();
    for entry in replayed.entries() {
        if let Event::AuthFailed { reason, .. } = &entry.event {
            seen.push(match reason {
                AuthFailedReason::MissingCredential => "missing_credential",
                AuthFailedReason::InvalidCredential => "invalid_credential",
                AuthFailedReason::WrongRole => "wrong_role",
                AuthFailedReason::InvalidNonce => "invalid_nonce",
                AuthFailedReason::ForeignOrigin => "foreign_origin",
                AuthFailedReason::MalformedUsername => "malformed_username",
            });
        }
    }
    for expected in ["invalid_nonce", "foreign_origin", "malformed_username"] {
        assert!(
            seen.contains(&expected),
            "the frozen fixture must exercise `{expected}`, got {seen:?}"
        );
    }
    // And the raw bytes carry those exact keys, not merely something
    // that happens to deserialize into the variant.
    let raw = std::fs::read_to_string(fixture_path()).unwrap();
    for expected in ["invalid_nonce", "foreign_origin", "malformed_username"] {
        assert!(
            raw.contains(expected),
            "`{expected}` must be the spelling on the wire"
        );
    }
}

/// Both shapes of `RecordedInputUnreadable`, and `ApplyPreparing`.
///
/// `input: None` is not decoration: it is the case where the whole
/// recorded `inputs` payload is unreadable and there is no single input
/// to name (finding 3). A future change that made `input` required would
/// make that line unwritable, and this is what says so.
#[test]
fn both_new_apply_refused_reasons_replay_including_an_unnamed_input() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let mut named = false;
    let mut unnamed = false;
    let mut preparing = false;
    for entry in replayed.entries() {
        if let Event::ApplyRefused { reason, .. } = &entry.event {
            match reason {
                ApplyRefusedReason::RecordedInputUnreadable { input: Some(name) } => {
                    assert_eq!(name.as_str(), "slug");
                    named = true;
                }
                ApplyRefusedReason::RecordedInputUnreadable { input: None } => unnamed = true,
                ApplyRefusedReason::ApplyPreparing => preparing = true,
                _ => {}
            }
        }
    }
    assert!(named, "RecordedInputUnreadable naming an input");
    assert!(unnamed, "RecordedInputUnreadable naming none");
    assert!(preparing, "ApplyPreparing");
}

/// `PlanRecorded.principal` is present and folds to the requester.
///
/// The pre-change fixture pins the other half -- a line with no
/// `principal` still folds, to `None` -- so the two together say the
/// field is genuinely optional in both directions.
#[test]
fn a_plan_recorded_with_a_principal_folds_to_that_requester() {
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let recorded = replayed
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::PlanRecorded {
                plan_id, principal, ..
            } => Some((*plan_id, principal.clone())),
            _ => None,
        })
        .expect("a PlanRecorded line");
    assert_eq!(
        recorded
            .1
            .as_ref()
            .map(willikins_journal::PrincipalId::as_str),
        Some("agent-0123456789ab"),
        "the recorded principal must survive the round trip"
    );
    let plan = replayed
        .plan(&recorded.0)
        .expect("the plan folds out of the journal");
    assert_eq!(
        plan.requested_by
            .as_ref()
            .map(willikins_journal::PrincipalId::as_str),
        Some("agent-0123456789ab"),
        "and fold as the requester"
    );
}

/// The four `NodeStatus` kinds the pre-change fixture does not exercise.
/// Each one is a payload an older binary could not have written, so this
/// is the only file that pins their spelling.
#[test]
fn every_node_status_kind_the_pre_change_fixture_misses_replays() {
    use willikins_core::NodeStatus;
    let replayed = willikins_journal::replay(fixture_path()).expect("replays");
    let mut seen: Vec<&'static str> = Vec::new();
    for entry in replayed.entries() {
        if let Event::NodeFinished { status, .. } = &entry.event {
            seen.push(match status {
                NodeStatus::Computed => "computed",
                NodeStatus::Created => "created",
                NodeStatus::Unchanged => "unchanged",
                NodeStatus::Converged => "converged",
                NodeStatus::Failed { .. } => "failed",
                NodeStatus::NotRun => "not_run",
            });
        }
    }
    for expected in ["computed", "unchanged", "converged", "not_run"] {
        assert!(
            seen.contains(&expected),
            "the frozen fixture must exercise `{expected}`, got {seen:?}"
        );
    }
}

// ---------------------------------------------------------------------
// The generator. `#[ignore]`d, and never to be pointed at the committed
// fixture again -- see this file's own module doc.
// ---------------------------------------------------------------------

/// Write a journal exercising every shape adversarial pass 2 added to
/// `$WILLIKINS_FIXTURE_OUT`, for the one-shot generation of the frozen
/// fixture. Kept in the tree as the fixture's provenance, not as a way
/// to refresh it.
#[test]
#[ignore = "one-shot fixture generation; the committed fixture is frozen"]
fn regenerate_the_frozen_fixture() {
    use common::{document_sha256, node, principal, workflow_name};
    use indexmap::IndexMap;
    use willikins_core::{Class, InstanceFingerprint, NodeStatus};
    use willikins_journal::{Clock, Journal, ManualClock, PlanId, Redacted, RunId, Timestamp};

    let out = std::env::var("WILLIKINS_FIXTURE_OUT")
        .expect("set WILLIKINS_FIXTURE_OUT to the path to write");
    let _ = std::fs::remove_file(&out);

    let clock = std::sync::Arc::new(ManualClock::new(
        Timestamp::parse("2026-09-15T12:00:00+00:00").unwrap(),
    ));
    let clock_for_journal: std::sync::Arc<dyn Clock> = clock.clone();
    let mut journal = willikins_journal::FileJournal::open_with_clock(&out, clock_for_journal)
        .expect("a fresh journal file");

    let plan_a = PlanId::new();
    let run_one = RunId::new();
    let agent = principal("agent-0123456789ab");

    let statuses = [
        NodeStatus::Computed,
        NodeStatus::Unchanged,
        NodeStatus::Converged,
        NodeStatus::NotRun,
    ];

    let mut events = vec![
        Event::PlanRecorded {
            plan_id: plan_a,
            workflow: workflow_name("new-rust-service"),
            document_sha256: document_sha256("new-rust-service"),
            inputs: Redacted::from(&IndexMap::new()),
            plan: Redacted::from(&willikins_core::Plan {
                workflow: workflow_name("new-rust-service"),
                nodes: Vec::new(),
                outputs: IndexMap::new(),
                class: Class::Reversible,
                requires_approval: false,
            }),
            fingerprint: Vec::<InstanceFingerprint>::new(),
            class: Class::Reversible,
            requires_approval: false,
            principal: Some(agent.clone()),
        },
        Event::ApplyRefused {
            plan_id: plan_a,
            principal: agent.clone(),
            reason: ApplyRefusedReason::RecordedInputUnreadable {
                input: Some(willikins_core::InputName::parse("slug").unwrap()),
            },
        },
        Event::ApplyRefused {
            plan_id: plan_a,
            principal: agent.clone(),
            reason: ApplyRefusedReason::RecordedInputUnreadable { input: None },
        },
        Event::ApplyRefused {
            plan_id: plan_a,
            principal: agent.clone(),
            reason: ApplyRefusedReason::ApplyPreparing,
        },
        Event::RunStarted {
            run_id: run_one,
            plan_id: plan_a,
            principal: agent.clone(),
        },
    ];
    for (index, status) in statuses.into_iter().enumerate() {
        events.push(Event::NodeFinished {
            run_id: run_one,
            node: node(["repo", "project", "config", "secret"][index]),
            instance: None,
            status,
            outputs: Redacted::from(&willikins_core::Outputs::new()),
            error: None,
        });
    }
    events.extend([
        Event::AuthFailed {
            transport: Transport::Http,
            reason: AuthFailedReason::InvalidNonce,
        },
        Event::AuthFailed {
            transport: Transport::Http,
            reason: AuthFailedReason::ForeignOrigin,
        },
        Event::AuthFailed {
            transport: Transport::Http,
            reason: AuthFailedReason::MalformedUsername,
        },
        Event::AuthFailed {
            transport: Transport::Stdio,
            reason: AuthFailedReason::MalformedUsername,
        },
    ]);

    for event in events {
        clock.advance(std::time::Duration::from_secs(1));
        journal.append(event).expect("every event appends");
    }
}
