//! Task 10b's MCP surface, exercised through an in-process rmcp client
//! over a `tokio::io::duplex` pair -- no process is spawned, so this is
//! the same handler `serve_stdio` would run, wired to a duplex transport
//! instead of stdio. The CLI-comparing half of acceptance test 11
//! (validate/describe/plan parity, `list_tools` vs `schema --catalog`,
//! `propose_slug`, the three negative-fixture `kind` checks) lives in
//! `crates/willikins-cli/tests/acceptance_11_mcp_parity.rs` instead,
//! because `CARGO_BIN_EXE_willikins` is only set for a test that is part
//! of the package owning that binary target -- the same reason the
//! library-level parity test lives there (see that file's own module
//! doc). What stays here needs no CLI at all: the tool list and schema
//! snapshots, `apply`'s `ApprovalRequired` result on the irreversible
//! fixture, `run_status` on an unknown run, and the secret-never-leaks
//! sweep.

mod common;

use std::sync::{Arc, Mutex};

use rmcp::model::{CallToolRequestParams, ProtocolVersion};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};

use willikins_journal::{Clock, MemoryJournal, PrincipalId};
use willikins_server::{Butler, ButlerConfig, WillikinsHandler};
use willikins_types::DomainType;

fn principal() -> PrincipalId {
    PrincipalId::parse("agent").unwrap()
}

/// A `Butler` over a fresh in-memory journal, reading documents from
/// `dir`, sharing `clock`.
fn butler(
    dir: &std::path::Path,
    catalog: willikins_core::Catalog,
    clock: Arc<dyn Clock>,
) -> Butler {
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

/// Serve `butler` over an in-process duplex pair and connect a plain
/// rmcp client (`()` implements `ClientHandler`) to it -- the "in-process
/// rmcp client over a `tokio::io::duplex` pair" the plan's task 10b entry
/// asks for. `tokio::io::DuplexStream` implements both `AsyncRead` and
/// `AsyncWrite`, so each end serves directly with no split needed (rmcp's
/// own `IntoTransport` covers a single combined-read-write type).
async fn connect(butler: Arc<Butler>, principal: PrincipalId) -> RunningService<RoleClient, ()> {
    connect_handler(WillikinsHandler::new(butler, principal)).await
}

/// As [`connect`], but over an already-built [`WillikinsHandler`] -- for
/// tests that need a builder option (`with_fake_catalog_note`) `connect`
/// itself has no parameter for.
async fn connect_handler(handler: WillikinsHandler) -> RunningService<RoleClient, ()> {
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

/// Build a `tools/call` request for `name` with `arguments` (a JSON
/// object, or `null` for no arguments).
fn call(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
    let object = match arguments {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Null => serde_json::Map::new(),
        other => panic!("tool arguments must be a JSON object, got {other}"),
    };
    CallToolRequestParams::new(name).with_arguments(object)
}

// ---------------------------------------------------------------------
// get_info: the protocol version this server advertises
// ---------------------------------------------------------------------

/// Verify item 5, answered empirically rather than assumed: this
/// server's `get_info` advertises `ProtocolVersion::V_2026_07_28`
/// explicitly, as the plan asks, but the in-process rmcp 3.3.0 client
/// still negotiates `2025-11-25` over the classic `initialize` handshake
/// -- read from `rmcp::service::server::negotiate_protocol_version`'s own
/// doc and source (`crates/rmcp-3.3.0/src/service/server.rs`) plus
/// `ClientInfo`'s (`= InitializeRequestParams`) `Default` impl in
/// `model.rs`. The mechanism is the first branch, not a fallback: this
/// test's client is `()`, whose `ClientHandler::get_info` returns
/// `ClientInfo::default()` (the crate's own blanket impl), which sets
/// `protocol_version: ProtocolVersion::default()` -- `Self::LATEST`,
/// i.e. `V_2025_11_25`, a *legacy* (pre-SEP-2567) version -- so
/// `negotiate_protocol_version`'s own first check,
/// `is_legacy_version(client_requested) && server_supported.contains(client_requested)`,
/// is already true and the function returns `client_requested` verbatim.
/// The `server_fallback`/`newest_legacy_version` branch (what a server
/// falls back to when the client asked for something newer or
/// unsupported) is never reached in this handshake at all. Separately,
/// and for the same underlying reason: SEP-2567's `2026-07-28` replaced
/// the handshake with per-request metadata (an HTTP header,
/// `ProtocolVersion::STANDARD_HEADERS`), which has no representation in
/// the classic `initialize` response's `protocol_version` field, so a
/// server's own `get_info()` value could not be echoed back at
/// `2026-07-28` even if a client did request it there -- true for every
/// transport reachable over the classic handshake, stdio (this crate's
/// own) included; `2026-07-28` proper is only reachable over Streamable
/// HTTP's per-request header, a fact for the next step once that
/// transport exists. `get_info`'s explicit
/// `.with_protocol_version(V_2026_07_28)` is not wasted: it is still what
/// `Service::get_info` reports (and what a caller reading the source
/// would see it declare), even though this particular negotiation path
/// can't surface it.
#[tokio::test(flavor = "multi_thread")]
async fn the_client_negotiates_the_newest_legacy_protocol_version_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (_state, catalog) = Butler::fake_catalog();
    let clock: Arc<dyn Clock> = common::manual_clock();
    let b = Arc::new(butler(dir.path(), catalog, clock));

    let client = connect(b, principal()).await;
    let info = client
        .peer_info()
        .expect("the server's InitializeResult is recorded");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2025_11_25);
    assert!(
        info.instructions
            .as_deref()
            .is_some_and(|text| text.contains("trusted directory")),
        "{:?}",
        info.instructions
    );
}

