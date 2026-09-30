//! Milestone 3e task 3, the honest limit G1/G2 both carried forward:
//! "driving `Action::Blocked`/`Skip` through a real `plan`/`apply`
//! *binary* run is still T3's job, once Walter's own gates exist" -- and
//! the task's own acceptance text, "no secret reaches a non-secret port,
//! plan output **or journal**". `crates/willikins-cli/tests/walter_document.rs`
//! proves the mechanism in-process; this proves it through the built
//! binary, against a real file journal, which is the one surface that
//! test cannot reach.
//!
//! One run, the four OBSERVED gates deliberately unmet (a fresh seed: no
//! bundle id, no app record, `APP_GROUPS` not enabled, `APP_ATTEST` not
//! enabled, and the pipeline's stored configuration never seeded to equal
//! the rendered bootstrap -- `bootstrap_gate`, milestone 3g's own
//! replacement for the old `m7_bootstrap` acknowledgement): `apply` must
//! exit
//! **3** (decision (j), point 7 -- `exit_for_run_state` maps
//! `RunState::Blocked` to 3, distinct from a failure's 1), stdout must
//! show the `blocked:` section and "re-run this document once done", and
//! neither stdout, stderr, nor the journal file may ever carry the
//! signing key's own PEM marker.
//!
//! # A real bug found while writing this test, and why it is worked
//! around here rather than fixed
//!
//! The two remaining `operator.acknowledge` leaves (M5, M6) are
//! deliberately supplied `done` here, **not** left unmet like the four
//! observed gates. Leaving either of them unsupplied hits a genuine,
//! pre-existing defect this test uncovered: `willikins_server::butler::resolve_recorded_inputs`
//! rebuilds a plan's inputs from the journal's own recorded
//! `PlanRecorded.inputs` by requiring **every** declared workflow input to
//! have an entry there -- but G3's own design
//! (`docs/plans/2026-09-27-milestone-3e-new-ios-app.md`, decision (j)
//! point 6: "The input stays absent from the resolved map") deliberately
//! never records an unsupplied `OperatorAcknowledgement` input at all.
//! The one-shot `apply --approve` path (plan, journal it, then rebuild
//! from the journal to actually run it) hits exactly this gap: a fresh
//! `plan` shows every blocked gate correctly (proven by this test's own
//! observed-gate assertions below, unaffected by the bug), but supplying
//! `--approve` with an acknowledgement gate still unmet fails with
//! `ButlerError::RecordedInputUnreadable`, exit 1, "recorded input
//! `<name>` could not be read back: the plan recorded no value for input
//! `<name>`" -- a **different** failure than a blocked run, and the exact
//! shape G3's own "Honest limits" note anticipated ("no live run has
//! exercised `operator.acknowledge` against a real MCP client end to
//! end"). Out of this task's own scope (a `willikins-server` engine fix,
//! not the Walter document); reported to the coordinator rather than
//! patched here or quietly avoided by weakening what this test proves
//! about the four observed gates.

use std::path::PathBuf;
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
        .env_clear()
        .output()
        .expect("failed to run the willikins binary")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "willikins-cli-walter-blocked-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A valid [`willikins_types::BuildkiteToken`], `concat!`-assembled so no
/// single literal in this file spells a real-shaped Buildkite token
/// contiguously -- mirrors
/// `crates/willikins-cli/tests/walter_document.rs`'s own
/// `SEEDED_BUILDKITE_TOKEN` and
/// `crates/willikins-providers-buildkite/tests/buildkite_token_documents.rs`'s
/// `SEEDED_TOKEN`.
const SEEDED_BUILDKITE_TOKEN: &str = concat!("bkua_", "wlknFixtureTokenNotARealCredential00");

