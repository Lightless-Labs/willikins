//! Every reachable (error variant, [`Site`] form) pair, written as a real
//! YAML document and run through `check` (or `plan`) against the fake
//! catalog.
//!
//! Task 1b replaced the `outputs` node sentinel and the `for_each` port
//! sentinel with `Site`. `check.rs`'s own unit tests construct every
//! `CheckError` variant with a `Site::Port`, which is exactly the form the
//! old sentinels were confusable with; this file covers the other two
//! forms, from documents rather than from hand-built values. Each case
//! asserts both the constructed `Site` and the `kind` tag the CLI
//! publishes, since unambiguous JSON is what acceptance test 16 asks for.
//!
//! Two pairs are deliberately absent:
//!
//! - `CheckError::SecretToNonSecretSink`'s `to` is only ever built inside
//!   `check_with_port`, so it is a `Site::Port` by construction and no
//!   document can site it anywhere else.
//! - `PlanError::ForEachUnknown` needs a `for_each` source that is a known
//!   list *type* with an unknown *value*. The only list-typed output port
//!   in the fake catalog is `fake.secret_list`'s secret `tokens`, which
//!   `check` refuses first (`SecretForEachSource`), so no document reaches
//!   it; `willikins-core/tests/plan.rs` covers it with dummy tools. It
//!   carries a bare `node` and no `Site` either way.

use indexmap::IndexMap;
use willikins_core::{
    CheckError, InputName, NodeName, OutputName, PlanError, PortName, Site, TypeName, TypeRef,
    Value, Workflow,
};
use willikins_dsl::parse_document;

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn output(name: &str) -> OutputName {
    OutputName::parse(name).unwrap()
}

fn parse(source: &str) -> Workflow {
    parse_document(source).unwrap_or_else(|err| panic!("document must parse: {err}"))
}

/// `check` `source` against the fake catalog, asserting it fails with
/// exactly one error, and return that error.
fn one_check_error(source: &str) -> CheckError {
    let workflow = parse(source);
    let (_state, catalog) = willikins_providers_fake::empty();
    let mut errors =
        willikins_core::check(&workflow, &catalog).expect_err("document must fail check");
    assert_eq!(errors.len(), 1, "expected one error, got {errors:?}");
    errors.remove(0)
}

/// The `site` field of `error`'s published JSON.
fn site_json(error: &CheckError) -> serde_json::Value {
    serde_json::to_value(error).expect("CheckError serializes")["site"].clone()
}

/// `check` `source`, then `plan` it with `supplied`, asserting the check
/// passes and the plan fails.
fn one_plan_error(source: &str, supplied: &IndexMap<InputName, Value>) -> PlanError {
    let workflow = parse(source);
    let (_state, catalog) = willikins_providers_fake::empty();
    let checked = willikins_core::check(&workflow, &catalog)
        .unwrap_or_else(|errors| panic!("document must check cleanly: {errors:?}"));
    willikins_core::plan(&checked, supplied, &catalog).expect_err("plan must fail")
}

fn scalar_input(name: &str, ty: &str, text: &str) -> (InputName, Value) {
    let ty = TypeRef::scalar(TypeName::parse(ty).unwrap());
    (
        InputName::parse(name).unwrap(),
        Value::parse(&ty, text).unwrap(),
    )
}

fn list_input(name: &str, ty: &str, items: &[&str]) -> (InputName, Value) {
    let ty = TypeRef::list_of(TypeName::parse(ty).unwrap());
    (
        InputName::parse(name).unwrap(),
        Value::parse_list(&ty, items).unwrap(),
    )
}

// ---------------------------------------------------------------------
// Site::ForEach: a node's own `for_each` binding
// ---------------------------------------------------------------------

#[test]
fn item_in_a_for_each_source_is_sited_at_the_for_each_binding() {
    let error = one_check_error(
        "\
name: item-in-for-each
inputs:
  project: { type: DopplerProject }
steps:
  configs:
    tool: doppler.config.ensure
    for_each: ${{ item }}
    with:
      project: ${{ inputs.project }}
      environment: ${{ item }}
",
    );
    assert_eq!(
        error,
        CheckError::ItemOutsideForEach {
            site: Site::ForEach {
                node: node("configs")
            },
        }
    );
    assert_eq!(site_json(&error)["kind"], "for_each");
}

