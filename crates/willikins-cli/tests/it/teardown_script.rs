//! Task 12, step E: `deploy/teardown.sh` is a shell script, not Rust, so
//! it cannot be unit-tested from inside a crate directly. This test is
//! the thin spawner the workspace gate uses to run its own bats-free
//! test suite, `deploy/teardown_test.sh` -- every actual scenario (both
//! ownership markers present, either one missing, `--yes` vs. dry run,
//! neither token ever reaching `curl`'s argv *or* its environment,
//! either credential unset, a failing read or delete from either
//! provider, a run record with no `doppler` output, the Buildkite arm's
//! own dry run/`--yes`/foreign/failed-read/missing-token/unreached-node/
//! malformed-url scenarios) lives there, stubbing `willikins` and `curl`
//! on `PATH` so nothing here ever makes a real network call.

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
// holds. A second test below closes the same circle for the Buildkite
// arm, against `workflows/new-rust-service-buildkite.yaml` instead --
// `deploy/teardown_test.sh`'s own hand-written pipeline-bearing records
// prove the arm's logic, but not that `.outputs.slug.value` and
// `.outputs.url.value` on the real `pipeline` node are shaped the way
// this test's own hand-written fixtures assumed.
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

/// Apply milestone 3a's positive fixture -- the one document whose plan
/// carries a `pipeline` node -- against the fake catalog, seeded with
/// the one Buildkite cluster it looks up
/// (`workflows/fixtures/state/buildkite-cluster.json`), recording into
/// `journal`, and return the run id it produced. The `buildkite_org`
/// value mirrors `acceptance_m3a_buildkite.rs`'s own `positive_inputs`.
#[cfg(unix)]
fn apply_the_positive_buildkite_fixture(journal: &std::path::Path) -> String {
    let applied = Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args([
            "--json",
            "apply",
            repo_root()
                .join("workflows")
                .join("new-rust-service-buildkite.yaml")
                .to_str()
                .unwrap(),
            "--input",
            "slug=third-thoughts",
            "--input",
            "org=lightless-labs",
            "--input",
            "buildkite_org=willikins-test",
            "--fake-state",
            repo_root()
                .join("workflows")
                .join("fixtures")
                .join("state")
                .join("buildkite-cluster.json")
                .to_str()
                .unwrap(),
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

/// Stubs for the two commands the script calls, so it makes no network
/// call and touches no real resource. `willikins` here replays the
/// document the real binary printed; `curl` answers every provider's
/// ownership read, telling them apart by which URL each call carries
/// (this test never passes `--yes`, so no delete endpoint is ever
/// called). The Buildkite arm is only ever reached by the second test
/// below, whose document has a `pipeline` node; the first test's
/// document does not, so that arm never fires for it.
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
        &bin.join("curl"),
        "#!/usr/bin/env bash\n\
         set -euo pipefail\n\
         cat > /dev/null\n\
         url=\"\"\n\
         for a in \"$@\"; do\n\
         \x20\x20case \"$a\" in\n\
         \x20\x20\x20\x20http://*|https://*) url=\"$a\" ;;\n\
         \x20\x20esac\n\
         done\n\
         case \"$url\" in\n\
         \x20\x20*api.github.com*) echo '{\"topics\": [\"managed-by-willikins\"]}' ;;\n\
         \x20\x20*api.doppler.com*) echo '{\"project\": {\"description\": \"managed-by: willikins\"}}' ;;\n\
         \x20\x20*api.buildkite.com*) echo '{\"description\": \"managed-by: willikins\"}' ;;\n\
         \x20\x20*) echo \"stub curl: unrecognized URL: $url\" >&2; exit 1 ;;\n\
         esac\n",
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
        .env(
            "WILLIKINS_GITHUB_TOKEN",
            "github_pat_teardown-shape-test-token",
        )
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

/// The same circle, closed for the Buildkite arm: applies milestone 3a's
/// positive fixture (the one document whose plan has a `pipeline` node)
/// with the real binary, feeds the real `run --json` document to the
/// real script, and checks the organisation and slug teardown.sh reports
/// against the run record's own `pipeline` node outputs -- parsed here
/// the same way the script parses them (strip the known `url` prefix and
/// the already-read `slug` suffix), never hardcoded to the
/// `buildkite_org` input this test happens to supply.
#[test]
#[cfg(unix)]
fn teardown_reads_the_buildkite_pipeline_the_real_run_record_prints() {
    let temp = tempfile::tempdir().unwrap();
    let journal = temp.path().join("journal.jsonl");

    let run_id = apply_the_positive_buildkite_fixture(&journal);
    let record_text = run_record_json(&run_id, &journal);
    let record: serde_json::Value =
        serde_json::from_str(&record_text).expect("run --json prints one JSON document");
    let repo = node_output(&record, "repo", "repo");
    let project = node_output(&record, "doppler", "project");
    let pipeline_slug = node_output(&record, "pipeline", "slug");
    let pipeline_url = node_output(&record, "pipeline", "url");
    let buildkite_org = pipeline_url
        .strip_prefix("https://buildkite.com/")
        .and_then(|rest| rest.strip_suffix(&format!("/{pipeline_slug}")))
        .unwrap_or_else(|| panic!("pipeline url {pipeline_url} did not parse"))
        .to_string();

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
        .env(
            "WILLIKINS_GITHUB_TOKEN",
            "github_pat_teardown-shape-test-token",
        )
        .env("WILLIKINS_DOPPLER_TOKEN", "dp.sa.teardown-shape-test-token")
        // Shorter than the 20-character run `secret_literal_guard.rs`
        // flags after a Buildkite prefix (`crates/willikins-core/tests/
        // secret_literal_guard.rs`'s `BUILDKITE_TOKEN`) -- the shape
        // teardown.sh's own `--config -` header takes is what matters
        // here, not a pattern-valid credential.
        .env("WILLIKINS_BUILDKITE_TOKEN", "bkua_test")
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
    assert!(
        text.contains(&format!(
            "would delete Buildkite pipeline: {buildkite_org}/{pipeline_slug}"
        )),
        "the script did not name the pipeline the run recorded \
         ({buildkite_org}/{pipeline_slug}): {text}"
    );
}