/// `WillikinsHandler::with_fake_catalog_note` appends one sentence to
/// `get_info`'s `instructions`, and only when set -- the plan does not
/// say how a `--fake`-served instance should announce itself; this is
/// that choice (see `main.rs`'s module doc and `mcp.rs`'s
/// `with_fake_catalog_note` doc), pinned from both directions so a
/// caller reading `initialize`'s response can always tell which catalog
/// it is talking to.
#[tokio::test(flavor = "multi_thread")]
async fn the_fake_catalog_note_appears_in_instructions_only_when_set() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    let (_state, catalog) = Butler::fake_catalog();
    let clock: Arc<dyn Clock> = common::manual_clock();

    let plain = butler(dir.path(), catalog, clock.clone());
    let plain_client = connect_handler(WillikinsHandler::new(Arc::new(plain), principal())).await;
    let plain_info = plain_client
        .peer_info()
        .expect("the server's InitializeResult is recorded");
    assert!(
        !plain_info
            .instructions
            .as_deref()
            .is_some_and(|text| text.contains("fake in-memory catalog")),
        "a plain handler must not mention the fake catalog: {:?}",
        plain_info.instructions
    );

    let (_state, noted_catalog) = Butler::fake_catalog();
    let noted = butler(dir.path(), noted_catalog, clock);
    let noted_client = connect_handler(
        WillikinsHandler::new(Arc::new(noted), principal()).with_fake_catalog_note(),
    )
    .await;
    let noted_info = noted_client
        .peer_info()
        .expect("the server's InitializeResult is recorded");
    assert!(
        noted_info
            .instructions
            .as_deref()
            .is_some_and(|text| text.contains("fake in-memory catalog")),
        "a handler built with with_fake_catalog_note must say so: {:?}",
        noted_info.instructions
    );
}

// ---------------------------------------------------------------------
// Tool list and schemas: insta snapshots
// ---------------------------------------------------------------------

