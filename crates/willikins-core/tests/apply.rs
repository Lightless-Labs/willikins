//! Integration tests for [`willikins_core::apply`]: task 4a's acceptance
//! tests 5, 6 (the parts a test-only tool can produce without task 4b's
//! fake-provider changes), 7 (core half), and 8 (the fingerprint unit
//! half lives in `crates/willikins-core/src/plan.rs`'s own test module).
//!
//! See `common::FixedTokenService`'s own doc for why these tests build
//! their own catalog (via `common::apply_test_catalog`) rather than
//! `willikins_providers_fake::catalog` unchanged: today's
//! `doppler.service_token.ensure` reports its `token` output `Unknown` on
//! every `ensure` call, including a freshly minting one, which would make
//! `ci_secret` hit `ApplyError::UnknownInput` on the very first run —
//! task 4b's job is to fix that in the shared fake crate itself.

mod common;

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;

use common::{
    RequiresKnownInputTool, ScriptedEnsureTool, UnreadableUpstreamTool, apply_test_catalog,
    distinctive_token, input, node, port, principal, timestamp, tool_name, ty, workflow_name,
};
use willikins_core::{
    Action, ApplyError, ApplyEvent, Approval, Binding, Catalog, Class, DriftKind, InputSpec, Node,
    NodeStatus, RecordingObserver, ToolErrorKind, Value, Workflow, apply, check, plan,
};
use willikins_providers_fake::state::FakeState;
use willikins_types::{DomainType, GitHubOrg, ProjectSlug, RepoVisibility, naming};

/// Every `NodeStarted`/`NodeFinished` pair in `events`, as
/// `(node, instance)` — asserts they always come in that order, adjacent,
/// one pair per attempted instance.
fn started_finished_pairs(events: &[ApplyEvent]) -> Vec<(String, Option<String>)> {
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let ApplyEvent::NodeStarted { node, instance, .. } = &events[i] else {
            panic!("event {i} is not NodeStarted: {:?}", events[i]);
        };
        let ApplyEvent::NodeFinished {
            node: finished_node,
            instance: finished_instance,
            ..
        } = &events[i + 1]
        else {
            panic!("event {} is not NodeFinished: {:?}", i + 1, events[i + 1]);
        };
        assert_eq!(node, finished_node, "Started/Finished node mismatch at {i}");
        assert_eq!(
            instance, finished_instance,
            "Started/Finished instance mismatch at {i}"
        );
        pairs.push((node.to_string(), instance.clone()));
        i += 2;
    }
    pairs
}

#[test]
fn happy_path_creates_everything_and_the_marker_never_leaks() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = apply_test_catalog(Arc::clone(&state));
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("the positive fixture checks cleanly");
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("empty state plans cleanly");
    assert_eq!(approved.class, Class::Reversible);
    assert!(!approved.requires_approval);

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("the happy path must apply cleanly");

    let by_name = |name: &str, instance: Option<&str>| {
        applied
            .nodes
            .iter()
            .find(|n| n.name.as_str() == name && n.instance.as_deref() == instance)
            .unwrap_or_else(|| panic!("no applied node `{name}` (instance {instance:?})"))
    };

    assert!(matches!(
        by_name("names", None).status,
        NodeStatus::Computed
    ));
    for name in ["repo", "doppler", "token", "ci_secret"] {
        assert!(
            matches!(by_name(name, None).status, NodeStatus::Created),
            "expected `{name}` Created, got {:?}",
            by_name(name, None).status
        );
    }
    for key in ["dev", "stg", "prd"] {
        assert!(
            matches!(by_name("configs", Some(key)).status, NodeStatus::Unchanged),
            "expected configs[{key}] Unchanged (seeded by doppler.project.ensure), got {:?}",
            by_name("configs", Some(key)).status
        );
    }

    let repo_url = applied
        .outputs
        .get(&willikins_core::OutputName::parse("repo_url").unwrap())
        .expect("repo_url output resolved");
    assert!(repo_url.is_known());

    // Every attempted instance saw NodeStarted then NodeFinished, in plan
    // order.
    let pairs = started_finished_pairs(&observer.events);
    let expected_order: Vec<(String, Option<String>)> = applied
        .nodes
        .iter()
        .map(|n| (n.name.to_string(), n.instance.clone()))
        .collect();
    assert_eq!(pairs, expected_order);

    // The distinctive marker never appears anywhere text-rendered, in
    // `Applied`'s JSON or its `Debug`, or in any journal-shaped event.
    let marker = distinctive_token().to_string();
    // `distinctive_token()` itself redacts through `Display`/`Debug` (it's
    // a secret domain type), so compare against the raw literal instead.
    let raw_marker = "MARKER".repeat(7);
    let json = serde_json::to_string(&applied).unwrap();
    let debug = format!("{applied:?}");
    assert!(
        !json.contains(&raw_marker),
        "json leaked the marker: {json}"
    );
    assert!(
        !debug.contains(&raw_marker),
        "debug leaked the marker: {debug}"
    );
    assert!(json.contains("REDACTED"), "json: {json}");
    assert!(debug.contains("REDACTED"), "debug: {debug}");
    // `marker` (the type's own rendering) is exactly the redaction marker,
    // never the raw bytes — sanity check that the assertions above are not
    // vacuous.
    assert!(marker.contains("REDACTED"));

    for event in &observer.events {
        let debug = format!("{event:?}");
        assert!(
            !debug.contains(&raw_marker),
            "event leaked the marker: {debug}"
        );
    }
}

