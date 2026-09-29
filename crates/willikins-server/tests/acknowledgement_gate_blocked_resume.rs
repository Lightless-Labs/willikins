//! Task B2 (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, T3c's
//! follow-up addendum): `resolve_recorded_inputs` used to require a
//! recorded value for *every* declared workflow input, but G3's own
//! design (decision (j), point 6) deliberately never records an
//! unsupplied `OperatorAcknowledgement` input at all. So the very first
//! `apply` of a plan whose acknowledgement gate is unmet -- decision (j)'s
//! "run, get blocked, re-run" -- refused instead of starting a blocked
//! run. `crates/willikins-core/tests/operator_acknowledge_document.rs`
//! and `crates/willikins-cli/tests/sample_apply_blocked_redaction.rs`
//! each proved a *piece* of decision (j) (the fake catalog's `plan`
//! alone; the CLI's `apply --approve` with the acknowledgements already
//! supplied, sidestepping this exact defect). This is the first test
//! that drives the missing case end to end: a real `Butler::apply` (over
//! a real `FileJournal`, reopened independently to prove durability) and
//! the MCP surface (`plan`/`apply`/`run_status`), both before and after
//! the operator supplies the acknowledgement.

mod common;

use std::str::FromStr;
use std::sync::Arc;

use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};

use willikins_core::NodeStatus;
use willikins_journal::{BLOCKED_NEXT_STEP, Journal, PrincipalId, RunId, RunNode, RunState};
use willikins_server::{Butler, WillikinsHandler};

const FIXTURE_NAME: &str = "acknowledgement-gate-resume";

fn principal() -> PrincipalId {
    PrincipalId::parse("agent").unwrap()
}

/// Serve `butler` over an in-process duplex pair and connect a plain
/// rmcp client to it, mirroring `tests/mcp_server.rs`'s own `connect`.
async fn connect(butler: Arc<Butler>, principal: PrincipalId) -> RunningService<RoleClient, ()> {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let handler = WillikinsHandler::new(butler, principal);
    tokio::spawn(async move {
        if let Ok(service) = handler.serve(server_io).await {
            let _ = service.waiting().await;
        }
    });
    ().serve(client_io)
        .await
        .expect("the client connects and initializes")
}

/// A `tools/call` request for `name` with `arguments` (a JSON object).
fn call(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
    let object = match arguments {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Null => serde_json::Map::new(),
        other => panic!("tool arguments must be a JSON object, got {other}"),
    };
    CallToolRequestParams::new(name).with_arguments(object)
}

