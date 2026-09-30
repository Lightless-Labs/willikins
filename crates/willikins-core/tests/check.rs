//! Integration tests for [`willikins_core::check`], against a catalog that
//! mirrors the plan's fake-provider port table (see
//! `docs/plans/2026-09-11-milestone-1-core.md`, "willikins-providers-fake")
//! exactly, and the milestone's positive and negative workflow fixtures,
//! built as [`Workflow`] values directly since the YAML DSL does not exist
//! yet (that is task 10).
//!
//! The positive fixture builder itself
//! ([`common::new_rust_service_workflow`]) is shared with `tests/plan.rs`
//! and friends, rather than duplicated here.

mod common;

use std::sync::Arc;

use indexmap::IndexMap;

use willikins_core::{
    Binding, Catalog, CheckError, CheckWarning, Class, Ensured, InputSpec, Inputs, Node, NodeName,
    Observation, Outputs, PortName, PortSpec, PortType, Site, Tool, ToolError, ToolName, ToolSpec,
    TypeName, TypeRef, TypeRegistry, Value, Workflow, check,
};
use willikins_types::{DomainType, SinkToken};

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

fn workflow_name(name: &str) -> willikins_types::WorkflowName {
    willikins_types::WorkflowName::parse(name).unwrap()
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

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: Outputs::new(),
            changed: true,
        })
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
                derived_only: false,
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
        // Milestone 3g, decision (a): a tool with a required `list<T>`
        // input port, for testing `Binding::List` against a real catalog
        // entry (no shipped tool has one yet -- `T1` adds the first,
        // `repo.file.render`).
        spec_of(
            "fake.list_sink.ensure",
            &[("orgs", PortType::Exact(list_ty("GitHubOrg")), true)],
            &[],
            &["orgs"],
            Class::Reversible,
            false,
        ),
        // Milestone 3g, task E2: shaped exactly like `T1`'s own
        // `repo.file.render` (SHARED VALUES table) so the check-level
        // refusals this task adds -- a `RepoFile` output cannot be
        // supplied by a literal, and a secret bound into `values` is
        // `SecretToNonSecretSink` -- are proved against the real port
        // shape before `T1` exists.
        spec_of(
            "repo.file.render",
            &[
                ("path", exact("RepoPath"), true),
                ("template", exact("TemplateSource"), true),
                ("values", PortType::Exact(list_ty("TemplateValue")), false),
            ],
            &[("file", ty("RepoFile"))],
            &[],
            Class::Reversible,
            true,
        ),
        // Milestone 3g, task E2: shaped exactly like `G2`'s own
        // `github.scaffold.ensure` (SHARED VALUES table), so the
        // list-element `RepoFile` literal refusal is proved against the
        // very port Walter's document binds (`files: list<RepoFile>`)
        // before `G2` exists.
        spec_of(
            "github.scaffold.ensure",
            &[
                ("repo", exact("GitHubRepo"), true),
                ("branch", exact("GitBranchName"), true),
                ("marker", exact("RepoPath"), true),
                ("files", PortType::Exact(list_ty("RepoFile")), true),
                ("message", exact("CommitHeadline"), true),
            ],
            &[
                ("repo", ty("GitHubRepo")),
                ("branch", ty("GitBranchName")),
                ("marker", ty("RepoPath")),
            ],
            &["repo", "branch", "marker"],
            Class::Irreversible,
            false,
        ),
        // A bare scalar `RepoFile` sink, for testing the literal refusal
        // on a non-list port too (no shipped tool has a scalar `RepoFile`
        // input; `github.scaffold.ensure`'s is a list).
        spec_of(
            "fake.repo_file_sink.ensure",
            &[("file", exact("RepoFile"), true)],
            &[],
            &["file"],
            Class::Reversible,
            false,
        ),
    ];
    for spec in specs {
        catalog.insert(Arc::new(DummyTool { spec })).unwrap();
    }
    catalog
}

/// `workflows/fixtures/secret-into-template.yaml`, built directly as a
/// [`Workflow`].
fn secret_into_template_workflow() -> Workflow {
    Workflow::new(workflow_name("secret-into-template"))
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
            to: Site::Port {
                node: node("readme"),
                port: port("value"),
            },
        }]
    );
}