#[test]
fn drift_on_action_stops_before_any_provider_call() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = apply_test_catalog(Arc::clone(&state));
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("the positive fixture checks cleanly");
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("empty state plans cleanly");

    // Seed the repo as ours, matching the requested (default) visibility,
    // between `plan` and `apply`: a fresh re-plan now sees it `NoOp`
    // where the approved plan said `Create`.
    let org = GitHubOrg::parse("lightless-labs").unwrap();
    let slug = ProjectSlug::parse("third-thoughts").unwrap();
    let repo = naming::v1::github_repo(&org, &slug);
    {
        let mut locked = state.lock().unwrap();
        *locked = locked
            .clone()
            .with_repo(&repo, RepoVisibility::Private, true);
    }

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("a drifted plan must refuse");

    match err {
        ApplyError::Drift {
            node,
            instance,
            kind,
        } => {
            assert_eq!(node.as_str(), "repo");
            assert_eq!(instance, None);
            let DriftKind::Action { planned, observed } = *kind else {
                panic!("expected Drift on Action for `repo`, got {kind:?}");
            };
            assert_eq!(planned, Action::Create);
            assert_eq!(observed, Action::NoOp);
        }
        other => panic!("expected Drift on Action for `repo`, got {other:?}"),
    }

    // Nothing beyond the manual seed happened: no provider call was made
    // (the drift check runs before the run's `SinkToken` is even minted).
    assert!(observer.events.is_empty(), "no events before any call");
    let locked = state.lock().unwrap();
    assert!(locked.doppler_projects.is_empty(), "doppler never called");
    assert_eq!(
        locked.github_repos.len(),
        1,
        "only the manually seeded repo"
    );
    // Drift is found by *reading* (the re-plan reads every node) and
    // refused before the first write: `read_calls` moved, `ensure_calls`
    // did not.
    assert!(
        !locked.read_calls.is_empty(),
        "the re-plan must have read the current state"
    );
    assert!(
        locked.ensure_calls.is_empty(),
        "drift must be refused before any ensure: {:?}",
        locked.ensure_calls
    );
}

/// `Approval::Human` is accepted for a plan that does not require
/// approval at all: the gate is "approval is at least as strong as the
/// class demands", not "the approval kind must match the class".
#[test]
fn human_approval_is_accepted_for_a_reversible_plan() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = apply_test_catalog(Arc::clone(&state));
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("the positive fixture checks cleanly");
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("empty state plans cleanly");
    assert!(
        !approved.requires_approval,
        "the positive fixture is Reversible"
    );

    let approval = Approval::Human {
        approver: principal("operator"),
        at: timestamp("2026-09-13T09:00:00Z"),
    };
    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &approval,
        &mut observer,
    )
    .expect("a human-approved Reversible plan runs");
    assert!(matches!(
        common::status_of(&applied, "repo", None),
        NodeStatus::Created
    ));
}

#[test]
fn a_tool_failure_reports_failed_then_not_run() {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(ScriptedEnsureTool::new("test.boom", true)))
        .unwrap();
    catalog
        .insert(Arc::new(ScriptedEnsureTool::new("test.after", false)))
        .unwrap();

    let workflow = Workflow::new(workflow_name("failure-test"))
        .node(node("boom"), Node::new(tool_name("test.boom")))
        .node(node("after"), Node::new(tool_name("test.after")));
    let checked = check(&workflow, &catalog).expect("a workflow with no bindings checks cleanly");
    let inputs = IndexMap::new();
    let approved = plan(&checked, &inputs, &catalog).expect("plan against no state succeeds");

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("a failing ensure must stop the run");

    let ApplyError::Tool {
        node: failed_node,
        instance,
        error,
        applied,
    } = err
    else {
        panic!("expected ApplyError::Tool");
    };
    assert_eq!(failed_node.as_str(), "boom");
    assert_eq!(instance, None);
    assert_eq!(error.kind, ToolErrorKind::Provider);
    assert_eq!(applied.nodes.len(), 2);
    assert_eq!(applied.nodes[0].name.as_str(), "boom");
    assert!(matches!(applied.nodes[0].status, NodeStatus::Failed { .. }));
    assert_eq!(applied.nodes[1].name.as_str(), "after");
    assert!(matches!(applied.nodes[1].status, NodeStatus::NotRun));

    // `after` never started: only `boom`'s Started/Finished pair exists.
    assert_eq!(observer.events.len(), 2);
}

