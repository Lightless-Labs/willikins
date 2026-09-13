//! Adversarial pass over [`willikins_core::plan`] and
//! [`willikins_core::describe`]: `plan` is the first stage that moves real
//! values through the graph, so this file attacks redaction (a seeded
//! secret flowing through several hops must never appear in any rendering
//! of a `Plan` or a `PlanError`), `for_each` expansion (empty sources,
//! duplicate keys), determinism, and the panic surface.

mod common;

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use common::{input, list_ty, node, output, port, tool_name, ty};
use willikins_core::{
    Action, Binding, Catalog, Class, InputSpec, Inputs, Node, Observation, Outputs, PartialInputs,
    PlanError, PortSpec, PortType, RawInput, SinkToken, Site, Tool, ToolError, ToolErrorKind,
    ToolSpec, TypeRef, Value, Workflow, check, describe, plan,
};
use willikins_providers_fake::{FakeState, catalog, empty};
use willikins_types::{
    DomainType, DopplerConfig, DopplerSecretValue, EnvironmentSlug, GitHubRepo, RepoVisibility,
    SecretName,
};

// -------------------------------------------------------------
// Dummy-tool scaffolding: scenarios no fake tool can produce
// -------------------------------------------------------------

/// A tool whose `read` result is fixed at construction.
struct Fixed {
    spec: ToolSpec,
    outcome: Outcome,
}

/// What [`Fixed::read`] reports.
enum Outcome {
    /// `Observation::Absent` with no predicted outputs.
    Absent,
    /// `Observation::Foreign`: the natural key exists and is not ours.
    Foreign,
    /// A `ToolError` whose message is the debug rendering of the inputs the
    /// tool was handed — the most hostile thing a tool can legally do with
    /// a secret input, since `ToolError` carries no `SinkToken`.
    LeakyError,
}

impl Tool for Fixed {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        match self.outcome {
            Outcome::Absent => Ok(Observation::Absent {
                predicted: Outputs::new(),
            }),
            Outcome::Foreign => Ok(Observation::Foreign),
            Outcome::LeakyError => Err(ToolError {
                kind: ToolErrorKind::Invalid,
                message: format!("refusing these inputs: {inputs:?}"),
            }),
        }
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
        Ok(Outputs::new())
    }
}

fn dummy_spec(
    name: &str,
    inputs: &[(&str, PortType, bool)],
    outputs: &[(&str, TypeRef)],
    key: &[&str],
    class: Class,
    pure: bool,
) -> ToolSpec {
    let mut in_map = IndexMap::new();
    for (port_name, port_ty, required) in inputs {
        in_map.insert(
            port(port_name),
            PortSpec {
                ty: port_ty.clone(),
                required: *required,
            },
        );
    }
    let mut out_map = IndexMap::new();
    for (port_name, port_ty) in outputs {
        out_map.insert(port(port_name), port_ty.clone());
    }
    ToolSpec {
        name: tool_name(name),
        description: format!("Dummy test tool `{name}`."),
        inputs: in_map,
        outputs: out_map,
        key: key.iter().map(|p| port(p)).collect(),
        class,
        pure,
    }
}

fn exact(name: &str) -> PortType {
    PortType::Exact(ty(name))
}

/// The three distinct seeded secret values used by the redaction tests.
/// High-entropy so a substring search cannot match them by accident.
const SECRET_BYTES: [&str; 3] = [
    "zqx-dev-7f3a91c4e8b2-never-print",
    "zqx-stg-11d0bb57aa93-never-print",
    "zqx-prd-4c8e2f60d1a7-never-print",
];

fn assert_no_secret_bytes(rendered: &str, what: &str) {
    for bytes in SECRET_BYTES {
        assert!(
            !rendered.contains(bytes),
            "{what} leaked a secret value: {rendered}"
        );
    }
}

// -------------------------------------------------------------
// A seeded secret flowing through for_each into three sinks
// -------------------------------------------------------------