#[test]
#[allow(clippy::too_many_lines)] // one linear scenario: build args, run, assert
fn a_blocked_walter_apply_exits_3_and_leaks_no_secret_into_the_journal() {
    let dir = TempDir::new("blocked");
    let journal = dir.join("journal.jsonl");
    let document = workspace_root().join("workflows/walter-ios-app.yaml");
    let seed_source = workspace_root().join("workflows/fixtures/state/walter-ios-app.json");

    // The committed fixture carries only the placeholder
    // `BUILDKITE_TOKEN_PLACEHOLDER` (D2), never a real-shaped Buildkite
    // token on disk -- `secret_literal_guard.rs` scans this JSON file
    // too, and a real `bkua_...` token is exactly its `BUILDKITE_TOKEN`
    // pattern. Unlike an in-process test, this one runs the built binary
    // against a file path, so the substitution has to happen here rather
    // than in a Rust reader: write a substituted copy into this test's
    // own `TempDir` and point `--fake-state` at that instead.
    let seed_json = std::fs::read_to_string(&seed_source)
        .unwrap_or_else(|err| panic!("{}: {err}", seed_source.display()));
    let seed_json = seed_json.replace("BUILDKITE_TOKEN_PLACEHOLDER", SEEDED_BUILDKITE_TOKEN);
    let seed = dir.join("state.json");
    std::fs::write(&seed, seed_json).expect("write substituted fake state");

    let output = run(&[
        "apply",
        document.to_str().unwrap(),
        "--fake-state",
        seed.to_str().unwrap(),
        "--journal",
        journal.to_str().unwrap(),
        "--approve",
        "--input",
        "app_identifier=com.example.walter",
        "--input",
        "nse_identifier=com.example.walter.nse",
        "--input",
        "widgets_identifier=com.example.walter.widgets",
        "--input",
        "certificate_type=DISTRIBUTION",
        "--input",
        "serial_number=7B3F2A9C1D4E5F607182930A1B2C3D4E",
        "--input",
        "environments=dev,stg,prd",
        "--input",
        "base_configs=appstore-connect/deploy_ios,github/bande-a-bonnot,open-telemetry/prd_signoz",
        // Supplied, not left awaited -- see this file's own module doc for
        // why: leaving either unmet trips a separate, pre-existing defect
        // in the journal-replay path, not the gate mechanism this test
        // means to prove. M3 and M7 are gone as acknowledgements
        // (milestone 3g, W1): M3 is real writes now, and M7 is
        // `bootstrap_gate`, an *observed* gate this run cannot satisfy
        // (nothing seeds the pipeline's stored configuration), so it is
        // deliberately left blocked below alongside the others.
        "--input",
        "m5_apns_key_done=done",
        "--input",
        "m6_ci_doppler_access_done=done",
    ]);

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert_eq!(
        output.status.code(),
        Some(3),
        "a blocked run must exit 3, not fail or succeed -- stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("blocked:"),
        "stdout must show the blocked: section -- stdout: {stdout}"
    );
    assert!(
        stdout.contains("re-run this document once done"),
        "stdout must tell the operator to re-run -- stdout: {stdout}"
    );
    // The leaf app-record gate, the three per-identifier app-group gates
    // (M1, M2), and the bootstrap gate (M7, milestone 3g: an observed
    // gate now, never satisfied by this run since nothing seeds the
    // pipeline's stored configuration).
    for node in [
        "app_record",
        "app_app_groups",
        "nse_app_groups",
        "widgets_app_groups",
        "app_app_attest",
        "bootstrap_gate",
    ] {
        assert!(
            stdout.contains(node),
            "stdout must name the blocked node `{node}` -- stdout: {stdout}"
        );
    }
    // The two acknowledgement leaves were supplied, so they must NOT
    // appear in the blocked section.
    for node in ["m5_apns_key", "m6_ci_doppler_access"] {
        assert!(
            !stdout.contains(&format!("{node} (operator.acknowledge): Blocked")),
            "`{node}` was supplied `done` and must not be blocked -- stdout: {stdout}"
        );
    }

    let journal_text = std::fs::read_to_string(&journal).expect("the journal was written");
    for (label, text) in [
        ("stdout", &stdout),
        ("stderr", &stderr),
        ("journal", &journal_text),
    ] {
        assert!(
            !text.contains("PRIVATE KEY"),
            "{label} carries the signing key's PEM marker: {text}"
        );
        assert!(
            !text.contains("fakeprofilecontent"),
            "{label} carries a fake profile's plaintext content: {text}"
        );
        assert!(
            !text.contains("ghp_example"),
            "{label} carries the seeded GitHub token's raw value: {text}"
        );
        assert!(
            !text.contains("ghp_write_example"),
            "{label} carries the seeded write token's raw value: {text}"
        );
        assert!(
            !text.contains(SEEDED_BUILDKITE_TOKEN),
            "{label} carries the seeded Buildkite token's raw value: {text}"
        );
    }
}
