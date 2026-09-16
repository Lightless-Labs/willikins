//! The milestone's acceptance suite: one `#[test]` per acceptance test in
//! `docs/plans/2026-09-11-milestone-1-core.md`'s "Acceptance tests"
//! section, numbered 1 through 11, driving the library API
//! (`willikins_dsl::load_document`/`parse_document`,
//! `willikins_core::{check, describe, plan}`, `willikins_providers_fake`)
//! against real YAML documents under `workflows/` and
//! `workflows/fixtures/` — the DSL parser did not exist when
//! `willikins-core`'s own integration tests (`crates/willikins-core/tests/`)
//! were written, so those build every workflow by hand with the
//! `Workflow` builder API; this suite instead proves the same behaviours
//! hold for an actual parsed document, end to end.
//!
//! Every fixture under `workflows/fixtures/` used here carries a header
//! comment naming the acceptance test it covers and the exact
//! [`willikins_core::CheckError`] (or, for test 4's positive half,
//! resolved type) it is expected to produce.
//!
//! Tests 9, 10, and 11 are already fully covered elsewhere — golden and
//! property tests in `willikins-types` for 9, a `ProjectSlug`-level test
//! named after the acceptance test itself for 10, and a `trybuild` suite
//! for 11 — so their functions here are thin: a comment pointing at the
//! authoritative coverage, plus only what the plan's own wording asks for
//! beyond it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::describe::{PartialInputs, RawInput};
use willikins_core::{
    Action, Applied, ApplyError, ApplyEvent, Approval, Catalog, CheckError, Class, InputName,
    Inputs, NodeName, NodeStatus, Observation, OutputName, Outputs, Plan, PlanError, PlannedNode,
    PortName, PortType, RecordingObserver, Site, ToolError, ToolErrorKind, ToolName, TypeName,
    TypeRef, Value, Workflow, apply,
};
use willikins_providers_fake::FakeState;
use willikins_types::DomainType;

/// A `DopplerServiceToken`-shaped literal (real shape does not matter here:
/// the registry refuses every secret literal before looking at its text).
/// `concat!`-split so this file holds no literal spelling it contiguously.
/// Used by [`acceptance_03_static_errors`]'s last check.
const SECRET_LITERAL_TOKEN: &str =
    concat!("dp.st.prd.", "hunter2hunter2hunter2hunter2hunter2hunter2");

// ---------------------------------------------------------------------
// paths and loading
// ---------------------------------------------------------------------

/// The workspace root: two levels up from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// A fixture under `workflows/fixtures/`.
fn fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join(name)
}

/// A fake-state JSON file under `workflows/fixtures/state/`.
fn state_fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join(name)
}

/// The milestone's positive fixture, `workflows/new-rust-service.yaml`.
fn positive_fixture() -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("new-rust-service.yaml")
}

/// Load and parse a workflow document, panicking with the [`willikins_dsl::DocumentError`]
/// on failure — every fixture this suite loads is expected to parse
/// cleanly; a parse failure is a bug in the fixture or the DSL, not a case
/// under test.
fn load(path: &Path) -> Workflow {
    willikins_dsl::load_document(path)
        .unwrap_or_else(|err| panic!("{}: failed to load: {err}", path.display()))
}

// ---------------------------------------------------------------------
// small identifier constructors, mirroring `willikins-core/tests/common`
// ---------------------------------------------------------------------

fn ty(name: &str) -> TypeRef {
    TypeRef::scalar(TypeName::parse(name).unwrap())
}

fn list_ty(name: &str) -> TypeRef {
    TypeRef::list_of(TypeName::parse(name).unwrap())
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

fn output(name: &str) -> OutputName {
    OutputName::parse(name).unwrap()
}

fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).unwrap()
}

/// A [`Site::Port`] from two raw names, so an expected error stays on one
/// line where it used to carry a bare `node`/`port` pair.
fn port_site(node_name: &str, port_name: &str) -> Site {
    Site::Port {
        node: node(node_name),
        port: port(port_name),
    }
}

// ---------------------------------------------------------------------
// catalogs
// ---------------------------------------------------------------------

/// A catalog of every fake tool against empty state.
fn empty_catalog() -> Catalog {
    willikins_providers_fake::empty().1
}

/// A catalog of every fake tool against the state seeded by the JSON file
/// at `path`.
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

// ---------------------------------------------------------------------
// input resolution
// ---------------------------------------------------------------------

