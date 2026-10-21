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
//!
//! **Milestone 2c, decision 25: handle-based opens.** On a writable
//! volume the trusted directory can change underneath a running server
//! (an operator re-publishes the document set), so [`TrustedDirectory`]
//! opens `WILLIKINS_WORKFLOWS_DIR` exactly once, with
//! `O_DIRECTORY | O_NOFOLLOW` (refusing a symlinked directory outright,
//! never following it), and every document it reads is opened *relative
//! to that same handle* with `openat(O_NOFOLLOW)` -- never by
//! re-joining and re-resolving a path from scratch, which is what would
//! leave a check-then-use window between deciding a path is safe and
//! actually opening it. [`load_named_document`] opens one handle for its
//! one lookup, and [`TrustedResolver`] opens its own single handle for a
//! whole `link` call's closure of children -- each is "open once", never
//! once per document inside the same operation (the pre-2c resolver
//! re-opened the directory by path for every `uses:` child it resolved).

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rustix::fd::OwnedFd;
use rustix::fs::{Mode, OFlags, openat};
use rustix::io::Errno;

use willikins_core::Workflow;
use willikins_core::compose::ResolveFailure;
use willikins_dsl::DocumentError;
use willikins_journal::DocumentSha256;
use willikins_types::WorkflowName;

/// Why [`TrustedDirectory::open`] could not open the trusted directory
/// itself.
#[derive(Debug)]
pub enum DirOpenError {
    /// The directory is a symlink -- refused rather than followed (see
    /// the module docs).
    Symlink,
    /// Any other failure (missing, not a directory, permission denied,
    /// ...), carrying the underlying message.
    Io(String),
}

/// Why [`TrustedDirectory::open_regular`] could not produce a readable
/// file for one entry.
#[derive(Debug)]
pub enum EntryOpenError {
    /// The entry is a symlink -- refused rather than followed.
    Symlink,
    /// Any other failure opening or `fstat`ing the entry once found.
    Io(String),
}

/// Whether `path` itself (its final component, not what it resolves to)
/// is a symlink -- `lstat`, never `stat`. Used only to classify an
/// already-failed `open` (see [`TrustedDirectory::open`]'s doc), never to
/// decide whether to open something: that decision is always `O_NOFOLLOW`
/// at the actual `open`/`openat` call, so this check being racy against a
/// concurrent swap cannot itself let a symlink through.
fn dir_is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink())
}

/// An open handle on the trusted workflow directory (milestone 2c,
/// decision 25). See the module docs for why every read goes through
/// one of these rather than a fresh path-based open.
pub struct TrustedDirectory {
    fd: OwnedFd,
    dir: PathBuf,
}

impl TrustedDirectory {
    /// Open `dir` once, with `O_DIRECTORY | O_NOFOLLOW`: refuses
    /// [`DirOpenError::Symlink`] if `dir` itself is a symlink, rather
    /// than following it to whatever directory it points at.
    ///
    /// **Platform note:** combining `O_DIRECTORY` with `O_NOFOLLOW` on a
    /// symlinked `dir` reports `ELOOP` on Linux but `ENOTDIR` on macOS
    /// (checked directly: a Python `os.open` repro on this host's
    /// Darwin kernel). Neither code alone is trustworthy on its own --
    /// `ENOTDIR` alone could just as well mean `dir` is a plain file,
    /// nothing to do with a symlink -- so a failed open with either
    /// falls back to one `symlink_metadata` (`lstat`, no `open`) on
    /// `dir` to tell the two apart. This adds no race: the open already
    /// failed and nothing was opened through the symlink either way;
    /// the `lstat` only decides which error variant to report.
    ///
    /// # Errors
    ///
    /// See [`DirOpenError`].
    pub fn open(dir: &Path) -> Result<Self, DirOpenError> {
        match rustix::fs::open(
            dir,
            OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::RDONLY,
            Mode::empty(),
        ) {
            Ok(fd) => Ok(Self {
                fd,
                dir: dir.to_path_buf(),
            }),
            Err(Errno::LOOP | Errno::NOTDIR) if dir_is_symlink(dir) => Err(DirOpenError::Symlink),
            Err(err) => Err(DirOpenError::Io(err.to_string())),
        }
    }

