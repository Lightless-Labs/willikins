//! Milestone 3l, task D1 (`docs/plans/2026-10-05-milestone-3l-new-repositories.md`,
//! acceptance 14-16): `workflows/github-new-repository-scaffold.yaml` over
//! the fake catalog -- the tracked proof that a document can create a
//! brand-new (or still-empty) GitHub repository and scaffold it in the
//! same run, decisions (a) and (b) end to end.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Action, Approval, Binding, InputName, NodeName, NodeStatus, PlanError, PortName, ToolErrorKind,
    Value, apply, check, plan,
};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::state::{repo_key, scaffold_key};
use willikins_types::{DomainType, GitBranchName, GitHubRepo, RepoVisibility};

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn document() -> willikins_core::Workflow {
    let path = workspace_root().join("workflows/github-new-repository-scaffold.yaml");
    willikins_dsl::load_document(&path).unwrap_or_else(|err| panic!("the document loads: {err}"))
}

fn state_fixture(name: &str) -> std::path::PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join(name)
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("example-org/example-repo").unwrap()
}

fn branch() -> GitBranchName {
    GitBranchName::parse("main").unwrap()
}

/// Every input the document declares, with a fixed value -- `plan` never
/// backfills a default itself (`Binding::Input` fails `MissingInput` on
/// an absent input).
fn base_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(InputName::parse("repo").unwrap(), Value::known(repo()));
    inputs.insert(
        InputName::parse("visibility").unwrap(),
        Value::known(RepoVisibility::Private),
    );
    inputs
}

/// Seeded from `new-repository-empty-trunk.json`: the repository already
/// exists, ours, empty, with default branch `trunk` -- the document's
/// own `branch: main` literal then mismatches it.
fn trunk_state() -> Arc<Mutex<FakeState>> {
    let json = std::fs::read_to_string(state_fixture("new-repository-empty-trunk.json")).unwrap();
    Arc::new(Mutex::new(
        FakeState::from_json(&json).unwrap_or_else(|err| panic!("seeded state: {err}")),
    ))
}

fn action_of(planned: &willikins_core::Plan, node: &str, instance: Option<&str>) -> Action {
    planned
        .nodes
        .iter()
        .find(|n| n.name.as_str() == node && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("node `{node}` (instance {instance:?}) was planned"))
        .action
}

fn status_of<'a>(
    applied: &'a willikins_core::Applied,
    node: &str,
    instance: Option<&str>,
) -> &'a NodeStatus {
    &applied
        .nodes
        .iter()
        .find(|n| n.name.as_str() == node && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("node `{node}` (instance {instance:?}) was applied"))
        .status
}

/// `Approval::Human`, which every class (including the scaffold's
/// `Irreversible`) accepts -- the same way existing CLI tests approve an
/// irreversible class (`crates/willikins-cli/tests/it/acceptance.rs`).
fn human_approval() -> Approval {
    Approval::Human {
        approver: willikins_core::PrincipalId::parse("operator").unwrap(),
        at: willikins_core::Timestamp::now(),
    }
}

#[test]
fn the_document_checks_cleanly_against_the_fake_catalog() {
    let (_state, catalog) = willikins_providers_fake::empty();
    check(&document(), &catalog).expect("the document checks cleanly");
}

/// Acceptance 14: a fresh, empty `FakeState`. Run 1 plans `repo` and
/// `scaffold` both `Create`, with no `PlanError`. Apply (approving the
/// scaffold's irreversible class), then run 2 plans both `NoOp`, and the
/// fake holds both seed files, the marker and branch `main`.
#[test]
fn fresh_run_creates_then_converges_with_both_files_the_marker_and_branch_main() {
    let (state, catalog) = willikins_providers_fake::empty();
    let checked = check(&document(), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = base_inputs();

    let planned1 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));
    assert_eq!(action_of(&planned1, "repo", None), Action::Create);
    assert_eq!(action_of(&planned1, "scaffold", None), Action::Create);

    let mut observer = willikins_core::RecordingObserver::new();
    let applied1 = apply(
        &checked,
        &inputs,
        &catalog,
        &planned1,
        &human_approval(),
        &mut observer,
    )
    .expect("run 1 applies");
    assert!(matches!(
        status_of(&applied1, "repo", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        status_of(&applied1, "scaffold", None),
        NodeStatus::Created
    ));

    let planned2 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 2 plans: {err}"));
    assert_eq!(action_of(&planned2, "repo", None), Action::NoOp);
    assert_eq!(action_of(&planned2, "scaffold", None), Action::NoOp);

    let locked = state.lock().unwrap();
    let record = locked
        .github_repos
        .get(&repo_key(&repo()))
        .expect("the repository was created");
    assert_eq!(
        record.branches,
        Some(vec![branch()]),
        "the scaffold's own first write must record branch `main` as existing"
    );
    let scaffold_map = locked
        .scaffolds
        .get(&scaffold_key(&repo(), &branch()))
        .expect("the scaffold landed");
    assert!(scaffold_map.contains_key(".editorconfig"));
    assert!(scaffold_map.contains_key("README.md"));
    assert!(scaffold_map.contains_key(".willikins-scaffold"));
}

/// Acceptance 15: `new-repository-empty-trunk.json` seeds a repository
/// that already exists, empty, with default branch `trunk`. The
/// document's own `branch: main` literal then mismatches it, so run 1
/// fails at plan with the mismatch `Conflict`, attributed to `scaffold`.
#[test]
fn a_mismatched_default_branch_fails_plan_with_a_conflict_attributed_to_scaffold() {
    let state = trunk_state();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&document(), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = base_inputs();

    let err = plan(&checked, &inputs, &catalog)
        .expect_err("a mismatched default branch must refuse to plan");
    match err {
        PlanError::Tool {
            node: failed_node,
            error,
        } => {
            assert_eq!(failed_node, NodeName::parse("scaffold").unwrap());
            assert_eq!(error.kind, ToolErrorKind::Conflict);
            assert!(
                error.message.contains("main") && error.message.contains("trunk"),
                "the mismatch must name both branches: {}",
                error.message
            );
        }
        other => panic!("expected PlanError::Tool at scaffold, got {other:?}"),
    }
}

/// Acceptance 16: `scaffold.repo` binds `repo.repo` through a
/// `Binding::Step`, pinning the order `github.repo.ensure` before
/// `github.scaffold.ensure`.
#[test]
fn scaffold_repo_binds_repo_repo() {
    let workflow = document();
    let node = &workflow.nodes[&NodeName::parse("scaffold").unwrap()];
    match node.with.get(&PortName::parse("repo").unwrap()) {
        Some(Binding::Step { node: n, port }) => {
            assert_eq!(n.as_str(), "repo");
            assert_eq!(port.as_str(), "repo");
        }
        other => panic!("expected a Binding::Step onto repo.repo, got {other:?}"),
    }
}