#[test]
fn unknown_input_blocks_a_create_node_reading_an_unreadable_upstream_output() {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(UnreadableUpstreamTool::new("test.upstream")))
        .unwrap();
    catalog
        .insert(Arc::new(RequiresKnownInputTool::new("test.consumer")))
        .unwrap();

    let workflow = Workflow::new(workflow_name("unknown-input-test"))
        .node(node("upstream"), Node::new(tool_name("test.upstream")))
        .node(
            node("consumer"),
            Node::new(tool_name("test.consumer")).port(
                port("value"),
                Binding::Step {
                    node: node("upstream"),
                    port: port("value"),
                },
            ),
        );
    let checked = check(&workflow, &catalog).expect("must check cleanly");
    let inputs = IndexMap::new();
    let approved = plan(&checked, &inputs, &catalog).expect("plan succeeds");
    assert_eq!(
        approved
            .nodes
            .iter()
            .find(|n| n.name.as_str() == "consumer")
            .unwrap()
            .action,
        Action::Create
    );

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("an unreadable required input must block");

    let ApplyError::UnknownInput {
        node: blocked_node,
        port: blocked_port,
        from,
        applied,
    } = err
    else {
        panic!("expected ApplyError::UnknownInput");
    };
    assert_eq!(blocked_node.as_str(), "consumer");
    assert_eq!(blocked_port.as_str(), "value");
    assert_eq!(from.as_str(), "upstream");

    // `upstream` itself has no required inputs, so it is never blocked: it
    // ran (Unchanged, since it always reports Present) and appears in the
    // partial result; `consumer` is absent, and there is nothing after it.
    assert_eq!(applied.nodes.len(), 1);
    assert_eq!(applied.nodes[0].name.as_str(), "upstream");
    assert!(matches!(applied.nodes[0].status, NodeStatus::Unchanged));

    assert_eq!(observer.events.len(), 2, "only upstream's Started/Finished");
}

#[test]
fn approval_gate_blocks_auto_and_runs_with_human() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = apply_test_catalog(Arc::clone(&state));
    let workflow = common::irreversible_workflow();
    let checked = check(&workflow, &catalog).expect("irreversible workflow checks cleanly");
    let inputs = common::new_rust_service_inputs();
    let approved = plan(&checked, &inputs, &catalog).expect("plan against empty state succeeds");
    assert_eq!(approved.class, Class::Irreversible);
    assert!(approved.requires_approval);

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("Auto on an irreversible plan must refuse");
    match err {
        ApplyError::ApprovalRequired { class } => assert_eq!(class, Class::Irreversible),
        other => panic!("expected ApprovalRequired, got {other:?}"),
    }
    assert!(observer.events.is_empty(), "nothing ran before the refusal");
    {
        let locked = state.lock().unwrap();
        assert!(locked.github_repos.is_empty(), "no provider call was made");
        assert!(
            locked.doppler_projects.is_empty(),
            "no provider call was made"
        );
    }

    let approval = Approval::Human {
        approver: principal("alice"),
        at: timestamp("2026-09-13T12:00:00+00:00"),
    };
    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &approval,
        &mut observer,
    )
    .expect("Human approval must let the plan run");

    let danger = applied
        .nodes
        .iter()
        .find(|n| n.name.as_str() == "danger")
        .expect("danger node applied");
    assert!(matches!(danger.status, NodeStatus::Created));
    let ci_secret = applied
        .nodes
        .iter()
        .find(|n| n.name.as_str() == "ci_secret")
        .expect("ci_secret node applied");
    assert!(matches!(ci_secret.status, NodeStatus::Created));
}

