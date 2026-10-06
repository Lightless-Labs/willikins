//! `github.scaffold.ensure`: lands (in memory) a set of files plus an
//! ownership marker on a branch, as `willikins-providers-github`'s live
//! tool of the same name does against a real commit. Milestone 3g, task
//! G2. Port table and behaviour must equal the live tool's field for
//! field (`tests/catalog_parity.rs`, `tests/scaffold_fake_agrees_with_live.rs`
//! in `willikins-providers-github`).
//!
//! **A repository that does not exist, or exists but is empty.**
//! Milestone 3l, task F1, mirroring decision (b)'s read table
//! (`docs/plans/2026-10-05-milestone-3l-new-repositories.md`) for the
//! cases this fake can model from `FakeState::github_repos`'
//! [`crate::state::GitHubRepoRecord::branches`]: no record at all
//! (`BranchExistence::RepositoryAbsent`); empty (`Some(vec![])`) with
//! `branch` equal to the record's own default
//! (`BranchExistence::Empty`); empty with a different `branch`
//! (`BranchExistence::Mismatch`, a `Conflict` naming both); a non-empty
//! `Some(list)` missing `branch` (`BranchExistence::MissingOnNonEmpty`,
//! the unchanged missing-branch `NotFound`); and `None`, legacy
//! "initialised, every branch exists" (`BranchExistence::Exists`,
//! unaffected by this check at all). `read` maps `RepositoryAbsent` and
//! `Empty` to [`Observation::Absent`], the same call decision (b) makes,
//! so a document that creates its own repository in the same plan still
//! plans cleanly. `ensure` on `Empty` lands the scaffold the same way it
//! always has (this fake has no network latency to retry against, so it
//! skips decision (a)'s own two-write split) and then records `branch`
//! on the repository's own record, so a second `ensure` sees
//! `BranchExistence::Exists` instead of re-entering this path.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use sha1::{Digest, Sha1};

use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolSpec, Value,
};
use willikins_types::{CommitHeadline, GitBranchName, GitHubRepo, RepoFile, RepoPath};

use crate::state::{FakeState, default_branch_or_main, repo_key, scaffold_key};
use crate::support::{
    conflict, exact, get, invalid, list, not_found, port, require_present, scalar, tool_name,
};

/// See `willikins_providers_github`'s live tool -- both crates share this
/// exact literal.
const MARKER_HEADER: &str = "managed-by: willikins";

/// See the live tool.
const MIN_FILES: usize = 1;

/// See the live tool.
const MAX_FILES: usize = 64;

/// This tool's own observation, carrying exactly what `ensure` needs.
enum ScaffoldState {
    Present,
    Foreign,
    Absent {
        /// Seed paths already present with byte-equal content: skipped
        /// when writing, mirroring the live tool's own convergence.
        already_equal: HashSet<String>,
    },
}

/// Milestone 3l, task F1: whether `branch` exists on `repo`, according to
/// `FakeState::github_repos`' own `branches` field -- this fake's model
/// of decision (b)'s read table, for the cases it can model. See this
/// module's own doc for the mapping to each read-table row.
enum BranchExistence {
    /// The branch exists: either a non-empty record whose `branches`
    /// names it, or a legacy record (`branches: None`, "every branch
    /// exists"). Proceed exactly as this tool always has, consulting
    /// only `FakeState::scaffolds`.
    Exists,
    /// The repository is empty (`branches: Some(vec![])`) and `branch`
    /// equals the record's own default.
    Empty { default_branch: GitBranchName },
    /// The repository is empty and `branch` does not equal the record's
    /// own default.
    Mismatch { default_branch: GitBranchName },
    /// The repository has at least one branch, but not the requested
    /// one.
    MissingOnNonEmpty,
    /// No record exists for this repository at all.
    RepositoryAbsent,
}

/// `github.scaffold.ensure`.
pub struct GitHubScaffoldEnsure {
    spec: ToolSpec,
    state: Arc<Mutex<FakeState>>,
}

impl GitHubScaffoldEnsure {
    /// This tool's own name, shared between its [`ToolSpec`] and the
    /// `"<tool>#<key>"` strings [`FakeState`]'s call counters and
    /// injected failures use.
    const TOOL_NAME: &'static str = "github.scaffold.ensure";