/// State seeding `DATABASE_URL` in `third-thoughts/{dev,stg,prd}` with the
/// three [`SECRET_BYTES`] values.
fn seeded_secret_state() -> Arc<Mutex<FakeState>> {
    let name = SecretName::parse("DATABASE_URL").unwrap();
    let mut state = FakeState::new();
    for (env, bytes) in ["dev", "stg", "prd"].iter().zip(SECRET_BYTES) {
        let config = DopplerConfig::parse(&format!("third-thoughts/{env}")).unwrap();
        state =
            state.with_doppler_secret(&config, &name, DopplerSecretValue::parse(bytes).unwrap());
    }
    Arc::new(Mutex::new(state))
}

/// `configs` expands over `environments`; `secrets` expands over the
/// configs `configs` produced, reading one seeded `DopplerSecretValue` per
/// instance; three `ci_*` nodes each sink one of those secrets into
/// `github.actions_secret.ensure`'s `value` port with a `Keyed` reference;
/// and a workflow output aggregates all three into a secret list.
fn secret_fan_out_workflow() -> Workflow {
    let mut workflow = Workflow::new("secret-fan-out")
        .input(
            input("environments"),
            InputSpec::new(list_ty("EnvironmentSlug")),
        )
        .node(
            node("configs"),
            Node::new(tool_name("doppler.config.ensure"))
                .for_each(Binding::Input(input("environments")))
                .port(
                    port("project"),
                    Binding::Literal("third-thoughts".to_string()),
                )
                .port(port("environment"), Binding::Item),
        )
        .node(
            node("secrets"),
            Node::new(tool_name("doppler.secret.get"))
                .for_each(Binding::Step {
                    node: node("configs"),
                    port: port("config"),
                })
                .port(port("config"), Binding::Item)
                .port(port("name"), Binding::Literal("DATABASE_URL".to_string())),
        );
    for (env, secret_name) in [("dev", "DEV_URL"), ("stg", "STG_URL"), ("prd", "PRD_URL")] {
        workflow = workflow.node(
            node(&format!("ci_{env}")),
            Node::new(tool_name("github.actions_secret.ensure"))
                .port(
                    port("repo"),
                    Binding::Literal("lightless-labs/third-thoughts".to_string()),
                )
                .port(port("name"), Binding::Literal(secret_name.to_string()))
                .port(
                    port("value"),
                    Binding::Keyed {
                        node: node("secrets"),
                        key: format!("third-thoughts/{env}"),
                        port: port("value"),
                    },
                ),
        );
    }
    workflow.output(
        output("all_secrets"),
        Binding::Step {
            node: node("secrets"),
            port: port("value"),
        },
    )
}

fn three_environments() -> IndexMap<willikins_core::InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("environments"),
        Value::known_list(vec![
            EnvironmentSlug::parse("dev").unwrap(),
            EnvironmentSlug::parse("stg").unwrap(),
            EnvironmentSlug::parse("prd").unwrap(),
        ]),
    );
    inputs
}

#[test]
fn three_secrets_fanned_out_through_for_each_never_appear_in_the_plan() {
    let fake_catalog = catalog(seeded_secret_state());
    let workflow = secret_fan_out_workflow();
    let checked = check(&workflow, &fake_catalog).expect("the fan-out workflow checks cleanly");
    let result = plan(&checked, &three_environments(), &fake_catalog).expect("plans cleanly");

    // Three secret-bearing sinks, each holding a *known* secret: this test
    // would pass vacuously if the values were `Unknown`.
    for env in ["dev", "stg", "prd"] {
        let sink = result
            .nodes
            .iter()
            .find(|n| n.name.as_str() == format!("ci_{env}"))
            .unwrap_or_else(|| panic!("no planned node `ci_{env}`"));
        let value = sink.inputs.get(&port("value")).unwrap();
        assert!(value.is_known(), "ci_{env} value should be known");
        assert_eq!(value.render().to_string(), "[REDACTED DopplerSecretValue]");
    }
    let all = result.outputs.get(&output("all_secrets")).unwrap();
    assert!(all.is_known());
    assert_eq!(all.as_list().unwrap().len(), 3);

    let json = serde_json::to_string_pretty(&result).unwrap();
    assert_no_secret_bytes(&json, "plan JSON");
    assert!(json.contains("[REDACTED DopplerSecretValue]"), "{json}");
    assert_no_secret_bytes(&format!("{result:?}"), "plan Debug");
    assert_no_secret_bytes(&format!("{result:#?}"), "plan pretty Debug");
}

