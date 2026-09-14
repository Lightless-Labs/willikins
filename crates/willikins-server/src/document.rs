//! Load a named workflow document from the trusted directory.
//!
//! The lookup convention is the filename stem: `plan("foo", ..)` looks
//! for `foo.yaml` (then `foo.yml`) directly under the configured
//! directory -- *not* a scan of every file's own internal `name:` field,
//! which would have to tolerate the directory's deliberately-broken
//! fixtures (a negative fixture such as `cycle.yaml` fails `check`, and
//! several fail to parse at all) just to build a name index.
//!
//! `willikins_core::check::check` refuses a document whose own `name:`
//! does not match the file it was found under, at *plan* time -- with
//! [`LoadError::NameMismatch`], which [`crate::Butler::plan`] reports as
//! [`crate::ButlerError::UnknownWorkflow`] (the directory does not, in
//! the sense that matters, hold a document *named* `foo` at `foo.yaml`).
//! Catching this at `plan` rather than only at `apply`'s reload is what
//! keeps `PlanRecord::workflow` and the document's own internal name
//! always equal for an untampered file, so `apply`'s later re-check of
//! the same equality is meaningful only for a file that changed underfoot
//! between `plan` and `apply` -- exactly the case
//! `docs/research/2026-09-14-executor-journal-adversarial-pass-1.md`'s
//! plan-identity attack needs caught.

use std::path::{Path, PathBuf};

use willikins_core::Workflow;
use willikins_dsl::DocumentError;
use willikins_journal::DocumentSha256;
use willikins_types::WorkflowName;

/// Why [`load_named_document`] could not produce a `(DocumentSha256,
/// Workflow)` pair.
#[derive(Debug)]
pub enum LoadError {
    /// Neither `<name>.yaml` nor `<name>.yml` exists under the directory
    /// as a plain file.
    NotFound,
    /// `<name>.yaml` (or `.yml`) exists but is a symlink. Refused rather
    /// than followed: see this module's docs' "Symlinks are refused"
    /// section. Every current caller (`Butler::plan`, `Butler::apply`'s
    /// reload, the read operations) collapses this to a generic refusal
    /// without naming the path; kept for this module's own tests and a
    /// future caller that wants it in a log line.
    Symlink(#[allow(dead_code)] PathBuf),
    /// The file exists but failed to parse.
    Document(DocumentError),
    /// The file parsed, but its own `name:` is not `name`.
    NameMismatch {
        /// What the document itself claims to be named. Every current
        /// caller collapses this variant to a generic refusal
        /// (`ButlerError::UnknownWorkflow` from `plan`,
        /// `ButlerError::DocumentChanged` from `apply`'s reload) without
        /// naming it, so nothing outside this module's own tests reads
        /// it yet; kept for a future caller that wants the specific
        /// mismatch in an error message or a log line.
        #[allow(dead_code)]
        found: WorkflowName,
    },
}

/// `<dir>/<name>.yaml`, or `<dir>/<name>.yml` if the former does not
/// exist as a plain file. `Err(None)` if neither exists at all; `Err(Some(path))`
/// if the first candidate that exists on disk is a symlink (refused, not
/// followed -- see the module docs).
fn workflow_path(dir: &Path, name: &WorkflowName) -> Result<PathBuf, Option<PathBuf>> {
    for ext in ["yaml", "yml"] {
        let candidate = dir.join(format!("{name}.{ext}"));
        // `symlink_metadata` does not follow the final component, unlike
        // `Path::is_file`/`std::fs::metadata` -- this is what lets a
        // symlink be told apart from a plain file at all.
        match std::fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(Some(candidate)),
            Ok(meta) if meta.is_file() => return Ok(candidate),
            _ => {}
        }
    }
    Err(None)
}

