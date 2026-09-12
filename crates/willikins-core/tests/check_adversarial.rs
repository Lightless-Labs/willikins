//! Adversarial pass on [`willikins_core::check`] (acceptance test 12, first
//! pass; see `docs/research/2026-09-12-check-adversarial-pass-1.md`).
//!
//! Every test here is an attempt to build a [`Workflow`] that `check`
//! accepts and that either moves a secret into a non-secret place, or that
//! a later stage (`describe`, `plan`) could not execute; or one that
//! `check` rejects when it should not. Attacks that found a real defect
//! name the fix; attacks that found none pin the behaviour so a later
//! refactor cannot quietly introduce one.
//!
//! The catalog mirrors `tests/check.rs`'s (the plan's fake-provider port
//! table), plus two tools that table has no row for and that only an
//! adversary would reach for: `fake.text_list` (a *non-secret* list
//! output) and `fake.text_sink` (a tool with a `list<Text>` input port).

use std::sync::Arc;

use indexmap::IndexMap;
use proptest::prelude::*;
use willikins_core::{
    Binding, Catalog, CheckError, Checked, Class, InputName, InputSpec, Inputs, Node, NodeName,
    Observation, OutputName, Outputs, PortName, PortSpec, PortType, Site, Tool, ToolError,
    ToolName, ToolSpec, TypeName, TypeRef, Value, Workflow, check,
};
use willikins_types::{
    DomainType, DopplerServiceToken, EnvironmentSlug, RepoVisibility, SinkToken,
};

fn ty(name: &str) -> TypeRef {
    TypeRef::scalar(TypeName::parse(name).unwrap())
}

fn list_ty(name: &str) -> TypeRef {
    TypeRef::list_of(TypeName::parse(name).unwrap())
}

fn exact(name: &str) -> PortType {
    PortType::Exact(ty(name))
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
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

/// A secret value, constructed on the concrete type (the registry refuses
/// to parse a secret type from a string, by design).
fn secret_value() -> Value {
    Value::known(DopplerServiceToken::parse("dp.st.fake-secret-bytes").unwrap())
}

/// A tool whose spec is fixed at construction; `check` never calls either
/// method.
struct DummyTool {
    spec: ToolSpec,
}

impl Tool for DummyTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Outputs, ToolError> {
        Ok(Outputs::new())
    }
}

fn spec_of(
    name: &str,
    inputs: &[(&str, PortType, bool)],
    outputs: &[(&str, TypeRef)],
    key: &[&str],
    class: Class,
    pure: bool,
) -> ToolSpec {
    let mut in_map = IndexMap::new();
    for (name, ty, required) in inputs {
        in_map.insert(
            port(name),
            PortSpec {
                ty: ty.clone(),
                required: *required,
            },
        );
    }
    let mut out_map = IndexMap::new();
    for (name, ty) in outputs {
        out_map.insert(port(name), ty.clone());
    }
    ToolSpec {
        name: tool_name(name),
        description: format!("Test double for `{name}`."),
        inputs: in_map,
        outputs: out_map,
        key: key.iter().map(|p| port(p)).collect(),
        class,
        pure,
    }
}

