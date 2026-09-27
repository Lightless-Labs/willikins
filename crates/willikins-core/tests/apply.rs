//! Integration tests for [`willikins_core::apply`]: task 4a's acceptance
//! tests 5, 6 (the parts a test-only tool can produce without task 4b's
//! fake-provider changes), 7 (core half), and 8 (the fingerprint unit
//! half lives in `crates/willikins-core/src/plan.rs`'s own test module).
//!
//! These tests build their own catalog (via `common::apply_test_catalog`)
//! rather than using `willikins_providers_fake::catalog` unchanged: when
//! they were written, `doppler.service_token.ensure` reported its `token`
//! output `Unknown` on every `ensure` call, including a freshly minting
//! one, which would have made `ci_secret` hit `ApplyError::UnknownInput`
//! on the very first run. Task 4b has since fixed the shared fake, so the
//! substitute is no longer load-bearing; swapping these tests onto the
//! real catalog is tracked in
//! `todos/2026-09-13-apply-tests-on-the-real-fake-catalog.md`.

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

/// Rule 1 must not take the approved plan's own word for whether approval
/// is needed. [`willikins_core::Plan`] is a plain struct with public
/// fields, reconstructed from a journal line or handed across a process
/// boundary, so its `requires_approval` can arrive cleared; and a plan
/// legitimately made for a `Reversible` workflow whose instances happen to
/// fingerprint identically would pass the drift check too. The class of
/// the work about to run is a property of the *checked workflow*, which
/// the caller cannot forge, so that is what the gate reads.
#[test]
fn a_cleared_approval_flag_does_not_bypass_the_gate() {
    let state = Arc::new(Mutex::new(FakeState::new()));
    let catalog = apply_test_catalog(Arc::clone(&state));
    let workflow = common::irreversible_workflow();
    let checked = check(&workflow, &catalog).expect("irreversible workflow checks cleanly");
    assert_eq!(checked.class, Class::Irreversible);
    let inputs = common::new_rust_service_inputs();

    let mut forged = plan(&checked, &inputs, &catalog).expect("plan against empty state succeeds");
    forged.requires_approval = false;
    forged.class = Class::Reversible;

    // `plan` reads; only what `apply` does from here on counts.
    {
        let mut locked = state.lock().unwrap();
        locked.read_calls.clear();
        locked.ensure_calls.clear();
    }

    let mut observer = RecordingObserver::new();
    let err = apply(
        &checked,
        &inputs,
        &catalog,
        &forged,
        &Approval::Auto,
        &mut observer,
    )
    .expect_err("the checked workflow's class decides, not the plan's flag");
    match err {
        ApplyError::ApprovalRequired { class } => assert_eq!(class, Class::Irreversible),
        other => panic!("expected ApprovalRequired, got {other:?}"),
    }

    assert!(observer.events.is_empty(), "nothing ran before the refusal");
    let locked = state.lock().unwrap();
    assert!(
        locked.read_calls.is_empty(),
        "a refused apply must not even re-plan: {:?}",
        locked.read_calls
    );
    assert!(
        locked.ensure_calls.is_empty(),
        "and must call no ensure: {:?}",
        locked.ensure_calls
    );
}

// ---------------------------------------------------------------------
// Milestone 3d, decision (f): `plan` and `apply` deliver a binding
// through the edge `check` recorded, and never look a conversion up
// themselves. Synthetic types in a custom registry, so the conversions
// here register nothing in production. Acceptance tests 6 and 7 and
// equivalence item 2 of `docs/plans/2026-09-23-milestone-3d-conversions.md`.
// ---------------------------------------------------------------------

mod conversions {
    use std::sync::{Arc, Mutex};

    use indexmap::IndexMap;
    use willikins_core::{
        ApplyError, Approval, Binding, Catalog, Checked, Class, ConversionMismatch, Ensured,
        InputName, InputSpec, Inputs, Node, NodeName, Observation, Outputs, PlanError, PortName,
        PortSpec, PortType, RecordingObserver, Site, Tool, ToolError, ToolName, ToolSpec, TypeName,
        TypeRef, TypeRegistry, Value, Workflow, apply, check, plan,
    };
    use willikins_types::registry::TypeEntry;
    use willikins_types::{DomainType, SinkToken};