#[test]
fn a_re_plan_failure_surfaces_as_apply_error_plan() {
    // `checked` was built with `org` bound; re-planning inside `apply`
    // against `inputs` that no longer bind it fails with `MissingInput`,
    // and `apply` wraps that as `ApplyError::Plan` rather than a panic —
    // the workflow declares one node that actually reads `org`, so the
    // re-plan genuinely fails rather than trivially succeeding.
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(willikins_tools::NamingV1::new()))
        .unwrap();
    let workflow = Workflow::new(workflow_name("missing-input-test"))
        .input(input("org"), InputSpec::new(ty("GitHubOrg")))
        .input(input("slug"), InputSpec::new(ty("ProjectSlug")))
        .node(
            node("names"),
            Node::new(tool_name("naming.v1"))
                .port(port("org"), Binding::Input(input("org")))
                .port(port("slug"), Binding::Input(input("slug"))),
        );
    let checked = check(&workflow, &catalog).expect("must check cleanly");
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("org"),
        Value::known(GitHubOrg::parse("lightless-labs").unwrap()),
    );
    inputs.insert(
        input("slug"),
        Value::known(ProjectSlug::parse("third-thoughts").unwrap()),
    );
    let approved = plan(&checked, &inputs, &catalog).expect("plan succeeds with both inputs bound");

    // `apply`'s own `inputs` withholds `slug`.
    let mut incomplete_inputs = IndexMap::new();
    incomplete_inputs.insert(
        input("org"),
        Value::known(GitHubOrg::parse("lightless-labs").unwrap()),
    );
    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &incomplete_inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("re-planning without `slug` must fail");

    let ApplyError::Plan { error } = err else {
        panic!("expected ApplyError::Plan, got a different variant");
    };
    assert!(matches!(
        error,
        willikins_core::PlanError::MissingInput { .. }
    ));
    let json = serde_json::to_value(&ApplyError::Plan { error }).unwrap();
    assert_eq!(json["kind"], "Plan");
    assert_eq!(json["error"]["kind"], "MissingInput");
}

/// Acceptance test 6c's `Converged` half: applying the positive fixture a
/// second time, against the state the first apply left behind, must
/// leave every reversible node `Unchanged` and `ci_secret` `Converged` --
/// its own `value` input (`token`'s `token` output) is `Unknown` in *this*
/// run (the token already exists, so `FixedTokenService`'s `ensure`, like
/// the real fake tool, cannot report its value back), and `ci_secret`'s
/// own fresh `read` reports it `Present` (so its planned action is
/// `NoOp`): exactly the "required input Unknown, planned `NoOp`" branch
/// (`NodeStatus::Converged`), the one status none of this file's other
/// tests exercise.
#[test]
fn a_second_apply_against_the_first_ones_state_converges_the_unreadable_secret_consumer() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = apply_test_catalog(Arc::clone(&state));
    let workflow = common::new_rust_service_workflow();
    let checked = check(&workflow, &catalog).expect("checks cleanly");
    let inputs = common::new_rust_service_inputs();

    let first_plan = plan(&checked, &inputs, &catalog).expect("first plan succeeds");
    apply(
        &checked,
        &inputs,
        &catalog,
        &first_plan,
        &Approval::Auto,
        &mut RecordingObserver::new(),
    )
    .expect("first apply creates everything");

    let second_plan = plan(&checked, &inputs, &catalog).expect("re-plan against populated state");
    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &second_plan,
        &Approval::Auto,
        &mut observer,
    )
    .expect("second apply converges");

    let by_name = |name: &str, instance: Option<&str>| {
        applied
            .nodes
            .iter()
            .find(|n| n.name.as_str() == name && n.instance.as_deref() == instance)
            .unwrap_or_else(|| panic!("no applied node `{name}` (instance {instance:?})"))
    };
    assert!(matches!(
        by_name("repo", None).status,
        NodeStatus::Unchanged
    ));
    assert!(matches!(
        by_name("doppler", None).status,
        NodeStatus::Unchanged
    ));
    for key in ["dev", "stg", "prd"] {
        assert!(
            matches!(by_name("configs", Some(key)).status, NodeStatus::Unchanged),
            "configs[{key}]: {:?}",
            by_name("configs", Some(key)).status
        );
    }
    assert!(
        matches!(by_name("token", None).status, NodeStatus::Unchanged),
        "token: {:?}",
        by_name("token", None).status
    );
    assert!(
        matches!(by_name("ci_secret", None).status, NodeStatus::Converged),
        "ci_secret: {:?}",
        by_name("ci_secret", None).status
    );

    // `ci_secret`'s Converged instance still got a Started/Finished pair
    // (rule 6: every attempted instance, not just the ones that call a
    // tool).
    let saw_ci_secret_converged_pair = observer.events.windows(2).any(|pair| {
        matches!(&pair[0], ApplyEvent::NodeStarted { node, .. } if node.as_str() == "ci_secret")
            && matches!(
                &pair[1],
                ApplyEvent::NodeFinished { node, status, .. }
                    if node.as_str() == "ci_secret" && matches!(status, NodeStatus::Converged)
            )
    });
    assert!(
        saw_ci_secret_converged_pair,
        "expected a Started/Finished pair for ci_secret's Converged instance: {:?}",
        observer.events
    );

    // No second GitHub Actions secret was written: the fake state's
    // membership set is still exactly the one entry the first apply
    // created (task 4b's `ensure_calls` counter would pin this more
    // directly; this is the signal available without it).
    let locked = state.lock().unwrap();
    assert_eq!(locked.github_actions_secrets.len(), 1);
}