#[test]
fn a_tool_error_from_a_tool_handed_a_secret_never_shows_its_bytes() {
    let state = seeded_secret_state();
    let mut fake_catalog = catalog(state);
    fake_catalog
        .insert(Arc::new(Fixed {
            spec: dummy_spec(
                "test.leaky",
                &[("value", PortType::AnySecret, true)],
                &[],
                &[],
                Class::Reversible,
                false,
            ),
            outcome: Outcome::LeakyError,
        }))
        .unwrap();

    let workflow = Workflow::new("leaky")
        .node(
            node("get"),
            Node::new(tool_name("doppler.secret.get"))
                .port(
                    port("config"),
                    Binding::Literal("third-thoughts/dev".to_string()),
                )
                .port(port("name"), Binding::Literal("DATABASE_URL".to_string())),
        )
        .node(
            node("sink"),
            Node::new(tool_name("test.leaky")).port(
                port("value"),
                Binding::Step {
                    node: node("get"),
                    port: port("value"),
                },
            ),
        );
    let checked = check(&workflow, &fake_catalog).expect("an AnySecret sink checks cleanly");
    let err = plan(&checked, &IndexMap::new(), &fake_catalog).unwrap_err();
    match &err {
        PlanError::Tool { node: n, error } => {
            assert_eq!(*n, node("sink"));
            assert!(error.message.contains("REDACTED"), "{}", error.message);
        }
        other => panic!("expected Tool, got {other:?}"),
    }
    assert_no_secret_bytes(&err.to_string(), "PlanError Display");
    assert_no_secret_bytes(&format!("{err:?}"), "PlanError Debug");
    assert_no_secret_bytes(&serde_json::to_string(&err).unwrap(), "PlanError JSON");
}

#[test]
fn name_taken_carries_only_key_ports_never_a_secret_non_key_port() {
    let state = seeded_secret_state();
    let mut fake_catalog = catalog(state);
    fake_catalog
        .insert(Arc::new(Fixed {
            spec: dummy_spec(
                "test.foreign",
                &[
                    ("k", exact("GitHubOrg"), true),
                    ("value", PortType::AnySecret, true),
                ],
                &[],
                &["k"],
                Class::Reversible,
                false,
            ),
            outcome: Outcome::Foreign,
        }))
        .unwrap();

    let workflow = Workflow::new("foreign")
        .node(
            node("get"),
            Node::new(tool_name("doppler.secret.get"))
                .port(
                    port("config"),
                    Binding::Literal("third-thoughts/prd".to_string()),
                )
                .port(port("name"), Binding::Literal("DATABASE_URL".to_string())),
        )
        .node(
            node("taken"),
            Node::new(tool_name("test.foreign"))
                .port(port("k"), Binding::Literal("lightless-labs".to_string()))
                .port(
                    port("value"),
                    Binding::Step {
                        node: node("get"),
                        port: port("value"),
                    },
                ),
        );
    let checked = check(&workflow, &fake_catalog).unwrap();
    let err = plan(&checked, &IndexMap::new(), &fake_catalog).unwrap_err();
    match &err {
        PlanError::NameTaken { node: n, tool, key } => {
            assert_eq!(*n, node("taken"));
            assert_eq!(*tool, tool_name("test.foreign"));
            let ports: Vec<&str> = key.iter().map(|(name, _)| name.as_str()).collect();
            assert_eq!(ports, vec!["k"], "key must hold key ports only");
        }
        other => panic!("expected NameTaken, got {other:?}"),
    }
    assert_no_secret_bytes(&err.to_string(), "NameTaken Display");
    assert_no_secret_bytes(&format!("{err:?}"), "NameTaken Debug");
    assert_no_secret_bytes(&serde_json::to_string(&err).unwrap(), "NameTaken JSON");
}

// -------------------------------------------------------------
// for_each expansion: empty sources and duplicate keys
// -------------------------------------------------------------