#[allow(clippy::too_many_lines)]
fn test_catalog() -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    let specs = vec![
        spec_of(
            "naming.v1",
            &[
                ("org", exact("GitHubOrg"), true),
                ("slug", exact("ProjectSlug"), true),
            ],
            &[
                ("github_repo", ty("GitHubRepo")),
                ("doppler_project", ty("DopplerProject")),
            ],
            &[],
            Class::Reversible,
            true,
        ),
        spec_of(
            "github.repo.ensure",
            &[
                ("repo", exact("GitHubRepo"), true),
                ("visibility", exact("RepoVisibility"), true),
            ],
            &[("repo", ty("GitHubRepo")), ("url", ty("HttpsUrl"))],
            &["repo"],
            Class::Reversible,
            false,
        ),
        spec_of(
            "github.actions_secret.ensure",
            &[
                ("repo", exact("GitHubRepo"), true),
                ("name", exact("ActionsSecretName"), true),
                ("value", PortType::AnySecret, true),
            ],
            &[],
            &["repo", "name"],
            Class::Reversible,
            false,
        ),
        spec_of(
            "doppler.project.ensure",
            &[("project", exact("DopplerProject"), true)],
            &[("project", ty("DopplerProject"))],
            &["project"],
            Class::Reversible,
            false,
        ),
        spec_of(
            "doppler.config.ensure",
            &[
                ("project", exact("DopplerProject"), true),
                ("environment", exact("EnvironmentSlug"), true),
            ],
            &[("config", ty("DopplerConfig"))],
            &["project", "environment"],
            Class::Reversible,
            false,
        ),
        spec_of(
            "doppler.service_token.ensure",
            &[
                ("config", exact("DopplerConfig"), true),
                ("name", exact("DopplerTokenName"), true),
            ],
            &[("token", ty("DopplerServiceToken"))],
            &["config", "name"],
            Class::Reversible,
            false,
        ),
        spec_of(
            "doppler.secret.get",
            &[
                ("config", exact("DopplerConfig"), true),
                ("name", exact("SecretName"), true),
            ],
            &[("value", ty("DopplerSecretValue"))],
            &[],
            Class::Reversible,
            true,
        ),
        spec_of(
            "fake.secret_list",
            &[("config", exact("DopplerConfig"), true)],
            &[("tokens", list_ty("DopplerServiceToken"))],
            &[],
            Class::Reversible,
            true,
        ),
        spec_of(
            "fake.irreversible.ensure",
            &[("key", exact("ProjectSlug"), true)],
            &[],
            &["key"],
            Class::Irreversible,
            false,
        ),
        // Not in the plan's port table: a non-secret *list* output, so the
        // `Step`-on-a-`for_each`-node list promotion has something to
        // double up on.
        spec_of(
            "fake.text_list",
            &[("seed", exact("Text"), true)],
            &[("lines", list_ty("Text"))],
            &[],
            Class::Reversible,
            true,
        ),
        // Not in the plan's port table: a port that accepts a list, so a
        // promoted `list<T>` has somewhere to land.
        spec_of(
            "fake.text_sink",
            &[("lines", PortType::Exact(list_ty("Text")), true)],
            &[],
            &[],
            Class::Reversible,
            true,
        ),
        spec_of(
            "template.render",
            &[
                ("template", exact("TemplateSource"), true),
                ("value", exact("Text"), true),
            ],
            &[("rendered", ty("Text"))],
            &[],
            Class::Reversible,
            true,
        ),
    ];
    for spec in specs {
        catalog.insert(Arc::new(DummyTool { spec })).unwrap();
    }
    catalog
}

/// The errors of a check that must fail, or a panic naming what it
/// wrongly accepted.
fn errors(workflow: &Workflow) -> Vec<CheckError> {
    match check(workflow, &test_catalog()) {
        Ok(checked) => panic!("expected check to reject this workflow, got {checked:?}"),
        Err(errors) => errors,
    }
}

/// The [`Checked`] of a check that must succeed, or a panic naming the
/// errors.
fn checked(workflow: &Workflow) -> Checked {
    match check(workflow, &test_catalog()) {
        Ok(checked) => checked,
        Err(errors) => panic!("expected check to accept this workflow, got {errors:?}"),
    }
}

/// A node calling `doppler.secret.get`, the pure tool whose `value` output
/// is a known secret; both its ports bound to literals.
fn secret_source() -> Node {
    Node::new(tool_name("doppler.secret.get"))
        .port(port("config"), Binding::Literal("acme-web/prd".to_string()))
        .port(port("name"), Binding::Literal("API_KEY".to_string()))
}

// ---------------------------------------------------------------------
// Attack: a secret reaching `template.render` through `for_each`.
// ---------------------------------------------------------------------

/// `Keyed` into a `for_each` node whose output port is a secret list: the
/// key selects an instance, so the binding still resolves to that
/// instance's `list<DopplerServiceToken>`, which is secret. It must not
/// reach `template.render`'s non-secret `value`.
#[test]
fn keyed_into_a_for_each_node_with_a_secret_list_output_cannot_feed_template_render() {
    let workflow = Workflow::new("keyed-secret-list")
        .input(input("environments"), InputSpec::new(list_ty("Text")))
        .node(node("secrets"), {
            Node::new(tool_name("fake.secret_list"))
                .for_each(Binding::Input(input("environments")))
                .port(port("config"), Binding::Literal("acme-web/prd".to_string()))
        })
        .node(
            node("readme"),
            Node::new(tool_name("template.render"))
                .port(
                    port("template"),
                    Binding::Literal("{{ value }}".to_string()),
                )
                .port(
                    port("value"),
                    Binding::Keyed {
                        node: node("secrets"),
                        key: "dev".to_string(),
                        port: port("tokens"),
                    },
                ),
        );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::SecretToNonSecretSink {
            from: (node("secrets"), port("tokens")),
            to: Site::Port {
                node: node("readme"),
                port: port("value")
            },
        }]
    );
}