#[test]
fn an_undeclared_input_in_a_for_each_source_is_sited_at_the_for_each_binding() {
    let error = one_check_error(
        "\
name: undeclared-in-for-each
inputs:
  project: { type: DopplerProject }
steps:
  configs:
    tool: doppler.config.ensure
    for_each: ${{ inputs.nope }}
    with:
      project: ${{ inputs.project }}
      environment: ${{ item }}
",
    );
    assert_eq!(
        error,
        CheckError::UndeclaredInput {
            site: Site::ForEach {
                node: node("configs")
            },
            input: InputName::parse("nope").unwrap(),
        }
    );
    assert_eq!(site_json(&error)["kind"], "for_each");
}

#[test]
fn an_unknown_node_in_a_for_each_source_is_sited_at_the_for_each_binding() {
    let error = one_check_error(
        "\
name: unknown-node-in-for-each
inputs:
  project: { type: DopplerProject }
steps:
  configs:
    tool: doppler.config.ensure
    for_each: ${{ steps.ghost.tokens }}
    with:
      project: ${{ inputs.project }}
      environment: ${{ item }}
",
    );
    assert_eq!(
        error,
        CheckError::UnknownNode {
            site: Site::ForEach {
                node: node("configs")
            },
            referenced: node("ghost"),
        }
    );
    assert_eq!(site_json(&error)["kind"], "for_each");
}

#[test]
fn a_keyed_for_each_source_onto_a_scalar_node_is_sited_at_the_for_each_binding() {
    let error = one_check_error(
        "\
name: keyed-scalar-in-for-each
inputs:
  project: { type: DopplerProject }
steps:
  doppler:
    tool: doppler.project.ensure
    with:
      project: ${{ inputs.project }}
  configs:
    tool: doppler.config.ensure
    for_each: ${{ steps.doppler[prd].project }}
    with:
      project: ${{ inputs.project }}
      environment: ${{ item }}
",
    );
    assert_eq!(
        error,
        CheckError::KeyedOnScalarNode {
            site: Site::ForEach {
                node: node("configs")
            },
            referenced: node("doppler"),
        }
    );
    assert_eq!(site_json(&error)["kind"], "for_each");
}

/// A `for_each` whose source is a `Step` onto another `for_each` node with
/// an already-list-typed output port: the promotion would need a
/// `list<list<T>>`. `NestedList` is reported inside `resolve_reference`,
/// before the secrecy check `fake.secret_list`'s port would also trip.
#[test]
fn a_nested_list_for_each_source_is_sited_at_the_for_each_binding() {
    let error = one_check_error(
        "\
name: nested-list-in-for-each
inputs:
  project: { type: DopplerProject }
  sources: { type: list<DopplerConfig> }
steps:
  lister:
    tool: fake.secret_list
    for_each: ${{ inputs.sources }}
    with:
      config: ${{ item }}
  loop:
    tool: doppler.config.ensure
    for_each: ${{ steps.lister.tokens }}
    with:
      project: ${{ inputs.project }}
      environment: ${{ item }}
",
    );
    assert_eq!(
        error,
        CheckError::NestedList {
            site: Site::ForEach { node: node("loop") },
            referenced: node("lister"),
        }
    );
    assert_eq!(site_json(&error)["kind"], "for_each");
}

// ---------------------------------------------------------------------
// Site::Output: a workflow output's own binding
// ---------------------------------------------------------------------

#[test]
fn an_undeclared_input_in_an_output_is_sited_at_the_output() {
    let error = one_check_error(
        "\
name: undeclared-in-output
steps:
  names:
    tool: naming.v1
    with:
      org: lightless-labs
      slug: demo
outputs:
  x: ${{ inputs.nope }}
",
    );
    assert_eq!(
        error,
        CheckError::UndeclaredInput {
            site: Site::Output { name: output("x") },
            input: InputName::parse("nope").unwrap(),
        }
    );
    assert_eq!(site_json(&error)["kind"], "output");
}

#[test]
fn item_in_an_output_is_sited_at_the_output() {
    let error = one_check_error(
        "\
name: item-in-output
steps:
  names:
    tool: naming.v1
    with:
      org: lightless-labs
      slug: demo
outputs:
  x: ${{ item }}
",
    );
    assert_eq!(
        error,
        CheckError::ItemOutsideForEach {
            site: Site::Output { name: output("x") },
        }
    );
    assert_eq!(site_json(&error)["kind"], "output");
}

#[test]
fn a_keyed_output_onto_a_scalar_node_is_sited_at_the_output() {
    let error = one_check_error(
        "\
name: keyed-scalar-in-output
steps:
  names:
    tool: naming.v1
    with:
      org: lightless-labs
      slug: demo
outputs:
  x: ${{ steps.names[prd].github_repo }}
",
    );
    assert_eq!(
        error,
        CheckError::KeyedOnScalarNode {
            site: Site::Output { name: output("x") },
            referenced: node("names"),
        }
    );
    assert_eq!(site_json(&error)["kind"], "output");
}

