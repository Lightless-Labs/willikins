//! Milestone 2b, task K1, acceptance 11
//! (`docs/plans/2026-10-05-milestone-2b-composition.md`): the CLI links a
//! `uses:` step's sibling, in `<file>`'s own parent directory, before
//! `describe`/`plan`/`apply <file>` ever reaches `check` (decision (d8)).
//!
//! This commit covers `describe` and `plan`'s own linking (`main.rs`);
//! `apply <file>`'s closure copy (`commands.rs`) is this task's second
//! commit and gets its own test there.
//!
//! `workflows/fixtures/composition/new-rust-service-in-org.yaml` and its
//! sibling `example-org.yaml` are this test's own fixtures (headers name
//! this file as their acceptance test): the organisation document holds
//! the Buildkite org/cluster lookup, so the only Buildkite node in the
//! whole linked graph lives *inside* the used document -- the shape
//! every test below needs to prove linking actually happened, rather
//! than merely that the root parsed.

use std::path::PathBuf;
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("composition")
        .join(name)
}

fn state_fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join(name)
}

fn root() -> PathBuf {
    fixture("new-rust-service-in-org.yaml")
}

/// Every invocation below runs with `env_clear`, exactly like
/// `tests/it/serve_and_live.rs`: none of these reads a developer's own
/// sandbox credential, and the live-credential test's negative result
/// (a missing variable) is never an accident of what happened to be in
/// the shell.
fn run(args: &[&str], vars: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_willikins"));
    command.args(args).env_clear();
    for (name, value) in vars {
        command.env(name, value);
    }
    command
        .output()
        .expect("failed to run the willikins binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("process was not signalled")
}

/// A Doppler service-account token, correctly shaped
/// (`willikins_providers_doppler::credential_from_env`'s own check), for
/// the one test below that needs the live credential scan to pass the
/// Doppler check so it reaches the Buildkite one. Assembled from two
/// literals, like `serve_and_live.rs`'s own `doppler_test_token`: a
/// contiguous `dp.sa.`-prefixed run of 40+ alphanumeric characters is
/// exactly the shape `secret_literal_guard.rs` flags tree-wide.
fn doppler_test_token() -> String {
    concat!("dp.sa.", "OhbVrpoiVgRV5IfLBcbfnoGMbJmTPSIAoCLrZ3aWZk").to_string()
}

/// Acceptance 11, first clause: `describe` links `new-rust-service-in-org
/// .yaml`'s sibling `example-org.yaml` before `check` ever sees the
/// document (a non-empty `uses:` reaching `check` unlinked is
/// `CheckError::Unlinked`, which would fail this with no `resolved` key
/// at all) -- and K1's own pin, "describe's output omits fixed inputs":
/// the organisation's two unbound defaulted inputs become the flat
/// graph's fixed inputs `org/buildkite_org` and `org/cluster_name`
/// (decision (d6)), which `resolved` must never show (R1/S1's hiding,
/// here exercised for the CLI surface).
#[test]
fn describe_links_the_sibling_and_hides_its_fixed_inputs() {
    let output = run(
        &[
            "--json",
            "describe",
            root().to_str().unwrap(),
            "--input",
            "slug=k1-describe-test",
        ],
        &[],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let json: serde_json::Value =
        serde_json::from_str(stdout(&output).trim()).expect("valid JSON on stdout");
    assert_eq!(json["errors"].as_array().map(Vec::len), Some(0), "{json}");
    assert_eq!(json["missing"].as_array().map(Vec::len), Some(0), "{json}");
    let resolved = json["resolved"].as_object().expect("a resolved object");
    assert!(resolved.contains_key("slug"), "{json}");
    assert!(
        !resolved.contains_key("org/buildkite_org"),
        "a fixed input leaked into describe's output: {json}"
    );
    assert!(
        !resolved.contains_key("org/cluster_name"),
        "a fixed input leaked into describe's output: {json}"
    );
}

/// Acceptance 11, first clause (the `plan` half): the linked graph's
/// organisation node appears under its full linked path,
/// `org/buildkite_cluster` -- a name that exists only *after* linking,
/// so its presence in the plan is itself the proof that `plan <file>`
/// linked the sibling rather than planning the root's own unlinked node
/// list.
#[test]
fn plan_links_the_sibling_and_shows_the_flattened_child_node() {
    let output = run(
        &[
            "--json",
            "plan",
            root().to_str().unwrap(),
            "--input",
            "slug=k1-plan-test",
            "--fake-state",
            state_fixture("buildkite-cluster.json").to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let json: serde_json::Value =
        serde_json::from_str(stdout(&output).trim()).expect("valid JSON on stdout");
    let nodes = json["nodes"].as_array().expect("a nodes array");
    assert!(
        nodes
            .iter()
            .any(|node| node["name"] == "org/buildkite_cluster"),
        "the linked child node is missing from the plan: {json}"
    );
    assert_eq!(json["outputs"]["pipeline_url"]["state"], "known", "{json}");
}

/// Acceptance 11, third clause: a composite whose only `uses:` child
/// lives in another directory (here, nowhere beside a freshly built
/// root in a fresh temporary directory, rather than in
/// `workflows/fixtures/composition/`) fails to link with exactly
/// `UnknownWorkflow` -- trust boundary 1, "a used document is... found
/// only by name in the trusted source", which for CLI file mode is
/// `<file>`'s own parent directory (decision (d8)), never any other
/// directory the same name happens to resolve in elsewhere.
#[test]
fn a_child_that_lives_only_in_another_directory_fails_with_unknown_workflow() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root_path = dir.path().join("standalone-root.yaml");
    std::fs::write(
        &root_path,
        "name: standalone-root\nsteps:\n  org:\n    uses: example-org\n",
    )
    .expect("failed to write the standalone root fixture");

    let output = run(&["--json", "describe", root_path.to_str().unwrap()], &[]);
    assert_eq!(
        exit_code(&output),
        1,
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let json: serde_json::Value =
        serde_json::from_str(stdout(&output).trim()).expect("valid JSON on stdout");
    let errors = json.as_array().expect("a CheckError array");
    assert_eq!(errors.len(), 1, "{json}");
    assert_eq!(errors[0]["kind"], "UnknownWorkflow", "{json}");
    assert_eq!(errors[0]["node"], "org", "{json}");
    assert_eq!(errors[0]["workflow"], "example-org", "{json}");
}

/// Acceptance 11, final clause: `apply --live`'s (and, identically,
/// `plan --live`'s) credential requirement is computed over the
/// *linked* graph. `new-rust-service-in-org.yaml`'s own, unlinked node
/// list calls no Buildkite tool at all -- the pipeline node's `org` and
/// `cluster` bindings both come from `steps.org`, and the only Buildkite
/// nodes (`buildkite.cluster.get`, called by `buildkite.pipeline.ensure`'s
/// own cluster id) live inside `example-org.yaml`. The GitHub and Doppler
/// credentials are set (correctly shaped, so each of those two checks,
/// which run first, passes) so the scan reaches the Buildkite check, the
/// one `--live`'s catalog construction must reach *through the linked
/// graph* to find at all -- mirrors
/// `serve_and_live.rs`'s own `plan_live_on_a_document_leaving_buildkite_unbound_still_refuses_naming_the_variable`,
/// the unlinked, single-document precedent for this exact shape. No
/// network call is made either way (`live_catalog_for_document`'s own
/// doc): this fails while *building* the catalog, before `check`, before
/// any node's `read()`.
#[test]
fn live_credential_requirement_is_computed_over_the_linked_graph() {
    let token = doppler_test_token();
    let output = run(
        &["--json", "plan", root().to_str().unwrap(), "--live"],
        &[
            ("WILLIKINS_DOPPLER_TOKEN", token.as_str()),
            ("WILLIKINS_GITHUB_TOKEN", "ghp_example"),
        ],
    );
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    assert!(
        stdout(&output).is_empty(),
        "nothing reached check: {}",
        stdout(&output)
    );
    let json: serde_json::Value =
        serde_json::from_str(stderr(&output).trim()).expect("valid JSON on stderr");
    assert_eq!(json["kind"], "Buildkite", "{json}");
    assert_eq!(json["document"], "new-rust-service-in-org", "{json}");
    // The linked path, not a bare `buildkite_cluster`: proof the scan
    // walked the *linked* graph, where this node only exists under its
    // full `<uses step>/<child node>` name (decision (d3)).
    assert_eq!(json["node"], "org/buildkite_cluster", "{json}");
    assert_eq!(json["tool"], "buildkite.cluster.get", "{json}");
    assert!(
        json["error"]
            .as_str()
            .is_some_and(|error| error.contains("WILLIKINS_BUILDKITE_TOKEN")),
        "{json}"
    );
}

/// `render.rs` prints a linked node's path unchanged, in the plan's text
/// output too, not only in JSON: the organisation node's full
/// `org/buildkite_cluster` name appears verbatim.
#[test]
fn text_output_shows_the_linked_node_path_unchanged() {
    let output = run(
        &[
            "plan",
            root().to_str().unwrap(),
            "--input",
            "slug=k1-text-test",
            "--fake-state",
            state_fixture("buildkite-cluster.json").to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(
        stdout(&output).contains("org/buildkite_cluster"),
        "stdout: {}",
        stdout(&output)
    );
}

// `apply --plan-id --workflows-dir` resolving children in that trusted
// directory "like the server" (decision (d8)'s last bullet) is not new
// behaviour this task adds: `Butler::start`'s own scan already links
// every document under a trusted directory (task S1), and `Butler::apply`'s
// reload already re-links through it (task S2) before this task existed.
// `crates/willikins-server/tests/it/composition_s2.rs` already exercises
// that machinery directly; nothing here would add coverage beyond
// re-proving S1/S2 through one more entry point, at the cost of needing a
// document whose class demands approval (so a plan stays pending between
// two separate CLI invocations) purely to exercise it through the CLI.
