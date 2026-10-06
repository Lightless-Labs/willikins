//! Milestone 2b, part P and acceptance 2
//! (`docs/plans/2026-10-05-milestone-2b-composition.md`): the `uses:`
//! keyword in the document format, and its four negative fixtures under
//! `workflows/fixtures/composition/`. L2 (the linker's refusals) extends
//! this file with the fixtures a document can express for the linker's
//! own errors.

use std::path::Path;

use indexmap::IndexMap;
use willikins_core::{
    CheckError, Checked, InputName, NodeName, OutputName, PortName, ResolveFailure, Site, TypeName,
    TypeRef, Value, Workflow, check, link, plan,
};
use willikins_dsl::{DocumentErrorKind, load_document};
use willikins_types::{DomainType, WorkflowName};

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

fn input(name: &str) -> InputName {
    InputName::parse(name).unwrap()
}

fn output(name: &str) -> OutputName {
    OutputName::parse(name).unwrap()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn fixture_path(relative: &str) -> String {
    format!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../{}"),
        format!("workflows/fixtures/composition/{relative}")
    )
}

fn wf_name(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

fn ty(name: &str) -> TypeRef {
    TypeRef::scalar(TypeName::parse(name).unwrap())
}

/// `link`'s `resolve` callback, for a root fixture whose children sit
/// beside it in `workflows/fixtures/composition/`: `<name>.yaml` loaded
/// with [`load_document`], [`ResolveFailure::NotFound`] when no such
/// file exists (never [`ResolveFailure::Refused`] -- no fixture here is
/// a symlink or a name mismatch), and [`ResolveFailure::Document`] when
/// the file exists but fails to parse.
fn directory_resolver() -> impl FnMut(&WorkflowName) -> Result<Workflow, ResolveFailure> {
    move |name: &WorkflowName| {
        let path = Path::new(&fixture_path(&format!("{name}.yaml"))).to_path_buf();
        if !path.is_file() {
            return Err(ResolveFailure::NotFound);
        }
        load_document(&path).map_err(|err| ResolveFailure::Document {
            message: err.to_string(),
        })
    }
}

/// Load `fixture` (a root) and link it through [`directory_resolver`],
/// returning the [`CheckError`]s linking failed with.
fn link_fixture(fixture: &str) -> Vec<CheckError> {
    let workflow = load_document(Path::new(&fixture_path(fixture)))
        .unwrap_or_else(|err| panic!("{fixture}: expected to load, got {err:?}"));
    link(&workflow, &mut directory_resolver()).unwrap_err()
}

/// Load `fixture` (a root), link it through [`directory_resolver`] (must
/// succeed -- these fixtures are for `check`'s own boundary errors, task
/// C1, never the linker's), then `check` the flat graph against
/// `willikins_providers_fake`'s real catalog, exactly as `acceptance.rs`
/// does for a root-level fixture.
fn check_fixture(fixture: &str) -> Result<Checked, Vec<CheckError>> {
    let workflow = load_document(Path::new(&fixture_path(fixture)))
        .unwrap_or_else(|err| panic!("{fixture}: expected to load, got {err:?}"));
    let linked = link(&workflow, &mut directory_resolver())
        .unwrap_or_else(|errs| panic!("{fixture}: expected to link, got {errs:?}"));
    let (_state, catalog) = willikins_providers_fake::empty();
    check(&linked.workflow, &catalog)
}

/// Load `fixture` and return the `(path, message)` of the
/// [`DocumentErrorKind::Semantic`] error it must fail with -- panicking
/// (naming the fixture) if it loads, or if it fails any other way.
fn semantic_error(fixture: &str) -> (String, String) {
    let path = fixture_path(fixture);
    let err = load_document(Path::new(&path))
        .err()
        .unwrap_or_else(|| panic!("{fixture}: expected to fail to load"));
    match err.kind {
        DocumentErrorKind::Semantic { path, message } => (path, message),
        other => panic!("{fixture}: expected a Semantic error, got {other:?}"),
    }
}

/// `workflows/fixtures/composition/uses-and-tool.yaml`'s header.
#[test]
fn uses_and_tool_together_fails_to_load_with_one_located_error() {
    let (path, message) = semantic_error("uses-and-tool.yaml");
    assert_eq!(path, "steps.a");
    assert_eq!(
        message,
        "a step may declare exactly one of `tool` or `uses`, not both"
    );
}

/// `workflows/fixtures/composition/uses-neither.yaml`'s header.
#[test]
fn uses_neither_fails_to_load_with_one_located_error() {
    let (path, message) = semantic_error("uses-neither.yaml");
    assert_eq!(path, "steps.a");
    assert_eq!(
        message,
        "a step must declare exactly one of `tool` or `uses`"
    );
}

/// `workflows/fixtures/composition/uses-for-each.yaml`'s header.
#[test]
fn uses_for_each_fails_to_load_with_one_located_error() {
    let (path, message) = semantic_error("uses-for-each.yaml");
    assert_eq!(path, "steps.a.for_each");
    assert_eq!(
        message,
        "a `uses:` step may not have a `for_each` (milestone 2b decision (d12)); use a \
         separate root document per item instead"
    );
}

/// `workflows/fixtures/composition/uses-bad-name.yaml`'s header.
#[test]
fn uses_bad_name_fails_to_load_with_one_located_error() {
    let (path, message) = semantic_error("uses-bad-name.yaml");
    assert_eq!(path, "steps.a.uses");
    assert_eq!(message, "does not match the required pattern");
}

/// Milestone 2b, SHARED VALUES, "Authored names": the DSL refuses a `/`
/// in a `with:` key under `uses:`, the same as it already refuses one in
/// a step key or an input name (P1).
#[test]
fn a_slash_in_a_uses_with_key_is_a_semantic_error_at_the_with_path() {
    let err = willikins_dsl::parse_document(
        "\
name: demo
steps:
  org:
    uses: example-org
    with:
      base/configs: literal
",
    )
    .unwrap_err();
    match err.kind {
        DocumentErrorKind::Semantic { path, message } => {
            assert_eq!(path, "steps.org.with.base/configs");
            assert!(message.contains('/'), "{message}");
        }
        other => panic!("expected a Semantic error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// L2 commit 1: cycle, depth, and size bounds (decision (d9)), each
// expressed as a document and linked with `directory_resolver`.
// ---------------------------------------------------------------------

/// `workflows/fixtures/composition/self-use.yaml`'s header.
#[test]
fn a_self_use_is_refused_as_uses_cycle() {
    let errors = link_fixture("self-use.yaml");
    assert_eq!(
        errors,
        vec![CheckError::UsesCycle {
            chain: vec![wf_name("self-use"), wf_name("self-use")],
        }]
    );
}

/// `workflows/fixtures/composition/cycle-a.yaml`'s header (with its
/// child `cycle-b.yaml`).
#[test]
fn a_two_document_cycle_is_refused_as_uses_cycle() {
    let errors = link_fixture("cycle-a.yaml");
    assert_eq!(
        errors,
        vec![CheckError::UsesCycle {
            chain: vec![wf_name("cycle-a"), wf_name("cycle-b"), wf_name("cycle-a")],
        }]
    );
}

/// `workflows/fixtures/composition/too-deep-root.yaml`'s header (with
/// its children `too-deep-1.yaml`..`too-deep-8.yaml`).
#[test]
fn a_nine_deep_chain_of_documents_is_refused_as_uses_too_deep() {
    let errors = link_fixture("too-deep-root.yaml");
    let expected_chain: Vec<WorkflowName> = std::iter::once("too-deep-root".to_string())
        .chain((1..=8).map(|i| format!("too-deep-{i}")))
        .map(|name| wf_name(&name))
        .collect();
    assert_eq!(
        errors,
        vec![CheckError::UsesTooDeep {
            chain: expected_chain,
        }]
    );
}

/// `workflows/fixtures/composition/diamond-root.yaml`'s header (with
/// its children `diamond-mid2.yaml`, `diamond-mid1.yaml`,
/// `diamond-leaf.yaml`).
#[test]
fn a_diamond_over_the_node_bound_is_refused_as_uses_too_large() {
    let errors = link_fixture("diamond-root.yaml");
    match errors.as_slice() {
        [CheckError::UsesTooLarge { nodes }] => {
            assert!(*nodes > 2048, "expected the bound to be crossed: {nodes}");
        }
        other => panic!("expected exactly one UsesTooLarge, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// L2 commit 2: the boundary and alias refusals (decisions (d4), (d7)).
// `PathInAuthoredName` has no fixture here: the DSL already refuses a
// `/` in an authored name at parse time (P1), so the linker's own
// protection only ever reaches a hand-built `Workflow`, covered at the
// core-unit level in `compose.rs`'s own tests.
// ---------------------------------------------------------------------

/// `workflows/fixtures/composition/unknown-child.yaml`'s header.
#[test]
fn an_unknown_child_is_refused_as_unknown_workflow() {
    let errors = link_fixture("unknown-child.yaml");
    assert_eq!(
        errors,
        vec![CheckError::UnknownWorkflow {
            node: node("child"),
            workflow: wf_name("ghost-child"),
        }]
    );
}

/// `workflows/fixtures/composition/used-document-root.yaml`'s header
/// (with its child `used-document-child.yaml`).
#[test]
fn a_child_that_fails_to_parse_is_refused_as_used_document() {
    let errors = link_fixture("used-document-root.yaml");
    match errors.as_slice() {
        [CheckError::UsedDocument { node, workflow, .. }] => {
            assert_eq!(*node, self::node("child"));
            assert_eq!(*workflow, wf_name("used-document-child"));
        }
        other => panic!("expected exactly one UsedDocument, got {other:?}"),
    }
}

/// `workflows/fixtures/composition/unknown-uses-input-root.yaml`'s
/// header (with its child `unknown-uses-input-child.yaml`).
#[test]
fn an_unknown_with_key_is_refused_as_unknown_uses_input() {
    let errors = link_fixture("unknown-uses-input-root.yaml");
    assert_eq!(
        errors,
        vec![CheckError::UnknownUsesInput {
            node: node("child"),
            input: input("mystery"),
        }]
    );
}

/// `workflows/fixtures/composition/unbound-uses-input-root.yaml`'s
/// header (with its child `unbound-uses-input-child.yaml`).
#[test]
fn a_required_unbound_undefaulted_input_is_refused_as_unbound_uses_input() {
    let errors = link_fixture("unbound-uses-input-root.yaml");
    assert_eq!(
        errors,
        vec![CheckError::UnboundUsesInput {
            node: node("child"),
            input: input("needed"),
        }]
    );
}

/// `workflows/fixtures/composition/item-in-uses-root.yaml`'s header
/// (with its child `item-in-uses-child.yaml`).
#[test]
fn item_bound_to_a_uses_input_is_refused_as_item_in_uses() {
    let errors = link_fixture("item-in-uses-root.yaml");
    assert_eq!(
        errors,
        vec![CheckError::ItemInUses {
            node: node("child"),
            input: input("known"),
        }]
    );
}

/// `workflows/fixtures/composition/unknown-uses-output-root.yaml`'s
/// header (with its child `unknown-uses-output-child.yaml`).
#[test]
fn an_unknown_uses_output_reference_is_refused() {
    let errors = link_fixture("unknown-uses-output-root.yaml");
    assert_eq!(
        errors,
        vec![CheckError::UnknownUsesOutput {
            site: Site::Output { name: output("x") },
            node: node("child"),
            output: output("missing"),
        }]
    );
}

/// `workflows/fixtures/composition/keyed-on-uses-root.yaml`'s header
/// (with its child `keyed-on-uses-child.yaml`).
#[test]
fn a_keyed_reference_onto_a_uses_step_is_refused() {
    let errors = link_fixture("keyed-on-uses-root.yaml");
    assert_eq!(
        errors,
        vec![CheckError::KeyedOnUses {
            site: Site::Output { name: output("x") },
            node: node("child"),
        }]
    );
}

/// `workflows/fixtures/composition/uses-output-cycle-root.yaml`'s
/// header (with its shared child `uses-output-cycle-child.yaml`).
#[test]
fn an_alias_cycle_between_two_uses_steps_outputs_is_refused() {
    let errors = link_fixture("uses-output-cycle-root.yaml");
    match errors.as_slice() {
        [CheckError::UsesOutputCycle { node: n, output: o }] => {
            assert!(*n == node("a") || *n == node("b"), "node: {n}");
            assert_eq!(*o, output("out"));
        }
        other => panic!("expected exactly one UsesOutputCycle, got {other:?}"),
    }
}

/// `workflows/fixtures/composition/nested-site-unknown-uses-output-root.yaml`'s
/// header (with its shared children `nested-site-achild.yaml` and
/// `nested-site-bchild.yaml`).
#[test]
fn an_unknown_uses_output_referenced_from_inside_another_uses_steps_with_is_sited_there() {
    let errors = link_fixture("nested-site-unknown-uses-output-root.yaml");
    assert_eq!(
        errors,
        vec![CheckError::UnknownUsesOutput {
            site: Site::Port {
                node: node("a"),
                port: port("x"),
            },
            node: node("b"),
            output: output("missing"),
        }]
    );
}

/// `workflows/fixtures/composition/nested-site-keyed-on-uses-root.yaml`'s
/// header (with its shared children `nested-site-achild.yaml` and
/// `nested-site-bchild.yaml`).
#[test]
fn a_keyed_reference_from_inside_another_uses_steps_with_is_sited_there() {
    let errors = link_fixture("nested-site-keyed-on-uses-root.yaml");
    assert_eq!(
        errors,
        vec![CheckError::KeyedOnUses {
            site: Site::Port {
                node: node("a"),
                port: port("x"),
            },
            node: node("b"),
        }]
    );
}

// ---------------------------------------------------------------------
// Task C1 (`check` at the boundary), acceptance 5 and 6.
// ---------------------------------------------------------------------

/// `workflows/fixtures/composition/uses-input-type-mismatch-root.yaml`'s
/// header (with its child `uses-input-type-mismatch-child.yaml`).
#[test]
fn a_github_repo_output_bound_to_a_doppler_project_input_gives_uses_input_type_mismatch() {
    let errors = check_fixture("uses-input-type-mismatch-root.yaml").expect_err("must fail check");
    assert_eq!(
        errors,
        vec![CheckError::UsesInputTypeMismatch {
            node: node("child"),
            input: input("project"),
            expected: ty("DopplerProject"),
            found: ty("GitHubRepo"),
        }]
    );
}

/// `workflows/fixtures/composition/uses-secret-input.yaml`'s header
/// (with its child `uses-secret-input-child.yaml`): a secret-typed
/// child input gives `SecretWorkflowInput`, under the combined
/// `<uses step>/<child input>` name, and `check` fails -- it never
/// reaches `plan`.
#[test]
fn a_secret_typed_child_input_gives_secret_workflow_input_and_never_reaches_plan() {
    let errors = check_fixture("uses-secret-input.yaml").expect_err("must fail check");
    assert_eq!(
        errors,
        vec![CheckError::SecretWorkflowInput {
            input: input("child/token"),
            ty: ty("DopplerSecretValue"),
        }]
    );
}

/// `workflows/fixtures/composition/uses-disallowed-input-root.yaml`'s
/// header (with its child `uses-disallowed-input-child.yaml`).
#[test]
fn a_child_declaring_template_source_gives_disallowed_input_type() {
    let errors = check_fixture("uses-disallowed-input-root.yaml").expect_err("must fail check");
    assert_eq!(
        errors,
        vec![CheckError::DisallowedInputType {
            input: input("child/template"),
            ty: ty("TemplateSource"),
        }]
    );
}

/// `workflows/fixtures/composition/uses-acknowledgement-default-root.yaml`'s
/// header (with its child `uses-acknowledgement-default-child.yaml`).
#[test]
fn a_child_declaring_a_defaulted_acknowledgement_gives_acknowledgement_default() {
    let errors =
        check_fixture("uses-acknowledgement-default-root.yaml").expect_err("must fail check");
    assert_eq!(
        errors,
        vec![CheckError::AcknowledgementDefault {
            input: input("child/ack"),
        }]
    );
}

/// `workflows/fixtures/composition/uses-class-root.yaml`'s header (with
/// its child `uses-class-child.yaml`): a root of only pure nodes using a
/// child with one `Irreversible` node has class `Irreversible` and
/// requires approval -- `Checked.class` is the maximum over the flat
/// graph's non-pure nodes, with no extra code for composition.
#[test]
fn a_root_of_only_pure_nodes_using_an_irreversible_child_has_class_irreversible() {
    let checked = check_fixture("uses-class-root.yaml").expect("must check cleanly");
    assert_eq!(checked.class, willikins_core::Class::Irreversible);
    assert!(checked.class.requires_approval());
}

// ---------------------------------------------------------------------
// Task F1, commit 2 (acceptance 12's closing clause): a characterization
// snapshot over every document under `workflows/fixtures/composition/`,
// mirroring `tests/it/acceptance.rs`'s own
// `characterization_of_every_document` (load, check, plan), with a
// `link` step inserted before `check` -- any fixture here may itself
// declare `uses:`, and a non-empty `uses:` reaching `check` unlinked is
// exactly `CheckError::Unlinked`, not a useful characterization. This is
// a separate snapshot from that one: the plan's SHARED VALUES row
// ("Fixture directory (P2 on)") says this directory "is not scanned by
// the existing characterization, which stays byte-identical", so
// composition fixtures are deliberately never added to it.
// ---------------------------------------------------------------------

/// Every `workflows/fixtures/composition/*.yaml` path, relative to the
/// workspace root, sorted by file name (`read_dir`'s own order is not
/// stable -- the same reason `tests/it/acceptance.rs`'s own
/// `every_document_path` sorts).
fn every_composition_document_path() -> Vec<String> {
    let full = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("workflows/fixtures/composition");
    let mut names: Vec<String> = std::fs::read_dir(&full)
        .unwrap_or_else(|err| panic!("{}: {err}", full.display()))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "yaml"))
        .map(|entry| {
            entry
                .file_name()
                .to_str()
                .expect("utf8 file name")
                .to_string()
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| format!("workflows/fixtures/composition/{name}"))
        .collect()
}

/// Synthesize a value for a declared workflow input that has no
/// default: its type's registry example, parsed the same way a literal
/// is. Mirrors `tests/it/acceptance.rs`'s own `synthesized_input`, kept
/// local on purpose -- that module's copy is private to it, and this
/// file already keeps its own helpers local throughout.
fn synthesized_input(ty: &TypeRef) -> Value {
    let entry = willikins_types::registry()
        .get(&ty.name)
        .unwrap_or_else(|| panic!("{ty}: declared input type is not registered"));
    if ty.list {
        Value::parse_list(ty, &[entry.info.example])
            .unwrap_or_else(|err| panic!("{ty}: example does not parse as a list element: {err}"))
    } else {
        Value::parse(ty, entry.info.example)
            .unwrap_or_else(|err| panic!("{ty}: own example does not parse as itself: {err}"))
    }
}

/// Every declared input of the *linked* `workflow` (so a fixed input,
/// decision (d6), is included -- it always carries its own default),
/// resolved from its default when it has one, else synthesized.
fn synthesized_inputs(workflow: &Workflow) -> IndexMap<InputName, Value> {
    workflow
        .inputs
        .iter()
        .map(|(name, spec)| {
            let value = spec
                .default
                .clone()
                .unwrap_or_else(|| synthesized_input(&spec.ty));
            (name.clone(), value)
        })
        .collect()
}

/// One composition document's whole characterization: its load result,
/// then its *link* result (against the same [`directory_resolver`] the
/// rest of this file uses, so a document's children are its siblings in
/// this same directory), then its check result, then a plan against
/// synthesized inputs. Rendered as plain text so an insta diff reads
/// directly as what changed.
fn characterize_composition(path: &str) -> String {
    use std::fmt::Write as _;

    let mut report = String::new();
    writeln!(report, "=== {path} ===").unwrap();

    let full = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    let workflow = match load_document(&full) {
        Ok(workflow) => workflow,
        Err(err) => {
            writeln!(report, "LOAD ERROR: {err}").unwrap();
            return report;
        }
    };

    let linked = match link(&workflow, &mut directory_resolver()) {
        Ok(linked) => linked,
        Err(errors) => {
            writeln!(report, "LINK ERRORS:").unwrap();
            for error in &errors {
                let json = serde_json::to_string(error).unwrap();
                writeln!(
                    report,
                    "- kind={} display={error} json={json}",
                    error.kind()
                )
                .unwrap();
            }
            return report;
        }
    };
    let used: Vec<String> = linked.used.iter().map(ToString::to_string).collect();
    writeln!(report, "USED: {}", used.join(", ")).unwrap();

    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = match check(&linked.workflow, &catalog) {
        Err(errors) => {
            writeln!(report, "CHECK ERRORS:").unwrap();
            for error in &errors {
                let json = serde_json::to_string(error).unwrap();
                writeln!(
                    report,
                    "- kind={} display={error} json={json}",
                    error.kind()
                )
                .unwrap();
            }
            return report;
        }
        Ok(checked) => checked,
    };

    writeln!(report, "WARNINGS:").unwrap();
    for warning in &checked.warnings {
        writeln!(report, "- {warning}").unwrap();
    }

    writeln!(report, "TYPES:").unwrap();
    for (node, ports) in &checked.types {
        for (port, edge) in ports {
            let ty = edge.ty();
            match edge.conversion() {
                None => writeln!(report, "{node}.{port}: {ty}").unwrap(),
                Some(conversion) => {
                    writeln!(report, "{node}.{port}: {ty} -> {}", conversion.to()).unwrap();
                }
            }
        }
    }

    writeln!(report, "OUTPUTS:").unwrap();
    for (name, ty) in &checked.output_types {
        writeln!(report, "{name}: {ty}").unwrap();
    }

    writeln!(report, "PLAN:").unwrap();
    let inputs = synthesized_inputs(&linked.workflow);
    match plan(&checked, &inputs, &catalog) {
        Err(err) => {
            writeln!(report, "PLAN ERROR: {err}").unwrap();
        }
        Ok(plan) => {
            let plan_json = serde_json::to_string(&plan).unwrap();
            let fingerprint_json = serde_json::to_string(&plan.fingerprint()).unwrap();
            writeln!(report, "plan_json: {plan_json}").unwrap();
            writeln!(report, "fingerprint: {fingerprint_json}").unwrap();
        }
    }

    report
}

/// The full characterization of every document under
/// `workflows/fixtures/composition/`, one at a time in sorted path
/// order -- every fixture under the public positive pair's own
/// directory, children included.
#[test]
fn characterization_of_every_composition_document() {
    let mut report = String::new();
    for path in every_composition_document_path() {
        report.push_str(&characterize_composition(&path));
        report.push('\n');
    }
    insta::assert_snapshot!(report);
}
