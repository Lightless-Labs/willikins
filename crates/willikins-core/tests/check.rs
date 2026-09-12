//! Integration tests for [`willikins_core::check`], against a catalog that
//! mirrors the plan's fake-provider port table (see
//! `docs/plans/2026-09-11-milestone-1-core.md`, "willikins-providers-fake")
//! exactly, and the milestone's positive and negative workflow fixtures,
//! built as [`Workflow`] values directly since the YAML DSL does not exist
//! yet (that is task 10).

use std::sync::Arc;

use indexmap::IndexMap;

use willikins_core::{
    Binding, Catalog, CheckError, CheckWarning, Class, InputSpec, Inputs, Node, NodeName,
    Observation, Outputs, PortName, PortSpec, PortType, Tool, ToolError, ToolName, ToolSpec,
    TypeName, TypeRef, Value, Workflow, check,
};
use willikins_types::{DomainType, EnvironmentSlug, RepoVisibility, SinkToken};

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

fn input(name: &str) -> willikins_core::InputName {
    willikins_core::InputName::parse(name).unwrap()
}

fn output(name: &str) -> willikins_core::OutputName {
    willikins_core::OutputName::parse(name).unwrap()
}

fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).unwrap()
}

/// A tool whose spec is fixed at construction; `read` always reports
/// `Absent` with no predicted outputs, and `ensure` is never exercised by
/// `check`.
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

#[allow(clippy::too_many_arguments)]
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

/// A catalog whose tools mirror the plan's fake-provider port table
/// exactly, one `DummyTool` per row.
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