    /// Build the tool against `state`, constructing its spec -- the exact
    /// shape `tests/catalog_parity.rs` (in `willikins-providers-github`)
    /// pins equal to the live tool's.
    #[must_use]
    pub fn new(state: Arc<Mutex<FakeState>>) -> Self {
        let mut inputs = IndexMap::new();
        inputs.insert(port("repo"), exact("GitHubRepo", true));
        inputs.insert(port("branch"), exact("GitBranchName", true));
        inputs.insert(port("marker"), exact("RepoPath", true));
        inputs.insert(
            port("files"),
            PortSpec {
                ty: PortType::Exact(list("RepoFile")),
                required: true,
                derived_only: false,
            },
        );
        inputs.insert(port("message"), exact("CommitHeadline", true));
        inputs.insert(port("token"), exact("GitHubToken", false));
        let mut outputs = IndexMap::new();
        outputs.insert(port("repo"), scalar("GitHubRepo"));
        outputs.insert(port("branch"), scalar("GitBranchName"));
        outputs.insert(port("marker"), scalar("RepoPath"));
        Self {
            spec: ToolSpec {
                name: tool_name(Self::TOOL_NAME),
                description: "Ensure a set of files, plus an ownership marker, has landed on a \
                               branch as one commit."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("repo"), port("branch"), port("marker")],
                class: Class::Irreversible,
                pure: false,
            },
            state,
        }
    }

    /// The outputs this tool ever reports: pass-through only, exactly as
    /// the live tool's own.
    fn outputs_for(repo: &GitHubRepo, branch: &GitBranchName, marker: &RepoPath) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("repo"), Value::known(repo.clone()));
        outputs.insert(port("branch"), Value::known(branch.clone()));
        outputs.insert(port("marker"), Value::known(marker.clone()));
        outputs
    }

    /// Read the required `files` port as a list of [`RepoFile`]s --
    /// mirrors `repo.file.render`'s own `read_values` and the live
    /// tool's `read_files`.
    fn read_files(inputs: &Inputs) -> Result<Vec<RepoFile>, ToolError> {
        let value = inputs
            .get(&port("files"))
            .ok_or_else(|| invalid("port `files` is required"))?;
        if !value.is_known() {
            return Err(invalid("port `files` is unknown"));
        }
        let items = value
            .as_list()
            .ok_or_else(|| invalid("port `files` has an unexpected type"))?;
        items
            .iter()
            .map(|object| {
                willikins_types::downcast::<RepoFile>(object.as_ref())
                    .cloned()
                    .ok_or_else(|| invalid("port `files` has an unexpected type"))
            })
            .collect()
    }

    /// Every shape refusal the live tool makes before any state lookup at
    /// all.
    fn validate_shape(files: &[RepoFile], marker: &RepoPath) -> Result<(), ToolError> {
        if files.len() < MIN_FILES || files.len() > MAX_FILES {
            return Err(invalid(format!(
                "`files` must hold between {MIN_FILES} and {MAX_FILES} entries, but held {}",
                files.len()
            )));
        }
        let mut seen: HashSet<&str> = HashSet::with_capacity(files.len());
        for file in files {
            if !seen.insert(file.path().as_str()) {
                return Err(invalid(format!(
                    "path `{}` is declared more than once in `files`",
                    file.path()
                )));
            }
        }
        if seen.contains(marker.as_str()) {
            return Err(invalid(format!(
                "marker path `{marker}` must not also be one of `files`"
            )));
        }
        // A declared path that is a directory of another (two files, or a
        // file and the marker, either way round) cannot coexist in one
        // tree. Adversarial pass (render and write).
        // Walked in declaration order, so the message is deterministic.
        seen.insert(marker.as_str());
        let declared = files
            .iter()
            .map(|file| file.path().as_str())
            .chain(std::iter::once(marker.as_str()));
        for path in declared {
            if let Some((index, _)) = path
                .match_indices('/')
                .find(|(index, _)| seen.contains(&path[..*index]))
            {
                return Err(invalid(format!(
                    "path `{}` is declared as a file and also as a directory of `{path}`",
                    &path[..index]
                )));
            }
        }
        Ok(())
    }

    /// SHA-1 over `blob <byte length>\0<bytes>`, the same algorithm git
    /// (and `willikins_providers_github`'s own `git_blob_sha`) uses --
    /// duplicated here (`pub(crate)` there, a different crate here) so
    /// this fake's marker content is byte-identical to the live tool's,
    /// which `tests/scaffold_fake_agrees_with_live.rs` pins.
    fn blob_sha(content: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut hasher = Sha1::new();
        hasher.update(format!("blob {}\0", content.len()));
        hasher.update(content);
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(40);
        for byte in digest {
            let _ = write!(hex, "{byte:02x}");
        }
        hex
    }

    /// The marker file's content -- byte-identical to the live tool's own
    /// [SHARED VALUES format].
    fn marker_content(files: &[RepoFile]) -> String {
        let mut lines: Vec<(String, String)> = files
            .iter()
            .map(|file| {
                (
                    file.path().as_str().to_string(),
                    Self::blob_sha(file.content().as_bytes()),
                )
            })
            .collect();
        lines.sort_by(|a, b| a.0.cmp(&b.0));
        let mut content = String::from(MARKER_HEADER);
        content.push('\n');
        for (path, sha) in lines {
            content.push_str(&sha);
            content.push(' ');
            content.push_str(&path);
            content.push('\n');
        }
        content
    }

    /// Build and validate the marker [`RepoFile`], exactly mirroring the
    /// live tool's `validated_marker_file`: both inputs are fully known
    /// from `files` and `marker` alone, so a marker over `RepoFile`'s own
    /// length bound (64 files each near `RepoPath`'s own bound) is
    /// refused here, before any state lookup, on both `read` and
    /// `ensure` -- not only on the live side.
    fn validated_marker_file(files: &[RepoFile], marker: &RepoPath) -> Result<RepoFile, ToolError> {
        let content = Self::marker_content(files);
        RepoFile::new(marker.clone(), content)
            .map_err(|err| invalid(format!("marker file is invalid: {}", err.reason)))
    }

    /// The `Conflict` a foreign marker or an owned-but-differing seed path
    /// produces -- the same message shape as the live tool's.
    fn foreign_conflict(repo: &GitHubRepo, branch: &GitBranchName, marker: &RepoPath) -> ToolError {
        conflict(format!(
            "`{marker}` on `{repo}`@`{branch}` already exists and is not willikins' scaffold \
             marker; this tool will not overwrite it"
        ))
    }

    /// Milestone 3l, task F1: this fake's own model of decision (b)'s read
    /// table, from `state.github_repos` alone -- see this module's own
    /// doc for the mapping.
    fn observe_branch_existence(
        state: &FakeState,
        repo: &GitHubRepo,
        branch: &GitBranchName,
    ) -> BranchExistence {
        let Some(record) = state.github_repos.get(&repo_key(repo)) else {
            return BranchExistence::RepositoryAbsent;
        };
        let Some(branches) = &record.branches else {
            return BranchExistence::Exists;
        };
        if branches.contains(branch) {
            return BranchExistence::Exists;
        }
        if branches.is_empty() {
            let default_branch = default_branch_or_main(record);
            if *branch == default_branch {
                BranchExistence::Empty { default_branch }
            } else {
                BranchExistence::Mismatch { default_branch }
            }
        } else {
            BranchExistence::MissingOnNonEmpty
        }
    }

    /// The `NotFound` `ensure` reports for
    /// [`BranchExistence::RepositoryAbsent`] -- the same message the live
    /// tool's `repository_absent_error` produces (milestone 3l, SHARED
    /// VALUES "Repository-absent message (S3, `NotFound`)").
    fn repository_absent_error(repo: &GitHubRepo) -> ToolError {
        not_found(format!(
            "`{repo}` does not exist; this tool never creates a repository (github.repo.ensure \
             does)"
        ))
    }

    /// The unchanged missing-branch `NotFound`, for
    /// [`BranchExistence::MissingOnNonEmpty`] -- the same message decision
    /// (b)'s table keeps for a non-empty repository missing the named
    /// branch.
    fn missing_branch_error(repo: &GitHubRepo, branch: &GitBranchName) -> ToolError {
        not_found(format!(
            "branch `{branch}` does not exist on `{repo}`; this tool never creates one"
        ))
    }

    /// The `Conflict` for [`BranchExistence::Mismatch`] -- the same
    /// message the live tool's `default_branch_mismatch` produces
    /// (milestone 3l, SHARED VALUES "Default-branch mismatch (S2,
    /// `Conflict`)").
    fn default_branch_mismatch(
        repo: &GitHubRepo,
        branch: &GitBranchName,
        default_branch: &GitBranchName,
    ) -> ToolError {
        conflict(format!(
            "`{repo}` is empty, and its first commit can only land on its default branch \
             `{default_branch}`, not `{branch}`; name `{default_branch}` in this document, or \
             change the organisation's default branch name before the repository is created"
        ))
    }

    /// Whether `path`, itself absent from the flat `map`, is nonetheless
    /// occupied the way the live tool's tree walk sees it: some key lies
    /// beneath it (so it is a directory, the live tool's non-blob), or a
    /// proper ancestor of it is a key (so it sits beneath a file, the live
    /// tool's `UnderNonDirectory`). Adversarial pass (render and write).
    fn occupied_as_directory_or_beneath_a_file(
        map: &HashMap<String, String>,
        path: &RepoPath,
    ) -> bool {
        let path = path.as_str();
        let beneath_a_file = path
            .match_indices('/')
            .any(|(index, _)| map.contains_key(&path[..index]));
        let prefix = format!("{path}/");
        beneath_a_file || map.keys().any(|key| key.starts_with(&prefix))
    }

    /// Observe `entry` (the scaffold's current path-to-content map, or
    /// none at all) the way decision (b)'s read table describes, with
    /// string equality standing in for the live tool's blob sha
    /// comparison.
    fn observe(
        entry: Option<&HashMap<String, String>>,
        marker: &RepoPath,
        files: &[RepoFile],
    ) -> Result<ScaffoldState, ToolError> {
        let empty = HashMap::new();
        let map = entry.unwrap_or(&empty);
        if let Some(content) = map.get(marker.as_str()) {
            let first_line = content.split('\n').next().unwrap_or("");
            if first_line == MARKER_HEADER {
                return Ok(ScaffoldState::Present);
            }
            return Ok(ScaffoldState::Foreign);
        }
        if Self::occupied_as_directory_or_beneath_a_file(map, marker) {
            return Ok(ScaffoldState::Foreign);
        }
        let mut already_equal: HashSet<String> = HashSet::new();
        let mut conflicts: Vec<String> = Vec::new();
        for file in files {
            match map.get(file.path().as_str()) {
                None if Self::occupied_as_directory_or_beneath_a_file(map, file.path()) => {
                    conflicts.push(file.path().as_str().to_string());
                }
                None => {}
                Some(existing) if existing == file.content() => {
                    already_equal.insert(file.path().as_str().to_string());
                }
                Some(_) => conflicts.push(file.path().as_str().to_string()),
            }
        }
        if !conflicts.is_empty() {
            conflicts.sort();
            conflicts.dedup();
            return Err(conflict(format!(
                "the scaffold would overwrite existing, different content at: {}",
                conflicts.join(", ")
            )));
        }
        Ok(ScaffoldState::Absent { already_equal })
    }
}

