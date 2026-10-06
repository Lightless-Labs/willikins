//! Milestone 2b, task S1
//! (`docs/plans/2026-10-05-milestone-2b-composition.md`, decision (d8),
//! acceptance 9 first half): `Butler::validate`/`describe` link a
//! document's `uses:` tree -- resolved in this `Butler`'s own trusted
//! workflow directory, never from a caller-supplied body (trust boundary
//! 2) -- before `willikins_core::check` ever sees it. Also pins
//! `Butler::describe`'s own hiding of a fixed input (decision (d6), ties
//! to R1; acceptance 7) from its agent-facing response.

use crate::common;

use willikins_core::{CheckError, InputName};
use willikins_server::{Butler, DocumentSource};
use willikins_types::{DomainType, WorkflowName};

fn wf(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

fn write(dir: &std::path::Path, filename: &str, contents: &str) {
    std::fs::write(dir.join(filename), contents).unwrap();
}

fn butler_over(dir: &std::path::Path) -> Butler {
    let (_state, catalog) = Butler::fake_catalog();
    let clock = common::manual_clock();
    let (butler, _journal) = common::butler_with_journal(dir, catalog, clock);
    butler
}

// ---------------------------------------------------------------------
// Trust boundary 2: a body never supplies a child; every `uses:` step
// resolves in this `Butler`'s own trusted directory.
// ---------------------------------------------------------------------

#[test]
fn validate_of_a_body_links_its_uses_step_against_the_trusted_directory() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "boundary-child.yaml",
        "name: boundary-child\ninputs:\n  key: { type: ProjectSlug }\nsteps: {}\n",
    );
    let butler = butler_over(dir.path());

    let body = "name: root\nsteps:\n  c:\n    uses: boundary-child\n    with:\n      \
                key: ${{ item }}\n"
        .to_string();
    // A nonsense body (`${{ item }}` outside any `for_each`) is still
    // enough to prove the *child* came from the trusted file: if the
    // child resolved from anywhere else (or not at all), this would fail
    // differently (`UnknownWorkflow`), not `ItemInUses`.
    let response = butler
        .validate(&DocumentSource::Body(body), common::principal("agent"))
        .unwrap();
    assert!(!response.ok);
    assert!(
        matches!(response.errors.as_slice(), [CheckError::ItemInUses { .. }]),
        "{:?}",
        response.errors
    );
}

#[test]
fn validate_of_a_body_checks_a_uses_binding_against_the_trusted_childs_declared_type() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "boundary-child.yaml",
        "name: boundary-child\ninputs:\n  key: { type: ProjectSlug }\nsteps: {}\n",
    );
    let butler = butler_over(dir.path());

    // `org` is `GitHubOrg`, not `ProjectSlug` -- the trusted child's own
    // declared type (not anything the body could have asserted) is what
    // makes this a mismatch.
    let body = "name: root\ninputs:\n  org: { type: GitHubOrg }\nsteps:\n  c:\n    \
                uses: boundary-child\n    with:\n      key: ${{ inputs.org }}\n"
        .to_string();
    let response = butler
        .validate(&DocumentSource::Body(body), common::principal("agent"))
        .unwrap();
    assert!(!response.ok);
    assert!(
        matches!(
            response.errors.as_slice(),
            [CheckError::UsesInputTypeMismatch { .. }]
        ),
        "{:?}",
        response.errors
    );
}

// ---------------------------------------------------------------------
// A symlinked child and a name-mismatched child both refuse as
// `UnknownWorkflow`, without saying which (acceptance 9).
// ---------------------------------------------------------------------

fn uses_child_body() -> String {
    "name: root\nsteps:\n  c:\n    uses: child\n".to_string()
}

