//! Milestone 2b, task K1, acceptance 11
//! (`docs/plans/2026-10-05-milestone-2b-composition.md`): the CLI links a
//! `uses:` step's sibling, in `<file>`'s own parent directory, before
//! `describe`/`plan`/`apply <file>` ever reaches `check` (decision (d8)).
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

/// `stdout` split into top-level JSON documents, mirroring
/// `tests/common/mod.rs`'s own `json_documents` (kept local: this file
/// does not otherwise need that module's fixture-specific constants).
fn json_documents(text: &str) -> Vec<serde_json::Value> {
    let mut docs = Vec::new();
    for next in serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>() {
        let Ok(doc) = next else { break };
        docs.push(doc);
    }
    docs
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
/// the organisation's four unbound defaulted inputs each become a flat
/// graph fixed input (`org/github_org`, `org/buildkite_org`,
/// `org/cluster`, `org/environments`; decision (d6)), none of which
/// `resolved` must ever show (R1/S1's hiding, here exercised for the CLI
/// surface).
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
    for fixed in [
        "org/github_org",
        "org/buildkite_org",
        "org/cluster",
        "org/environments",
    ] {
        assert!(
            !resolved.contains_key(fixed),
            "fixed input `{fixed}` leaked into describe's output: {json}"
        );
    }
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

/// Acceptance 11, second clause: `apply <file> --fake-state` succeeds --
/// which is only possible because `apply` copied the whole linked
/// closure (the root *and* `example-org.yaml`) into its private
/// temporary directory, so `Butler::start`'s own scan-and-link of that
/// directory finds the child right beside the root and succeeds exactly
/// as the real trusted directory would. Had only the root been copied
/// (the pre-2b behaviour), `Butler::start` would refuse at startup,
/// naming the unresolved `uses:` child.
#[test]
fn apply_with_fake_state_succeeds_by_copying_the_linked_closure() {
    let output = run(
        &[
            "--json",
            "apply",
            root().to_str().unwrap(),
            "--input",
            "slug=k1-apply-test",
            "--fake-state",
            state_fixture("buildkite-cluster.json").to_str().unwrap(),
            "--approve",
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
    let docs = json_documents(&stdout(&output));
    let run_record = docs
        .iter()
        .rev()
        .find(|doc| doc.get("run_id").is_some())
        .unwrap_or_else(|| panic!("no run record in: {}", stdout(&output)));
    assert_eq!(run_record["state"], "succeeded", "{run_record}");
    let nodes = run_record["nodes"].as_array().expect("a nodes array");
    assert!(
        nodes
            .iter()
            .any(|node| node["node"] == "org/buildkite_cluster"),
        "the linked child node is missing from the run, so the closure copy did not take effect: {run_record}"
    );
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

/// Acceptance 11, final clause: `plan --live`'s credential requirement is
/// computed over the *linked* graph, not the root's own unlinked node
/// list. This root's own `pipeline` node also calls a Buildkite tool
/// (`buildkite.pipeline.ensure`), so an unlinked scan would *still* end
/// up demanding `WILLIKINS_BUILDKITE_TOKEN` -- but it would name `pipeline`
/// as the node that needs it. The organisation's own `uses: example-org`
/// step is declared *before* `pipeline`, so in the linked graph its
/// expanded node `org/buildkite_cluster` sits earlier in node order
/// (decision, "Step order", SHARED VALUES) and is the one an `--live`
/// credential scan actually reports first. That identity -- not merely
/// *that* the refusal happened -- is the proof the scan walked the
/// linked graph: an unlinked scan cannot see `org/buildkite_cluster` at
/// all, since it exists only after linking renames and inlines it.
///
/// The GitHub and Doppler credentials are set (correctly shaped, so
/// each of those two checks, which run first, passes) so the scan
/// reaches the Buildkite check -- mirrors `serve_and_live.rs`'s own
/// `plan_live_on_a_document_leaving_buildkite_unbound_still_refuses_naming_the_variable`,
/// the unlinked, single-document precedent for this exact shape. No
/// network call is made either way (`live_catalog_for_document`'s own
/// doc): this fails while *building* the catalog, before `check`, before
/// any node's `read()`.
#[test]
fn plan_live_credential_requirement_is_computed_over_the_linked_graph() {
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
    // The linked path, not `pipeline` (this root's own Buildkite node,
    // which an unlinked scan would have found instead): proof the scan
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

/// The `apply <file> --live` twin of
/// `plan_live_credential_requirement_is_computed_over_the_linked_graph`:
/// `cmd_apply_file` builds its own `--live` catalog from the same
/// `link_workflow` result `cmd_plan` does, before it ever creates the
/// temporary directory or touches `Butler::start`, so the refusal is
/// identical in shape, kind, and exit code.
#[test]
fn apply_live_credential_requirement_is_computed_over_the_linked_graph() {
    let token = doppler_test_token();
    let output = run(
        &["--json", "apply", root().to_str().unwrap(), "--live"],
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

/// X1 (the adversarial pass), closing an explicit priority target: "a
/// caller setting a fixed input through the CLI ... describe". `describe`
/// already hides `org/cluster` from `resolved` (the test above); this
/// proves a caller cannot smuggle a value into it either, through the one
/// real surface the fixture exposes a fixed input on.
#[test]
fn describe_refuses_a_callers_value_for_a_fixed_input() {
    let output = run(
        &[
            "--json",
            "describe",
            root().to_str().unwrap(),
            "--input",
            "slug=k1-notsettable-describe",
            "--input",
            "org/cluster=hacked-cluster",
        ],
        &[],
    );
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_str(stdout(&output).trim()).expect("valid JSON on stdout");
    let errors = json["errors"].as_array().expect("an errors array");
    assert!(
        errors
            .iter()
            .any(|e| { e["input"] == "org/cluster" && e["error"]["type_name"] == "NotSettable" }),
        "expected a NotSettable error for `org/cluster`: {json}"
    );
}

/// The same priority target's `plan` half: `cmd_plan` shares the same
/// `describe` call `cmd_describe` does before it ever reaches
/// `willikins_core::plan`, so a fixed input is refused at the same point,
/// never silently accepted into a plan.
#[test]
fn plan_refuses_a_callers_value_for_a_fixed_input() {
    let output = run(
        &[
            "--json",
            "plan",
            root().to_str().unwrap(),
            "--input",
            "slug=k1-notsettable-plan",
            "--input",
            "org/buildkite_org=hacked-org",
        ],
        &[],
    );
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    let json: serde_json::Value =
        serde_json::from_str(stdout(&output).trim()).expect("valid JSON on stdout");
    let errors = json["errors"].as_array().expect("an errors array");
    assert!(
        errors.iter().any(|e| {
            e["input"] == "org/buildkite_org" && e["error"]["type_name"] == "NotSettable"
        }),
        "expected a NotSettable error for `org/buildkite_org`: {json}"
    );
}

// `apply --plan-id --workflows-dir` resolving children in that trusted
// directory "like the server" (decision (d8)'s last bullet) is not new
// behaviour this task adds: `Butler::start`'s own scan already links
// every document under a trusted directory (task S1), and `Butler::apply`'s
// reload already re-links through it (task S2) before this task existed.
// `crates/willikins-server/tests/it/composition_s2.rs` already exercises
// that machinery directly.
//
// A CLI-level test was tried and dropped for two separate reasons, not
// one: (1) `workflows/fixtures/composition/` cannot itself be pointed at
// as `--workflows-dir` -- `Butler::start` scans and checks *every*
// document in it, and that directory deliberately also holds documents
// that fail to link on purpose (`cycle-a.yaml` and siblings), so the
// scan refuses outright, naming the first cycle it finds, before this
// test's own document is ever reached. A clean directory holding only
// the root and `example-org.yaml` would dodge that. (2) This document's
// class is `Reversible` (no approval needed), so `apply <file>` plans
// *and* applies in the same call -- there is no CLI command that only
// records a plan, so a second, separate `apply --plan-id` call against
// the same id always meets `ButlerError::AlreadyApplied` first, before
// `--workflows-dir` is ever linked against. Reaching a genuinely pending
// plan through the CLI needs a document whose class demands approval
// (`workflows/fixtures/composition/uses-class-root.yaml` and its child
// are already shaped for that, acceptance 6), so a plan stays pending
// between the two invocations this would need.
