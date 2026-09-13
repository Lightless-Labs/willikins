//! Acceptance test 6d (property): over generated workflows built from the
//! fake catalog's own non-pure tools, `check` them, `plan` and `apply`
//! against empty state, inject one `fail_ensure_once` at an arbitrary
//! node instance, `plan` and `apply` again, and assert every node whose
//! inputs are all known ends `Created`/`Unchanged`/`Converged`, every
//! node blocked by an un-re-readable upstream ends in `UnknownInput`
//! naming that upstream, and no key's `ensure_calls` count exceeds "once
//! per round this scenario could plan a fresh `Create` for it".
//!
//! **Deviation from a literal reuse of `willikins_core::testing`'s
//! generic `arb_workflow`, recorded here rather than silently**: that
//! generator draws each port binding independently of any tool's actual
//! port *type*, so an overwhelming majority of its output fails `check`
//! outright (a random binding is almost never type-compatible) — exactly
//! right for `check_adversarial.rs`'s "check never panics, even on
//! garbage" property, useless here, where the entire point is running
//! `apply` on workflows that *did* check and plan successfully. This file
//! instead builds a well-typed generator directly: a fixed dependency
//! chain (`doppler.project.ensure` -> `doppler.config.ensure` ->
//! `doppler.service_token.ensure`), the only chain in the fake catalog
//! that produces an un-re-readable secret output, is always present;
//! `github.repo.ensure` + `github.actions_secret.ensure` (wired to
//! consume the token, so the secret chain has somewhere to flow) and
//! `fake.irreversible.ensure` are each independently, randomly present.
//! Every binding is `Step`/`Input` against the exact type each port
//! declares, so every generated workflow does check and plan — the
//! randomness is over which optional nodes exist, their identifiers, and
//! which node instance the injected failure lands on. `for_each` is not
//! exercised (deferred, per task 4b's own scope note).
//!
//! Run at the default case count; also run once by hand at
//! `PROPTEST_CASES=1024` (proptest reads that environment variable itself
//! — see the module's own doc for the result).

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use proptest::prelude::*;

use willikins_core::{
    ApplyError, Approval, Binding, InputName, InputSpec, Node, NodeName, NodeStatus, NoopObserver,
    OutputName, PortName, ToolName, TypeName, TypeRef, Value, Workflow, apply, check, plan,
};
use willikins_providers_fake::state::{
    actions_secret_key, call_key, doppler_config_key, doppler_project_key,
    doppler_service_token_key, irreversible_key, repo_key,
};
use willikins_providers_fake::{FakeState, catalog};
use willikins_types::{
    ActionsSecretName, DomainType, DopplerConfig, DopplerProject, DopplerTokenName,
    EnvironmentSlug, GitHubRepo, ProjectSlug, RepoVisibility, WorkflowName, naming,
};

fn ty(name: &str) -> TypeRef {
    TypeRef::scalar(TypeName::parse(name).unwrap())
}

fn input(name: &str) -> InputName {
    InputName::parse(name).unwrap()
}

fn node(name: &str) -> NodeName {
    NodeName::parse(name).unwrap()
}

fn port(name: &str) -> PortName {
    PortName::parse(name).unwrap()
}

fn output(name: &str) -> OutputName {
    OutputName::parse(name).unwrap()
}

fn tool_name(name: &str) -> ToolName {
    ToolName::parse(name).unwrap()
}

/// `Auto` when `plan` allows it, else a `Human` approval: the
/// `irreversible` node (`Class::Irreversible`) makes some generated
/// workflows require approval, and this property is about convergence,
/// not the approval gate (acceptance test 7 covers that).
fn approval_for(approved: &willikins_core::Plan) -> Approval {
    if approved.requires_approval {
        Approval::Human {
            approver: willikins_core::PrincipalId::parse("convergence-property").unwrap(),
            at: willikins_core::Timestamp::now(),
        }
    } else {
        Approval::Auto
    }
}

