//! [`scan_directory`]: the one non-recursive scan of the trusted workflow
//! directory that backs both [`crate::Butler::start`] (task 10a part A)
//! and [`crate::Butler::list_workflows`] -- one function, two callers, so
//! the two can never disagree about what the directory holds or how a
//! broken entry in it is reported.
//!
//! **Symlinks are refused**, not followed: a `.yaml`/`.yml` entry whose
//! final path component is a symlink could point at content outside the
//! directory the operator vetted as the trusted checkout, so
//! [`scan_directory`] refuses the whole scan with [`StartupError::Symlink`]
//! the moment it finds one, the same decision
//! `crate::document::load_named_document` makes for a single named lookup
//! (see that module's own doc). Checked with `std::fs::symlink_metadata`
//! /`DirEntry::file_type`, neither of which follows the final component --
//! unlike `Path::is_file`, which would happily read straight through a
//! symlink and make this refusal a no-op.
//!
//! **Non-recursive.** A subdirectory (`workflows/fixtures/` under the
//! real trusted directory, say) is silently skipped rather than descended
//! into, so the directory's own negative fixtures -- several of which do
//! not even parse -- never have to live anywhere else. Only `.yaml` and
//! `.yml` files at the top level are scanned, matching
//! `crate::document::load_named_document`'s own `.yaml`-then-`.yml`
//! fallback: a `.yml` document is validated at startup exactly as a
//! `.yaml` one is, or `apply`'s later reload (which accepts either
//! extension) could run a document that was never checked at startup.

use std::path::{Path, PathBuf};

use willikins_core::{Catalog, CheckError, Checked, Workflow, check};
use willikins_dsl::DocumentError;
use willikins_journal::DocumentSha256;
use willikins_types::{DomainType, WorkflowName};

/// Why [`scan_directory`] (and so [`crate::Butler::start`] or
/// [`crate::Butler::list_workflows`]) refused, naming the file.
///
/// Serializes internally tagged (`#[serde(tag = "kind")]`), the same
/// convention every other error in this workspace follows; pinned by
/// `tests::every_variant_is_represented_and_serializes_with_its_kind`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum StartupError {
    /// The directory itself (or one entry's metadata inside it) could not
    /// be read.
    Directory {
        /// The directory or entry that failed.
        path: PathBuf,
        /// The underlying I/O message.
        message: String,
    },
    /// A `.yaml`/`.yml` entry is a symlink; refused rather than followed.
    Symlink {
        /// The symlinked entry.
        path: PathBuf,
    },
    /// The file's name (without its extension) is not a valid
    /// [`WorkflowName`].
    InvalidName {
        /// The offending file.
        path: PathBuf,
        /// Why the stem was rejected.
        message: String,
    },
    /// The file parsed, but its own `name:` does not match its filename
    /// stem.
    NameMismatch {
        /// The file.
        path: PathBuf,
        /// The name its filename stem implied.
        expected: WorkflowName,
        /// The name the document itself declares.
        found: WorkflowName,
    },
    /// The file failed to parse.
    Document {
        /// The file.
        path: PathBuf,
        /// The parse failure.
        error: DocumentError,
    },
    /// The file parsed but failed `check` against the catalog.
    Check {
        /// The file.
        path: PathBuf,
        /// Every failure.
        errors: Vec<CheckError>,
    },
    /// Recording `ServerStarted` in the journal failed.
    Journal {
        /// What went wrong.
        message: String,
    },
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Directory { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
            Self::Symlink { path } => {
                write!(f, "{}: a symlinked document is refused", path.display())
            }
            Self::InvalidName { path, message } => write!(f, "{}: {message}", path.display()),
            Self::NameMismatch {
                path,
                expected,
                found,
            } => write!(
                f,
                "{}: expected a document named `{expected}`, found `{found}`",
                path.display()
            ),
            Self::Document { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Check { path, errors } => {
                write!(
                    f,
                    "{}: the document fails check: {} error(s)",
                    path.display(),
                    errors.len()
                )
            }
            Self::Journal { message } => write!(f, "journal: {message}"),
        }
    }
}