#[test]
fn acceptance_2_secret_workflow_input_is_rejected() {
    let workflow = Workflow::new(workflow_name("bad"))
        .input(input("token"), InputSpec::new(ty("DopplerServiceToken")));
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
    let workflow = Workflow::new(workflow_name("bad")).input(
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

// ---------------------------------------------------------------------
// Milestone 3g, task E2 (decision (e); acceptance 4): `TemplateSource` and
// `RepoFile` may never be a workflow input type, a default of either is
// exactly the same one error (not two), and a literal may never supply a
// `RepoFile`, scalar or as a list element.
// ---------------------------------------------------------------------

#[test]
fn e2_a_template_source_workflow_input_is_rejected() {
    let workflow = Workflow::new(workflow_name("bad"))
        .input(input("tmpl"), InputSpec::new(ty("TemplateSource")));
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("a TemplateSource input must be rejected");
    assert_eq!(
        errors,
        vec![CheckError::DisallowedInputType {
            input: input("tmpl"),
            ty: ty("TemplateSource"),
        }]
    );
}

#[test]
fn e2_a_repo_file_workflow_input_is_rejected() {
    let workflow =
        Workflow::new(workflow_name("bad")).input(input("file"), InputSpec::new(ty("RepoFile")));
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog).expect_err("a RepoFile input must be rejected");
    assert_eq!(
        errors,
        vec![CheckError::DisallowedInputType {
            input: input("file"),
            ty: ty("RepoFile"),
        }]
    );
}

#[test]
fn e2_a_template_source_input_with_a_default_is_still_exactly_one_error() {
    let workflow = Workflow::new(workflow_name("bad")).input(
        input("tmpl"),
        InputSpec::new(ty("TemplateSource")).with_default(Value::known(
            willikins_types::TemplateSource::parse("Hello, {{ 0 }}!").unwrap(),
        )),
    );
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog)
        .expect_err("a TemplateSource input with a default must still be rejected");
    assert_eq!(
        errors,
        vec![CheckError::DisallowedInputType {
            input: input("tmpl"),
            ty: ty("TemplateSource"),
        }]
    );
}

#[test]
fn e2_a_repo_file_input_with_a_default_is_still_exactly_one_error() {
    let path = willikins_types::RepoPath::parse("apps/walter/BUILD.bazel").unwrap();
    let file = willikins_types::RepoFile::new(path, "content\n").unwrap();
    let workflow = Workflow::new(workflow_name("bad")).input(
        input("file"),
        InputSpec::new(ty("RepoFile")).with_default(Value::known(file)),
    );
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog)
        .expect_err("a RepoFile input with a default must still be rejected");
    assert_eq!(
        errors,
        vec![CheckError::DisallowedInputType {
            input: input("file"),
            ty: ty("RepoFile"),
        }]
    );
}

#[test]
fn e2_a_literal_cannot_supply_a_scalar_repo_file_port() {
    let workflow = Workflow::new(workflow_name("bad")).node(
        node("sink"),
        Node::new(tool_name("fake.repo_file_sink.ensure")).port(
            port("file"),
            Binding::Literal("apps/walter/BUILD.bazel\ncontent\n".to_string()),
        ),
    );
    let catalog = test_catalog();
    let errors =
        check(&workflow, &catalog).expect_err("a literal must not supply a scalar RepoFile");
    assert_eq!(
        errors,
        vec![CheckError::RepoFileLiteral {
            node: node("sink"),
            port: port("file"),
        }]
    );
}

#[test]
fn e2_a_literal_cannot_supply_a_repo_file_list_element() {
    // `github.scaffold.ensure`'s own port shape: `files: list<RepoFile>`
    // is exactly what Walter's document binds (decision (b)).
    let workflow = Workflow::new(workflow_name("bad")).node(
        node("scaffold"),
        Node::new(tool_name("github.scaffold.ensure"))
            .port(
                port("repo"),
                Binding::Literal("lightless-labs/monorepo".to_string()),
            )
            .port(port("branch"), Binding::Literal("main".to_string()))
            .port(
                port("marker"),
                Binding::Literal("apps/walter/.willikins-scaffold".to_string()),
            )
            .port(
                port("files"),
                Binding::List(vec![Binding::Literal(
                    "apps/walter/BUILD.bazel\ncontent\n".to_string(),
                )]),
            )
            .port(
                port("message"),
                Binding::Literal("feat: scaffold".to_string()),
            ),
    );
    let catalog = test_catalog();
    let errors =
        check(&workflow, &catalog).expect_err("a literal must not supply a RepoFile list element");
    assert_eq!(
        errors,
        vec![CheckError::RepoFileLiteral {
            node: node("scaffold"),
            port: port("files"),
        }]
    );
}

