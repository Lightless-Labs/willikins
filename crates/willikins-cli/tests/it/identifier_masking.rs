//! Milestone 3i, task B5 (decision (b5); acceptance 16, and acceptance 19's
//! regression): process-level tests for the global `--reveal` flag,
//! driving the built `willikins` binary exactly the way
//! `tests/it/apply_and_journal.rs` and `tests/it/appstore_profile_apply_redaction.rs`
//! do.
//!
//! The fixture throughout is `workflows/appstore-signing-profile-from-doppler.yaml`
//! against the seeded fake state `workflows/fixtures/state/appstore-signing-profile.json`
//! (the same pair `appstore_profile_apply_redaction.rs` uses): it produces
//! three identifier-typed outputs in one plan --
//! `certificate.certificate` (`AppleCertificateId`, seeded `CERT1`),
//! `profile.profile`/the workflow's own `profile` output (`AppleProfileId`,
//! seeded `PROFILE1`) -- and takes an identifier-typed *input*,
//! `serial_number` (`AppleCertificateSerial`). `plan_text` never renders a
//! node's `inputs` (only its `outputs`, per `render::plan_text`'s own
//! doc), so the serial is absent from `plan`'s *text* output either way;
//! `--json plan` does serialize `PlannedNode::inputs`
//! (`crates/willikins-core/src/plan.rs`), so there the full 32-character
//! seeded value (`7B3F2A9C1D4E5F607182930A1B2C3D4E`) is exactly the
//! "masked by default, whole under --reveal" witness this file pins, and
//! `describe`'s own `resolved` section (below) is the text-mode
//! counterpart. `profile.content` (`AppleProfileContent`) stays a secret
//! either way.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::LazyLock;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn workflow(rel: &str) -> PathBuf {
    workspace_root().join(rel)
}

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

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "willikins-cli-identifier-masking-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
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

const FULL_SERIAL: &str = "7B3F2A9C1D4E5F607182930A1B2C3D4E";
const MASKED_SERIAL: &str = "7B3F...";
const FULL_CERTIFICATE: &str = "CERT1";
const MASKED_CERTIFICATE: &str = "CE...";
const FULL_PROFILE: &str = "PROFILE1";
const MASKED_PROFILE: &str = "PROF...";

// Resolved through `workspace_root()` rather than kept as bare
// `"workflows/..."` literals: cargo runs an integration test binary with
// its cwd at the *package* root (`crates/willikins-cli`), not the
// workspace root, so a relative path would resolve to
// `crates/willikins-cli/workflows/...` and the CLI would exit 2 on every
// test here (`tests/it/cli.rs`'s own `workspace_root`/`workflow` helpers, and
// `appstore_profile_apply_redaction.rs`'s own `workspace_root().join(...)`
// calls, exist for exactly this reason).
static SEED: LazyLock<String> = LazyLock::new(|| {
    workflow("workflows/fixtures/state/appstore-signing-profile.json")
        .to_str()
        .unwrap()
        .to_string()
});
static DOCUMENT: LazyLock<String> = LazyLock::new(|| {
    workflow("workflows/appstore-signing-profile-from-doppler.yaml")
        .to_str()
        .unwrap()
        .to_string()
});

/// The seed's *first* bundle identifier already carries a matching
/// profile, so `plan` reads everything `Present`/`NoOp` -- no write, no
/// approval gate to route around, which keeps these `plan`-only tests
/// simple. `apply`-based tests below use the second identifier instead
/// (see `appstore_profile_apply_redaction.rs`'s own comment), which has no
/// profile yet.
fn plan_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "plan",
        DOCUMENT.as_str(),
        "--fake-state",
        SEED.as_str(),
        "--input",
        "config=app-store-connect/prd",
        "--input",
        "identifier=com.example.willikins-demo",
        "--input",
        "bundle_name=willikins-demo",
        "--input",
        "platform=UNIVERSAL",
        "--input",
        "certificate_type=DISTRIBUTION",
        "--input",
        "serial_number=7B3F2A9C1D4E5F607182930A1B2C3D4E",
        "--input",
        "destination_project=third-thoughts",
        "--input",
        "destination_environment=prd",
        "--input",
        "secret_name=APPSTORE_SIGNING_PROFILE",
    ];
    args.extend_from_slice(extra);
    args
}