/// The eight tools this milestone defines, and each one's input and
/// output schema -- an insta snapshot, read and accepted deliberately
/// (never blanket-accepted) per this task's own instructions. Sorted by
/// name first: the router's own iteration order is an implementation
/// detail this snapshot should not depend on.
#[tokio::test(flavor = "multi_thread")]
async fn the_tool_list_and_every_schema_is_snapshotted() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, catalog) = Butler::fake_catalog();
    let clock: Arc<dyn Clock> = common::manual_clock();
    let b = Arc::new(butler(dir.path(), catalog, clock));

    let client = connect(b, principal()).await;
    let result = client.list_tools(None).await.expect("list_tools succeeds");

    let mut tools: Vec<_> = result.tools;
    tools.sort_by(|a, b| a.name.cmp(&b.name));

    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(
        names,
        [
            "apply",
            "describe",
            "list_tools",
            "list_workflows",
            "plan",
            "propose_slug",
            "run_status",
            "validate",
        ]
    );

    let snapshot: Vec<serde_json::Value> = tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
                "output_schema": tool.output_schema,
            })
        })
        .collect();
    insta::assert_json_snapshot!(snapshot);
}

// ---------------------------------------------------------------------
// apply on an irreversible fixture: ApprovalRequired, no CLI needed
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn apply_on_an_irreversible_plan_returns_approval_required() {
    let dir = tempfile::tempdir().unwrap();
    common::copy_irreversible(dir.path());
    let (_state, catalog) = Butler::fake_catalog();
    let clock: Arc<dyn Clock> = common::manual_clock();
    let b = Arc::new(butler(dir.path(), catalog, clock));

    let client = connect(b, principal()).await;

    let plan_result = client
        .call_tool(call(
            "plan",
            serde_json::json!({
                "workflow": common::IRREVERSIBLE_NAME,
                "inputs": {"slug": "third-thoughts", "org": "lightless-labs"},
            }),
        ))
        .await
        .expect("plan is routed");
    assert_ne!(plan_result.is_error, Some(true), "{plan_result:?}");
    let plan_json = plan_result
        .structured_content
        .expect("plan returns structured content");
    assert_eq!(plan_json["approval"], "pending", "{plan_json}");
    let plan_id = plan_json["plan_id"]
        .as_str()
        .expect("plan_id is a string")
        .to_string();

    let apply_result = client
        .call_tool(call("apply", serde_json::json!({ "plan_id": plan_id })))
        .await
        .expect("apply is routed");
    assert_eq!(apply_result.is_error, Some(true), "{apply_result:?}");
    let error_json = apply_result
        .structured_content
        .expect("a domain error carries structured content");
    assert_eq!(error_json["kind"], "ApprovalRequired", "{error_json}");
    assert!(error_json["message"].is_string(), "{error_json}");
}

// ---------------------------------------------------------------------
// run_status on an unknown run: a kind-tagged error
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn run_status_on_an_unknown_run_is_a_kind_tagged_error() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, catalog) = Butler::fake_catalog();
    let clock: Arc<dyn Clock> = common::manual_clock();
    let b = Arc::new(butler(dir.path(), catalog, clock));

    let client = connect(b, principal()).await;

    let unknown_run = willikins_journal::RunId::new().to_string();
    let result = client
        .call_tool(call(
            "run_status",
            serde_json::json!({ "run_id": unknown_run }),
        ))
        .await
        .expect("run_status is routed");
    assert_eq!(result.is_error, Some(true), "{result:?}");
    let error_json = result
        .structured_content
        .expect("a domain error carries structured content");
    assert_eq!(error_json["kind"], "UnknownRun", "{error_json}");
    assert!(error_json["message"].is_string(), "{error_json}");
}

// ---------------------------------------------------------------------
// No secret byte reaches any MCP response
// ---------------------------------------------------------------------