#[test]
fn e2_a_secret_value_as_a_repo_file_render_values_element_is_exactly_one_taint_error() {
    // Decision (e): `workflows/fixtures/secret-into-repo-file.yaml`'s
    // shape, built directly as a `Workflow` (`T1` does not exist yet, so
    // this proves the rule against `repo.file.render`'s real port shape
    // from `test_catalog`, exactly as `secret_into_template_workflow`
    // does for `template.render`).
    let workflow = Workflow::new(workflow_name("secret-into-repo-file"))
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
            node("secret"),
            Node::new(tool_name("doppler.secret.get"))
                .port(
                    port("config"),
                    Binding::Step {
                        node: node("config"),
                        port: port("config"),
                    },
                )
                .port(port("name"), Binding::Literal("SOME_SECRET".to_string())),
        )
        .node(
            node("render"),
            Node::new(tool_name("repo.file.render"))
                .port(
                    port("path"),
                    Binding::Literal("apps/walter/BUILD.bazel".to_string()),
                )
                .port(port("template"), Binding::Literal("{{ 0 }}".to_string()))
                .port(
                    port("values"),
                    Binding::List(vec![Binding::Step {
                        node: node("secret"),
                        port: port("value"),
                    }]),
                ),
        );
    let catalog = test_catalog();
    let errors = check(&workflow, &catalog)
        .expect_err("a secret must not reach repo.file.render's values list");
    assert_eq!(
        errors,
        vec![CheckError::SecretToNonSecretSink {
            from: (node("secret"), port("value")),
            to: Site::list_element(node("render"), port("values"), 0),
        }]
    );
}

#[test]
fn acceptance_3_the_registry_refuses_a_secret_literal_input_value() {
    let ty = ty("DopplerServiceToken");
    // `concat!`-split so this file holds no literal spelling the whole
    // token contiguously.
    let err = Value::parse(
        &ty,
        concat!("dp.st.prd.", "exampleexampleexampleexampleexampleexample"),
    )
    .unwrap_err();
    assert!(err.reason.contains("cannot be supplied"), "{}", err.reason);
}

#[test]
fn positive_fixture_checks_successfully_with_the_expected_order_and_class() {
    let workflow = common::new_rust_service_workflow();
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
    let workflow = common::new_rust_service_workflow();
    let catalog = test_catalog();
    let checked = check(&workflow, &catalog).expect("the positive fixture must check cleanly");

    // `token`'s `config` port is `${{ steps.configs[prd].config }}`.
    assert_eq!(
        checked.types[&node("token")][&port("config")].ty(),
        &ty("DopplerConfig")
    );
}

#[test]
fn acceptance_4_plain_step_reference_into_a_for_each_node_resolves_to_a_list() {
    // The positive fixture never plainly `Step`s into `configs` (only the
    // `Keyed` reference from `token` does), so this is checked through an
    // extra output on a copy of the fixture rather than the canonical
    // fixture itself.
    let workflow = common::new_rust_service_workflow().output(
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
    let workflow = Workflow::new(workflow_name("secret-for-each"))
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
    let workflow = Workflow::new(workflow_name("scalar-for-each"))
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
    let workflow = Workflow::new(workflow_name("item-outside"))
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
            site: Site::Port {
                node: node("names"),
                port: port("slug"),
            },
        }]
    );
}

#[test]
fn acceptance_4_keyed_reference_to_a_node_without_for_each_is_rejected() {
    let workflow = Workflow::new(workflow_name("keyed-on-scalar"))
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
            site: Site::Port {
                node: node("token"),
                port: port("config"),
            },
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
    let workflow = Workflow::new(workflow_name("w"))
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
    let workflow = common::new_rust_service_workflow().node(
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
    let workflow = Workflow::new(workflow_name("unused"))
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
    let workflow = Workflow::new(workflow_name("multi-error"))
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

// ---------------------------------------------------------------------
// Milestone 3d, decision (c): "one hop, no chains" -- `probe_conversion`
// is a single probe of the `(from, to)` table, never a transitive
// search. If `A` converts to `B` and `B` converts to `C`, an `A` bound
// where a `C` is wanted is refused exactly as if neither row existed.
// `docs/plans/2026-09-23-milestone-3d-conversions.md`, decision (c) and
// acceptance test 4.
// ---------------------------------------------------------------------

#[derive(willikins_types::DomainType)]
#[domain(
    pattern = "[a-z]+",
    description = "The first link in a conversion chain.",
    example = "a"
)]
struct ChainA(String);