impl Tool for GitHubScaffoldEnsure {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn read(&self, inputs: &Inputs) -> Result<Observation, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let branch: GitBranchName = get(inputs, "branch")?;
        let marker: RepoPath = get(inputs, "marker")?;
        let files = Self::read_files(inputs)?;
        Self::validate_shape(&files, &marker)?;
        Self::validated_marker_file(&files, &marker)?;
        let mut state = self.state.lock().unwrap();
        let key = scaffold_key(&repo, &branch);
        state.record_read_call(Self::TOOL_NAME, &key);
        match Self::observe_branch_existence(&state, &repo, &branch) {
            BranchExistence::RepositoryAbsent | BranchExistence::Empty { .. } => {
                return Ok(Observation::Absent {
                    predicted: Self::outputs_for(&repo, &branch, &marker),
                });
            }
            BranchExistence::Mismatch { default_branch } => {
                return Err(Self::default_branch_mismatch(
                    &repo,
                    &branch,
                    &default_branch,
                ));
            }
            BranchExistence::MissingOnNonEmpty => {
                return Err(Self::missing_branch_error(&repo, &branch));
            }
            BranchExistence::Exists => {}
        }
        match Self::observe(state.scaffolds.get(&key), &marker, &files)? {
            ScaffoldState::Present => Ok(Observation::Present(Self::outputs_for(
                &repo, &branch, &marker,
            ))),
            ScaffoldState::Foreign => Ok(Observation::Foreign),
            ScaffoldState::Absent { .. } => Ok(Observation::Absent {
                predicted: Self::outputs_for(&repo, &branch, &marker),
            }),
        }
    }

    fn ensure(&self, inputs: &Inputs, _token: &SinkToken) -> Result<Ensured, ToolError> {
        require_present(&self.spec, inputs)?;
        let repo: GitHubRepo = get(inputs, "repo")?;
        let branch: GitBranchName = get(inputs, "branch")?;
        let marker: RepoPath = get(inputs, "marker")?;
        let files = Self::read_files(inputs)?;
        Self::validate_shape(&files, &marker)?;
        let marker_file = Self::validated_marker_file(&files, &marker)?;
        let _message: CommitHeadline = get(inputs, "message")?;
        let mut state = self.state.lock().unwrap();
        let key = scaffold_key(&repo, &branch);
        state.record_ensure_call(Self::TOOL_NAME, &key);
        if let Some(err) = state.take_fail_ensure_once(Self::TOOL_NAME, &key) {
            return Err(err);
        }
        let outputs = Self::outputs_for(&repo, &branch, &marker);
        let was_empty = match Self::observe_branch_existence(&state, &repo, &branch) {
            BranchExistence::RepositoryAbsent => {
                return Err(Self::repository_absent_error(&repo));
            }
            BranchExistence::Mismatch { default_branch } => {
                return Err(Self::default_branch_mismatch(
                    &repo,
                    &branch,
                    &default_branch,
                ));
            }
            BranchExistence::MissingOnNonEmpty => {
                return Err(Self::missing_branch_error(&repo, &branch));
            }
            BranchExistence::Empty { default_branch } => {
                debug_assert_eq!(
                    default_branch, branch,
                    "observe_branch_existence only produces Empty when branch already equals \
                     the record's own default"
                );
                true
            }
            BranchExistence::Exists => false,
        };
        match Self::observe(state.scaffolds.get(&key), &marker, &files)? {
            ScaffoldState::Present => Ok(Ensured {
                outputs,
                changed: false,
            }),
            ScaffoldState::Foreign => Err(Self::foreign_conflict(&repo, &branch, &marker)),
            ScaffoldState::Absent { already_equal } => {
                {
                    let map = state.scaffolds.entry(key).or_default();
                    for file in &files {
                        if !already_equal.contains(file.path().as_str()) {
                            map.insert(
                                file.path().as_str().to_string(),
                                file.content().to_string(),
                            );
                        }
                    }
                    map.insert(
                        marker_file.path().as_str().to_string(),
                        marker_file.content().to_string(),
                    );
                }
                // Milestone 3l, task F1: an empty-repository ensure's own
                // first write makes the repository non-empty -- record
                // `branch` so a second `ensure`/`read` sees
                // `BranchExistence::Exists` rather than re-entering this
                // path (decision (b): "empty" is "no branches at all").
                if was_empty
                    && let Some(record) = state.github_repos.get_mut(&repo_key(&repo))
                {
                    record.branches = Some(vec![branch.clone()]);
                }
                Ok(Ensured {
                    outputs,
                    changed: true,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use willikins_core::PortName;
    use willikins_types::{DomainType, RepoVisibility};

    fn repo() -> GitHubRepo {
        GitHubRepo::parse("acme/widget").unwrap()
    }

    /// A legacy-seeded repository (`branches: None`, "every branch
    /// exists") -- what every test in this module that is not itself
    /// about milestone 3l's existence/emptiness check wants, so this
    /// task's new branch-existence check never changes their outcome.
    fn legacy_repo_state() -> FakeState {
        FakeState::new().with_repo(&repo(), RepoVisibility::Private, true)
    }

    fn branch() -> GitBranchName {
        GitBranchName::parse("main").unwrap()
    }

    fn marker() -> RepoPath {
        RepoPath::parse(".willikins-scaffold").unwrap()
    }

    fn seed_files() -> Vec<RepoFile> {
        vec![
            RepoFile::new(RepoPath::parse("BUILD.bazel").unwrap(), "# reserve\n").unwrap(),
            RepoFile::new(RepoPath::parse("ios/BUILD.bazel").unwrap(), "ios content\n").unwrap(),
        ]
    }

    fn inputs(files: Vec<RepoFile>) -> Inputs {
        let mut inputs = Inputs::new();
        inputs.insert(PortName::parse("repo").unwrap(), Value::known(repo()));
        inputs.insert(PortName::parse("branch").unwrap(), Value::known(branch()));
        inputs.insert(PortName::parse("marker").unwrap(), Value::known(marker()));
        inputs.insert(PortName::parse("files").unwrap(), Value::known_list(files));
        inputs.insert(
            PortName::parse("message").unwrap(),
            Value::known(CommitHeadline::parse("feat: scaffold").unwrap()),
        );
        inputs
    }

    fn tool() -> GitHubScaffoldEnsure {
        GitHubScaffoldEnsure::new(Arc::new(Mutex::new(legacy_repo_state())))
    }

    #[test]
    fn spec_validates_against_the_registry() {
        tool().spec().validate(willikins_types::registry()).unwrap();
    }

    #[test]
    fn read_reports_absent_when_empty() {
        let observation = tool().read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_then_read_gives_present() {
        let tool = tool();
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(ensured.changed);
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_is_idempotent_and_reports_unchanged_on_the_second_call() {
        let tool = tool();
        let token = SinkToken::new();
        tool.ensure(&inputs(seed_files()), &token).unwrap();
        let second = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(!second.changed);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn a_scaffold_half_applied_by_hand_still_converges() {
        let state = Arc::new(Mutex::new(legacy_repo_state().with_scaffold_files(
            &repo(),
            &branch(),
            &[("BUILD.bazel", "# reserve\n")],
        )));
        let tool = GitHubScaffoldEnsure::new(state);
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(ensured.changed);
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_conflicts_on_a_differing_seed_path_and_writes_nothing() {
        let state = Arc::new(Mutex::new(legacy_repo_state().with_scaffold_files(
            &repo(),
            &branch(),
            &[("BUILD.bazel", "someone else's content\n")],
        )));
        let tool = GitHubScaffoldEnsure::new(state);
        let token = SinkToken::new();
        let err = tool.ensure(&inputs(seed_files()), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(err.message.contains("BUILD.bazel"));
        assert!(!err.message.contains("someone else's content"));
    }

    #[test]
    fn read_reports_foreign_when_the_marker_is_not_ours() {
        let state = Arc::new(Mutex::new(legacy_repo_state().with_scaffold_files(
            &repo(),
            &branch(),
            &[(".willikins-scaffold", "not ours\n")],
        )));
        let tool = GitHubScaffoldEnsure::new(state);
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Foreign));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_refuses_zero_files_before_touching_state() {
        let tool = tool();
        let token = SinkToken::new();
        let err = tool.ensure(&inputs(Vec::new()), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::Invalid);
    }

    // -----------------------------------------------------------------
    // Milestone 3l, task F1, acceptance 12: decision (b)'s table, for
    // the cases this fake can model.
    // -----------------------------------------------------------------

    /// No `github_repos` record at all: `read` reports `Absent` (so a
    /// document creating its own repository in the same plan still plans
    /// cleanly), exactly like decision (b)'s table.
    #[test]
    fn read_reports_absent_when_the_repository_has_no_record() {
        let tool = GitHubScaffoldEnsure::new(Arc::new(Mutex::new(FakeState::new())));
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
    }

    /// No `github_repos` record at all: `ensure` refuses `NotFound`,
    /// naming `github.repo.ensure` as the tool that creates one, and
    /// writes nothing.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_reports_not_found_when_the_repository_has_no_record() {
        let state = Arc::new(Mutex::new(FakeState::new()));
        let tool = GitHubScaffoldEnsure::new(state.clone());
        let token = SinkToken::new();
        let err = tool.ensure(&inputs(seed_files()), &token).unwrap_err();
        assert_eq!(err.kind, willikins_core::ToolErrorKind::NotFound);
        assert!(err.message.contains("does not exist"), "{}", err.message);
        assert!(
            err.message.contains("github.repo.ensure"),
            "{}",
            err.message
        );
        assert!(state.lock().unwrap().scaffolds.is_empty());
    }

    /// An empty repository (`branches: Some(vec![])`) whose default
    /// branch (unset, so `main`) equals the requested `branch`: `read`
    /// reports `Absent`, and `ensure` lands the scaffold and then records
    /// `branch` on the repository's own record.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn ensure_lands_on_an_empty_repository_with_the_default_branch_and_records_it() {
        let state = Arc::new(Mutex::new(FakeState::new().with_empty_repo(
            &repo(),
            RepoVisibility::Private,
            None,
        )));
        let tool = GitHubScaffoldEnsure::new(state.clone());
        let read = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(read, Observation::Absent { .. }));

        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(ensured.changed);

        let record = state
            .lock()
            .unwrap()
            .github_repos
            .get(&repo_key(&repo()))
            .unwrap()
            .clone();
        assert_eq!(record.branches, Some(vec![branch()]));

        // A second ensure now sees `BranchExistence::Exists` and the
        // scaffold already landed: idempotent.
        let second = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(!second.changed);
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Present(_)));
    }

    /// An empty repository whose default branch differs from the
    /// requested `branch`: both `read` and `ensure` refuse `Conflict`,
    /// naming both names, and write nothing.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn read_and_ensure_conflict_when_the_empty_repositorys_default_branch_differs() {
        let trunk = GitBranchName::parse("trunk").unwrap();
        let state = Arc::new(Mutex::new(FakeState::new().with_empty_repo(
            &repo(),
            RepoVisibility::Private,
            Some(&trunk),
        )));
        let tool = GitHubScaffoldEnsure::new(state.clone());

        let read_err = tool.read(&inputs(seed_files())).unwrap_err();
        assert_eq!(read_err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(read_err.message.contains("main"), "{}", read_err.message);
        assert!(read_err.message.contains("trunk"), "{}", read_err.message);

        let token = SinkToken::new();
        let ensure_err = tool.ensure(&inputs(seed_files()), &token).unwrap_err();
        assert_eq!(ensure_err.kind, willikins_core::ToolErrorKind::Conflict);
        assert!(state.lock().unwrap().scaffolds.is_empty());
    }

    /// A non-empty repository (`branches: Some(list)`) missing the
    /// requested `branch`: both `read` and `ensure` keep the unchanged
    /// missing-branch `NotFound`, and write nothing.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn read_and_ensure_report_not_found_when_the_branch_is_missing_on_a_non_empty_repository() {
        let mut seed_state = FakeState::new();
        seed_state.github_repos.insert(
            repo_key(&repo()),
            crate::state::GitHubRepoRecord {
                visibility: RepoVisibility::Private,
                ours: true,
                archived: false,
                branches: Some(vec![GitBranchName::parse("other").unwrap()]),
                default_branch: None,
            },
        );
        let state = Arc::new(Mutex::new(seed_state));
        let tool = GitHubScaffoldEnsure::new(state.clone());

        let read_err = tool.read(&inputs(seed_files())).unwrap_err();
        assert_eq!(read_err.kind, willikins_core::ToolErrorKind::NotFound);
        assert!(
            read_err.message.contains("does not exist on"),
            "{}",
            read_err.message
        );

        let token = SinkToken::new();
        let ensure_err = tool.ensure(&inputs(seed_files()), &token).unwrap_err();
        assert_eq!(ensure_err.kind, willikins_core::ToolErrorKind::NotFound);
        assert!(state.lock().unwrap().scaffolds.is_empty());
    }

    /// A legacy record (`branches: None`, seeded by `with_repo`): both
    /// `read` and `ensure` behave exactly as before this task, never
    /// consulting branch existence at all.
    #[test]
    #[allow(clippy::disallowed_methods)] // a test mints its own token
    fn a_legacy_repo_record_is_unaffected_by_the_branch_existence_check() {
        let tool = GitHubScaffoldEnsure::new(Arc::new(Mutex::new(legacy_repo_state())));
        let observation = tool.read(&inputs(seed_files())).unwrap();
        assert!(matches!(observation, Observation::Absent { .. }));
        let token = SinkToken::new();
        let ensured = tool.ensure(&inputs(seed_files()), &token).unwrap();
        assert!(ensured.changed);
    }
}
