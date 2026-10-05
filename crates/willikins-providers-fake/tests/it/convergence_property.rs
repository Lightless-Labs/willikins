//! Acceptance test 6d (property): over generated workflows built from the
//! fake catalog's own non-pure tools, `check` them, queue one
//! `fail_ensure_once` at an arbitrary node instance, `plan` and `apply`
//! against empty state — which stops at exactly that instance — then
//! `plan` and `apply` again and assert the second round converges: every
//! node whose inputs are all known ends `Created`/`Unchanged`, every node
//! blocked by an un-re-readable upstream ends in `UnknownInput` naming
//! that upstream, a finished node is `NoOp` in the fresh plan and an
//! unfinished one `Create`, and every key's `ensure_calls` count equals
//! exactly the number of rounds its instance was attempted in — so a
//! `Converged` instance is proven to make no call, a `NotRun` one none
//! either, and nothing already created is created again.
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
//! The one injected failure is always queued *before* the first round,
//! which is what makes the second round a convergence test rather than a
//! second failure: round one stops at the injected instance, and round
//! two re-plans against the state round one did reach and either settles
//! everything or stops at the single documented gap — a token minted in
//! round one but not yet consumed, whose value can never be re-read
//! (`ApplyError::UnknownInput` naming `token`). Which of the two happens
//! is a property of where the failure landed, and is asserted as one: the
//! gap appears exactly when round one created `token` and did not store
//! `ci_secret`. A counter asserts the gap branch was reached at least
//! once across the whole run, rather than trusting that the generator can
//! still produce it.
//!
//! (An earlier shape queued the failure *between* the two rounds. That
//! made round two the failing round, so the property never observed a run
//! converging at all, and its call accounting asserted a lower bound of
//! one call for every node — which is wrong for any node the stopped run
//! never reached, and is how it failed on a scenario whose
//! `fake.irreversible.ensure` node sits after the blocked one.)
//!
//! Driven through a `TestRunner` directly rather than the `proptest!`
//! macro, for exactly that reason: the macro's body sees one case at a
//! time and can assert nothing about the run as a whole. `source_file` is
//! set the way the macro sets it, so failure persistence
//! (`.proptest-regressions`) still works.
//!
//! Run at the default case count; also run by hand at
//! `PROPTEST_CASES=1024` (proptest reads that environment variable
//! itself), which took 1.2 seconds of test time on this host on
//! 2026-09-13 — the fakes are in-memory, so the build dominates.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestCaseError, TestRunner};

