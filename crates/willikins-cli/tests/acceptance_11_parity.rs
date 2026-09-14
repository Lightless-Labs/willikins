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
        let cli_json: serde_json::Value = serde_json::from_str(&cli.stdout).unwrap_or_else(|err| {
            panic!(
                "{name}: CLI validate --json did not parse: {err}\n{}",
                cli.stdout
            )
        });

        let response = butler()
            .validate(
                &DocumentSource::Name(WorkflowName::parse(name).unwrap()),
                principal(),
            )
            .unwrap_or_else(|err| panic!("{name}: Butler::validate: {err}"));
        assert!(response.ok, "{name}: {response:?}");

        // The CLI prints the bare warnings array on a clean check;
        // `ValidateResponse.warnings` is the same list. `response.warnings`
        // on its own would serialize as plain `CheckWarning` JSON
        // (`serialize_with` only fires through `ValidateResponse`'s own
        // derived `Serialize`), so the whole response is serialized here
        // and `warnings` is read back out of it -- see
        // `check_failure_parity`'s own comment below for why. Both
        // positive fixtures carry no warnings today, so this only proves
        // `[] == []`; a fixture that gains one would exercise the
        // `message` field too.
        let response_json = serde_json::to_value(&response).unwrap();
        assert_eq!(
            cli_json, response_json["warnings"],
            "{name}: validate parity"
        );
    }
}

// ---------------------------------------------------------------------
// describe
// ---------------------------------------------------------------------