/// `Step` into a `for_each` node whose element output is secret: the
/// binding is promoted to `list<DopplerServiceToken>`, still secret.
#[test]
fn step_into_a_for_each_node_with_a_secret_element_cannot_feed_template_render() {
    let workflow = Workflow::new("step-secret-elements")
        .input(input("environments"), InputSpec::new(list_ty("Text")))
        .node(
            node("tokens"),
            Node::new(tool_name("doppler.service_token.ensure"))
                .for_each(Binding::Input(input("environments")))
                .port(port("config"), Binding::Literal("acme-web/prd".to_string()))
                .port(port("name"), Binding::Literal("ci".to_string())),
        )
        .node(
            node("readme"),
            Node::new(tool_name("template.render"))
                .port(
                    port("template"),
                    Binding::Literal("{{ value }}".to_string()),
                )
                .port(
                    port("value"),
                    Binding::Step {
                        node: node("tokens"),
                        port: port("token"),
                    },
                ),
        );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::SecretToNonSecretSink {
            from: (node("tokens"), port("token")),
            to: Site::Port {
                node: node("readme"),
                port: port("value")
            },
        }]
    );
}

/// A `for_each` over a workflow input whose element type is secret. The
/// task brief expected `SecretForEachSource`; the implementation reports
/// the *root cause* instead — a secret workflow input is refused where it
/// is declared, and the `for_each` that consumes it is cascade-suppressed.
/// Pinned as-is: one error naming the real problem beats two naming a
/// symptom. Noted as a task-brief/implementation discrepancy in the
/// research note.
#[test]
fn for_each_over_a_secret_workflow_input_reports_only_the_secret_input() {
    let workflow = Workflow::new("secret-input-for-each")
        .input(
            input("tokens"),
            InputSpec::new(list_ty("DopplerServiceToken")),
        )
        .node(
            node("ci_secret"),
            Node::new(tool_name("github.actions_secret.ensure"))
                .for_each(Binding::Input(input("tokens")))
                .port(
                    port("repo"),
                    Binding::Literal("lightless-labs/acme-web".to_string()),
                )
                .port(port("name"), Binding::Literal("DOPPLER_TOKEN".to_string()))
                .port(port("value"), Binding::Item),
        );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::SecretWorkflowInput {
            input: input("tokens"),
            ty: list_ty("DopplerServiceToken"),
        }]
    );
}

// ---------------------------------------------------------------------
// Attack: secrets and literals at the edges.
// ---------------------------------------------------------------------

/// The redaction marker string is not magic: bound to a `Text` port it is
/// an ordinary literal, accepted, and resolves to `Text`. Nothing
/// special-cases it, which is the intended behaviour — the marker is what
/// redaction *prints*, never what it recognises.
#[test]
fn the_redaction_marker_string_is_an_ordinary_text_literal() {
    let marker = "[REDACTED DopplerServiceToken]";
    let workflow = Workflow::new("marker-literal").node(
        node("readme"),
        Node::new(tool_name("template.render"))
            .port(
                port("template"),
                Binding::Literal("{{ value }}".to_string()),
            )
            .port(port("value"), Binding::Literal(marker.to_string())),
    );

    let checked = checked(&workflow);
    assert_eq!(checked.types[&node("readme")][&port("value")], ty("Text"));
}

/// A secret bound to a non-secret sink *and* type-mismatched on the same
/// edge reports only the taint violation: the security error is never
/// hidden behind a type error.
#[test]
fn a_tainted_edge_reports_the_taint_violation_and_not_the_type_mismatch() {
    let workflow = Workflow::new("taint-beats-mismatch")
        .node(node("api_key"), secret_source())
        .node(
            node("repo"),
            Node::new(tool_name("github.repo.ensure"))
                // `DopplerSecretValue` is neither `GitHubRepo` (a type
                // mismatch) nor acceptable on a non-secret port (a taint
                // violation). Only the latter is reported.
                .port(
                    port("repo"),
                    Binding::Step {
                        node: node("api_key"),
                        port: port("value"),
                    },
                )
                .port(port("visibility"), Binding::Literal("private".to_string())),
        );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::SecretToNonSecretSink {
            from: (node("api_key"), port("value")),
            to: Site::Port {
                node: node("repo"),
                port: port("repo")
            },
        }]
    );
}

// ---------------------------------------------------------------------
// Attack: reference shapes.
// ---------------------------------------------------------------------