/// Resolve `overrides` (name, raw value) pairs against `checked`'s
/// declared inputs via `describe`, asserting nothing is missing or
/// rejected, and returning the fully resolved input map `plan` accepts.
/// `label` (an acceptance test number, e.g. `"acceptance test 6"`) names
/// the caller in a failed assertion, since this helper is shared across
/// several acceptance tests.
fn resolve_inputs(
    label: &str,
    checked: &willikins_core::Checked,
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

/// [`resolve_inputs`] for the positive fixture's two required inputs.
fn positive_inputs(label: &str, checked: &willikins_core::Checked) -> IndexMap<InputName, Value> {
    resolve_inputs(
        label,
        checked,
        &[
            ("slug", RawInput::Scalar("third-thoughts".to_string())),
            ("org", RawInput::Scalar("lightless-labs".to_string())),
        ],
    )
}

// ---------------------------------------------------------------------
// CLI invocation, for acceptance test 8b's text-and-JSON assertion
// ---------------------------------------------------------------------

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
        .output()
        .expect("failed to run the willikins binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("process was not signalled")
}

// ---------------------------------------------------------------------
// acceptance test 1: taint rejection
// ---------------------------------------------------------------------

#[test]
fn acceptance_01_taint_rejection() {
    let workflow = load(&fixture("secret-into-template.yaml"));
    let catalog = empty_catalog();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("acceptance test 1: secret-into-template.yaml must fail check");
    assert_eq!(
        errors,
        vec![CheckError::SecretToNonSecretSink {
            from: (node("token"), port("token")),
            to: port_site("readme", "value"),
        }],
        "acceptance test 1: expected exactly one SecretToNonSecretSink"
    );
}

// ---------------------------------------------------------------------
// acceptance test 2: secret workflow input
// ---------------------------------------------------------------------

#[test]
fn acceptance_02_secret_workflow_input() {
    let workflow = load(&fixture("secret-input.yaml"));
    let catalog = empty_catalog();
    let errors = willikins_core::check(&workflow, &catalog)
        .expect_err("acceptance test 2: secret-input.yaml must fail check");
    assert_eq!(
        errors,
        vec![CheckError::SecretWorkflowInput {
            input: input("token"),
            ty: ty("DopplerServiceToken"),
        }],
        "acceptance test 2: expected exactly one SecretWorkflowInput"
    );
}

// ---------------------------------------------------------------------
// acceptance test 3: static errors
// ---------------------------------------------------------------------

#[test]
fn acceptance_03_static_errors() {
    let catalog = empty_catalog();

    let unbound = load(&fixture("unbound-input.yaml"));
    assert_eq!(
        willikins_core::check(&unbound, &catalog).unwrap_err(),
        vec![CheckError::UnboundInput {
            node: node("repo"),
            port: port("repo"),
        }],
        "acceptance test 3: UnboundInput"
    );

    let undeclared = load(&fixture("undeclared-input.yaml"));
    assert_eq!(
        willikins_core::check(&undeclared, &catalog).unwrap_err(),
        vec![CheckError::UndeclaredInput {
            site: port_site("repo", "repo"),
            input: input("slug"),
        }],
        "acceptance test 3: UndeclaredInput"
    );

    let invalid_literal = load(&fixture("invalid-literal.yaml"));
    let errors = willikins_core::check(&invalid_literal, &catalog).unwrap_err();
    assert_eq!(
        errors.len(),
        1,
        "acceptance test 3: InvalidLiteral: {errors:?}"
    );
    match &errors[0] {
        CheckError::InvalidLiteral {
            node: n,
            port: p,
            error,
        } => {
            assert_eq!(n, &node("repo"), "acceptance test 3: InvalidLiteral node");
            assert_eq!(
                p,
                &port("visibility"),
                "acceptance test 3: InvalidLiteral port"
            );
            assert!(
                error.reason.contains("internal"),
                "acceptance test 3: InvalidLiteral error should name the offending literal: {}",
                error.reason
            );
        }
        other => panic!("acceptance test 3: expected InvalidLiteral, got {other:?}"),
    }

    let type_mismatch = load(&fixture("type-mismatch.yaml"));
    assert_eq!(
        willikins_core::check(&type_mismatch, &catalog).unwrap_err(),
        vec![CheckError::TypeMismatch {
            node: node("repo"),
            port: port("repo"),
            expected: PortType::Exact(ty("GitHubRepo")),
            found: ty("ProjectSlug"),
        }],
        "acceptance test 3: TypeMismatch"
    );

    let secret_literal = load(&fixture("secret-literal.yaml"));
    assert_eq!(
        willikins_core::check(&secret_literal, &catalog).unwrap_err(),
        vec![CheckError::SecretLiteral {
            node: node("ci_secret"),
            port: port("value"),
        }],
        "acceptance test 3: SecretLiteral"
    );

    let unknown_tool = load(&fixture("unknown-tool.yaml"));
    assert_eq!(
        willikins_core::check(&unknown_tool, &catalog).unwrap_err(),
        vec![CheckError::UnknownTool {
            node: node("mystery"),
            tool: tool_name("no.such.tool"),
        }],
        "acceptance test 3: UnknownTool"
    );

    let unknown_port = load(&fixture("unknown-port.yaml"));
    assert_eq!(
        willikins_core::check(&unknown_port, &catalog).unwrap_err(),
        vec![CheckError::UnknownPort {
            site: port_site("names", "bogus"),
            tool: tool_name("naming.v1"),
        }],
        "acceptance test 3: UnknownPort"
    );

    let cycle = load(&fixture("cycle.yaml"));
    assert_eq!(
        willikins_core::check(&cycle, &catalog).unwrap_err(),
        vec![CheckError::Cycle {
            nodes: vec![node("a")],
        }],
        "acceptance test 3: Cycle"
    );

    // "The registry also refuses `--input token=dp.st...` for a secret
    // type": the mechanism is `Value::parse` against the type registry,
    // the exact call `describe`'s own raw-input resolution (and so the
    // CLI's `--input` flag) makes. Already pinned the same way by
    // `willikins-core/tests/check.rs`'s
    // `acceptance_3_the_registry_refuses_a_secret_literal_input_value`;
    // re-verified here since it is this acceptance test's own last
    // sentence.
    let err = Value::parse(&ty("DopplerServiceToken"), SECRET_LITERAL_TOKEN).unwrap_err();
    assert!(
        err.reason.contains("cannot be supplied"),
        "acceptance test 3: registry must refuse a secret literal: {}",
        err.reason
    );
}

// ---------------------------------------------------------------------
// acceptance test 4: for_each
// ---------------------------------------------------------------------

#[test]
fn acceptance_04_for_each() {
    let catalog = empty_catalog();

    let secret_source = load(&fixture("secret-for-each.yaml"));
    assert_eq!(
        willikins_core::check(&secret_source, &catalog).unwrap_err(),
        vec![CheckError::SecretForEachSource { node: node("loop") }],
        "acceptance test 4: SecretForEachSource"
    );

    let scalar_source = load(&fixture("for-each-over-scalar.yaml"));
    assert_eq!(
        willikins_core::check(&scalar_source, &catalog).unwrap_err(),
        vec![CheckError::ForEachOverScalar { node: node("loop") }],
        "acceptance test 4: ForEachOverScalar"
    );

    let item_outside = load(&fixture("item-outside-for-each.yaml"));
    assert_eq!(
        willikins_core::check(&item_outside, &catalog).unwrap_err(),
        vec![CheckError::ItemOutsideForEach {
            site: port_site("config", "project"),
        }],
        "acceptance test 4: ItemOutsideForEach"
    );

    let keyed_on_scalar = load(&fixture("keyed-on-scalar-node.yaml"));
    assert_eq!(
        willikins_core::check(&keyed_on_scalar, &catalog).unwrap_err(),
        vec![CheckError::KeyedOnScalarNode {
            site: port_site("token", "config"),
            referenced: node("doppler"),
        }],
        "acceptance test 4: KeyedOnScalarNode"
    );

    // "On the positive fixture, `steps.configs[prd].config` type-checks
    // as `DopplerConfig`": the `Keyed` half is literally present in the
    // positive fixture at `token.config`, so it is checked against the
    // real, checked-in document.
    let positive = load(&positive_fixture());
    let positive_checked = willikins_core::check(&positive, &catalog)
        .expect("acceptance test 4: the positive fixture must check cleanly");
    assert_eq!(
        positive_checked
            .types
            .get(&node("token"))
            .and_then(|ports| ports.get(&port("config"))),
        Some(&ty("DopplerConfig")),
        "acceptance test 4: steps.configs[prd].config type-checks as DopplerConfig"
    );

    // "... and `steps.configs.config` as `list<DopplerConfig>`": no plain
    // `Step` aggregate reference into a `for_each` node's port appears
    // anywhere in the checked-in positive fixture (it only ever uses the
    // `Keyed` form above) — a plan defect (nothing to fix, since the
    // fixture is fine as written; noted in the task report). Proven
    // instead against a small inline document built the same way.
    let aggregate_source = "\
name: for-each-aggregate-type
inputs:
  project: { type: DopplerProject }
  environments: { type: list<EnvironmentSlug>, default: [dev, stg, prd] }
steps:
  configs:
    tool: doppler.config.ensure
    for_each: ${{ inputs.environments }}
    with:
      project: ${{ inputs.project }}
      environment: ${{ item }}
outputs:
  all_configs: ${{ steps.configs.config }}
  one_config: ${{ steps.configs[prd].config }}
";
    let aggregate_workflow = willikins_dsl::parse_document(aggregate_source)
        .expect("acceptance test 4: the inline for_each aggregate document must parse");
    let checked = willikins_core::check(&aggregate_workflow, &catalog)
        .expect("acceptance test 4: the inline for_each aggregate document must check cleanly");
    assert_eq!(
        checked.output_types.get(&output("one_config")),
        Some(&ty("DopplerConfig")),
        "acceptance test 4: a Keyed reference resolves to the scalar type"
    );
    assert_eq!(
        checked.output_types.get(&output("all_configs")),
        Some(&list_ty("DopplerConfig")),
        "acceptance test 4: a plain Step reference into a for_each node resolves to a list"
    );
}

// ---------------------------------------------------------------------
// acceptance test 5: describe loop
// ---------------------------------------------------------------------

#[test]
fn acceptance_05_describe_loop() {
    let workflow = load(&positive_fixture());
    let catalog = empty_catalog();
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("acceptance test 5: the positive fixture must check cleanly");

    // With no inputs at all: slug and org are missing (each with a
    // prompt), visibility and environments are not (both have defaults).
    let empty_partial = PartialInputs::new();
    let description = willikins_core::describe(&checked, &empty_partial);
    assert!(
        description.errors.is_empty(),
        "acceptance test 5: {:?}",
        description.errors
    );
    let missing_names: Vec<&str> = description
        .missing
        .iter()
        .map(|missing| missing.name.as_str())
        .collect();
    assert_eq!(
        missing_names,
        vec!["slug", "org"],
        "acceptance test 5: exactly slug and org are missing"
    );
    for missing in &description.missing {
        assert!(
            !missing.prompt.is_empty(),
            "acceptance test 5: `{}` needs a prompt",
            missing.name
        );
    }
    assert!(
        !description.resolved.contains_key(&input("slug")),
        "acceptance test 5: slug has no default, so it must not resolve"
    );
    assert!(
        !description.resolved.contains_key(&input("org")),
        "acceptance test 5: org has no default, so it must not resolve"
    );
    assert_eq!(
        description.resolved.get(&input("visibility")),
        Some(&Value::known(willikins_types::RepoVisibility::Private)),
        "acceptance test 5: visibility resolves to its default"
    );
    assert_eq!(
        description.resolved.get(&input("environments")),
        Some(&Value::known_list(vec![
            willikins_types::EnvironmentSlug::parse("dev").unwrap(),
            willikins_types::EnvironmentSlug::parse("stg").unwrap(),
            willikins_types::EnvironmentSlug::parse("prd").unwrap(),
        ])),
        "acceptance test 5: environments resolves to its default list"
    );

    // With slug and org supplied: nothing missing, everything resolved
    // into the expected domain types.
    let full = positive_inputs("acceptance test 5", &checked);
    assert_eq!(
        full.len(),
        4,
        "acceptance test 5: all four declared inputs must resolve: {full:?}"
    );
    assert_eq!(
        full.get(&input("slug")),
        Some(&Value::known(
            willikins_types::ProjectSlug::parse("third-thoughts").unwrap()
        )),
        "acceptance test 5: slug resolves into a ProjectSlug"
    );
    assert_eq!(
        full.get(&input("org")),
        Some(&Value::known(
            willikins_types::GitHubOrg::parse("lightless-labs").unwrap()
        )),
        "acceptance test 5: org resolves into a GitHubOrg"
    );
}

// ---------------------------------------------------------------------
// acceptance test 6: plan against empty state
// ---------------------------------------------------------------------

fn find_planned<'a>(plan: &'a Plan, name: &str, instance: Option<&str>) -> &'a PlannedNode {
    plan.nodes
        .iter()
        .find(|planned| planned.name == node(name) && planned.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("no planned node `{name}` (instance {instance:?})"))
}

/// The [`NodeStatus`] of one attempted node instance in an [`Applied`]
/// result, shared by every task 4 acceptance test below. Panics (with the
/// instance named) if `applied` has no such node — an absent instance
/// (blocked by [`ApplyError::UnknownInput`] before it ever started) is a
/// distinct case tests check for separately, never confused with a
/// present one.
fn node_status<'a>(applied: &'a Applied, name: &str, instance: Option<&str>) -> &'a NodeStatus {
    &applied
        .nodes
        .iter()
        .find(|n| n.name == node(name) && n.instance.as_deref() == instance)
        .unwrap_or_else(|| panic!("no applied node `{name}` (instance {instance:?})"))
        .status
}

/// The path to the second positive fixture, `workflows/rotate-service-token.yaml`.
fn rotate_fixture() -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("rotate-service-token.yaml")
}

#[test]
fn acceptance_06_plan_against_empty_state() {
    let catalog = empty_catalog();
    let workflow = load(&positive_fixture());
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("acceptance test 6: the positive fixture must check cleanly");
    let inputs = positive_inputs("acceptance test 6", &checked);
    let result = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("acceptance test 6: plan against empty state must succeed");

    assert_eq!(
        find_planned(&result, "names", None).action,
        Action::Compute,
        "acceptance test 6: names is Compute"
    );
    assert_eq!(
        find_planned(&result, "repo", None).action,
        Action::Create,
        "acceptance test 6: repo is Create"
    );
    assert_eq!(
        find_planned(&result, "doppler", None).action,
        Action::Create,
        "acceptance test 6: doppler is Create"
    );
    assert_eq!(
        find_planned(&result, "token", None).action,
        Action::Create,
        "acceptance test 6: token is Create"
    );
    assert_eq!(
        find_planned(&result, "ci_secret", None).action,
        Action::Create,
        "acceptance test 6: ci_secret is Create"
    );
    for key in ["dev", "stg", "prd"] {
        assert_eq!(
            find_planned(&result, "configs", Some(key)).action,
            Action::Create,
            "acceptance test 6: configs[{key}]"
        );
    }
    assert_eq!(
        result.nodes.len(),
        8,
        "acceptance test 6: names, repo, doppler, configs x3, token, ci_secret"
    );

    // token's output is Unknown; ci_secret's value input (bound to it) is
    // Unknown too.
    let token_node = find_planned(&result, "token", None);
    assert!(
        !token_node
            .outputs
            .get(&port("token"))
            .expect("token output port exists")
            .is_known(),
        "acceptance test 6: token's output must be Unknown"
    );
    let ci_secret_node = find_planned(&result, "ci_secret", None);
    assert!(
        !ci_secret_node
            .inputs
            .get(&port("value"))
            .expect("value input port exists")
            .is_known(),
        "acceptance test 6: ci_secret's value must be Unknown"
    );

    // repo's url is the derived https:// URL.
    let repo_node = find_planned(&result, "repo", None);
    assert_eq!(
        repo_node.outputs.get(&port("url")),
        Some(&Value::known(
            willikins_types::HttpsUrl::parse("https://github.com/lightless-labs/third-thoughts")
                .unwrap()
        )),
        "acceptance test 6: repo's url"
    );

    assert_eq!(result.class, Class::Reversible, "acceptance test 6: class");
    assert!(
        !result.requires_approval,
        "acceptance test 6: requires_approval"
    );

    // Second half: the positive fixture plus one fake.irreversible.ensure
    // node yields Irreversible / true.
    let irreversible_workflow = load(&fixture("irreversible.yaml"));
    let irreversible_checked = willikins_core::check(&irreversible_workflow, &catalog)
        .expect("acceptance test 6: irreversible.yaml must check cleanly");
    let irreversible_inputs = resolve_inputs(
        "acceptance test 6",
        &irreversible_checked,
        &[
            ("slug", RawInput::Scalar("third-thoughts".to_string())),
            ("org", RawInput::Scalar("lightless-labs".to_string())),
        ],
    );
    let irreversible_plan =
        willikins_core::plan(&irreversible_checked, &irreversible_inputs, &catalog)
            .expect("acceptance test 6: irreversible.yaml plan must succeed");
    assert_eq!(
        irreversible_plan.class,
        Class::Irreversible,
        "acceptance test 6: an irreversible node bumps the class"
    );
    assert!(
        irreversible_plan.requires_approval,
        "acceptance test 6: an irreversible plan requires approval"
    );
}