#[test]
fn a_nested_list_output_is_sited_at_the_output() {
    let error = one_check_error(
        "\
name: nested-list-in-output
inputs:
  sources: { type: list<DopplerConfig> }
steps:
  lister:
    tool: fake.secret_list
    for_each: ${{ inputs.sources }}
    with:
      config: ${{ item }}
outputs:
  x: ${{ steps.lister.tokens }}
",
    );
    assert_eq!(
        error,
        CheckError::NestedList {
            site: Site::Output { name: output("x") },
            referenced: node("lister"),
        }
    );
    assert_eq!(site_json(&error)["kind"], "output");
}

/// `UnknownPort`'s referenced-node form sites the *looked-up* node and
/// port, not the binding's own location -- even when the binding lives in
/// a workflow output and the referenced node is literally named `outputs`.
/// The `kind` is `port` because the port was looked up on a node; nothing
/// here is a workflow-output site.
#[test]
fn an_unknown_output_port_on_a_step_named_outputs_sites_the_referenced_node() {
    let error = one_check_error(
        "\
name: unknown-port-on-step-named-outputs
steps:
  outputs:
    tool: naming.v1
    with:
      org: lightless-labs
      slug: demo
outputs:
  x: ${{ steps.outputs.nope }}
",
    );
    assert_eq!(
        error,
        CheckError::UnknownPort {
            site: Site::Port {
                node: node("outputs"),
                port: port("nope"),
            },
            tool: willikins_core::ToolName::parse("naming.v1").unwrap(),
        }
    );
    assert_eq!(
        site_json(&error),
        serde_json::json!({"kind": "port", "node": "outputs", "port": "nope"})
    );
}

// ---------------------------------------------------------------------
// plan time
// ---------------------------------------------------------------------

/// `PlanError::KeyNotInForEach` names the *referencing* site. When the
/// reference lives in a workflow output, that site is `Site::Output` --
/// which, before the enum, had no honest node name to report.
#[test]
fn a_keyed_output_with_no_matching_instance_is_sited_at_the_output() {
    let supplied: IndexMap<InputName, Value> = [
        scalar_input("project", "DopplerProject", "third-thoughts"),
        list_input("environments", "EnvironmentSlug", &["dev"]),
    ]
    .into_iter()
    .collect();

    let error = one_plan_error(
        "\
name: keyed-output-missing-instance
inputs:
  project: { type: DopplerProject }
  environments: { type: list<EnvironmentSlug> }
steps:
  configs:
    tool: doppler.config.ensure
    for_each: ${{ inputs.environments }}
    with:
      project: ${{ inputs.project }}
      environment: ${{ item }}
outputs:
  x: ${{ steps.configs[prd].config }}
",
        &supplied,
    );

    match &error {
        PlanError::KeyNotInForEach { site, key } => {
            assert_eq!(site, &Site::Output { name: output("x") });
            assert_eq!(key, "prd");
        }
        other => panic!("expected KeyNotInForEach, got {other:?}"),
    }
    let json = serde_json::to_value(&error).expect("PlanError serializes");
    assert_eq!(json["site"]["kind"], "output");
}

/// `PlanError::DuplicateForEachKey` names a node and nothing else, so a
/// node literally named `outputs` cannot be mistaken for a workflow
/// output's site: there is no `site` field to mistake.
#[test]
fn duplicate_for_each_keys_name_the_node_even_when_it_is_called_outputs() {
    let supplied: IndexMap<InputName, Value> = [
        scalar_input("project", "DopplerProject", "third-thoughts"),
        list_input("environments", "EnvironmentSlug", &["dev", "dev"]),
    ]
    .into_iter()
    .collect();

    let error = one_plan_error(
        "\
name: duplicate-for-each-key
inputs:
  project: { type: DopplerProject }
  environments: { type: list<EnvironmentSlug> }
steps:
  outputs:
    tool: doppler.config.ensure
    for_each: ${{ inputs.environments }}
    with:
      project: ${{ inputs.project }}
      environment: ${{ item }}
",
        &supplied,
    );

    match &error {
        PlanError::DuplicateForEachKey { node: name, key } => {
            assert_eq!(name, &node("outputs"));
            assert_eq!(key, "dev");
        }
        other => panic!("expected DuplicateForEachKey, got {other:?}"),
    }
    let json = serde_json::to_value(&error).expect("PlanError serializes");
    assert_eq!(json["node"], "outputs");
    assert!(json.get("site").is_none(), "json: {json}");
}