fn status_of<'a>(nodes: &'a [RunNode], name: &str) -> &'a NodeStatus {
    &nodes
        .iter()
        .find(|n| n.node.as_str() == name)
        .unwrap_or_else(|| panic!("node `{name}` is in the run record: {nodes:?}"))
        .status
}

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines)] // one scenario, walked start to end across two runs; splitting scatters it
async fn an_unmet_acknowledgement_gate_blocks_then_resumes_through_a_real_apply_journal_and_mcp_run_status()
 {
    let workflows_dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(
        workflows_dir.path(),
        "acknowledgement-gate-resume.yaml",
        "acknowledgement-gate-resume.yaml",
    );
    let journal_dir = tempfile::tempdir().unwrap();
    let journal_path = journal_dir.path().join("journal.jsonl");

    // -----------------------------------------------------------------
    // Run 1: the acknowledgement is not supplied at all. Scoped so its
    // own `Butler` (and every strong reference to its `FileJournal`, the
    // exclusive lock's own last holder) is fully dropped before the
    // journal is reopened independently below -- mirroring
    // `tests/file_journal_round_trip.rs`'s own `drop(butler)`.
    let run_id = {
        let (_state, catalog) = Butler::fake_catalog();
        let clock = common::manual_clock();
        let butler = Arc::new(common::butler_over_file_journal(
            workflows_dir.path(),
            &journal_path,
            catalog,
            clock,
        ));
        let client = connect(Arc::clone(&butler), principal()).await;

        let plan_result = client
            .call_tool(call(
                "plan",
                serde_json::json!({ "workflow": FIXTURE_NAME, "inputs": {} }),
            ))
            .await
            .expect("plan is routed");
        assert_ne!(plan_result.is_error, Some(true), "{plan_result:?}");
        let plan_json = plan_result
            .structured_content
            .expect("plan returns structured content");
        assert_eq!(plan_json["approval"], "automatic", "{plan_json}");
        let blocked = plan_json["plan"]["blocked"]
            .as_array()
            .expect("blocked is an array");
        assert_eq!(blocked.len(), 1, "{plan_json}");
        assert_eq!(blocked[0]["tool"], "operator.acknowledge", "{plan_json}");
        let plan_id = plan_json["plan_id"].as_str().unwrap().to_string();

        // Before this task's fix: `apply` on exactly this plan refused with
        // `RecordedInputUnreadable` (naming `gate_done`) instead of starting
        // a blocked run -- the defect this test exists to close.
        let apply_result = client
            .call_tool(call("apply", serde_json::json!({ "plan_id": plan_id })))
            .await
            .expect("apply is routed");
        assert_ne!(
            apply_result.is_error,
            Some(true),
            "apply on a plan whose gate is unmet must start a (blocked) run, not refuse: {apply_result:?}"
        );
        let apply_json = apply_result
            .structured_content
            .expect("apply returns structured content");
        let run_id_str = apply_json["run_id"].as_str().unwrap().to_string();
        let run_id = RunId::from_str(&run_id_str).unwrap();

        let run_record = common::wait_for_run(&butler, run_id, 2000);
        assert_eq!(run_record.state, RunState::Blocked, "{run_record:?}");
        assert_eq!(run_record.blocked.len(), 1, "{run_record:?}");
        assert_eq!(
            run_record.next_step.as_deref(),
            Some(BLOCKED_NEXT_STEP),
            "{run_record:?}"
        );
        assert!(
            matches!(
                status_of(&run_record.nodes, "independent"),
                NodeStatus::Computed
            ),
            "{run_record:?}"
        );
        assert!(
            matches!(status_of(&run_record.nodes, "gate"), NodeStatus::Blocked),
            "{run_record:?}"
        );
        assert!(
            matches!(
                status_of(&run_record.nodes, "downstream"),
                NodeStatus::Skipped
            ),
            "{run_record:?}"
        );

        // The MCP `run_status` surface itself agrees.
        let run_status_result = client
            .call_tool(call(
                "run_status",
                serde_json::json!({ "run_id": run_id_str }),
            ))
            .await
            .expect("run_status is routed");
        assert_ne!(
            run_status_result.is_error,
            Some(true),
            "{run_status_result:?}"
        );
        let status_json = run_status_result
            .structured_content
            .expect("run_status returns structured content");
        assert_eq!(status_json["state"], "blocked", "{status_json}");
        assert_eq!(status_json["next_step"], BLOCKED_NEXT_STEP, "{status_json}");

        drop(client);
        drop(butler);
        run_id
    };

    // The journal itself durably recorded the blocked outcome -- reopen
    // it independently of the `Butler` that produced it (a simulated
    // restart), mirroring `tests/file_journal_round_trip.rs`.
    {
        let reopened = common::open_file_journal_read_only(&journal_path);
        let replayed = reopened
            .run(&run_id)
            .expect("the run is in the reopened journal");
        assert_eq!(replayed.state, RunState::Blocked, "{replayed:?}");
        assert_eq!(replayed.blocked.len(), 1, "{replayed:?}");
        // `reopened` drops here, releasing the exclusive lock before a
        // fresh `Butler` opens the same file for run 2 below.
    }

    // -----------------------------------------------------------------
    // Run 2 ("re-run this document once done"): supply the
    // acknowledgement and converge, over a fresh `Butler` reopening the
    // same on-disk journal -- the "redeploy between plan and apply"
    // scenario this module's own doc names, and decision (j)'s own "no
    // saved run state: re-running converges" design.
    // -----------------------------------------------------------------
    let (_state, catalog) = Butler::fake_catalog();
    let clock = common::manual_clock();
    let butler = Arc::new(common::butler_over_file_journal(
        workflows_dir.path(),
        &journal_path,
        catalog,
        clock,
    ));
    let client = connect(Arc::clone(&butler), principal()).await;

    let plan_result = client
        .call_tool(call(
            "plan",
            serde_json::json!({ "workflow": FIXTURE_NAME, "inputs": { "gate_done": "done" } }),
        ))
        .await
        .expect("plan is routed");
    assert_ne!(plan_result.is_error, Some(true), "{plan_result:?}");
    let plan_json = plan_result
        .structured_content
        .expect("plan returns structured content");
    // `Plan.blocked` is `skip_serializing_if = "Vec::is_empty"`, so an
    // empty one is absent from the JSON entirely, not an empty array.
    assert!(
        plan_json["plan"]["blocked"].is_null()
            || plan_json["plan"]["blocked"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "{plan_json}"
    );
    let plan_id = plan_json["plan_id"].as_str().unwrap().to_string();

    let apply_result = client
        .call_tool(call("apply", serde_json::json!({ "plan_id": plan_id })))
        .await
        .expect("apply is routed");
    assert_ne!(apply_result.is_error, Some(true), "{apply_result:?}");
    let apply_json = apply_result
        .structured_content
        .expect("apply returns structured content");
    let run_id_2 = RunId::from_str(apply_json["run_id"].as_str().unwrap()).unwrap();

    let run_record_2 = common::wait_for_run(&butler, run_id_2, 2000);
    assert_eq!(run_record_2.state, RunState::Succeeded, "{run_record_2:?}");
    assert!(run_record_2.blocked.is_empty(), "{run_record_2:?}");
    assert!(
        matches!(
            status_of(&run_record_2.nodes, "downstream"),
            NodeStatus::Computed
        ),
        "{run_record_2:?}"
    );
}