#[test]
fn a_name_mismatched_child_is_unknown_workflow() {
    let dir = tempfile::tempdir().unwrap();
    // The file is named `child.yaml`, but its own `name:` is `other` --
    // `load_named_document` refuses this (see that module's own docs),
    // collapsed by the resolver into the same `ResolveFailure::Refused`
    // a symlink gets.
    write(dir.path(), "child.yaml", "name: other\nsteps: {}\n");
    let butler = butler_over(dir.path());

    let response = butler
        .validate(
            &DocumentSource::Body(uses_child_body()),
            common::principal("agent"),
        )
        .unwrap();
    assert!(!response.ok);
    match response.errors.as_slice() {
        [CheckError::UnknownWorkflow { node, workflow }] => {
            assert_eq!(node.as_str(), "c");
            assert_eq!(workflow.as_str(), "child");
        }
        other => panic!("expected exactly one UnknownWorkflow, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_child_is_unknown_workflow_and_identical_to_a_name_mismatch() {
    let mismatch_dir = tempfile::tempdir().unwrap();
    write(
        mismatch_dir.path(),
        "child.yaml",
        "name: other\nsteps: {}\n",
    );
    let mismatch_butler = butler_over(mismatch_dir.path());
    let mismatch_response = mismatch_butler
        .validate(
            &DocumentSource::Body(uses_child_body()),
            common::principal("agent"),
        )
        .unwrap();

    let symlink_dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "real.yaml", "name: child\nsteps: {}\n");
    std::os::unix::fs::symlink(
        outside.path().join("real.yaml"),
        symlink_dir.path().join("child.yaml"),
    )
    .unwrap();
    let symlink_butler = butler_over(symlink_dir.path());
    let symlink_response = symlink_butler
        .validate(
            &DocumentSource::Body(uses_child_body()),
            common::principal("agent"),
        )
        .unwrap();

    assert!(!symlink_response.ok);
    assert!(
        matches!(
            symlink_response.errors.as_slice(),
            [CheckError::UnknownWorkflow { .. }]
        ),
        "{:?}",
        symlink_response.errors
    );

    // "Without saying which": a symlink and a name mismatch are reported
    // through the exact same shape -- byte-identical JSON, not just the
    // same variant name.
    let mismatch_json = serde_json::to_value(&mismatch_response).unwrap();
    let symlink_json = serde_json::to_value(&symlink_response).unwrap();
    assert_eq!(mismatch_json, symlink_json);
}

// ---------------------------------------------------------------------
// `Butler::describe` hides a fixed input from its agent-facing response
// (decision (d6); ties to R1; acceptance 7).
// ---------------------------------------------------------------------

#[test]
fn describe_of_a_composite_omits_a_fixed_input_from_resolved_and_missing() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "fixed-child.yaml",
        "name: fixed-child\ninputs:\n  constant: { type: GitHubOrg, default: Example-Org }\n\
         steps: {}\noutputs:\n  constant: ${{ inputs.constant }}\n",
    );
    let butler = butler_over(dir.path());

    // The root never binds `constant`, so it becomes the fixed input
    // `c/constant` (decision (d6)).
    let body = "name: root\nsteps:\n  c:\n    uses: fixed-child\n".to_string();
    let description = butler
        .describe(
            &DocumentSource::Body(body),
            &indexmap::IndexMap::default(),
            common::principal("agent"),
        )
        .unwrap();

    let fixed = InputName::parse("c/constant").unwrap();
    assert!(
        !description.resolved.contains_key(&fixed),
        "{:?}",
        description.resolved
    );
    assert!(
        description.missing.iter().all(|m| m.name != fixed),
        "{:?}",
        description.missing
    );
}

#[test]
fn validate_an_unknown_child_by_name_is_a_check_error_too() {
    // Sanity: a *named* (not body) root with an unresolvable `uses:`
    // child behaves the same way through `Butler::validate` -- folded
    // into `ok: false`, never a hard `Err`.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "root.yaml", &uses_child_body());
    let butler = butler_over(dir.path());
    let response = butler
        .validate(
            &DocumentSource::Name(wf("root")),
            common::principal("agent"),
        )
        .expect("linking failures fold into ok:false, never an Err");
    assert!(!response.ok);
    assert!(
        matches!(
            response.errors.as_slice(),
            [CheckError::UnknownWorkflow { .. }]
        ),
        "{:?}",
        response.errors
    );
}
