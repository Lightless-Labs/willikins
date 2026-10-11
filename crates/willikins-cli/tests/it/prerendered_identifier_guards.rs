//! Milestone 3i, task B7 (decision (b8); acceptance 20): one
//! document-level pre-rendered-string guard, plus the end-to-end proof
//! that `describe` renders an identifier-typed input *default* masked.
//!
//! Decision (b8) lists four routes a `Value` reaches text before any
//! output mode exists: a tool's `key` port and a gate's `subject` port
//! (pinned in `crates/willikins-server/src/catalog.rs`'s own
//! `no_live_or_fake_tool_has_an_identifier_typed_key_or_gate_subject_port`),
//! a `for_each` instance key, and `describe`'s rendered `default`. This
//! file pinned the remaining two at the document level: no shipped
//! document's `for_each` source resolves to a list of an identifier
//! type, and no shipped document declares an identifier-typed input
//! default.
//!
//! Milestone 3n, decision (b1): the `for_each` guard is deleted here.
//! `check` now refuses an identifier-typed `for_each` source outright
//! (`CheckError::IdentifierForEachSource`, pinned by
//! `crates/willikins-core/tests/it/check.rs` and this crate's own
//! `milestone_3n_acceptance_11_identifier_for_each_source_is_refused`),
//! so the document-level search this guard ran is no longer the only
//! thing standing between a shipped document and that shape -- `check`
//! itself is. The input-default guard stays: a default is not a
//! `for_each` key, and `describe` already masks it. Read-only over
//! every document under `workflows/` and `workflows/fixtures/` --
//! this test writes no new fixture there, since the characterization
//! snapshot
//! (`crates/willikins-dsl/tests/it/snapshots/it__acceptance__characterization_of_every_document.snap`)
//! must stay byte-identical for this task (acceptance 21).
//!
//! The describe-default test below is the complement: it builds its own
//! throwaway document in a temp directory, declaring an identifier-typed
//! input with a document default, and drives the real `willikins`
//! binary exactly as `tests/it/identifier_masking.rs` does. `describe`'s
//! `missing_input_renders_an_identifier_default_masked` (milestone 3i
//! task B4, `crates/willikins-core/src/describe.rs`) already pins the
//! mechanism `MissingInput::default` would use; this test instead proves
//! the actual, reachable route -- an input with a default is never
//! `missing` (it resolves), so its masked rendering comes from the same
//! `resolved` -> `value_text` -> `Value::display(Disclosure)` path every
//! other resolved value takes.

use std::path::PathBuf;
use std::process::{Command, Output};

use willikins_core::TypeRef;
use willikins_dsl::load_document;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Every `workflows/*.yaml` and `workflows/fixtures/*.yaml` path, as a full
/// filesystem path. Mirrors `crates/willikins-dsl/tests/it/acceptance.rs`'s
/// own `every_document_path`, redefined here rather than shared because
/// that one is a private helper of a different crate's test binary.
fn every_document_path() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut paths = Vec::new();
    for dir in ["workflows", "workflows/fixtures"] {
        let full = root.join(dir);
        let mut entries: Vec<_> = std::fs::read_dir(&full)
            .unwrap_or_else(|err| panic!("{}: {err}", full.display()))
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "yaml"))
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        paths.extend(entries);
    }
    paths
}

fn is_identifier(ty: &TypeRef) -> bool {
    willikins_types::registry()
        .get(&ty.name)
        .is_some_and(|entry| entry.info.identifier)
}

// ---------------------------------------------------------------------
// Guard: no shipped document declares an identifier-typed input
// default.
// ---------------------------------------------------------------------

#[test]
fn no_shipped_document_declares_an_identifier_typed_input_default() {
    let mut saw_a_default = false;

    for path in every_document_path() {
        let Ok(workflow) = load_document(&path) else {
            continue;
        };
        for (name, spec) in &workflow.inputs {
            if spec.default.is_none() {
                continue;
            }
            saw_a_default = true;
            assert!(
                !is_identifier(&spec.ty),
                "{}: input `{name}` declares an identifier-typed default (`{}`)",
                path.display(),
                spec.ty
            );
        }
    }

    assert!(
        saw_a_default,
        "no shipped document declares any input default at all; this guard would otherwise pass vacuously"
    );
}

// ---------------------------------------------------------------------
// `describe` renders an identifier-typed input *default* masked, and
// whole under --reveal -- the end-to-end complement of
// `crates/willikins-core/src/describe.rs`'s
// `missing_input_renders_an_identifier_default_masked`.
// ---------------------------------------------------------------------

const FULL_ISSUER_ID: &str = "57246542-96fe-1a63-e053-0824d011072a";
const MASKED_ISSUER_ID: &str = "5724...";

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
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

/// A throwaway document, written to a temp directory rather than
/// `workflows/fixtures/`: both guards above, and the characterization
/// test, iterate every file under that directory, and acceptance 21
/// requires that snapshot to stay byte-identical for this task.
fn write_temp_document() -> PathBuf {
    let workflow_yaml = format!(
        r#"
name: identifier-default-guard
description: An input whose document default is an identifier type, for B7's describe test.
inputs:
  issuer:
    type: AppleIssuerId
    default: "{FULL_ISSUER_ID}"
steps:
  names:
    tool: naming.v1
    with:
      org: lightless-labs
      slug: demo
outputs:
  issuer_out: ${{{{ inputs.issuer }}}}
"#
    );
    let dir = std::env::temp_dir().join(format!(
        "willikins-cli-prerendered-guard-{}-identifier-default-guard",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("identifier-default-guard.yaml");
    std::fs::write(&path, workflow_yaml).unwrap();
    path
}

#[test]
fn describe_masks_an_identifier_typed_input_default_and_reveal_shows_it_whole() {
    let path = write_temp_document();

    let output = run(&["describe", path.to_str().unwrap()]);
    assert_eq!(exit_code(&output), 0, "stderr: {}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!("issuer: {MASKED_ISSUER_ID}")),
        "text: {text}"
    );
    assert!(!text.contains(FULL_ISSUER_ID), "text: {text}");

    let revealed = run(&["--reveal", "describe", path.to_str().unwrap()]);
    assert_eq!(exit_code(&revealed), 0, "stderr: {}", stderr(&revealed));
    let revealed_text = stdout(&revealed);
    assert!(
        revealed_text.contains(&format!("issuer: {FULL_ISSUER_ID}")),
        "text: {revealed_text}"
    );

    let json_output = run(&["--json", "describe", path.to_str().unwrap()]);
    assert_eq!(
        exit_code(&json_output),
        0,
        "stderr: {}",
        stderr(&json_output)
    );
    let raw = stdout(&json_output);
    assert!(!raw.contains(FULL_ISSUER_ID), "raw: {raw}");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(
        json["resolved"]["issuer"]["value"],
        serde_json::json!(MASKED_ISSUER_ID)
    );
    assert_eq!(
        json["resolved"]["issuer"]["masked"],
        serde_json::json!(true)
    );
}