use willikins_core::{
    Action, AppliedNode, ApplyError, Approval, Binding, InputName, InputSpec, Node, NodeName,
    NodeStatus, NoopObserver, OutputName, PortName, ToolName, TypeName, TypeRef, Value, Workflow,
    apply, check, plan,
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

    /// `(node, tool, key)` for every node this scenario's workflow has:
    /// the universe the single injected failure is drawn from, and the
    /// table the call accounting at the end of a case reads.
    fn instances(&self) -> Vec<(NodeName, &'static str, String)> {
        let mut out = vec![
            (
                node("project"),
                "doppler.project.ensure",
                doppler_project_key(&self.project),
            ),
            (
                node("config"),
                "doppler.config.ensure",
                doppler_config_key(&self.config()),
            ),
            (
                node("token"),
                "doppler.service_token.ensure",
                doppler_service_token_key(&self.config(), &self.token_name),
            ),
        ];
        if let Some(repo) = &self.repo {
            out.push((node("repo"), "github.repo.ensure", repo_key(repo)));
            out.push((
                node("ci_secret"),
                "github.actions_secret.ensure",
                actions_secret_key(repo, &self.secret_name),
            ));
        }
        if let Some(slug) = &self.irreversible {
            out.push((
                node("irreversible"),
                "fake.irreversible.ensure",
                irreversible_key(slug),
            ));
        }
        out
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

/// One generated injection: the node whose `ensure` the single
/// `fail_ensure_once` entry stops on its first round-one call, and the
/// `(tool, key)` pair `FakeState` keys that entry by.
#[derive(Debug, Clone)]
struct Injection {
    node: NodeName,
    tool: &'static str,
    key: String,
}

/// How many cases took the `ApplyError::UnknownInput` branch, across the
/// whole run: the property asserts this is non-zero afterwards, so the
/// branch cannot quietly become unreachable — it was, under the earlier
/// between-the-rounds injection shape this file's module doc describes.
static UNKNOWN_INPUT_HITS: AtomicUsize = AtomicUsize::new(0);

fn scenario() -> impl Strategy<Value = (Scenario, Injection)> {
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
            "every drawn name parses as its domain type, and the environment is not a seeded one",
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
                let environment = EnvironmentSlug::parse(&environment).ok()?;
                // `doppler.project.ensure` seeds the `dev`, `stg` and
                // `prd` root configs with a project it creates, so a
                // generated environment spelling one of them would make
                // round one's `config` node `Unchanged` rather than
                // `Created` — a real fake behaviour (acceptance test 5
                // pins it) but not this property's subject.
                if ["dev", "stg", "prd"].contains(&environment.words().snake().as_str()) {
                    return None;
                }
                Some(Scenario {
                    project: DopplerProject::parse(&project).ok()?,
                    environment,
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
            let injections: Vec<Injection> = scenario
                .instances()
                .into_iter()
                .map(|(node, tool, key)| Injection { node, tool, key })
                .collect();
            (Just(scenario), proptest::sample::select(injections))
        })
}

/// The number of `ensure` calls one round's result implies for `name`:
/// an instance that ended `Created`, `Unchanged` or `Failed` was called,
/// while `Computed` (a pure tool), `Converged` (a planned `NoOp` whose
/// inputs are not all known) and `NotRun` are each "no call". Comparing
/// the sum over both rounds against `FakeState::ensure_calls` is what
/// pins those three to a call count of exactly zero.
fn calls_implied(nodes: &[AppliedNode], name: &NodeName) -> u32 {
    let called = nodes.iter().filter(|applied| {
        applied.name == *name
            && matches!(
                applied.status,
                NodeStatus::Created | NodeStatus::Unchanged | NodeStatus::Failed { .. }
            )
    });
    u32::try_from(called.count()).expect("a generated workflow has at most six nodes")
}

/// Whether `nodes` shows `name` finished — created or already right — in
/// the round that produced it.
fn finished(nodes: &[AppliedNode], name: &NodeName) -> bool {
    nodes.iter().any(|applied| {
        applied.name == *name
            && matches!(
                applied.status,
                NodeStatus::Created | NodeStatus::Unchanged | NodeStatus::Converged
            )
    })
}

/// The action `plan` gave `name`'s single instance (every node this
/// generator builds is a scalar node).
fn action_of(planned: &willikins_core::Plan, name: &NodeName) -> Option<Action> {
    planned
        .nodes
        .iter()
        .find(|instance| instance.name == *name)
        .map(|instance| instance.action)
}

/// One case of the property: see this file's own module doc for the
/// generator's shape and the deliberate deviation from
/// `willikins_core::testing::arb_workflow`.
#[allow(clippy::too_many_lines)] // two rounds walked end to end; splitting scatters the sequence
fn convergence_case(scenario: &Scenario, injection: &Injection) -> Result<(), TestCaseError> {
    let workflow = scenario.workflow();
    let state = Arc::new(Mutex::new(
        FakeState::new().with_fail_ensure_once(injection.tool, &injection.key),
    ));
    let fake_catalog = catalog(Arc::clone(&state));
    let checked = check(&workflow, &fake_catalog)
        .unwrap_or_else(|errors| panic!("generated workflow must check cleanly: {errors:?}"));
    let inputs = scenario.inputs();

    // Round 1, against empty state with the failure already queued: every
    // node is a fresh `Create`, and the run stops at exactly the injected
    // instance.
    let plan1 = plan(&checked, &inputs, &fake_catalog)
        .unwrap_or_else(|err| panic!("round 1 plan must succeed: {err}"));
    for instance in &plan1.nodes {
        prop_assert_eq!(
            instance.action,
            Action::Create,
            "round 1 plans Create for every node against empty state, not for {}",
            instance.name
        );
    }
    let approval1 = approval_for(&plan1);
    let mut observer1 = NoopObserver;
    let nodes1 = match apply(
        &checked,
        &inputs,
        &fake_catalog,
        &plan1,
        &approval1,
        &mut observer1,
    ) {
        Err(ApplyError::Tool {
            node: failed,
            applied,
            ..
        }) => {
            prop_assert_eq!(
                &failed,
                &injection.node,
                "round 1 must stop at the injected node"
            );
            applied.nodes
        }
        Ok(applied) => {
            return Err(TestCaseError::fail(format!(
                "round 1 must stop at {}, but it finished: {:?}",
                injection.node, applied.nodes
            )));
        }
        Err(other) => {
            return Err(TestCaseError::fail(format!(
                "round 1: expected the injected failure, got {other}"
            )));
        }
    };
    for applied_node in &nodes1 {
        prop_assert!(
            matches!(
                applied_node.status,
                NodeStatus::Created | NodeStatus::Failed { .. } | NodeStatus::NotRun
            ),
            "round 1: {} ended {:?}, expected Created against empty state",
            applied_node.name,
            applied_node.status
        );
    }

    // The one documented convergence gap: round one minted the token and
    // did not get as far as storing it, so its value is gone for good and
    // round two cannot supply `ci_secret`'s `value`.
    let expects_gap = scenario.repo.is_some()
        && finished(&nodes1, &node("token"))
        && !finished(&nodes1, &node("ci_secret"));

    // Round 2: a fresh plan shows every finished node `NoOp` and the rest
    // `Create`, and the apply either settles everything or stops at the
    // gap.
    let plan2 = plan(&checked, &inputs, &fake_catalog)
        .unwrap_or_else(|err| panic!("round 2 plan must succeed: {err}"));
    for (name, _, _) in scenario.instances() {
        let expected = if finished(&nodes1, &name) {
            Action::NoOp
        } else {
            Action::Create
        };
        prop_assert_eq!(
            action_of(&plan2, &name),
            Some(expected),
            "round 2's fresh plan for {}",
            name
        );
    }
    let approval2 = approval_for(&plan2);
    let mut observer2 = NoopObserver;
    let nodes2 = match apply(
        &checked,
        &inputs,
        &fake_catalog,
        &plan2,
        &approval2,
        &mut observer2,
    ) {
        Ok(applied) => {
            prop_assert!(
                !expects_gap,
                "a token minted but not stored in round 1 must block its consumer, not settle"
            );
            for applied_node in &applied.nodes {
                prop_assert!(
                    matches!(
                        applied_node.status,
                        NodeStatus::Created | NodeStatus::Unchanged | NodeStatus::Converged
                    ),
                    "round 2: {} ended {:?}, expected a settled status",
                    applied_node.name,
                    applied_node.status
                );
            }
            prop_assert!(
                finished(&applied.nodes, &injection.node),
                "round 2 must finish the node round 1 failed at: {:?}",
                applied.nodes
            );
            applied.nodes
        }
        Err(ApplyError::UnknownInput {
            node: blocked,
            port: blocked_port,
            from,
            applied,
        }) => {
            UNKNOWN_INPUT_HITS.fetch_add(1, Ordering::Relaxed);
            prop_assert!(
                expects_gap,
                "only a token minted in round 1 and never stored can block a second round"
            );
            prop_assert_eq!(blocked, node("ci_secret"));
            prop_assert_eq!(blocked_port, port("value"));
            prop_assert_eq!(from, node("token"));
            applied.nodes
        }
        Err(other) => {
            return Err(TestCaseError::fail(format!(
                "round 2: the one-shot injection fires in round 1 only, got {other}"
            )));
        }
    };

    // Call accounting: every key's `ensure_calls` is exactly the number of
    // rounds its instance was attempted in — no `Converged` or `NotRun`
    // instance was called, no created resource was created twice, and the
    // failed one was retried exactly once.
    let locked = state.lock().unwrap();
    for (name, tool, key) in scenario.instances() {
        let calls = locked
            .ensure_calls
            .get(&call_key(tool, &key))
            .copied()
            .unwrap_or(0);
        prop_assert_eq!(
            calls,
            calls_implied(&nodes1, &name) + calls_implied(&nodes2, &name),
            "{}#{}: ensure_calls",
            tool,
            key
        );
    }
    Ok(())
}

/// Acceptance test 6d. Driven through a [`TestRunner`] rather than
/// `proptest!` so the run as a whole can be asserted on: the
/// `UnknownInput` branch must be reached at least once, not merely
/// tolerated if it happens.
#[test]
fn convergence_holds_after_one_injected_failure() {
    let config = ProptestConfig {
        source_file: Some(file!()),
        ..ProptestConfig::default()
    };
    let mut runner = TestRunner::new(config);
    runner
        .run(&scenario(), |(scenario, injection)| {
            convergence_case(&scenario, &injection)
        })
        .expect("convergence must hold for every generated scenario");
    assert!(
        UNKNOWN_INPUT_HITS.load(Ordering::Relaxed) > 0,
        "no generated case reached ApplyError::UnknownInput: the generator can no longer \
         produce a workflow whose blocked node is downstream of an un-re-readable output, \
         so the branch above is untested"
    );
}