/// `configs` over `environments`, with the whole instance list aggregated
/// into a workflow output — no `Keyed` reference, so an empty source is not
/// an error here.
fn configs_only_workflow() -> Workflow {
    Workflow::new("configs-only")
        .input(
            input("environments"),
            InputSpec::new(list_ty("EnvironmentSlug")),
        )
        .node(
            node("configs"),
            Node::new(tool_name("doppler.config.ensure"))
                .for_each(Binding::Input(input("environments")))
                .port(
                    port("project"),
                    Binding::Literal("third-thoughts".to_string()),
                )
                .port(port("environment"), Binding::Item),
        )
        .output(
            output("all_configs"),
            Binding::Step {
                node: node("configs"),
                port: port("config"),
            },
        )
}

fn environments(values: &[&str]) -> IndexMap<willikins_core::InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("environments"),
        Value::parse_list(&list_ty("EnvironmentSlug"), values).unwrap(),
    );
    inputs
}

#[test]
fn an_empty_for_each_source_plans_zero_instances_and_a_known_empty_list() {
    let (_state, fake_catalog) = empty();
    let workflow = configs_only_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let result = plan(&checked, &environments(&[]), &fake_catalog).expect("an empty list plans");

    assert!(
        result.nodes.is_empty(),
        "an empty source contributes no planned nodes: {:?}",
        result.nodes
    );
    let all = result.outputs.get(&output("all_configs")).unwrap();
    assert!(all.is_known(), "an empty aggregation is known, not unknown");
    assert!(all.as_list().unwrap().is_empty());
    assert_eq!(all.ty(), &list_ty("DopplerConfig"));
}

#[test]
fn a_keyed_reference_into_an_empty_for_each_is_key_not_in_for_each() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let mut inputs = common::new_rust_service_inputs();
    inputs.insert(
        input("environments"),
        Value::parse_list(&list_ty("EnvironmentSlug"), &[]).unwrap(),
    );
    let err = plan(&checked, &inputs, &fake_catalog).unwrap_err();
    match err {
        PlanError::KeyNotInForEach { site, key } => {
            assert_eq!(
                site,
                Site::Port {
                    node: node("token"),
                    port: port("config"),
                }
            );
            assert_eq!(key, "prd");
        }
        other => panic!("expected KeyNotInForEach, got {other:?}"),
    }
}

// -------------------------------------------------------------
// Determinism, panics, and classification
// -------------------------------------------------------------

#[test]
fn planning_twice_yields_identical_json() {
    let fake_catalog = catalog(seeded_secret_state());
    let workflow = secret_fan_out_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let inputs = three_environments();
    let first =
        serde_json::to_string_pretty(&plan(&checked, &inputs, &fake_catalog).unwrap()).unwrap();
    let second =
        serde_json::to_string_pretty(&plan(&checked, &inputs, &fake_catalog).unwrap()).unwrap();
    assert_eq!(first, second);
}

#[test]
fn missing_inputs_are_an_error_not_a_panic_even_when_nothing_is_supplied() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let err = plan(&checked, &IndexMap::new(), &fake_catalog).unwrap_err();
    assert!(matches!(err, PlanError::MissingInput { .. }), "{err:?}");
}

/// A catalog that does not hold the tools a `Checked` was checked against
/// is a caller-contract violation, documented on `plan` as a panic rather
/// than a `PlanError` (the plan document's `PlanError` list is closed).
#[test]
#[should_panic(expected = "compatible catalog")]
fn planning_against_a_catalog_missing_a_tool_panics_by_contract() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let other = Catalog::new(willikins_types::registry());
    let _ = plan(&checked, &common::new_rust_service_inputs(), &other);
}

/// The other half of the contract above: a workflow whose tools are not in
/// the catalog never reaches `plan` at all, because `check` rejects it.
#[test]
fn check_rejects_the_same_workflow_against_the_empty_catalog() {
    let workflow = common::new_rust_service_workflow();
    let errors = check(&workflow, &Catalog::new(willikins_types::registry()))
        .expect_err("no tool is registered");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, willikins_core::CheckError::UnknownTool { .. })),
        "{errors:?}"
    );
}