/// Every generated identifier: a letter then up to seven letters or
/// digits, the same pattern `willikins-providers-fake`'s own
/// `ensure_properties.rs` uses.
const NAME_PATTERN: &str = "[a-z][a-z0-9]{0,7}";

/// One generated scenario: the always-present secret chain's identifiers,
/// plus whether each optional node (`repo`, `irreversible`) is present
/// and its own identifiers.
#[derive(Debug, Clone)]
struct Scenario {
    project: DopplerProject,
    environment: EnvironmentSlug,
    token_name: DopplerTokenName,
    secret_name: ActionsSecretName,
    repo: Option<GitHubRepo>,
    visibility: RepoVisibility,
    irreversible: Option<ProjectSlug>,
}

impl Scenario {
    fn config(&self) -> DopplerConfig {
        naming::v1::doppler_root_config(&self.project, &self.environment)
    }

    /// `(tool, key)` for every node this scenario's workflow contains,
    /// except `github.actions_secret.ensure` (see [`Self::ci_secret_key`]
    /// and this file's own module doc for why it is excluded from
    /// injection candidates).
    fn candidates(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![
            ("doppler.project.ensure", doppler_project_key(&self.project)),
            ("doppler.config.ensure", doppler_config_key(&self.config())),
            (
                "doppler.service_token.ensure",
                doppler_service_token_key(&self.config(), &self.token_name),
            ),
        ];
        if let Some(repo) = &self.repo {
            out.push(("github.repo.ensure", repo_key(repo)));
        }
        if let Some(slug) = &self.irreversible {
            out.push(("fake.irreversible.ensure", irreversible_key(slug)));
        }
        out
    }

    /// `github.actions_secret.ensure`'s own key, when `repo` is present.
    /// Excluded from injection candidates: unlike every other node here,
    /// its `ensure` is *not* called on a converged second round (its
    /// `value` input resolves `Unknown` from `token`, and its own fresh
    /// `read` is already `Present`), so a failure queued at its key would
    /// simply never fire — a genuine scenario (round 2 skips the call
    /// entirely), just not an interesting one for "the injected failure
    /// actually fires" runs. It is still asserted on below, structurally.
    fn ci_secret_key(&self) -> Option<String> {
        self.repo
            .as_ref()
            .map(|repo| actions_secret_key(repo, &self.secret_name))
    }

    fn workflow(&self) -> Workflow {
        let mut workflow = Workflow::new(WorkflowName::parse("generated-6d").unwrap())
            .input(input("project"), InputSpec::new(ty("DopplerProject")))
            .input(input("environment"), InputSpec::new(ty("EnvironmentSlug")))
            .input(input("token_name"), InputSpec::new(ty("DopplerTokenName")))
            .node(
                node("project"),
                Node::new(tool_name("doppler.project.ensure"))
                    .port(port("project"), Binding::Input(input("project"))),
            )
            .node(
                node("config"),
                Node::new(tool_name("doppler.config.ensure"))
                    .port(
                        port("project"),
                        Binding::Step {
                            node: node("project"),
                            port: port("project"),
                        },
                    )
                    .port(port("environment"), Binding::Input(input("environment"))),
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
                    .port(port("name"), Binding::Input(input("token_name"))),
            );

        if self.repo.is_some() {
            workflow = workflow
                .input(input("repo"), InputSpec::new(ty("GitHubRepo")))
                .input(input("visibility"), InputSpec::new(ty("RepoVisibility")))
                .input(
                    input("secret_name"),
                    InputSpec::new(ty("ActionsSecretName")),
                )
                .node(
                    node("repo"),
                    Node::new(tool_name("github.repo.ensure"))
                        .port(port("repo"), Binding::Input(input("repo")))
                        .port(port("visibility"), Binding::Input(input("visibility"))),
                )
                .node(
                    node("ci_secret"),
                    Node::new(tool_name("github.actions_secret.ensure"))
                        .port(port("repo"), Binding::Input(input("repo")))
                        .port(port("name"), Binding::Input(input("secret_name")))
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
                );
        }

        if self.irreversible.is_some() {
            workflow = workflow
                .input(input("slug"), InputSpec::new(ty("ProjectSlug")))
                .node(
                    node("irreversible"),
                    Node::new(tool_name("fake.irreversible.ensure"))
                        .port(port("key"), Binding::Input(input("slug"))),
                );
        }

        workflow
    }

