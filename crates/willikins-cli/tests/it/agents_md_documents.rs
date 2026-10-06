//! Milestone 3o, tasks E1 and E2
//! (`docs/plans/2026-10-06-milestone-3o-agents-md.md`):
//!
//! **E1 (acceptance 1-3): the standalone pair** -- an organisation-owned
//! used document (`workflows/fixtures/agents-md/example-org-agents-md.yaml`)
//! renders a standalone repository's whole `AGENTS.md`, decisions (d1)
//! and (d3). `standalone-service.yaml` `uses:` it and lists its `file`
//! output in its one scaffold commit, and `project-prose-root.yaml` is
//! the negative fixture proving a `uses:` step's `with:` literal is type
//! -checked against the child's declared input, never embedded unchecked.
//!
//! **E2 (acceptance 4-7): the monorepo app document** --
//! `workflows/fixtures/agents-md/monorepo-app.yaml`, decisions (d2) and
//! (d4): the app's own document scaffolds `apps/sample/` and then seeds
//! its `AGENTS.md` through a **second** `github.scaffold.ensure` node
//! with its own marker, so the same document shape plans `Create` on a
//! brand-new app and on one already scaffolded alike.
//!
//! Linking is done here with a small local `resolve` closure mirroring
//! `willikins-cli`'s own `resolve_sibling` (`src/main.rs`, milestone 2b
//! decision (d8): a sibling file, by name, in the document's own parent
//! directory) -- that function is `pub(crate)` to the `willikins-cli`
//! binary and so unreachable from this separate integration-test binary.
//! The apply/approval helpers below mirror
//! `new_repository_scaffold_document.rs`'s own (`human_approval`,
//! `action_of`, `status_of`), kept local rather than shared since neither
//! file otherwise needs the other's fixture-specific constants.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Action, Approval, Binding, CheckError, InputName, NodeName, NodeStatus, Plan, PlanError,
    PortName, RecordingObserver, ResolveFailure, ToolErrorKind, Value, apply, check, link, plan,
};
use willikins_providers_fake::FakeState;
use willikins_providers_fake::state::{repo_key, scaffold_key};
use willikins_types::{
    AppleBundleIdentifier, DomainType, GitBranchName, GitHubRepo, RepoFile, RepoVisibility,
    TemplateValue,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures_dir() -> PathBuf {
    workspace_root().join("workflows/fixtures/agents-md")
}

fn load(name: &str) -> willikins_core::Workflow {
    let path = fixtures_dir().join(format!("{name}.yaml"));
    willikins_dsl::load_document(&path).unwrap_or_else(|err| panic!("{name} loads: {err}"))
}

/// Mirrors `willikins-cli`'s own `resolve_sibling` (`src/main.rs`): a
/// `uses:` step's used document resolves by name, as a plain (never
/// symlinked) `<name>.yaml` sibling in this fixture directory, with its
/// own `name:` field checked against the requested name.
fn resolve_sibling(
    dir: &Path,
    name: &willikins_types::WorkflowName,
) -> Result<willikins_core::Workflow, ResolveFailure> {
    let candidate = dir.join(format!("{name}.yaml"));
    match std::fs::symlink_metadata(&candidate) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(ResolveFailure::Refused),
        Ok(meta) if meta.is_file() => {}
        _ => return Err(ResolveFailure::NotFound),
    }
    let workflow =
        willikins_dsl::load_document(&candidate).map_err(|err| ResolveFailure::Document {
            message: err.to_string(),
        })?;
    if &workflow.name != name {
        return Err(ResolveFailure::Refused);
    }
    Ok(workflow)
}

/// `name`, loaded and linked against its siblings in `fixtures_dir()`.
fn linked(name: &str) -> willikins_core::Workflow {
    let root = load(name);
    let dir = fixtures_dir();
    link(&root, &mut |n| resolve_sibling(&dir, n))
        .unwrap_or_else(|errors| panic!("{name} links: {errors:?}"))
        .workflow
}