/// A `Keyed` binding naming an output port the referenced `for_each`
/// node's tool does not have: the error names the *referenced* node,
/// whose spec is missing the port.
#[test]
fn keyed_reference_to_a_missing_port_on_a_for_each_node_names_the_referenced_node() {
    let workflow = Workflow::new("keyed-missing-port")
        .input(
            input("environments"),
            InputSpec::new(list_ty("EnvironmentSlug")),
        )
        .node(
            node("configs"),
            Node::new(tool_name("doppler.config.ensure"))
                .for_each(Binding::Input(input("environments")))
                .port(port("project"), Binding::Literal("acme-web".to_string()))
                .port(port("environment"), Binding::Item),
        )
        .node(
            node("token"),
            Node::new(tool_name("doppler.service_token.ensure"))
                .port(
                    port("config"),
                    Binding::Keyed {
                        node: node("configs"),
                        key: "prd".to_string(),
                        port: port("nope"),
                    },
                )
                .port(port("name"), Binding::Literal("ci".to_string())),
        );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::UnknownPort {
            site: Site::Port {
                node: node("configs"),
                port: port("nope"),
            },
            tool: tool_name("doppler.config.ensure"),
        }]
    );
}

/// A duplicate `with` key cannot exist: `IndexMap` collapses it, keeping
/// the key's position and the last binding. Pinned because the DSL (task
/// 10) must detect a duplicate YAML key itself, before the map swallows
/// it — the same gap `CheckError::DuplicateNode` exists for.
#[test]
fn a_duplicate_with_key_collapses_to_the_last_binding() {
    let node_with_duplicate = Node::new(tool_name("github.repo.ensure"))
        .port(port("visibility"), Binding::Literal("public".to_string()))
        .port(
            port("repo"),
            Binding::Literal("lightless-labs/acme-web".to_string()),
        )
        .port(port("visibility"), Binding::Literal("private".to_string()));

    assert_eq!(node_with_duplicate.with.len(), 2);
    assert_eq!(
        node_with_duplicate.with[&port("visibility")],
        Binding::Literal("private".to_string())
    );
    // The first insertion fixes the key's position.
    assert_eq!(
        node_with_duplicate.with.keys().collect::<Vec<_>>(),
        vec![&port("visibility"), &port("repo")]
    );

    let workflow = Workflow::new("duplicate-with-key").node(node("repo"), node_with_duplicate);
    let checked = checked(&workflow);
    assert_eq!(
        checked.types[&node("repo")][&port("visibility")],
        ty("RepoVisibility")
    );
}

/// A node whose `for_each` source is its own output is a one-node cycle,
/// reported exactly once and with nothing cascading from the `Item`
/// bindings it makes unresolvable.
#[test]
fn a_node_whose_for_each_reads_its_own_output_is_one_cycle_error() {
    let workflow = Workflow::new("self-for-each").node(
        node("configs"),
        Node::new(tool_name("doppler.config.ensure"))
            .for_each(Binding::Step {
                node: node("configs"),
                port: port("config"),
            })
            .port(port("project"), Binding::Literal("acme-web".to_string()))
            .port(port("environment"), Binding::Item),
    );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::Cycle {
            nodes: vec![node("configs")],
        }]
    );
}

/// A *required* port bound to `Item` outside a `for_each` node reports
/// only `ItemOutsideForEach`: the port is bound, so it is not also
/// `UnboundInput`.
#[test]
fn a_required_port_bound_to_item_outside_for_each_is_not_also_unbound() {
    let workflow = Workflow::new("item-outside").node(
        node("configs"),
        Node::new(tool_name("doppler.config.ensure"))
            .port(port("project"), Binding::Literal("acme-web".to_string()))
            .port(port("environment"), Binding::Item),
    );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::ItemOutsideForEach {
            site: Site::Port {
                node: node("configs"),
                port: port("environment"),
            },
        }]
    );
}

/// A node with an unknown tool swallows every other error on itself:
/// there is no spec to check its ports against. The errors that remain
/// are its own `UnknownTool` and whatever other nodes contribute, in the
/// documented order.
#[test]
fn an_unknown_tool_suppresses_that_nodes_other_errors_deterministically() {
    let workflow = Workflow::new("unknown-tool-plus-garbage")
        .node(
            node("mystery"),
            Node::new(tool_name("no.such.tool"))
                .for_each(Binding::Literal("not-a-list".to_string()))
                .port(port("whatever"), Binding::Item)
                .port(port("other"), Binding::Input(input("undeclared"))),
        )
        .node(
            node("configs"),
            Node::new(tool_name("doppler.config.ensure"))
                .port(port("project"), Binding::Literal("acme-web".to_string()))
                .port(port("environment"), Binding::Item),
        );

    let found = errors(&workflow);
    assert_eq!(
        found,
        vec![
            CheckError::UnknownTool {
                node: node("mystery"),
                tool: tool_name("no.such.tool"),
            },
            CheckError::ItemOutsideForEach {
                site: Site::Port {
                    node: node("configs"),
                    port: port("environment"),
                },
            },
        ]
    );
    // Determinism: the same workflow checks to the same errors every run.
    assert_eq!(errors(&workflow), found);
}