#[derive(willikins_types::DomainType)]
#[domain(
    pattern = "[a-z]+",
    description = "The second link in a conversion chain.",
    example = "b"
)]
struct ChainB(String);

#[derive(willikins_types::DomainType)]
#[domain(
    pattern = "[a-z]+",
    description = "The third link in a conversion chain.",
    example = "c"
)]
struct ChainC(String);

impl From<ChainA> for ChainB {
    fn from(a: ChainA) -> Self {
        Self::parse(a.as_str()).unwrap_or_else(|err| unreachable!("ChainA's own string: {err}"))
    }
}

impl From<ChainB> for ChainC {
    fn from(b: ChainB) -> Self {
        Self::parse(b.as_str()).unwrap_or_else(|err| unreachable!("ChainB's own string: {err}"))
    }
}

/// A registry with `ChainA => ChainB` and `ChainB => ChainC` registered,
/// and deliberately **no** `ChainA => ChainC` row. Leaked once per
/// process, the same way every other `&'static TypeRegistry` this
/// workspace builds is.
fn chain_registry() -> &'static TypeRegistry {
    Box::leak(Box::new(TypeRegistry::new(
        vec![
            willikins_types::registry::TypeEntry::of::<ChainA>(),
            willikins_types::registry::TypeEntry::of::<ChainB>(),
            willikins_types::registry::TypeEntry::of::<ChainC>(),
        ],
        willikins_types::conversions![ChainA => ChainB, ChainB => ChainC],
    )))
}

/// `sink_c.c` (`Exact(ChainC)`), `sink_b.b` (`Exact(ChainB)`), and a pure
/// `echo.in -> echo.out`, both `ChainB`.
fn chain_catalog() -> Catalog {
    let mut catalog = Catalog::new(chain_registry());
    catalog
        .insert(Arc::new(DummyTool {
            spec: spec_of(
                "chain.sink_c",
                &[("c", exact("ChainC"), true)],
                &[],
                &[],
                Class::Reversible,
                false,
            ),
        }))
        .unwrap();
    catalog
        .insert(Arc::new(DummyTool {
            spec: spec_of(
                "chain.sink_b",
                &[("b", exact("ChainB"), true)],
                &[],
                &[],
                Class::Reversible,
                false,
            ),
        }))
        .unwrap();
    catalog
        .insert(Arc::new(DummyTool {
            spec: spec_of(
                "chain.echo",
                &[("in", exact("ChainB"), true)],
                &[("out", ty("ChainB"))],
                &[],
                Class::Reversible,
                true,
            ),
        }))
        .unwrap();
    catalog
        .insert(Arc::new(DummyTool {
            spec: spec_of(
                "chain.sink_list_b",
                &[("xs", PortType::Exact(list_ty("ChainB")), true)],
                &[],
                &[],
                Class::Reversible,
                false,
            ),
        }))
        .unwrap();
    catalog
}

/// An `A` bound where a `C` is wanted, with only `A => B` and `B => C`
/// registered (no `A => C`), is refused exactly as if neither row
/// existed: today's exact `TypeMismatch`, byte for byte.
#[test]
fn a_bound_where_c_is_wanted_is_refused_exactly_as_before() {
    let workflow = Workflow::new(workflow_name("no-chains"))
        .input(input("a"), InputSpec::new(ty("ChainA")))
        .node(
            node("sink"),
            Node::new(tool_name("chain.sink_c")).port(port("c"), Binding::Input(input("a"))),
        );
    let errors =
        check(&workflow, &chain_catalog()).expect_err("A must not convert to C in one hop");
    assert_eq!(
        errors,
        vec![CheckError::TypeMismatch {
            node: node("sink"),
            port: port("c"),
            expected: PortType::Exact(ty("ChainC")),
            found: ty("ChainA"),
        }]
    );
    assert_eq!(
        errors[0].to_string(),
        "node `sink`, port `c`: expected ChainC, found `ChainA`"
    );
}