    /// The directory this handle was opened from -- for building a
    /// display path only; never re-opened or re-resolved by this path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.dir
    }

    /// Every `.yaml`/`.yml` entry's bare filename, in sorted order --
    /// the same deterministic order [`crate::startup::scan_directory`]
    /// always walked. A non-matching extension, a dotfile, `.` and `..`
    /// are all filtered out here, by name alone, before anything is
    /// opened; this makes no claim about what kind of entry each name
    /// names (a plain file, a directory, a symlink) -- that is decided
    /// only once [`Self::open_regular`] actually opens it.
    ///
    /// # Errors
    ///
    /// [`DirOpenError::Io`] if the directory stream cannot be read (the
    /// directory itself is already open and cannot have become a
    /// symlink after [`Self::open`] succeeded, so [`DirOpenError::Symlink`]
    /// is unreachable here).
    pub fn yaml_entries(&self) -> Result<Vec<PathBuf>, DirOpenError> {
        let stream = rustix::fs::Dir::read_from(&self.fd)
            .map_err(|err| DirOpenError::Io(err.to_string()))?;
        let mut names = Vec::new();
        for entry in stream {
            let entry = entry.map_err(|err| DirOpenError::Io(err.to_string()))?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            let path = PathBuf::from(name);
            let is_yaml = matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("yaml" | "yml")
            );
            if is_yaml {
                names.push(path);
            }
        }
        names.sort();
        Ok(names)
    }

    /// Open `filename` (a bare name, no separators) relative to this
    /// handle's own descriptor with `openat(O_NOFOLLOW)`, never by
    /// re-joining and re-resolving `self.dir` plus `filename` as a path.
    ///
    /// `Ok(None)` when no such entry exists, or one exists but is not a
    /// regular file once opened (a subdirectory, say) -- the same
    /// "does not count, move on" outcome a path-based lookup gave for
    /// either case, which callers that try more than one extension
    /// (`.yaml` then `.yml`) rely on to keep looking.
    ///
    /// `O_NONBLOCK` is set on the open itself: a regular file ignores it,
    /// but without it a `.yaml`-named FIFO placed in the directory would
    /// block this call (and so the whole scan or lookup) forever, which
    /// `O_NOFOLLOW` alone does not prevent (a FIFO is not a symlink).
    ///
    /// # Errors
    ///
    /// [`EntryOpenError::Symlink`] when `filename` is a symlink;
    /// [`EntryOpenError::Io`] for any other failure to open or `fstat`
    /// it.
    pub fn open_regular(&self, filename: &OsStr) -> Result<Option<std::fs::File>, EntryOpenError> {
        match openat(
            &self.fd,
            filename,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(fd) => {
                let file = std::fs::File::from(fd);
                match file.metadata() {
                    Ok(metadata) if metadata.is_file() => Ok(Some(file)),
                    Ok(_) => Ok(None),
                    Err(err) => Err(EntryOpenError::Io(err.to_string())),
                }
            }
            Err(Errno::NOENT) => Ok(None),
            Err(Errno::LOOP) => Err(EntryOpenError::Symlink),
            Err(err) => Err(EntryOpenError::Io(err.to_string())),
        }
    }

    /// Load, hash, and parse the document named `name` relative to this
    /// handle, checking that its own internal `name:` matches. Tries
    /// `<name>.yaml` then `<name>.yml`, exactly as the pre-2c, path-based
    /// lookup did.
    ///
    /// # Errors
    ///
    /// See [`LoadError`].
    pub fn load_named(&self, name: &WorkflowName) -> Result<(DocumentSha256, Workflow), LoadError> {
        for ext in ["yaml", "yml"] {
            let filename = format!("{name}.{ext}");
            match self.open_regular(OsStr::new(&filename)) {
                Ok(Some(file)) => {
                    let path = self.dir.join(&filename);
                    // One read: `load_document_from_file` hashes exactly
                    // the bytes it parses from this same descriptor, so a
                    // file replaced between this open and a later read
                    // can never record a digest that does not match the
                    // document actually parsed (the property milestone
                    // 2b's adversarial pass already required of the
                    // path-based version; now true by construction since
                    // there is no second read at all).
                    let (workflow, bytes) = willikins_dsl::load_document_from_file(file, &path)
                        .map_err(LoadError::Document)?;
                    if &workflow.name != name {
                        return Err(LoadError::NameMismatch {
                            found: workflow.name,
                        });
                    }
                    return Ok((DocumentSha256::compute(&bytes), workflow));
                }
                Err(EntryOpenError::Symlink) => {
                    return Err(LoadError::Symlink(self.dir.join(&filename)));
                }
                // Same permissiveness the old `symlink_metadata`-based
                // lookup had: no entry, or any other failure to open this
                // candidate, just moves on to the next extension.
                Ok(None) | Err(EntryOpenError::Io(_)) => {}
            }
        }
        Err(LoadError::NotFound)
    }
}