// ---------------------------------------------------------------------
// Drift: identity, not only position
//
// `check_drift` walks the approved and fresh fingerprints together. These
// three tests pin that it compares *which instance* sits at each position,
// not only that position's action and rendered outputs: an approved plan
// that does not describe the same instances as the fresh one must be
// refused as `Drift`, never silently accepted and never a panic.
// ---------------------------------------------------------------------

/// An approved plan built from a *different* workflow, whose node happens
/// to share a name and a planned action but declares a different output
/// port, must be refused rather than panicking inside the output
/// comparison (which used to look the approved side's port up in the fresh
/// side's outputs and `unreachable!` when it was missing).
#[test]
fn an_approved_plan_from_another_workflow_is_drift_not_a_panic() {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(common::FixedOutputTool::new(
            "test.alpha",
            "alpha",
            "lightless-labs",
        )))
        .unwrap();
    catalog
        .insert(Arc::new(common::FixedOutputTool::new(
            "test.beta",
            "beta",
            "other-org",
        )))
        .unwrap();

    let workflow_a = Workflow::new(workflow_name("alpha-workflow"))
        .node(node("solo"), Node::new(tool_name("test.alpha")));
    let workflow_b = Workflow::new(workflow_name("beta-workflow"))
        .node(node("solo"), Node::new(tool_name("test.beta")));
    let checked_a = check(&workflow_a, &catalog).expect("workflow A checks");
    let checked_b = check(&workflow_b, &catalog).expect("workflow B checks");
    let inputs = IndexMap::new();
    let approved = plan(&checked_a, &inputs, &catalog).expect("workflow A plans");

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked_b,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("an approved plan describing another workflow must be refused");

    let ApplyError::Drift {
        node: name, kind, ..
    } = err
    else {
        panic!("expected Drift, got {err:?}");
    };
    assert_eq!(name.as_str(), "solo");
    assert!(
        matches!(*kind, DriftKind::Instance { .. }),
        "expected Instance drift for two plans that do not share an output port, got {kind:?}"
    );
    assert!(observer.events.is_empty(), "nothing ran");
}

/// Two plans whose `for_each` instances carry the same keys in a different
/// *order*, with every instance's action and rendered outputs identical,
/// still differ: the approved plan said "dev first, then stg". Comparing
/// by position alone would accept it.
#[test]
fn for_each_instances_are_compared_by_key_not_only_by_position() {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(common::ConstantForEachTool::new("test.each")))
        .unwrap();

    let workflow = Workflow::new(workflow_name("each-workflow"))
        .input(
            input("environments"),
            InputSpec::new(common::list_ty("EnvironmentSlug")),
        )
        .node(
            node("each"),
            Node::new(tool_name("test.each"))
                .for_each(Binding::Input(input("environments")))
                .port(port("key"), Binding::Item),
        );
    let checked = check(&workflow, &catalog).expect("the for_each workflow checks");

    let environments = |first: &str, second: &str| {
        let mut inputs = IndexMap::new();
        inputs.insert(
            input("environments"),
            Value::known_list(vec![
                willikins_types::EnvironmentSlug::parse(first).unwrap(),
                willikins_types::EnvironmentSlug::parse(second).unwrap(),
            ]),
        );
        inputs
    };

    let approved = plan(&checked, &environments("dev", "stg"), &catalog).expect("plans");
    let swapped = environments("stg", "dev");

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &swapped,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("a plan whose instances were re-ordered must be refused");

    let ApplyError::Drift {
        node: name, kind, ..
    } = err
    else {
        panic!("expected Drift, got {err:?}");
    };
    assert_eq!(name.as_str(), "each");
    let DriftKind::Instance { planned, observed } = *kind else {
        panic!("expected Instance drift, got {kind:?}");
    };
    assert_eq!(
        planned
            .expect("the approved side has this instance")
            .instance,
        Some("dev".to_string())
    );
    assert_eq!(
        observed.expect("the fresh side has this instance").instance,
        Some("stg".to_string())
    );
    assert!(observer.events.is_empty(), "nothing ran");
}