    #[derive(willikins_types::DomainType)]
    #[domain(
        pattern = "[a-z]+",
        description = "A conversion test source type.",
        example = "a"
    )]
    struct ConvA(String);

    #[derive(willikins_types::DomainType)]
    #[domain(
        pattern = "[a-z]+",
        description = "A conversion test target type.",
        example = "b"
    )]
    struct ConvB(String);

    #[derive(willikins_types::DomainType)]
    #[domain(
        min_len = 8,
        secret,
        description = "A secret conversion test source type.",
        example = "sekret-source"
    )]
    struct SecA(secrecy::SecretString);

    #[derive(willikins_types::DomainType)]
    #[domain(
        min_len = 8,
        secret,
        description = "A secret conversion test target type.",
        example = "sekret-target"
    )]
    struct SecB(secrecy::SecretString);

    impl From<ConvA> for ConvB {
        fn from(a: ConvA) -> Self {
            Self::parse(a.as_str()).unwrap_or_else(|_| unreachable!("the same grammar"))
        }
    }

    impl From<SecA> for SecB {
        fn from(a: SecA) -> Self {
            // Test-only: a secret-to-secret conversion has to read its
            // source to build its target.
            #[allow(clippy::disallowed_methods)]
            let token = SinkToken::new();
            Self::parse(a.expose(&token)).unwrap_or_else(|_| unreachable!("the same grammar"))
        }
    }

    /// The bytes of the one secret these tests carry. Assembled at run
    /// time so no dump can match the source literal by accident.
    fn secret_bytes() -> String {
        "BYTES".repeat(4)
    }

    fn registry() -> &'static TypeRegistry {
        Box::leak(Box::new(TypeRegistry::new(
            vec![
                TypeEntry::of::<ConvA>(),
                TypeEntry::of::<ConvB>(),
                TypeEntry::of::<SecA>(),
                TypeEntry::of::<SecB>(),
            ],
            willikins_types::conversions![ConvA => ConvB, SecA => SecB],
        )))
    }

    fn name(text: &str) -> TypeName {
        TypeName::parse(text).unwrap()
    }
    fn scalar(text: &str) -> TypeRef {
        TypeRef::scalar(name(text))
    }
    fn port(text: &str) -> PortName {
        PortName::parse(text).unwrap()
    }
    fn node(text: &str) -> NodeName {
        NodeName::parse(text).unwrap()
    }
    fn input(text: &str) -> InputName {
        InputName::parse(text).unwrap()
    }
    fn a(text: &str) -> ConvA {
        ConvA::parse(text).unwrap()
    }

    fn spec(tool: &str, inputs: &[(&str, &str)], outputs: &[(&str, &str)], pure: bool) -> ToolSpec {
        let mut in_map = IndexMap::new();
        for (port_name, ty) in inputs {
            in_map.insert(
                port(port_name),
                PortSpec {
                    ty: PortType::Exact(scalar(ty)),
                    required: true,
                    derived_only: false,
                },
            );
        }
        let mut out_map = IndexMap::new();
        for (port_name, ty) in outputs {
            out_map.insert(port(port_name), scalar(ty));
        }
        ToolSpec {
            name: ToolName::parse(tool).unwrap(),
            description: format!("Conversion test double `{tool}`."),
            inputs: in_map,
            outputs: out_map,
            key: Vec::new(),
            class: Class::Reversible,
            pure,
        }
    }

    /// A sink that records every input set it is called with, by `read`
    /// and by `ensure`, and produces no output.
    struct Recorder {
        spec: ToolSpec,
        reads: Mutex<Vec<Inputs>>,
        ensures: Mutex<Vec<Inputs>>,
    }

    impl Tool for Recorder {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }
        fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
            self.reads.lock().unwrap().push(inputs.clone());
            Ok(Observation::Absent {
                predicted: Outputs::new(),
            })
        }
        fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            self.ensures.lock().unwrap().push(inputs.clone());
            Ok(Ensured {
                outputs: Outputs::new(),
                changed: true,
            })
        }
    }

    /// A non-pure source whose `read` cannot predict its `out` (a
    /// `ConvA`), and whose `ensure` produces `ConvA("fromensure")`.
    struct UnknownAtPlan {
        spec: ToolSpec,
    }

    impl Tool for UnknownAtPlan {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }
        fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
            let mut predicted = Outputs::new();
            predicted.insert(port("out"), Value::unknown(scalar("ConvA")));
            Ok(Observation::Absent { predicted })
        }
        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            let mut outputs = Outputs::new();
            outputs.insert(port("out"), Value::known(a("fromensure")));
            Ok(Ensured {
                outputs,
                changed: true,
            })
        }
    }

    /// A pure echo of its `ConvA` input, for the `Keyed` edge.
    struct Echo {
        spec: ToolSpec,
    }

    impl Tool for Echo {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }
        fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
            let mut outputs = Outputs::new();
            outputs.insert(port("out"), inputs.get(&port("in")).unwrap().clone());
            Ok(Observation::Present(outputs))
        }
        fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            let Observation::Present(outputs) = self.read(inputs)? else {
                unreachable!("Echo always reads Present")
            };
            Ok(Ensured {
                outputs,
                changed: false,
            })
        }
    }

    /// A pure source of one known `SecA` secret.
    struct MintSecret {
        spec: ToolSpec,
    }

    impl Tool for MintSecret {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }
        fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
            let mut outputs = Outputs::new();
            outputs.insert(
                port("secret"),
                Value::known(SecA::parse(&secret_bytes()).unwrap()),
            );
            Ok(Observation::Present(outputs))
        }
        fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            let Observation::Present(outputs) = self.read(inputs)? else {
                unreachable!("MintSecret always reads Present")
            };
            Ok(Ensured {
                outputs,
                changed: false,
            })
        }
    }

    struct Fixture {
        catalog: Catalog,
        sink_b: Arc<Recorder>,
        sink_secret: Arc<Recorder>,
    }

    fn fixture() -> Fixture {
        let mut catalog = Catalog::new(registry());
        let sink_b = Arc::new(Recorder {
            spec: spec("conv.sink_b", &[("b", "ConvB")], &[], false),
            reads: Mutex::new(Vec::new()),
            ensures: Mutex::new(Vec::new()),
        });
        let sink_secret = Arc::new(Recorder {
            spec: spec("conv.sink_secret", &[("s", "SecB")], &[], false),
            reads: Mutex::new(Vec::new()),
            ensures: Mutex::new(Vec::new()),
        });
        catalog
            .insert(Arc::clone(&sink_b) as Arc<dyn Tool>)
            .unwrap();
        catalog
            .insert(Arc::clone(&sink_secret) as Arc<dyn Tool>)
            .unwrap();
        catalog
            .insert(Arc::new(UnknownAtPlan {
                spec: spec("conv.source", &[], &[("out", "ConvA")], false),
            }))
            .unwrap();
        catalog
            .insert(Arc::new(Echo {
                spec: spec("conv.echo", &[("in", "ConvA")], &[("out", "ConvA")], true),
            }))
            .unwrap();
        catalog
            .insert(Arc::new(MintSecret {
                spec: spec("conv.mint", &[], &[("secret", "SecA")], true),
            }))
            .unwrap();
        Fixture {
            catalog,
            sink_b,
            sink_secret,
        }
    }

    fn workflow_name(text: &str) -> willikins_types::WorkflowName {
        willikins_types::WorkflowName::parse(text).unwrap()
    }

    fn sink_b_node(binding: Binding) -> Node {
        Node::new(ToolName::parse("conv.sink_b").unwrap()).port(port("b"), binding)
    }

    fn only_b(inputs: &Inputs) -> &Value {
        inputs
            .get(&port("b"))
            .expect("the sink's `b` port is bound")
    }

    fn b_text(value: &Value) -> &str {
        value
            .downcast::<ConvB>()
            .expect("the sink's `b` port receives a known ConvB")
            .as_str()
    }

    /// Acceptance 6, the `Input` edge: an `A` workflow input known at
    /// plan time reaches the `B` port as `Known(B)`, in the planned node's
    /// inputs, in what `read` saw, and in what `ensure` was called with.
    #[test]
    fn a_known_input_edge_delivers_known_b_at_plan_and_apply() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-input"))
            .input(input("a"), InputSpec::new(scalar("ConvA")))
            .node(node("sink"), sink_b_node(Binding::Input(input("a"))));
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");
        let mut inputs = IndexMap::new();
        inputs.insert(input("a"), Value::known(a("hello")));

        let planned = plan(&checked, &inputs, &fixture.catalog).expect("plans");
        let delivered = only_b(&planned.nodes[0].inputs);
        assert_eq!(delivered.ty(), &scalar("ConvB"));
        assert_eq!(b_text(delivered), "hello");
        assert_eq!(
            serde_json::to_string(delivered).unwrap(),
            r#"{"type":"ConvB","list":false,"state":"known","value":"hello"}"#
        );
        assert_eq!(
            b_text(only_b(&fixture.sink_b.reads.lock().unwrap()[0])),
            "hello"
        );

        let mut observer = RecordingObserver::new();
        apply(
            &checked,
            &inputs,
            &fixture.catalog,
            &planned,
            &Approval::Auto,
            &mut observer,
        )
        .expect("applies");
        let ensures = fixture.sink_b.ensures.lock().unwrap();
        assert_eq!(ensures.len(), 1);
        assert_eq!(only_b(&ensures[0]).ty(), &scalar("ConvB"));
        assert_eq!(b_text(only_b(&ensures[0])), "hello");
    }

    /// Acceptance 6, the `Step` edge unknown at plan time: `plan` delivers
    /// `Unknown(A)` as `Unknown(B)`, and `apply` re-resolves the step
    /// against the run's real result and delivers `Known(B)` through the
    /// same edge before `ensure`.
    #[test]
    fn an_unknown_step_edge_plans_unknown_b_and_ensures_known_b() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-step"))
            .node(
                node("source"),
                Node::new(ToolName::parse("conv.source").unwrap()),
            )
            .node(
                node("sink"),
                sink_b_node(Binding::Step {
                    node: node("source"),
                    port: port("out"),
                }),
            );
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");
        let inputs = IndexMap::new();

        let planned = plan(&checked, &inputs, &fixture.catalog).expect("plans");
        let sink = planned
            .nodes
            .iter()
            .find(|n| n.name == node("sink"))
            .unwrap();
        let delivered = only_b(&sink.inputs);
        assert_eq!(delivered, &Value::unknown(scalar("ConvB")));
        assert_eq!(
            serde_json::to_string(delivered).unwrap(),
            r#"{"type":"ConvB","list":false,"state":"unknown"}"#
        );

        let mut observer = RecordingObserver::new();
        apply(
            &checked,
            &inputs,
            &fixture.catalog,
            &planned,
            &Approval::Auto,
            &mut observer,
        )
        .expect("applies");
        let ensures = fixture.sink_b.ensures.lock().unwrap();
        assert_eq!(ensures.len(), 1);
        assert_eq!(only_b(&ensures[0]).ty(), &scalar("ConvB"));
        assert_eq!(b_text(only_b(&ensures[0])), "fromensure");
    }

    /// Acceptance 6, the `Keyed` and `Item` edges: a `for_each` over a
    /// `list<A>` input delivers each item into a `B` port, and a keyed
    /// read of an `A` output delivers into a `B` port.
    #[test]
    fn keyed_and_item_edges_deliver_known_b() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-keyed-item"))
            .input(input("xs"), InputSpec::new(TypeRef::list_of(name("ConvA"))))
            .node(
                node("each"),
                sink_b_node(Binding::Item).for_each(Binding::Input(input("xs"))),
            )
            .node(
                node("echo"),
                Node::new(ToolName::parse("conv.echo").unwrap())
                    .port(port("in"), Binding::Item)
                    .for_each(Binding::Input(input("xs"))),
            )
            .node(
                node("sink"),
                sink_b_node(Binding::Keyed {
                    node: node("echo"),
                    key: "y".to_string(),
                    port: port("out"),
                }),
            );
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");
        let mut inputs = IndexMap::new();
        inputs.insert(input("xs"), Value::known_list(vec![a("x"), a("y")]));

        let planned = plan(&checked, &inputs, &fixture.catalog).expect("plans");
        let delivered: Vec<(String, Option<String>, String)> = planned
            .nodes
            .iter()
            .filter(|n| n.name != node("echo"))
            .map(|n| {
                let value = only_b(&n.inputs);
                assert_eq!(value.ty(), &scalar("ConvB"));
                (
                    n.name.to_string(),
                    n.instance.clone(),
                    b_text(value).to_owned(),
                )
            })
            .collect();
        assert_eq!(
            delivered,
            vec![
                ("each".to_owned(), Some("x".to_owned()), "x".to_owned()),
                ("each".to_owned(), Some("y".to_owned()), "y".to_owned()),
                ("sink".to_owned(), None, "y".to_owned()),
            ]
        );

        let mut observer = RecordingObserver::new();
        apply(
            &checked,
            &inputs,
            &fixture.catalog,
            &planned,
            &Approval::Auto,
            &mut observer,
        )
        .expect("applies");
        let ensures = fixture.sink_b.ensures.lock().unwrap();
        let texts: Vec<&str> = ensures.iter().map(|i| b_text(only_b(i))).collect();
        assert_eq!(texts, vec!["x", "y", "y"]);
    }

    /// Acceptance 7: a secret converted into a secret port renders
    /// `[REDACTED SecB]` in the plan and in every observer event, and its
    /// bytes appear nowhere, while the sink still receives them.
    #[test]
    fn a_converted_secret_is_redacted_as_its_target_everywhere() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-secret"))
            .node(
                node("mint"),
                Node::new(ToolName::parse("conv.mint").unwrap()),
            )
            .node(
                node("sink"),
                Node::new(ToolName::parse("conv.sink_secret").unwrap()).port(
                    port("s"),
                    Binding::Step {
                        node: node("mint"),
                        port: port("secret"),
                    },
                ),
            );
        let checked = check(&workflow, &fixture.catalog).expect("SecA converts to SecB");
        let inputs = IndexMap::new();

        let planned = plan(&checked, &inputs, &fixture.catalog).expect("plans");
        let sink = planned
            .nodes
            .iter()
            .find(|n| n.name == node("sink"))
            .unwrap();
        let delivered = sink.inputs.get(&port("s")).unwrap();
        assert_eq!(delivered.ty(), &scalar("SecB"));
        assert_eq!(delivered.render().to_string(), "[REDACTED SecB]");

        let mut observer = RecordingObserver::new();
        let applied = apply(
            &checked,
            &inputs,
            &fixture.catalog,
            &planned,
            &Approval::Auto,
            &mut observer,
        )
        .expect("applies");

        let bytes = secret_bytes();
        for (what, dump) in [
            ("plan json", serde_json::to_string(&planned).unwrap()),
            ("plan debug", format!("{planned:?}")),
            ("applied json", serde_json::to_string(&applied).unwrap()),
            (
                "events json",
                serde_json::to_string(&observer.events).unwrap(),
            ),
            ("events debug", format!("{:?}", observer.events)),
        ] {
            assert!(!dump.contains(&bytes), "{what} leaked the converted secret");
        }
        let events = serde_json::to_string(&observer.events).unwrap();
        assert!(events.contains("[REDACTED SecB]"), "events: {events}");

        #[allow(clippy::disallowed_methods)]
        let token = SinkToken::new();
        let ensures = fixture.sink_secret.ensures.lock().unwrap();
        let received = ensures[0].get(&port("s")).unwrap();
        assert_eq!(received.ty(), &scalar("SecB"));
        assert_eq!(
            received.downcast::<SecB>().unwrap().expose(&token),
            bytes,
            "the sink receives the converted secret's own bytes"
        );
    }

    /// Equivalence item 2: an edge with no conversion delivers its
    /// argument unchanged, equal and serializing identically. This is what
    /// keeps every pre-3d document's plan byte-identical.
    #[test]
    fn an_exact_edge_delivers_its_argument_unchanged() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-exact"))
            .input(input("b"), InputSpec::new(scalar("ConvB")))
            .node(node("sink"), sink_b_node(Binding::Input(input("b"))));
        let checked = check(&workflow, &fixture.catalog).expect("B binds to B exactly");
        let edge = &checked.types[&node("sink")][&port("b")];
        assert!(edge.conversion().is_none());
        for value in [
            Value::known(ConvB::parse("same").unwrap()),
            Value::unknown(scalar("ConvB")),
            // Even a value of another type passes through untouched: an
            // exact edge never converts anything, so there is nothing for
            // the backstop below to refuse either.
            Value::known(a("other")),
        ] {
            let delivered = edge
                .deliver(value.clone())
                .expect("an exact edge never refuses");
            assert_eq!(delivered, value);
            assert_eq!(
                serde_json::to_string(&delivered).unwrap(),
                serde_json::to_string(&value).unwrap()
            );
        }
    }

    /// Follow-up to milestone 3d, 2026-09-24. The operator: "I'd much
    /// rather have it fail loudly at parsing than silently go through."
    /// `Edge::deliver` used to pass a value of another type through
    /// unchanged so the generated converter's one downcast could never be
    /// reached with the wrong type; it now refuses instead, loudly and by
    /// type names only -- this is the backstop `Edge::deliver` itself
    /// carries, reached even when nothing upstream (a hand-built
    /// `Checked`, for instance) caught the mismatch first.
    #[test]
    fn an_edge_with_a_conversion_refuses_a_foreign_value_directly() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-foreign"))
            .input(input("a"), InputSpec::new(scalar("ConvA")))
            .node(node("sink"), sink_b_node(Binding::Input(input("a"))));
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");
        let edge = &checked.types[&node("sink")][&port("b")];
        assert!(edge.conversion().is_some());
        let foreign = Value::known(ConvB::parse("already").unwrap());
        assert_eq!(
            edge.deliver(foreign),
            Err(ConversionMismatch {
                expected: scalar("ConvA"),
                found: scalar("ConvB"),
            })
        );
    }

    /// `err` is exactly `PlanError::InputTypeMismatch` for workflow input
    /// `name`, declared `expected`, supplied `found`.
    fn assert_input_type_mismatch(
        err: &PlanError,
        name: &str,
        expected: &TypeRef,
        found: &TypeRef,
    ) {
        match err {
            PlanError::InputTypeMismatch {
                input: got_input,
                expected: got_expected,
                found: got_found,
            } => {
                assert_eq!(got_input, &input(name));
                assert_eq!(got_expected, expected);
                assert_eq!(got_found, found);
            }
            other => panic!("expected InputTypeMismatch, got {other:?}"),
        }
    }

    /// Follow-up to milestone 3d, 2026-09-27 (the operator: "I'd much
    /// rather have it fail loudly at parsing than silently go through"):
    /// a caller that supplies a wrong-typed workflow input to `plan` is
    /// refused while `plan` parses its inputs, as
    /// `PlanError::InputTypeMismatch`, before any node is planned. Until
    /// that check landed this reached the converting edge and was refused
    /// there, as `EdgeTypeMismatch`, which stays the backstop for a
    /// hand-built `Checked`.
    #[test]
    fn a_converted_edge_refuses_a_foreign_workflow_input_through_plan() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-foreign-plan"))
            .input(input("a"), InputSpec::new(scalar("ConvA")))
            .node(node("sink"), sink_b_node(Binding::Input(input("a"))));
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");
        let foreign = Value::known(ConvB::parse("already").unwrap());

        let mut inputs = IndexMap::new();
        inputs.insert(input("a"), foreign);
        let err = plan(&checked, &inputs, &fixture.catalog).expect_err("a ConvB is not a ConvA");
        assert_input_type_mismatch(&err, "a", &scalar("ConvA"), &scalar("ConvB"));
        assert_eq!(
            err.to_string(),
            "workflow input `a`: expected ConvA, found `ConvB`"
        );
        assert!(fixture.sink_b.reads.lock().unwrap().is_empty());
        assert!(fixture.sink_b.ensures.lock().unwrap().is_empty());
    }

    /// Another Rust type whose `TYPE_NAME` is also `ConvA`.
    mod impostor {
        #[derive(willikins_types::DomainType)]
        #[domain(
            pattern = "[a-z]+",
            description = "Not the registered ConvA.",
            example = "a"
        )]
        pub(super) struct ConvA(String);
    }

    /// Independent review of milestone 3d, and its follow-up: a workflow
    /// input whose type *name* is the declared type but whose Rust type is
    /// not is refused while `plan` parses its inputs -- by the registry
    /// entry's own `TypeId` test, never by comparing names -- at `plan`
    /// and at `apply`'s opening replan, before any tool reads or ensures
    /// anything. `expected` and `found` print the same, so `Display` says
    /// what actually differs.
    #[test]
    fn a_same_named_input_of_another_rust_type_is_refused_not_panicked() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-impostor"))
            .input(input("a"), InputSpec::new(scalar("ConvA")))
            .node(node("sink"), sink_b_node(Binding::Input(input("a"))));
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");
        let impostor = Value::known(impostor::ConvA::parse("imp").unwrap());
        assert_eq!(impostor.ty(), &scalar("ConvA"));

        let mut genuine_inputs = IndexMap::new();
        genuine_inputs.insert(input("a"), Value::known(a("real")));
        let approved =
            plan(&checked, &genuine_inputs, &fixture.catalog).expect("a real ConvA plans");
        fixture.sink_b.reads.lock().unwrap().clear();

        let mut inputs = IndexMap::new();
        inputs.insert(input("a"), impostor);
        let err = plan(&checked, &inputs, &fixture.catalog)
            .expect_err("a same-named impostor is not a real ConvA");
        assert_input_type_mismatch(&err, "a", &scalar("ConvA"), &scalar("ConvA"));
        assert_eq!(
            err.to_string(),
            "workflow input `a`: expected ConvA, found a value of another Rust type also named \
             `ConvA`"
        );

        let mut observer = RecordingObserver::new();
        let apply_err = apply(
            &checked,
            &inputs,
            &fixture.catalog,
            &approved,
            &Approval::Auto,
            &mut observer,
        )
        .expect_err("apply's opening replan parses the same inputs");
        match apply_err {
            ApplyError::Plan { error } => {
                assert_input_type_mismatch(&error, "a", &scalar("ConvA"), &scalar("ConvA"));
            }
            other => panic!("expected ApplyError::Plan{{InputTypeMismatch}}, got {other:?}"),
        }
        assert!(observer.events.is_empty(), "apply refused before any event");
        assert!(fixture.sink_b.reads.lock().unwrap().is_empty());
        assert!(fixture.sink_b.ensures.lock().unwrap().is_empty());
    }

    /// A declared `list<ConvA>` input whose supplied list holds one object
    /// that is not a real `ConvA` -- a foreign `ConvB`, or a same-named
    /// impostor -- is refused as a whole before any instance is planned,
    /// naming the offending element's own type. Without the parse-time
    /// check the first, well-typed item was already read by the sink when
    /// the bad one reached its edge.
    #[test]
    fn a_list_input_with_one_foreign_or_impostor_element_is_refused_before_any_read() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-list-element"))
            .input(input("xs"), InputSpec::new(TypeRef::list_of(name("ConvA"))))
            .node(
                node("each"),
                sink_b_node(Binding::Item).for_each(Binding::Input(input("xs"))),
            );
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");

        for (element, found) in [
            (
                Arc::new(ConvB::parse("bad").unwrap()) as Arc<dyn willikins_types::DomainObject>,
                TypeRef::list_of(name("ConvB")),
            ),
            (
                Arc::new(impostor::ConvA::parse("imp").unwrap())
                    as Arc<dyn willikins_types::DomainObject>,
                TypeRef::list_of(name("ConvA")),
            ),
        ] {
            let forged = Value::known_dyn_list(
                name("ConvA"),
                vec![
                    Arc::new(a("ok")) as Arc<dyn willikins_types::DomainObject>,
                    element,
                ],
            );
            let mut inputs = IndexMap::new();
            inputs.insert(input("xs"), forged);
            let err = plan(&checked, &inputs, &fixture.catalog)
                .expect_err("one element is not a real ConvA");
            assert_input_type_mismatch(&err, "xs", &TypeRef::list_of(name("ConvA")), &found);
            assert!(
                fixture.sink_b.reads.lock().unwrap().is_empty(),
                "no instance was read, not even the well-typed first one"
            );
        }
    }

    /// The root cause, on an edge with no conversion at all: a wrong-typed
    /// workflow input bound to an exact port used to reach the tool
    /// unchecked (the `Echo` double reads it without a typed accessor, so
    /// `plan` succeeded). It is now refused while `plan` parses its inputs.
    #[test]
    fn an_exact_edge_input_of_another_type_is_refused_before_any_tool_reads_it() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-exact-foreign"))
            .input(input("a"), InputSpec::new(scalar("ConvA")))
            .node(
                node("echo"),
                Node::new(ToolName::parse("conv.echo").unwrap())
                    .port(port("in"), Binding::Input(input("a"))),
            );
        let checked = check(&workflow, &fixture.catalog).expect("A binds to A exactly");
        assert!(
            checked.types[&node("echo")][&port("in")]
                .conversion()
                .is_none()
        );

        for (value, found) in [
            (Value::known(ConvB::parse("bad").unwrap()), scalar("ConvB")),
            (
                Value::known(impostor::ConvA::parse("imp").unwrap()),
                scalar("ConvA"),
            ),
        ] {
            let mut inputs = IndexMap::new();
            inputs.insert(input("a"), value);
            let err = plan(&checked, &inputs, &fixture.catalog)
                .expect_err("the echo's exact ConvA port must not receive it");
            assert_input_type_mismatch(&err, "a", &scalar("ConvA"), &found);
        }
    }

    /// The declared `TypeRef` must match exactly, list flag included, for
    /// an `Unknown` value as much as a known one; an `Unknown` of the
    /// declared type itself is accepted, as it always was.
    #[test]
    fn an_input_of_the_wrong_shape_or_unknown_type_is_refused_and_a_right_unknown_plans() {
        let fixture = fixture();
        let scalar_workflow = Workflow::new(workflow_name("conv-shape-scalar"))
            .input(input("a"), InputSpec::new(scalar("ConvA")))
            .node(node("sink"), sink_b_node(Binding::Input(input("a"))));
        let scalar_checked =
            check(&scalar_workflow, &fixture.catalog).expect("A converts to B in one hop");

        for (value, found) in [
            (Value::unknown(scalar("ConvB")), scalar("ConvB")),
            (
                Value::unknown(TypeRef::list_of(name("ConvA"))),
                TypeRef::list_of(name("ConvA")),
            ),
            (
                Value::known_list(vec![a("x")]),
                TypeRef::list_of(name("ConvA")),
            ),
        ] {
            let mut inputs = IndexMap::new();
            inputs.insert(input("a"), value);
            let err =
                plan(&scalar_checked, &inputs, &fixture.catalog).expect_err("not a scalar ConvA");
            assert_input_type_mismatch(&err, "a", &scalar("ConvA"), &found);
        }
        assert!(fixture.sink_b.reads.lock().unwrap().is_empty());

        let list_workflow = Workflow::new(workflow_name("conv-shape-list"))
            .input(input("xs"), InputSpec::new(TypeRef::list_of(name("ConvA"))))
            .node(
                node("each"),
                sink_b_node(Binding::Item).for_each(Binding::Input(input("xs"))),
            );
        let list_checked =
            check(&list_workflow, &fixture.catalog).expect("A converts to B in one hop");
        let mut inputs = IndexMap::new();
        inputs.insert(input("xs"), Value::known(a("x")));
        let err = plan(&list_checked, &inputs, &fixture.catalog).expect_err("not a list");
        assert_input_type_mismatch(
            &err,
            "xs",
            &TypeRef::list_of(name("ConvA")),
            &scalar("ConvA"),
        );

        let mut inputs = IndexMap::new();
        inputs.insert(input("a"), Value::unknown(scalar("ConvA")));
        let planned = plan(&scalar_checked, &inputs, &fixture.catalog)
            .expect("an Unknown of the declared type is well typed");
        assert_eq!(
            only_b(&planned.nodes[0].inputs),
            &Value::unknown(scalar("ConvB"))
        );
    }

    /// A secret supplied for a non-secret workflow input is refused by type
    /// name only: its bytes reach neither the error's `Display`, `Debug`,
    /// or JSON, nor any tool, nor an apply event.
    #[test]
    fn a_secret_supplied_for_a_public_input_is_refused_without_its_bytes() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-secret-input"))
            .input(input("a"), InputSpec::new(scalar("ConvA")))
            .node(node("sink"), sink_b_node(Binding::Input(input("a"))));
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");

        let mut genuine_inputs = IndexMap::new();
        genuine_inputs.insert(input("a"), Value::known(a("real")));
        let approved =
            plan(&checked, &genuine_inputs, &fixture.catalog).expect("a real ConvA plans");
        fixture.sink_b.reads.lock().unwrap().clear();

        let mut inputs = IndexMap::new();
        inputs.insert(
            input("a"),
            Value::known(SecA::parse(&secret_bytes()).unwrap()),
        );
        let err = plan(&checked, &inputs, &fixture.catalog).expect_err("a SecA is not a ConvA");
        assert_input_type_mismatch(&err, "a", &scalar("ConvA"), &scalar("SecA"));
        for dump in [
            err.to_string(),
            format!("{err:?}"),
            serde_json::to_string(&err).unwrap(),
        ] {
            assert!(!dump.contains(&secret_bytes()), "leaked: {dump}");
        }

        let mut observer = RecordingObserver::new();
        let apply_err = apply(
            &checked,
            &inputs,
            &fixture.catalog,
            &approved,
            &Approval::Auto,
            &mut observer,
        )
        .expect_err("apply's opening replan refuses it too");
        assert!(!format!("{apply_err:?}").contains(&secret_bytes()));
        assert!(!apply_err.to_string().contains(&secret_bytes()));
        assert!(observer.events.is_empty());
        assert!(fixture.sink_b.reads.lock().unwrap().is_empty());
        assert!(fixture.sink_b.ensures.lock().unwrap().is_empty());
    }

    /// The backstop's own reason to exist: a `Checked` `check` never
    /// built. `Checked::types`' fields are public precisely so a test like
    /// this one can move a real, `check`-built converting edge onto a
    /// different port than `check` chose it for. `plan`'s own input
    /// parsing cannot see this -- both `a` and `c` are exactly the types
    /// their declared inputs say -- so only `Edge::deliver`'s own backstop
    /// catches it, at both `plan` and `apply`.
    #[test]
    fn a_converted_edge_moved_onto_another_port_errors_at_plan_and_apply() {
        let fixture = fixture();
        let workflow = Workflow::new(workflow_name("conv-forged"))
            .input(input("a"), InputSpec::new(scalar("ConvA")))
            .input(input("c"), InputSpec::new(scalar("ConvB")))
            .node(node("sink1"), sink_b_node(Binding::Input(input("a"))))
            .node(node("sink2"), sink_b_node(Binding::Input(input("c"))));
        let checked =
            check(&workflow, &fixture.catalog).expect("A converts to B; B binds to B exactly");

        let converting_edge = checked.types[&node("sink1")][&port("b")].clone();
        assert!(converting_edge.conversion().is_some());
        assert!(
            checked.types[&node("sink2")][&port("b")]
                .conversion()
                .is_none()
        );

        let mut forged_types = checked.types.clone();
        forged_types
            .get_mut(&node("sink2"))
            .expect("sink2 has a types entry")
            .insert(port("b"), converting_edge);
        let forged = Checked {
            types: forged_types,
            ..checked.clone()
        };

        let mut inputs = IndexMap::new();
        inputs.insert(input("a"), Value::known(a("hello")));
        inputs.insert(input("c"), Value::known(ConvB::parse("world").unwrap()));

        let plan_err = plan(&forged, &inputs, &fixture.catalog)
            .expect_err("sink2's real ConvB does not match the moved edge's ConvA source");
        match plan_err {
            PlanError::EdgeTypeMismatch {
                site,
                expected,
                found,
            } => {
                assert_eq!(
                    site,
                    Site::Port {
                        node: node("sink2"),
                        port: port("b"),
                    }
                );
                assert_eq!(expected, scalar("ConvA"));
                assert_eq!(found, scalar("ConvB"));
            }
            other => panic!("expected EdgeTypeMismatch, got {other:?}"),
        }

        // A genuine plan against the real (unforged) `Checked`, so `apply`
        // has an `approved: &Plan` to diff against -- its own opening
        // replan against the forged `Checked` is what surfaces the same
        // refusal for `apply`, before any drift check or `ensure` call.
        let genuine_planned =
            plan(&checked, &inputs, &fixture.catalog).expect("the real Checked plans");
        let mut observer = RecordingObserver::new();
        let apply_err = apply(
            &forged,
            &inputs,
            &fixture.catalog,
            &genuine_planned,
            &Approval::Auto,
            &mut observer,
        )
        .expect_err("apply's own opening replan hits the same forged edge");
        match apply_err {
            ApplyError::Plan {
                error:
                    PlanError::EdgeTypeMismatch {
                        site,
                        expected,
                        found,
                    },
            } => {
                assert_eq!(
                    site,
                    Site::Port {
                        node: node("sink2"),
                        port: port("b"),
                    }
                );
                assert_eq!(expected, scalar("ConvA"));
                assert_eq!(found, scalar("ConvB"));
            }
            other => panic!("expected ApplyError::Plan{{EdgeTypeMismatch}}, got {other:?}"),
        }
        assert!(
            fixture.sink_b.ensures.lock().unwrap().is_empty(),
            "nothing was ever ensured"
        );
    }

    /// A non-pure source whose `read` cannot predict its `out` (a
    /// `ConvA`), like `UnknownAtPlan`, but whose `ensure` returns a
    /// same-named *impostor* rather than a real `ConvA`. This is the one
    /// route into `resolve_instance_inputs`'s own re-delivery that `plan`'s
    /// opening replan cannot see: at plan time the value is `Unknown`, and
    /// `Value::converted`'s `Unknown` arm never inspects an object, so
    /// nothing is wrong yet. Only once `ensure` actually runs, at apply
    /// time, does the impostor exist to be delivered.
    struct ImpostorAtEnsure {
        spec: ToolSpec,
    }

    impl Tool for ImpostorAtEnsure {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }
        fn read(&self, _inputs: &Inputs) -> Result<Observation, ToolError> {
            let mut predicted = Outputs::new();
            predicted.insert(port("out"), Value::unknown(scalar("ConvA")));
            Ok(Observation::Absent { predicted })
        }
        fn ensure(&self, _inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
            let mut outputs = Outputs::new();
            outputs.insert(
                port("out"),
                Value::known(impostor::ConvA::parse("imp").unwrap()),
            );
            Ok(Ensured {
                outputs,
                changed: true,
            })
        }
    }

    /// Advisor-identified gap, follow-up to milestone 3d, 2026-09-24:
    /// `resolve_instance_inputs`'s own re-delivery, at apply time, is
    /// otherwise unreached by any other test here -- `plan`'s opening
    /// replan already refuses everything the tests above reach. Pins that
    /// this second delivery point refuses the impostor too, loudly,
    /// rather than delivering it to the sink's `ensure`.
    #[test]
    fn apply_refuses_a_same_named_impostor_from_ensure_that_plan_could_not_see() {
        let mut fixture = fixture();
        fixture
            .catalog
            .insert(Arc::new(ImpostorAtEnsure {
                spec: spec("conv.source_impostor", &[], &[("out", "ConvA")], false),
            }))
            .unwrap();
        let workflow = Workflow::new(workflow_name("conv-ensure-impostor"))
            .node(
                node("source"),
                Node::new(ToolName::parse("conv.source_impostor").unwrap()),
            )
            .node(
                node("sink"),
                sink_b_node(Binding::Step {
                    node: node("source"),
                    port: port("out"),
                }),
            );
        let checked = check(&workflow, &fixture.catalog).expect("A converts to B in one hop");
        let inputs = IndexMap::new();

        let planned = plan(&checked, &inputs, &fixture.catalog)
            .expect("plans: the value is Unknown at plan time, so nothing is wrong yet");
        let sink = planned
            .nodes
            .iter()
            .find(|n| n.name == node("sink"))
            .unwrap();
        assert_eq!(only_b(&sink.inputs), &Value::unknown(scalar("ConvB")));

        let mut observer = RecordingObserver::new();
        let err = apply(
            &checked,
            &inputs,
            &fixture.catalog,
            &planned,
            &Approval::Auto,
            &mut observer,
        )
        .expect_err("ensure's impostor is not a real ConvA");
        match err {
            ApplyError::Plan {
                error:
                    PlanError::EdgeTypeMismatch {
                        site,
                        expected,
                        found,
                    },
            } => {
                assert_eq!(
                    site,
                    Site::Port {
                        node: node("sink"),
                        port: port("b"),
                    }
                );
                assert_eq!(expected, scalar("ConvA"));
                assert_eq!(found, scalar("ConvA"));
            }
            other => panic!("expected ApplyError::Plan{{EdgeTypeMismatch}}, got {other:?}"),
        }
        assert!(
            fixture.sink_b.ensures.lock().unwrap().is_empty(),
            "the sink's ensure never runs"
        );
    }
}