// ---------------------------------------------------------------------
// Attack: workflow inputs' declared type versus their default value.
// ---------------------------------------------------------------------

/// A non-secret input type with a *secret* default value: `check` looks
/// only at the declared type, so the binding resolves as `Text` and the
/// secret default is what a later stage would actually push into
/// `template.render`. Rejected by `CheckError::DefaultTypeMismatch`
/// (added by this pass; the plan lists no such variant).
#[test]
fn a_secret_default_on_a_non_secret_input_is_rejected() {
    let workflow = Workflow::new("secret-default")
        .input(
            input("motd"),
            InputSpec::new(ty("Text")).with_default(secret_value()),
        )
        .node(
            node("readme"),
            Node::new(tool_name("template.render"))
                .port(
                    port("template"),
                    Binding::Literal("{{ value }}".to_string()),
                )
                .port(port("value"), Binding::Input(input("motd"))),
        );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::DefaultTypeMismatch {
            input: input("motd"),
            expected: ty("Text"),
            found: ty("DopplerServiceToken"),
        }]
    );
}

/// The same defect without a secret: a default whose type is not the
/// declared type is a workflow no later stage could execute.
#[test]
fn a_default_of_the_wrong_non_secret_type_is_rejected() {
    let workflow = Workflow::new("wrong-default").input(
        input("visibility"),
        InputSpec::new(ty("RepoVisibility"))
            .with_default(Value::known(EnvironmentSlug::parse("prd").unwrap())),
    );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::DefaultTypeMismatch {
            input: input("visibility"),
            expected: ty("RepoVisibility"),
            found: ty("EnvironmentSlug"),
        }]
    );
}

/// Cardinality counts too: a scalar default on a list input, and the
/// reverse.
#[test]
fn a_default_of_the_right_type_but_the_wrong_cardinality_is_rejected() {
    let scalar_default = Workflow::new("scalar-default-on-list").input(
        input("environments"),
        InputSpec::new(list_ty("EnvironmentSlug"))
            .with_default(Value::known(EnvironmentSlug::parse("prd").unwrap())),
    );
    assert_eq!(
        errors(&scalar_default),
        vec![CheckError::DefaultTypeMismatch {
            input: input("environments"),
            expected: list_ty("EnvironmentSlug"),
            found: ty("EnvironmentSlug"),
        }]
    );

    let list_default = Workflow::new("list-default-on-scalar").input(
        input("environment"),
        InputSpec::new(ty("EnvironmentSlug")).with_default(Value::known_list(vec![
            EnvironmentSlug::parse("prd").unwrap(),
        ])),
    );
    assert_eq!(
        errors(&list_default),
        vec![CheckError::DefaultTypeMismatch {
            input: input("environment"),
            expected: ty("EnvironmentSlug"),
            found: list_ty("EnvironmentSlug"),
        }]
    );
}

/// The positive fixture's own defaults must keep passing: a scalar enum
/// and a list of slugs, each matching its declared type. An `Unknown`
/// default of the declared type is accepted too — it declares a type
/// without asserting a value, which `describe` can still present.
#[test]
fn well_typed_defaults_including_unknown_are_accepted() {
    let workflow = Workflow::new("good-defaults")
        .input(
            input("visibility"),
            InputSpec::new(ty("RepoVisibility"))
                .with_default(Value::known(RepoVisibility::Private)),
        )
        .input(
            input("environments"),
            InputSpec::new(list_ty("EnvironmentSlug")).with_default(Value::known_list(vec![
                EnvironmentSlug::parse("dev").unwrap(),
                EnvironmentSlug::parse("stg").unwrap(),
                EnvironmentSlug::parse("prd").unwrap(),
            ])),
        )
        .input(
            input("org"),
            InputSpec::new(ty("GitHubOrg")).with_default(Value::unknown(ty("GitHubOrg"))),
        );

    let checked = checked(&workflow);
    assert_eq!(checked.warnings.len(), 3);
}

// ---------------------------------------------------------------------
// Attack: the `outputs` sentinel, and cardinality through a for_each node.
// ---------------------------------------------------------------------