/// Positive control: `B` bound where `C` is wanted converts (`B` is one
/// hop from `C`), and the edge records exactly that conversion.
#[test]
fn b_bound_where_c_is_wanted_converts_one_hop() {
    let workflow = Workflow::new(workflow_name("no-chains-b-to-c"))
        .input(input("b"), InputSpec::new(ty("ChainB")))
        .node(
            node("sink"),
            Node::new(tool_name("chain.sink_c")).port(port("c"), Binding::Input(input("b"))),
        );
    let checked = check(&workflow, &chain_catalog()).expect("B must convert to C in one hop");
    let edge = &checked.types[&node("sink")][&port("c")];
    assert_eq!(edge.ty(), &ty("ChainB"));
    let conversion = edge
        .conversion()
        .expect("B -> C is a registered conversion");
    assert_eq!(conversion.from().as_str(), "ChainB");
    assert_eq!(conversion.to().as_str(), "ChainC");
}

/// Positive control: `A` bound where `B` is wanted converts, the other
/// registered hop.
#[test]
fn a_bound_where_b_is_wanted_converts_one_hop() {
    let workflow = Workflow::new(workflow_name("no-chains-a-to-b"))
        .input(input("a"), InputSpec::new(ty("ChainA")))
        .node(
            node("sink"),
            Node::new(tool_name("chain.sink_b")).port(port("b"), Binding::Input(input("a"))),
        );
    let checked = check(&workflow, &chain_catalog()).expect("A must convert to B in one hop");
    let edge = &checked.types[&node("sink")][&port("b")];
    assert_eq!(edge.ty(), &ty("ChainA"));
    let conversion = edge
        .conversion()
        .expect("A -> B is a registered conversion");
    assert_eq!(conversion.from().as_str(), "ChainA");
    assert_eq!(conversion.to().as_str(), "ChainB");
}

/// A document that spells out two edges -- `A` converted into `echo`
/// (one hop, `A => B`), then `echo`'s own `ChainB` output bound exactly
/// to `sink_b` (no conversion needed) -- checks cleanly. The no-chains
/// refusal is of a chain *inside one edge*, never of a document that
/// writes the two edges out itself.
#[test]
fn a_document_that_spells_out_two_edges_checks() {
    let workflow = Workflow::new(workflow_name("no-chains-two-edges"))
        .input(input("a"), InputSpec::new(ty("ChainA")))
        .node(
            node("echo"),
            Node::new(tool_name("chain.echo")).port(port("in"), Binding::Input(input("a"))),
        )
        .node(
            node("sink"),
            Node::new(tool_name("chain.sink_b")).port(
                port("b"),
                Binding::Step {
                    node: node("echo"),
                    port: port("out"),
                },
            ),
        );
    let checked = check(&workflow, &chain_catalog()).unwrap_or_else(|errors| {
        panic!("a document spelling out two edges must check: {errors:?}")
    });

    let echo_edge = &checked.types[&node("echo")][&port("in")];
    assert_eq!(echo_edge.ty(), &ty("ChainA"));
    assert_eq!(
        echo_edge.conversion().map(|c| c.to().as_str()),
        Some("ChainB"),
        "the first edge is one hop, A -> B"
    );

    let sink_edge = &checked.types[&node("sink")][&port("b")];
    assert_eq!(sink_edge.ty(), &ty("ChainB"));
    assert!(
        sink_edge.conversion().is_none(),
        "the second edge is an exact match, no conversion needed"
    );
}

/// A literal is never converted (decision (e), step 2): it has no source
/// type of its own, and it parses directly as the port's own type. Its
/// edge records exactly that type, with no conversion -- even when the
/// text would also have parsed as the one registered conversion's source
/// (`com.example.MyApp` is a valid `AppleBundleIdentifier` too). Literals
/// parse through the global registry, so this uses the production row
/// (`AppleBundleIdentifier => AppleProfileName`), not the chain types.
#[test]
fn a_literal_records_an_edge_with_no_conversion() {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(DummyTool {
            spec: spec_of(
                "literal.sink",
                &[("name", exact("AppleProfileName"), true)],
                &[],
                &[],
                Class::Reversible,
                false,
            ),
        }))
        .unwrap();
    let workflow = Workflow::new(workflow_name("no-chains-literal")).node(
        node("sink"),
        Node::new(tool_name("literal.sink")).port(
            port("name"),
            Binding::Literal("com.example.MyApp".to_string()),
        ),
    );
    let checked = check(&workflow, &catalog).expect("a matching literal must check");
    let edge = &checked.types[&node("sink")][&port("name")];
    assert_eq!(edge.ty(), &ty("AppleProfileName"));
    assert!(edge.conversion().is_none(), "a literal is never converted");
}

