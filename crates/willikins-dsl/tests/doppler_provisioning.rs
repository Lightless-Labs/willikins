//! Graph tests for the four `doppler-*` provisioning documents under
//! `workflows/`: `doppler-project.yaml` (the base) and the three that
//! give an existing project what a kind of consumer needs via config
//! inheritance (`doppler-ios.yaml`, `doppler-backend.yaml`,
//! `doppler-backend-for-ios.yaml`).
//!
//! [`compose_project_then_ios`] and
//! [`compose_project_then_backend_for_ios_inherits_both_shared_bases`]
//! prove the documents' own claim: run `doppler-project.yaml`, then one of
//! the other three against the project it made, against one shared fake
//! state -- the same "run the first, then one of the other three" shape
//! the documents' own header comments describe. The remaining tests cover
//! `doppler-project.yaml` and `doppler-backend.yaml` each alone, and the
//! two error classes the fixtures under `workflows/fixtures/` are named
//! for.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::describe::{PartialInputs, RawInput};
use willikins_core::{
    Applied, Approval, CheckError, Checked, Class, InputName, NodeName, NodeStatus, PlanError,
    PortName, PortType, RecordingObserver, Site, TypeName, TypeRef, Value, Workflow, apply, check,
    plan,
};
use willikins_providers_fake::FakeState;
use willikins_types::{DomainType, DopplerConfig, DopplerProject, ParseError};

// ---------------------------------------------------------------------
// paths and loading
// ---------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn workflow_path(name: &str) -> PathBuf {
    workspace_root().join("workflows").join(name)
}

fn fixture_path(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join(name)
}

fn load(path: &Path) -> Workflow {
    willikins_dsl::load_document(path)
        .unwrap_or_else(|err| panic!("{}: failed to load: {err}", path.display()))
}

// ---------------------------------------------------------------------
// small identifier constructors
// ---------------------------------------------------------------------

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn input(name: &str) -> InputName {
    InputName::parse(name).unwrap()
}

fn ty(name: &str) -> TypeRef {
    TypeRef::scalar(TypeName::parse(name).unwrap())
}

fn list_ty(name: &str) -> TypeRef {
    TypeRef::list_of(TypeName::parse(name).unwrap())
}

fn config(text: &str) -> DopplerConfig {
    DopplerConfig::parse(text).unwrap()
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

// ---------------------------------------------------------------------
// applied-node lookup, mirroring willikins-cli's acceptance suite
// ---------------------------------------------------------------------

fn node_status<'a>(applied: &'a Applied, name: &str, instance: Option<&str>) -> &'a NodeStatus {
    &applied
        .nodes
        .iter()
        .find(|n| n.name == node(name) && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("no applied node `{name}` (instance {instance:?})"))
        .status
}

// ---------------------------------------------------------------------
// doppler-project.yaml alone, against empty state
// ---------------------------------------------------------------------

#[test]
fn doppler_project_creates_a_project_and_its_default_configs() {
    let workflow = load(&workflow_path("doppler-project.yaml"));
    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("doppler-project.yaml must check cleanly: {errors:?}"));
    assert_eq!(checked.class, Class::Reversible);
    assert!(checked.warnings.is_empty());

    let inputs = resolve_inputs(
        "doppler-project",
        &checked,
        &[(
            "project",
            RawInput::Scalar("lightless-labs-widgets".to_string()),
        )],
    );
    let approved = plan(&checked, &inputs, &catalog)
        .expect("doppler-project.yaml must plan cleanly against empty state");
    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("doppler-project.yaml must apply cleanly against empty state");

    assert!(matches!(
        node_status(&applied, "doppler", None),
        NodeStatus::Created
    ));
    for env in ["dev", "stg", "prd"] {
        assert!(
            matches!(
                node_status(&applied, "configs", Some(env)),
                NodeStatus::Unchanged
            ),
            "configs[{env}] must be Unchanged: doppler.project.ensure already seeded it"
        );
    }

    let project = applied
        .outputs
        .get(&willikins_core::OutputName::parse("project").unwrap())
        .expect("`project` output must be resolved");
    assert_eq!(
        project.render().to_string(),
        "lightless-labs-widgets",
        "the output is the project's own name, which is also its id"
    );
}