#[test]
fn naming_v1_is_compute_even_when_the_state_is_seeded() {
    let repo = GitHubRepo::parse("lightless-labs/third-thoughts").unwrap();
    let state = Arc::new(Mutex::new(FakeState::new().with_repo(
        &repo,
        RepoVisibility::Private,
        true,
    )));
    let fake_catalog = catalog(state);
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let result = plan(&checked, &common::new_rust_service_inputs(), &fake_catalog).unwrap();
    let names = result
        .nodes
        .iter()
        .find(|n| n.name.as_str() == "names")
        .unwrap();
    assert_eq!(names.action, Action::Compute);
}

#[test]
fn requires_approval_is_true_exactly_above_reversible() {
    for (class, expected) in [
        (Class::Reversible, false),
        (Class::Irreversible, true),
        (Class::Destructive, true),
    ] {
        let mut fake_catalog = Catalog::new(willikins_types::registry());
        fake_catalog
            .insert(Arc::new(Fixed {
                spec: dummy_spec("test.classified", &[], &[], &[], class, false),
                outcome: Outcome::Absent,
            }))
            .unwrap();
        let workflow =
            Workflow::new("classified").node(node("only"), Node::new(tool_name("test.classified")));
        let checked = check(&workflow, &fake_catalog).unwrap();
        let result = plan(&checked, &IndexMap::new(), &fake_catalog).unwrap();
        assert_eq!(result.class, class);
        assert_eq!(result.requires_approval, expected, "class {class:?}");
    }
}

// -------------------------------------------------------------
// describe: cardinality, defaults, and marker-shaped raw text
// -------------------------------------------------------------

/// Inputs only: `slug` (scalar, required), `environments`
/// (`list<EnvironmentSlug>`, defaulted), `note` (`Text`, required).
fn describe_workflow() -> Workflow {
    Workflow::new("describe-adversarial")
        .input(input("slug"), InputSpec::new(ty("ProjectSlug")))
        .input(
            input("environments"),
            InputSpec::new(list_ty("EnvironmentSlug")).with_default(Value::known_list(vec![
                EnvironmentSlug::parse("dev").unwrap(),
                EnvironmentSlug::parse("prd").unwrap(),
            ])),
        )
        .input(input("note"), InputSpec::new(ty("Text")))
}

fn describe_checked() -> willikins_core::Checked {
    check(
        &describe_workflow(),
        &Catalog::new(willikins_types::registry()),
    )
    .expect("an input-only workflow checks cleanly")
}

#[test]
fn describe_rejects_a_scalar_raw_value_for_a_list_input() {
    let checked = describe_checked();
    let mut partial = PartialInputs::new();
    partial.insert(
        input("environments"),
        RawInput::Scalar("dev,prd".to_string()),
    );
    let description = describe(&checked, &partial);
    assert!(!description.resolved.contains_key(&input("environments")));
    let error = description
        .errors
        .iter()
        .find(|e| e.input == input("environments"))
        .expect("a scalar raw value for a list input is a cardinality error");
    assert!(error.error.to_string().contains("list"), "{}", error.error);
}

#[test]
fn describe_rejects_a_list_raw_value_for_a_scalar_input() {
    let checked = describe_checked();
    let mut partial = PartialInputs::new();
    partial.insert(
        input("slug"),
        RawInput::List(vec!["third-thoughts".to_string(), "other".to_string()]),
    );
    let description = describe(&checked, &partial);
    assert!(!description.resolved.contains_key(&input("slug")));
    let error = description
        .errors
        .iter()
        .find(|e| e.input == input("slug"))
        .expect("a list raw value for a scalar input is a cardinality error");
    assert!(
        error.error.to_string().contains("scalar"),
        "{}",
        error.error
    );
}

#[test]
fn describe_resolves_a_list_default_and_never_reports_it_missing() {
    let checked = describe_checked();
    let description = describe(&checked, &PartialInputs::new());
    let resolved = description
        .resolved
        .get(&input("environments"))
        .expect("a defaulted list input resolves from its default");
    assert_eq!(resolved.render().to_string(), "[dev, prd]");
    assert!(
        !description
            .missing
            .iter()
            .any(|m| m.name == input("environments"))
    );
    let missing: Vec<&str> = description
        .missing
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(missing, vec!["slug", "note"]);
}

