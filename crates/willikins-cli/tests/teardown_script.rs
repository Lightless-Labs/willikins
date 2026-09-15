//! Task 12, step E: `deploy/teardown.sh` is a shell script, not Rust, so
//! it cannot be unit-tested from inside a crate directly. This test is
//! the thin spawner the workspace gate uses to run its own bats-free
//! test suite, `deploy/teardown_test.sh` -- every actual scenario (both
//! ownership markers present, either one missing, `--yes` vs. dry run,
//! the Doppler token never reaching `curl`'s argv, a run record with no
//! `doppler` output) lives there, stubbing `willikins`, `gh`, and
//! `curl` on `PATH` so nothing here ever makes a real network call.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn teardown_sh_passes_its_own_scenario_suite() {
    let script = repo_root().join("deploy").join("teardown_test.sh");
    assert!(script.is_file(), "missing {}", script.display());

    let output = Command::new("bash")
        .arg(&script)
        .output()
        .expect("failed to run deploy/teardown_test.sh");

    assert!(
        output.status.success(),
        "deploy/teardown_test.sh failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------
// Task 12 verification: `deploy/teardown_test.sh` writes its own run
// records by hand, so its scenarios prove the script's logic but not
// that the jq paths it reads (`.nodes[] | select(.node == "repo") |
// .outputs.repo.value`) match what `willikins run --json` actually
// prints. This test closes that circle: it applies the positive fixture
// with the real binary, feeds the real `run --json` document to the real
// script, and compares what the script names with what the document
// holds.
// ---------------------------------------------------------------------

fn write_executable(path: &std::path::Path, body: &str) {
    std::fs::write(path, body).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Apply the positive fixture against the fake catalog, recording into
/// `journal`, and return the run id it produced.
#[cfg(unix)]
fn apply_the_positive_fixture(journal: &std::path::Path) -> String {
    let applied = Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args([
            "--json",
            "apply",
            repo_root()
                .join("workflows")
                .join("new-rust-service.yaml")
                .to_str()
                .unwrap(),
            "--input",
            "slug=third-thoughts",
            "--input",
            "org=lightless-labs",
            "--journal",
            journal.to_str().unwrap(),
        ])
        .env_clear()
        .output()
        .expect("failed to run willikins apply");
    assert!(
        applied.status.success(),
        "apply failed: {}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let text = String::from_utf8_lossy(&applied.stdout).into_owned();
    serde_json::Deserializer::from_str(&text)
        .into_iter::<serde_json::Value>()
        .filter_map(Result::ok)
        .find_map(|doc| doc["run_id"].as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("no run_id in apply's output: {text}"))
}

/// The document `deploy/teardown.sh` actually parses, printed by the
/// real binary.
#[cfg(unix)]
fn run_record_json(run_id: &str, journal: &std::path::Path) -> String {
    let record = Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args([
            "--json",
            "run",
            run_id,
            "--journal",
            journal.to_str().unwrap(),
        ])
        .env_clear()
        .output()
        .expect("failed to run willikins run");
    assert!(record.status.success(), "run failed for a succeeded run");
    String::from_utf8_lossy(&record.stdout).into_owned()
}

/// One node output, found by an independent path walk: if the script's
/// own jq expressions name the same strings, they agree with the real
/// document's shape.
#[cfg(unix)]
fn node_output(record: &serde_json::Value, node: &str, port: &str) -> String {
    record["nodes"]
        .as_array()
        .expect("nodes is an array")
        .iter()
        .find(|entry| entry["node"] == node)
        .unwrap_or_else(|| panic!("no `{node}` node in {record}"))["outputs"][port]["value"]
        .as_str()
        .unwrap_or_else(|| panic!("no `{port}` output on `{node}` in {record}"))
        .to_string()
}

/// Stubs for the three commands the script calls, so it makes no network
/// call and touches no real resource. `willikins` here replays the
/// document the real binary printed.
#[cfg(unix)]
fn write_stubs(bin: &std::path::Path, record_path: &std::path::Path) {
    std::fs::create_dir_all(bin).unwrap();
    write_executable(
        &bin.join("willikins"),
        &format!(
            "#!/usr/bin/env bash\nset -euo pipefail\ncat {}\n",
            record_path.display()
        ),
    );
    write_executable(
        &bin.join("gh"),
        "#!/usr/bin/env bash\nset -euo pipefail\necho '{\"topics\": [\"managed-by-willikins\"]}'\n",
    );
    write_executable(
        &bin.join("curl"),
        "#!/usr/bin/env bash\nset -euo pipefail\ncat > /dev/null\n\
         echo '{\"project\": {\"description\": \"managed-by: willikins\"}}'\n",
    );
}

#[test]
#[cfg(unix)]
fn teardown_reads_the_document_the_real_willikins_run_prints() {
    let temp = tempfile::tempdir().unwrap();
    let journal = temp.path().join("journal.jsonl");

    let run_id = apply_the_positive_fixture(&journal);
    let record_text = run_record_json(&run_id, &journal);
    let record: serde_json::Value =
        serde_json::from_str(&record_text).expect("run --json prints one JSON document");
    let repo = node_output(&record, "repo", "repo");
    let project = node_output(&record, "doppler", "project");

    let record_path = temp.path().join("run.json");
    std::fs::write(&record_path, &record_text).unwrap();
    let bin = temp.path().join("bin");
    write_stubs(&bin, &record_path);

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("bash")
        .arg(repo_root().join("deploy").join("teardown.sh"))
        .arg(&run_id)
        .arg(journal.to_str().unwrap())
        .env_clear()
        .env("PATH", path)
        .env("HOME", temp.path())
        .env("WILLIKINS_DOPPLER_TOKEN", "dp.sa.teardown-shape-test-token")
        .output()
        .expect("failed to run deploy/teardown.sh");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "teardown.sh refused: {text}");
    assert!(
        text.contains(&format!("would delete GitHub repository: {repo}")),
        "the script did not name the repository the run recorded ({repo}): {text}"
    );
    assert!(
        text.contains(&format!("would delete Doppler project:   {project}")),
        "the script did not name the project the run recorded ({project}): {text}"
    );
}
