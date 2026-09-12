#![allow(dead_code)]
//! Shared fixtures for `willikins-core`'s integration tests: small
//! constructors for the identifier types, and the milestone's positive
//! fixture (`workflows/new-rust-service.yaml`), built directly as a
//! [`Workflow`] since the YAML DSL does not exist yet (task 10). Each test
//! binary compiles this module on its own, so an item this particular
//! binary does not use is expected — hence the blanket `dead_code` allow
//! above, rather than annotating every unused helper individually.

use indexmap::IndexMap;

use willikins_core::{
    Binding, InputName, InputSpec, Node, NodeName, OutputName, PortName, ToolName, TypeName,
    TypeRef, Value, Workflow,
};
use willikins_types::{DomainType, EnvironmentSlug, RepoVisibility};

pub fn ty(name: &str) -> TypeRef {
    TypeRef::scalar(TypeName::parse(name).unwrap())
}

pub fn list_ty(name: &str) -> TypeRef {
    TypeRef::list_of(TypeName::parse(name).unwrap())
}

pub fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

pub fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

pub fn input(name: &str) -> InputName {
    InputName::parse(name).unwrap()
}

pub fn output(name: &str) -> OutputName {
    OutputName::parse(name).unwrap()
}

pub fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).unwrap()
}

/// `workflows/new-rust-service.yaml`, built directly as a [`Workflow`].
#[allow(clippy::too_many_lines)]
pub fn new_rust_service_workflow() -> Workflow {
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

/// Fully resolved inputs for [`new_rust_service_workflow`]: `slug =
/// third-thoughts`, `org = lightless-labs`, and the declared defaults for
/// `visibility` and `environments`.
pub fn new_rust_service_inputs() -> IndexMap<InputName, Value> {
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("slug"),
        Value::known(willikins_types::ProjectSlug::parse("third-thoughts").unwrap()),
    );
    inputs.insert(
        input("org"),
        Value::known(willikins_types::GitHubOrg::parse("lightless-labs").unwrap()),
    );
    inputs.insert(input("visibility"), Value::known(RepoVisibility::Private));
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
