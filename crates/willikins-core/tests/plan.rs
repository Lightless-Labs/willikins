//! Integration tests for [`willikins_core::plan`], against
//! `willikins-providers-fake`'s real fake tools and the milestone's
//! positive fixture (see `tests/common`), plus a handful of dummy tools for
//! scenarios no fake tool can produce on its own.

mod common;

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use common::{input, list_ty, node, output, port, tool_name, ty, workflow_name};
use willikins_core::{
    Action, Binding, Catalog, Class, Ensured, InputSpec, Inputs, Node, Observation, Outputs,
    PlanError, PortSpec, PortType, SinkToken, Site, Tool, ToolError, ToolSpec, Value, Workflow,
    check, plan,
};
use willikins_providers_fake::{FakeState, catalog, empty};
use willikins_types::{
    DomainType, DopplerConfig, DopplerSecretValue, GitHubRepo, RepoVisibility, SecretName,
};

// -------------------------------------------------------------
// Acceptance test 6: plan against empty state
// -------------------------------------------------------------

#[test]
fn acceptance_6_plan_against_empty_state() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).expect("the positive fixture checks cleanly");
    let inputs = common::new_rust_service_inputs();
    let result = plan(&checked, &inputs, &fake_catalog).expect("empty state plans cleanly");

    assert_eq!(result.workflow.as_str(), "new-rust-service");
    assert_eq!(result.class, Class::Reversible);
    assert!(!result.requires_approval);

    // Wire pin: `Plan::workflow` still serializes as a plain JSON string
    // under the same field name, so a caller that never sees the Rust
    // type is unaffected by task 1e's core-side type change.
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["workflow"], "new-rust-service");

    let by_name = |name: &str, instance: Option<&str>| {
        result
            .nodes
            .iter()
            .find(|n| n.name.as_str() == name && n.instance.as_deref() == instance)
            .unwrap_or_else(|| panic!("no planned node `{name}` (instance {instance:?})"))
    };

    assert_eq!(by_name("names", None).action, Action::Compute);
    assert_eq!(by_name("repo", None).action, Action::Create);
    assert_eq!(by_name("doppler", None).action, Action::Create);
    assert_eq!(by_name("token", None).action, Action::Create);
    assert_eq!(by_name("ci_secret", None).action, Action::Create);

    let config_instances: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| n.name.as_str() == "configs")
        .collect();
    assert_eq!(config_instances.len(), 3);
    let keys: Vec<&str> = config_instances
        .iter()
        .map(|n| n.instance.as_deref().unwrap())
        .collect();
    assert_eq!(keys, vec!["dev", "stg", "prd"]);
    for instance in &config_instances {
        assert_eq!(instance.action, Action::Create);
    }

    let token_outputs = &by_name("token", None).outputs;
    let token_value = token_outputs.get(&port("token")).unwrap();
    assert!(!token_value.is_known());

    let ci_secret_inputs = &by_name("ci_secret", None).inputs;
    let value = ci_secret_inputs.get(&port("value")).unwrap();
    assert!(!value.is_known());

    let repo_outputs = &by_name("repo", None).outputs;
    let url = repo_outputs.get(&port("url")).unwrap();
    assert_eq!(
        url.render().to_string(),
        "https://github.com/lightless-labs/third-thoughts"
    );
}

#[test]
fn acceptance_6_insta_snapshot_of_the_empty_state_plan() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let inputs = common::new_rust_service_inputs();
    let result = plan(&checked, &inputs, &fake_catalog).unwrap();
    insta::assert_snapshot!(serde_json::to_string_pretty(&result).unwrap());
}

#[test]
fn acceptance_6_an_irreversible_node_makes_the_plan_require_approval() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow().node(
        node("danger"),
        Node::new(tool_name("fake.irreversible.ensure"))
            .port(port("key"), Binding::Input(input("slug"))),
    );
    let checked =
        check(&workflow, &fake_catalog).expect("adding an irreversible node still checks");
    let inputs = common::new_rust_service_inputs();
    let result = plan(&checked, &inputs, &fake_catalog).unwrap();
    assert_eq!(result.class, Class::Irreversible);
    assert!(result.requires_approval);
    let danger = result
        .nodes
        .iter()
        .find(|n| n.name.as_str() == "danger")
        .unwrap();
    assert_eq!(danger.action, Action::Create);
}