// ---------------------------------------------------------------------
// acceptance test 7: plan against seeded state
// ---------------------------------------------------------------------

#[test]
fn acceptance_07_plan_against_seeded_state() {
    let workflow = load(&positive_fixture());

    // Repo exists and is ours: NoOp, url is Known.
    let ours_catalog = seeded_catalog(&state_fixture("repo-ours.json"));
    let ours_checked = willikins_core::check(&workflow, &ours_catalog)
        .expect("acceptance test 7: must check cleanly against the ours-seeded catalog");
    let ours_inputs = positive_inputs("acceptance test 7", &ours_checked);
    let ours_plan = willikins_core::plan(&ours_checked, &ours_inputs, &ours_catalog)
        .expect("acceptance test 7: plan against an owned repo must succeed");
    let repo_node = find_planned(&ours_plan, "repo", None);
    assert_eq!(
        repo_node.action,
        Action::NoOp,
        "acceptance test 7: an owned, seeded repo is a no-op"
    );
    assert_eq!(
        repo_node.outputs.get(&port("url")),
        Some(&Value::known(
            willikins_types::HttpsUrl::parse("https://github.com/lightless-labs/third-thoughts")
                .unwrap()
        )),
        "acceptance test 7: the no-op repo's url is still Known"
    );

    // Repo exists and is foreign: NameTaken.
    let foreign_catalog = seeded_catalog(&state_fixture("repo-foreign.json"));
    let foreign_checked = willikins_core::check(&workflow, &foreign_catalog)
        .expect("acceptance test 7: must check cleanly against the foreign-seeded catalog");
    let foreign_inputs = positive_inputs("acceptance test 7", &foreign_checked);
    let foreign_err = willikins_core::plan(&foreign_checked, &foreign_inputs, &foreign_catalog)
        .expect_err("acceptance test 7: a foreign repo must not plan");
    match foreign_err {
        PlanError::NameTaken { node: n, tool, key } => {
            assert_eq!(n, node("repo"), "acceptance test 7: NameTaken node");
            assert_eq!(
                tool,
                tool_name("github.repo.ensure"),
                "acceptance test 7: NameTaken tool"
            );
            // `key` is restricted to the tool's key ports only (`repo`),
            // never the full bound input set — `visibility` is bound too,
            // but is not a key port and must not appear here.
            assert_eq!(
                key.len(),
                1,
                "acceptance test 7: NameTaken key must hold only the key ports: {key:?}"
            );
            assert_eq!(
                key.get(&port("repo")),
                Some(&Value::known(
                    willikins_types::GitHubRepo::parse("lightless-labs/third-thoughts").unwrap()
                )),
                "acceptance test 7: NameTaken key's repo"
            );
            assert!(
                key.get(&port("visibility")).is_none(),
                "acceptance test 7: NameTaken key must not leak the non-key visibility port"
            );
        }
        other => panic!("acceptance test 7: expected NameTaken, got {other:?}"),
    }

    // environments=[dev, qa] with configs[prd] referenced: KeyNotInForEach
    // named at `token`.`config`, the referencing site, not `configs`.
    let catalog = empty_catalog();
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("acceptance test 7: must check cleanly against the empty catalog");
    let inputs = resolve_inputs(
        "acceptance test 7",
        &checked,
        &[
            ("slug", RawInput::Scalar("third-thoughts".to_string())),
            ("org", RawInput::Scalar("lightless-labs".to_string())),
            (
                "environments",
                RawInput::List(vec!["dev".to_string(), "qa".to_string()]),
            ),
        ],
    );
    let err = willikins_core::plan(&checked, &inputs, &catalog)
        .expect_err("acceptance test 7: `prd` is not in [dev, qa]");
    match err {
        PlanError::KeyNotInForEach { site, key } => {
            assert_eq!(
                site,
                port_site("token", "config"),
                "acceptance test 7: KeyNotInForEach site"
            );
            assert_eq!(key, "prd", "acceptance test 7: KeyNotInForEach key");
        }
        other => panic!("acceptance test 7: expected KeyNotInForEach, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// acceptance test 8: redaction by construction
// ---------------------------------------------------------------------

/// Acceptance test 8a: construct a secret [`Value`] directly and embed it
/// in every container the plan text names — [`Inputs`], [`Outputs`],
/// [`Observation::Present`], [`PlannedNode`], [`Plan`], and a
/// [`ToolError`] — asserting the literal bytes appear in none of
/// `serde_json::to_string`, `format!("{:?}")`, or `to_string()` of any of
/// them, and the redaction marker does. This does not depend on any
/// provider state.
///
/// `crates/willikins-core/tests/redaction.rs` already pins this same
/// guarantee for `Inputs`, `Outputs`, `Observation`, and a `ToolError`
/// built from a deliberately careless tool; this test adds the
/// `PlannedNode`/`Plan` embedding the plan text also names, and the
/// `to_string()` case (a `ToolError`'s own `Display`).
#[test]
fn acceptance_08a_redaction_by_construction() {
    const SECRET_TAIL: &str = "acceptance08afakesecretbytesaaaaaaaaaaaaaa";
    // `concat!`-joined with the same tail text as `SECRET_TAIL` so this
    // file holds no literal spelling the whole token contiguously.
    const TOKEN: &str = concat!("dp.st.prd.", "acceptance08afakesecretbytesaaaaaaaaaaaaaa");
    const MARKER: &str = "[REDACTED DopplerServiceToken]";

    let value = Value::known(willikins_types::DopplerServiceToken::parse(TOKEN).unwrap());
    assert!(
        value.is_secret(),
        "acceptance test 8a: the fixture value must itself be secret"
    );

    let mut inputs = Inputs::new();
    inputs.insert(port("token"), value.clone());
    let mut outputs = Outputs::new();
    outputs.insert(port("token"), value.clone());
    let present = Observation::Present(outputs.clone());

    let planned = PlannedNode {
        name: node("token"),
        instance: None,
        tool: tool_name("doppler.service_token.ensure"),
        action: Action::Create,
        inputs: inputs.clone(),
        outputs: outputs.clone(),
    };
    let whole_plan = Plan {
        workflow: willikins_types::WorkflowName::parse("acceptance-test-8a").unwrap(),
        nodes: vec![planned.clone()],
        outputs: IndexMap::new(),
        class: Class::Reversible,
        requires_approval: false,
    };
    let tool_error = ToolError {
        kind: ToolErrorKind::Provider,
        message: format!("provider rejected inputs: {inputs:?}"),
    };

    let cases: Vec<(&str, String)> = vec![
        (
            "serde_json(inputs)",
            serde_json::to_string(&inputs).unwrap(),
        ),
        (
            "serde_json(outputs)",
            serde_json::to_string(&outputs).unwrap(),
        ),
        (
            "serde_json(present)",
            serde_json::to_string(&present).unwrap(),
        ),
        (
            "serde_json(planned_node)",
            serde_json::to_string(&planned).unwrap(),
        ),
        (
            "serde_json(plan)",
            serde_json::to_string(&whole_plan).unwrap(),
        ),
        (
            "serde_json(tool_error)",
            serde_json::to_string(&tool_error).unwrap(),
        ),
        ("Debug(inputs)", format!("{inputs:?}")),
        ("Debug(outputs)", format!("{outputs:?}")),
        ("Debug(present)", format!("{present:?}")),
        ("Debug(planned_node)", format!("{planned:?}")),
        ("Debug(plan)", format!("{whole_plan:?}")),
        ("Debug(tool_error)", format!("{tool_error:?}")),
        ("to_string(tool_error)", tool_error.to_string()),
        ("render(value)", value.render().to_string()),
    ];

    for (label, haystack) in cases {
        assert!(
            !haystack.contains(SECRET_TAIL),
            "acceptance test 8a: {label} leaked the secret's bytes: {haystack}"
        );
        assert!(
            haystack.contains(MARKER),
            "acceptance test 8a: {label} did not show the redaction marker: {haystack}"
        );
    }
}

/// Acceptance test 8b: `secret-get.yaml`, with its Doppler secret seeded
/// in fake state, must carry that value as `Known` in the finished
/// [`Plan`] and never print the seeded bytes — in the plan's own JSON
/// (checked directly against the library), and in both the CLI's `--json`
/// and plain-text output (checked by running the actual binary).
#[test]
fn acceptance_08b_seeded_secret_never_leaks() {
    const SEEDED_BYTES: &str = "acceptance-test-8b-fake-secret-bytes-do-not-leak";

    let fake_state_path = state_fixture("secret-seeded.json");
    let seeded_json = std::fs::read_to_string(&fake_state_path).unwrap();
    assert!(
        seeded_json.contains(SEEDED_BYTES),
        "acceptance test 8b: the fixture must actually seed the bytes this test checks for"
    );

    // Library level.
    let catalog = seeded_catalog(&fake_state_path);
    let workflow = load(&fixture("secret-get.yaml"));
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("acceptance test 8b: secret-get.yaml must check cleanly");
    let inputs = resolve_inputs(
        "acceptance test 8b",
        &checked,
        &[("project", RawInput::Scalar("widgets".to_string()))],
    );
    let result = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("acceptance test 8b: plan must succeed");
    let secret_node = find_planned(&result, "secret", None);
    let secret_value = secret_node
        .outputs
        .get(&port("value"))
        .expect("secret.value output exists");
    assert!(
        secret_value.is_known(),
        "acceptance test 8b: the seeded secret must plan as Known"
    );
    let json = serde_json::to_string(&result).unwrap();
    assert!(
        !json.contains(SEEDED_BYTES),
        "acceptance test 8b: plan JSON leaked: {json}"
    );
    assert!(
        json.contains("[REDACTED DopplerSecretValue]"),
        "acceptance test 8b: plan JSON: {json}"
    );

    // CLI level: text.
    let workflow_path = fixture("secret-get.yaml");
    let text_output = run(&[
        "plan",
        workflow_path.to_str().unwrap(),
        "--input",
        "project=widgets",
        "--fake-state",
        fake_state_path.to_str().unwrap(),
    ]);
    assert_eq!(
        exit_code(&text_output),
        0,
        "acceptance test 8b: stderr: {}",
        stderr(&text_output)
    );
    let text = stdout(&text_output);
    assert!(
        !text.contains(SEEDED_BYTES),
        "acceptance test 8b: CLI text leaked: {text}"
    );
    assert!(
        text.contains("[REDACTED DopplerSecretValue]"),
        "acceptance test 8b: CLI text: {text}"
    );

    // CLI level: JSON.
    let json_output = run(&[
        "--json",
        "plan",
        workflow_path.to_str().unwrap(),
        "--input",
        "project=widgets",
        "--fake-state",
        fake_state_path.to_str().unwrap(),
    ]);
    assert_eq!(
        exit_code(&json_output),
        0,
        "acceptance test 8b: stderr: {}",
        stderr(&json_output)
    );
    let json_text = stdout(&json_output);
    assert!(
        !json_text.contains(SEEDED_BYTES),
        "acceptance test 8b: CLI json leaked: {json_text}"
    );
    assert!(
        json_text.contains("[REDACTED DopplerSecretValue]"),
        "acceptance test 8b: CLI json: {json_text}"
    );
}

// ---------------------------------------------------------------------
// acceptance test 9: naming
// ---------------------------------------------------------------------

/// Golden and property-test coverage for acceptance test 9 already lives
/// in `willikins-types`: golden tests in `src/naming.rs` (`golden_github_repo`,
/// `golden_doppler_project`, `golden_doppler_root_config`, and the
/// multi-word-environment case), and property tests in
/// `tests/naming_properties.rs` and `tests/naming_v1_properties.rs` — the
/// latter two explicitly named after this acceptance test in their own
/// module docs. Pascal's non-injectivity for digit-only words is pinned
/// there too. This test only re-derives the design doc's own worked
/// example once more, through the exact types a workflow document
/// produces, and confirms each derived identity parses back as its own
/// target type.
#[test]
fn acceptance_09_naming() {
    let org = willikins_types::GitHubOrg::parse("lightless-labs").unwrap();
    let slug = willikins_types::ProjectSlug::parse("third-thoughts").unwrap();

    let repo = willikins_types::naming::v1::github_repo(&org, &slug);
    assert_eq!(
        repo.to_string(),
        "lightless-labs/third-thoughts",
        "acceptance test 9: naming::v1::github_repo golden value"
    );
    assert!(
        willikins_types::GitHubRepo::parse(&repo.to_string()).is_ok(),
        "acceptance test 9: a derived GitHubRepo must parse back as its own type"
    );

    let project = willikins_types::naming::v1::doppler_project(&slug);
    assert_eq!(
        project.to_string(),
        "third-thoughts",
        "acceptance test 9: naming::v1::doppler_project golden value"
    );
    assert!(
        willikins_types::DopplerProject::parse(&project.to_string()).is_ok(),
        "acceptance test 9: a derived DopplerProject must parse back as its own type"
    );

    let environment = willikins_types::EnvironmentSlug::parse("prd").unwrap();
    let config = willikins_types::naming::v1::doppler_root_config(&project, &environment);
    assert_eq!(
        config.to_string(),
        "third-thoughts/prd",
        "acceptance test 9: naming::v1::doppler_root_config golden value"
    );
    assert!(
        willikins_types::DopplerConfig::parse(&config.to_string()).is_ok(),
        "acceptance test 9: a derived DopplerConfig must parse back as its own type"
    );
}

// ---------------------------------------------------------------------
// acceptance test 10: reserved words
// ---------------------------------------------------------------------

/// Fully pinned already by `willikins-types`' own
/// `acceptance_test_10_reserved_single_words_rejected`
/// (`crates/willikins-types/src/slug.rs`), which asserts exactly this list.
/// Re-verified here since nothing more is required by the plan's wording.
#[test]
fn acceptance_10_reserved_words() {
    for word in ["native", "default", "type", "match", "self", "nul"] {
        assert!(
            willikins_types::ProjectSlug::parse(word).is_err(),
            "acceptance test 10: `{word}` should be rejected as reserved"
        );
    }
    for word in ["type-system", "self-hosted"] {
        assert!(
            willikins_types::ProjectSlug::parse(word).is_ok(),
            "acceptance test 10: `{word}` should be accepted"
        );
    }
}

// ---------------------------------------------------------------------
// acceptance test 11: compile-time guarantees
// ---------------------------------------------------------------------

/// Acceptance test 11's guarantees are compile-time and already pinned by
/// `willikins-types`' own `trybuild` suite
/// (`crates/willikins-types/tests/derive_compile_fail.rs`'s `compile_fail`
/// test, against fixtures under `tests/derive/fail/`): `#[domain(secret)]`
/// on a `String` newtype, `secrecy::SecretString` storage without
/// `secret`, and `serde_json::to_string(&token)` on a secret type (which
/// fails because a secret type generates no `Serialize`) all fail to
/// compile there. `SinkToken::new()` requiring the `executor` feature is a
/// structural guarantee documented on that same test, not a runnable
/// `trybuild` case — see its module docs for why. Nothing here can re-run
/// a `trybuild` suite from another crate's own test binary, so this test
/// only guards against that fixture set silently losing coverage.
#[test]
fn acceptance_11_compile_time_guarantees() {
    let fail_dir = workspace_root()
        .join("crates")
        .join("willikins-types")
        .join("tests")
        .join("derive")
        .join("fail");
    for name in [
        "secret_on_string.rs",
        "secretstring_without_secret.rs",
        "secret_serialize.rs",
        "invalid_regex.rs",
        "generic_struct.rs",
        "enum_input.rs",
        "non_secret_macro_on_secret_type.rs",
    ] {
        assert!(
            fail_dir.join(name).exists(),
            "acceptance test 11: missing trybuild fixture `{name}`"
        );
        assert!(
            fail_dir
                .join(format!("{}.stderr", name.trim_end_matches(".rs")))
                .exists(),
            "acceptance test 11: missing trybuild `.stderr` for `{name}`"
        );
    }
}

// ---------------------------------------------------------------------
// milestone 2 acceptance test 9: attribute mismatch
// (docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md, "Acceptance
// tests" #9 — distinct from this file's own, milestone-1-numbered
// `acceptance_09_naming` above.)
//
// The state fixture, `workflows/fixtures/state/repo-ours-public.json`,
// seeds `lightless-labs/third-thoughts` as ours and `public`. Unlike the
// `.yaml` fixtures under `workflows/fixtures/`, a `--fake-state` JSON file
// carries no header comment of its own (JSON has none, and `FakeState`'s
// `deny_unknown_fields` refuses an extra `_comment` key); this doc comment
// is that fixture's header, naming this test and the exact error:
// `plan` against the positive fixture (`visibility` defaults to
// `private`) must return
// `PlanError::AttributeMismatch { site: Site::Port { node: repo, port:
// visibility } }`.
// ---------------------------------------------------------------------

#[test]
fn milestone_2_acceptance_09_attribute_mismatch_visibility() {
    let workflow = load(&positive_fixture());
    let catalog = seeded_catalog(&state_fixture("repo-ours-public.json"));
    let checked = willikins_core::check(&workflow, &catalog).expect(
        "milestone 2 acceptance test 9: must check cleanly against the ours-public catalog",
    );
    let inputs = positive_inputs("milestone 2 acceptance test 9", &checked);
    let err = willikins_core::plan(&checked, &inputs, &catalog)
        .expect_err("milestone 2 acceptance test 9: a public-vs-private mismatch must not plan");
    match err {
        PlanError::AttributeMismatch { site } => {
            assert_eq!(
                site,
                port_site("repo", "visibility"),
                "milestone 2 acceptance test 9: AttributeMismatch site"
            );
        }
        other => panic!("milestone 2 acceptance test 9: expected AttributeMismatch, got {other:?}"),
    }
}

/// The other half of acceptance test 9, through the same fixture: `ensure`
/// on the mismatching repository is `Conflict`, and the *resource* state
/// it was asked to change is byte-identical afterwards — the mock
/// server's "records no `PATCH`" for the fake provider. `plan`'s refusal
/// is what a caller sees first, but the tool must refuse on its own too:
/// task 4's executor calls `ensure` from a plan taken before any node
/// ran.
///
/// `ensure_calls`/`read_calls` are excluded from the before/after
/// comparison: task 4b's own contract is "record the call, then decide",
/// so a refused call still bumps its counter (a test can otherwise never
/// tell "never called" from "called and refused" apart) — that bookkeeping
/// is not the resource state this test is about.
#[test]
fn milestone_2_acceptance_09_ensure_on_the_mismatch_conflicts_and_writes_nothing() {
    let json = std::fs::read_to_string(state_fixture("repo-ours-public.json"))
        .expect("the acceptance test 9 state fixture is readable");
    let state = Arc::new(Mutex::new(
        FakeState::from_json(&json).expect("the acceptance test 9 state fixture is valid"),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let tool = catalog
        .get(&tool_name("github.repo.ensure"))
        .expect("the fake catalog holds `github.repo.ensure`");

    let mut inputs = Inputs::new();
    inputs.insert(
        port("repo"),
        Value::known(
            willikins_types::GitHubRepo::parse("lightless-labs/third-thoughts")
                .expect("the fixture's repository parses"),
        ),
    );
    inputs.insert(
        port("visibility"),
        // The positive fixture's own default, which is what makes this a
        // mismatch against the fixture's `public`.
        Value::known(willikins_types::RepoVisibility::Private),
    );

    let resource_state = |value: &serde_json::Value| -> serde_json::Value {
        let mut value = value.clone();
        if let serde_json::Value::Object(map) = &mut value {
            map.remove("ensure_calls");
            map.remove("read_calls");
        }
        value
    };

    let before = serde_json::to_value(&*state.lock().unwrap()).expect("FakeState serializes");
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    let token = willikins_core::SinkToken::new();
    let err = tool
        .ensure(&inputs, &token)
        .expect_err("milestone 2 acceptance test 9: ensure must refuse a visibility mismatch");
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    let after = serde_json::to_value(&*state.lock().unwrap()).expect("FakeState serializes");
    assert_eq!(
        resource_state(&before),
        resource_state(&after),
        "a refused ensure must write no resource state"
    );
    assert_eq!(
        state.lock().unwrap().ensure_calls[&willikins_providers_fake::state::call_key(
            "github.repo.ensure",
            "lightless-labs/third-thoughts"
        )],
        1,
        "a refused call must still count as an attempt"
    );
}

/// Task 4a's acceptance test 7, the `ApprovalRequired` half, through a
/// real parsed document (`workflows/fixtures/irreversible.yaml`) against
/// the real, unmodified fake catalog. The refusal happens at `apply`'s
/// rule 1 -- before the re-plan and before any `ensure` call -- so it
/// holds even though this fixture's `ci_secret` node would otherwise run
/// into the fake `doppler.service_token.ensure` gap that
/// `willikins-core`'s own `tests/common::FixedTokenService` works around
/// (see that type's doc comment, and this task's notes): rule 1 refuses
/// before the walk ever reaches that node, so the unmodified fake catalog
/// is exactly right for this half. The `Human`-approval half (which does
/// reach that node) is covered at the core level instead, against the
/// substitute catalog.
#[test]
fn milestone_2_acceptance_07_auto_on_the_irreversible_fixture_refuses_before_any_provider_call() {
    let workflow = load(&fixture("irreversible.yaml"));
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("milestone 2 acceptance test 7: irreversible.yaml must check cleanly");
    let inputs = resolve_inputs(
        "milestone 2 acceptance test 7",
        &checked,
        &[
            ("slug", RawInput::Scalar("third-thoughts".to_string())),
            ("org", RawInput::Scalar("lightless-labs".to_string())),
        ],
    );
    let approved = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("milestone 2 acceptance test 7: plan against empty state succeeds");
    assert_eq!(approved.class, Class::Irreversible);
    assert!(approved.requires_approval);

    let mut observer = willikins_core::NoopObserver;
    let err = willikins_core::apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &willikins_core::Approval::Auto,
        &mut observer,
    )
    .expect_err("milestone 2 acceptance test 7: Auto must refuse an Irreversible plan");
    match err {
        willikins_core::ApplyError::ApprovalRequired { class } => {
            assert_eq!(class, Class::Irreversible);
        }
        other => panic!("milestone 2 acceptance test 7: expected ApprovalRequired, got {other:?}"),
    }

    let locked = state.lock().unwrap();
    assert!(locked.github_repos.is_empty(), "no provider call was made");
    assert!(
        locked.doppler_projects.is_empty(),
        "no provider call was made"
    );
}

// ---------------------------------------------------------------------
// milestone 2 acceptance test 5: executor happy path
// ---------------------------------------------------------------------

/// A [`willikins_types::DopplerServiceToken`]-shaped literal distinctive
/// enough that its bytes are unmistakable in any leaked output. Same
/// construction as `willikins-core`'s own `tests/common::distinctive_token`.
fn distinctive_token() -> willikins_types::DopplerServiceToken {
    willikins_types::DopplerServiceToken::parse(&format!("dp.st.prd.{}", "MARKER".repeat(7)))
        .expect("a valid token literal")
}

const MARKER_BYTES: &str = "MARKERMARKERMARKERMARKERMARKERMARKERMARKER";

/// Acceptance test 5: the positive fixture against empty fake state, with
/// `next_token` seeded to a distinctive marker. Every status the plan
/// lists, `repo_url` `Known`, and the marker absent from `Applied`'s JSON,
/// its `Debug`, and every observer event's `Debug` and JSON — while
/// `ci_secret`'s `NodeStarted` inputs still print the general redaction
/// marker (it binds a secret port). The CLI text half of the same claim
/// lives in `render.rs`'s own tests (no `main.rs` caller for `apply` yet;
/// see `applied_text`'s doc). The journal and `tracing` halves are
/// deferred to tasks 5 and 10b, which do not exist yet.
#[test]
#[allow(clippy::too_many_lines)] // one scenario, walked start to end; splitting scatters it
fn milestone_2_acceptance_05_executor_happy_path_with_marker_and_qa_environment() {
    let workflow = load(&positive_fixture());
    let state = Arc::new(Mutex::new(
        FakeState::new().with_next_token(distinctive_token()),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("milestone 2 acceptance test 5: positive fixture must check cleanly");

    let inputs = resolve_inputs(
        "milestone 2 acceptance test 5",
        &checked,
        &[
            ("slug", RawInput::Scalar("third-thoughts".to_string())),
            ("org", RawInput::Scalar("lightless-labs".to_string())),
            (
                "environments",
                RawInput::List(vec![
                    "dev".to_string(),
                    "stg".to_string(),
                    "prd".to_string(),
                    "qa".to_string(),
                ]),
            ),
        ],
    );

    let approved = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("milestone 2 acceptance test 5: plan against empty state must succeed");

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("milestone 2 acceptance test 5: apply against empty state must succeed");

    assert!(matches!(
        node_status(&applied, "names", None),
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
        node_status(&applied, "token", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&applied, "ci_secret", None),
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
    assert!(
        matches!(
            node_status(&applied, "configs", Some("qa")),
            NodeStatus::Created
        ),
        "configs[qa] is outside the seeded defaults and must be Created"
    );

    let repo_url = applied
        .outputs
        .get(&output("repo_url"))
        .expect("repo_url must be resolved");
    assert!(repo_url.is_known(), "repo_url must be Known");

    let applied_json = serde_json::to_string(&applied).expect("Applied serializes");
    assert!(
        !applied_json.contains(MARKER_BYTES),
        "Applied JSON leaked: {applied_json}"
    );
    let applied_debug = format!("{applied:?}");
    assert!(
        !applied_debug.contains(MARKER_BYTES),
        "Applied Debug leaked: {applied_debug}"
    );

    for event in &observer.events {
        let debug = format!("{event:?}");
        assert!(!debug.contains(MARKER_BYTES), "event Debug leaked: {debug}");
        let json = serde_json::to_string(event).expect("ApplyEvent serializes");
        assert!(!json.contains(MARKER_BYTES), "event JSON leaked: {json}");
    }

    let ci_secret_started_inputs = observer
        .events
        .iter()
        .find_map(|event| match event {
            ApplyEvent::NodeStarted {
                node: n, inputs, ..
            } if *n == node("ci_secret") => Some(inputs),
            _ => None,
        })
        .expect("a NodeStarted event for ci_secret must exist");
    let ci_secret_inputs_json =
        serde_json::to_string(ci_secret_started_inputs).expect("Inputs serializes");
    assert!(
        ci_secret_inputs_json.contains("REDACTED"),
        "ci_secret's secret input must print the redaction marker: {ci_secret_inputs_json}"
    );
    assert!(!ci_secret_inputs_json.contains(MARKER_BYTES));
}

// ---------------------------------------------------------------------
// milestone 2 acceptance test 6: convergence
// ---------------------------------------------------------------------

/// Acceptance test 6a: `fail_ensure_once` at
/// `doppler.config.ensure#third-thoughts/stg`. The first apply stops with
/// `repo` and `doppler` `Created`, `configs[dev]` `Unchanged` (seeded by
/// `doppler.project.ensure`), `configs[stg]` `Failed` (the injection fires
/// before the tool ever observes that `stg` is already present too), and
/// everything after `NotRun`. A fresh plan and a second apply then
/// converge: `stg` was never actually mutated by the failed attempt, so it
/// (like `dev` and `prd`) reports `Unchanged`; `token` and `ci_secret`,
/// never reached the first time, are `Created`.
#[test]
fn milestone_2_acceptance_06a_convergence_after_a_config_failure() {
    let workflow = load(&positive_fixture());
    let state = Arc::new(Mutex::new(
        FakeState::new().with_fail_ensure_once("doppler.config.ensure", "third-thoughts/stg"),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("milestone 2 acceptance test 6a: positive fixture must check cleanly");
    let inputs = positive_inputs("milestone 2 acceptance test 6a", &checked);

    let first_plan = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("milestone 2 acceptance test 6a: first plan against empty state must succeed");
    let mut observer = willikins_core::NoopObserver;
    let first_err = apply(
        &checked,
        &inputs,
        &catalog,
        &first_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("the injected failure must stop the first apply");
    let ApplyError::Tool {
        node: failed_node,
        instance,
        applied: first_applied,
        ..
    } = first_err
    else {
        panic!("milestone 2 acceptance test 6a: expected ApplyError::Tool");
    };
    assert_eq!(failed_node, node("configs"));
    assert_eq!(instance.as_deref(), Some("stg"));
    assert!(matches!(
        node_status(&first_applied, "repo", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&first_applied, "doppler", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&first_applied, "configs", Some("dev")),
        NodeStatus::Unchanged
    ));
    assert!(matches!(
        node_status(&first_applied, "configs", Some("stg")),
        NodeStatus::Failed { .. }
    ));
    assert!(matches!(
        node_status(&first_applied, "configs", Some("prd")),
        NodeStatus::NotRun
    ));
    assert!(matches!(
        node_status(&first_applied, "token", None),
        NodeStatus::NotRun
    ));
    assert!(matches!(
        node_status(&first_applied, "ci_secret", None),
        NodeStatus::NotRun
    ));

    let second_plan = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("milestone 2 acceptance test 6a: a fresh plan must succeed after the failure");
    let mut observer2 = willikins_core::NoopObserver;
    let second_applied = apply(
        &checked,
        &inputs,
        &catalog,
        &second_plan,
        &Approval::Auto,
        &mut observer2,
    )
    .expect("milestone 2 acceptance test 6a: the second apply must converge");
    assert!(matches!(
        node_status(&second_applied, "repo", None),
        NodeStatus::Unchanged
    ));
    assert!(matches!(
        node_status(&second_applied, "doppler", None),
        NodeStatus::Unchanged
    ));
    for env in ["dev", "stg", "prd"] {
        assert!(
            matches!(
                node_status(&second_applied, "configs", Some(env)),
                NodeStatus::Unchanged
            ),
            "configs[{env}] must be Unchanged on the second apply"
        );
    }
    assert!(matches!(
        node_status(&second_applied, "token", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&second_applied, "ci_secret", None),
        NodeStatus::Created
    ));
}

/// Acceptance test 6b: `fail_ensure_once` at `ci_secret`
/// (`github.actions_secret.ensure#lightless-labs/third-thoughts#DOPPLER_TOKEN`).
/// The first apply mints `token` (`Created`) but fails storing it; a fresh
/// plan shows `token` `NoOp` (its value can never be re-read); the second
/// apply is blocked by `ApplyError::UnknownInput` naming `token`, with no
/// further call to `github.actions_secret.ensure` (its `ensure_calls`
/// count stays at the one failed attempt). Running the rotation workflow
/// with `Approval::Human` mints a fresh token and stores it, converging
/// the consumer: both `token` and `ci_secret` report `Created`. The
/// marker seeded for this second part never appears anywhere.
#[test]
#[allow(clippy::too_many_lines)] // one scenario, walked start to end; splitting scatters it
fn milestone_2_acceptance_06b_unknown_input_gap_and_rotation_workflow_converges_it() {
    let workflow = load(&positive_fixture());
    let ci_secret_key = "lightless-labs/third-thoughts#DOPPLER_TOKEN";
    let state = Arc::new(Mutex::new(
        FakeState::new().with_fail_ensure_once("github.actions_secret.ensure", ci_secret_key),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("milestone 2 acceptance test 6b: positive fixture must check cleanly");
    let inputs = positive_inputs("milestone 2 acceptance test 6b", &checked);

    let first_plan = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("milestone 2 acceptance test 6b: first plan must succeed");
    let mut observer = willikins_core::NoopObserver;
    let first_err = apply(
        &checked,
        &inputs,
        &catalog,
        &first_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("the injected ci_secret failure must stop the first apply");
    let ApplyError::Tool {
        node: failed_node,
        applied: first_applied,
        ..
    } = first_err
    else {
        panic!("milestone 2 acceptance test 6b: expected ApplyError::Tool");
    };
    assert_eq!(failed_node, node("ci_secret"));
    assert!(matches!(
        node_status(&first_applied, "token", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&first_applied, "ci_secret", None),
        NodeStatus::Failed { .. }
    ));

    let second_plan = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("milestone 2 acceptance test 6b: a fresh plan must succeed after the failure");
    assert_eq!(
        find_planned(&second_plan, "token", None).action,
        Action::NoOp,
        "token already exists and cannot be re-read, so it plans NoOp"
    );

    let ensure_calls_key =
        willikins_providers_fake::state::call_key("github.actions_secret.ensure", ci_secret_key);
    let calls_before = state
        .lock()
        .unwrap()
        .ensure_calls
        .get(&ensure_calls_key)
        .copied();
    let mut observer2 = willikins_core::NoopObserver;
    let second_err = apply(
        &checked,
        &inputs,
        &catalog,
        &second_plan,
        &Approval::Auto,
        &mut observer2,
    )
    .expect_err("token's value cannot be re-read, so the second apply must be blocked");
    match second_err {
        ApplyError::UnknownInput { node: n, from, .. } => {
            assert_eq!(n, node("ci_secret"));
            assert_eq!(from, node("token"));
        }
        other => panic!("milestone 2 acceptance test 6b: expected UnknownInput, got {other:?}"),
    }
    let calls_after = state
        .lock()
        .unwrap()
        .ensure_calls
        .get(&ensure_calls_key)
        .copied();
    assert_eq!(
        calls_before, calls_after,
        "ci_secret must not be called again while blocked"
    );

    // Seed a fresh distinctive marker for the rotation run, in place,
    // under the same `Arc`.
    {
        let mut locked = state.lock().unwrap();
        let taken = std::mem::take(&mut *locked);
        *locked = taken.with_next_token(distinctive_token());
    }

    let rotate_workflow = load(&rotate_fixture());
    let rotate_checked = willikins_core::check(&rotate_workflow, &catalog)
        .expect("milestone 2 acceptance test 6b: rotate-service-token.yaml must check cleanly");
    let rotate_inputs = resolve_inputs(
        "milestone 2 acceptance test 6b (rotate)",
        &rotate_checked,
        &[
            ("project", RawInput::Scalar("third-thoughts".to_string())),
            (
                "repo",
                RawInput::Scalar("lightless-labs/third-thoughts".to_string()),
            ),
        ],
    );
    let rotate_plan = willikins_core::plan(&rotate_checked, &rotate_inputs, &catalog)
        .expect("milestone 2 acceptance test 6b: rotate plan must succeed");
    assert!(rotate_plan.requires_approval, "rotate is Destructive");

    let mut rotate_observer = RecordingObserver::new();
    let approval = Approval::Human {
        approver: willikins_core::PrincipalId::parse("operator").unwrap(),
        at: willikins_core::Timestamp::now(),
    };
    let rotate_applied = apply(
        &rotate_checked,
        &rotate_inputs,
        &catalog,
        &rotate_plan,
        &approval,
        &mut rotate_observer,
    )
    .expect("milestone 2 acceptance test 6b: the rotation must converge the consumer");
    assert!(matches!(
        node_status(&rotate_applied, "token", None),
        NodeStatus::Created
    ));
    assert!(matches!(
        node_status(&rotate_applied, "ci_secret", None),
        NodeStatus::Created
    ));

    let rotate_json = serde_json::to_string(&rotate_applied).expect("Applied serializes");
    assert!(
        !rotate_json.contains(MARKER_BYTES),
        "rotation Applied JSON leaked: {rotate_json}"
    );
    assert!(!format!("{rotate_applied:?}").contains(MARKER_BYTES));
    for event in &rotate_observer.events {
        assert!(!format!("{event:?}").contains(MARKER_BYTES));
        assert!(
            !serde_json::to_string(event)
                .expect("ApplyEvent serializes")
                .contains(MARKER_BYTES)
        );
    }
}

/// Acceptance test 6c: three applies of the positive fixture in a row,
/// with no failure injected. The first creates everything (`configs`
/// `Unchanged` where seeded by `doppler.project.ensure`, everything else
/// `Created`). By the second, `ci_secret`'s `value` input resolves to
/// `Unknown` (`token` already exists and cannot be re-read), and
/// `ci_secret`'s own fresh `read` reports `Present` (it was stored on the
/// first apply), so it converges without a call: `Converged`, not
/// `Unchanged`. The third apply repeats the second exactly, and
/// `github.actions_secret.ensure`'s own call count does not move between
/// them — proof `Converged` never calls the tool.
#[test]
fn milestone_2_acceptance_06c_steady_state_third_apply_converges_without_a_call() {
    let workflow = load(&positive_fixture());
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("milestone 2 acceptance test 6c: positive fixture must check cleanly");
    let inputs = positive_inputs("milestone 2 acceptance test 6c", &checked);

    let run = |label: &str| -> Applied {
        let plan = willikins_core::plan(&checked, &inputs, &catalog)
            .unwrap_or_else(|err| panic!("{label}: plan must succeed: {err}"));
        let mut observer = willikins_core::NoopObserver;
        apply(
            &checked,
            &inputs,
            &catalog,
            &plan,
            &Approval::Auto,
            &mut observer,
        )
        .unwrap_or_else(|err| panic!("{label}: apply must succeed: {err}"))
    };

    let first = run("milestone 2 acceptance test 6c, first apply");
    assert!(matches!(
        node_status(&first, "ci_secret", None),
        NodeStatus::Created
    ));

    let ci_secret_calls_key = willikins_providers_fake::state::call_key(
        "github.actions_secret.ensure",
        "lightless-labs/third-thoughts#DOPPLER_TOKEN",
    );

    let second = run("milestone 2 acceptance test 6c, second apply");
    assert!(matches!(
        node_status(&second, "repo", None),
        NodeStatus::Unchanged
    ));
    assert!(matches!(
        node_status(&second, "doppler", None),
        NodeStatus::Unchanged
    ));
    for env in ["dev", "stg", "prd"] {
        assert!(matches!(
            node_status(&second, "configs", Some(env)),
            NodeStatus::Unchanged
        ));
    }
    assert!(matches!(
        node_status(&second, "token", None),
        NodeStatus::Unchanged
    ));
    assert!(matches!(
        node_status(&second, "ci_secret", None),
        NodeStatus::Converged
    ));
    let calls_after_second = state
        .lock()
        .unwrap()
        .ensure_calls
        .get(&ci_secret_calls_key)
        .copied();

    let third = run("milestone 2 acceptance test 6c, third apply");
    assert!(matches!(
        node_status(&third, "repo", None),
        NodeStatus::Unchanged
    ));
    for env in ["dev", "stg", "prd"] {
        assert!(matches!(
            node_status(&third, "configs", Some(env)),
            NodeStatus::Unchanged
        ));
    }
    assert!(matches!(
        node_status(&third, "token", None),
        NodeStatus::Unchanged
    ));
    assert!(matches!(
        node_status(&third, "ci_secret", None),
        NodeStatus::Converged
    ));
    let calls_after_third = state
        .lock()
        .unwrap()
        .ensure_calls
        .get(&ci_secret_calls_key)
        .copied();
    assert_eq!(
        calls_after_second, calls_after_third,
        "a Converged node must never call ensure"
    );
}

// ---------------------------------------------------------------------
// milestone 2 acceptance test 9 (core half): apply refuses drift via re-plan
// ---------------------------------------------------------------------

/// Acceptance test 9's core-level half: a plan approved against empty
/// state, then applied after the fake state changed underneath it so
/// `repo` now reads `Mismatch` — `apply`'s re-plan (rule 2) refuses with
/// `ApplyError::Plan { AttributeMismatch }` before minting a token or
/// calling any tool.
#[test]
fn milestone_2_acceptance_09_core_apply_refuses_a_visibility_mismatch_via_the_replan() {
    let workflow = load(&positive_fixture());
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("milestone 2 acceptance test 9 core: positive fixture must check cleanly");
    let inputs = positive_inputs("milestone 2 acceptance test 9 core", &checked);
    let approved = willikins_core::plan(&checked, &inputs, &catalog)
        .expect("milestone 2 acceptance test 9 core: plan against empty state must succeed");

    let json = std::fs::read_to_string(state_fixture("repo-ours-public.json"))
        .expect("the acceptance test 9 state fixture is readable");
    let mismatched = FakeState::from_json(&json)
        .expect("the acceptance test 9 state fixture is a valid FakeState");
    *state.lock().unwrap() = mismatched;

    let mut observer = willikins_core::NoopObserver;
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err(
        "milestone 2 acceptance test 9 core: a visibility mismatch must refuse via re-plan",
    );
    match err {
        ApplyError::Plan {
            error: PlanError::AttributeMismatch { site },
        } => {
            assert_eq!(site, port_site("repo", "visibility"));
        }
        other => panic!(
            "milestone 2 acceptance test 9 core: expected ApplyError::Plan{{AttributeMismatch}}, got {other:?}"
        ),
    }
    assert!(
        state.lock().unwrap().ensure_calls.is_empty(),
        "no provider call was made: the re-plan refused before rule 3 minted a token"
    );
}

// ---------------------------------------------------------------------
// milestone 2 acceptance test 6, generalised: every node, both fixtures
// ---------------------------------------------------------------------

/// Every non-pure node instance of `workflows/new-rust-service.yaml` for
/// `slug = third-thoughts`, `org = lightless-labs`, as the `(tool, key)`
/// pair `FakeState::with_fail_ensure_once` takes.
const POSITIVE_FIXTURE_INSTANCES: &[(&str, &str)] = &[
    ("github.repo.ensure", "lightless-labs/third-thoughts"),
    ("doppler.project.ensure", "third-thoughts"),
    ("doppler.config.ensure", "third-thoughts/dev"),
    ("doppler.config.ensure", "third-thoughts/stg"),
    ("doppler.config.ensure", "third-thoughts/prd"),
    ("doppler.service_token.ensure", "third-thoughts/prd#ci"),
    (
        "github.actions_secret.ensure",
        "lightless-labs/third-thoughts#DOPPLER_TOKEN",
    ),
];

/// The same for `workflows/rotate-service-token.yaml`.
const ROTATE_FIXTURE_INSTANCES: &[(&str, &str)] = &[
    ("doppler.config.ensure", "third-thoughts/prd"),
    ("doppler.service_token.rotate", "third-thoughts/prd#ci"),
    (
        "github.actions_secret.ensure",
        "lightless-labs/third-thoughts#DOPPLER_TOKEN",
    ),
];

/// Every `(node, instance)` an observer saw a `NodeStarted` for, in order,
/// asserting each is immediately followed by its own `NodeFinished`.
fn attempted_instances(observer: &RecordingObserver) -> Vec<(String, Option<String>)> {
    let mut attempted = Vec::new();
    let mut index = 0;
    while index < observer.events.len() {
        let ApplyEvent::NodeStarted {
            node: started,
            instance,
            ..
        } = &observer.events[index]
        else {
            panic!(
                "event {index} is not a NodeStarted: {:?}",
                observer.events[index]
            );
        };
        let ApplyEvent::NodeFinished {
            node: finished,
            instance: finished_instance,
            ..
        } = &observer.events[index + 1]
        else {
            panic!(
                "event {} is not the matching NodeFinished: {:?}",
                index + 1,
                observer.events[index + 1]
            );
        };
        assert_eq!(started, finished, "a Started/Finished pair must agree");
        assert_eq!(instance, finished_instance);
        attempted.push((started.to_string(), instance.clone()));
        index += 2;
    }
    attempted
}

/// The inputs `workflows/rotate-service-token.yaml` needs for the same
/// resources the positive fixture provisions.
fn rotate_inputs(label: &str, checked: &willikins_core::Checked) -> IndexMap<InputName, Value> {
    resolve_inputs(
        label,
        checked,
        &[
            ("project", RawInput::Scalar("third-thoughts".to_string())),
            (
                "repo",
                RawInput::Scalar("lightless-labs/third-thoughts".to_string()),
            ),
        ],
    )
}

/// `Approval::Human`, which every class accepts.
fn human_approval() -> Approval {
    Approval::Human {
        approver: willikins_core::PrincipalId::parse("operator").unwrap(),
        at: willikins_core::Timestamp::now(),
    }
}

/// Assert `applied` finished with nothing left to do: no instance is
/// `Failed` or `NotRun`.
fn assert_settled(label: &str, applied: &Applied) {
    for applied_node in &applied.nodes {
        assert!(
            matches!(
                applied_node.status,
                NodeStatus::Computed
                    | NodeStatus::Created
                    | NodeStatus::Unchanged
                    | NodeStatus::Converged
            ),
            "{label}: {}{} ended {:?}",
            applied_node.name,
            match &applied_node.instance {
                Some(key) => format!("[{key}]"),
                None => String::new(),
            },
            applied_node.status
        );
    }
}

/// Acceptance test 6, generalised from 6a and 6b's two hand-picked
/// injection points to *every* non-pure node instance of both positive
/// fixtures, one at a time: inject a single `fail_ensure_once` there, run
/// the workflow, and assert the run stops exactly there (that instance
/// `Failed`, everything after it `NotRun` with no observer events at all,
/// the failed one with exactly its own `NodeStarted`/`NodeFinished` pair),
/// then that a fresh plan and a second apply either settle everything or
/// stop at the one documented gap — `ci_secret` blocked on a `token` whose
/// value cannot be re-read — which `workflows/rotate-service-token.yaml`
/// then converges. The rotation fixture itself has no such gap: its
/// `doppler.service_token.rotate` always re-mints, so a failure at any of
/// its own nodes converges on the second run.
#[test]
#[allow(clippy::too_many_lines)] // one scenario walked twice per injection point; splitting scatters it
fn milestone_2_acceptance_06_every_instance_of_both_fixtures_converges_after_a_failure() {
    for (tool, key) in POSITIVE_FIXTURE_INSTANCES {
        let label = format!("positive fixture, failure injected at {tool}#{key}");
        let workflow = load(&positive_fixture());
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_next_token(distinctive_token())
                .with_fail_ensure_once(tool, key),
        ));
        let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
        let checked = willikins_core::check(&workflow, &catalog)
            .unwrap_or_else(|errors| panic!("{label}: must check cleanly: {errors:?}"));
        let inputs = positive_inputs(&label, &checked);

        let first_plan = willikins_core::plan(&checked, &inputs, &catalog)
            .unwrap_or_else(|err| panic!("{label}: first plan must succeed: {err}"));
        let mut observer = RecordingObserver::new();
        let Err(err) = apply(
            &checked,
            &inputs,
            &catalog,
            &first_plan,
            &Approval::Auto,
            &mut observer,
        ) else {
            panic!("{label}: the injected failure must stop the first apply")
        };
        // The failure the fake shaped (`ToolErrorKind::Provider`, message
        // built from the tool and its key) reaches the caller without the
        // minted token's bytes, in the error and in the partial result.
        for rendering in [
            err.to_string(),
            format!("{err:?}"),
            serde_json::to_string(&err).expect("ApplyError serializes"),
        ] {
            assert!(
                !rendering.contains(MARKER_BYTES),
                "{label}: a failed apply leaked the marker: {rendering}"
            );
        }
        let ApplyError::Tool {
            applied: partial, ..
        } = err
        else {
            panic!("{label}: expected ApplyError::Tool, got {err:?}");
        };

        // The run stopped exactly at the injected instance: it is the one
        // `Failed`, everything after it is `NotRun`, and the attempted
        // instances the observer saw are exactly the ones that are not.
        let failed_position = partial
            .nodes
            .iter()
            .position(|applied_node| matches!(applied_node.status, NodeStatus::Failed { .. }))
            .unwrap_or_else(|| panic!("{label}: no Failed instance in {:?}", partial.nodes));
        for applied_node in &partial.nodes[failed_position + 1..] {
            assert!(
                matches!(applied_node.status, NodeStatus::NotRun),
                "{label}: {} after the failure is {:?}",
                applied_node.name,
                applied_node.status
            );
        }
        let attempted = attempted_instances(&observer);
        let expected: Vec<(String, Option<String>)> = partial.nodes[..=failed_position]
            .iter()
            .map(|applied_node| (applied_node.name.to_string(), applied_node.instance.clone()))
            .collect();
        assert_eq!(
            attempted, expected,
            "{label}: a NotRun instance must produce no observer event"
        );

        // Round two: a fresh plan, and either everything settles or the
        // one documented gap appears.
        let second_plan = willikins_core::plan(&checked, &inputs, &catalog)
            .unwrap_or_else(|err| panic!("{label}: second plan must succeed: {err}"));
        let mut observer2 = willikins_core::NoopObserver;
        match apply(
            &checked,
            &inputs,
            &catalog,
            &second_plan,
            &Approval::Auto,
            &mut observer2,
        ) {
            Ok(applied) => assert_settled(&label, &applied),
            Err(ApplyError::UnknownInput {
                node: blocked,
                port: blocked_port,
                from,
                ..
            }) => {
                assert_eq!(
                    *tool, "github.actions_secret.ensure",
                    "{label}: only the token's consumer may be blocked"
                );
                assert_eq!(blocked, node("ci_secret"));
                assert_eq!(blocked_port, port("value"));
                assert_eq!(from, node("token"));

                // The remedy: the rotation workflow, which needs approval.
                let rotate_workflow = load(&rotate_fixture());
                let rotate_checked = willikins_core::check(&rotate_workflow, &catalog)
                    .unwrap_or_else(|errors| panic!("{label}: rotate must check: {errors:?}"));
                let rotate_resolved = rotate_inputs(&label, &rotate_checked);
                let rotate_plan = willikins_core::plan(&rotate_checked, &rotate_resolved, &catalog)
                    .unwrap_or_else(|err| panic!("{label}: rotate plan: {err}"));
                let mut rotate_observer = willikins_core::NoopObserver;
                let rotated = apply(
                    &rotate_checked,
                    &rotate_resolved,
                    &catalog,
                    &rotate_plan,
                    &human_approval(),
                    &mut rotate_observer,
                )
                .unwrap_or_else(|err| panic!("{label}: the rotation must converge it: {err}"));
                assert_settled(&format!("{label} (rotation)"), &rotated);

                // And the original workflow now settles too.
                let third_plan = willikins_core::plan(&checked, &inputs, &catalog)
                    .unwrap_or_else(|err| panic!("{label}: third plan: {err}"));
                let mut observer3 = willikins_core::NoopObserver;
                let settled = apply(
                    &checked,
                    &inputs,
                    &catalog,
                    &third_plan,
                    &Approval::Auto,
                    &mut observer3,
                )
                .unwrap_or_else(|err| panic!("{label}: third apply: {err}"));
                assert_settled(&format!("{label} (after rotation)"), &settled);
            }
            Err(other) => panic!("{label}: unexpected second-apply failure: {other:?}"),
        }
    }

    for (tool, key) in ROTATE_FIXTURE_INSTANCES {
        let label = format!("rotation fixture, failure injected at {tool}#{key}");
        let workflow = load(&rotate_fixture());
        let state = Arc::new(Mutex::new(
            FakeState::new()
                .with_next_token(distinctive_token())
                .with_fail_ensure_once(tool, key),
        ));
        let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
        let checked = willikins_core::check(&workflow, &catalog)
            .unwrap_or_else(|errors| panic!("{label}: must check cleanly: {errors:?}"));
        let inputs = rotate_inputs(&label, &checked);

        let first_plan = willikins_core::plan(&checked, &inputs, &catalog)
            .unwrap_or_else(|err| panic!("{label}: first plan must succeed: {err}"));
        let mut observer = willikins_core::NoopObserver;
        let err = apply(
            &checked,
            &inputs,
            &catalog,
            &first_plan,
            &human_approval(),
            &mut observer,
        )
        .expect_err("the injected failure must stop the first apply");
        assert!(
            matches!(err, ApplyError::Tool { .. }),
            "{label}: expected ApplyError::Tool, got {err:?}"
        );
        // The rotation mints the marker token before it can fail at
        // `ci_secret`, so the partial `Applied` this error carries holds a
        // `Known` secret: it must still render redacted everywhere.
        for rendering in [
            err.to_string(),
            format!("{err:?}"),
            serde_json::to_string(&err).expect("ApplyError serializes"),
        ] {
            assert!(
                !rendering.contains(MARKER_BYTES),
                "{label}: a failed rotation leaked the marker: {rendering}"
            );
        }

        let second_plan = willikins_core::plan(&checked, &inputs, &catalog)
            .unwrap_or_else(|err| panic!("{label}: second plan must succeed: {err}"));
        let mut observer2 = willikins_core::NoopObserver;
        let applied = apply(
            &checked,
            &inputs,
            &catalog,
            &second_plan,
            &human_approval(),
            &mut observer2,
        )
        .unwrap_or_else(|err| {
            panic!("{label}: the rotation workflow has no un-re-readable gap: {err:?}")
        });
        assert_settled(&label, &applied);

        // The distinctive marker the rotation minted never surfaces.
        let json = serde_json::to_string(&applied).expect("Applied serializes");
        assert!(!json.contains(MARKER_BYTES), "{label}: Applied JSON leaked");
        assert!(!format!("{applied:?}").contains(MARKER_BYTES));
    }
}

/// Rule 1 of the executor runs before rule 2's re-plan: a plan that needs
/// approval, applied with `Approval::Auto`, makes no provider call at all —
/// not even the `read`s a re-plan would do. Acceptance test 7's "before
/// touching a provider" clause, asserted through the fake's own call
/// counters rather than by inspection.
#[test]
fn milestone_2_acceptance_07_approval_required_makes_no_read_and_no_ensure_call() {
    let workflow = load(&rotate_fixture());
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog)
        .expect("rotate-service-token.yaml must check cleanly");
    let inputs = rotate_inputs("milestone 2 acceptance test 7", &checked);
    let approved = willikins_core::plan(&checked, &inputs, &catalog).expect("rotate plans");
    assert!(approved.requires_approval, "the rotation is Destructive");

    // `plan` itself reads; clear the counters so what follows is only
    // `apply`'s own doing.
    {
        let mut locked = state.lock().unwrap();
        locked.read_calls.clear();
        locked.ensure_calls.clear();
    }

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("a Destructive plan must refuse Approval::Auto");
    assert!(
        matches!(
            err,
            ApplyError::ApprovalRequired {
                class: Class::Destructive
            }
        ),
        "expected ApprovalRequired, got {err:?}"
    );

    let locked = state.lock().unwrap();
    assert!(
        locked.read_calls.is_empty(),
        "a refused apply must not even re-plan: {:?}",
        locked.read_calls
    );
    assert!(
        locked.ensure_calls.is_empty(),
        "a refused apply must call no ensure: {:?}",
        locked.ensure_calls
    );
    assert!(observer.events.is_empty(), "and report no events");
}

/// `apply` re-plans before it runs, so one `plan` + one `apply` of the
/// positive fixture reads every node's resource exactly twice (the
/// caller's plan, then the executor's own) and calls each `ensure` exactly
/// once. Pins the call accounting the convergence tests above rely on.
#[test]
fn milestone_2_acceptance_06_one_plan_and_one_apply_read_twice_and_ensure_once() {
    let workflow = load(&positive_fixture());
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog).expect("checks cleanly");
    let inputs = positive_inputs("milestone 2 call accounting", &checked);
    let approved = willikins_core::plan(&checked, &inputs, &catalog).expect("plans");
    let mut observer = willikins_core::NoopObserver;
    apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("applies");

    let locked = state.lock().unwrap();
    for (tool, key) in POSITIVE_FIXTURE_INSTANCES {
        let counter_key = willikins_providers_fake::state::call_key(tool, key);
        assert_eq!(
            locked.read_calls.get(&counter_key).copied(),
            Some(2),
            "{counter_key}: one read per plan, and `apply` re-plans"
        );
        assert_eq!(
            locked.ensure_calls.get(&counter_key).copied(),
            Some(1),
            "{counter_key}: exactly one ensure"
        );
    }
}

/// Acceptance test 8's "secret-is-not-drift" rule, end to end rather than
/// as a `Plan::fingerprint` unit test: a `doppler.secret.get` value
/// rotated out of band between `plan` and `apply` propagates as the current
/// value instead of refusing the run, because a secret port contributes
/// only its redaction marker to the fingerprint. Neither the old nor the
/// new bytes appear in the result.
#[test]
fn milestone_2_acceptance_08_a_secret_rotated_between_plan_and_apply_is_not_drift() {
    let config = willikins_types::DopplerConfig::parse("third-thoughts/prd").unwrap();
    let secret_name = willikins_types::SecretName::parse("DATABASE_URL").unwrap();
    let before = "before-rotation-bytes";
    let after = "after-rotation-bytes";

    let workflow = load(&fixture("secret-get.yaml"));
    let state = Arc::new(Mutex::new(
        FakeState::new()
            .with_doppler_config(&config)
            .with_doppler_secret(
                &config,
                &secret_name,
                willikins_types::DopplerSecretValue::parse(before).unwrap(),
            ),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let checked = willikins_core::check(&workflow, &catalog).expect("secret-get.yaml checks");
    let inputs = resolve_inputs(
        "milestone 2 acceptance test 8 (secret drift)",
        &checked,
        &[("project", RawInput::Scalar("third-thoughts".to_string()))],
    );
    let approved = willikins_core::plan(&checked, &inputs, &catalog).expect("plans");

    // Rotate the secret out of band, between approval and apply.
    {
        let mut locked = state.lock().unwrap();
        let taken = std::mem::take(&mut *locked);
        *locked = taken.with_doppler_secret(
            &config,
            &secret_name,
            willikins_types::DopplerSecretValue::parse(after).unwrap(),
        );
    }

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("a secret's bytes changing is not drift");
    assert!(matches!(
        node_status(&applied, "ci_secret", None),
        NodeStatus::Created
    ));

    let json = serde_json::to_string(&applied).expect("Applied serializes");
    let debug = format!("{applied:?}");
    for bytes in [before, after] {
        assert!(!json.contains(bytes), "Applied JSON leaked {bytes}: {json}");
        assert!(!debug.contains(bytes), "Applied Debug leaked {bytes}");
        for event in &observer.events {
            assert!(!format!("{event:?}").contains(bytes), "event Debug leaked");
            assert!(
                !serde_json::to_string(event)
                    .expect("ApplyEvent serializes")
                    .contains(bytes),
                "event JSON leaked"
            );
        }
    }
}
