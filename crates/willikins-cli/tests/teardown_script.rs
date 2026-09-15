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
