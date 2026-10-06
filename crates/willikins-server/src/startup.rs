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

use willikins_core::{Catalog, CheckError, Checked, Workflow, check, link};
use willikins_dsl::DocumentError;
use willikins_journal::DocumentSha256;
use willikins_types::{DomainType, WorkflowName};

use crate::document::TrustedResolver;

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
    /// The file parsed, but linking its `uses:` tree (milestone 2b,
    /// `willikins_core::compose::link`) failed -- a cycle, an unknown or
    /// unparsable child, or a boundary refusal, each resolved against
    /// this same trusted directory (decision (d8): "children resolve in
    /// the trusted directory"; §2.3: "a cycle refuses startup"). Checked
    /// before `check` itself, since `check` refuses outright
    /// ([`CheckError::Unlinked`]) on a document that still has an
    /// unlinked `uses:` step.
    Compose {
        /// The top-level document whose own `link` call failed -- always
        /// one of this scan's own candidates, even when the actual
        /// defect sits inside a transitively-resolved child (the linker
        /// walks the whole tree from this document down).
        path: PathBuf,
        /// Every error the linker returned.
        errors: Vec<CheckError>,
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
            Self::Compose { path, errors } => {
                // `link` is first-error-wins (milestone 2b, task L2's own
                // doc), so this is normally exactly one error -- but every
                // one is joined, never only the count, so a cycle's own
                // `CheckError::UsesCycle` Display (which names the chain)
                // actually reaches whoever reads this message, matching
                // acceptance 9's "refuses startup, naming the chain".
                write!(f, "{}: fails to link: ", path.display())?;
                for (index, error) in errors.iter().enumerate() {
                    if index > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{error}")?;
                }
                Ok(())
            }
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
    /// The parsed document, exactly as authored -- unlinked: its own
    /// `uses:` steps (if any) are still on [`Workflow::uses`], never
    /// expanded. Kept this way (rather than the flattened graph
    /// [`Self::checked`] holds) so a caller reading `workflow.inputs` or
    /// `workflow.uses` sees only this document's own authored surface,
    /// never a used document's fixed inputs or renamed nodes.
    pub workflow: Workflow,
    /// The result of `check`ing this document's *linked* graph (milestone
    /// 2b, decision (d8)): [`Checked::workflow`] is the flattened
    /// workflow [`willikins_core::link`] produced, not [`Self::workflow`]
    /// unchanged -- `check` refuses outright on one that still has an
    /// unlinked `uses:` step.
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

        let workflow =
            willikins_dsl::load_document(&path).map_err(|error| StartupError::Document {
                path: path.clone(),
                error,
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

        // Milestone 2b, decision (d8): link this document's own `uses:`
        // tree against the trusted directory itself (every document a
        // root names resolves right here, by name, in this same `dir`)
        // before `check` ever sees it -- `check` refuses outright
        // (`CheckError::Unlinked`) on a document that still has one.
        // This is what makes a cross-document cycle, an unknown or
        // unparsable child, or a boundary refusal fail *startup*, naming
        // this top-level document, rather than surfacing only once
        // something later tries to `plan`/`apply` it.
        let mut resolver = TrustedResolver::new(dir);
        let linked = link(&workflow, &mut |used| resolver.resolve(used)).map_err(|errors| {
            StartupError::Compose {
                path: path.clone(),
                errors,
            }
        })?;

        let checked = check(&linked.workflow, catalog).map_err(|errors| StartupError::Check {
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
    // Milestone 2b: never includes a *fixed* input
    // (`InputSpec::fixed_by.is_some()`) -- not that one could reach here,
    // since this is built from `Loaded::workflow`, the authored and
    // still-unlinked document (see that field's own doc), which never
    // carries a fixed input at all; those only exist on the *linked* flat
    // graph. The filter is kept anyway so this type's own contract does
    // not depend on which of `Loaded`'s two workflows happens to feed it
    // today. A plain comment, not a doc comment, so the published
    // `mcp_server` schema's description of this field stays unchanged
    // (the "Published shapes" gate rule: additions only).
    pub inputs: Vec<InputSummary>,
    /// This workflow's own direct children -- every `uses:` step's
    /// workflow name, in declaration order -- never a used document's own
    /// children. Empty, and omitted from the wire, for a document with no
    /// `uses:` step. `list_workflows`' `composes` (decision (d11)); the
    /// shas for these names are already in `ServerStarted.workflow_hashes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<WorkflowName>,
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
                .filter(|(_, spec)| spec.fixed_by.is_none())
                .map(|(name, spec)| InputSummary {
                    name,
                    required: spec.default.is_none(),
                    ty: spec.ty,
                })
                .collect(),
            uses: loaded
                .workflow
                .uses
                .values()
                .map(|uses| uses.workflow.clone())
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
        write(
            dir.path(),
            "bar.yml",
            "name: bar\ndescription: b\nsteps: {}\n",
        );
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
        write(
            dir.path(),
            "Not_Valid.yaml",
            "name: x\ndescription: d\nsteps: {}\n",
        );
        match scan_directory(dir.path(), &empty_catalog()) {
            Err(StartupError::InvalidName { path, .. }) => {
                assert_eq!(path.file_name().unwrap(), "Not_Valid.yaml");
            }
            other => panic!(
                "expected InvalidName, got {other:?}",
                other = debug_kind(&other)
            ),
        }
    }

    #[test]
    fn an_internal_name_mismatch_refuses_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "foo.yaml",
            "name: bar\ndescription: d\nsteps: {}\n",
        );
        match scan_directory(dir.path(), &empty_catalog()) {
            Err(StartupError::NameMismatch {
                expected, found, ..
            }) => {
                assert_eq!(expected.as_str(), "foo");
                assert_eq!(found.as_str(), "bar");
            }
            other => panic!(
                "expected NameMismatch, got {other:?}",
                other = debug_kind(&other)
            ),
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

    // -------------------------------------------------------------
    // Composition (milestone 2b, task S1, decision (d8)): `scan_directory`
    // links every top-level document against the directory itself before
    // `check`ing it.
    // -------------------------------------------------------------

    #[test]
    fn a_cross_document_cycle_refuses_startup_naming_the_document_and_chain() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "a.yaml",
            "name: a\nsteps:\n  to_b:\n    uses: b\n",
        );
        write(
            dir.path(),
            "b.yaml",
            "name: b\nsteps:\n  to_a:\n    uses: a\n",
        );
        // `scan_directory`'s `Ok` side (`Vec<Loaded>`) has no `Debug`, so
        // this is matched directly rather than through `.unwrap_err()`
        // (which needs one even on the never-taken `Ok` arm).
        let Err(error) = scan_directory(dir.path(), &empty_catalog()) else {
            panic!("a cross-document cycle must refuse the scan");
        };
        // The chain itself must actually reach whoever reads the message
        // -- acceptance 9's "refuses startup, naming the chain" -- not
        // just a count of how many errors `link` returned.
        let message = error.to_string();
        assert!(message.contains("a -> b -> a"), "{message}");
        match error {
            StartupError::Compose { path, errors } => {
                assert_eq!(path.file_name().unwrap(), "a.yaml");
                match errors.as_slice() {
                    [CheckError::UsesCycle { chain }] => {
                        let names: Vec<&str> = chain.iter().map(WorkflowName::as_str).collect();
                        assert_eq!(names, ["a", "b", "a"]);
                    }
                    other => panic!("expected exactly one UsesCycle, got {other:?}"),
                }
            }
            other => panic!("expected Compose/UsesCycle, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_child_refuses_startup_naming_the_document() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "root.yaml",
            "name: root\nsteps:\n  child:\n    uses: ghost\n",
        );
        match scan_directory(dir.path(), &empty_catalog()) {
            Err(StartupError::Compose { path, errors }) => {
                assert_eq!(path.file_name().unwrap(), "root.yaml");
                assert!(
                    matches!(errors.as_slice(), [CheckError::UnknownWorkflow { .. }]),
                    "{errors:?}"
                );
            }
            other => panic!(
                "expected Compose/UnknownWorkflow, got {}",
                debug_kind(&other)
            ),
        }
    }

    #[test]
    fn a_child_that_fails_to_parse_refuses_startup_as_used_document() {
        let dir = tempfile::tempdir().unwrap();
        // "root" sorts before "z-broken", so `root.yaml` is scanned (and
        // linked) first -- the resolver's own attempt to load `z-broken`
        // is what must fail here, not `scan_directory`'s own top-level
        // parse of `z-broken.yaml` as a candidate in its own right (which
        // would report a plain `StartupError::Document` instead, naming
        // the wrong thing and never exercising the linker's own
        // `ResolveFailure::Document` -> `CheckError::UsedDocument` path).
        write(dir.path(), "z-broken.yaml", "not: [valid");
        write(
            dir.path(),
            "root.yaml",
            "name: root\nsteps:\n  child:\n    uses: z-broken\n",
        );
        match scan_directory(dir.path(), &empty_catalog()) {
            Err(StartupError::Compose { path, errors }) => {
                assert_eq!(path.file_name().unwrap(), "root.yaml");
                assert!(
                    matches!(errors.as_slice(), [CheckError::UsedDocument { .. }]),
                    "{errors:?}"
                );
            }
            other => panic!("expected Compose/UsedDocument, got {}", debug_kind(&other)),
        }
    }

    #[test]
    fn a_document_using_a_trusted_sibling_links_before_check() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "child.yaml", "name: child\nsteps: {}\n");
        write(
            dir.path(),
            "root.yaml",
            "name: root\nsteps:\n  c:\n    uses: child\n",
        );
        let loaded = scan_directory(dir.path(), &empty_catalog()).unwrap();
        let names: Vec<&str> = loaded.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["child", "root"]);

        let root = loaded
            .iter()
            .find(|entry| entry.name.as_str() == "root")
            .unwrap();
        // The root's own authored document still names its `uses:` step
        // unexpanded (`Loaded::workflow`'s own doc).
        assert!(!root.workflow.uses.is_empty());
        // ...but `check` ran against the *linked*, flattened graph, which
        // has no `uses:` left at all.
        assert!(root.checked.workflow.uses.is_empty());
    }

    #[test]
    fn workflow_summary_reports_uses_for_a_composite_and_omits_it_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "child.yaml", "name: child\nsteps: {}\n");
        write(
            dir.path(),
            "root.yaml",
            "name: root\nsteps:\n  c:\n    uses: child\n",
        );
        let loaded = scan_directory(dir.path(), &empty_catalog()).unwrap();
        let summaries: Vec<WorkflowSummary> =
            loaded.into_iter().map(WorkflowSummary::from).collect();

        let child = summaries
            .iter()
            .find(|summary| summary.name.as_str() == "child")
            .unwrap();
        assert!(child.uses.is_empty());
        assert_eq!(
            serde_json::to_value(child).unwrap().get("uses"),
            None,
            "an empty `uses` is omitted from the wire entirely"
        );

        let root = summaries
            .iter()
            .find(|summary| summary.name.as_str() == "root")
            .unwrap();
        assert_eq!(
            root.uses
                .iter()
                .map(WorkflowName::as_str)
                .collect::<Vec<_>>(),
            ["child"]
        );
    }

    #[test]
    fn workflow_summary_excludes_a_fixed_input() {
        // Hand-built, not scanned: `scan_directory`'s own `Loaded::workflow`
        // is always the *unlinked*, authored document (its own doc comment),
        // which never carries a fixed input at all -- only the linker ever
        // sets `InputSpec::fixed_by`. This pins `WorkflowSummary::from`'s own
        // filter directly, the way `describe.rs`'s
        // `checked_with_a_fixed_input` pins `describe` against the same
        // hand-built shape the linker would actually produce.
        let workflow = Workflow::new(willikins_types::WorkflowName::parse("fixture").unwrap())
            .input(
                willikins_core::InputName::parse("slug").unwrap(),
                willikins_core::InputSpec::new(
                    willikins_core::TypeRef::parse("ProjectSlug").unwrap(),
                ),
            )
            .input(
                willikins_core::InputName::parse("org/base_configs").unwrap(),
                {
                    let mut spec = willikins_core::InputSpec::new(
                        willikins_core::TypeRef::parse("list<DopplerConfig>").unwrap(),
                    )
                    .with_default(willikins_core::Value::known_list(vec![
                        willikins_types::DopplerConfig::parse("shared/base").unwrap(),
                    ]));
                    spec.fixed_by = Some(willikins_core::NodeName::parse("org").unwrap());
                    spec
                },
            );
        let catalog = empty_catalog();
        let checked = check(&workflow, &catalog).expect("no nodes: nothing to fail check");
        let loaded = Loaded {
            path: PathBuf::from("fixture.yaml"),
            name: workflow.name.clone(),
            document_sha256: DocumentSha256::compute(b""),
            workflow,
            checked,
        };

        let summary = WorkflowSummary::from(loaded);
        let names: Vec<&str> = summary
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["slug"],
            "the fixed input `org/base_configs` is excluded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_document_refuses_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "real.yaml", DOC);
        std::os::unix::fs::symlink(
            outside.path().join("real.yaml"),
            dir.path().join("foo.yaml"),
        )
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