/// A fake state seeded exactly the way the documents' own headers say the
/// operator must set things up by hand first: the shared base config(s)
/// present and marked inheritable. Nothing about the *real* project is
/// seeded -- that is what `doppler-project.yaml`'s own run below creates.
fn state_with_shared_bases(bases: &[DopplerConfig]) -> Arc<Mutex<FakeState>> {
    let mut state = FakeState::new();
    for base in bases {
        state = state
            .with_doppler_config(base)
            .with_doppler_config_inheritable(base);
    }
    Arc::new(Mutex::new(state))
}

// ---------------------------------------------------------------------
// doppler-backend.yaml alone, against a project that already exists
// (the same "existing project" shape rotate-service-token.yaml exercises)
// ---------------------------------------------------------------------

#[test]
fn doppler_backend_alone_inherits_the_shared_backend_base() {
    let backend_base = config("lightless-labs-shared/backend_base");
    let project = DopplerProject::parse("ledger").unwrap();
    let state = Arc::new(Mutex::new(
        FakeState::new()
            .with_doppler_config(&backend_base)
            .with_doppler_config_inheritable(&backend_base)
            .with_doppler_project(&project, true),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));

    let workflow = load(&workflow_path("doppler-backend.yaml"));
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("doppler-backend.yaml must check cleanly: {errors:?}"));
    assert_eq!(checked.class, Class::Reversible);

    let inputs = resolve_inputs(
        "doppler-backend",
        &checked,
        &[("project", RawInput::Scalar("ledger".to_string()))],
    );
    let approved =
        plan(&checked, &inputs, &catalog).expect("doppler-backend.yaml must plan cleanly");
    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("doppler-backend.yaml must apply cleanly");

    for env in ["dev", "stg", "prd"] {
        assert!(
            matches!(
                node_status(&applied, "configs", Some(env)),
                NodeStatus::Created
            ),
            "configs[{env}] must be Created: the project exists but this env's config does not yet"
        );
        let instance = format!("ledger/{env}");
        assert!(
            matches!(
                node_status(&applied, "inherit", Some(instance.as_str())),
                NodeStatus::Created
            ),
            "inherit[{instance}] must be Created: nothing inherits the shared backend base yet"
        );
    }

    let locked = state.lock().unwrap();
    for env in ["dev", "stg", "prd"] {
        let key = format!("ledger/{env}");
        let inherits = locked
            .doppler_config_inherits
            .get(&key)
            .unwrap_or_else(|| panic!("{key} must now record an inherits set"));
        assert_eq!(
            inherits,
            &std::collections::HashSet::from(["lightless-labs-shared/backend_base".to_string()])
        );
    }
}

// ---------------------------------------------------------------------
// composition: run doppler-project.yaml, then doppler-ios.yaml against
// the project it made, over one shared fake state
// ---------------------------------------------------------------------