/// An approved plan with *more* instances than the fresh one is drift too,
/// reported as the instance the fresh plan no longer has — not as an
/// action that "no longer matches" itself.
#[test]
fn an_approved_plan_with_an_extra_instance_is_instance_drift() {
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(common::ConstantForEachTool::new("test.each")))
        .unwrap();

    let workflow = Workflow::new(workflow_name("each-workflow"))
        .input(
            input("environments"),
            InputSpec::new(common::list_ty("EnvironmentSlug")),
        )
        .node(
            node("each"),
            Node::new(tool_name("test.each"))
                .for_each(Binding::Input(input("environments")))
                .port(port("key"), Binding::Item),
        );
    let checked = check(&workflow, &catalog).expect("the for_each workflow checks");

    let slugs = |names: &[&str]| {
        let mut inputs = IndexMap::new();
        inputs.insert(
            input("environments"),
            Value::known_list(
                names
                    .iter()
                    .map(|name| willikins_types::EnvironmentSlug::parse(name).unwrap())
                    .collect::<Vec<_>>(),
            ),
        );
        inputs
    };

    let approved = plan(&checked, &slugs(&["dev", "stg"]), &catalog).expect("plans");

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &slugs(&["dev"]),
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("a plan with an instance the fresh plan lacks must be refused");

    let ApplyError::Drift { kind, .. } = &err else {
        panic!("expected Drift, got {err:?}");
    };
    let DriftKind::Instance { planned, observed } = kind.as_ref() else {
        panic!("expected Instance drift, got {kind:?}");
    };
    assert_eq!(
        planned
            .as_ref()
            .expect("approved has the extra instance")
            .instance,
        Some("stg".to_string())
    );
    assert!(observed.is_none(), "the fresh plan has no such instance");
    assert!(
        !err.to_string().contains("no longer matches"),
        "an identity mismatch must not be described as an action mismatch: {err}"
    );
    assert!(observer.events.is_empty(), "nothing ran");
}

// ---------------------------------------------------------------------
// Secrets: a for_each node that mints one, and a workflow output that
// carries the whole list of them
// ---------------------------------------------------------------------