/// `workflows/new-rust-service.yaml`, built directly as a [`Workflow`].
#[allow(clippy::too_many_lines)]
fn new_rust_service_workflow() -> Workflow {
    Workflow::new("new-rust-service")
        .with_description("Provision a GitHub repository and Doppler project for a Rust service.")
        .input(
            input("slug"),
            InputSpec::new(ty("ProjectSlug")).with_description("Canonical project slug"),
        )
        .input(
            input("org"),
            InputSpec::new(ty("GitHubOrg"))
                .with_description("GitHub organization that owns the repository"),
        )
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
        .node(
            node("names"),
            Node::new(tool_name("naming.v1"))
                .port(port("org"), Binding::Input(input("org")))
                .port(port("slug"), Binding::Input(input("slug"))),
        )
        .node(
            node("repo"),
            Node::new(tool_name("github.repo.ensure"))
                .port(
                    port("repo"),
                    Binding::Step {
                        node: node("names"),
                        port: port("github_repo"),
                    },
                )
                .port(port("visibility"), Binding::Input(input("visibility"))),
        )
        .node(
            node("doppler"),
            Node::new(tool_name("doppler.project.ensure")).port(
                port("project"),
                Binding::Step {
                    node: node("names"),
                    port: port("doppler_project"),
                },
            ),
        )
        .node(
            node("configs"),
            Node::new(tool_name("doppler.config.ensure"))
                .for_each(Binding::Input(input("environments")))
                .port(
                    port("project"),
                    Binding::Step {
                        node: node("doppler"),
                        port: port("project"),
                    },
                )
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
        .node(
            node("ci_secret"),
            Node::new(tool_name("github.actions_secret.ensure"))
                .port(
                    port("repo"),
                    Binding::Step {
                        node: node("repo"),
                        port: port("repo"),
                    },
                )
                .port(port("name"), Binding::Literal("DOPPLER_TOKEN".to_string()))
                .port(
                    port("value"),
                    Binding::Step {
                        node: node("token"),
                        port: port("token"),
                    },
                ),
        )
        .output(
            output("repo_url"),
            Binding::Step {
                node: node("repo"),
                port: port("url"),
            },
        )
}

/// `workflows/fixtures/secret-into-template.yaml`, built directly as a
/// [`Workflow`].
fn secret_into_template_workflow() -> Workflow {
    Workflow::new("secret-into-template")
        .input(input("project"), InputSpec::new(ty("DopplerProject")))
        .node(
            node("doppler"),
            Node::new(tool_name("doppler.project.ensure"))
                .port(port("project"), Binding::Input(input("project"))),
        )
        .node(
            node("config"),
            Node::new(tool_name("doppler.config.ensure"))
                .port(
                    port("project"),
                    Binding::Step {
                        node: node("doppler"),
                        port: port("project"),
                    },
                )
                .port(port("environment"), Binding::Literal("prd".to_string())),
        )
        .node(
            node("token"),
            Node::new(tool_name("doppler.service_token.ensure"))
                .port(
                    port("config"),
                    Binding::Step {
                        node: node("config"),
                        port: port("config"),
                    },
                )
                .port(port("name"), Binding::Literal("ci".to_string())),
        )
        .node(
            node("readme"),
            Node::new(tool_name("template.render"))
                .port(
                    port("template"),
                    Binding::Literal("DOPPLER_TOKEN={{ value }}".to_string()),
                )
                .port(
                    port("value"),
                    Binding::Step {
                        node: node("token"),
                        port: port("token"),
                    },
                ),
        )
}

#[test]
fn acceptance_1_taint_rejection_reports_exactly_the_secret_to_non_secret_sink() {
    let workflow = secret_into_template_workflow();
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("a secret must not reach a non-secret sink");
    assert_eq!(
        errors,
        vec![CheckError::SecretToNonSecretSink {
            from: (node("token"), port("token")),
            to: (node("readme"), port("value")),
        }]
    );
}

#[test]
fn acceptance_2_secret_workflow_input_is_rejected() {
    let workflow =
        Workflow::new("bad").input(input("token"), InputSpec::new(ty("DopplerServiceToken")));
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("a secret-typed input must be rejected");
    assert_eq!(
        errors,
        vec![CheckError::SecretWorkflowInput {
            input: input("token"),
            ty: ty("DopplerServiceToken"),
        }]
    );
}

#[test]
fn acceptance_2_secret_workflow_input_is_rejected_for_a_secret_list_too() {
    let workflow = Workflow::new("bad").input(
        input("tokens"),
        InputSpec::new(list_ty("DopplerServiceToken")),
    );
    let catalog = test_catalog();
    let errors =
        check(&workflow, &catalog).expect_err("a secret-typed list input must be rejected too");
    assert_eq!(
        errors,
        vec![CheckError::SecretWorkflowInput {
            input: input("tokens"),
            ty: list_ty("DopplerServiceToken"),
        }]
    );
}

#[test]
fn acceptance_3_the_registry_refuses_a_secret_literal_input_value() {
    let ty = ty("DopplerServiceToken");
    let err = Value::parse(&ty, "dp.st.prd.exampleexampleexample").unwrap_err();
    assert!(err.reason.contains("cannot be supplied"), "{}", err.reason);
}

#[test]
fn positive_fixture_checks_successfully_with_the_expected_order_and_class() {
    let workflow = new_rust_service_workflow();
    let catalog = test_catalog();
    let checked = check(&workflow, &catalog).expect("the positive fixture must check cleanly");

    let expected_order: Vec<NodeName> =
        ["names", "repo", "doppler", "configs", "token", "ci_secret"]
            .into_iter()
            .map(node)
            .collect();
    assert_eq!(checked.order, expected_order);
    assert_eq!(checked.class, Class::Reversible);
    assert!(!checked.class.requires_approval());
    assert!(checked.warnings.is_empty(), "{:?}", checked.warnings);
}

#[test]
fn acceptance_4_keyed_reference_into_a_for_each_node_resolves_to_the_scalar_type() {
    let workflow = new_rust_service_workflow();
    let catalog = test_catalog();
    let checked = check(&workflow, &catalog).expect("the positive fixture must check cleanly");

    // `token`'s `config` port is `${{ steps.configs[prd].config }}`.
    assert_eq!(
        checked.types[&node("token")][&port("config")],
        ty("DopplerConfig")
    );
}

#[test]
fn acceptance_4_plain_step_reference_into_a_for_each_node_resolves_to_a_list() {
    // The positive fixture never plainly `Step`s into `configs` (only the
    // `Keyed` reference from `token` does), so this is checked through an
    // extra output on a copy of the fixture rather than the canonical
    // fixture itself.
    let workflow = new_rust_service_workflow().output(
        output("all_configs"),
        Binding::Step {
            node: node("configs"),
            port: port("config"),
        },
    );
    let catalog = test_catalog();
    let checked =
        check(&workflow, &catalog).expect("adding a plain-Step output must still check cleanly");

    assert_eq!(
        checked.output_types[&output("all_configs")],
        list_ty("DopplerConfig")
    );
}

#[test]
fn acceptance_4_for_each_over_a_secret_source_is_rejected() {
    let workflow = Workflow::new("secret-for-each")
        .input(input("project"), InputSpec::new(ty("DopplerProject")))
        .node(
            node("config"),
            Node::new(tool_name("doppler.config.ensure"))
                .port(port("project"), Binding::Input(input("project")))
                .port(port("environment"), Binding::Literal("prd".to_string())),
        )
        .node(
            node("secrets"),
            Node::new(tool_name("fake.secret_list")).port(
                port("config"),
                Binding::Step {
                    node: node("config"),
                    port: port("config"),
                },
            ),
        )
        .node(
            node("render"),
            Node::new(tool_name("template.render"))
                .for_each(Binding::Step {
                    node: node("secrets"),
                    port: port("tokens"),
                })
                .port(port("template"), Binding::Literal("x".to_string()))
                .port(port("value"), Binding::Literal("y".to_string())),
        );
    let catalog = test_catalog();
    let errors =
        check(&workflow, &catalog).expect_err("a for_each over a secret list must be rejected");
    assert_eq!(
        errors,
        vec![CheckError::SecretForEachSource {
            node: node("render")
        }]
    );
}

#[test]
fn acceptance_4_for_each_over_a_scalar_input_is_rejected() {
    let workflow = Workflow::new("scalar-for-each")
        .input(input("org"), InputSpec::new(ty("GitHubOrg")))
        .node(
            node("names"),
            Node::new(tool_name("naming.v1"))
                .for_each(Binding::Input(input("org")))
                .port(port("org"), Binding::Input(input("org")))
                .port(port("slug"), Binding::Literal("demo".to_string())),
        );
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("a for_each over a scalar must be rejected");
    assert_eq!(
        errors,
        vec![CheckError::ForEachOverScalar {
            node: node("names")
        }]
    );
}

#[test]
fn acceptance_4_item_outside_a_for_each_node_is_rejected() {
    let workflow = Workflow::new("item-outside")
        .input(input("org"), InputSpec::new(ty("GitHubOrg")))
        .node(
            node("names"),
            Node::new(tool_name("naming.v1"))
                .port(port("org"), Binding::Input(input("org")))
                .port(port("slug"), Binding::Item),
        );
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("`item` outside for_each must be rejected");
    assert_eq!(
        errors,
        vec![CheckError::ItemOutsideForEach {
            node: node("names"),
            port: port("slug"),
        }]
    );
}

#[test]
fn acceptance_4_keyed_reference_to_a_node_without_for_each_is_rejected() {
    let workflow = Workflow::new("keyed-on-scalar")
        .input(input("project"), InputSpec::new(ty("DopplerProject")))
        .node(
            node("doppler"),
            Node::new(tool_name("doppler.project.ensure"))
                .port(port("project"), Binding::Input(input("project"))),
        )
        .node(
            node("token"),
            Node::new(tool_name("doppler.service_token.ensure"))
                .port(
                    port("config"),
                    Binding::Keyed {
                        node: node("doppler"),
                        key: "prd".to_string(),
                        port: port("project"),
                    },
                )
                .port(port("name"), Binding::Literal("ci".to_string())),
        );
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("a keyed reference needs a for_each node");
    assert_eq!(
        errors,
        vec![CheckError::KeyedOnScalarNode {
            node: node("token"),
            port: port("config"),
            referenced: node("doppler"),
        }]
    );
}

#[test]
fn a_secret_list_bound_to_an_any_secret_port_is_a_type_mismatch_not_a_taint_violation() {
    // `AnySecret` accepts a secret scalar but not a secret list (cardinality),
    // so this is `TypeMismatch`, not `SecretToNonSecretSink`: the precedence
    // rule is "secret flowing into a port that cannot accept a secret at
    // all", and `AnySecret` *can* accept a secret, just not this shape of
    // one.
    let workflow = Workflow::new("w")
        .input(input("config"), InputSpec::new(ty("DopplerConfig")))
        .node(
            node("secrets"),
            Node::new(tool_name("fake.secret_list"))
                .port(port("config"), Binding::Input(input("config"))),
        )
        .node(
            node("repo"),
            Node::new(tool_name("github.repo.ensure"))
                .port(
                    port("repo"),
                    Binding::Literal("lightless-labs/demo".to_string()),
                )
                .port(port("visibility"), Binding::Literal("private".to_string())),
        )
        .node(
            node("ci_secret"),
            Node::new(tool_name("github.actions_secret.ensure"))
                .port(
                    port("repo"),
                    Binding::Step {
                        node: node("repo"),
                        port: port("repo"),
                    },
                )
                .port(port("name"), Binding::Literal("DOPPLER_TOKEN".to_string()))
                .port(
                    port("value"),
                    Binding::Step {
                        node: node("secrets"),
                        port: port("tokens"),
                    },
                ),
        );
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("a secret list must not satisfy AnySecret");
    assert_eq!(
        errors,
        vec![CheckError::TypeMismatch {
            node: node("ci_secret"),
            port: port("value"),
            expected: PortType::AnySecret,
            found: list_ty("DopplerServiceToken"),
        }]
    );
}

#[test]
fn acceptance_6_a_workflow_with_an_irreversible_node_requires_approval() {
    let workflow = new_rust_service_workflow().node(
        node("danger"),
        Node::new(tool_name("fake.irreversible.ensure"))
            .port(port("key"), Binding::Input(input("slug"))),
    );
    let catalog = test_catalog();
    let checked =
        check(&workflow, &catalog).expect("adding an irreversible node must still check cleanly");
    assert_eq!(checked.class, Class::Irreversible);
    assert!(checked.class.requires_approval());
}

#[test]
fn unused_input_produces_a_warning_not_an_error() {
    let workflow = Workflow::new("unused")
        .input(input("org"), InputSpec::new(ty("GitHubOrg")))
        .input(input("slug"), InputSpec::new(ty("ProjectSlug")))
        .node(
            node("names"),
            Node::new(tool_name("naming.v1"))
                .port(port("org"), Binding::Input(input("org")))
                .port(port("slug"), Binding::Literal("demo".to_string())),
        );
    let catalog = test_catalog();
    let checked = check(&workflow, &catalog).expect("an unused input is a warning, not an error");
    assert_eq!(
        checked.warnings,
        vec![CheckWarning::UnusedInput {
            input: input("slug")
        }]
    );
}

#[test]
fn multiple_errors_are_all_reported_in_order() {
    // Two independent, unrelated problems: an unknown tool on one node,
    // and an unbound required port on another. `check` reports every
    // `UnknownTool` first (one pass over every node resolves tool specs
    // before any node's ports are checked), then per-node port errors in
    // node declaration order; see the module docs on `check` for the full
    // ordering rule.
    let workflow = Workflow::new("multi-error")
        .node(node("names"), Node::new(tool_name("naming.v1")))
        .node(
            node("mystery"),
            Node::new(tool_name("no.such.tool")).port(port("x"), Binding::Literal("y".to_string())),
        );
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("both problems must be reported");
    assert_eq!(
        errors,
        vec![
            CheckError::UnknownTool {
                node: node("mystery"),
                tool: tool_name("no.such.tool"),
            },
            CheckError::UnboundInput {
                node: node("names"),
                port: port("org"),
            },
            CheckError::UnboundInput {
                node: node("names"),
                port: port("slug"),
            },
        ]
    );
}