#[test]
fn a_step_reference_into_a_for_each_node_aggregates_a_known_list_of_three() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow().output(
        output("all_configs"),
        Binding::Step {
            node: node("configs"),
            port: port("config"),
        },
    );
    let checked = check(&workflow, &fake_catalog).expect("a plain Step onto configs still checks");
    let inputs = common::new_rust_service_inputs();
    let result = plan(&checked, &inputs, &fake_catalog).unwrap();
    let all_configs = result.outputs.get(&output("all_configs")).unwrap();
    assert!(all_configs.is_known());
    let rendered: Vec<String> = all_configs
        .as_list()
        .unwrap()
        .iter()
        .map(|object| object.render().to_string())
        .collect();
    assert_eq!(
        rendered,
        vec![
            "third-thoughts/dev".to_string(),
            "third-thoughts/stg".to_string(),
            "third-thoughts/prd".to_string(),
        ]
    );
}

// -------------------------------------------------------------
// Acceptance test 7: plan against seeded state
// -------------------------------------------------------------

#[test]
fn acceptance_7_a_repo_that_exists_and_is_ours_is_a_no_op_with_a_known_url() {
    let repo = GitHubRepo::parse("lightless-labs/third-thoughts").unwrap();
    let state = Arc::new(Mutex::new(FakeState::new().with_repo(
        &repo,
        RepoVisibility::Private,
        true,
    )));
    let fake_catalog = catalog(state);
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let inputs = common::new_rust_service_inputs();
    let result = plan(&checked, &inputs, &fake_catalog).unwrap();

    let repo_node = result
        .nodes
        .iter()
        .find(|n| n.name.as_str() == "repo")
        .unwrap();
    assert_eq!(repo_node.action, Action::NoOp);
    let url = repo_node.outputs.get(&port("url")).unwrap();
    assert!(url.is_known());
    assert_eq!(
        url.render().to_string(),
        "https://github.com/lightless-labs/third-thoughts"
    );
}

#[test]
fn acceptance_7_a_foreign_repo_is_name_taken() {
    let repo = GitHubRepo::parse("lightless-labs/third-thoughts").unwrap();
    let state = Arc::new(Mutex::new(FakeState::new().with_repo(
        &repo,
        RepoVisibility::Public,
        false,
    )));
    let fake_catalog = catalog(state);
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let inputs = common::new_rust_service_inputs();
    let err = plan(&checked, &inputs, &fake_catalog).unwrap_err();
    match err {
        PlanError::NameTaken { node: n, tool, key } => {
            assert_eq!(n, node("repo"));
            assert_eq!(tool, tool_name("github.repo.ensure"));
            assert!(key.get(&port("repo")).is_some());
        }
        other => panic!("expected NameTaken, got {other:?}"),
    }
}

