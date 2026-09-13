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
