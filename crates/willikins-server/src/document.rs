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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use willikins_core::Workflow;
use willikins_core::compose::ResolveFailure;
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

/// A [`willikins_core::compose::link`] resolver rooted at one trusted
/// directory -- milestone 2b decision (d8): "children resolve in the
/// trusted directory through `load_named_document`". Every `uses:` step
/// `link` asks this to resolve goes through [`load_named_document`]
/// against [`Self`]'s own `dir`, so a child is found exactly where trust
/// boundary 1 says it must be: by name, in the parent's own directory,
/// never a path, a symlink, or a body-supplied document.
///
/// Every document this resolver successfully loads is recorded by name in
/// [`Self::shas`] -- the running closure a `link` call built up so far.
/// `Butler::plan` (task S2) reads it once `link` returns `Ok`, to fill
/// `PlanRecorded.used` (decision (d10)); this task's own callers
/// ([`crate::startup::scan_directory`], `Butler::validate`/`describe`)
/// only care that `link` itself succeeded or failed, and do not read it
/// yet.
///
/// **Caches a successful load by name** (X1, the adversarial pass):
/// [`willikins_core::compose::link`] calls `resolve` once per
/// *occurrence* of a `uses:` step, with no memoization of its own -- a
/// diamond's second reference to the same child, or simply many `uses:`
/// steps naming the same document, each trigger their own call. Without
/// this cache, each occurrence re-reads and re-parses the file from
/// disk, so a caller-controlled root (`validate`'s body, over the
/// network) referencing one real, trusted-but-sparse document (few tool
/// nodes, so [`MAX_LINKED_NODES`](willikins_core::compose::MAX_LINKED_NODES)
/// never trips) many times forces repeated full re-reads of up to
/// [`willikins_dsl::MAX_DOCUMENT_BYTES`] each -- an expansion bomb that
/// does real I/O and parsing work before any refusal fires, bounded only
/// by the number of `uses:` steps a 256 KiB body can hold. Caching here
/// is sound because one `link` call is one synchronous snapshot of the
/// trusted directory: every occurrence of the same name within it must
/// see the same bytes regardless, so reading them once is not merely
/// faster but more consistent than re-reading mid-walk. `used` (the
/// linker's own multiset of flattened node occurrences, decision (d9))
/// is unaffected: `link` still calls `resolve` -- and therefore `flatten`
/// -- once per occurrence; only the disk read and the YAML parse
/// underneath it are shared. See `tests::a_repeated_name_is_read_from_disk_once`.
pub struct TrustedResolver<'a> {
    dir: &'a Path,
    shas: BTreeMap<WorkflowName, DocumentSha256>,
    // `BTreeMap`, not `HashMap`: `WorkflowName` (never secret) has a
    // hand-written `Ord`/`PartialOrd` for exactly this reason (milestone
    // 2b, task J1's addendum), but `#[derive(DomainType)]` deliberately
    // never derives `Hash` for any domain type -- a secret one hashed
    // into a `HashMap` could leak its length through bucket placement,
    // and the macro has no way to tell a secret type from this one at
    // the point it decides which traits to emit.
    cache: BTreeMap<WorkflowName, (DocumentSha256, Workflow)>,
    reads: usize,
}

impl<'a> TrustedResolver<'a> {
    /// A fresh resolver rooted at `dir`, having resolved nothing yet.
    #[must_use]
    pub fn new(dir: &'a Path) -> Self {
        Self {
            dir,
            shas: BTreeMap::new(),
            cache: BTreeMap::new(),
            reads: 0,
        }
    }

    /// Resolve `name` the way [`willikins_core::compose::link`]'s own
    /// `resolve` callback expects: [`load_named_document`] under this
    /// resolver's directory on the first call for `name`, served from
    /// [`Self`]'s own cache on every later one (see the struct docs), and
    /// recording the loaded document's sha on success either way.
    ///
    /// # Errors
    ///
    /// [`ResolveFailure::NotFound`] when no such document exists;
    /// [`ResolveFailure::Refused`] for a symlinked or internally
    /// name-mismatched one -- deliberately not told apart, matching
    /// [`LoadError`]'s own docs, so a caller probing for either learns
    /// nothing a legitimate lookup would not; or [`ResolveFailure::Document`]
    /// when the named document exists but fails to parse. A failure is
    /// never cached: `link` is first-error-wins and aborts the whole call
    /// through `?` the moment one occurs, so a failed name is never asked
    /// for again within the same `link` call.
    pub fn resolve(&mut self, name: &WorkflowName) -> Result<Workflow, ResolveFailure> {
        if let Some((sha, workflow)) = self.cache.get(name) {
            self.shas.insert(name.clone(), sha.clone());
            return Ok(workflow.clone());
        }
        self.reads += 1;
        match load_named_document(self.dir, name) {
            Ok((sha, workflow)) => {
                self.shas.insert(name.clone(), sha.clone());
                self.cache.insert(name.clone(), (sha, workflow.clone()));
                Ok(workflow)
            }
            Err(LoadError::NotFound) => Err(ResolveFailure::NotFound),
            Err(LoadError::Symlink(_) | LoadError::NameMismatch { .. }) => {
                Err(ResolveFailure::Refused)
            }
            Err(LoadError::Document(error)) => Err(ResolveFailure::Document {
                message: error.to_string(),
            }),
        }
    }

