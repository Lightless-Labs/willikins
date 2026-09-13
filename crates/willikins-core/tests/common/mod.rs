#![allow(dead_code)]
//! Shared fixtures for `willikins-core`'s integration tests: small
//! constructors for the identifier types, and the milestone's positive
//! fixture (`workflows/new-rust-service.yaml`), built directly as a
//! [`Workflow`] since the YAML DSL does not exist yet (task 10). Each test
//! binary compiles this module on its own, so an item this particular
//! binary does not use is expected — hence the blanket `dead_code` allow
//! above, rather than annotating every unused helper individually.

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use willikins_core::{
    Binding, Catalog, Class, Ensured, InputName, InputSpec, Inputs, Node, NodeName, Observation,
    OutputName, Outputs, PortName, PrincipalId, SinkToken, Timestamp, Tool, ToolError,
    ToolErrorKind, ToolName, ToolSpec, TypeName, TypeRef, Value, Workflow,
};
use willikins_providers_fake::state::{FakeState, doppler_service_token_key};
use willikins_types::{
    DomainType, DopplerConfig, DopplerServiceToken, DopplerTokenName, EnvironmentSlug,
    RepoVisibility, WorkflowName,
};

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

pub fn workflow_name(name: &str) -> WorkflowName {
    WorkflowName::parse(name).unwrap()
}

pub fn description(text: &str) -> willikins_types::Description {
    willikins_types::Description::parse(text).unwrap()
}

/// `workflows/new-rust-service.yaml`, built directly as a [`Workflow`].
#[allow(clippy::too_many_lines)]
pub fn new_rust_service_workflow() -> Workflow {
    Workflow::new(workflow_name("new-rust-service"))
        .with_description(description(
            "Provision a GitHub repository and Doppler project for a Rust service.",
        ))
        .input(
            input("slug"),
            InputSpec::new(ty("ProjectSlug"))
                .with_description(description("Canonical project slug")),
        )
        .input(
            input("org"),
            InputSpec::new(ty("GitHubOrg"))
                .with_description(description("GitHub organization that owns the repository")),
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

/// [`new_rust_service_workflow`] plus one `fake.irreversible.ensure` node
/// keyed on `slug` — the same shape as
/// `workflows/fixtures/irreversible.yaml`, built directly since these
/// tests exercise `willikins-core` without going through the DSL.
pub fn irreversible_workflow() -> Workflow {
    new_rust_service_workflow().node(
        node("danger"),
        Node::new(tool_name("fake.irreversible.ensure"))
            .port(port("key"), Binding::Input(input("slug"))),
    )
}

pub fn principal(name: &str) -> PrincipalId {
    PrincipalId::parse(name).unwrap()
}

pub fn timestamp(rfc3339: &str) -> Timestamp {
    Timestamp::parse(rfc3339).unwrap()
}

/// A distinctive [`DopplerServiceToken`], used as the seeded/minted
/// marker in apply-executor tests: it must never appear anywhere an
/// `Applied` result or a journal renders text, only its redaction marker.
pub fn distinctive_token() -> DopplerServiceToken {
    DopplerServiceToken::parse(&format!("dp.st.prd.{}", "MARKER".repeat(7))).unwrap()
}

/// A drop-in substitute for `willikins_providers_fake`'s
/// `doppler.service_token.ensure`, sharing its exact [`ToolSpec`] (name,
/// ports, key, class) and its own state map, differing only in what
/// `ensure` returns on creation.
///
/// The real fake tool (as it stands before task 4b adds
/// `FakeState::next_token`) always reports the `token` output
/// [`Value::unknown`], on both `read` and `ensure` alike — correct for
/// `read` (a service token's value can never be re-read once issued), but
/// wrong for a freshly minted `ensure` call: the milestone plan's own
/// "Convergence" section describes `apply`'s `UnknownInput` gap as a
/// *second*-run phenomenon (the consumer of a token that already existed,
/// so could not be re-read this time), which only makes sense if a
/// *first*-run `ensure` on a not-yet-existing token hands its consumer a
/// `Known` value. This substitute does that, minting [`distinctive_token`]
/// on the create path (`changed: true`) and reporting `Unknown` on every
/// other path (`changed: false`, or `read`), so:
///
/// - the happy-path apply test can actually observe `ci_secret` `Created`
///   rather than universally blocked by `ApplyError::UnknownInput`;
/// - the distinctive marker gives the redaction tests something to prove
///   never leaks;
/// - seeding the token as already-existing still reproduces the
///   `UnknownInput` gap the design describes, since a `Present` token's
///   value is still `Unknown` here, exactly like the real fake tool.
///
/// This is a test-local stand-in, not an edit to `willikins-providers-fake`
/// (task 4b's own job): see the task notes for why the acceptance
/// tests this crate's own `tests/apply.rs` implements cannot pass against
/// the shared fake catalog as it stands today.
pub struct FixedTokenService {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl FixedTokenService {
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("config"),
            willikins_core::PortSpec {
                ty: willikins_core::PortType::Exact(ty("DopplerConfig")),
                required: true,
            },
        );
        inputs.insert(
            port("name"),
            willikins_core::PortSpec {
                ty: willikins_core::PortType::Exact(ty("DopplerTokenName")),
                required: true,
            },
        );
        let mut outputs = IndexMap::new();
        outputs.insert(port("token"), ty("DopplerServiceToken"));
        Self {
            spec: ToolSpec {
                name: tool_name("doppler.service_token.ensure"),
                description: "Test substitute for doppler.service_token.ensure.".to_string(),
                inputs,
                outputs,
                key: vec![port("config"), port("name")],
                class: Class::Reversible,
                pure: false,
            },
            state,
        }
    }

    fn key_ports(inputs: &Inputs) -> (DopplerConfig, DopplerTokenName) {
        let config = inputs
            .get(&port("config"))
            .and_then(Value::downcast::<DopplerConfig>)
            .cloned()
            .expect("test tool always called with a known config");
        let name = inputs
            .get(&port("name"))
            .and_then(Value::downcast::<DopplerTokenName>)
            .cloned()
            .expect("test tool always called with a known name");
        (config, name)
    }

    fn unknown_outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("token"), Value::unknown(ty("DopplerServiceToken")));
        outputs
    }
}