// ---------------------------------------------------------------------
// `plan`: text and JSON, masked by default, whole under --reveal
// ---------------------------------------------------------------------

#[test]
fn plan_text_masks_every_identifier_output() {
    let output = run(&plan_args(&[]));
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains(MASKED_CERTIFICATE), "text: {text}");
    assert!(text.contains(MASKED_PROFILE), "text: {text}");
    assert!(
        !text.contains(FULL_CERTIFICATE),
        "the full certificate id must never appear: {text}"
    );
    assert!(
        !text.contains(FULL_PROFILE),
        "the full profile id must never appear: {text}"
    );
    assert!(
        text.contains("[REDACTED AppleProfileContent]"),
        "the profile's content is a secret: {text}"
    );
}

#[test]
fn plan_json_masks_every_identifier_output_and_input_and_marks_each_one() {
    let mut args = vec!["--json"];
    args.extend(plan_args(&[]));
    let output = run(&args);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let raw = stdout(&output);
    let json: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    // The full serial never appears: `PlannedNode::inputs` serializes
    // `certificate`'s own `serial_number` input, which is where it would
    // otherwise show up (see the module doc).
    assert!(!raw.contains(FULL_SERIAL), "raw: {raw}");
    assert!(!raw.contains(FULL_CERTIFICATE), "raw: {raw}");
    assert!(!raw.contains(FULL_PROFILE), "raw: {raw}");
    assert!(raw.contains(MASKED_SERIAL), "raw: {raw}");
    assert!(raw.contains(MASKED_CERTIFICATE), "raw: {raw}");
    assert!(raw.contains(MASKED_PROFILE), "raw: {raw}");
    // At least one masked `Value` carries the `"masked": true` marker
    // decision (b4) adds beside `"value"`.
    assert!(
        json_has_masked_marker(&json),
        "no masked:true marker found in {json}"
    );
    assert!(raw.contains("REDACTED"), "raw: {raw}");
}

#[test]
fn plan_text_reveal_shows_every_identifier_output_in_full() {
    let output = run(&plan_args(&["--reveal"]));
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains(FULL_CERTIFICATE), "text: {text}");
    assert!(text.contains(FULL_PROFILE), "text: {text}");
    // Secrets never reveal, --reveal or not.
    assert!(
        text.contains("[REDACTED AppleProfileContent]"),
        "text: {text}"
    );
}

#[test]
fn plan_json_reveal_shows_every_identifier_in_full_and_no_masked_marker() {
    let mut args = vec!["--json"];
    args.extend(plan_args(&["--reveal"]));
    let output = run(&args);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let raw = stdout(&output);
    let json: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert!(raw.contains(FULL_SERIAL), "raw: {raw}");
    assert!(raw.contains(FULL_CERTIFICATE), "raw: {raw}");
    assert!(raw.contains(FULL_PROFILE), "raw: {raw}");
    assert!(
        !json_has_masked_marker(&json),
        "a revealed plan must carry no masked:true marker: {json}"
    );
    assert!(raw.contains("REDACTED"), "raw: {raw}");
}

/// Depth-first search for any object carrying `"masked": true`.
fn json_has_masked_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("masked") == Some(&serde_json::Value::Bool(true)) {
                return true;
            }
            map.values().any(json_has_masked_marker)
        }
        serde_json::Value::Array(items) => items.iter().any(json_has_masked_marker),
        _ => false,
    }
}

// ---------------------------------------------------------------------
// `describe`: a raw identifier-typed input resolves masked too
// ---------------------------------------------------------------------

#[test]
fn describe_text_masks_a_resolved_identifier_typed_input() {
    let inputs: Vec<String> = [
        ("config", "app-store-connect/prd"),
        ("identifier", "com.example.willikins-demo"),
        ("bundle_name", "willikins-demo"),
        ("platform", "UNIVERSAL"),
        ("certificate_type", "DISTRIBUTION"),
        ("serial_number", FULL_SERIAL),
        ("destination_project", "third-thoughts"),
        ("destination_environment", "prd"),
        ("secret_name", "APPSTORE_SIGNING_PROFILE"),
    ]
    .into_iter()
    .map(|(name, value)| format!("{name}={value}"))
    .collect();

    let mut args: Vec<&str> = vec!["describe", DOCUMENT.as_str()];
    for input in &inputs {
        args.push("--input");
        args.push(input.as_str());
    }
    let output = run(&args);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!("serial_number: {MASKED_SERIAL}")),
        "text: {text}"
    );
    assert!(!text.contains(FULL_SERIAL), "text: {text}");

    let mut reveal_args: Vec<&str> = vec!["--reveal"];
    reveal_args.extend(args.iter().copied());
    let revealed = run(&reveal_args);
    assert_eq!(exit_code(&revealed), 0, "stderr: {}", stderr(&revealed));
    let revealed_text = stdout(&revealed);
    assert!(
        revealed_text.contains(&format!("serial_number: {FULL_SERIAL}")),
        "text: {revealed_text}"
    );
}

