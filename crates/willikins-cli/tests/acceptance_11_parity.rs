//! Acceptance test 11, library half: for both positive fixtures,
//! `validate`, `describe`, and `plan` called through
//! `willikins_server::Butler` return JSON structurally equal to the CLI's
//! own `--json` output for the same arguments and (empty) fake state;
//! `list_tools` equals `schema --catalog`; `propose_slug` equals the
//! CLI's plain-text slug. The MCP half of the same test (an in-process
//! rmcp client instead of `Butler` directly) is task 10b's.
//!
//! `CARGO_BIN_EXE_willikins` is only set for a test that is itself part of
//! the package owning the `willikins` binary target -- which is exactly
//! why this test lives in `willikins-cli/tests/` (a dev-dependency on
//! `willikins-server`, never the other way round: no cycle) rather than
//! in `willikins-server`'s own test suite.
//!
//! **A plan defect, not fixed here** (recorded per
//! `todos/2026-09-12-error-json-uniformity-gaps.md`'s naming-questions
//! section, for task 11 to reconcile): the CLI's JSON shape and the
//! `Butler` result types are not literally the same envelope --
//! `validate --json` prints a bare array (warnings on success, errors on
//! failure), not `{ok, errors, warnings}`; `plan --json` prints a bare
//! `Plan`, not the whole `PlanResponse` (whose `plan_id`/`expires_at`
//! could not match the CLI's fileless run anyway); `propose-slug --json`
//! prints the slug as plain text, ignoring `--json` entirely. This test
//! therefore compares at the field level (`ValidateResponse.warnings`
//! against the CLI's bare array, `PlanResponse.plan` against the CLI's
//! bare `Plan`, `ProposeSlugResponse.slug` against the CLI's trimmed
//! stdout) rather than asserting the two envelopes are identical.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use willikins_journal::{Clock, ManualClock, MemoryJournal, PrincipalId, Timestamp};
use willikins_server::{Butler, ButlerConfig, DocumentSource};
use willikins_types::{DomainType, WorkflowName};

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

fn principal() -> PrincipalId {
    PrincipalId::parse("agent").unwrap()
}

/// A `Butler` over an empty fake catalog, matching the CLI's own default
/// (no `--fake-state`) state exactly, so `plan`'s output is directly
/// comparable.
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

fn input_args(inputs: &[(&str, &str)]) -> Vec<String> {
    inputs
        .iter()
        .flat_map(|(name, value)| ["--input".to_string(), format!("{name}={value}")])
        .collect()
}

// ---------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------

#[test]
fn validate_parity_for_both_positive_fixtures() {
    for (name, _inputs) in POSITIVE_FIXTURES {
        let path = workflow_path(name);
        let cli = run_cli(&["--json", "validate", path.to_str().unwrap()]);
        assert_eq!(cli.code, 0, "{name}: {}", cli.stderr);
        let cli_json: serde_json::Value =
            serde_json::from_str(&cli.stdout).unwrap_or_else(|err| {
                panic!("{name}: CLI validate --json did not parse: {err}\n{}", cli.stdout)
            });

        let response = butler()
            .validate(
                DocumentSource::Name(WorkflowName::parse(name).unwrap()),
                principal(),
            )
            .unwrap_or_else(|err| panic!("{name}: Butler::validate: {err}"));
        assert!(response.ok, "{name}: {response:?}");

        // The CLI prints the bare warnings array on a clean check;
        // `ValidateResponse.warnings` is the same list.
        let butler_json = serde_json::to_value(&response.warnings).unwrap();
        assert_eq!(cli_json, butler_json, "{name}: validate parity");
    }
}

// ---------------------------------------------------------------------
// describe
// ---------------------------------------------------------------------

#[test]
fn describe_parity_for_both_positive_fixtures() {
    for (name, inputs) in POSITIVE_FIXTURES {
        let path = workflow_path(name);
        let mut args = vec!["--json".to_string(), "describe".to_string(), path.to_str().unwrap().to_string()];
        args.extend(input_args(inputs));
        let cli = run_cli(&args.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(cli.code, 0, "{name}: {}", cli.stderr);
        let cli_json: serde_json::Value = serde_json::from_str(&cli.stdout)
            .unwrap_or_else(|err| panic!("{name}: CLI describe --json did not parse: {err}"));

        let mut partial = indexmap::IndexMap::default();
        for (input_name, value) in inputs {
            partial.insert(
                willikins_core::InputName::parse(input_name).unwrap(),
                willikins_core::describe::RawInput::Scalar((*value).to_string()),
            );
        }
        let description = butler()
            .describe(
                DocumentSource::Name(WorkflowName::parse(name).unwrap()),
                &partial,
                principal(),
            )
            .unwrap_or_else(|err| panic!("{name}: Butler::describe: {err}"));

        let butler_json = serde_json::to_value(&description).unwrap();
        assert_eq!(cli_json, butler_json, "{name}: describe parity");
    }
}

// ---------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------

#[test]
fn plan_parity_for_both_positive_fixtures() {
    for (name, inputs) in POSITIVE_FIXTURES {
        let path = workflow_path(name);
        let mut args = vec!["--json".to_string(), "plan".to_string(), path.to_str().unwrap().to_string()];
        args.extend(input_args(inputs));
        let cli = run_cli(&args.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(cli.code, 0, "{name}: {}", cli.stderr);
        let cli_json: serde_json::Value = serde_json::from_str(&cli.stdout)
            .unwrap_or_else(|err| panic!("{name}: CLI plan --json did not parse: {err}"));

        let mut partial = indexmap::IndexMap::default();
        for (input_name, value) in inputs {
            partial.insert(
                willikins_core::InputName::parse(input_name).unwrap(),
                willikins_core::describe::RawInput::Scalar((*value).to_string()),
            );
        }
        let response = butler()
            .plan(WorkflowName::parse(name).unwrap(), &partial, principal())
            .unwrap_or_else(|err| panic!("{name}: Butler::plan: {err}"));

        // The CLI prints a bare `Plan`; `PlanResponse.plan` is the same
        // plan -- `plan_id` and `expires_at` have no CLI counterpart to
        // compare against (the CLI never records a journaled plan at
        // all), so only the `plan` field is compared.
        let butler_json = serde_json::to_value(&response.plan).unwrap();
        assert_eq!(cli_json, butler_json, "{name}: plan parity");
    }
}

// ---------------------------------------------------------------------
// list_tools / schema --catalog
// ---------------------------------------------------------------------

#[test]
fn list_tools_equals_schema_catalog() {
    let cli = run_cli(&["schema", "--catalog"]);
    assert_eq!(cli.code, 0, "{}", cli.stderr);
    let cli_json: serde_json::Value = serde_json::from_str(&cli.stdout).unwrap();

    let butler_json = butler().list_tools(principal());
    assert_eq!(cli_json, butler_json);
}

// ---------------------------------------------------------------------
// propose_slug
// ---------------------------------------------------------------------

#[test]
fn propose_slug_equals_the_cli() {
    let cli = run_cli(&["propose-slug", "Third Thoughts"]);
    assert_eq!(cli.code, 0, "{}", cli.stderr);

    let response = butler()
        .propose_slug("Third Thoughts", principal())
        .unwrap();
    assert_eq!(response.slug.to_string(), cli.stdout.trim());
}
