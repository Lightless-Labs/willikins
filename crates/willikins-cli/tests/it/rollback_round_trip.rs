//! Milestone 3m task R9, acceptance test 11: `willikins rollback` end to
//! end through the built binary, on the fake providers, over a
//! test-local document (never under `workflows/`, so the dsl
//! characterization and its own fixtures stay untouched) that says
//! `teardown: delete` on its one deletable node.
//!
//! `apply --fake-state S --fake-state-out A`, then `rollback --run R
//! --fake-state A`, then `approve`, then `rollback --plan-id P
//! --fake-state A --fake-state-out B`: `B` holds none of the run's
//! created resources and everything seeded in `S`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
        .env_clear()
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

/// Every top-level JSON value concatenated in `text`, in order -- the
/// same shape `apply --json` prints (a `PlanResponse` then a
/// `RunRecord`), reused here for `rollback --json`'s own single-document
/// outputs. See `apply_and_journal.rs`'s own copy of this helper.
fn json_documents(text: &str) -> Vec<serde_json::Value> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<serde_json::Value>()
        .map(|result| result.unwrap_or_else(|err| panic!("invalid JSON in {text:?}: {err}")))
        .collect()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "willikins-cli-rollback-round-trip-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A minimal document with exactly one `Delete`-capable node
/// (`doppler.project.ensure`, which the fake catalog needs no credential
/// for), `teardown: delete` written out (decision 3a's default is
/// `retain`, so this is what makes the node a candidate at all).
const DOCUMENT: &str = "\
name: rollback-r9-roundtrip
description: Milestone 3m task R9's own CLI round trip fixture (test-local; never checked into workflows/).
inputs:
  project: { type: DopplerProject }
steps:
  doppler:
    tool: doppler.project.ensure
    teardown: delete
    with:
      project: ${{ inputs.project }}
outputs:
  project: ${{ steps.doppler.project }}
";

/// Seeds one Doppler project this document's own run never touches, so
/// the round trip's final assertion ("everything seeded in `S`") has
/// something to tell apart from what the run itself created.
const SEED: &str = r#"{"doppler_projects": {"example-untouched-project": {"ours": true}}}"#;

#[test]
#[allow(clippy::too_many_lines)] // one coherent four-process round trip (acceptance test 11); splitting it would scatter the sequence across helpers that each need the same state
fn apply_then_rollback_deletes_exactly_what_the_run_created() {
    let dir = TempDir::new("basic");
    let document = dir.join("rollback-r9-roundtrip.yaml");
    std::fs::write(&document, DOCUMENT).unwrap();
    let seed = dir.join("seed.json");
    std::fs::write(&seed, SEED).unwrap();
    let journal = dir.join("journal.jsonl");
    let state_a = dir.join("state-a.json");
    let state_b = dir.join("state-b.json");

    // 1. `apply --fake-state S --fake-state-out A`.
    let applied = run(&[
        "--json",
        "apply",
        document.to_str().unwrap(),
        "--input",
        "project=example-rollback-r9",
        "--fake-state",
        seed.to_str().unwrap(),
        "--fake-state-out",
        state_a.to_str().unwrap(),
        "--journal",
        journal.to_str().unwrap(),
        "--approve",
    ]);
    assert_eq!(exit_code(&applied), 0, "stderr: {}", stderr(&applied));
    let applied_docs = json_documents(&stdout(&applied));
    let run_record = applied_docs
        .last()
        .unwrap_or_else(|| panic!("no JSON document in {applied_docs:?}"));
    assert_eq!(run_record["state"], "succeeded", "{run_record:?}");
    let run_id = run_record["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no run_id in {run_record:?}"))
        .to_string();

    let dump_a = std::fs::read_to_string(&state_a).expect("state A was dumped");
    assert!(
        dump_a.contains("example-rollback-r9"),
        "state A should hold the created project: {dump_a}"
    );

    // 2. `rollback --run R --fake-state A`: plans the teardown.
    let planned = run(&[
        "--json",
        "rollback",
        document.to_str().unwrap(),
        "--run",
        &run_id,
        "--fake-state",
        state_a.to_str().unwrap(),
        "--journal",
        journal.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&planned), 0, "stderr: {}", stderr(&planned));
    let plan_docs = json_documents(&stdout(&planned));
    let teardown_plan = plan_docs
        .first()
        .unwrap_or_else(|| panic!("no JSON document in {plan_docs:?}"));
    let plan_id = teardown_plan["plan_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no plan_id in {teardown_plan:?}"))
        .to_string();
    let steps = teardown_plan["teardown"]["steps"]
        .as_array()
        .unwrap_or_else(|| panic!("no steps in {teardown_plan:?}"));
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0]["action"]["kind"], "delete", "{steps:?}");
    // The production wiring (`butler.run(run_id)` -> matching the step's
    // own `InstanceRef` against the forward run's recorded nodes ->
    // attaching `resource`), not only the renderer the unit tests build a
    // `RunRecord` by hand for: this run's own `project` output must have
    // reached the printed plan.
    assert!(steps[0]["resource"]["project"].is_object(), "{steps:?}");

    // 3. `approve`.
    let approved = run(&["approve", &plan_id, "--journal", journal.to_str().unwrap()]);
    assert_eq!(exit_code(&approved), 0, "stderr: {}", stderr(&approved));

    // 4. `rollback --plan-id P --fake-state A --fake-state-out B`: applies it.
    let rolled_back = run(&[
        "--json",
        "rollback",
        "--plan-id",
        &plan_id,
        "--fake-state",
        state_a.to_str().unwrap(),
        "--fake-state-out",
        state_b.to_str().unwrap(),
        "--journal",
        journal.to_str().unwrap(),
        "--workflows-dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(
        exit_code(&rolled_back),
        0,
        "stderr: {}",
        stderr(&rolled_back)
    );
    let run_docs = json_documents(&stdout(&rolled_back));
    let teardown_run = run_docs
        .first()
        .unwrap_or_else(|| panic!("no JSON document in {run_docs:?}"));
    assert_eq!(teardown_run["succeeded"], true, "{teardown_run:?}");

    // `B` holds none of the run's created resources and everything
    // seeded in `S`.
    let dump_b: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state_b).expect("state B was dumped"))
            .expect("state B is valid JSON");
    let projects = dump_b["doppler_projects"]
        .as_object()
        .unwrap_or_else(|| panic!("no doppler_projects in {dump_b:?}"));
    assert!(
        !projects.contains_key("example-rollback-r9"),
        "state B should not hold the deleted project: {dump_b}"
    );
    assert!(
        projects.contains_key("example-untouched-project"),
        "state B should still hold the seeded, untouched project: {dump_b}"
    );
}
