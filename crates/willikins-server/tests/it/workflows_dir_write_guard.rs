//! Decision 25 (`docs/plans/2026-09-16-milestone-2c-authorization.md`,
//! task 21): "nothing in the server writes under `WILLIKINS_WORKFLOWS_DIR`",
//! held as a written rule rather than only a reading of the code, the
//! same way `no_gh_writes_guard.rs` and `sink_token_guard.rs` hold their
//! own invariants. Only the operator ever writes the trusted directory,
//! by hand, from their own machine (decision 25's "the operator's
//! procedure, human only"); a server that could write it too would be a
//! second way documents reach a deployment, never audited the way the
//! operator's own publish procedure is.
//!
//! A plain text scan, not a `syn`-AST walk like the two guards above:
//! this is a narrower claim (one identifier, one crate's `src/`) and a
//! text match is the shape decision 25's own wording asks for ("no file
//! ... that names `workflows_dir` calls a write API"). It does not
//! follow an alias or a fully-qualified `<Type>::method()` call the way
//! `sink_token_guard.rs` does; see
//! `a_known_safe_use_of_workflows_dir_does_not_trip_the_guard` for what
//! today's tree actually looks like under it.

use std::path::{Path, PathBuf};

/// Every API name decision 25 names as a write, in the form it is
/// actually written at a call site in this workspace's style (a method
/// call, or a fully-qualified free function call).
const WRITE_API_PATTERNS: &[&str] = &[
    "std::fs::write(",
    "File::create(",
    ".create(true)",
    ".write(true)",
    "std::fs::rename(",
    ".rename(",
    "std::fs::remove_file(",
    ".remove_file(",
    "std::fs::remove_dir_all(",
    ".remove_dir_all(",
    "std::fs::create_dir_all(",
    ".create_dir_all(",
];

fn server_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir`, recursively.
fn all_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|err| panic!("reading {}: {err}", dir.display()));
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            all_rs_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Whether `source` names the identifier `workflows_dir` as a whole word
/// -- not merely as a substring of a longer identifier such as
/// `my_workflows_directory`.
fn names_workflows_dir(source: &str) -> bool {
    let needle = "workflows_dir";
    let bytes = source.as_bytes();
    let mut start = 0;
    while let Some(offset) = source[start..].find(needle) {
        let at = start + offset;
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        let after = at + needle.len();
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        start = at + needle.len();
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

/// Every write-API pattern found in `source`, each paired with the
/// pattern that matched.
fn write_api_hits(source: &str) -> Vec<&'static str> {
    WRITE_API_PATTERNS
        .iter()
        .copied()
        .filter(|pattern| source.contains(pattern))
        .collect()
}

#[test]
fn no_file_naming_workflows_dir_calls_a_write_api() {
    let mut files = Vec::new();
    all_rs_files(&server_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected to find source files to scan");

    let mut violations = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        if !names_workflows_dir(&source) {
            continue;
        }
        for hit in write_api_hits(&source) {
            violations.push(format!("{}: {hit}", path.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "a file naming `workflows_dir` must never call a write API; found:\n{}",
        violations.join("\n")
    );
}

// -------------------------------------------------------------
// `names_workflows_dir` / `write_api_hits` unit tests: proof the guard
// actually catches something, independent of the real tree (which has
// zero violations today).
// -------------------------------------------------------------

#[test]
fn names_workflows_dir_matches_the_whole_word() {
    assert!(names_workflows_dir("pub workflows_dir: PathBuf,"));
    assert!(names_workflows_dir("self.workflows_dir"));
    assert!(names_workflows_dir("workflows_dir"));
}

#[test]
fn names_workflows_dir_does_not_match_a_longer_identifier() {
    assert!(!names_workflows_dir("my_workflows_directory_other"));
    assert!(!names_workflows_dir("workflows_dir_but_longer"));
    assert!(!names_workflows_dir("not related text"));
}

#[test]
fn write_api_hits_finds_every_named_pattern() {
    let source = "std::fs::write(p, b) ; File::create(p) ; opts.create(true) ; \
                  opts.write(true) ; std::fs::rename(a, b) ; h.rename(a, b) ; \
                  std::fs::remove_file(p) ; h.remove_file(p) ; \
                  std::fs::remove_dir_all(p) ; h.remove_dir_all(p) ; \
                  std::fs::create_dir_all(p) ; h.create_dir_all(p)";
    let hits = write_api_hits(source);
    assert_eq!(hits.len(), WRITE_API_PATTERNS.len(), "{hits:?}");
}

#[test]
fn write_api_hits_is_empty_for_read_only_source() {
    let source = "std::fs::read(p); File::open(p); handle.open_regular(name)";
    assert!(write_api_hits(source).is_empty());
}

/// The guard's premise, pinned directly: today's actual
/// `workflows_dir`-naming sites really are read-only, so the main test
/// above is not vacuously green because nothing in the scanned tree ever
/// names `workflows_dir` at all.
#[test]
fn a_known_safe_use_of_workflows_dir_does_not_trip_the_guard() {
    let config_rs = server_src_dir().join("config.rs");
    let source = std::fs::read_to_string(&config_rs).unwrap();
    assert!(
        names_workflows_dir(&source),
        "expected {} to name `workflows_dir` (this test's premise)",
        config_rs.display()
    );
    assert!(
        write_api_hits(&source).is_empty(),
        "expected {} to call no write API: {:?}",
        config_rs.display(),
        write_api_hits(&source)
    );
}
