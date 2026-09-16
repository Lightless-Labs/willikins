//! Task 14, the offline half of acceptance test 18: the same four
//! `willikins` invocations the live smoke test makes, in the same order,
//! against the **fake** catalog -- so every JSON shape
//! `tests/live_smoke.rs` reads is exercised by the workspace gate before
//! the live run ever happens.
//!
//! This target is not feature-gated and not `#[ignore]`d. It reaches no
//! network: no invocation carries `--live`, so the CLI builds the fake
//! catalog and never even looks for a credential.
//!
//! What it deliberately does *not* cover, and why:
//!
//! - the `gh` scope pre-flight and the leftover checks, which are
//!   questions about a real account;
//! - `deploy/teardown.sh`, already covered twice over by
//!   `tests/teardown_script.rs` -- once against its own stubbed scenario
//!   suite, once against a real `willikins run --json` document, which
//!   is the tie that matters (the script's `jq` paths).
//!
//! The one place the two halves differ on purpose: the live run starts
//! from an account where nothing exists, while fake state does not
//! persist between two CLI processes at all, so the convergence step
//! carries the first run's ending state forward with
//! `--fake-state-out`/`--fake-state`. Nothing in
//! `workflows/fixtures/state/` names this run's resources, and inventing
//! a seed that did would only assert that a hand-written file says what
//! the first apply already proved.

mod common;

use std::path::{Path, PathBuf};

use common::{
    FIRST_APPLY, Invocation, PRINCIPAL, ROTATION, SECOND_APPLY, assert_no_credential_bytes,
    assert_nodes, assert_repo_url, assert_token_unknown, positive_inputs, recorded_run_count,
    rotation_inputs, willikins, workspace_path,
};

/// A directory removed when the test ends, however it ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "willikins-smoke-parity-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary directory");
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

fn as_str(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 path")
}

/// `apply <document> --json --journal <j> --principal smoke-operator`,
/// plus the given inputs and fake-state plumbing.
fn apply(document: &str, journal: &str, inputs: &[String], extra: &[&str]) -> Invocation {
    let document = workspace_path(document);
    let mut args: Vec<&str> = vec![
        "--json",
        "apply",
        document.as_str(),
        "--journal",
        journal,
        "--principal",
        PRINCIPAL,
    ];
    args.extend(inputs.iter().map(String::as_str));
    args.extend(extra.iter().copied());
    willikins(&args, &[])
}