/// A registered scalar conversion does not lift to lists: `list<A>`
/// bound to a `list<B>` port stays a plain `TypeMismatch`, the probe
/// never runs (decision (e)'s exclusions: "the probe requires two
/// scalars"). In Rust, `Vec<A>` is not `Into<Vec<B>>` either.
#[test]
fn list_a_into_a_list_b_port_stays_a_type_mismatch() {
    let workflow = Workflow::new(workflow_name("no-chains-list"))
        .input(input("xs"), InputSpec::new(list_ty("ChainA")))
        .node(
            node("sink"),
            Node::new(tool_name("chain.sink_list_b")).port(port("xs"), Binding::Input(input("xs"))),
        );
    let errors = check(&workflow, &chain_catalog())
        .expect_err("list<ChainA> must not convert to list<ChainB>");
    assert_eq!(
        errors,
        vec![CheckError::TypeMismatch {
            node: node("sink"),
            port: port("xs"),
            expected: PortType::Exact(list_ty("ChainB")),
            found: list_ty("ChainA"),
        }]
    );
}

// ---------------------------------------------------------------------
// Milestone 3g, decision (a): `Binding::List`, acceptance test 1.
// ---------------------------------------------------------------------

/// A `Binding::List` bound to a scalar port is `ListOnScalarPort`, named
/// at the binding's own `Site::Port`.
#[test]
fn list_binding_on_a_scalar_port_is_rejected() {
    let workflow = Workflow::new(workflow_name("list-on-scalar")).node(
        node("repo"),
        Node::new(tool_name("github.repo.ensure"))
            .port(
                port("repo"),
                Binding::List(vec![Binding::Literal("a/b".to_string())]),
            )
            .port(port("visibility"), Binding::Literal("private".to_string())),
    );
    let errors = check(&workflow, &test_catalog())
        .expect_err("a list cannot bind to a scalar GitHubRepo port");
    assert_eq!(
        errors,
        vec![CheckError::ListOnScalarPort {
            site: Site::Port {
                node: node("repo"),
                port: port("repo"),
            },
            expected: exact("GitHubRepo"),
        }]
    );
}

/// A `Binding::List` bound to an `AnySecret` port is `ListOnScalarPort`
/// too: `AnySecret` names no single element type to check a list against.
#[test]
fn list_binding_on_an_any_secret_port_is_rejected() {
    let workflow = Workflow::new(workflow_name("list-on-any-secret")).node(
        node("secret"),
        Node::new(tool_name("github.actions_secret.ensure"))
            .port(port("repo"), Binding::Literal("a/b".to_string()))
            .port(port("name"), Binding::Literal("TOKEN".to_string()))
            .port(
                port("value"),
                Binding::List(vec![Binding::Literal("x".to_string())]),
            ),
    );
    let errors =
        check(&workflow, &test_catalog()).expect_err("a list cannot bind to an AnySecret port");
    assert_eq!(
        errors,
        vec![CheckError::ListOnScalarPort {
            site: Site::Port {
                node: node("secret"),
                port: port("value"),
            },
            expected: PortType::AnySecret,
        }]
    );
}

/// A `Binding::List` bound to a `derived_only` port is `UnderivedBinding`:
/// no list element can be "the output of one earlier, non-pure node" the
/// way a bare `Step` binding can.
#[test]
fn list_binding_on_a_derived_only_port_is_underived() {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(DummyTool {
            spec: ToolSpec {
                name: tool_name("derived.sink"),
                description: "Test double.".to_string(),
                inputs: IndexMap::from([(
                    port("config"),
                    PortSpec {
                        ty: PortType::Exact(list_ty("DopplerConfig")),
                        required: true,
                        derived_only: true,
                    },
                )]),
                outputs: IndexMap::new(),
                key: vec![],
                class: Class::Reversible,
                pure: false,
            },
        }))
        .unwrap();
    let workflow = Workflow::new(workflow_name("list-derived-only")).node(
        node("sink"),
        Node::new(tool_name("derived.sink")).port(
            port("config"),
            Binding::List(vec![Binding::Literal("x".to_string())]),
        ),
    );
    let errors =
        check(&workflow, &catalog).expect_err("no list binding is ever a derived-only source");
    assert_eq!(
        errors,
        vec![CheckError::UnderivedBinding {
            node: node("sink"),
            port: port("config"),
        }]
    );
}