fn expected_file(name: &str) -> String {
    let path = fixtures_dir().join("expected").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn action_of(planned: &Plan, node: &str, instance: Option<&str>) -> Action {
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

/// The planned `file` output of `repo.file.render` node `node`, cloned
/// out of the plan's own predicted outputs -- pure nodes compute their
/// outputs at plan time, so this needs no apply.
fn rendered_file_of(planned: &Plan, node: &str, instance: Option<&str>) -> RepoFile {
    let planned_node = planned
        .nodes
        .iter()
        .find(|n| n.name.as_str() == node && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("node `{node}` (instance {instance:?}) was planned"));
    let value = planned_node
        .outputs
        .get(&PortName::parse("file").unwrap())
        .unwrap_or_else(|| panic!("node `{node}` has no `file` output"));
    value
        .downcast::<RepoFile>()
        .unwrap_or_else(|| panic!("node `{node}`'s `file` output is not a RepoFile"))
        .clone()
}

/// `Approval::Human`, which every class (including the scaffold's
/// `Irreversible`) accepts -- `new_repository_scaffold_document.rs`'s own
/// convention.
fn human_approval() -> Approval {
    Approval::Human {
        approver: willikins_core::PrincipalId::parse("operator").unwrap(),
        at: willikins_core::Timestamp::now(),
    }
}

fn repo() -> GitHubRepo {
    GitHubRepo::parse("example-org/agents-md-standalone").unwrap()
}

fn branch() -> GitBranchName {
    GitBranchName::parse("main").unwrap()
}

fn standalone_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(InputName::parse("repo").unwrap(), Value::known(repo()));
    inputs.insert(
        InputName::parse("project").unwrap(),
        Value::known(TemplateValue::parse("sample-service").unwrap()),
    );
    inputs.insert(
        InputName::parse("visibility").unwrap(),
        Value::known(RepoVisibility::Private),
    );
    inputs
}

/// Acceptance 1: `example-org-agents-md` checks **as a root** against the
/// fake catalog -- it has no `uses:` step, so it needs no linking first.
/// This is decision (d3)'s own property: the alternative it was chosen
/// over (the organisation document exporting a literal template-text
/// output for the project to render itself) fails `check_outputs` with
/// `LiteralOutput` the moment it is checked as a root, which is exactly
/// why this document instead exports a `RepoFile` (a `Step`-bound output,
/// never a literal).
#[test]
fn example_org_agents_md_checks_as_a_root() {
    let (_state, catalog) = willikins_providers_fake::empty();
    let workflow = load("example-org-agents-md");
    check(&workflow, &catalog).unwrap_or_else(|errors| panic!("checks cleanly: {errors:?}"));
}

/// Acceptance 2. `standalone-service` links its sibling and checks. With
/// an empty `FakeState`, run 1 plans `repo` `Create`, the linked render
/// node `agents_md/agents` `Compute`, and `scaffold` `Create`; the
/// rendered file's path is `AGENTS.md` and its content equals
/// `expected/standalone-AGENTS.md`. Apply (approving the scaffold's
/// `Irreversible` class the way `new_repository_scaffold_document.rs`
/// does), then run 2 plans every non-pure node `NoOp`, and the fake holds
/// `README.md`, `AGENTS.md` and the marker.
#[test]
fn standalone_service_links_and_checks() {
    let (state, catalog) = willikins_providers_fake::empty();
    let workflow = linked("standalone-service");
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the linked document checks cleanly: {errors:?}"));
    let inputs = standalone_inputs();

    let planned1 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));
    assert_eq!(action_of(&planned1, "repo", None), Action::Create);
    assert_eq!(
        action_of(&planned1, "agents_md/agents", None),
        Action::Compute
    );
    assert_eq!(action_of(&planned1, "readme_file", None), Action::Compute);
    assert_eq!(action_of(&planned1, "scaffold", None), Action::Create);

    let agents_file = rendered_file_of(&planned1, "agents_md/agents", None);
    assert_eq!(agents_file.path().as_str(), "AGENTS.md");
    assert_eq!(agents_file.content(), expected_file("standalone-AGENTS.md"));

    let mut observer = RecordingObserver::new();
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
    // Pure render nodes always compute, apply or no: they never read
    // external state, so they have no `NoOp` to converge to.
    assert_eq!(
        action_of(&planned2, "agents_md/agents", None),
        Action::Compute
    );
    assert_eq!(action_of(&planned2, "readme_file", None), Action::Compute);

    let locked = state.lock().unwrap();
    locked
        .github_repos
        .get(&repo_key(&repo()))
        .expect("the repository was created");
    let scaffold_map = locked
        .scaffolds
        .get(&scaffold_key(&repo(), &branch()))
        .expect("the scaffold landed");
    assert!(scaffold_map.contains_key("README.md"));
    assert!(scaffold_map.contains_key("AGENTS.md"));
    assert!(scaffold_map.contains_key(".willikins-scaffold"));
    let expected = expected_file("standalone-AGENTS.md");
    assert_eq!(scaffold_map.get("AGENTS.md"), Some(&expected));
}

