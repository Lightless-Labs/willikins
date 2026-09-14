//! Acceptance test 11, MCP half: the same comparisons
//! `acceptance_11_parity.rs` makes against `willikins_server::Butler`
//! directly, made instead through an in-process rmcp client talking to
//! `willikins_server::WillikinsHandler` over a `tokio::io::duplex` pair
//! (no process spawned -- the same handler `serve_stdio` runs, wired to
//! a different transport). Lives in this crate, not `willikins-server`'s
//! own tests, for the same reason the library half does:
//! `CARGO_BIN_EXE_willikins` is only set for a test that is part of the
//! package owning the `willikins` binary target.
//!
//! Same recorded plan defect as the library half (see that file's module
//! doc): the CLI's JSON and this crate's own result types are not one
//! literal envelope, so this file compares at the field level too, not
//! by asserting the whole `structured_content` equals the CLI's raw
//! stdout.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};

use willikins_journal::{Clock, ManualClock, MemoryJournal, PrincipalId, Timestamp};
use willikins_server::{Butler, ButlerConfig, WillikinsHandler};

const POSITIVE_FIXTURES: [(&str, &[(&str, &str)]); 2] = [
    (
        "new-rust-service",
        &[("slug", "third-thoughts"), ("org", "lightless-labs")],
    ),
    (
        "rotate-service-token",
        &[
            ("project", "third-thoughts"),
            ("repo", "lightless-labs/third-thoughts"),
        ],
    ),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn workflows_dir() -> PathBuf {
    workspace_root().join("workflows")
}

fn workflow_path(name: &str) -> PathBuf {
    workflows_dir().join(format!("{name}.yaml"))
}

fn fixture_body(name: &str) -> String {
    std::fs::read_to_string(
        workflows_dir()
            .join("fixtures")
            .join(format!("{name}.yaml")),
    )
    .unwrap_or_else(|err| panic!("fixture `{name}` is readable: {err}"))
}

fn fixture_path(name: &str) -> PathBuf {
    workflows_dir()
        .join("fixtures")
        .join(format!("{name}.yaml"))
}

fn principal() -> PrincipalId {
    PrincipalId::parse("agent").unwrap()
}

/// A `Butler` over an empty fake catalog, matching the CLI's own default
/// (no `--fake-state`) state exactly -- the same construction
/// `acceptance_11_parity.rs::butler()` uses, so a plan's output is
/// directly comparable between the two test files too.
fn butler() -> Butler {
    let clock: Arc<ManualClock> = Arc::new(ManualClock::new(
        Timestamp::parse("2026-09-14T00:00:00+00:00").unwrap(),
    ));
    let journal = Arc::new(Mutex::new(MemoryJournal::with_clock(
        clock.clone() as Arc<dyn Clock>
    )));
    let (_state, catalog) = Butler::fake_catalog();
    Butler::new(ButlerConfig {
        workflows_dir: workflows_dir(),
        journal,
        catalog,
        clock: clock as Arc<dyn Clock>,
        approval_window: ButlerConfig::DEFAULT_APPROVAL_WINDOW,
        apply_window: ButlerConfig::DEFAULT_APPLY_WINDOW,
        plan_rate_per_minute: ButlerConfig::DEFAULT_PLAN_RATE_PER_MINUTE,
        read_rate_per_minute: ButlerConfig::DEFAULT_READ_RATE_PER_MINUTE,
    })
}

/// Serve a fresh `butler()` over an in-process duplex pair and connect a
/// plain rmcp client to it.
async fn connect() -> RunningService<RoleClient, ()> {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    let b = Arc::new(butler());
    tokio::spawn(async move {
        let handler = WillikinsHandler::new(b, principal());
        if let Ok(service) = handler.serve(server_io).await {
            let _ = service.waiting().await;
        }
    });
    ().serve(client_io)
        .await
        .expect("the client connects and initializes")
}

fn call(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
    let object = match arguments {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Null => serde_json::Map::new(),
        other => panic!("tool arguments must be a JSON object, got {other}"),
    };
    CallToolRequestParams::new(name).with_arguments(object)
}

fn inputs_object(inputs: &[(&str, &str)]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (name, value) in inputs {
        map.insert(
            (*name).to_string(),
            serde_json::Value::String((*value).to_string()),
        );
    }
    serde_json::Value::Object(map)
}

struct CliOutput {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run_cli(args: &[&str]) -> CliOutput {
    let output = Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
        .output()
        .expect("the willikins binary runs");
    CliOutput {
        stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
        code: output.status.code().unwrap_or(-1),
    }
}

// ---------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn validate_parity_for_both_positive_fixtures() {
    let client = connect().await;
    for (name, _inputs) in POSITIVE_FIXTURES {
        let path = workflow_path(name);
        let cli = run_cli(&["--json", "validate", path.to_str().unwrap()]);
        assert_eq!(cli.code, 0, "{name}: {}", cli.stderr);
        let cli_json: serde_json::Value = serde_json::from_str(&cli.stdout).unwrap_or_else(|err| {
            panic!(
                "{name}: CLI validate --json did not parse: {err}\n{}",
                cli.stdout
            )
        });

        let result = client
            .call_tool(call("validate", serde_json::json!({ "workflow": name })))
            .await
            .unwrap_or_else(|err| panic!("{name}: validate is routed: {err}"));
        assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
        let structured = result
            .structured_content
            .unwrap_or_else(|| panic!("{name}: validate returns structured content"));
        assert_eq!(structured["ok"], true, "{name}: {structured}");
        assert_eq!(cli_json, structured["warnings"], "{name}: validate parity");
    }
}

// ---------------------------------------------------------------------
// describe
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn describe_parity_for_both_positive_fixtures() {
    let client = connect().await;
    for (name, inputs) in POSITIVE_FIXTURES {
        let path = workflow_path(name);
        let mut args = vec![
            "--json".to_string(),
            "describe".to_string(),
            path.to_str().unwrap().to_string(),
        ];
        for (input_name, value) in inputs {
            args.push("--input".to_string());
            args.push(format!("{input_name}={value}"));
        }
        let cli = run_cli(&args.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(cli.code, 0, "{name}: {}", cli.stderr);
        let cli_json: serde_json::Value = serde_json::from_str(&cli.stdout)
            .unwrap_or_else(|err| panic!("{name}: CLI describe --json did not parse: {err}"));

        let result = client
            .call_tool(call(
                "describe",
                serde_json::json!({ "workflow": name, "inputs": inputs_object(inputs) }),
            ))
            .await
            .unwrap_or_else(|err| panic!("{name}: describe is routed: {err}"));
        assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
        let structured = result
            .structured_content
            .unwrap_or_else(|| panic!("{name}: describe returns structured content"));
        assert_eq!(cli_json, structured, "{name}: describe parity");
    }
}

// ---------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn plan_parity_for_both_positive_fixtures() {
    let client = connect().await;
    for (name, inputs) in POSITIVE_FIXTURES {
        let path = workflow_path(name);
        let mut args = vec![
            "--json".to_string(),
            "plan".to_string(),
            path.to_str().unwrap().to_string(),
        ];
        for (input_name, value) in inputs {
            args.push("--input".to_string());
            args.push(format!("{input_name}={value}"));
        }
        let cli = run_cli(&args.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(cli.code, 0, "{name}: {}", cli.stderr);
        let cli_json: serde_json::Value = serde_json::from_str(&cli.stdout)
            .unwrap_or_else(|err| panic!("{name}: CLI plan --json did not parse: {err}"));

        let result = client
            .call_tool(call(
                "plan",
                serde_json::json!({ "workflow": name, "inputs": inputs_object(inputs) }),
            ))
            .await
            .unwrap_or_else(|err| panic!("{name}: plan is routed: {err}"));
        assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
        let structured = result
            .structured_content
            .unwrap_or_else(|| panic!("{name}: plan returns structured content"));

        // The CLI prints a bare `Plan`; the tool's `plan` field is the
        // same plan -- `plan_id`/`expires_at` have no CLI counterpart.
        assert_eq!(cli_json, structured["plan"], "{name}: plan parity");
    }
}

// ---------------------------------------------------------------------
// list_tools / schema --catalog
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn list_tools_equals_schema_catalog() {
    let cli = run_cli(&["schema", "--catalog"]);
    assert_eq!(cli.code, 0, "{}", cli.stderr);
    let cli_json: serde_json::Value = serde_json::from_str(&cli.stdout).unwrap();

    let client = connect().await;
    let result = client
        .call_tool(call("list_tools", serde_json::Value::Null))
        .await
        .expect("list_tools is routed");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let structured = result
        .structured_content
        .expect("list_tools returns structured content");
    assert_eq!(cli_json, structured);
}

// ---------------------------------------------------------------------
// propose_slug
// ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn propose_slug_equals_the_cli() {
    let cli = run_cli(&["propose-slug", "Third Thoughts"]);
    assert_eq!(cli.code, 0, "{}", cli.stderr);

    let client = connect().await;
    let result = client
        .call_tool(call(
            "propose_slug",
            serde_json::json!({ "name": "Third Thoughts" }),
        ))
        .await
        .expect("propose_slug is routed");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let structured = result
        .structured_content
        .expect("propose_slug returns structured content");
    assert_eq!(
        structured["slug"].as_str().unwrap(),
        cli.stdout.trim(),
        "{structured}"
    );
}

// ---------------------------------------------------------------------
// Negative fixtures: every error carries kind and message
// ---------------------------------------------------------------------

/// Stage 1, parse. Mirrors `acceptance_11_parity.rs::parse_failure_parity`:
/// the CLI prints the bare `DocumentError` (`{"kind": "Yaml", ...}`) to
/// stderr, while `validate` here wraps the identical error in a
/// `ButlerError::Document` domain error (`is_error: true`,
/// `structured_content: {kind: "Document", error: {...}, message}`).
#[tokio::test(flavor = "multi_thread")]
async fn parse_failure_parity() {
    let name = "newline-in-document-error";
    let cli = run_cli(&["--json", "validate", fixture_path(name).to_str().unwrap()]);
    assert_ne!(cli.code, 0, "the fixture must fail: {}", cli.stdout);
    let cli_json: serde_json::Value = serde_json::from_str(cli.stderr.trim())
        .unwrap_or_else(|err| panic!("CLI validate --json did not parse: {err}\n{}", cli.stderr));
    assert_eq!(cli_json["kind"], "Yaml");

    let client = connect().await;
    let result = client
        .call_tool(call(
            "validate",
            serde_json::json!({ "document": fixture_body(name) }),
        ))
        .await
        .expect("validate is routed");
    assert_eq!(result.is_error, Some(true), "{result:?}");
    let structured = result
        .structured_content
        .expect("a domain error carries structured content");
    assert_eq!(structured["kind"], "Document", "the recorded divergence");
    assert!(structured["message"].is_string(), "{structured}");
    assert_eq!(
        structured["error"], cli_json,
        "the wrapped error must be the CLI's, verbatim"
    );
}

/// Stage 2, `check`. Mirrors `acceptance_11_parity.rs::check_failure_parity`:
/// a check failure is not a domain error here either -- `validate`
/// returns normally (`is_error` not `true`) with `ok: false` and every
/// element of `errors` carrying `kind` and `message`, exactly like the
/// CLI's own JSON.
#[tokio::test(flavor = "multi_thread")]
async fn check_failure_parity() {
    let client = connect().await;
    for name in ["cycle", "unknown-tool"] {
        let cli = run_cli(&["--json", "validate", fixture_path(name).to_str().unwrap()]);
        assert_ne!(cli.code, 0, "{name} must fail: {}", cli.stderr);
        let cli_json: serde_json::Value = serde_json::from_str(cli.stdout.trim())
            .unwrap_or_else(|err| panic!("{name}: CLI validate --json did not parse: {err}"));

        let result = client
            .call_tool(call(
                "validate",
                serde_json::json!({ "document": fixture_body(name) }),
            ))
            .await
            .unwrap_or_else(|err| panic!("{name}: validate is routed: {err}"));
        assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
        let structured = result
            .structured_content
            .unwrap_or_else(|| panic!("{name}: validate returns structured content"));
        assert_eq!(structured["ok"], false, "{name}: {structured}");

        let cli_errors = cli_json.as_array().expect("the CLI prints an array");
        let mcp_errors = structured["errors"].as_array().expect("errors is an array");
        assert_eq!(cli_errors.len(), mcp_errors.len(), "{name}");
        for (cli_error, mcp_error) in cli_errors.iter().zip(mcp_errors) {
            assert!(
                cli_error["message"].is_string(),
                "{name}: the CLI's own errors carry a message"
            );
            assert!(
                mcp_error["message"].is_string(),
                "{name}: the tool's errors carry one too"
            );
            assert_eq!(cli_error, mcp_error, "{name}: check-error parity");
        }
    }
}

/// Stage 3, `plan`'s own input resolution. Mirrors
/// `acceptance_11_parity.rs::plan_input_failure_parity`: `plan` refuses
/// with a `ButlerError::Input` domain error carrying the same `errors`
/// and `missing` lists the CLI's own `Description` does.
#[tokio::test(flavor = "multi_thread")]
async fn plan_input_failure_parity() {
    let name = "new-rust-service";
    let path = workflow_path(name);
    let cli = run_cli(&[
        "--json",
        "plan",
        path.to_str().unwrap(),
        "--input",
        "slug=BAD_SLUG",
    ]);
    assert_ne!(cli.code, 0, "the inputs are bad: {}", cli.stdout);
    let cli_json: serde_json::Value = serde_json::from_str(cli.stdout.trim())
        .unwrap_or_else(|err| panic!("CLI plan --json did not parse: {err}\n{}", cli.stdout));

    let client = connect().await;
    let result = client
        .call_tool(call(
            "plan",
            serde_json::json!({ "workflow": name, "inputs": {"slug": "BAD_SLUG"} }),
        ))
        .await
        .expect("plan is routed");
    assert_eq!(result.is_error, Some(true), "{result:?}");
    let structured = result
        .structured_content
        .expect("a domain error carries structured content");
    assert_eq!(structured["kind"], "Input");
    assert!(structured["message"].is_string(), "{structured}");
    assert_eq!(
        structured["errors"], cli_json["errors"],
        "rejected-input parity"
    );
    assert_eq!(
        structured["missing"], cli_json["missing"],
        "missing-input parity"
    );
}
