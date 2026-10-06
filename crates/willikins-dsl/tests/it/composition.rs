//! Milestone 2b, part P and acceptance 2
//! (`docs/plans/2026-10-05-milestone-2b-composition.md`): the `uses:`
//! keyword in the document format, and its four negative fixtures under
//! `workflows/fixtures/composition/`. L2 (the linker's refusals) extends
//! this file with the fixtures a document can express for the linker's
//! own errors.

use std::path::Path;

use willikins_dsl::{DocumentErrorKind, load_document};

fn fixture_path(relative: &str) -> String {
    format!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../{}"),
        format!("workflows/fixtures/composition/{relative}")
    )
}

/// Load `fixture` and return the `(path, message)` of the
/// [`DocumentErrorKind::Semantic`] error it must fail with -- panicking
/// (naming the fixture) if it loads, or if it fails any other way.
fn semantic_error(fixture: &str) -> (String, String) {
    let path = fixture_path(fixture);
    let err = load_document(Path::new(&path))
        .err()
        .unwrap_or_else(|| panic!("{fixture}: expected to fail to load"));
    match err.kind {
        DocumentErrorKind::Semantic { path, message } => (path, message),
        other => panic!("{fixture}: expected a Semantic error, got {other:?}"),
    }
}

/// `workflows/fixtures/composition/uses-and-tool.yaml`'s header.
#[test]
fn uses_and_tool_together_fails_to_load_with_one_located_error() {
    let (path, message) = semantic_error("uses-and-tool.yaml");
    assert_eq!(path, "steps.a");
    assert_eq!(
        message,
        "a step may declare exactly one of `tool` or `uses`, not both"
    );
}

/// `workflows/fixtures/composition/uses-neither.yaml`'s header.
#[test]
fn uses_neither_fails_to_load_with_one_located_error() {
    let (path, message) = semantic_error("uses-neither.yaml");
    assert_eq!(path, "steps.a");
    assert_eq!(
        message,
        "a step must declare exactly one of `tool` or `uses`"
    );
}

/// `workflows/fixtures/composition/uses-for-each.yaml`'s header.
#[test]
fn uses_for_each_fails_to_load_with_one_located_error() {
    let (path, message) = semantic_error("uses-for-each.yaml");
    assert_eq!(path, "steps.a.for_each");
    assert_eq!(
        message,
        "a `uses:` step may not have a `for_each` (milestone 2b decision (d12)); use a \
         separate root document per item instead"
    );
}

/// `workflows/fixtures/composition/uses-bad-name.yaml`'s header.
#[test]
fn uses_bad_name_fails_to_load_with_one_located_error() {
    let (path, message) = semantic_error("uses-bad-name.yaml");
    assert_eq!(path, "steps.a.uses");
    assert_eq!(message, "does not match the required pattern");
}

/// Milestone 2b, SHARED VALUES, "Authored names": the DSL refuses a `/`
/// in a `with:` key under `uses:`, the same as it already refuses one in
/// a step key or an input name (P1).
#[test]
fn a_slash_in_a_uses_with_key_is_a_semantic_error_at_the_with_path() {
    let err = willikins_dsl::parse_document(
        "\
name: demo
steps:
  org:
    uses: example-org
    with:
      base/configs: literal
",
    )
    .unwrap_err();
    match err.kind {
        DocumentErrorKind::Semantic { path, message } => {
            assert_eq!(path, "steps.org.with.base/configs");
            assert!(message.contains('/'), "{message}");
        }
        other => panic!("expected a Semantic error, got {other:?}"),
    }
}