impl std::error::Error for StartupError {}

/// One document [`scan_directory`] loaded and validated.
pub struct Loaded {
    /// Its path under the trusted directory.
    pub path: PathBuf,
    /// Its filename stem, parsed -- equal to `workflow.name` by
    /// construction (a mismatch is a [`StartupError::NameMismatch`]).
    pub name: WorkflowName,
    /// The document bytes' content hash.
    pub document_sha256: DocumentSha256,
    /// The parsed document.
    pub workflow: Workflow,
    /// The result of `check`ing it against the catalog.
    pub checked: Checked,
}

/// Scan `dir` non-recursively for `.yaml`/`.yml` documents, in filename
/// order (deterministic, so [`crate::Butler::start`]'s recorded
/// `workflow_hashes` and [`crate::Butler::list_workflows`]'s listing never
/// depend on a directory's own iteration order), parsing and `check`ing
/// each one against `catalog`.
///
/// The first entry that fails refuses the whole scan, naming the file --
/// see the module docs for the symlink and non-recursion rules, and
/// [`StartupError`]'s variants for exactly what "fails" covers.
///
/// # Errors
///
/// See [`StartupError`].
pub fn scan_directory(dir: &Path, catalog: &Catalog) -> Result<Vec<Loaded>, StartupError> {
    let mut candidates = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|err| StartupError::Directory {
        path: dir.to_path_buf(),
        message: err.to_string(),
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| StartupError::Directory {
            path: dir.to_path_buf(),
            message: err.to_string(),
        })?;
        let path = entry.path();
        let is_yaml = matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yaml" | "yml")
        );
        if !is_yaml {
            continue;
        }
        let file_type = entry.file_type().map_err(|err| StartupError::Directory {
            path: path.clone(),
            message: err.to_string(),
        })?;
        if file_type.is_symlink() {
            return Err(StartupError::Symlink { path });
        }
        if !file_type.is_file() {
            continue;
        }
        candidates.push(path);
    }
    candidates.sort();

    let mut loaded = Vec::with_capacity(candidates.len());
    for path in candidates {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        let name = WorkflowName::parse(stem).map_err(|error| StartupError::InvalidName {
            path: path.clone(),
            message: error.to_string(),
        })?;

        let workflow = willikins_dsl::load_document(&path).map_err(|error| {
            StartupError::Document {
                path: path.clone(),
                error,
            }
        })?;
        if workflow.name != name {
            return Err(StartupError::NameMismatch {
                path,
                expected: name,
                found: workflow.name,
            });
        }

        let bytes = std::fs::read(&path).map_err(|err| StartupError::Directory {
            path: path.clone(),
            message: err.to_string(),
        })?;

        let checked = check(&workflow, catalog).map_err(|errors| StartupError::Check {
            path: path.clone(),
            errors,
        })?;

        loaded.push(Loaded {
            document_sha256: DocumentSha256::compute(&bytes),
            path,
            name,
            workflow,
            checked,
        });
    }
    Ok(loaded)
}

/// One declared input, summarised for [`WorkflowSummary::inputs`].
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct InputSummary {
    /// The input's name.
    pub name: willikins_core::InputName,
    /// Its declared type.
    #[serde(rename = "type")]
    pub ty: willikins_core::TypeRef,
    /// Whether a caller must supply it (it has no declared default).
    pub required: bool,
}

/// One entry of [`crate::Butler::list_workflows`]'s result: enough to let
/// an agent pick a workflow and see what it needs, without loading the
/// document itself.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct WorkflowSummary {
    /// The workflow's name.
    pub name: WorkflowName,
    /// Its own one-line description, verbatim from the document -- see
    /// the "Document text is data" trust boundary; this is document text,
    /// not willikins' own words.
    pub document_description: Option<willikins_types::Description>,
    /// Every declared input, in declaration order.
    pub inputs: Vec<InputSummary>,
}

