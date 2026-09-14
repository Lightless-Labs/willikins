//! Adversarial pass 1 (acceptance test 19, first pass; task 9), the one
//! attack of that pass whose target is a *rendered line* rather than the
//! executor or the journal: a document that spells willikins' own
//! redaction marker out as plain text.
//!
//! Recorded in
//! `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`; the
//! rest of the pass lives in `willikins-core`'s `tests/apply_adversarial.rs`
//! and `willikins-journal`'s `tests/adversarial_pass_1.rs`.

use std::path::PathBuf;
use std::process::{Command, Output};

/// The marker a redacted `DopplerServiceToken` renders as, and the exact
/// string `workflows/fixtures/redaction-marker-default.yaml` puts in a
/// plain `Text` default.
const MARKER: &str = "[REDACTED DopplerServiceToken]";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("workflows")
        .join("fixtures")
        .join("redaction-marker-default.yaml")
}

fn willikins(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_willikins"))
        .args(args)
        .output()
        .expect("failed to run the willikins binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A document may put willikins' own redaction marker in a non-secret
/// value, and the text renderer has nothing to say about it: the marker
/// holds no character `single_line` escapes, so a forged one prints
/// exactly like a real one. Nothing leaks -- there is no secret in this
/// document at all -- but a human reading `plan`'s text cannot tell "this
/// value is hidden from you" from "this value *is* those characters".
///
/// The JSON surface, which is what an agent reads, *can* tell them apart:
/// a genuinely redacted value carries `"redacted": true` next to its
/// marker and this one carries no `redacted` key at all. Pinned here, and
/// handed to pass 2 as the renderer question (a text mode that marks
/// redaction structurally -- quoting a document-supplied value, say --
/// rather than by the marker's spelling).
#[test]
fn a_document_can_spell_the_redaction_marker_in_text_but_not_forge_it_in_json() {
    let path = fixture();
    let path = path.to_str().expect("the fixture path is UTF-8");

    let text = willikins(&["plan", path]);
    assert_eq!(text.status.code(), Some(0), "{}", stdout(&text));
    assert!(
        stdout(&text).contains(MARKER),
        "the forged marker prints as itself: {}",
        stdout(&text)
    );

    let json = willikins(&["plan", path, "--json"]);
    assert_eq!(json.status.code(), Some(0), "{}", stdout(&json));
    let body = stdout(&json);
    assert!(
        body.contains(MARKER),
        "the value is carried verbatim: {body}"
    );
    assert!(
        !body.contains("\"redacted\""),
        "and is never marked redacted, which is what tells the two apart: {body}"
    );
}
