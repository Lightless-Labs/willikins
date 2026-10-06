//! Milestone 2b, task F1, commit 2, acceptance 12
//! (`docs/plans/2026-10-05-milestone-2b-composition.md`): the public
//! equivalence between `workflows/fixtures/composition/
//! new-rust-service-in-org.yaml` (composed from its sibling
//! `example-org.yaml`, F1's commit 1, already landed by K1) and
//! `workflows/new-rust-service-buildkite.yaml` (the operator's real,
//! uncomposed process, milestone 3a's positive fixture) -- decision
//! (o3)'s convergence proof, at public scale: once the organisation-
//! level facts the buildkite document carries as its own inputs are
//! instead supplied by composition, the two documents must plan the
//! identical multiset of tool calls.
//!
//! The acceptance text's tuple is `(tool, instance, action, inputs,
//! outputs)`; this test also carries each node's own `name`, with the
//! in-org plan's leading `org/` segment stripped before comparing --
//! "with the `org/` prefix stripped" has no other referent in the
//! acceptance text, since the organisation's `buildkite_cluster` lookup
//! is the only node whose linked name (`org/buildkite_cluster`) differs
//! from its unprefixed twin in the buildkite document
//! (`buildkite_cluster`). Every other node (`names`, `repo`, `doppler`,
//! `configs[dev/stg/prd]`, `pipeline`) shares its bare name across both
//! documents already.

use std::path::PathBuf;
use std::process::{Command, Output};

/// Mirrors `tests/it/composition.rs`'s own helpers of the same names,
/// kept local on purpose (that file's own comment explains why: this
/// file does not otherwise need its fixture-specific constants).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn composition_fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("composition")
        .join(name)
}

fn workflow_fixture(name: &str) -> PathBuf {
    workspace_root().join("workflows").join(name)
}

fn state_fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("workflows")
        .join("fixtures")
        .join("state")
        .join(name)
}

fn in_org_root() -> PathBuf {
    composition_fixture("new-rust-service-in-org.yaml")
}

fn buildkite_root() -> PathBuf {
    workflow_fixture("new-rust-service-buildkite.yaml")
}

/// Every invocation below runs with `env_clear`, exactly like
/// `tests/it/composition.rs`'s own `run`: neither of this file's two
/// tests reaches `--live`, but the habit is the same -- no developer
/// sandbox credential leaks in by accident.
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
/// `tests/it/composition.rs`'s own `json_documents`.
fn json_documents(text: &str) -> Vec<serde_json::Value> {
    let mut docs = Vec::new();
    for next in serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>() {
        let Ok(doc) = next else { break };
        docs.push(doc);
    }
    docs
}

