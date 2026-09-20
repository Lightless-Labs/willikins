//! Milestone 3a acceptance tests 11 through 14
//! (`docs/plans/2026-09-16-milestone-3a-buildkite-and-the-real-workflow.md`):
//! the second positive fixture, `workflows/new-rust-service-buildkite.yaml`,
//! plus three deliberately wrong documents proving the typed-secret and
//! typed-port guarantees hold across the Buildkite provider's own surface,
//! not only the milestone 1 one.
//!
//! Mirrors `tests/acceptance.rs`'s own conventions (a real parsed
//! [`willikins_dsl`] document, the real `willikins-providers-fake` catalog,
//! and — for the type-parity check — the real live `ToolSpec`s built
//! through `willikins_server::live_catalog_with` against an `Http` that
//! never leaves the process) rather than duplicating them in a hand-built
//! catalog. Every fixture used here carries its own header comment naming
//! the exact [`CheckError`]/[`PlanError`] it is expected to produce; this
//! file asserts the exact error kind and site, never a message substring.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::describe::{PartialInputs, RawInput};
use willikins_core::{
    Action, Approval, Catalog, CheckError, Checked, Class, InputName, NodeName, NodeStatus,
    OutputName, PlanError, PortName, PortType, Site, ToolErrorKind, TypeName, TypeRef, Value,
    Workflow, apply,
};
use willikins_providers_fake::FakeState;

// ---------------------------------------------------------------------
// paths and loading
// ---------------------------------------------------------------------

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

fn state_fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join(name)
}

fn positive_fixture() -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("new-rust-service-buildkite.yaml")
}

fn load(path: &Path) -> Workflow {
    willikins_dsl::load_document(path)
        .unwrap_or_else(|err| panic!("{}: failed to load: {err}", path.display()))
}

// ---------------------------------------------------------------------
// small identifier constructors, mirroring `tests/acceptance.rs`
// ---------------------------------------------------------------------

fn ty(name: &str) -> TypeRef {
    TypeRef::scalar(TypeName::parse(name).unwrap())
}

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn input(name: &str) -> InputName {
    InputName::parse(name).unwrap()
}

fn port_site(node_name: &str, port_name: &str) -> Site {
    Site::Port {
        node: node(node_name),
        port: port(port_name),
    }
}

// ---------------------------------------------------------------------
// catalogs
// ---------------------------------------------------------------------

/// A catalog of every fake tool against empty state — no Buildkite
/// cluster seeded, so `buildkite.cluster.get` reports `NotFound` for
/// every name.
fn empty_catalog() -> Catalog {
    willikins_providers_fake::empty().1
}

/// A catalog of every fake tool against the state seeded by the JSON file
/// at `path` (`workflows/fixtures/state/buildkite-cluster.json` for the
/// positive fixture's default cluster).
fn seeded_catalog(path: &Path) -> Catalog {
    let json = std::fs::read_to_string(path).unwrap_or_else(|err| {
        panic!(
            "{}: failed to read fake-state fixture: {err}",
            path.display()
        )
    });
    let state = Arc::new(Mutex::new(FakeState::from_json(&json).unwrap_or_else(
        |err| panic!("{}: invalid fake-state JSON: {err}", path.display()),
    )));
    willikins_providers_fake::catalog(state)
}

/// The live-shaped catalog: every real `ToolSpec`, built through
/// `willikins_server::live_catalog_with` against an `Http` that points
/// nowhere (`check`/`describe` never call a tool, only read its spec) and
/// test credentials that satisfy each provider's own credential-shape
/// check without reaching a network. Exactly the pattern
/// `willikins-server`'s own `catalog::tests::test_catalog` uses; built
/// here too since that helper is private to its crate.
fn live_shaped_catalog() -> Catalog {
    use willikins_providers_http::{Credential, Http};
    const NOWHERE: &str = "http://127.0.0.1:1";
    let github = Credential::for_testing("WILLIKINS_TEST_GITHUB_TOKEN", "ghp_testtoken");
    let doppler = Credential::for_testing("WILLIKINS_TEST_DOPPLER_TOKEN", "dp.sa.testtoken");
    let buildkite =
        Credential::for_testing("WILLIKINS_TEST_BUILDKITE_TOKEN", "bkua_testtoken12345678");
    willikins_server::live_catalog_with(
        Http::new(
            NOWHERE,
            willikins_providers_github::default_headers(),
            github,
        ),
        Http::new(NOWHERE, Vec::new(), doppler),
        Http::new(NOWHERE, Vec::new(), buildkite),
    )
}