/// Load, hash, and parse the document named `name` under `dir`, checking
/// that its own internal `name:` matches.
///
/// **Symlinks are refused.** A symlink named `<name>.yaml` under the
/// trusted directory could point at content outside it -- content the
/// operator never vetted as part of the trusted checkout -- so it is
/// refused with [`LoadError::Symlink`] rather than followed, exactly as
/// [`crate::startup::scan_directory`] refuses one found during the
/// startup scan. This is a deliberate, conservative decision: a legitimate
/// use of a symlinked document (sharing one document across two names, say)
/// is not supported by this milestone; add it explicitly later if needed,
/// rather than silently following an in-directory link today.
///
/// # Errors
///
/// See [`LoadError`].
pub fn load_named_document(
    dir: &Path,
    name: &WorkflowName,
) -> Result<(DocumentSha256, Workflow), LoadError> {
    let path = workflow_path(dir, name).map_err(|symlink| match symlink {
        Some(path) => LoadError::Symlink(path),
        None => LoadError::NotFound,
    })?;
    let workflow = willikins_dsl::load_document(&path).map_err(LoadError::Document)?;
    if &workflow.name != name {
        return Err(LoadError::NameMismatch {
            found: workflow.name.clone(),
        });
    }
    // A second, independent read purely for the digest: `load_document`
    // already validated the byte cap and read the file once to parse it;
    // re-reading rather than threading the raw bytes through its own
    // return keeps this module out of `willikins-dsl`'s internals (its
    // `DocumentError` constructors are private to that crate) at the
    // cost of one extra small local read, on a file that was just proven
    // readable a moment ago.
    let bytes = std::fs::read(&path).map_err(|_| LoadError::NotFound)?;
    Ok((DocumentSha256::compute(&bytes), workflow))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    fn wf(name: &str) -> WorkflowName {
        use willikins_types::DomainType;
        WorkflowName::parse(name).unwrap()
    }

    const DOC: &str = "name: foo\ndescription: a workflow\nsteps: {}\n";

    #[test]
    fn finds_a_yaml_file_by_stem() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "foo.yaml", DOC);
        let (sha, workflow) = load_named_document(dir.path(), &wf("foo")).unwrap();
        assert_eq!(workflow.name.as_str(), "foo");
        assert_eq!(sha, DocumentSha256::compute(DOC.as_bytes()));
    }

    #[test]
    fn falls_back_to_a_yml_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "foo.yml", DOC);
        let (_sha, workflow) = load_named_document(dir.path(), &wf("foo")).unwrap();
        assert_eq!(workflow.name.as_str(), "foo");
    }

    #[test]
    fn missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            load_named_document(dir.path(), &wf("foo")),
            Err(LoadError::NotFound)
        ));
    }

    #[test]
    fn a_document_whose_internal_name_differs_is_a_name_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "foo.yaml",
            "name: bar\ndescription: x\nsteps: {}\n",
        );
        match load_named_document(dir.path(), &wf("foo")) {
            Err(LoadError::NameMismatch { found }) => assert_eq!(found.as_str(), "bar"),
            other => panic!("expected NameMismatch, got {}", matches_label(&other)),
        }
    }

    fn matches_label(result: &Result<(DocumentSha256, Workflow), LoadError>) -> &'static str {
        match result {
            Ok(_) => "Ok",
            Err(LoadError::NotFound) => "NotFound",
            Err(LoadError::Symlink(_)) => "Symlink",
            Err(LoadError::Document(_)) => "Document",
            Err(LoadError::NameMismatch { .. }) => "NameMismatch",
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_document_is_refused_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "real.yaml", DOC);
        std::os::unix::fs::symlink(
            outside.path().join("real.yaml"),
            dir.path().join("foo.yaml"),
        )
        .unwrap();
        match load_named_document(dir.path(), &wf("foo")) {
            Err(LoadError::Symlink(path)) => {
                assert_eq!(path, dir.path().join("foo.yaml"));
            }
            other => panic!("expected Symlink, got {}", matches_label(&other)),
        }
    }

    #[test]
    fn a_malformed_document_is_a_document_error() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "foo.yaml", "not: [valid, workflow");
        assert!(matches!(
            load_named_document(dir.path(), &wf("foo")),
            Err(LoadError::Document(_))
        ));
    }
}