#[test]
fn acceptance_7_environments_dev_qa_with_configs_prd_referenced_is_key_not_in_for_each() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    let mut inputs = common::new_rust_service_inputs();
    inputs.insert(
        input("environments"),
        Value::known_list(vec![
            willikins_types::EnvironmentSlug::parse("dev").unwrap(),
            willikins_types::EnvironmentSlug::parse("qa").unwrap(),
        ]),
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
// Acceptance test 8b: a known secret in a plan stays redacted
// -------------------------------------------------------------

#[test]
fn acceptance_8b_a_known_secret_output_stays_redacted_in_json_and_debug() {
    let config = DopplerConfig::parse("third-thoughts/prd").unwrap();
    let name = SecretName::parse("DATABASE_URL").unwrap();
    let seeded = DopplerSecretValue::parse("s3cr3t-bytes-nobody-should-see").unwrap();
    let state = Arc::new(Mutex::new(
        FakeState::new().with_doppler_secret(&config, &name, seeded),
    ));
    let fake_catalog = catalog(state);

    let workflow = Workflow::new(workflow_name("secret-get"))
        .input(input("config"), InputSpec::new(ty("DopplerConfig")))
        .node(
            node("get"),
            Node::new(tool_name("doppler.secret.get"))
                .port(port("config"), Binding::Input(input("config")))
                .port(port("name"), Binding::Literal("DATABASE_URL".to_string())),
        )
        .output(
            output("value"),
            Binding::Step {
                node: node("get"),
                port: port("value"),
            },
        );
    let checked = check(&workflow, &fake_catalog).unwrap();
    let mut inputs = IndexMap::new();
    inputs.insert(input("config"), Value::known(config));
    let result = plan(&checked, &inputs, &fake_catalog).unwrap();

    let json = serde_json::to_string_pretty(&result).unwrap();
    assert!(json.contains("[REDACTED DopplerSecretValue]"), "{json}");
    assert!(!json.contains("s3cr3t-bytes-nobody-should-see"), "{json}");

    let debug = format!("{result:?}");
    assert!(debug.contains("REDACTED"), "{debug}");
    assert!(!debug.contains("s3cr3t-bytes-nobody-should-see"), "{debug}");
}

// -------------------------------------------------------------
// KeyUnknown and ForEachUnknown, with dummy tools
// -------------------------------------------------------------

/// A tool whose spec is fixed at construction; `read` always reports
/// `Absent` with no predicted outputs at all, so every declared output
/// port comes back `Unknown` once `plan` fills it in.
struct AlwaysAbsent {
    spec: ToolSpec,
}

impl Tool for AlwaysAbsent {
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

fn dummy_spec(
    name: &str,
    inputs: &[(&str, PortType, bool)],
    outputs: &[(&str, willikins_core::TypeRef)],
    key: &[&str],
    pure: bool,
) -> ToolSpec {
    let mut in_map = IndexMap::new();
    for (port_name, port_ty, required) in inputs {
        in_map.insert(
            port(port_name),
            PortSpec {
                ty: port_ty.clone(),
                required: *required,
                derived_only: false,
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
        class: Class::Reversible,
        pure,
    }
}

fn exact(name: &str) -> PortType {
    PortType::Exact(ty(name))
}

#[test]
fn key_unknown_when_a_tools_key_port_resolves_to_unknown() {
    let mut fake_catalog = Catalog::new(willikins_types::registry());
    fake_catalog
        .insert(Arc::new(AlwaysAbsent {
            spec: dummy_spec(
                "test.produces_unknown",
                &[],
                &[("out", ty("GitHubOrg"))],
                &[],
                false,
            ),
        }))
        .unwrap();
    fake_catalog
        .insert(Arc::new(AlwaysAbsent {
            spec: dummy_spec(
                "test.needs_known_key",
                &[("out", exact("GitHubOrg"), true)],
                &[],
                &["out"],
                false,
            ),
        }))
        .unwrap();

    let workflow = Workflow::new(workflow_name("key-unknown"))
        .node(
            node("producer"),
            Node::new(tool_name("test.produces_unknown")),
        )
        .node(
            node("consumer"),
            Node::new(tool_name("test.needs_known_key")).port(
                port("out"),
                Binding::Step {
                    node: node("producer"),
                    port: port("out"),
                },
            ),
        );
    let checked = check(&workflow, &fake_catalog).expect("both dummy tools check cleanly");
    let err = plan(&checked, &IndexMap::new(), &fake_catalog).unwrap_err();
    match err {
        PlanError::KeyUnknown { node: n, port: p } => {
            assert_eq!(n, node("consumer"));
            assert_eq!(p, port("out"));
        }
        other => panic!("expected KeyUnknown, got {other:?}"),
    }
}

#[test]
fn for_each_unknown_when_the_source_resolves_to_unknown() {
    let mut fake_catalog = Catalog::new(willikins_types::registry());
    fake_catalog
        .insert(Arc::new(AlwaysAbsent {
            spec: dummy_spec(
                "test.produces_unknown_list",
                &[],
                &[("out", list_ty("GitHubOrg"))],
                &[],
                false,
            ),
        }))
        .unwrap();
    fake_catalog
        .insert(Arc::new(AlwaysAbsent {
            spec: dummy_spec(
                "test.consumes_item",
                &[("value", exact("GitHubOrg"), true)],
                &[],
                &[],
                false,
            ),
        }))
        .unwrap();

    let workflow = Workflow::new(workflow_name("for-each-unknown"))
        .node(
            node("producer"),
            Node::new(tool_name("test.produces_unknown_list")),
        )
        .node(
            node("consumer"),
            Node::new(tool_name("test.consumes_item"))
                .for_each(Binding::Step {
                    node: node("producer"),
                    port: port("out"),
                })
                .port(port("value"), Binding::Item),
        );
    let checked = check(&workflow, &fake_catalog).expect("both dummy tools check cleanly");
    let err = plan(&checked, &IndexMap::new(), &fake_catalog).unwrap_err();
    match err {
        PlanError::ForEachUnknown { node: n } => assert_eq!(n, node("consumer")),
        other => panic!("expected ForEachUnknown, got {other:?}"),
    }
}

// -------------------------------------------------------------
// MissingInput and PlanError::Display
// -------------------------------------------------------------

#[test]
fn missing_input_when_a_binding_names_an_input_the_caller_did_not_supply() {
    let (_state, fake_catalog) = empty();
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &fake_catalog).unwrap();
    // Deliberately omit `slug`.
    let mut inputs = common::new_rust_service_inputs();
    inputs.shift_remove(&input("slug"));
    let err = plan(&checked, &inputs, &fake_catalog).unwrap_err();
    match err {
        PlanError::MissingInput { input: i } => assert_eq!(i, input("slug")),
        other => panic!("expected MissingInput, got {other:?}"),
    }
}

#[test]
fn plan_error_display_is_one_line() {
    let err = PlanError::KeyNotInForEach {
        site: Site::Port {
            node: node("token"),
            port: port("config"),
        },
        key: "prd".to_string(),
    };
    let message = err.to_string();
    assert!(!message.contains('\n'));
    assert!(message.contains("token"));
    assert!(message.contains("prd"));
}

// -------------------------------------------------------------
// AttributeMismatch, with a dummy tool
// -------------------------------------------------------------

/// A tool whose `read` always reports [`Observation::Mismatch`] at a fixed
/// port — including, deliberately, a port the node never bound and one the
/// tool does not even declare, which is how a buggy provider would behave.
/// `ensure` refuses the same way every fake tool does, so nothing can run
/// past the refusal.
struct AlwaysMismatch {
    spec: ToolSpec,
    mismatched: willikins_core::PortName,
}

impl Tool for AlwaysMismatch {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Mismatch {
            port: self.mismatched.clone(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Err(ToolError {
            kind: willikins_core::ToolErrorKind::Conflict,
            message: "the resource is ours but does not match".to_string(),
        })
    }
}

/// A catalog holding one `AlwaysMismatch` tool that reports `mismatched`.
fn mismatching_catalog(mismatched: &str) -> Catalog {
    let mut fake_catalog = Catalog::new(willikins_types::registry());
    fake_catalog
        .insert(Arc::new(AlwaysMismatch {
            spec: dummy_spec(
                "test.always_mismatch",
                &[("value", exact("GitHubOrg"), true)],
                &[],
                &["value"],
                false,
            ),
            mismatched: port(mismatched),
        }))
        .unwrap();
    fake_catalog
}

/// A `for_each` node's instance mismatching is still refused, and the site
/// names the node the instance belongs to. `Site::Port` carries no
/// instance key, so an error from a three-instance node names the node and
/// the port only — see the report accompanying this test.
#[test]
fn attribute_mismatch_reaches_plan_through_a_for_each_instance() {
    let fake_catalog = mismatching_catalog("value");
    let workflow = Workflow::new(workflow_name("for-each-mismatch"))
        .input(input("orgs"), InputSpec::new(list_ty("GitHubOrg")))
        .node(
            node("consumer"),
            Node::new(tool_name("test.always_mismatch"))
                .for_each(Binding::Input(input("orgs")))
                .port(port("value"), Binding::Item),
        );
    let checked = check(&workflow, &fake_catalog).expect("the dummy tool checks cleanly");
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("orgs"),
        Value::known_list(vec![
            willikins_types::GitHubOrg::parse("first-org").unwrap(),
            willikins_types::GitHubOrg::parse("second-org").unwrap(),
        ]),
    );
    let err = plan(&checked, &inputs, &fake_catalog).unwrap_err();
    match err {
        PlanError::AttributeMismatch { site } => assert_eq!(
            site,
            Site::Port {
                node: node("consumer"),
                port: port("value"),
            }
        ),
        other => panic!("expected AttributeMismatch, got {other:?}"),
    }
}

/// A tool that names a port the node never bound — and that the tool does
/// not declare as an input at all — is a bug in that tool. `plan` must
/// still report it as a `PlanError`, naming the port the tool named,
/// rather than panicking on a lookup that cannot succeed.
#[test]
fn attribute_mismatch_on_a_port_the_node_never_bound_is_an_error_not_a_panic() {
    let fake_catalog = mismatching_catalog("ghost");
    let workflow = Workflow::new(workflow_name("ghost-mismatch")).node(
        node("only"),
        Node::new(tool_name("test.always_mismatch")).port(
            port("value"),
            Binding::Literal("lightless-labs".to_string()),
        ),
    );
    let checked = check(&workflow, &fake_catalog).expect("the dummy tool checks cleanly");
    let err = plan(&checked, &IndexMap::new(), &fake_catalog).unwrap_err();
    match err {
        PlanError::AttributeMismatch { site } => {
            assert_eq!(
                site,
                Site::Port {
                    node: node("only"),
                    port: port("ghost"),
                }
            );
            assert!(err_mentions(
                &PlanError::AttributeMismatch { site },
                "only.ghost"
            ));
        }
        other => panic!("expected AttributeMismatch, got {other:?}"),
    }
}

/// Whether `error`'s one-line `Display` contains `needle`.
fn err_mentions(error: &PlanError, needle: &str) -> bool {
    error.to_string().contains(needle)
}