// ---------------------------------------------------------------------
// input resolution
// ---------------------------------------------------------------------

fn resolve_inputs(
    label: &str,
    checked: &Checked,
    overrides: &[(&str, RawInput)],
) -> IndexMap<InputName, Value> {
    let mut partial = PartialInputs::new();
    for (name, raw) in overrides {
        partial.insert(input(name), raw.clone());
    }
    let description = willikins_core::describe(checked, &partial);
    assert!(
        description.errors.is_empty(),
        "{label}: describe reported unexpected errors: {:?}",
        description.errors
    );
    assert!(
        description.missing.is_empty(),
        "{label}: describe reported unexpected missing inputs: {:?}",
        description.missing
    );
    description.resolved
}

/// [`resolve_inputs`] for the positive fixture's three required inputs.
fn positive_inputs(label: &str, checked: &Checked) -> IndexMap<InputName, Value> {
    resolve_inputs(
        label,
        checked,
        &[
            ("slug", RawInput::Scalar("third-thoughts".to_string())),
            ("org", RawInput::Scalar("lightless-labs".to_string())),
            (
                "buildkite_org",
                RawInput::Scalar("willikins-test".to_string()),
            ),
        ],
    )
}

fn find_planned<'a>(
    plan: &'a willikins_core::Plan,
    name: &str,
    instance: Option<&str>,
) -> &'a willikins_core::PlannedNode {
    plan.nodes
        .iter()
        .find(|planned| planned.name == node(name) && planned.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("no planned node `{name}` (instance {instance:?})"))
}

fn node_status<'a>(
    applied: &'a willikins_core::Applied,
    name: &str,
    instance: Option<&str>,
) -> &'a NodeStatus {
    &applied
        .nodes
        .iter()
        .find(|n| n.name == node(name) && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("no applied node `{name}` (instance {instance:?})"))
        .status
}

// ---------------------------------------------------------------------
// acceptance test 11: the document checks and loads
// ---------------------------------------------------------------------

#[test]
fn acceptance_11_the_document_checks_cleanly_against_the_fake_catalog() {
    let workflow = load(&positive_fixture());
    let catalog = empty_catalog();
    let checked = willikins_core::check(&workflow, &catalog).expect(
        "acceptance test 11: the buildkite fixture must check cleanly against the fake catalog",
    );

    let expected_order: Vec<NodeName> = [
        "names",
        "repo",
        "doppler",
        "configs",
        "buildkite_cluster",
        "pipeline",
    ]
    .into_iter()
    .map(node)
    .collect();
    assert_eq!(
        checked.order, expected_order,
        "acceptance test 11: node order"
    );
    assert_eq!(
        checked.class,
        Class::Reversible,
        "acceptance test 11: class"
    );
    assert!(
        !checked.class.requires_approval(),
        "acceptance test 11: a document with no irreversible node must auto-approve"
    );
    assert!(
        checked.warnings.is_empty(),
        "acceptance test 11: {:?}",
        checked.warnings
    );
}

