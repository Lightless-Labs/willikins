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
//! The catalog mirrors `tests/check.rs`'s: the plan's fake-provider port
//! table, one `DummyTool` per row.

use std::sync::Arc;

use indexmap::IndexMap;
use willikins_core::{
    Binding, Catalog, CheckError, Checked, Class, InputName, InputSpec, Inputs, Node, NodeName,
    Observation, Outputs, PortName, PortSpec, PortType, Tool, ToolError, ToolName, ToolSpec,
    TypeName, TypeRef, Value, Workflow, check,
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
            to: (node("readme"), port("value")),
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
            to: (node("readme"), port("value")),
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
            to: (node("repo"), port("repo")),
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
            node: node("configs"),
            tool: tool_name("doppler.config.ensure"),
            port: port("nope"),
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
            node: node("configs"),
            port: port("environment"),
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
                node: node("configs"),
                port: port("environment"),
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
