//! Milestone 3c acceptance tests 8 and 11, end to end through the built
//! binary: a fake `apply` of `workflows/appstore-signing-profile-from-doppler.yaml`
//! that really creates a profile and really writes its content through
//! `doppler.secret.set`, with a file journal and a `--fake-state-out` dump.
//! The profile's content must appear in none of the three -- stdout, the
//! journal, the dumped state -- only its redaction marker may.
//!
//! Task 2's own document test planned the document and checked the plan's
//! rendering; nothing applied it, so neither the journal nor the dump was
//! ever looked at, and the dump did print the content (the task-3
//! adversarial pass, `docs/research/2026-09-22-m3c-adversarial-pass.md`).

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
            "willikins-cli-profile-apply-{}-{label}",
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

/// Every profile the fake creates carries content beginning with this
/// (`FakeAppstoreProfileEnsure`'s own `fakeprofilecontent{id}==`).
const FAKE_CONTENT_PREFIX: &str = "fakeprofilecontent";

#[test]
fn applying_the_signing_document_never_writes_profile_content_anywhere() {
    let dir = TempDir::new("redaction");
    let journal = dir.join("journal.jsonl");
    let dump = dir.join("state-out.json");
    let document = workspace_root().join("workflows/appstore-signing-profile-from-doppler.yaml");
    let seed = workspace_root().join("workflows/fixtures/state/appstore-signing-profile.json");

    let output = run(&[
        "apply",
        document.to_str().unwrap(),
        "--fake-state",
        seed.to_str().unwrap(),
        "--fake-state-out",
        dump.to_str().unwrap(),
        "--journal",
        journal.to_str().unwrap(),
        "--approve",
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
        // Not the seeded profile's name, so the fake really creates one.
        "--input",
        "profile_name=willikins-demo-profile-two",
        "--input",
        "destination_project=third-thoughts",
        "--input",
        "destination_environment=prd",
        "--input",
        "secret_name=APPSTORE_SIGNING_PROFILE",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("state: succeeded"), "stdout: {stdout}");
    assert!(
        stdout.contains("[REDACTED AppleProfileContent]"),
        "the created profile's content should render as its marker: {stdout}"
    );

    let journal_text = std::fs::read_to_string(&journal).expect("the journal was written");
    let dump_text = std::fs::read_to_string(&dump).expect("the state was dumped");
    assert!(
        dump_text.contains("willikins-demo-profile-two") || dump_text.contains("FAKEPR0F"),
        "the dump should hold the created profile's record: {dump_text}"
    );
    for (label, text) in [
        ("stdout", &stdout),
        ("stderr", &stderr),
        ("journal", &journal_text),
        ("fake-state-out", &dump_text),
    ] {
        assert!(
            !text.contains(FAKE_CONTENT_PREFIX),
            "{label} carries the profile's content: {text}"
        );
    }
}