#[test]
fn acceptance_11_the_document_checks_cleanly_against_the_live_shaped_catalog_with_the_same_types() {
    let workflow = load(&positive_fixture());
    let fake = empty_catalog();
    let live = live_shaped_catalog();

    let checked_fake = willikins_core::check(&workflow, &fake)
        .expect("acceptance test 11: must check cleanly against the fake catalog");
    let checked_live = willikins_core::check(&workflow, &live)
        .expect("acceptance test 11: must check cleanly against the live-shaped catalog too");

    assert_eq!(
        checked_fake.order, checked_live.order,
        "acceptance test 11: node order must agree between catalogs"
    );
    assert_eq!(
        checked_fake.class, checked_live.class,
        "acceptance test 11: class must agree between catalogs"
    );
    for node_name in &checked_fake.order {
        assert_eq!(
            checked_fake.types.get(node_name),
            checked_live.types.get(node_name),
            "acceptance test 11: resolved port types for `{node_name}` must agree between \
             the fake and live-shaped catalogs"
        );
    }
}

#[test]
fn acceptance_11_describe_with_no_inputs_reports_exactly_slug_org_and_buildkite_org() {
    let workflow = load(&positive_fixture());
    let catalog = empty_catalog();
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("acceptance test 11: the buildkite fixture must check cleanly");

    let empty_partial = PartialInputs::new();
    let description = willikins_core::describe(&checked, &empty_partial);
    assert!(
        description.errors.is_empty(),
        "acceptance test 11: {:?}",
        description.errors
    );
    let missing_names: Vec<&str> = description
        .missing
        .iter()
        .map(|missing| missing.name.as_str())
        .collect();
    assert_eq!(
        missing_names,
        vec!["slug", "org", "buildkite_org"],
        "acceptance test 11: cluster, visibility and environments all have defaults"
    );
}

#[test]
fn acceptance_11_the_plan_is_reversible_and_auto_approves() {
    let workflow = load(&positive_fixture());
    let catalog = seeded_catalog(&state_fixture("buildkite-cluster.json"));
    let checked =
        willikins_core::check(&workflow, &catalog).expect("acceptance test 11: must check cleanly");
    let inputs = positive_inputs("acceptance test 11", &checked);
    let planned = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("acceptance test 11: plan against the seeded cluster must succeed");

    assert_eq!(
        planned.class,
        Class::Reversible,
        "acceptance test 11: plan class"
    );
    assert!(
        !planned.requires_approval,
        "acceptance test 11: a plan with no irreversible node must not require approval"
    );

    // `Approval::Auto` must actually be accepted end to end (this is what
    // "auto-approves" means operationally): the run must not be refused
    // with `ApplyError::ApprovalRequired`.
    let mut observer = willikins_core::NoopObserver;
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &planned,
        &Approval::Auto,
        &mut observer,
    )
    .expect("acceptance test 11: an auto-approved reversible plan must apply");
    assert!(
        applied
            .nodes
            .iter()
            .all(|n| !matches!(n.status, NodeStatus::Failed { .. })),
        "acceptance test 11: {:?}",
        applied.nodes
    );
}

// The container-image half of acceptance test 11 -- the glob test naming
// the workflows the image admits -- lives in
// `crates/willikins-server/tests/image_contents.rs::the_image_workflows_directory_holds_exactly_the_seven_positive_documents`,
// updated alongside this file.

// ---------------------------------------------------------------------
// acceptance test 12: executor happy path
// ---------------------------------------------------------------------