impl From<Loaded> for WorkflowSummary {
    fn from(loaded: Loaded) -> Self {
        Self {
            name: loaded.name,
            document_description: loaded.workflow.description,
            inputs: loaded
                .workflow
                .inputs
                .into_iter()
                .map(|(name, spec)| InputSummary {
                    name,
                    required: spec.default.is_none(),
                    ty: spec.ty,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    fn empty_catalog() -> Catalog {
        Catalog::new(willikins_types::registry())
    }

    const DOC: &str = "name: foo\ndescription: a workflow\nsteps: {}\n";

    #[test]
    fn an_empty_directory_scans_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = scan_directory(dir.path(), &empty_catalog()).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn scans_yaml_and_yml_in_filename_order() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "bar.yml", "name: bar\ndescription: b\nsteps: {}\n");
        write(dir.path(), "foo.yaml", DOC);
        let loaded = scan_directory(dir.path(), &empty_catalog()).unwrap();
        let names: Vec<&str> = loaded.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["bar", "foo"]);
    }

    #[test]
    fn a_subdirectory_is_skipped_not_descended_into() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "foo.yaml", DOC);
        std::fs::create_dir(dir.path().join("fixtures")).unwrap();
        write(&dir.path().join("fixtures"), "broken.yaml", "not: [valid");
        let loaded = scan_directory(dir.path(), &empty_catalog()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name.as_str(), "foo");
    }

    #[test]
    fn a_non_yaml_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "foo.yaml", DOC);
        write(dir.path(), "README.md", "not a workflow");
        let loaded = scan_directory(dir.path(), &empty_catalog()).unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn a_filename_that_is_not_a_valid_workflow_name_refuses_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Not_Valid.yaml", "name: x\ndescription: d\nsteps: {}\n");
        match scan_directory(dir.path(), &empty_catalog()) {
            Err(StartupError::InvalidName { path, .. }) => {
                assert_eq!(path.file_name().unwrap(), "Not_Valid.yaml");
            }
            other => panic!("expected InvalidName, got {other:?}", other = debug_kind(&other)),
        }
    }

    #[test]
    fn an_internal_name_mismatch_refuses_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "foo.yaml", "name: bar\ndescription: d\nsteps: {}\n");
        match scan_directory(dir.path(), &empty_catalog()) {
            Err(StartupError::NameMismatch { expected, found, .. }) => {
                assert_eq!(expected.as_str(), "foo");
                assert_eq!(found.as_str(), "bar");
            }
            other => panic!("expected NameMismatch, got {other:?}", other = debug_kind(&other)),
        }
    }

    #[test]
    fn a_document_that_fails_to_parse_refuses_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "foo.yaml", "not: [valid");
        assert!(matches!(
            scan_directory(dir.path(), &empty_catalog()),
            Err(StartupError::Document { .. })
        ));
    }

    #[test]
    fn a_document_that_fails_check_refuses_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        // References an undeclared tool, so `check` fails.
        write(
            dir.path(),
            "foo.yaml",
            "name: foo\ndescription: d\nsteps:\n  a:\n    tool: no.such.tool\n",
        );
        assert!(matches!(
            scan_directory(dir.path(), &empty_catalog()),
            Err(StartupError::Check { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_document_refuses_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "real.yaml", DOC);
        std::os::unix::fs::symlink(outside.path().join("real.yaml"), dir.path().join("foo.yaml"))
            .unwrap();
        assert!(matches!(
            scan_directory(dir.path(), &empty_catalog()),
            Err(StartupError::Symlink { .. })
        ));
    }

    fn debug_kind(result: &Result<Vec<Loaded>, StartupError>) -> String {
        match result {
            Ok(_) => "Ok".to_string(),
            Err(err) => format!("{err:?}"),
        }
    }
}
