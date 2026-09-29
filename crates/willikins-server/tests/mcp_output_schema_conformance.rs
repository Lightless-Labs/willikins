//! Every structured result this server returns conforms to the
//! `outputSchema` it publishes for that tool -- the MCP specification's
//! "servers MUST provide structured results that conform to this
//! schema", which a client SHOULD check (the TypeScript SDK's client
//! does, and refuses the result when it fails).
//!
//! Found by the 2026-09-29 adversarial pass over milestone 3e's gates
//! (`docs/research/2026-09-29-m3e-adversarial-pass-2.md`): `Plan.blocked`,
//! `Description.awaiting` and `RunRecord.blocked` are skipped when empty
//! (so a gate-free result is byte-identical to before decision (j)), but
//! rmcp's `schema_for_output` builds its schema for the deserialize
//! contract, which lists every field without a serde default as
//! `required`. So every gate-free `describe`, `plan` and `run_status`
//! result -- every document that existed before the gates -- failed its
//! own published schema.
//!
//! Both halves are exercised: a gate-free document
//! (`new-rust-service.yaml`) and a blocked one
//! (`fixtures/acknowledgement-gate-resume.yaml`, its acknowledgement left
//! unsupplied), through an in-process rmcp client, exactly as
//! `mcp_server.rs` drives the handler.

mod common;

use std::sync::{Arc, Mutex};

use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};

use willikins_journal::{Clock, MemoryJournal, PrincipalId};
use willikins_server::{Butler, ButlerConfig, WillikinsHandler};

fn butler(dir: &std::path::Path) -> Butler {
    let (_state, catalog) = Butler::fake_catalog();
    let clock: Arc<dyn Clock> = common::manual_clock();
    let journal = Arc::new(Mutex::new(MemoryJournal::with_clock(clock.clone())));
    Butler::new(ButlerConfig {
        workflows_dir: dir.to_path_buf(),
        journal,
        catalog,
        clock,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    })
}

async fn connect(butler: Arc<Butler>) -> RunningService<RoleClient, ()> {
    let handler = WillikinsHandler::new(butler, PrincipalId::parse("agent").unwrap());
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    tokio::spawn(async move {
        if let Ok(service) = handler.serve(server_io).await {
            let _ = service.waiting().await;
        }
    });
    ().serve(client_io)
        .await
        .expect("the client connects and initializes")
}

fn call(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
    let serde_json::Value::Object(object) = arguments else {
        panic!("tool arguments must be a JSON object");
    };
    CallToolRequestParams::new(name).with_arguments(object)
}

/// Call `tool` and assert its structured result validates against the
/// `outputSchema` the server itself published for it; return the result.
async fn call_conforming(
    client: &RunningService<RoleClient, ()>,
    schemas: &std::collections::HashMap<String, serde_json::Value>,
    tool: &'static str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let result = client
        .call_tool(call(tool, arguments))
        .await
        .unwrap_or_else(|err| panic!("{tool} is routed: {err}"));
    assert_ne!(result.is_error, Some(true), "{tool}: {result:?}");
    let structured = result
        .structured_content
        .unwrap_or_else(|| panic!("{tool} returns structured content"));
    let schema = schemas
        .get(tool)
        .unwrap_or_else(|| panic!("{tool} publishes an outputSchema"));
    let validator = jsonschema::validator_for(schema).expect("the published schema compiles");
    let errors: Vec<String> = validator
        .iter_errors(&structured)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "{tool}'s structured result does not conform to its own published outputSchema: \
         {errors:?}\nresult: {structured}"
    );
    structured
}

async fn run_to_end(
    client: &RunningService<RoleClient, ()>,
    schemas: &std::collections::HashMap<String, serde_json::Value>,
    plan_id: &str,
) -> serde_json::Value {
    let applied = call_conforming(
        client,
        schemas,
        "apply",
        serde_json::json!({ "plan_id": plan_id }),
    )
    .await;
    let run_id = applied["run_id"].as_str().expect("run_id").to_string();
    for _ in 0..400 {
        let status = call_conforming(
            client,
            schemas,
            "run_status",
            serde_json::json!({ "run_id": run_id }),
        )
        .await;
        if status["state"] != "running" {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("the run never finished");
}

#[tokio::test(flavor = "multi_thread")]
async fn gate_free_and_blocked_results_both_conform_to_their_published_output_schemas() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    common::copy_fixture_as(
        dir.path(),
        "acknowledgement-gate-resume.yaml",
        "acknowledgement-gate-resume.yaml",
    );
    let client = connect(Arc::new(butler(dir.path()))).await;

    let tools = client.list_tools(None).await.expect("list_tools succeeds");
    let schemas: std::collections::HashMap<String, serde_json::Value> = tools
        .tools
        .iter()
        .filter_map(|tool| {
            tool.output_schema.as_ref().map(|schema| {
                (
                    tool.name.to_string(),
                    serde_json::Value::Object((**schema).clone()),
                )
            })
        })
        .collect();
    for tool in ["describe", "plan", "apply", "run_status"] {
        assert!(
            schemas.contains_key(tool),
            "{tool} publishes no outputSchema"
        );
    }

    // Gate-free: no `awaiting`, no `blocked`, no `next_step` on the wire.
    let gate_free_inputs = serde_json::json!({"slug": "third-thoughts", "org": "lightless-labs"});
    let described = call_conforming(
        &client,
        &schemas,
        "describe",
        serde_json::json!({ "workflow": "new-rust-service", "inputs": gate_free_inputs }),
    )
    .await;
    assert!(described.get("awaiting").is_none(), "{described}");
    let planned = call_conforming(
        &client,
        &schemas,
        "plan",
        serde_json::json!({ "workflow": "new-rust-service", "inputs": gate_free_inputs }),
    )
    .await;
    assert!(planned["plan"].get("blocked").is_none(), "{planned}");
    let finished = run_to_end(
        &client,
        &schemas,
        planned["plan_id"].as_str().expect("plan_id"),
    )
    .await;
    assert_eq!(finished["state"], "succeeded", "{finished}");
    assert!(finished.get("blocked").is_none(), "{finished}");

    // Blocked: the acknowledgement left unsupplied.
    let described = call_conforming(
        &client,
        &schemas,
        "describe",
        serde_json::json!({ "workflow": "acknowledgement-gate-resume", "inputs": {} }),
    )
    .await;
    assert_eq!(described["awaiting"][0]["name"], "gate_done", "{described}");
    let planned = call_conforming(
        &client,
        &schemas,
        "plan",
        serde_json::json!({ "workflow": "acknowledgement-gate-resume", "inputs": {} }),
    )
    .await;
    assert_eq!(planned["plan"]["blocked"][0]["node"], "gate", "{planned}");
    let finished = run_to_end(
        &client,
        &schemas,
        planned["plan_id"].as_str().expect("plan_id"),
    )
    .await;
    assert_eq!(finished["state"], "blocked", "{finished}");
    assert_eq!(
        finished["blocked"][0]["awaiting_inputs"][0], "gate_done",
        "{finished}"
    );
    assert!(finished["next_step"].is_string(), "{finished}");
}