#[test]
fn acceptance_12_executor_happy_path_against_seeded_cluster_state() {
    let workflow = load(&positive_fixture());
    let catalog = seeded_catalog(&state_fixture("buildkite-cluster.json"));
    let checked =
        willikins_core::check(&workflow, &catalog).expect("acceptance test 12: must check cleanly");
    let inputs = positive_inputs("acceptance test 12", &checked);

    let planned = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("acceptance test 12: plan against the seeded cluster must succeed");
    assert_eq!(
        find_planned(&planned, "names", None).action,
        Action::Compute
    );
    assert_eq!(
        find_planned(&planned, "buildkite_cluster", None).action,
        Action::Compute
    );
    assert_eq!(find_planned(&planned, "repo", None).action, Action::Create);
    assert_eq!(
        find_planned(&planned, "doppler", None).action,
        Action::Create
    );
    assert_eq!(
        find_planned(&planned, "pipeline", None).action,
        Action::Create
    );
    // `doppler.project.ensure`'s fake auto-seeds the three default root
    // configs, so a fresh project's own `dev`/`stg`/`prd` configs are
    // already present by the time `configs` reads them.
    for env in ["dev", "stg", "prd"] {
        assert_eq!(
            find_planned(&planned, "configs", Some(env)).action,
            Action::Create,
            "acceptance test 12: configs[{env}] is planned fresh against empty Doppler state"
        );
    }

    let mut observer = willikins_core::NoopObserver;
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &planned,
        &Approval::Auto,
        &mut observer,
    )
    .expect("acceptance test 12: apply must succeed");

    assert!(matches!(
        node_status(&applied, "names", None),
        NodeStatus::Computed
    ));
    assert!(matches!(
        node_status(&applied, "buildkite_cluster", None),
        NodeStatus::Computed
    ));
    assert!(matches!(
        node_status(&applied, "repo", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&applied, "doppler", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&applied, "pipeline", None),
        NodeStatus::Created
    ));
    // `configs` fan-out over the three default environments is exercised,
    // not assumed: each instance is asserted individually, so a bug that
    // only ran one iteration or collapsed the for_each would fail here.
    for env in ["dev", "stg", "prd"] {
        assert!(
            matches!(
                node_status(&applied, "configs", Some(env)),
                NodeStatus::Unchanged
            ),
            "acceptance test 12: configs[{env}] must be Unchanged (auto-seeded by doppler.project.ensure)"
        );
    }

    assert!(
        applied
            .outputs
            .get(&OutputName::parse("repo_url").unwrap())
            .is_some_and(Value::is_known),
        "acceptance test 12: repo_url must be Known"
    );
    assert!(
        applied
            .outputs
            .get(&OutputName::parse("pipeline_url").unwrap())
            .is_some_and(Value::is_known),
        "acceptance test 12: pipeline_url must be Known"
    );

    // Nothing in this graph is secret: no redaction marker anywhere in
    // the plan or the applied result's own debug text.
    let plan_text = format!("{planned:?}");
    let applied_text = format!("{applied:?}");
    assert!(
        !plan_text.contains("REDACTED"),
        "acceptance test 12: the plan must carry no redaction marker: {plan_text}"
    );
    assert!(
        !applied_text.contains("REDACTED"),
        "acceptance test 12: the applied result must carry no redaction marker: {applied_text}"
    );
}

// ---------------------------------------------------------------------
// acceptance test 13: convergence
// ---------------------------------------------------------------------

