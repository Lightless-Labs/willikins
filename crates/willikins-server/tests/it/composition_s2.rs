//! Milestone 2b, task S2
//! (`docs/plans/2026-10-05-milestone-2b-composition.md`, trust boundary
//! 5, acceptance 9 second half): `Butler::plan` records `PlanRecorded.used`
//! with the content hash of every document its link resolved, and
//! `Butler::apply`'s reload (`reload_and_check`) re-links and refuses
//! `DocumentChanged` the moment that closure -- not just the root's own
//! bytes -- no longer matches what was recorded.

use std::sync::Arc;

use crate::common;

use willikins_core::Catalog;
use willikins_journal::{DocumentSha256, Event, RunState};
use willikins_server::ButlerError;
use willikins_types::{DomainType, WorkflowName};

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

fn write(dir: &std::path::Path, filename: &str, contents: &str) {
    std::fs::write(dir.join(filename), contents).unwrap();
}

/// A minimal catalog holding only `test.counting.ensure` -- a reversible,
/// non-pure tool that always `read`s `Absent` (so it always plans
/// `Create`, with no fake-provider state to seed and nothing for a fresh
/// re-plan to drift against).
fn counting_catalog() -> (Catalog, Arc<std::sync::Mutex<u32>>) {
    let (tool, calls) = common::CountingTool::new();
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog.insert(Arc::new(tool)).unwrap();
    (catalog, calls)
}

const CHILD_V1: &str = "name: child\ninputs:\n  key: { type: EnvironmentSlug }\nsteps:\n  c:\n    tool: test.counting.ensure\n    with:\n      key: ${{ inputs.key }}\n";
// Same name, same signature, different bytes (an added comment line) --
// what "the child is edited" (acceptance 9) means.
const CHILD_V2: &str = "# edited\nname: child\ninputs:\n  key: { type: EnvironmentSlug }\nsteps:\n  c:\n    tool: test.counting.ensure\n    with:\n      key: ${{ inputs.key }}\n";

const ROOT: &str = "name: root\nsteps:\n  c:\n    uses: child\n    with:\n      key: dev\n";
// Adds a second `uses:` step (`c2`, naming a child that need not even
// exist -- the root's own sha mismatch refuses before anything resolves
// it): "the root is edited to add a second child" (acceptance 9).
const ROOT_WITH_SECOND_CHILD: &str = "name: root\nsteps:\n  c:\n    uses: child\n    with:\n      key: dev\n  c2:\n    uses: child2\n    with:\n      key: prd\n";

const LEAF_V1: &str = "name: leaf\ninputs:\n  key: { type: EnvironmentSlug }\nsteps:\n  c:\n    tool: test.counting.ensure\n    with:\n      key: ${{ inputs.key }}\n";
const LEAF_V2: &str = "# edited\nname: leaf\ninputs:\n  key: { type: EnvironmentSlug }\nsteps:\n  c:\n    tool: test.counting.ensure\n    with:\n      key: ${{ inputs.key }}\n";
const MID: &str = "name: mid\nsteps:\n  leaf:\n    uses: leaf\n    with:\n      key: dev\n";
const ROOT_USES_MID: &str = "name: root\nsteps:\n  mid:\n    uses: mid\n";

// ---------------------------------------------------------------------
// `Butler::plan` records `used` with the child's sha (decision (d10)).
// ---------------------------------------------------------------------

#[test]
fn plan_of_a_composite_records_used_with_the_childs_sha() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "child.yaml", CHILD_V1);
    write(dir.path(), "root.yaml", ROOT);
    let (catalog, _calls) = counting_catalog();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("root"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("a composite with one uses: step plans cleanly");

    let entries = journal.lock().unwrap();
    let used = entries
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::PlanRecorded { plan_id, used, .. } if *plan_id == response.plan_id => {
                Some(used.clone())
            }
            _ => None,
        })
        .expect("a PlanRecorded line for this plan");

    let mut expected = std::collections::BTreeMap::new();
    expected.insert(wf("child"), DocumentSha256::compute(CHILD_V1.as_bytes()));
    assert_eq!(used, expected);
}

/// A root with no `uses:` step still records an empty `used` -- no
/// regression for the common case.
#[test]
fn plan_of_a_plain_document_records_an_empty_used() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (_state, catalog) = willikins_providers_fake::empty();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("new-rust-service"),
            &common::new_rust_service_inputs(),
            common::principal("agent"),
        )
        .expect("the positive fixture plans cleanly");

    let entries = journal.lock().unwrap();
    let used = entries
        .entries()
        .iter()
        .find_map(|entry| match &entry.event {
            Event::PlanRecorded { plan_id, used, .. } if *plan_id == response.plan_id => {
                Some(used.clone())
            }
            _ => None,
        })
        .expect("a PlanRecorded line for this plan");
    assert!(used.is_empty(), "{used:?}");
}

// ---------------------------------------------------------------------
// Unchanged: apply succeeds.
// ---------------------------------------------------------------------