impl Tool for FixedTokenService {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        let (config, name) = Self::key_ports(inputs);
        let state = self.state.lock().unwrap();
        if state
            .doppler_service_tokens
            .contains(&doppler_service_token_key(&config, &name))
        {
            Ok(Observation::Present(Self::unknown_outputs()))
        } else {
            Ok(Observation::Absent {
                predicted: Self::unknown_outputs(),
            })
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        let (config, name) = Self::key_ports(inputs);
        let mut state = self.state.lock().unwrap();
        let key = doppler_service_token_key(&config, &name);
        if state.doppler_service_tokens.contains(&key) {
            Ok(Ensured {
                outputs: Self::unknown_outputs(),
                changed: false,
            })
        } else {
            state.doppler_service_tokens.insert(key);
            let mut outputs = Outputs::new();
            outputs.insert(port("token"), Value::known(distinctive_token()));
            Ok(Ensured {
                outputs,
                changed: true,
            })
        }
    }
}

/// Every fake tool `willikins_providers_fake::catalog` registers, plus
/// `willikins-tools`' two pure tools, except `doppler.service_token.ensure`
/// is [`FixedTokenService`] instead of the crate's own
/// `DopplerServiceTokenEnsure` — see that type's own doc for why. Not an
/// edit to `willikins-providers-fake`: every other tool comes from that
/// crate's own public constructors, sharing the same `state`.
#[must_use]
pub fn apply_test_catalog(state: Arc<Mutex<FakeState>>) -> Catalog {
    let mut catalog = Catalog::new(willikins_types::registry());
    macro_rules! insert {
        ($tool:expr) => {
            catalog.insert(Arc::new($tool)).unwrap()
        };
    }
    insert!(willikins_tools::NamingV1::new());
    insert!(willikins_providers_fake::tools::GitHubRepoEnsure::new(
        state.clone()
    ));
    insert!(willikins_providers_fake::tools::GitHubActionsSecretEnsure::new(state.clone()));
    insert!(willikins_providers_fake::tools::DopplerProjectEnsure::new(
        state.clone()
    ));
    insert!(willikins_providers_fake::tools::DopplerConfigEnsure::new(
        state.clone()
    ));
    insert!(FixedTokenService::new(state.clone()));
    insert!(willikins_providers_fake::tools::DopplerSecretGet::new(
        state.clone()
    ));
    insert!(willikins_providers_fake::tools::FakeSecretList::new());
    insert!(willikins_providers_fake::tools::FakeIrreversibleEnsure::new(state));
    insert!(willikins_tools::TemplateRender::new());
    catalog
}

