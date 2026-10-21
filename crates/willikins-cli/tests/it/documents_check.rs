//! Milestone 2c, task 22 (decision 25): `willikins documents check <dir>`.
//!
//! Every case here runs with `env_clear` (this file's own `run`), so
//! none of these ever reads a developer's sandbox credential or makes a
//! network call -- the whole point of the command is that `check` never
//! calls a provider, so it needs none of the three provider credentials
//! to check a document that uses one.

use std::path::{Path, PathBuf};
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
            "willikins-cli-documents-check-test-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
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

fn write(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

const FOO: &str = "name: foo\ndescription: a workflow\nsteps: {}\n";
const BAR: &str = "name: bar\ndescription: another workflow\nsteps: {}\n";

/// Every entry's name and bytes, sorted by name -- used to prove a run
/// changed nothing in the directory it checked.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .expect("readable directory")
        .map(|entry| {
            let entry = entry.expect("readable directory entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            let bytes = std::fs::read(entry.path()).expect("readable file");
            (name, bytes)
        })
        .collect();
    entries.sort();
    entries
}

// ---------------------------------------------------------------------
// The manifest itself
// ---------------------------------------------------------------------

#[test]
fn prints_a_shasum_format_manifest_in_filename_order() {
    let dir = TempDir::new("manifest");
    write(dir.path(), "foo.yaml", FOO);
    write(dir.path(), "bar.yaml", BAR);

    let output = run(&["documents", "check", dir.path().to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));

    let foo_sha = willikins_journal::DocumentSha256::compute(FOO.as_bytes()).to_string();
    let bar_sha = willikins_journal::DocumentSha256::compute(BAR.as_bytes()).to_string();
    // `shasum -a 256` order: two spaces between the digest and the
    // filename; filename order (`bar` before `foo`), matching
    // `scan_directory`'s own deterministic order.
    assert_eq!(
        stdout(&output),
        format!("{bar_sha}  bar.yaml\n{foo_sha}  foo.yaml\n")
    );
}

#[test]
fn json_mode_prints_the_filename_to_hash_mapping() {
    let dir = TempDir::new("json");
    write(dir.path(), "foo.yaml", FOO);

    let output = Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(["--json", "documents", "check", dir.path().to_str().unwrap()])
        .env_clear()
        .output()
        .expect("failed to run the willikins binary");
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));

    let value: serde_json::Value =
        serde_json::from_str(stdout(&output).trim()).expect("valid JSON");
    let sha = willikins_journal::DocumentSha256::compute(FOO.as_bytes()).to_string();
    assert_eq!(value, serde_json::json!({ "foo.yaml": sha }));
}

#[test]
fn an_empty_directory_checks_clean_with_an_empty_manifest() {
    let dir = TempDir::new("empty");
    let output = run(&["documents", "check", dir.path().to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "");
}

// ---------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------

#[test]
fn a_document_that_fails_to_parse_refuses_naming_the_file() {
    let dir = TempDir::new("broken-parse");
    write(dir.path(), "broken.yaml", "not: [valid");

    let output = run(&["documents", "check", dir.path().to_str().unwrap()]);
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "", "nothing is printed on a refusal");
    assert!(
        stderr(&output).contains("broken.yaml"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_document_that_fails_check_refuses_with_exit_one_naming_the_file() {
    let dir = TempDir::new("broken-check");
    write(
        dir.path(),
        "mystery.yaml",
        "name: mystery\ndescription: calls a tool that does not exist\nsteps:\n  thing:\n    tool: no.such.tool\n    with: {}\n",
    );

    let output = run(&["documents", "check", dir.path().to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "", "nothing is printed on a refusal");
    assert!(
        stderr(&output).contains("mystery.yaml"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_broken_uses_child_refuses_with_exit_one_naming_the_root() {
    let dir = TempDir::new("broken-uses");
    write(
        dir.path(),
        "root.yaml",
        "name: root\nsteps:\n  child:\n    uses: ghost\n",
    );

    let output = run(&["documents", "check", dir.path().to_str().unwrap()]);
    assert_eq!(exit_code(&output), 1, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "", "nothing is printed on a refusal");
    assert!(
        stderr(&output).contains("root.yaml"),
        "stderr: {}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_directory_is_refused_like_the_server_refuses_it() {
    let real = TempDir::new("symlink-target");
    write(real.path(), "foo.yaml", FOO);
    let parent = TempDir::new("symlink-parent");
    let link = parent.path().join("workflows");
    std::os::unix::fs::symlink(real.path(), &link).expect("failed to create symlink");

    let output = run(&["documents", "check", link.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 2, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "", "nothing is printed on a refusal");
    assert!(
        stderr(&output).contains("symlink"),
        "stderr: {}",
        stderr(&output)
    );
}

// ---------------------------------------------------------------------
// "Reads only `dir`; writes nothing" (the task's own wording)
// ---------------------------------------------------------------------

#[test]
fn writes_nothing_to_the_directory_it_checks() {
    let dir = TempDir::new("no-write");
    write(dir.path(), "foo.yaml", FOO);
    let before = snapshot(dir.path());

    let output = run(&["documents", "check", dir.path().to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));

    assert_eq!(before, snapshot(dir.path()));
}

// ---------------------------------------------------------------------
// Against the live tool set, with no provider credential at all
// ---------------------------------------------------------------------

/// The repository's own public `workflows/*.yaml` are exactly the set
/// `crates/willikins-server/tests/it/image_contents.rs` pins as what a
/// deployment's image carries (several of them call `github.*` and
/// `doppler.*` tools) -- this test is what establishes that the set
/// checks against the **live** catalog, not only the fake one `validate`
/// and `plan` use by default. Checking it with every credential variable
/// cleared (`run`'s own `env_clear`) is the proof that
/// `willikins documents check` needs none of them: `check` validates a
/// document's wiring against the live tool set's *shapes*, and never
/// calls a provider.
#[test]
fn checks_the_public_workflows_directory_against_the_live_tool_set_with_no_credential() {
    let dir = workspace_root().join("workflows");
    let output = run(&["documents", "check", dir.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));

    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 17, "stdout: {text}");
    for line in &lines {
        let (sha, _filename) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("not `shasum -a 256` shaped: {line:?}"));
        assert_eq!(sha.len(), 64, "not a sha256 hex digest: {line:?}");
        assert!(
            sha.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "not lower-case hex: {line:?}"
        );
    }
}