/// The same `for_each` node reached by both `Step` and `Keyed`: the two
/// bindings resolve to different cardinalities of the same element type,
/// and both are accepted.
#[test]
fn one_for_each_node_referenced_by_both_step_and_keyed_resolves_to_list_and_scalar() {
    let workflow = Workflow::new("step-and-keyed")
        .input(
            input("environments"),
            InputSpec::new(list_ty("EnvironmentSlug")),
        )
        .node(
            node("configs"),
            Node::new(tool_name("doppler.config.ensure"))
                .for_each(Binding::Input(input("environments")))
                .port(port("project"), Binding::Literal("acme-web".to_string()))
                .port(port("environment"), Binding::Item),
        )
        .node(
            node("token"),
            Node::new(tool_name("doppler.service_token.ensure"))
                .port(
                    port("config"),
                    Binding::Keyed {
                        node: node("configs"),
                        key: "prd".to_string(),
                        port: port("config"),
                    },
                )
                .port(port("name"), Binding::Literal("ci".to_string())),
        )
        .output(
            output("all_configs"),
            Binding::Step {
                node: node("configs"),
                port: port("config"),
            },
        );

    let checked = checked(&workflow);
    assert_eq!(
        checked.types[&node("token")][&port("config")],
        ty("DopplerConfig")
    );
    assert_eq!(
        checked.output_types[&output("all_configs")],
        list_ty("DopplerConfig")
    );
    // Both references are one edge from `configs`, so it is ordered first.
    assert_eq!(checked.order, vec![node("configs"), node("token")]);
}
/// The empty workflow is valid: nothing to order, nothing to approve.
#[test]
fn an_empty_workflow_is_accepted_with_an_empty_order_and_the_lowest_class() {
    let checked = checked(&Workflow::new("empty"));
    assert!(checked.order.is_empty());
    assert_eq!(checked.class, Class::Reversible);
    assert!(checked.warnings.is_empty());
    assert!(checked.types.is_empty());
    assert!(checked.output_types.is_empty());
}
/// A workflow whose node is literally named `outputs`, with a port whose
/// name matches a workflow output's name. Before this pass, output types
/// were recorded under a synthetic `NodeName("outputs")` in the same map
/// as node port types, so the output silently overwrote the real node's
/// port type. Output types now live in their own map.
#[test]
fn a_node_named_outputs_keeps_its_own_port_types() {
    let workflow = Workflow::new("outputs-collision")
        .node(
            node("outputs"),
            Node::new(tool_name("doppler.project.ensure"))
                .port(port("project"), Binding::Literal("acme-web".to_string())),
        )
        .node(
            node("repo"),
            Node::new(tool_name("github.repo.ensure"))
                .port(
                    port("repo"),
                    Binding::Literal("lightless-labs/acme-web".to_string()),
                )
                .port(port("visibility"), Binding::Literal("private".to_string())),
        )
        .output(
            output("project"),
            Binding::Step {
                node: node("repo"),
                port: port("url"),
            },
        );

    let checked = checked(&workflow);
    assert_eq!(
        checked.types[&node("outputs")][&port("project")],
        ty("DopplerProject"),
        "the real node's port type must survive the output of the same name"
    );
    assert_eq!(checked.output_types[&output("project")], ty("HttpsUrl"));
}
/// A workflow output bound to a secret is **accepted**, deliberately.
///
/// The design doc constrains where a secret may *flow* ("a secret output
/// may only flow to a secret-accepting input"); a workflow output is not
/// an input and, in milestone 1, is not part of `Plan` at all. Every
/// rendering path goes through `Value`, which redacts by construction, so
/// no byte escapes. Milestone 2's workflow-as-tool must type a composite's
/// output ports, at which point a secret workflow output becomes a secret
/// output port and the ordinary sink rule covers it. Pinned here so the
/// decision is a choice, not an accident.
#[test]
fn a_secret_workflow_output_is_accepted_and_keeps_its_secret_type() {
    let workflow = Workflow::new("secret-output")
        .node(node("api_key"), secret_source())
        .output(
            output("leaked"),
            Binding::Step {
                node: node("api_key"),
                port: port("value"),
            },
        );

    let checked = checked(&workflow);
    assert_eq!(
        checked.output_types[&output("leaked")],
        ty("DopplerSecretValue")
    );
    // And it is still redacted everywhere it can be printed.
    let rendered = format!("{:?}", checked.workflow);
    assert!(!rendered.contains("dp.st."));
}

// ---------------------------------------------------------------------
// Attack: cardinality a later stage could not represent.
// ---------------------------------------------------------------------