    /// Every document this resolver has loaded so far, by name -- the
    /// closure a `link` call actually used.
    ///
    /// Read by task S2's `Butler::plan_inner` (to fill
    /// `PlanRecorded.used`, decision (d10)) and `Butler::reload_and_check`
    /// (to compare apply time's fresh closure against the one `plan`
    /// recorded, trust boundary 5): both build a fresh `TrustedResolver`,
    /// `link` through it, and read this map once `link` returns `Ok`.
    #[must_use]
    pub fn shas(&self) -> &BTreeMap<WorkflowName, DocumentSha256> {
        &self.shas
    }

    /// How many times this resolver actually read a document from disk,
    /// as opposed to serving a cached parse -- at most one per distinct
    /// name `resolve` was ever asked to load successfully (X1). Exists
    /// for this module's own cache test; a production caller has no
    /// present reason to read it, so `#[allow(dead_code)]` outside
    /// `cfg(test)` rather than a narrower visibility that would need
    /// widening the moment one does.
    #[must_use]
    #[allow(dead_code)]
    pub fn reads(&self) -> usize {
        self.reads
    }
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

    // -------------------------------------------------------------
    // TrustedResolver (milestone 2b, task S1, decision (d8))
    // -------------------------------------------------------------

    #[test]
    fn trusted_resolver_resolves_a_sibling_and_records_its_sha() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "child.yaml", &doc_named("child"));
        let mut resolver = TrustedResolver::new(dir.path());
        let workflow = resolver.resolve(&wf("child")).unwrap();
        assert_eq!(workflow.name.as_str(), "child");
        assert_eq!(
            resolver.shas().get(&wf("child")),
            Some(&DocumentSha256::compute(doc_named("child").as_bytes()))
        );
    }

    #[test]
    fn trusted_resolver_reports_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let mut resolver = TrustedResolver::new(dir.path());
        assert_eq!(
            resolver.resolve(&wf("missing")).unwrap_err(),
            ResolveFailure::NotFound
        );
        assert!(resolver.shas().is_empty());
    }

    /// X1 (the adversarial pass): repeatedly resolving the same name
    /// within one resolver reads the file from disk once, not once per
    /// call -- the fix for the expansion bomb the struct docs describe
    /// (a caller-controlled root with many `uses:` steps naming the same
    /// trusted-but-sparse document would otherwise force one full
    /// re-read and re-parse per occurrence, bounded only by how many
    /// `uses:` steps a 256 KiB body can hold, long before
    /// `MAX_LINKED_NODES` ever has a reason to refuse). Every call still
    /// returns the right workflow and records the sha, so `link`'s own
    /// per-occurrence behaviour (acceptance 4's diamond test, in
    /// `compose.rs`) is unaffected; only the disk read and the parse are
    /// shared.
    #[test]
    fn a_repeated_name_is_read_from_disk_once() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "child.yaml", &doc_named("child"));
        let mut resolver = TrustedResolver::new(dir.path());
        for _ in 0..50 {
            let workflow = resolver.resolve(&wf("child")).unwrap();
            assert_eq!(workflow.name.as_str(), "child");
        }
        assert_eq!(
            resolver.reads(),
            1,
            "50 occurrences of the same name must read the file once"
        );
        assert_eq!(
            resolver.shas().get(&wf("child")),
            Some(&DocumentSha256::compute(doc_named("child").as_bytes())),
            "the sha is still recorded even when served from the cache"
        );
    }

    /// The cache is per name: resolving two different documents still
    /// reads each of them once.
    #[test]
    fn two_different_names_are_each_read_once() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.yaml", &doc_named("a"));
        write(dir.path(), "b.yaml", &doc_named("b"));
        let mut resolver = TrustedResolver::new(dir.path());
        for _ in 0..10 {
            resolver.resolve(&wf("a")).unwrap();
            resolver.resolve(&wf("b")).unwrap();
        }
        assert_eq!(resolver.reads(), 2);
        assert_eq!(resolver.shas().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn trusted_resolver_refuses_a_symlinked_child_without_recording_it() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "real.yaml", &doc_named("child"));
        std::os::unix::fs::symlink(
            outside.path().join("real.yaml"),
            dir.path().join("child.yaml"),
        )
        .unwrap();
        let mut resolver = TrustedResolver::new(dir.path());
        assert_eq!(
            resolver.resolve(&wf("child")).unwrap_err(),
            ResolveFailure::Refused
        );
        assert!(resolver.shas().is_empty());
    }

    #[test]
    fn trusted_resolver_refuses_a_name_mismatched_child_as_refused() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "child.yaml", &doc_named("other"));
        let mut resolver = TrustedResolver::new(dir.path());
        assert_eq!(
            resolver.resolve(&wf("child")).unwrap_err(),
            ResolveFailure::Refused
        );
        assert!(resolver.shas().is_empty());
    }

    #[test]
    fn trusted_resolver_reports_a_malformed_child_as_document() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "child.yaml", "not: [valid, workflow");
        let mut resolver = TrustedResolver::new(dir.path());
        match resolver.resolve(&wf("child")) {
            Err(ResolveFailure::Document { .. }) => {}
            other => panic!("expected Document, got {other:?}"),
        }
    }

    fn doc_named(name: &str) -> String {
        format!("name: {name}\ndescription: a workflow\nsteps: {{}}\n")
    }
}