/// Acceptance 3: `project-prose-root.yaml` links cleanly (the literal is
/// present, so no `UnboundUsesInput`), but then fails `check` with
/// `CheckError::InvalidLiteral`, naming the `uses:` step as `node` and
/// the child's own declared input as `port` -- `check_boundaries` parses
/// a `uses:` step's literal `with:` binding directly against the used
/// document's declared input type, before it is ever embedded into a
/// deeper node. This is `validate`'s own pipeline (load, link, check),
/// reached here with no `plan` call at all.
#[test]
fn project_prose_root_is_refused_invalid_literal_naming_the_with_input() {
    let (_state, catalog) = willikins_providers_fake::empty();
    let workflow = linked("project-prose-root");
    let errors = check(&workflow, &catalog).expect_err("a prose literal must fail check, not link");

    assert!(
        errors.iter().any(|error| matches!(
            error,
            CheckError::InvalidLiteral { node, port, .. }
                if node.as_str() == "agents_md" && port.as_str() == "project"
        )),
        "expected an InvalidLiteral naming node `agents_md`, port `project`: {errors:?}"
    );
}

// --- Milestone 3o, task E2 (acceptance 4-7): the monorepo app document ---
//
// `monorepo-app.yaml` has no `uses:` step, so it is loaded and checked
// directly (never through `linked`): `monorepo` (`github.repo.get`)
// references a repository this document does not own, `app_files` is the
// app's own first scaffold, and `agents`/`agents_md` are the second
// scaffold's pair, decision (d4). `agents.repo` binds `app_files.repo`
// (acceptance 7), ordering the two commits.

fn monorepo_repo() -> GitHubRepo {
    GitHubRepo::parse("Example-Org/monorepo").unwrap()
}

fn app_identifier() -> AppleBundleIdentifier {
    AppleBundleIdentifier::parse("com.example.app").unwrap()
}

fn monorepo_app_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        InputName::parse("app_identifier").unwrap(),
        Value::known(app_identifier()),
    );
    inputs
}

/// The monorepo exists (this document never creates it: `github.repo.get`
/// only references), with no scaffold of either kind landed yet.
fn fresh_monorepo_state() -> Arc<Mutex<FakeState>> {
    Arc::new(Mutex::new(FakeState::new().with_repo(
        &monorepo_repo(),
        RepoVisibility::Private,
        true,
    )))
}

/// The app's first scaffold has already landed (its marker alone is
/// enough: a scaffold never re-reads its files once the marker is
/// present). Seeded directly with `with_scaffold_files`, never by
/// applying the whole document -- that would land the second marker too.
fn already_scaffolded_state() -> Arc<Mutex<FakeState>> {
    Arc::new(Mutex::new(
        FakeState::new()
            .with_repo(&monorepo_repo(), RepoVisibility::Private, true)
            .with_scaffold_files(
                &monorepo_repo(),
                &branch(),
                &[("apps/sample/.willikins-scaffold", "managed-by: willikins\n")],
            ),
    ))
}

/// Acceptance 4: `monorepo-app` checks. A fresh monorepo (the repository
/// exists, no markers): `app_files` and `agents` both plan `Create`. The
/// rendered `agents_md` file's path is `apps/sample/AGENTS.md` and its
/// content equals `expected/monorepo-app-AGENTS.md`, with
/// `com.example.app` in place of `{{ 0 }}`. Apply creates both scaffolds.
#[test]
fn monorepo_app_fresh_creates_both_scaffolds_and_renders_the_file() {
    let state = fresh_monorepo_state();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let workflow = load("monorepo-app");
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = monorepo_app_inputs();

    let planned1 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));
    assert_eq!(action_of(&planned1, "app_files", None), Action::Create);
    assert_eq!(action_of(&planned1, "agents", None), Action::Create);

    let agents_file = rendered_file_of(&planned1, "agents_md", None);
    assert_eq!(agents_file.path().as_str(), "apps/sample/AGENTS.md");
    assert_eq!(
        agents_file.content(),
        expected_file("monorepo-app-AGENTS.md")
    );

    let mut observer = RecordingObserver::new();
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
        status_of(&applied1, "app_files", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        status_of(&applied1, "agents", None),
        NodeStatus::Created
    ));

    let planned2 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 2 plans: {err}"));
    assert_eq!(action_of(&planned2, "app_files", None), Action::NoOp);
    assert_eq!(action_of(&planned2, "agents", None), Action::NoOp);
}

/// Acceptance 5: already scaffolded (the first marker present in the
/// fake, through the fake's existing seeding, never through an apply of
/// the whole document): `app_files` plans `NoOp` and `agents` `Create`.
/// Apply, then re-plan: every node `NoOp`.
#[test]
fn monorepo_app_already_scaffolded_creates_only_the_agents_marker() {
    let state = already_scaffolded_state();
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&load("monorepo-app"), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = monorepo_app_inputs();

    let planned1 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));
    assert_eq!(action_of(&planned1, "app_files", None), Action::NoOp);
    assert_eq!(action_of(&planned1, "agents", None), Action::Create);

    let mut observer = RecordingObserver::new();
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
        status_of(&applied1, "agents", None),
        NodeStatus::Created
    ));

    let planned2 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 2 plans: {err}"));
    assert_eq!(action_of(&planned2, "app_files", None), Action::NoOp);
    assert_eq!(action_of(&planned2, "agents", None), Action::NoOp);

    let locked = state.lock().unwrap();
    let scaffold_map = locked
        .scaffolds
        .get(&scaffold_key(&monorepo_repo(), &branch()))
        .expect("the scaffold map exists");
    assert!(scaffold_map.contains_key("apps/sample/.willikins-scaffold"));
    assert!(scaffold_map.contains_key("apps/sample/.willikins-agents"));
    assert!(scaffold_map.contains_key("apps/sample/AGENTS.md"));
}