/// Why [`load_named_document`] could not produce a `(DocumentSha256,
/// Workflow)` pair.
#[derive(Debug)]
pub enum LoadError {
    /// Neither `<name>.yaml` nor `<name>.yml` exists under the directory
    /// as a plain file -- or the trusted directory itself could not be
    /// opened for any reason other than being a symlink (collapsed to
    /// the same outcome: from a single named lookup's point of view, a
    /// directory that cannot be read does not hold a document named
    /// `name` either).
    NotFound,
    /// `<name>.yaml` (or `.yml`) exists but is a symlink, or the trusted
    /// directory itself is one. Refused rather than followed: see the
    /// module docs' handle-based-opens section. Every current caller
    /// (`Butler::plan`, `Butler::apply`'s reload, the read operations)
    /// collapses this to a generic refusal without naming the path; kept
    /// for this module's own tests and a future caller that wants it in
    /// a log line.
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

/// Open the trusted directory `dir` once and load the document named
/// `name` from it -- every `Butler` call site's own root lookup. A
/// sibling [`TrustedResolver`], built separately over the same `dir` to
/// resolve `name`'s `uses:` tree (if it has one), opens its own handle in
/// turn ([`TrustedResolver::new`]); the two are not the same descriptor.
/// Both still satisfy the "no re-resolution by path" property this
/// module's docs describe -- each opens exactly once for its own
/// lifetime -- at the cost of one extra `open` syscall per lookup over
/// sharing a single handle between the two, which this milestone does
/// not build.
///
/// # Errors
///
/// See [`LoadError`].
pub fn load_named_document(
    dir: &Path,
    name: &WorkflowName,
) -> Result<(DocumentSha256, Workflow), LoadError> {
    let handle = TrustedDirectory::open(dir).map_err(|err| match err {
        DirOpenError::Symlink => LoadError::Symlink(dir.to_path_buf()),
        DirOpenError::Io(_) => LoadError::NotFound,
    })?;
    handle.load_named(name)
}

/// A [`willikins_core::compose::link`] resolver rooted at one trusted
/// directory -- milestone 2b decision (d8): "children resolve in the
/// trusted directory through `load_named_document`". Every `uses:` step
/// `link` asks this to resolve goes through this resolver's own
/// [`TrustedDirectory`], opened once ([`Self::new`]) and reused for
/// every occurrence within the same `link` call, never re-opened by
/// path.
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
pub struct TrustedResolver {
    /// `None` when the trusted directory itself could not be opened at
    /// all. Carried this way, rather than failing eagerly in [`Self::new`],
    /// so a workflow with no `uses:` step at all -- which never calls
    /// [`Self::resolve`] -- is never refused just because the directory
    /// happened to be unreadable or swapped to a symlink; a workflow
    /// that *does* have a `uses:` step sees the failure the first time
    /// [`Self::resolve`] is actually called, reported as
    /// [`ResolveFailure::NotFound`] -- indistinguishable, which is
    /// exactly the existing "a missing child" outcome.
    dir: Option<TrustedDirectory>,
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

impl TrustedResolver {
    /// A fresh resolver rooted at `dir`, having resolved nothing yet.
    /// Opens `dir` once, right away (see [`Self::dir`]'s own doc for why
    /// a failure here is not reported until [`Self::resolve`] actually
    /// needs it).
    #[must_use]
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: TrustedDirectory::open(dir).ok(),
            shas: BTreeMap::new(),
            cache: BTreeMap::new(),
            reads: 0,
        }
    }