#[test]
fn acceptance_13_convergence_after_a_pipeline_failure() {
    let workflow = load(&positive_fixture());
    let seed_json = std::fs::read_to_string(state_fixture("buildkite-cluster.json"))
        .expect("seed fixture readable");
    let state = Arc::new(Mutex::new(
        FakeState::from_json(&seed_json)
            .expect("seed fixture parses")
            .with_fail_ensure_once("buildkite.pipeline.ensure", "willikins-test/third-thoughts"),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked =
        willikins_core::check(&workflow, &catalog).expect("acceptance test 13: must check cleanly");
    let inputs = positive_inputs("acceptance test 13", &checked);

    let first_plan = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("acceptance test 13: first plan must succeed");
    let mut observer = willikins_core::NoopObserver;
    let first_err = apply(
        &checked,
        &inputs,
        &catalog,
        &first_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("acceptance test 13: the injected pipeline failure must stop the first apply");
    let willikins_core::ApplyError::Tool {
        node: failed_node,
        applied: first_applied,
        ..
    } = first_err
    else {
        panic!("acceptance test 13: expected ApplyError::Tool");
    };
    assert_eq!(failed_node, node("pipeline"));
    assert!(matches!(
        node_status(&first_applied, "pipeline", None),
        NodeStatus::Failed { .. }
    ));
    assert!(matches!(
        node_status(&first_applied, "repo", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&first_applied, "doppler", None),
        NodeStatus::Created
    ));

    // A fresh plan shows every finished node NoOp/Unchanged/Computed and
    // `pipeline` still to create.
    let second_plan = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("acceptance test 13: second plan must succeed");
    assert_eq!(
        find_planned(&second_plan, "pipeline", None).action,
        Action::Create,
        "acceptance test 13: pipeline is still to create after the failed attempt"
    );
    assert_eq!(
        find_planned(&second_plan, "repo", None).action,
        Action::NoOp,
        "acceptance test 13: repo, already created, is a no-op on the second plan"
    );

    let mut observer2 = willikins_core::NoopObserver;
    let second_applied = apply(
        &checked,
        &inputs,
        &catalog,
        &second_plan,
        &Approval::Auto,
        &mut observer2,
    )
    .expect("acceptance test 13: the second apply must converge");
    assert!(matches!(
        node_status(&second_applied, "pipeline", None),
        NodeStatus::Created
    ));

    // A third plan against the now-finished state shows NoOp/Unchanged/
    // Computed everywhere -- nothing left to do.
    let third_plan = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("acceptance test 13: third plan must succeed");
    for name in ["repo", "doppler", "pipeline"] {
        assert_eq!(
            find_planned(&third_plan, name, None).action,
            Action::NoOp,
            "acceptance test 13: `{name}` must be NoOp on the third plan"
        );
    }
    for env in ["dev", "stg", "prd"] {
        assert_eq!(
            find_planned(&third_plan, "configs", Some(env)).action,
            Action::NoOp,
            "acceptance test 13: configs[{env}] must be NoOp on the third plan"
        );
    }
}

// ---------------------------------------------------------------------
// acceptance test 14: a cluster that is gone
// ---------------------------------------------------------------------

#[test]
fn acceptance_14_plan_fails_at_buildkite_cluster_when_no_cluster_matches() {
    let workflow = load(&positive_fixture());
    // Deliberately not seeded: no cluster named "Default cluster" exists.
    let catalog = empty_catalog();
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("acceptance test 14: must check cleanly even though the cluster is absent");
    let inputs = positive_inputs("acceptance test 14", &checked);

    let err = willikins_core::plan(&checked, &inputs, &catalog)
        .expect_err("acceptance test 14: plan must fail when the cluster does not exist");
    match err {
        PlanError::Tool {
            node: failed_node,
            error,
        } => {
            assert_eq!(
                failed_node,
                node("buildkite_cluster"),
                "acceptance test 14: the failure must be at buildkite_cluster"
            );
            assert_eq!(
                error.kind,
                ToolErrorKind::NotFound,
                "acceptance test 14: the tool error must be NotFound, not a generic Provider error"
            );
            assert!(
                error.message.contains("Default cluster"),
                "acceptance test 14: the error must name the cluster it looked for: {}",
                error.message
            );
        }
        other => panic!("acceptance test 14: expected PlanError::Tool, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// three deliberately wrong documents: typing holds on the Buildkite
// surface, not only the milestone 1 one
// ---------------------------------------------------------------------

/// A secret output (a Doppler service token) bound to a non-secret sink
/// on `buildkite.pipeline.ensure`'s own `repo` port.
/// `workflows/fixtures/buildkite-secret-into-pipeline.yaml`.
#[test]
fn wrong_01_a_secret_output_bound_to_buildkites_non_secret_repo_port_is_rejected() {
    let workflow = load(&fixture("buildkite-secret-into-pipeline.yaml"));
    let catalog = empty_catalog();
    let errors = willikins_core::check(&workflow, &catalog).expect_err(
        "buildkite-secret-into-pipeline.yaml must fail check: a secret must not reach \
         a non-secret Buildkite port",
    );
    assert_eq!(
        errors,
        vec![CheckError::SecretToNonSecretSink {
            from: (node("token"), port("token")),
            to: port_site("pipeline", "repo"),
        }],
        "expected exactly one SecretToNonSecretSink"
    );
}

/// `buildkite.pipeline.ensure`'s `repo` port bound to a step that no node
/// in the document produces.
/// `workflows/fixtures/buildkite-pipeline-unbound-repo.yaml`.
#[test]
fn wrong_02_a_pipeline_bound_to_a_repository_no_node_produces_is_rejected() {
    let workflow = load(&fixture("buildkite-pipeline-unbound-repo.yaml"));
    let catalog = empty_catalog();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("buildkite-pipeline-unbound-repo.yaml must fail check: `repo` is unknown");
    assert_eq!(
        errors,
        vec![CheckError::UnknownNode {
            site: port_site("pipeline", "repo"),
            referenced: node("repo"),
        }],
        "expected exactly one UnknownNode"
    );
}

/// A type mismatch between a Doppler port (`doppler.project.ensure`'s
/// `project` output, `DopplerProject`) and a Buildkite port
/// (`buildkite.pipeline.ensure`'s `cluster`, `BuildkiteClusterId`).
/// `workflows/fixtures/buildkite-doppler-type-mismatch.yaml`.
#[test]
fn wrong_03_a_doppler_port_bound_to_a_buildkite_port_of_a_different_type_is_rejected() {
    let workflow = load(&fixture("buildkite-doppler-type-mismatch.yaml"));
    let catalog = empty_catalog();
    let errors = willikins_core::check(&workflow, &catalog).expect_err(
        "buildkite-doppler-type-mismatch.yaml must fail check: DopplerProject is not a \
         BuildkiteClusterId",
    );
    assert_eq!(
        errors,
        vec![CheckError::TypeMismatch {
            node: node("pipeline"),
            port: port("cluster"),
            expected: PortType::Exact(ty("BuildkiteClusterId")),
            found: ty("DopplerProject"),
        }],
        "expected exactly one TypeMismatch"
    );
}

// ---------------------------------------------------------------------
// for_each over environments, exercised explicitly rather than through
// the positive fixture's default three
// ---------------------------------------------------------------------

/// The positive fixture's `configs` node fans out over
/// `inputs.environments` (default `[dev, stg, prd]`), already exercised
/// by acceptance tests 12 and 13 above with all three default instances
/// checked individually. This test drives the same node with a
/// caller-supplied, differently-sized list (two environments, neither of
/// them a default), proving the fan-out is driven by the input rather
/// than hard-coded to three.
#[test]
fn for_each_fan_out_over_a_caller_supplied_environment_list() {
    let workflow = load(&positive_fixture());
    let catalog = seeded_catalog(&state_fixture("buildkite-cluster.json"));
    let checked = willikins_core::check(&workflow, &catalog).expect("must check cleanly");
    let inputs = resolve_inputs(
        "for_each fan-out",
        &checked,
        &[
            ("slug", RawInput::Scalar("third-thoughts".to_string())),
            ("org", RawInput::Scalar("lightless-labs".to_string())),
            (
                "buildkite_org",
                RawInput::Scalar("willikins-test".to_string()),
            ),
            (
                "environments",
                RawInput::List(vec!["qa".to_string(), "canary".to_string()]),
            ),
        ],
    );
    let planned = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("plan with a caller-supplied environment list must succeed");
    let config_instances: Vec<Option<String>> = planned
        .nodes
        .iter()
        .filter(|n| n.name == node("configs"))
        .map(|n| n.instance.clone())
        .collect();
    assert_eq!(
        config_instances,
        vec![Some("qa".to_string()), Some("canary".to_string())],
        "the for_each must expand to exactly the two supplied environments, in order"
    );

    let mut observer = willikins_core::NoopObserver;
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &planned,
        &Approval::Auto,
        &mut observer,
    )
    .expect("apply with a caller-supplied environment list must succeed");
    for env in ["qa", "canary"] {
        assert!(
            matches!(
                node_status(&applied, "configs", Some(env)),
                NodeStatus::Created
            ),
            "configs[{env}] (neither a default environment nor pre-seeded) must be Created"
        );
    }
}