/// `willikins --json plan <path> --input <k=v>... --fake-state <state>`,
/// asserting success, and returning the parsed `Plan` JSON (`plan`
/// prints exactly one JSON document on success).
fn plan_json(
    path: &std::path::Path,
    inputs: &[(&str, &str)],
    fake_state: &std::path::Path,
) -> serde_json::Value {
    let mut args: Vec<String> = vec!["--json".into(), "plan".into(), path.display().to_string()];
    for (name, value) in inputs {
        args.push("--input".into());
        args.push(format!("{name}={value}"));
    }
    args.push("--fake-state".into());
    args.push(fake_state.display().to_string());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let output = run(&args, &[]);
    assert_eq!(
        exit_code(&output),
        0,
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    serde_json::from_str(stdout(&output).trim()).expect("valid JSON on stdout")
}

/// One `name|tool|instance|action|inputs|outputs` string per planned
/// node, `name`'s leading `org/` segment stripped when `strip_org` is
/// set. A string, not a `serde_json::Value` tuple: this workspace's
/// `serde_json` has no `preserve_order` feature, so every object here
/// serializes with its keys sorted -- two structurally equal JSON
/// values therefore always render to the identical string, which is all
/// a sorted-vector multiset comparison needs.
fn node_tuples(plan: &serde_json::Value, strip_org: bool) -> Vec<String> {
    plan["nodes"]
        .as_array()
        .expect("a nodes array")
        .iter()
        .map(|node| {
            let name = node["name"].as_str().expect("a node name");
            let name = if strip_org {
                name.strip_prefix("org/").unwrap_or(name)
            } else {
                name
            };
            format!(
                "{name}|{}|{}|{}|{}|{}",
                node["tool"], node["instance"], node["action"], node["inputs"], node["outputs"],
            )
        })
        .collect()
}

/// Acceptance 12, first clause: under one fake state, the plan of
/// `new-rust-service-in-org.yaml` (inputs `slug`, `visibility`) equals
/// the plan of `new-rust-service-buildkite.yaml` (inputs `slug`,
/// `org=Example-Org`, `buildkite_org=example-bk-org`, `visibility`) as a
/// multiset of `(tool, instance, action, inputs, outputs)`, with the
/// `org/` prefix stripped.
#[test]
fn the_in_org_and_buildkite_documents_plan_the_identical_multiset_of_tool_calls() {
    let state = state_fixture("buildkite-cluster.json");

    let in_org = plan_json(
        &in_org_root(),
        &[("slug", "f1-equivalence"), ("visibility", "private")],
        &state,
    );
    let buildkite = plan_json(
        &buildkite_root(),
        &[
            ("slug", "f1-equivalence"),
            ("org", "Example-Org"),
            ("buildkite_org", "example-bk-org"),
            ("visibility", "private"),
        ],
        &state,
    );

    let mut in_org_tuples = node_tuples(&in_org, true);
    let mut buildkite_tuples = node_tuples(&buildkite, false);
    in_org_tuples.sort();
    buildkite_tuples.sort();

    // Non-vacuous: `names`, `repo`, `doppler`, `configs` x3 (dev/stg/prd),
    // the Buildkite cluster lookup, `pipeline`.
    assert_eq!(
        in_org_tuples.len(),
        8,
        "expected 8 planned node instances: {in_org_tuples:#?}"
    );
    assert_eq!(in_org_tuples, buildkite_tuples);

    // The root-level outputs agree too, not only the node multiset: both
    // documents declare exactly `repo_url` and `pipeline_url`.
    assert_eq!(in_org["outputs"], buildkite["outputs"]);
}

/// Acceptance 12, second clause: `apply` with `--fake-state-out`, then
/// `plan` again, reads every non-pure node `NoOp` -- the convergence
/// claim (decision (o3)), proven here for the public pair the same way
/// O1's own private test is meant to prove it for the operator's split
/// (O1 lands after this task).
#[test]
fn applying_then_replanning_the_in_org_document_reads_every_non_pure_node_noop() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let ending_state = dir.path().join("ending-state.json");

    let apply_output = run(
        &[
            "--json",
            "apply",
            in_org_root().to_str().unwrap(),
            "--input",
            "slug=f1-replan",
            "--input",
            "visibility=private",
            "--fake-state",
            state_fixture("buildkite-cluster.json").to_str().unwrap(),
            "--fake-state-out",
            ending_state.to_str().unwrap(),
            "--approve",
        ],
        &[],
    );
    assert_eq!(
        exit_code(&apply_output),
        0,
        "stdout: {}\nstderr: {}",
        stdout(&apply_output),
        stderr(&apply_output)
    );
    let docs = json_documents(&stdout(&apply_output));
    let run_record = docs
        .iter()
        .rev()
        .find(|doc| doc.get("run_id").is_some())
        .unwrap_or_else(|| panic!("no run record in: {}", stdout(&apply_output)));
    assert_eq!(run_record["state"], "succeeded", "{run_record}");
    let applied_nodes = run_record["nodes"].as_array().expect("a nodes array");
    assert!(!applied_nodes.is_empty());

    let second_plan = plan_json(
        &in_org_root(),
        &[("slug", "f1-replan"), ("visibility", "private")],
        &ending_state,
    );
    let planned_nodes = second_plan["nodes"].as_array().expect("a nodes array");

    for applied in applied_nodes {
        let name = applied["node"].as_str().expect("a node name");
        let instance = applied
            .get("instance")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let status = applied["status"]["kind"]
            .as_str()
            .unwrap_or_else(|| panic!("no status.kind: {applied}"));
        // Every non-pure node (anything that was not merely `computed`,
        // the first apply's own `created`/`unchanged`) now reads `noop`:
        // the resource it created the first time around is still there,
        // matching what `inputs` describes. A pure node keeps computing.
        let expected_action = if status == "computed" {
            "compute"
        } else {
            "noop"
        };

        let replanned = planned_nodes
            .iter()
            .find(|node| {
                node["name"] == name
                    && node
                        .get("instance")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
                        == instance
            })
            .unwrap_or_else(|| panic!("{name} missing from the re-plan: {second_plan}"));
        assert_eq!(
            replanned["action"], expected_action,
            "{name} (status {status}) did not re-plan as expected: {replanned}"
        );
    }
}