#[test]
fn describe_accepts_marker_shaped_text_as_an_ordinary_text_value() {
    let checked = describe_checked();
    let marker = "[REDACTED DopplerServiceToken]";
    let mut partial = PartialInputs::new();
    partial.insert(input("note"), RawInput::Scalar(marker.to_string()));
    let description = describe(&checked, &partial);
    assert!(description.errors.is_empty(), "{:?}", description.errors);
    let note = description.resolved.get(&input("note")).unwrap();
    assert!(note.is_known());
    // It is plain `Text`, not a secret: it renders back as itself, and is
    // not mistaken for a redaction by any rendering path.
    assert_eq!(note.render().to_string(), marker);
    assert!(!note.ty().list);
    let json = serde_json::to_string(&description).unwrap();
    assert!(!json.contains("\"redacted\""), "{json}");
}
#[test]
fn two_for_each_items_rendering_to_the_same_key_are_rejected() {
    let (_state, fake_catalog) = empty();
    let workflow = configs_only_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let err = plan(
        &checked,
        &environments(&["dev", "dev", "prd"]),
        &fake_catalog,
    )
    .expect_err("two instances with the same key are ambiguous and must be refused");
    match err {
        PlanError::DuplicateForEachKey { node: n, key } => {
            assert_eq!(n, node("configs"));
            assert_eq!(key, "dev");
        }
        other => panic!("expected DuplicateForEachKey, got {other:?}"),
    }
}

#[test]
fn describe_echoes_a_rejected_non_secret_raw_value_and_never_sees_a_secret_one() {
    // A non-secret domain type's own parser names the offending text, and
    // `describe` passes that `ParseError` through untouched: the value is
    // the caller's own, and an agent needs to see what it got wrong.
    let checked = describe_checked();
    let mut partial = PartialInputs::new();
    partial.insert(
        input("slug"),
        RawInput::Scalar("Not A Slug At All!".to_string()),
    );
    let description = describe(&checked, &partial);
    let error = &description.errors[0];
    assert_eq!(error.input, input("slug"));
    assert!(
        error.error.to_string().contains("Not A Slug At All!"),
        "{}",
        error.error
    );

    // The safety of that echo rests on a secret-typed input never reaching
    // a parser at all: `check` refuses to produce a `Checked` for one.
    let secret_workflow = Workflow::new("secret-input")
        .input(input("token"), InputSpec::new(ty("DopplerServiceToken")));
    let errors = check(&secret_workflow, &Catalog::new(willikins_types::registry()))
        .expect_err("a secret-typed workflow input is refused");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, willikins_core::CheckError::SecretWorkflowInput { .. })),
        "{errors:?}"
    );

    // And the registry itself refuses a secret type before looking at the
    // text, so even a direct parse cannot echo one.
    let refusal = Value::parse(
        &ty("DopplerServiceToken"),
        "dp.st.prd.realtokenbytesaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap_err();
    assert!(!refusal.to_string().contains("realtokenbytes"), "{refusal}");
}

/// `plan` resolves a `for_each` source with no item in hand, so its
/// `resolve_binding` treats `Binding::Literal` and `Binding::Item` there as
/// `unreachable!`. That is only sound because `check` refuses both before a
/// `Checked` can exist — pinned here, since a regression in `check` would
/// turn into a panic in `plan`.
#[test]
fn check_refuses_the_for_each_sources_plan_treats_as_unreachable() {
    let (_state, fake_catalog) = empty();
    let over = |source: Binding| {
        Workflow::new("fe").node(
            node("configs"),
            Node::new(tool_name("doppler.config.ensure"))
                .for_each(source)
                .port(
                    port("project"),
                    Binding::Literal("third-thoughts".to_string()),
                )
                .port(port("environment"), Binding::Item),
        )
    };

    let errors = check(&over(Binding::Literal("dev".to_string())), &fake_catalog)
        .expect_err("a literal for_each source is a scalar");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, willikins_core::CheckError::ForEachOverScalar { .. })),
        "{errors:?}"
    );

    let errors = check(&over(Binding::Item), &fake_catalog)
        .expect_err("`item` cannot be its own for_each source");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, willikins_core::CheckError::ItemOutsideForEach { .. })),
        "{errors:?}"
    );
}