// ---------------------------------------------------------------------
// `apply` / `run` / `runs` over a real file journal
// ---------------------------------------------------------------------

/// Every top-level JSON value concatenated in `text`, in order (see
/// `apply_and_journal.rs`'s own helper of the same name).
fn json_documents(text: &str) -> Vec<serde_json::Value> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<serde_json::Value>()
        .map(|result| result.unwrap_or_else(|err| panic!("invalid JSON in {text:?}: {err}")))
        .collect()
}

fn apply_create_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "apply",
        DOCUMENT.as_str(),
        "--fake-state",
        SEED.as_str(),
        "--approve",
        "--input",
        "config=app-store-connect/prd",
        // The seed's *second* identifier, which has no profile yet, so
        // this really creates one (see `appstore_profile_apply_redaction.rs`).
        "--input",
        "identifier=com.example.willikins-demo-two",
        "--input",
        "bundle_name=willikins-demo",
        "--input",
        "platform=UNIVERSAL",
        "--input",
        "certificate_type=DISTRIBUTION",
        "--input",
        "serial_number=7B3F2A9C1D4E5F607182930A1B2C3D4E",
        "--input",
        "destination_project=third-thoughts",
        "--input",
        "destination_environment=prd",
        "--input",
        "secret_name=APPSTORE_SIGNING_PROFILE",
    ];
    args.extend_from_slice(extra);
    args
}

#[test]
fn run_and_runs_over_a_journal_mask_the_same_way_as_plan() {
    let dir = TempDir::new("run-runs");
    let journal = dir.join("journal.jsonl");

    let applied = run(&apply_create_args(&[
        "--journal",
        journal.to_str().unwrap(),
    ]));
    assert_eq!(exit_code(&applied), 0, "stderr: {}", stderr(&applied));
    let applied_text = stdout(&applied);
    assert!(applied_text.contains("state: succeeded"), "{applied_text}");
    assert!(!applied_text.contains(FULL_SERIAL), "text: {applied_text}");

    // `runs --journal`: masked by default.
    let runs = run(&["runs", "--journal", journal.to_str().unwrap()]);
    assert_eq!(exit_code(&runs), 0, "stderr: {}", stderr(&runs));
    let runs_text = stdout(&runs);
    assert!(!runs_text.contains(FULL_SERIAL), "{runs_text}");
    assert!(
        runs_text.contains("[REDACTED AppleProfileContent]"),
        "{runs_text}"
    );

    // `runs --journal` revealed: full values, secret still redacted. The
    // seed's second identifier's certificate and freshly created profile
    // are `CERT1` and a `FAKEPR0F...`-prefixed id (see
    // `appstore_profile_apply_redaction.rs`'s own `FAKE_CONTENT_PREFIX`
    // sibling constant); masked, neither full id appears at all.
    assert!(
        !runs_text.contains("CERT1"),
        "masked runs text leaked the certificate id: {runs_text}"
    );
    let runs_revealed = run(&["--reveal", "runs", "--journal", journal.to_str().unwrap()]);
    assert_eq!(
        exit_code(&runs_revealed),
        0,
        "stderr: {}",
        stderr(&runs_revealed)
    );
    let runs_revealed_text = stdout(&runs_revealed);
    assert!(
        runs_revealed_text.contains("[REDACTED AppleProfileContent]"),
        "{runs_revealed_text}"
    );
    assert!(
        runs_revealed_text.contains("CERT1"),
        "--reveal should show the full certificate id: {runs_revealed_text}"
    );

    // `--json run <id>`: find the run id from the apply, then check both
    // `run` and `--json run` agree with `runs`'s own masking.
    let journal2 = dir.join("journal2.jsonl");
    let mut json_apply_args = vec!["--json"];
    json_apply_args.extend(apply_create_args(&[
        "--journal",
        journal2.to_str().unwrap(),
    ]));
    let json_applied = run(&json_apply_args);
    assert_eq!(
        exit_code(&json_applied),
        0,
        "stderr: {}",
        stderr(&json_applied)
    );
    let docs = json_documents(&stdout(&json_applied));
    let run_id = docs[1]["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no run_id in {docs:?}"))
        .to_string();

    let one_run = run(&[
        "--json",
        "run",
        &run_id,
        "--journal",
        dir.join("journal2.jsonl").to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&one_run), 0, "stderr: {}", stderr(&one_run));
    let raw = stdout(&one_run);
    assert!(!raw.contains(FULL_SERIAL), "raw: {raw}");
    assert!(
        !raw.contains(FULL_CERTIFICATE),
        "the full certificate id must never appear: {raw}"
    );
    assert!(raw.contains("REDACTED"), "raw: {raw}");

    let one_run_revealed = run(&[
        "--reveal",
        "--json",
        "run",
        &run_id,
        "--journal",
        dir.join("journal2.jsonl").to_str().unwrap(),
    ]);
    assert_eq!(
        exit_code(&one_run_revealed),
        0,
        "stderr: {}",
        stderr(&one_run_revealed)
    );
    let revealed_raw = stdout(&one_run_revealed);
    // The secret stays redacted even revealed, while the certificate id
    // (a `RunNode` output, not an input: it reaches `--json run` too)
    // shows whole.
    assert!(revealed_raw.contains("REDACTED"), "raw: {revealed_raw}");
    assert!(
        revealed_raw.contains(FULL_CERTIFICATE),
        "--reveal should show the full certificate id: {revealed_raw}"
    );
}