/// Acceptance 6, first half: a different `apps/sample/AGENTS.md` already
/// present (seeded with `with_scaffold_files`, no marker): the plan fails
/// with a `Conflict` naming the path, attributed to `agents`.
#[test]
fn monorepo_app_a_different_existing_agents_md_fails_plan_naming_the_path() {
    let state = Arc::new(Mutex::new(
        FakeState::new()
            .with_repo(&monorepo_repo(), RepoVisibility::Private, true)
            .with_scaffold_files(
                &monorepo_repo(),
                &branch(),
                &[("apps/sample/AGENTS.md", "someone else's file\n")],
            ),
    ));
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&load("monorepo-app"), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = monorepo_app_inputs();

    let err = plan(&checked, &inputs, &catalog)
        .expect_err("a different existing AGENTS.md must refuse to plan");
    match err {
        PlanError::Tool {
            node: failed_node,
            error,
        } => {
            assert_eq!(failed_node, NodeName::parse("agents").unwrap());
            assert_eq!(error.kind, ToolErrorKind::Conflict);
            assert!(
                error.message.contains("apps/sample/AGENTS.md"),
                "the conflict must name the path: {}",
                error.message
            );
        }
        other => panic!("expected PlanError::Tool at `agents`, got {other:?}"),
    }
}

/// Acceptance 6, second half: a byte-equal `apps/sample/AGENTS.md`
/// already present: `agents` plans `Create` (the marker is still
/// absent), the apply commits only the marker (the seed path is already
/// equal, so it is skipped on write), and the re-plan is `NoOp`. The
/// fake's flat path map cannot itself distinguish "skipped, byte-equal"
/// from "rewrote the same bytes"; what this asserts is the marker
/// landing and the file staying byte-equal to the draft, which is what
/// `ScaffoldState::Absent { already_equal }` (`github_scaffold_ensure.rs`)
/// is for.
#[test]
fn monorepo_app_a_byte_equal_existing_agents_md_commits_only_the_marker() {
    let state = Arc::new(Mutex::new(
        FakeState::new()
            .with_repo(&monorepo_repo(), RepoVisibility::Private, true)
            .with_scaffold_files(
                &monorepo_repo(),
                &branch(),
                &[(
                    "apps/sample/AGENTS.md",
                    expected_file("monorepo-app-AGENTS.md").as_str(),
                )],
            ),
    ));
    let catalog = willikins_providers_fake::catalog(state.clone());
    let checked = check(&load("monorepo-app"), &catalog)
        .unwrap_or_else(|errors| panic!("the document checks cleanly: {errors:?}"));
    let inputs = monorepo_app_inputs();

    let planned1 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 1 plans: {err}"));
    assert_eq!(action_of(&planned1, "agents", None), Action::Create);

    let mut observer = RecordingObserver::new();
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
        status_of(&applied1, "agents", None),
        NodeStatus::Created
    ));

    let planned2 =
        plan(&checked, &inputs, &catalog).unwrap_or_else(|err| panic!("run 2 plans: {err}"));
    assert_eq!(action_of(&planned2, "agents", None), Action::NoOp);

    let locked = state.lock().unwrap();
    let scaffold_map = locked
        .scaffolds
        .get(&scaffold_key(&monorepo_repo(), &branch()))
        .expect("the scaffold map exists");
    assert!(scaffold_map.contains_key("apps/sample/.willikins-agents"));
}

/// Acceptance 7: `agents.repo` binds `app_files.repo` through a
/// `Binding::Step`, ordering the two commits by a data edge rather than
/// by the commit retry alone.
#[test]
fn agents_repo_binds_app_files_repo() {
    let workflow = load("monorepo-app");
    let node = &workflow.nodes[&NodeName::parse("agents").unwrap()];
    match node.with.get(&PortName::parse("repo").unwrap()) {
        Some(Binding::Step { node: n, port }) => {
            assert_eq!(n.as_str(), "app_files");
            assert_eq!(port.as_str(), "repo");
        }
        other => panic!("expected a Binding::Step onto app_files.repo, got {other:?}"),
    }
}