/// A minimal, keyless, non-pure test tool used only to exercise the apply
/// executor's own control flow (a tool failure, or a plain success),
/// independent of any fake provider. `read` always reports `Absent`; when
/// `fail` is `false`, `ensure` always succeeds with no outputs and
/// `changed: true`; when `true`, `ensure` always fails with
/// [`ToolErrorKind::Provider`].
pub struct ScriptedEnsureTool {
    spec: ToolSpec,
    fail: bool,
}

impl ScriptedEnsureTool {
    #[must_use]
    pub fn new(name: &str, fail: bool) -> Self {
        Self {
            spec: ToolSpec {
                name: tool_name(name),
                description: "Test tool: scripted ensure outcome.".to_string(),
                inputs: IndexMap::new(),
                outputs: IndexMap::new(),
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
            fail,
        }
    }
}

impl Tool for ScriptedEnsureTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Absent {
            predicted: Outputs::new(),
        })
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        if self.fail {
            Err(ToolError {
                kind: ToolErrorKind::Provider,
                message: "boom".to_string(),
            })
        } else {
            Ok(Ensured {
                outputs: Outputs::new(),
                changed: true,
            })
        }
    }
}

/// A minimal, keyless, non-pure test tool that always reports its one
/// resource `Present`, with its one output port always `Unknown` (as if it
/// were a secret that cannot be re-read) — an upstream node that ends
/// `Converged`/`Unchanged` but can never hand its real value downstream.
/// Used to exercise `ApplyError::UnknownInput`.
pub struct UnreadableUpstreamTool {
    spec: ToolSpec,
}

impl UnreadableUpstreamTool {
    #[must_use]
    pub fn new(name: &str) -> Self {
        let mut outputs = IndexMap::new();
        outputs.insert(port("value"), ty("GitHubOrg"));
        Self {
            spec: ToolSpec {
                name: tool_name(name),
                description: "Test tool: always present, output always unknown.".to_string(),
                inputs: IndexMap::new(),
                outputs,
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
        }
    }

    fn outputs() -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("value"), Value::unknown(ty("GitHubOrg")));
        outputs
    }
}

impl Tool for UnreadableUpstreamTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
        Ok(Observation::Present(Self::outputs()))
    }

    fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        Ok(Ensured {
            outputs: Self::outputs(),
            changed: false,
        })
    }
}

/// A minimal test tool requiring one input (`value`, an exact `GitHubOrg`
/// port), always `Absent`/`Create`, with a trivial `ensure`. Used alongside
/// [`UnreadableUpstreamTool`] to exercise `ApplyError::UnknownInput`: its
/// required `value` input, bound to the upstream tool's own always-unknown
/// output, can never be satisfied.
pub struct RequiresKnownInputTool {
    spec: ToolSpec,
}

impl RequiresKnownInputTool {
    #[must_use]
    pub fn new(name: &str) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(
            port("value"),
            willikins_core::PortSpec {
                ty: willikins_core::PortType::Exact(ty("GitHubOrg")),
                required: true,
            },
        );
        Self {
            spec: ToolSpec {
                name: tool_name(name),
                description: "Test tool: requires a known input.".to_string(),
                inputs,
                outputs: IndexMap::new(),
                key: Vec::new(),
                class: Class::Reversible,
                pure: false,
            },
        }
    }
}

impl Tool for RequiresKnownInputTool {
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