/// Every element of a `Binding::List` is checked, and one error per bad
/// element is reported -- not just the first.
#[test]
fn every_bad_element_of_a_list_binding_is_reported() {
    let workflow = Workflow::new(workflow_name("list-two-bad"))
        .input(input("url"), InputSpec::new(ty("HttpsUrl")))
        .node(
            node("sink"),
            Node::new(tool_name("fake.list_sink.ensure")).port(
                port("orgs"),
                Binding::List(vec![
                    Binding::Input(input("url")),
                    Binding::Input(input("url")),
                ]),
            ),
        );
    let errors = check(&workflow, &test_catalog())
        .expect_err("neither element is a GitHubOrg, and there is no HttpsUrl => GitHubOrg row");
    assert_eq!(
        errors,
        vec![
            CheckError::ListElementTypeMismatch {
                site: Site::list_element(node("sink"), port("orgs"), 0),
                expected: ty("GitHubOrg"),
                found: ty("HttpsUrl"),
            },
            CheckError::ListElementTypeMismatch {
                site: Site::list_element(node("sink"), port("orgs"), 1),
                expected: ty("GitHubOrg"),
                found: ty("HttpsUrl"),
            },
        ]
    );
    assert_eq!(
        errors[0].to_string(),
        "sink.orgs[0]: expected GitHubOrg, found `HttpsUrl`"
    );
}

/// A list-typed element -- here, a plain `Step` reference onto a
/// `for_each` node's own output, itself already `list<DopplerConfig>` --
/// is `ListElementTypeMismatch`: no flattening, exactly like a scalar
/// port would refuse the same reference.
#[test]
fn a_list_typed_element_is_rejected_with_no_flattening() {
    let workflow = common::new_rust_service_workflow().node(
        node("sink"),
        Node::new(tool_name("fake.list_sink.ensure")).port(
            port("orgs"),
            Binding::List(vec![Binding::Step {
                node: node("configs"),
                port: port("config"),
            }]),
        ),
    );
    let errors = check(&workflow, &test_catalog())
        .expect_err("configs.config, as a plain Step, is already list<DopplerConfig>");
    assert_eq!(
        errors,
        vec![CheckError::ListElementTypeMismatch {
            site: Site::list_element(node("sink"), port("orgs"), 0),
            expected: ty("GitHubOrg"),
            found: list_ty("DopplerConfig"),
        }]
    );
}

/// A secret element flowing into a list-typed, non-secret port is
/// `SecretToNonSecretSink`, attributed to its own `Site::ListElement`, and
/// reported instead of the type mismatch it also is -- exactly the scalar
/// rule (decision (a): "reported before any type mismatch").
#[test]
fn a_secret_element_in_a_list_is_a_taint_violation() {
    let workflow = Workflow::new(workflow_name("list-secret-element"))
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
            node("sink"),
            Node::new(tool_name("fake.list_sink.ensure")).port(
                port("orgs"),
                Binding::List(vec![Binding::Step {
                    node: node("token"),
                    port: port("token"),
                }]),
            ),
        );
    let errors = check(&workflow, &test_catalog())
        .expect_err("a secret token must not reach a non-secret list element");
    assert_eq!(
        errors,
        vec![CheckError::SecretToNonSecretSink {
            from: (node("token"), port("token")),
            to: Site::list_element(node("sink"), port("orgs"), 0),
        }]
    );
}