// ---------------------------------------------------------------------
// acceptance 19: apply --plan-id of an approved plan with an
// AppleCertificateSerial input still applies
// ---------------------------------------------------------------------

#[test]
fn apply_by_plan_id_still_applies_with_an_identifier_typed_input() {
    let dir = TempDir::new("plan-id-identifier");
    let journal = dir.join("journal.jsonl");
    std::fs::copy(
        DOCUMENT.as_str(),
        dir.join("appstore-signing-profile-from-doppler.yaml"),
    )
    .unwrap();

    // Process 1: apply records the plan and refuses (no --approve):
    // `appstore.profile.ensure` is `Class::Destructive`, so approval is
    // always required regardless of masking.
    let mut first_args = vec!["--json"];
    first_args.extend(apply_create_args(&["--journal", journal.to_str().unwrap()]));
    // Drop `--approve` (index known: it is the 5th element of
    // `apply_create_args`'s own list) by rebuilding without it.
    let first_args: Vec<&str> = first_args
        .into_iter()
        .filter(|arg| *arg != "--approve")
        .collect();
    let first = run(&first_args);
    assert_eq!(exit_code(&first), 1, "stderr: {}", stderr(&first));
    let plan_id = {
        let docs = json_documents(&stdout(&first));
        docs[0]["plan_id"]
            .as_str()
            .unwrap_or_else(|| panic!("no plan_id in {docs:?}"))
            .to_string()
    };

    // Process 2: approve.
    let approved = run(&["approve", &plan_id, "--journal", journal.to_str().unwrap()]);
    assert_eq!(exit_code(&approved), 0, "stderr: {}", stderr(&approved));

    // Process 3: apply --plan-id runs, with the same `--fake-state` seed
    // so `doppler.value.get`/`doppler.secret.get` resolve the same way at
    // apply time as they did at plan time.
    let applied = run(&[
        "--json",
        "apply",
        "--plan-id",
        &plan_id,
        "--journal",
        journal.to_str().unwrap(),
        "--workflows-dir",
        dir.path().to_str().unwrap(),
        "--fake-state",
        SEED.as_str(),
    ]);
    assert_eq!(exit_code(&applied), 0, "stderr: {}", stderr(&applied));
    let applied_docs = json_documents(&stdout(&applied));
    assert_eq!(applied_docs[0]["state"], "succeeded", "{applied_docs:?}");
}