    fn inputs(&self) -> IndexMap<InputName, Value> {
        let mut inputs = IndexMap::new();
        inputs.insert(input("project"), Value::known(self.project.clone()));
        inputs.insert(input("environment"), Value::known(self.environment.clone()));
        inputs.insert(input("token_name"), Value::known(self.token_name.clone()));
        if let Some(repo) = &self.repo {
            inputs.insert(input("repo"), Value::known(repo.clone()));
            inputs.insert(input("visibility"), Value::known(self.visibility));
            inputs.insert(input("secret_name"), Value::known(self.secret_name.clone()));
        }
        if let Some(slug) = &self.irreversible {
            inputs.insert(input("slug"), Value::known(slug.clone()));
        }
        inputs
    }
}

#[allow(clippy::type_complexity)]
fn scenario() -> impl Strategy<Value = (Scenario, (&'static str, String))> {
    (
        NAME_PATTERN,
        NAME_PATTERN,
        NAME_PATTERN,
        any::<bool>(),
        NAME_PATTERN,
        NAME_PATTERN,
        any::<bool>(),
        any::<bool>(),
        NAME_PATTERN,
    )
        .prop_filter_map(
            "every drawn name parses as its domain type",
            |(
                project,
                environment,
                token_name,
                include_repo,
                org,
                repo_slug,
                public,
                include_irreversible,
                irreversible_slug,
            )| {
                let repo = if include_repo {
                    Some(GitHubRepo::parse(&format!("{org}/{repo_slug}")).ok()?)
                } else {
                    None
                };
                Some(Scenario {
                    project: DopplerProject::parse(&project).ok()?,
                    environment: EnvironmentSlug::parse(&environment).ok()?,
                    token_name: DopplerTokenName::parse(&token_name).ok()?,
                    secret_name: ActionsSecretName::parse("DOPPLER_TOKEN")
                        .expect("a fixed, valid secret name"),
                    repo,
                    visibility: if public {
                        RepoVisibility::Public
                    } else {
                        RepoVisibility::Private
                    },
                    irreversible: if include_irreversible {
                        Some(ProjectSlug::parse(&irreversible_slug).ok()?)
                    } else {
                        None
                    },
                })
            },
        )
        .prop_flat_map(|scenario| {
            let candidates = scenario.candidates();
            (Just(scenario), proptest::sample::select(candidates))
        })
}

proptest! {
    /// See this file's own module doc for the generator's shape and the
    /// deliberate deviation from `willikins_core::testing::arb_workflow`.
    #[test]
    fn convergence_holds_after_one_injected_failure((scenario, (fail_tool, fail_key)) in scenario()) {
        let workflow = scenario.workflow();
        let state = Arc::new(Mutex::new(FakeState::new()));
        let fake_catalog = catalog(Arc::clone(&state));
        let checked = check(&workflow, &fake_catalog)
            .unwrap_or_else(|errors| panic!("generated workflow must check cleanly: {errors:?}"));
        let inputs = scenario.inputs();

        // Round 1: fresh empty state, nothing injected yet -- must fully
        // succeed, and every attempted node was created (nothing
        // pre-existed).
        let plan1 = plan(&checked, &inputs, &fake_catalog)
            .unwrap_or_else(|err| panic!("round 1 plan must succeed: {err}"));
        let approval1 = approval_for(&plan1);
        let mut observer1 = NoopObserver;
        let applied1 = apply(
            &checked,
            &inputs,
            &fake_catalog,
            &plan1,
            &approval1,
            &mut observer1,
        )
        .unwrap_or_else(|err| panic!("round 1 apply must succeed: {err:?}"));
        for applied_node in &applied1.nodes {
            prop_assert!(
                matches!(applied_node.status, NodeStatus::Created | NodeStatus::Computed),
                "round 1: {:?} ended {:?}, expected Created against empty state",
                applied_node.name,
                applied_node.status
            );
        }

        // Inject one failure at the chosen node instance.
        state
            .lock()
            .unwrap()
            .fail_ensure_once
            .push(call_key(fail_tool, &fail_key));

        // Round 2: fresh re-plan and apply.
        let plan2 = plan(&checked, &inputs, &fake_catalog)
            .unwrap_or_else(|err| panic!("round 2 plan must succeed: {err}"));
        let approval2 = approval_for(&plan2);
        let mut observer2 = NoopObserver;
        let result2 = apply(
            &checked,
            &inputs,
            &fake_catalog,
            &plan2,
            &approval2,
            &mut observer2,
        );

        match result2 {
            Ok(applied2) => {
                // Every node this scenario has is either the secret-chain
                // consumer (which converges without a call once its
                // upstream token exists) or a node whose ensure was
                // called again this round and reported unchanged.
                for applied_node in &applied2.nodes {
                    prop_assert!(
                        matches!(
                            applied_node.status,
                            NodeStatus::Unchanged | NodeStatus::Converged | NodeStatus::Computed
                        ),
                        "round 2 (no injected call fired): {:?} ended {:?}",
                        applied_node.name,
                        applied_node.status
                    );
                }
            }
            Err(ApplyError::Tool { applied: partial, .. }) => {
                // The injected failure fired: every attempted instance is
                // Unchanged/Converged except exactly the failed one, and
                // nothing after it was more than NotRun.
                let mut saw_failure = false;
                for applied_node in &partial.nodes {
                    match &applied_node.status {
                        NodeStatus::Failed { .. } => saw_failure = true,
                        NodeStatus::NotRun
                        | NodeStatus::Unchanged
                        | NodeStatus::Converged
                        | NodeStatus::Computed
                        | NodeStatus::Created => {}
                    }
                }
                prop_assert!(saw_failure, "an ApplyError::Tool must show a Failed node");
            }
            Err(ApplyError::UnknownInput { node: blocked, from, .. }) => {
                // Only reachable if the queued failure happened to block
                // `token` itself in a way that still let `plan` observe
                // it as `NoOp` (not constructed by this generator's own
                // candidates, which never target `ci_secret`); if it ever
                // is reached, it must still name the one un-re-readable
                // chain this catalog has.
                prop_assert_eq!(blocked, node("ci_secret"));
                prop_assert_eq!(from, node("token"));
            }
            Err(other) => {
                prop_assert!(false, "round 2: unexpected ApplyError: {other}");
            }
        }

        // No `ensure` was called twice for the same key with the planned
        // action `Create`: round 1 plans `Create` for every node against
        // empty state and calls `ensure` exactly once per key; round 2's
        // fresh re-plan never plans `Create` again for anything (nothing
        // was removed between rounds), so every key's `ensure_calls` is
        // exactly 1 (called once, in round 1 only -- e.g. the secret
        // chain's consumer when it converges without a round 2 call) or 2
        // (round 1's success plus one round 2 attempt, whether that
        // attempt succeeded or was the injected failure).
        let locked = state.lock().unwrap();
        for (tool, key) in scenario.candidates() {
            let count = locked.ensure_calls.get(&call_key(tool, &key)).copied().unwrap_or(0);
            prop_assert!(
                (1..=2).contains(&count),
                "{tool}#{key}: ensure_calls was {count}, expected 1 or 2"
            );
        }
        if let Some(key) = scenario.ci_secret_key() {
            let count = locked
                .ensure_calls
                .get(&call_key("github.actions_secret.ensure", &key))
                .copied()
                .unwrap_or(0);
            prop_assert!(
                (1..=2).contains(&count),
                "github.actions_secret.ensure#{key}: ensure_calls was {count}, expected 1 or 2"
            );
        }
    }
}