/// Seeds `next_token` and a stored Doppler secret with distinctive
/// markers, drives the positive fixture and `secret-get.yaml` through
/// `plan`, `apply`, and `run_status` over the MCP client, and greps every
/// JSON string the client received (every `structured_content` and every
/// `content` text block, not only the former) for the seeded bytes.
#[tokio::test(flavor = "multi_thread")]
async fn no_seeded_secret_byte_reaches_any_mcp_response() {
    const TOKEN_BODY: &str = "MCPSWEEPTOKENMARKERDOESNOTLEAKAAAAAAAAAAAAA";
    const SECRET_BYTES: &str = "mcp-sweep-stored-secret-bytes-do-not-leak";

    let seeded_token =
        willikins_types::DopplerServiceToken::parse(&format!("dp.st.prd.{TOKEN_BODY}")).unwrap();
    let config = willikins_types::DopplerConfig::parse("third-thoughts/prd").unwrap();
    let secret_name = willikins_types::SecretName::parse("DATABASE_URL").unwrap();

    let dir = tempfile::tempdir().unwrap();
    common::copy_fixture_as(dir.path(), "new-rust-service.yaml", "new-rust-service.yaml");
    common::copy_fixture_as(dir.path(), "secret-get.yaml", "secret-get.yaml");

    let state = Arc::new(Mutex::new(
        willikins_providers_fake::FakeState::new()
            .with_next_token(seeded_token)
            .with_doppler_secret(
                &config,
                &secret_name,
                willikins_types::DopplerSecretValue::parse(SECRET_BYTES).unwrap(),
            ),
    ));
    let catalog = willikins_providers_fake::catalog(Arc::clone(&state));
    let clock: Arc<dyn Clock> = common::manual_clock();
    let b = Arc::new(butler(dir.path(), catalog, clock));

    let client = connect(b, principal()).await;

    let mut swept = Vec::new();

    for (workflow, inputs) in [
        (
            "new-rust-service",
            serde_json::json!({"slug": "third-thoughts", "org": "lightless-labs"}),
        ),
        (
            "secret-get",
            serde_json::json!({"project": "third-thoughts"}),
        ),
    ] {
        let plan_result = client
            .call_tool(call(
                "plan",
                serde_json::json!({ "workflow": workflow, "inputs": inputs }),
            ))
            .await
            .unwrap_or_else(|err| panic!("{workflow}: plan is routed: {err}"));
        assert_ne!(plan_result.is_error, Some(true), "{plan_result:?}");
        let plan_json = plan_result
            .structured_content
            .clone()
            .expect("plan returns structured content");
        swept.push(serde_json::to_string(&plan_result).unwrap());
        let plan_id = plan_json["plan_id"].as_str().unwrap().to_string();

        let apply_result = client
            .call_tool(call("apply", serde_json::json!({ "plan_id": plan_id })))
            .await
            .unwrap_or_else(|err| panic!("{workflow}: apply is routed: {err}"));
        assert_ne!(apply_result.is_error, Some(true), "{apply_result:?}");
        swept.push(serde_json::to_string(&apply_result).unwrap());
        let run_id = apply_result.structured_content.unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Poll run_status until the run is no longer `running`.
        for _ in 0..200 {
            let status = client
                .call_tool(call("run_status", serde_json::json!({ "run_id": run_id })))
                .await
                .unwrap_or_else(|err| panic!("{workflow}: run_status is routed: {err}"));
            let json = status.structured_content.clone();
            swept.push(serde_json::to_string(&status).unwrap());
            if json.is_some_and(|j| j["state"] != "running") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    for marker in [TOKEN_BODY, SECRET_BYTES] {
        for (index, blob) in swept.iter().enumerate() {
            assert!(
                !blob.contains(marker),
                "response {index} carries the seeded marker `{marker}`: {blob}"
            );
        }
    }
    // Not vacuous: the redaction marker really is there where the secret
    // was, and the run really did write a secret.
    let joined = swept.join("\n");
    assert!(joined.contains("REDACTED"), "{joined}");
    assert!(
        !state.lock().unwrap().github_actions_secrets.is_empty(),
        "the run should have written at least one GitHub Actions secret"
    );
}