/// `Step` on a `for_each` node promotes the port type to `list<T>`. When
/// the port is *already* a list there is no `list<list<T>>` in the type
/// model to promote it to, so the workflow is one no later stage could
/// execute. Rejected by `CheckError::NestedList` (added by this pass).
#[test]
fn step_on_a_for_each_node_with_a_list_output_is_rejected() {
    let workflow = Workflow::new("nested-list")
        .input(input("seeds"), InputSpec::new(list_ty("Text")))
        .node(
            node("lines"),
            Node::new(tool_name("fake.text_list"))
                .for_each(Binding::Input(input("seeds")))
                .port(port("seed"), Binding::Item),
        )
        .node(
            node("sink"),
            Node::new(tool_name("fake.text_sink")).port(
                port("lines"),
                Binding::Step {
                    node: node("lines"),
                    port: port("lines"),
                },
            ),
        );

    assert_eq!(
        errors(&workflow),
        vec![CheckError::NestedList {
            site: Site::Port {
                node: node("sink"),
                port: port("lines"),
            },
            referenced: node("lines"),
        }]
    );
}
/// The same list output reached without `for_each` in the way is fine:
/// only the promotion is refused, not list-typed outputs themselves.
#[test]
fn a_list_output_on_a_plain_node_still_binds_to_a_list_port() {
    let workflow = Workflow::new("plain-list")
        .node(
            node("lines"),
            Node::new(tool_name("fake.text_list"))
                .port(port("seed"), Binding::Literal("hello".to_string())),
        )
        .node(
            node("sink"),
            Node::new(tool_name("fake.text_sink")).port(
                port("lines"),
                Binding::Step {
                    node: node("lines"),
                    port: port("lines"),
                },
            ),
        );

    let checked = checked(&workflow);
    assert_eq!(
        checked.types[&node("sink")][&port("lines")],
        list_ty("Text")
    );
}

// ---------------------------------------------------------------------
// Property: `check` never panics, is deterministic, and never accepts a
// workflow whose invariants a later stage would need and not have.
// ---------------------------------------------------------------------

/// Every tool in the test catalog, with the ports a generated node may
/// bind.
const TOOLS: &[&str] = &[
    "naming.v1",
    "github.repo.ensure",
    "github.actions_secret.ensure",
    "doppler.project.ensure",
    "doppler.config.ensure",
    "doppler.service_token.ensure",
    "doppler.secret.get",
    "fake.secret_list",
    "fake.irreversible.ensure",
    "template.render",
    "fake.text_list",
    "fake.text_sink",
    // One name the catalog does not have.
    "no.such.tool",
];

const PORTS: &[&str] = &[
    "org",
    "slug",
    "repo",
    "visibility",
    "name",
    "value",
    "project",
    "environment",
    "config",
    "key",
    "template",
    "seed",
    "lines",
    "token",
    "url",
    "tokens",
];

const INPUT_NAMES: &[&str] = &["a", "b", "c"];
const NODE_NAMES: &[&str] = &["n1", "n2", "n3"];

const TYPE_NAMES: &[&str] = &[
    "Text",
    "GitHubOrg",
    "ProjectSlug",
    "GitHubRepo",
    "RepoVisibility",
    "EnvironmentSlug",
    "DopplerProject",
    "DopplerConfig",
    "DopplerServiceToken",
    // One name the registry does not have.
    "NoSuchType",
];

const LITERALS: &[&str] = &[
    "",
    "private",
    "prd",
    "acme-web",
    "lightless-labs/acme-web",
    "dp.st.prd.hunter2",
    "[REDACTED DopplerServiceToken]",
];

fn arb_binding() -> impl Strategy<Value = Binding> {
    prop_oneof![
        prop::sample::select(INPUT_NAMES).prop_map(|n| Binding::Input(input(n))),
        (
            prop::sample::select(NODE_NAMES),
            prop::sample::select(PORTS)
        )
            .prop_map(|(n, p)| Binding::Step {
                node: node(n),
                port: port(p),
            }),
        (
            prop::sample::select(NODE_NAMES),
            prop::sample::select(LITERALS),
            prop::sample::select(PORTS)
        )
            .prop_map(|(n, k, p)| Binding::Keyed {
                node: node(n),
                key: k.to_string(),
                port: port(p),
            }),
        Just(Binding::Item),
        prop::sample::select(LITERALS).prop_map(|l| Binding::Literal(l.to_string())),
    ]
}

fn arb_node() -> impl Strategy<Value = Node> {
    (
        prop::sample::select(TOOLS),
        prop::option::of(arb_binding()),
        prop::collection::vec((prop::sample::select(PORTS), arb_binding()), 0..4),
    )
        .prop_map(|(tool, for_each, bindings)| {
            let mut node = Node::new(tool_name(tool));
            if let Some(binding) = for_each {
                node = node.for_each(binding);
            }
            for (p, binding) in bindings {
                node = node.port(port(p), binding);
            }
            node
        })
}