/// Positive control: a list binding whose elements need different
/// treatment -- one an exact match, one a one-hop conversion -- checks
/// cleanly and records one [`Edge`] per element, in order.
#[test]
fn list_binding_records_one_edge_per_element_with_conversion_where_needed() {
    // A `Binding::Literal` always parses through the *global* type
    // registry (`Value::parse`), never a catalog's own, so element 0 is an
    // `Input` of `ChainB` (an exact match, no conversion) rather than a
    // literal -- `chain_catalog`'s `ChainA`/`ChainB`/`ChainC` exist only in
    // its own isolated registry.
    let workflow = Workflow::new(workflow_name("list-per-element-edges"))
        .input(input("b"), InputSpec::new(ty("ChainB")))
        .input(input("a"), InputSpec::new(ty("ChainA")))
        .node(
            node("sink"),
            Node::new(tool_name("chain.sink_list_b")).port(
                port("xs"),
                Binding::List(vec![Binding::Input(input("b")), Binding::Input(input("a"))]),
            ),
        );
    let checked = check(&workflow, &chain_catalog())
        .expect("an exact ChainB and a converting ChainA input both belong in list<ChainB>");
    let edge = &checked.types[&node("sink")][&port("xs")];
    assert_eq!(edge.delivered(), list_ty("ChainB"));
    let elements = edge
        .elements()
        .expect("a list binding records element edges");
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].ty(), &ty("ChainB"));
    assert!(
        elements[0].conversion().is_none(),
        "an exact-match element is never converted"
    );
    assert_eq!(elements[1].ty(), &ty("ChainA"));
    assert_eq!(
        elements[1]
            .conversion()
            .expect("the ChainA element converts one hop")
            .to()
            .as_str(),
        "ChainB"
    );
}

/// An input referenced only as an element of a `Binding::List` counts as
/// used: no [`CheckWarning::UnusedInput`].
#[test]
fn an_input_used_only_inside_a_list_is_not_reported_unused() {
    let workflow = Workflow::new(workflow_name("list-marks-input-used"))
        .input(input("a"), InputSpec::new(ty("ChainA")))
        .node(
            node("sink"),
            Node::new(tool_name("chain.sink_list_b"))
                .port(port("xs"), Binding::List(vec![Binding::Input(input("a"))])),
        );
    let checked = check(&workflow, &chain_catalog()).expect("a converting element checks cleanly");
    assert!(checked.warnings.is_empty(), "{:?}", checked.warnings);
}

/// A `Binding::List` is never valid as a `for_each` source -- `check`
/// refuses it with `SequenceNotAllowedHere`, attributed to the node's own
/// `Site::ForEach`. Never produced by `willikins-dsl` (`for_each:` is a
/// plain string field, so a YAML sequence there fails to parse before
/// `check` ever runs), so this exercises a hand-built `Workflow` --
/// exactly the defensive path `Resolver::resolve`'s `Binding::List` arm
/// exists for. `environment`'s own `Binding::Item` binding raises no
/// second error: `ItemContext::ForEachBroken` cascade-suppresses it,
/// exactly as an already-broken `for_each` source already does for a
/// plain bad reference.
#[test]
fn a_list_bound_for_each_source_is_sequence_not_allowed_here() {
    let workflow = Workflow::new(workflow_name("list-for-each")).node(
        node("configs"),
        Node::new(tool_name("doppler.config.ensure"))
            .for_each(Binding::List(vec![Binding::Literal("dev".to_string())]))
            .port(port("project"), Binding::Literal("test-proj".to_string()))
            .port(port("environment"), Binding::Item),
    );
    let errors =
        check(&workflow, &test_catalog()).expect_err("a list can never be a for_each source");
    assert_eq!(
        errors,
        vec![CheckError::SequenceNotAllowedHere {
            site: Site::ForEach {
                node: node("configs"),
            },
        }]
    );
}

/// A `Binding::List` is never valid as a workflow output binding either --
/// same refusal, attributed to `Site::Output`. Also unreachable through
/// `willikins-dsl` (`outputs:` is a plain string map), so this too is the
/// defensive hand-built-`Workflow` path.
#[test]
fn a_list_bound_workflow_output_is_sequence_not_allowed_here() {
    let workflow = Workflow::new(workflow_name("list-output")).output(
        output("x"),
        Binding::List(vec![Binding::Literal("a".to_string())]),
    );
    let errors = check(&workflow, &test_catalog())
        .expect_err("a list can never be a workflow output binding");
    assert_eq!(
        errors,
        vec![CheckError::SequenceNotAllowedHere {
            site: Site::Output { name: output("x") },
        }]
    );
}