#[test]
fn apply_of_a_composite_succeeds_when_the_closure_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "child.yaml", CHILD_V1);
    write(dir.path(), "root.yaml", ROOT);
    let (catalog, _calls) = counting_catalog();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("root"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("plans cleanly");
    assert!(
        !response.requires_approval,
        "test.counting.ensure is Reversible"
    );

    let handle = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect("an unchanged composite applies");
    let run = common::wait_for_run(&butler, handle.run_id, 2000);
    assert_eq!(run.state, RunState::Succeeded, "{run:?}");
}

// ---------------------------------------------------------------------
// DocumentChanged, each of acceptance 9's three cases.
// ---------------------------------------------------------------------

/// "The child is edited between plan and apply."
#[test]
fn a_child_edited_between_plan_and_apply_is_document_changed() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "child.yaml", CHILD_V1);
    write(dir.path(), "root.yaml", ROOT);
    let (catalog, calls) = counting_catalog();
    let clock = common::manual_clock();
    let (butler, journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("root"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("plans cleanly");

    write(dir.path(), "child.yaml", CHILD_V2);

    let calls_before = *calls.lock().unwrap();
    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("an edited child must refuse");
    assert!(
        matches!(err, ButlerError::DocumentChanged { .. }),
        "{err:?}"
    );
    assert_eq!(
        *calls.lock().unwrap(),
        calls_before,
        "no provider call must have happened"
    );

    let entries = journal.lock().unwrap();
    let refused = entries.entries().iter().any(|entry| {
        matches!(
            &entry.event,
            Event::ApplyRefused {
                plan_id,
                reason: willikins_journal::ApplyRefusedReason::DocumentChanged,
                ..
            } if *plan_id == response.plan_id
        )
    });
    assert!(refused, "no ApplyRefused{{DocumentChanged}} recorded");
}

/// "The root is edited to add a second child." Caught by the existing
/// root-sha check alone (the second child, `child2`, need not even exist)
/// -- pinned here so the composite case keeps that pre-2b behaviour.
#[test]
fn the_root_edited_to_add_a_second_child_is_document_changed() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "child.yaml", CHILD_V1);
    write(dir.path(), "root.yaml", ROOT);
    let (catalog, _calls) = counting_catalog();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("root"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("plans cleanly");

    write(dir.path(), "root.yaml", ROOT_WITH_SECOND_CHILD);

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("a root that grew a second uses: step must refuse");
    assert!(
        matches!(err, ButlerError::DocumentChanged { .. }),
        "{err:?}"
    );
}

/// "A child changes from one used document to another under the same
/// root bytes": the root's own bytes (and so its sha) never change --
/// only which file on disk satisfies the name `child` does, from
/// `child.yaml` to a `child.yml` with different content. The root's own
/// sha check (compared first) cannot see this; only the `used` closure
/// comparison can.
#[test]
fn a_child_resolved_to_a_different_document_under_the_same_root_bytes_is_document_changed() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "child.yaml", CHILD_V1);
    write(dir.path(), "root.yaml", ROOT);
    let (catalog, _calls) = counting_catalog();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("root"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("plans cleanly");

    // The root file is untouched: still exactly `ROOT`'s bytes.
    assert_eq!(
        std::fs::read(dir.path().join("root.yaml")).unwrap(),
        ROOT.as_bytes()
    );
    std::fs::remove_file(dir.path().join("child.yaml")).unwrap();
    write(dir.path(), "child.yml", CHILD_V2);

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("a child swapped for a different document under the same name must refuse");
    assert!(
        matches!(err, ButlerError::DocumentChanged { .. }),
        "{err:?}"
    );
}

/// The closure covers the *whole* tree, not just the root's direct
/// children: editing a grandchild (`leaf`, used by `mid`, used by
/// `root`) while `root` and `mid` stay byte-identical still refuses.
#[test]
fn an_edited_grandchild_is_document_changed_even_though_the_direct_child_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "leaf.yaml", LEAF_V1);
    write(dir.path(), "mid.yaml", MID);
    write(dir.path(), "root.yaml", ROOT_USES_MID);
    let (catalog, _calls) = counting_catalog();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir.path(), catalog, clock);

    let response = butler
        .plan(
            wf("root"),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .expect("a two-level composite plans cleanly");

    write(dir.path(), "leaf.yaml", LEAF_V2);
    // `root` and `mid` themselves are untouched.
    assert_eq!(
        std::fs::read(dir.path().join("mid.yaml")).unwrap(),
        MID.as_bytes()
    );
    assert_eq!(
        std::fs::read(dir.path().join("root.yaml")).unwrap(),
        ROOT_USES_MID.as_bytes()
    );

    let err = butler
        .apply(response.plan_id, common::principal("agent"))
        .expect_err("an edited grandchild must refuse even though root and mid did not change");
    assert!(
        matches!(err, ButlerError::DocumentChanged { .. }),
        "{err:?}"
    );
}