fn arb_input_spec() -> impl Strategy<Value = InputSpec> {
    (
        prop::sample::select(TYPE_NAMES),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(name, is_list, has_default)| {
            let declared = if is_list { list_ty(name) } else { ty(name) };
            let spec = InputSpec::new(declared.clone());
            if has_default {
                spec.with_default(Value::unknown(declared))
            } else {
                spec
            }
        })
}

fn arb_workflow() -> impl Strategy<Value = Workflow> {
    (
        prop::collection::vec((prop::sample::select(INPUT_NAMES), arb_input_spec()), 0..3),
        prop::collection::vec((prop::sample::select(NODE_NAMES), arb_node()), 0..4),
        prop::collection::vec((prop::sample::select(INPUT_NAMES), arb_binding()), 0..2),
    )
        .prop_map(|(inputs, nodes, outputs)| {
            let mut workflow = Workflow::new("generated");
            for (name, spec) in inputs {
                workflow = workflow.input(input(name), spec);
            }
            for (name, n) in nodes {
                workflow = workflow.node(node(name), n);
            }
            for (name, binding) in outputs {
                workflow = workflow.output(output(name), binding);
            }
            workflow
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// `check` is total (no panic), deterministic, and — when it accepts —
    /// leaves a `Checked` a later stage can rely on: a complete
    /// topological order, a resolved type for every bound port, and no
    /// secret type on a port that does not accept secrets.
    #[test]
    fn check_is_total_deterministic_and_sound(workflow in arb_workflow()) {
        let catalog = test_catalog();
        let first = check(&workflow, &catalog);
        let second = check(&workflow, &catalog);

        match (&first, &second) {
            (Ok(a), Ok(b)) => prop_assert_eq!(&a.order, &b.order),
            (Err(a), Err(b)) => prop_assert_eq!(a, b),
            _ => prop_assert!(false, "check disagreed with itself across two runs"),
        }

        match first {
            Err(errors) => prop_assert!(!errors.is_empty(), "a rejection must name a reason"),
            Ok(checked) => {
                // The order is a permutation of the nodes.
                prop_assert_eq!(checked.order.len(), workflow.nodes.len());
                for name in workflow.nodes.keys() {
                    prop_assert!(checked.order.contains(name));
                }
                // Every dependency comes before its dependent.
                for (position, name) in checked.order.iter().enumerate() {
                    let node = &workflow.nodes[name];
                    let referenced = node
                        .with
                        .values()
                        .chain(node.for_each.iter())
                        .filter_map(|binding| match binding {
                            Binding::Step { node, .. } | Binding::Keyed { node, .. } => Some(node),
                            _ => None,
                        });
                    for dependency in referenced {
                        let at = checked
                            .order
                            .iter()
                            .position(|n| n == dependency)
                            .expect("a resolved reference names a node in the order");
                        prop_assert!(at < position, "{dependency} must run before {name}");
                    }
                }
                // Every bound port has a resolved type: `check` never
                // accepts a binding it silently dropped.
                let empty = IndexMap::new();
                for (name, node) in &workflow.nodes {
                    let resolved = checked.types.get(name).unwrap_or(&empty);
                    for bound in node.with.keys() {
                        prop_assert!(
                            resolved.contains_key(bound),
                            "node {name}, port {bound} was accepted with no resolved type",
                        );
                    }
                }
                // Every declared workflow output has a resolved type:
                // `check` never accepts an output it recorded nothing for,
                // which is what let `plan` silently drop one (adversarial
                // pass 2, finding 1).
                for name in workflow.outputs.keys() {
                    prop_assert!(
                        checked.output_types.contains_key(name),
                        "output {name} was accepted with no resolved type",
                    );
                }
                // No secret landed on a port that does not accept secrets.
                let registry = willikins_types::registry();
                for (name, resolved) in &checked.types {
                    let spec = catalog.get(&workflow.nodes[name].tool).unwrap().spec();
                    for (bound, found) in resolved {
                        if registry.is_secret(&found.name) == Some(true) {
                            let accepts = match &spec.inputs[bound].ty {
                                PortType::AnySecret => true,
                                PortType::Exact(expected) => {
                                    registry.is_secret(&expected.name) == Some(true)
                                }
                            };
                            prop_assert!(
                                accepts,
                                "secret {found} accepted on non-secret port {name}.{bound}",
                            );
                        }
                    }
                }
            }
        }
    }
}