#[test]
fn describe_parity_for_both_positive_fixtures() {
    for (name, inputs) in POSITIVE_FIXTURES {
        let path = workflow_path(name);
        let mut args = vec![
            "--json".to_string(),
            "describe".to_string(),
            path.to_str().unwrap().to_string(),
        ];
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
                &DocumentSource::Name(WorkflowName::parse(name).unwrap()),
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
        let mut args = vec![
            "--json".to_string(),
            "plan".to_string(),
            path.to_str().unwrap().to_string(),
        ];
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

// ---------------------------------------------------------------------
// Negative fixtures: the same failure, the same JSON, on both surfaces
// ---------------------------------------------------------------------

/// Acceptance test 11's second half ("every error the MCP tools return
/// has `kind` and `message`; the CLI's JSON for the same failures has the
/// same `kind`"), at the library level, for one failure from each of the
/// three stages a document can fail at.
///
/// Where the two envelopes differ this compares the *inner* error rather
/// than pretending they agree, and says so: that divergence is the same
/// plan defect this file's module doc already records for task 11.
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

/// Stage 1, parse. `newline-in-document-error.yaml` never becomes a
/// `Workflow` at all, so there is no `check` result to report: the CLI
/// prints the bare `DocumentError` (`{"kind": "Yaml", ...}`), while
/// `Butler::validate` wraps the identical error in
/// `ButlerError::Document`. The wrapper is the recorded divergence; the
/// error inside it is compared byte for byte.
///
/// The CLI writes this one to *stderr* (a document that will not parse is
/// a failure of the command) while it writes `check` errors to stdout
/// (they are the command's answer). Pinned here rather than changed:
/// which stream each failure class uses is task 11's call, not this
/// verification's.
#[test]
fn parse_failure_parity() {
    let name = "newline-in-document-error";
    let cli = run_cli(&["--json", "validate", fixture_path(name).to_str().unwrap()]);
    assert_ne!(cli.code, 0, "the fixture must fail: {}", cli.stdout);
    assert!(
        cli.stdout.trim().is_empty(),
        "a parse failure goes to stderr, not stdout: {}",
        cli.stdout
    );
    let cli_json: serde_json::Value = serde_json::from_str(cli.stderr.trim())
        .unwrap_or_else(|err| panic!("CLI validate --json did not parse: {err}\n{}", cli.stderr));
    assert_eq!(cli_json["kind"], "Yaml");

    let error = butler()
        .validate(&DocumentSource::Body(fixture_body(name)), principal())
        .expect_err("a document that does not parse is an error, not an `ok: false` result");
    let butler_json = serde_json::to_value(&error).unwrap();
    assert_eq!(butler_json["kind"], "Document", "the recorded divergence");
    assert_eq!(
        butler_json["error"], cli_json,
        "the wrapped error must be the CLI's, verbatim"
    );
    // Both surfaces carry a `kind` and a `message`, whatever the wrapper.
    let reported = serde_json::to_value(willikins_core::Reported::new(&error)).unwrap();
    assert!(reported["kind"].is_string() && reported["message"].is_string());
}

/// Stage 2, `check`. Two fixtures, each failing a different way: the CLI
/// prints its `Vec<CheckError>` through `willikins_core::Reported`, so
/// every element carries `kind` *and* `message`; `ValidateResponse.errors`
/// now carries every element through the same `Reported` wrapper on the
/// wire (task 10b, closing item 5 of
/// `todos/2026-09-12-error-json-uniformity-gaps.md` -- the plan's task 10a
/// addendum records the decision), so the two shapes are pinned equal
/// rather than equal-except-`message`.
#[test]
fn check_failure_parity() {
    for name in ["cycle", "unknown-tool"] {
        let cli = run_cli(&["--json", "validate", fixture_path(name).to_str().unwrap()]);
        assert_ne!(cli.code, 0, "{name} must fail: {}", cli.stderr);
        let cli_json: serde_json::Value = serde_json::from_str(cli.stdout.trim())
            .unwrap_or_else(|err| panic!("{name}: CLI validate --json did not parse: {err}"));

        let response = butler()
            .validate(&DocumentSource::Body(fixture_body(name)), principal())
            .unwrap_or_else(|err| panic!("{name}: a parseable document validates: {err}"));
        assert!(!response.ok, "{name}");
        // `response.errors` on its own serializes as plain `CheckError`
        // JSON (`serialize_with` only fires through the containing
        // struct's own derived `Serialize`), so the whole response is
        // serialized here and `errors` is read back out of it -- the same
        // path an MCP tool result or the CLI's own JSON output goes
        // through.
        let response_json = serde_json::to_value(&response).unwrap();
        let butler_errors = response_json["errors"]
            .as_array()
            .expect("errors is an array");

        let cli_errors = cli_json.as_array().expect("the CLI prints an array");
        assert_eq!(cli_errors.len(), butler_errors.len(), "{name}");
        for (cli_error, butler_error) in cli_errors.iter().zip(butler_errors) {
            assert!(
                cli_error["message"].is_string(),
                "{name}: the CLI's own errors carry a message"
            );
            assert!(
                butler_error["message"].is_string(),
                "{name}: `ValidateResponse.errors` now carries one too"
            );
            assert_eq!(cli_error, butler_error, "{name}: check-error parity");
        }
    }
}

/// Stage 3, `plan`'s own input resolution: the positive fixture with one
/// input missing and one rejected. The CLI prints the whole
/// `Description`; `Butler::plan` refuses with `ButlerError::Input`,
/// carrying the same two lists under the same two names.
#[test]
fn plan_input_failure_parity() {
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

    let mut partial = indexmap::IndexMap::default();
    partial.insert(
        willikins_core::InputName::parse("slug").unwrap(),
        willikins_core::describe::RawInput::Scalar("BAD_SLUG".to_string()),
    );
    let error = butler()
        .plan(WorkflowName::parse(name).unwrap(), &partial, principal())
        .expect_err("a rejected input refuses the plan");
    let butler_json = serde_json::to_value(&error).unwrap();
    assert_eq!(butler_json["kind"], "Input");
    assert_eq!(
        butler_json["errors"], cli_json["errors"],
        "rejected-input parity"
    );
    assert_eq!(
        butler_json["missing"], cli_json["missing"],
        "missing-input parity"
    );
    let reported = serde_json::to_value(willikins_core::Reported::new(&error)).unwrap();
    assert_eq!(reported["kind"], "Input");
    assert!(reported["message"].is_string());
}