#[test]
fn compose_project_then_ios() {
    let ios_base = config("lightless-labs-shared/ios_base");
    let state = state_with_shared_bases(&[ios_base]);
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));

    // First: doppler-project.yaml creates the project.
    let project_workflow = load(&workflow_path("doppler-project.yaml"));
    let project_checked = check(&project_workflow, &catalog)
        .unwrap_or_else(|errors| panic!("doppler-project.yaml must check cleanly: {errors:?}"));
    let project_inputs = resolve_inputs(
        "doppler-project (compose)",
        &project_checked,
        &[("project", RawInput::Scalar("widgets".to_string()))],
    );
    let project_plan = plan(&project_checked, &project_inputs, &catalog)
        .expect("doppler-project.yaml must plan cleanly");
    let mut observer = RecordingObserver::new();
    let project_applied = apply(
        &project_checked,
        &project_inputs,
        &catalog,
        &project_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect("doppler-project.yaml must apply cleanly");
    assert!(matches!(
        node_status(&project_applied, "doppler", None),
        NodeStatus::Created
    ));

    // Second: doppler-ios.yaml, against the project the first run made,
    // over the SAME fake state.
    let ios_workflow = load(&workflow_path("doppler-ios.yaml"));
    let ios_checked = check(&ios_workflow, &catalog)
        .unwrap_or_else(|errors| panic!("doppler-ios.yaml must check cleanly: {errors:?}"));
    let ios_inputs = resolve_inputs(
        "doppler-ios (compose)",
        &ios_checked,
        &[("project", RawInput::Scalar("widgets".to_string()))],
    );
    let ios_plan = plan(&ios_checked, &ios_inputs, &catalog)
        .expect("doppler-ios.yaml must plan cleanly against the project doppler-project made");
    let mut observer = RecordingObserver::new();
    let ios_applied = apply(
        &ios_checked,
        &ios_inputs,
        &catalog,
        &ios_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect("doppler-ios.yaml must apply cleanly");

    for env in ["dev", "stg", "prd"] {
        assert!(
            matches!(
                node_status(&ios_applied, "configs", Some(env)),
                NodeStatus::Unchanged
            ),
            "configs[{env}] must be Unchanged: doppler-project.yaml's run already made it"
        );
        let instance = format!("widgets/{env}");
        assert!(
            matches!(
                node_status(&ios_applied, "inherit", Some(instance.as_str())),
                NodeStatus::Created
            ),
            "inherit[{instance}] must be Created: the shared iOS base was not yet inherited"
        );
    }

    let locked = state.lock().unwrap();
    for env in ["dev", "stg", "prd"] {
        let key = format!("widgets/{env}");
        let inherits = locked
            .doppler_config_inherits
            .get(&key)
            .unwrap_or_else(|| panic!("{key} must now record an inherits set"));
        assert_eq!(
            inherits,
            &std::collections::HashSet::from(["lightless-labs-shared/ios_base".to_string()])
        );
    }
}

#[test]
fn compose_project_then_backend_for_ios_inherits_both_shared_bases() {
    let ios_base = config("lightless-labs-shared/ios_base");
    let backend_base = config("lightless-labs-shared/backend_base");
    let state = state_with_shared_bases(&[ios_base, backend_base]);
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));

    let project_workflow = load(&workflow_path("doppler-project.yaml"));
    let project_checked = check(&project_workflow, &catalog)
        .unwrap_or_else(|errors| panic!("doppler-project.yaml must check cleanly: {errors:?}"));
    let project_inputs = resolve_inputs(
        "doppler-project (compose bfi)",
        &project_checked,
        &[("project", RawInput::Scalar("cortex".to_string()))],
    );
    let project_plan = plan(&project_checked, &project_inputs, &catalog)
        .expect("doppler-project.yaml must plan cleanly");
    let mut observer = RecordingObserver::new();
    let project_applied = apply(
        &project_checked,
        &project_inputs,
        &catalog,
        &project_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect("doppler-project.yaml must apply cleanly");
    assert!(matches!(
        node_status(&project_applied, "doppler", None),
        NodeStatus::Created
    ));

    let bfi_workflow = load(&workflow_path("doppler-backend-for-ios.yaml"));
    let bfi_checked = check(&bfi_workflow, &catalog).unwrap_or_else(|errors| {
        panic!("doppler-backend-for-ios.yaml must check cleanly: {errors:?}")
    });
    let bfi_inputs = resolve_inputs(
        "doppler-backend-for-ios (compose)",
        &bfi_checked,
        &[("project", RawInput::Scalar("cortex".to_string()))],
    );
    let bfi_plan = plan(&bfi_checked, &bfi_inputs, &catalog)
        .expect("doppler-backend-for-ios.yaml must plan cleanly");
    let mut observer = RecordingObserver::new();
    let bfi_applied = apply(
        &bfi_checked,
        &bfi_inputs,
        &catalog,
        &bfi_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect("doppler-backend-for-ios.yaml must apply cleanly");

    for env in ["dev", "stg", "prd"] {
        let instance = format!("cortex/{env}");
        assert!(matches!(
            node_status(&bfi_applied, "inherit", Some(instance.as_str())),
            NodeStatus::Created
        ));
    }

    let locked = state.lock().unwrap();
    let wanted: std::collections::HashSet<String> = [
        "lightless-labs-shared/ios_base".to_string(),
        "lightless-labs-shared/backend_base".to_string(),
    ]
    .into_iter()
    .collect();
    for env in ["dev", "stg", "prd"] {
        let key = format!("cortex/{env}");
        let inherits = locked
            .doppler_config_inherits
            .get(&key)
            .unwrap_or_else(|| panic!("{key} must now record an inherits set"));
        assert_eq!(inherits, &wanted);
    }
}

// ---------------------------------------------------------------------
// plan-time guard: a config that already inherits something this
// document was not asked for
// ---------------------------------------------------------------------

#[test]
fn doppler_ios_plan_refuses_a_config_that_already_inherits_an_extra() {
    let ios_base = config("lightless-labs-shared/ios_base");
    let project = DopplerProject::parse("widgets").unwrap();
    let prd = config("widgets/prd");
    let hand_wired = config("shared-apple/hand_wired_by_a_human");

    let state = Arc::new(Mutex::new(
        FakeState::new()
            .with_doppler_config(&ios_base)
            .with_doppler_config_inheritable(&ios_base)
            .with_doppler_project(&project, true)
            .with_doppler_config(&prd)
            // Seeded as if a human had wired `prd` up to inherit something
            // this document's own `base_configs` does not name.
            .with_doppler_config_inherits(&prd, &[hand_wired]),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));

    let workflow = load(&workflow_path("doppler-ios.yaml"));
    let checked = check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("doppler-ios.yaml must check cleanly: {errors:?}"));
    let inputs = resolve_inputs(
        "doppler-ios (mismatch)",
        &checked,
        &[("project", RawInput::Scalar("widgets".to_string()))],
    );

    let err = plan(&checked, &inputs, &catalog)
        .expect_err("plan must refuse rather than silently drop the hand-wired extra");
    match err {
        PlanError::AttributeMismatch { site } => {
            assert_eq!(
                site,
                Site::Port {
                    node: node("inherit"),
                    port: port("inherits"),
                }
            );
        }
        other => panic!("expected AttributeMismatch, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// error class 1: a bare string bound to inherits (list<DopplerConfig>)
// ---------------------------------------------------------------------

#[test]
fn inherits_literal_list_is_rejected_by_check() {
    let workflow = load(&fixture_path("inherits-literal-list.yaml"));
    let (_state, catalog) = willikins_providers_fake::empty();
    let errors =
        check(&workflow, &catalog).expect_err("a bare string on a list port must fail check");
    assert_eq!(errors.len(), 1, "expected exactly one error: {errors:?}");
    match &errors[0] {
        CheckError::InvalidLiteral {
            node: bad_node,
            port: bad_port,
            error,
        } => {
            assert_eq!(*bad_node, node("inherit"));
            assert_eq!(*bad_port, port("inherits"));
            assert_eq!(
                error,
                &ParseError::new("Binding", "lists cannot be literals")
            );
        }
        other => panic!("expected InvalidLiteral, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// error class 2: a for_each node's aggregated list into a scalar port
// ---------------------------------------------------------------------

#[test]
fn config_list_into_scalar_port_is_rejected_by_check() {
    let workflow = load(&fixture_path("config-list-into-scalar-port.yaml"));
    let (_state, catalog) = willikins_providers_fake::empty();
    let errors = check(&workflow, &catalog)
        .expect_err("a for_each node's own list output on a scalar port must fail check");
    assert_eq!(
        errors,
        vec![CheckError::TypeMismatch {
            node: node("inherit"),
            port: port("config"),
            expected: PortType::Exact(ty("DopplerConfig")),
            found: list_ty("DopplerConfig"),
        }]
    );
}