/// A `for_each` node whose tool mints a secret, with a workflow output
/// bound to that node's secret port — so the output is a *list* of
/// secrets, the widest shape a secret can reach `Applied` in. Nothing in
/// `Applied` (JSON, `Debug`), in any observer event (JSON, `Debug`), or in
/// the resolved workflow outputs may carry the minted bytes; every one of
/// them must show the redaction marker instead.
///
/// `check` does not refuse a secret-valued workflow output (a workflow
/// output has no port spec to be a non-secret sink), so redaction by
/// construction is the only thing standing between a minted token and the
/// caller here. This test is that claim, stated as a test.
#[test]
#[allow(clippy::too_many_lines)] // one workflow built, run, and swept for the marker end to end
fn a_for_each_node_minting_secrets_redacts_them_everywhere_including_the_output_list() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = apply_test_catalog(Arc::clone(&state));

    let workflow = Workflow::new(workflow_name("for-each-secrets"))
        .input(input("project"), InputSpec::new(ty("DopplerProject")))
        .input(
            input("environments"),
            InputSpec::new(common::list_ty("EnvironmentSlug")),
        )
        .node(
            node("doppler"),
            Node::new(tool_name("doppler.project.ensure"))
                .port(port("project"), Binding::Input(input("project"))),
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
            node("tokens"),
            Node::new(tool_name("doppler.service_token.ensure"))
                .for_each(Binding::Step {
                    node: node("configs"),
                    port: port("config"),
                })
                .port(port("config"), Binding::Item)
                .port(port("name"), Binding::Literal("ci".to_string())),
        )
        .output(
            common::output("minted"),
            Binding::Step {
                node: node("tokens"),
                port: port("token"),
            },
        );

    let checked = check(&workflow, &catalog).expect("the for_each secret workflow checks cleanly");
    let mut inputs = IndexMap::new();
    inputs.insert(
        input("project"),
        Value::known(willikins_types::DopplerProject::parse("third-thoughts").unwrap()),
    );
    inputs.insert(
        input("environments"),
        Value::known_list(vec![
            willikins_types::EnvironmentSlug::parse("stg").unwrap(),
            willikins_types::EnvironmentSlug::parse("qa").unwrap(),
        ]),
    );

    let approved = plan(&checked, &inputs, &catalog).expect("plans against empty state");
    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("applies against empty state");

    // The same bytes `common::distinctive_token` is built from; a secret
    // type never hands them back, so the test holds its own copy.
    let raw_marker = "MARKER".repeat(7);

    // Both token instances minted, and the workflow output aggregated
    // them into one list value.
    for instance in ["third-thoughts/stg", "third-thoughts/qa"] {
        let found = applied
            .nodes
            .iter()
            .find(|applied_node| {
                applied_node.name == node("tokens")
                    && applied_node.instance.as_deref() == Some(instance)
            })
            .unwrap_or_else(|| panic!("no tokens[{instance}] instance"));
        assert!(matches!(found.status, NodeStatus::Created));
    }
    let minted = applied
        .outputs
        .get(&common::output("minted"))
        .expect("the `minted` workflow output resolves");
    assert!(minted.is_known(), "both instances minted a known value");
    assert!(minted.is_secret(), "a list of tokens is still a secret");
    assert_eq!(
        minted.render().to_string(),
        "[REDACTED DopplerServiceToken]",
        "a secret list renders as one marker"
    );

    // Every `NodeStarted` carries *that instance's* own resolved inputs:
    // `configs[stg]` is started with `environment = stg`, and
    // `tokens[third-thoughts/qa]` with `config = third-thoughts/qa`.
    for event in &observer.events {
        let ApplyEvent::NodeStarted {
            node: started,
            instance: Some(key),
            inputs: started_inputs,
        } = event
        else {
            continue;
        };
        if *started == node("configs") {
            let environment = started_inputs
                .get(&port("environment"))
                .expect("configs binds `environment` to `item`");
            assert_eq!(
                environment.render().to_string(),
                *key,
                "configs[{key}] was started with another instance's inputs"
            );
        } else if *started == node("tokens") {
            let config = started_inputs
                .get(&port("config"))
                .expect("tokens binds `config` to `item`");
            assert_eq!(
                config.render().to_string(),
                *key,
                "tokens[{key}] was started with another instance's inputs"
            );
        }
    }

    // And the bytes appear nowhere.
    let applied_json = serde_json::to_string(&applied).expect("Applied serializes");
    assert!(
        !applied_json.contains(&raw_marker),
        "Applied JSON leaked: {applied_json}"
    );
    assert!(applied_json.contains("REDACTED"), "{applied_json}");
    let applied_debug = format!("{applied:?}");
    assert!(
        !applied_debug.contains(&raw_marker),
        "Applied Debug leaked: {applied_debug}"
    );
    for event in &observer.events {
        let json = serde_json::to_string(event).expect("ApplyEvent serializes");
        assert!(!json.contains(&raw_marker), "event JSON leaked: {json}");
        let debug = format!("{event:?}");
        assert!(!debug.contains(&raw_marker), "event Debug leaked: {debug}");
    }
}