/// The whole sequence in one test: the four invocations share one
/// journal and one carried-forward fake state, exactly as the live run's
/// four invocations share one journal and one real account.
#[test]
// The four invocations are one sequence: each step's assertions read
// state the previous step left behind, so splitting them into separate
// test functions would either re-run the whole sequence four times or
// share mutable state between tests. The live half carries the same
// allow for the same reason.
#[allow(clippy::too_many_lines)]
fn the_smoke_sequence_holds_against_the_fake_catalog() {
    let dir = TempDir::new("sequence");
    let journal = dir.join("journal.jsonl");
    let journal = as_str(&journal).to_string();
    let after_first = dir.join("state-1.json");
    let after_second = dir.join("state-2.json");

    // --- b. first apply: nothing exists yet. -------------------------
    let first = apply(
        "workflows/new-rust-service.yaml",
        &journal,
        &positive_inputs(),
        &["--fake-state-out", as_str(&after_first)],
    );
    // The sweep runs before any assertion whose message prints a
    // stream, the same order the live half keeps.
    assert_no_credential_bytes(
        "first apply",
        &[("stdout", &first.stdout), ("stderr", &first.stderr)],
    );
    assert_eq!(
        first.code(),
        0,
        "first apply:\nstdout:\n{}\nstderr:\n{}",
        first.stdout,
        first.stderr
    );
    let record = first.run_record();
    assert_eq!(record["state"].as_str(), Some("succeeded"), "{record}");
    assert_nodes(record, FIRST_APPLY, "first apply");
    assert_repo_url(record, "first apply");
    let first_run_id = first.run_id();

    // --- c. second apply: the convergence claim. ---------------------
    let second = apply(
        "workflows/new-rust-service.yaml",
        &journal,
        &positive_inputs(),
        &[
            "--fake-state",
            as_str(&after_first),
            "--fake-state-out",
            as_str(&after_second),
        ],
    );
    assert_no_credential_bytes(
        "second apply",
        &[("stdout", &second.stdout), ("stderr", &second.stderr)],
    );
    assert_eq!(
        second.code(),
        0,
        "second apply:\nstdout:\n{}\nstderr:\n{}",
        second.stdout,
        second.stderr
    );
    let record = second.run_record();
    assert_eq!(record["state"].as_str(), Some("succeeded"), "{record}");
    assert_nodes(record, SECOND_APPLY, "second apply");
    assert_token_unknown(record, "second apply");
    assert_repo_url(record, "second apply");

    // --- d. the rotation, refused without approval. ------------------
    let before = recorded_run_count(&journal);
    let refused = apply(
        "workflows/rotate-service-token.yaml",
        &journal,
        &rotation_inputs(),
        &["--fake-state", as_str(&after_second)],
    );
    assert_no_credential_bytes(
        "refused rotation",
        &[("stdout", &refused.stdout), ("stderr", &refused.stderr)],
    );
    assert_eq!(
        refused.code(),
        1,
        "refused rotation:\nstdout:\n{}\nstderr:\n{}",
        refused.stdout,
        refused.stderr
    );
    let error = refused
        .docs
        .iter()
        .find(|doc| doc.get("kind").is_some())
        .unwrap_or_else(|| panic!("no error document: {}", refused.stdout));
    assert_eq!(error["kind"].as_str(), Some("ApprovalRequired"), "{error}");
    assert_eq!(error["class"].as_str(), Some("destructive"), "{error}");
    assert_eq!(
        refused.plan_response()["requires_approval"].as_bool(),
        Some(true),
        "{}",
        refused.stdout
    );
    assert_eq!(
        recorded_run_count(&journal),
        before,
        "the refused rotation started a run"
    );

    // --- e. the rotation, applied with approval. ---------------------
    let rotated = apply(
        "workflows/rotate-service-token.yaml",
        &journal,
        &rotation_inputs(),
        &["--fake-state", as_str(&after_second), "--approve"],
    );
    assert_no_credential_bytes(
        "rotation",
        &[("stdout", &rotated.stdout), ("stderr", &rotated.stderr)],
    );
    assert_eq!(
        rotated.code(),
        0,
        "rotation:\nstdout:\n{}\nstderr:\n{}",
        rotated.stdout,
        rotated.stderr
    );
    let record = rotated.run_record();
    assert_eq!(record["state"].as_str(), Some("succeeded"), "{record}");
    assert_nodes(record, ROTATION, "rotation");
    let rotation_run_id = rotated.run_id();

    // --- f/g. both runs are readable by id, which is what
    // `deploy/teardown.sh` reads names from. -------------------------
    assert_ne!(first_run_id, rotation_run_id);
    for run_id in [&first_run_id, &rotation_run_id] {
        let shown = willikins(
            &[
                "--json",
                "run",
                run_id.as_str(),
                "--journal",
                journal.as_str(),
            ],
            &[],
        );
        assert_eq!(shown.code(), 0, "run {run_id}: {}", shown.stderr);
        assert_eq!(shown.run_record()["run_id"].as_str(), Some(run_id.as_str()));
    }

    // The two resources `teardown.sh` names come from the *first* run's
    // own recorded outputs, never re-derived from the slug. Read them
    // the way the script's `jq` does.
    let shown = willikins(
        &[
            "--json",
            "run",
            first_run_id.as_str(),
            "--journal",
            journal.as_str(),
        ],
        &[],
    );
    let record = shown.run_record();
    assert_eq!(
        common::node_output(record, "repo", "repo")["value"].as_str(),
        Some(common::REPO)
    );
    assert_eq!(
        common::node_output(record, "doppler", "project")["value"].as_str(),
        Some(common::PROJECT)
    );
}