    /// Resolve `name` the way [`willikins_core::compose::link`]'s own
    /// `resolve` callback expects: [`TrustedDirectory::load_named`] on
    /// the first call for `name`, served from [`Self`]'s own cache on
    /// every later one (see the struct docs), and recording the loaded
    /// document's sha on success either way.
    ///
    /// # Errors
    ///
    /// [`ResolveFailure::NotFound`] when no such document exists, or the
    /// trusted directory itself could not be opened at all;
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
        let Some(dir) = &self.dir else {
            return Err(ResolveFailure::NotFound);
        };
        match dir.load_named(name) {
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
    fn a_missing_directory_is_not_found_not_a_distinct_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(matches!(
            load_named_document(&missing, &wf("foo")),
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

    #[cfg(unix)]
    #[test]
    fn a_symlinked_trusted_directory_is_refused_not_followed() {
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "foo.yaml", DOC);
        let parent = tempfile::tempdir().unwrap();
        let link = parent.path().join("workflows");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        match load_named_document(&link, &wf("foo")) {
            Err(LoadError::Symlink(path)) => assert_eq!(path, link),
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

    /// A workflow with no `uses:` step never calls [`TrustedResolver::resolve`]
    /// at all (`link`'s own behaviour), so a resolver built over a
    /// directory that cannot even be opened must still exist without
    /// erroring eagerly -- [`TrustedResolver::new`] never fails.
    #[test]
    fn a_resolver_over_an_unreadable_directory_builds_fine_until_asked_to_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let mut resolver = TrustedResolver::new(&missing);
        assert_eq!(
            resolver.resolve(&wf("anything")).unwrap_err(),
            ResolveFailure::NotFound
        );
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

    // -------------------------------------------------------------
    // TrustedDirectory (milestone 2c, decision 25)
    // -------------------------------------------------------------

    #[test]
    fn yaml_entries_lists_only_yaml_and_yml_in_sorted_order() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "bar.yml", DOC);
        write(dir.path(), "foo.yaml", DOC);
        write(dir.path(), "README.md", "not a workflow");
        std::fs::create_dir(dir.path().join("fixtures")).unwrap();
        let handle = TrustedDirectory::open(dir.path()).unwrap();
        let names: Vec<String> = handle
            .yaml_entries()
            .unwrap()
            .into_iter()
            .map(|p| p.to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["bar.yml", "foo.yaml"]);
    }

    #[test]
    fn open_regular_returns_none_for_a_missing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let handle = TrustedDirectory::open(dir.path()).unwrap();
        assert!(matches!(
            handle.open_regular(OsStr::new("missing.yaml")),
            Ok(None)
        ));
    }

    #[test]
    fn open_regular_returns_none_for_a_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub.yaml")).unwrap();
        let handle = TrustedDirectory::open(dir.path()).unwrap();
        assert!(matches!(
            handle.open_regular(OsStr::new("sub.yaml")),
            Ok(None)
        ));
    }

    fn doc_named(name: &str) -> String {
        format!("name: {name}\ndescription: a workflow\nsteps: {{}}\n")
    }
}