/// A live-reading tool whose observed non-secret output changed between
/// the approved plan and `apply`'s own re-plan is `DriftKind::Output`,
/// naming the port and carrying both values — and nothing runs.
#[test]
fn a_changed_observed_non_secret_output_is_output_drift() {
    let live_value = Arc::new(Mutex::new(
        willikins_types::GitHubOrg::parse("lightless-labs").unwrap(),
    ));
    let mut catalog = Catalog::new(willikins_types::registry());
    catalog
        .insert(Arc::new(common::MutableOutputTool::new(
            "test.live",
            Arc::clone(&live_value),
        )))
        .unwrap();

    let workflow = Workflow::new(workflow_name("live-read"))
        .node(node("live"), Node::new(tool_name("test.live")));
    let checked = check(&workflow, &catalog).expect("checks");
    let inputs = IndexMap::new();
    let approved = plan(&checked, &inputs, &catalog).expect("plans");

    // The provider's own state changes under us between approval and run.
    *live_value.lock().unwrap() = willikins_types::GitHubOrg::parse("other-org").unwrap();

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("a changed observed value must refuse");

    let ApplyError::Drift {
        node: name, kind, ..
    } = err
    else {
        panic!("expected Drift, got {err:?}");
    };
    assert_eq!(name.as_str(), "live");
    let DriftKind::Output {
        port: drifted_port,
        planned,
        observed: now,
    } = *kind
    else {
        panic!("expected Output drift, got {kind:?}");
    };
    assert_eq!(drifted_port.as_str(), "observed");
    assert_eq!(planned.render().to_string(), "lightless-labs");
    assert_eq!(now.render().to_string(), "other-org");
    assert!(observer.events.is_empty(), "nothing ran");
}
/// An `Unknown` value supplied for a required input that the tool's own
/// `read` never consults (the fake `github.repo.ensure` does not look at
/// `visibility` when the repository is absent) reaches the walk with a
/// plan that says `Create`. The executor refuses it —
/// [`ApplyError::UnknownRequiredInput`], naming the port and the workflow
/// input — rather than panicking or calling `ensure` without the value.
#[test]
fn an_unknown_value_for_a_required_input_is_refused_and_never_calls_ensure() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = apply_test_catalog(Arc::clone(&state));
    let workflow = Workflow::new(workflow_name("unknown-input"))
        .input(input("repo"), InputSpec::new(ty("GitHubRepo")))
        .input(input("visibility"), InputSpec::new(ty("RepoVisibility")))
        .node(
            node("repo"),
            Node::new(tool_name("github.repo.ensure"))
                .port(port("repo"), Binding::Input(input("repo")))
                .port(port("visibility"), Binding::Input(input("visibility"))),
        );
    let checked = check(&workflow, &catalog).expect("checks cleanly");

    let mut inputs = IndexMap::new();
    inputs.insert(
        input("repo"),
        Value::known(willikins_types::GitHubRepo::parse("lightless-labs/third-thoughts").unwrap()),
    );
    inputs.insert(input("visibility"), Value::unknown(ty("RepoVisibility")));

    // `plan` itself is happy: `github.repo.ensure`'s `read` only needs the
    // repository's own name to see that it is absent.
    let approved = plan(&checked, &inputs, &catalog).expect("plans with an unknown visibility");
    assert_eq!(approved.nodes[0].action, Action::Create);

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("a required input with no known value must refuse");

    let ApplyError::UnknownRequiredInput {
        node: blocked,
        port: blocked_port,
        input: blocked_input,
        applied,
        ..
    } = err
    else {
        panic!("expected UnknownRequiredInput, got {err:?}");
    };
    assert_eq!(blocked.as_str(), "repo");
    assert_eq!(blocked_port.as_str(), "visibility");
    assert_eq!(blocked_input.as_str(), "visibility");
    assert!(applied.nodes.is_empty(), "nothing was attempted");
    assert!(observer.events.is_empty(), "and nothing was reported");
    assert!(
        state.lock().unwrap().ensure_calls.is_empty(),
        "`ensure` must never be called without a required value"
    );
}

/// The same shape as the test above, with the resource already present:
/// the fresh plan says `NoOp`, so rule 4's `NoOp` branch answers
/// `Converged` without a provider call instead of refusing. Where the
/// `Unknown` came from — a workflow input rather than an un-re-readable
/// upstream output — makes no difference there, which is the one
/// behaviour [`ApplyError::UnknownRequiredInput`]'s new branch could have
/// changed and must not.
#[test]
fn an_unknown_required_input_on_a_planned_no_op_converges_without_a_call() {
    let repo = naming::v1::github_repo(
        &GitHubOrg::parse("lightless-labs").unwrap(),
        &ProjectSlug::parse("third-thoughts").unwrap(),
    );
    let state = Arc::new(Mutex::new(FakeState::new().with_repo(
        &repo,
        RepoVisibility::Private,
        true,
    )));
    let catalog = apply_test_catalog(Arc::clone(&state));
    let workflow = Workflow::new(workflow_name("converged-unknown-input"))
        .input(input("repo"), InputSpec::new(ty("GitHubRepo")))
        .input(input("visibility"), InputSpec::new(ty("RepoVisibility")))
        .node(
            node("repo"),
            Node::new(tool_name("github.repo.ensure"))
                .port(port("repo"), Binding::Input(input("repo")))
                .port(port("visibility"), Binding::Input(input("visibility"))),
        );
    let checked = check(&workflow, &catalog).expect("checks cleanly");

    let mut inputs = IndexMap::new();
    inputs.insert(input("repo"), Value::known(repo.clone()));
    inputs.insert(input("visibility"), Value::unknown(ty("RepoVisibility")));

    // The repository is present and its visibility is not knowably
    // different, so the fresh plan has nothing to do here.
    let approved = plan(&checked, &inputs, &catalog).expect("plans with an unknown visibility");
    assert_eq!(approved.nodes[0].action, Action::NoOp);

    let mut observer = RecordingObserver::new();
    let applied = apply(
        &checked,
        &inputs,
        &catalog,
        &approved,
        &Approval::Auto,
        &mut observer,
    )
    .expect("a planned no-op with an unknown input converges");

    assert!(matches!(applied.nodes[0].status, NodeStatus::Converged));
    assert_eq!(
        started_finished_pairs(&observer.events),
        vec![("repo".to_string(), None)],
        "a converged instance is still reported to the observer"
    );
    assert!(
        state.lock().unwrap().ensure_calls.is_empty(),
        "`Converged` means no provider call was made"
    );
}
