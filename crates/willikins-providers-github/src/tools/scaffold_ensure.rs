//! `github.scaffold.ensure`: milestone 3g, task G2, decisions (b) and (c)
//! of `docs/plans/2026-09-30-milestone-3g-file-writing.md`; extended by
//! milestone 3l, task S2, decision (b) of
//! `docs/plans/2026-10-05-milestone-3l-new-repositories.md`, for a
//! repository that does not exist yet or exists but is empty (no
//! branches).
//!
//! The resource this tool ensures is "this scaffold has landed on this
//! branch", keyed by `(repo, branch, marker)` -- `files` is the content,
//! not the key. A scaffold is a **seed**: once its marker is present, the
//! files it seeded belong to the repository and are never read or written
//! again (decision (c)); before landing, a declared path that already
//! holds different content is a refusal naming that path, never an
//! overwrite. See [`GitHubScaffoldEnsure::observe`] for the read table and
//! [`Tool::ensure`]'s impl for the bounded retry against a moving head.
//!
//! **A repository that does not exist, or exists but is empty.**
//! `get_branch_head` stays the first call, so a repository whose branch
//! already exists issues exactly the same requests it always has. Only
//! when that call fails `404` or `409` does this tool ask more: `GET
//! /repos/{owner}/{name}` tells apart "no repository at all" (`404`,
//! mapped to [`ScaffoldState::RepositoryAbsent`]) from one that exists,
//! and `GET .../branches?per_page=1` tells apart "no branches at all"
//! (empty, GitHub's own definition) from "the named branch alone is
//! missing". `read` maps both `RepositoryAbsent` and an empty repository
//! whose `branch` matches the organisation's default to
//! [`Observation::Absent`] (milestone 3l decision (b): a plan-time error
//! for a repository `github.repo.ensure` would create in the same plan
//! makes every new-repository document unplannable, the same call
//! milestone 3j made for Doppler). `ensure` tells them apart: a missing
//! repository is `NotFound` naming `github.repo.ensure` as the tool that
//! creates one; an empty repository whose branch matches the default is
//! [`ScaffoldState::Empty`], and `ensure` lands the scaffold's own first
//! write there (milestone 3l, task S3, decision (a)): the byte-order-smallest
//! file in `files` is written through `GitHubClient::create_first_file` (the
//! one route GitHub documents for an empty repository), then `ensure`
//! re-observes -- never trusting the `PUT`'s own body -- and decides what to
//! do next purely from that read: once the re-observe reports the root file
//! landed, the rest of `files` plus the marker land as one ordinary
//! `createCommitOnBranch`, exactly as for any other repository. A `409`
//! whose re-observe still reads empty is retried (bounded,
//! `GitHubClient::pause` between attempts); a successful `PUT` whose
//! re-observe still reads empty is polled for ref visibility (also bounded,
//! also paused); any other failure whose re-observe still reads empty is
//! reported as-is. An empty repository whose `branch` does *not* match the
//! default is a `Conflict` naming both
//! names, at `read` and at `ensure` alike -- this tool has no way to
//! create a branch other than the organisation's default on an empty
//! repository (decision (c)). A non-empty repository still missing the
//! named branch keeps today's unchanged `NotFound` message; the same
//! case surfacing as a `409` instead of a `404` (the branch not yet
//! visible right after the repository's own creation) is reported as a
//! `Provider` "not available yet" error instead, since the branch may
//! simply not have propagated yet. Any other failure -- including a
//! `301` from `GET /repos` (the repository was renamed away) -- is
//! [`to_tool_error`], failing loudly, never `Absent`.
//!
//! **Mitigation for a typo'd, already-existing repository.** Decision
//! (b)'s cost: a document naming an existing repository with a typo now
//! plans `Create` and fails only at apply, with `NotFound`, rather than
//! at plan. Binding `repo` from `github.repo.get`'s own output (which
//! fails at plan on a `404`) catches this earlier.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;

use willikins_core::tool::helpers::{
    conflict, exact, get, get_optional, invalid, list, not_found, port, require_present, scalar,
    tool_name,
};
use willikins_core::{
    Class, Ensured, Inputs, Observation, Outputs, PortSpec, PortType, SinkToken, Tool, ToolError,
    ToolErrorKind, ToolSpec, Value,
};
use willikins_types::{
    CommitHeadline, DomainType, GitBranchName, GitHubRepo, GitHubToken, RepoFile, RepoPath,
};

use crate::client::{GitHubClient, PathEntry, ScopedClient, git_blob_sha, to_tool_error};

/// The marker file's required first line -- the codebase's own
/// ownership-marker precedent (`managed-by-willikins` topic,
/// `managed-by: willikins` pipeline description).
const MARKER_HEADER: &str = "managed-by: willikins";

/// The fewest files a call may seed.
const MIN_FILES: usize = 1;

/// The most files a call may seed -- SHARED VALUES' `files: list<RepoFile>`
/// bound.
const MAX_FILES: usize = 64;

/// Milestone 3l, SHARED VALUES "Waits (S3)": after a first-file `PUT`
/// answers `409` and a re-observe still reads the repository as empty,
/// the most attempts `ensure` makes at that `PUT` in total, including the
/// first.
const MAX_UNAVAILABLE_ATTEMPTS: u32 = 5;

/// Milestone 3l, SHARED VALUES "Waits (S3)": the wait between successive
/// first-file `PUT` attempts after a `409`.
const UNAVAILABLE_WAIT: Duration = Duration::from_secs(2);

/// Milestone 3l, SHARED VALUES "Waits (S3)": after a first-file `PUT`
/// answers `2xx` but a re-observe still reads the repository as empty
/// (ref visibility lags creation), the most extra re-observes `ensure`
/// polls for before reporting "not visible yet".
const MAX_REF_VISIBLE_POLLS: u32 = 10;

/// Milestone 3l, SHARED VALUES "Waits (S3)": the wait between successive
/// ref-visibility polls.
const REF_VISIBLE_WAIT: Duration = Duration::from_millis(300);

/// The most `createCommitOnBranch` attempts `ensure` makes in total,
/// including the first: a busy trunk can move twice while this call is in
/// flight, but three attempts, each decided only from a fresh re-read
/// (never from a commit failure's own body), is where decision (b) stops
/// retrying and reports the original failure instead.
const MAX_COMMIT_ATTEMPTS: u32 = 3;

/// `github.scaffold.ensure`.
pub struct GitHubScaffoldEnsure {
    spec: ToolSpec,
    client: Arc<GitHubClient>,
}

/// What [`GitHubScaffoldEnsure::observe`] found at the current head,
/// carrying exactly what `ensure` needs and no more: never a blob sha
/// (outputs, and so this state, must never leak one -- acceptance 8).
enum ScaffoldState {
    /// The marker is present with the right first line: the scaffold has
    /// landed. No seed path was ever read to reach this state.
    Present,
    /// Something exists at the marker's key but is not willikins' own
    /// marker, or a seed path holds content this tool did not put there.
    Foreign,
    /// The marker is absent and every seed path is either absent or
    /// already byte-equal to what this call would write.
    Absent {
        /// The head commit sha this observation was made against --
        /// `ensure`'s compare-and-swap.
        head: String,
        /// Seed paths (by [`RepoPath::as_str`]) already present with the
        /// exact content this call would write: skipped from the commit's
        /// `additions`, so a scaffold half-applied by hand from the same
        /// templates still converges.
        already_equal: HashSet<String>,
    },
    /// Milestone 3l, task S2: the repository exists but has no branches
    /// at all (GitHub's own definition of "empty"), and `branch` names
    /// its default branch. `read` maps this to [`Observation::Absent`];
    /// `ensure` lands the scaffold's own first write here (milestone 3l,
    /// task S3, decision (a)).
    Empty {
        /// The repository's reported default branch -- always equal to
        /// the `branch` input when this state is produced; see
        /// [`GitHubScaffoldEnsure::observe_without_branch`].
        /// [`GitHubScaffoldEnsure::initialize_empty_repository`] takes
        /// this as its own `debug_assert_eq!` of decision (a)'s first
        /// check, rather than this crate carrying a field nothing reads.
        default_branch: GitBranchName,
    },
    /// Milestone 3l, task S2: no repository exists at this key that this
    /// credential can see -- GitHub's `404` on `GET /repos/{owner}/{name}`
    /// cannot tell "does not exist" apart from "exists but invisible to
    /// us", the same conflation `github.repo.ensure` already lives with.
    /// `read` maps this to [`Observation::Absent`]; `ensure` reports the
    /// repository-absent `NotFound`, since this tool never creates a
    /// repository.
    RepositoryAbsent,
}

impl GitHubScaffoldEnsure {
    /// Build the tool against `client`, constructing its spec.
    #[must_use]
    pub fn new(client: Arc<GitHubClient>) -> Self {
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
                name: tool_name("github.scaffold.ensure"),
                description: "Ensure a set of files, plus an ownership marker, has landed on a \
                               branch as one commit."
                    .to_string(),
                inputs,
                outputs,
                key: vec![port("repo"), port("branch"), port("marker")],
                class: Class::Irreversible,
                pure: false,
            },
            client,
        }
    }

    /// The outputs this tool ever reports: pass-through only, never a
    /// blob sha or a commit oid (decision (b): an output carrying either
    /// would make a plan-to-apply window on a busy `main` drift).
    fn outputs_for(repo: &GitHubRepo, branch: &GitBranchName, marker: &RepoPath) -> Outputs {
        let mut outputs = Outputs::new();
        outputs.insert(port("repo"), Value::known(repo.clone()));
        outputs.insert(port("branch"), Value::known(branch.clone()));
        outputs.insert(port("marker"), Value::known(marker.clone()));
        outputs
    }

    /// Read the required `files` port as a list of [`RepoFile`]s.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] of kind `Invalid` when the port is bound but
    /// unknown, or not a list of `RepoFile` -- both mean `check` was
    /// bypassed, since `plan`/`apply` only ever deliver a well-typed value
    /// here.
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

    /// Every shape refusal decision (b)'s table makes **before any
    /// request**: a file count out of `1..=64`, a path declared twice, the
    /// marker path equal to one of `files`, or one declared path (a file or
    /// the marker) a directory of another.
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

    /// The marker file's content: [`MARKER_HEADER`], then one
    /// `<40-hex blob sha> <path>` line per seeded file, sorted by path,
    /// trailing newline (SHARED VALUES' exact format).
    fn marker_content(files: &[RepoFile]) -> String {
        let mut lines: Vec<(String, String)> = files
            .iter()
            .map(|file| {
                (
                    file.path().as_str().to_string(),
                    git_blob_sha(file.content().as_bytes()),
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

    /// Build and validate the marker [`RepoFile`] from `files` and
    /// `marker` -- both fully known from inputs alone, so this is a shape
    /// refusal decision (b)'s table puts "before any request", not one
    /// [`ensure`](Tool::ensure) may discover only after committing
    /// everything else. Without this, 64 files each near [`RepoPath`]'s
    /// own 1,024-character bound can produce a marker over
    /// [`RepoFile`]'s 65,536-character bound, and the failure would
    /// otherwise surface from inside the commit attempt instead of here.
    fn validated_marker_file(files: &[RepoFile], marker: &RepoPath) -> Result<RepoFile, ToolError> {
        let content = Self::marker_content(files);
        RepoFile::new(marker.clone(), content)
            .map_err(|err| invalid(format!("marker file is invalid: {}", err.reason)))
    }

    /// The `Conflict` a foreign marker or an owned-but-differing seed path
    /// produces.
    fn foreign_conflict(repo: &GitHubRepo, branch: &GitBranchName, marker: &RepoPath) -> ToolError {
        conflict(format!(
            "`{marker}` on `{repo}`@`{branch}` already exists and is not willikins' scaffold \
             marker; this tool will not overwrite it"
        ))
    }

    /// The `NotFound` `ensure` reports for [`ScaffoldState::RepositoryAbsent`]
    /// (milestone 3l, SHARED VALUES "Repository-absent message (S3,
    /// `NotFound`)" -- named for the task that documents the value, not
    /// the one that returns it here).
    fn repository_absent_error(repo: &GitHubRepo) -> ToolError {
        not_found(format!(
            "`{repo}` does not exist; this tool never creates a repository (github.repo.ensure \
             does)"
        ))
    }

    /// The element of `files` whose [`RepoPath::as_str`] is smallest in
    /// byte order (milestone 3l, SHARED VALUES "Root file (S3)") -- the
    /// one seed path `ensure` writes through the Contents API to
    /// initialise an empty repository's first branch (decision (a)).
    /// `validate_shape` already enforces at least [`MIN_FILES`] entries
    /// and no duplicate path, so this always finds exactly one smallest
    /// element.
    fn root_file(files: &[RepoFile]) -> &RepoFile {
        files
            .iter()
            .min_by(|a, b| a.path().as_str().cmp(b.path().as_str()))
            .expect("validate_shape enforces at least one file")
    }

    /// The first commit's message when `ensure` initialises an empty
    /// repository (milestone 3l, SHARED VALUES "Root commit message
    /// (S3)"): `message`'s headline, then a fixed body naming `marker` --
    /// distinct from [`Tool::ensure`]'s own second-commit body
    /// (`"Seeded by willikins. Marker: {marker}."`) so a reader of either
    /// commit can tell which one seeded the repository.
    fn root_commit_message(message: &CommitHeadline, marker: &RepoPath) -> String {
        format!(
            "{message}\n\nThe first commit of an empty repository, seeded by willikins. Marker: \
             {marker}."
        )
    }

    /// The `Provider` `ensure` reports when the first-file `PUT` landed
    /// (or a later re-observe showed it had) but `branch`'s head still
    /// is not visible after [`MAX_REF_VISIBLE_POLLS`] polls (milestone
    /// 3l, SHARED VALUES "Ref not yet visible (S3, Provider)").
    fn ref_not_yet_visible_error(repo: &GitHubRepo, branch: &GitBranchName) -> ToolError {
        ToolError {
            kind: ToolErrorKind::Provider,
            message: format!(
                "the first commit of `{repo}` landed, but `{branch}` is not visible yet; \
                 re-run this document to finish the scaffold"
            ),
        }
    }

    /// The `Conflict` [`Self::observe_without_branch`] produces when an
    /// empty repository's own default branch does not match the
    /// requested `branch` (milestone 3l, SHARED VALUES "Default-branch
    /// mismatch (S2, `Conflict`)").
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

    /// The `Provider` [`Self::observe_without_branch`] produces when an
    /// otherwise-empty repository is still unavailable right after
    /// creation (milestone 3l, SHARED VALUES "Unavailable (S2,
    /// `Provider`)").
    fn repository_unavailable(repo: &GitHubRepo) -> ToolError {
        ToolError {
            kind: ToolErrorKind::Provider,
            message: format!(
                "`{repo}` is not available yet (GitHub may still be creating it); re-run this \
                 document"
            ),
        }
    }

    /// The `Provider` [`Self::observe_without_branch`] produces when
    /// `GET /repos` reported a `default_branch` this tool cannot use at
    /// all -- absent, or not a valid [`GitBranchName`]. Static (never
    /// echoes GitHub's own string): milestone 3l, SHARED VALUES
    /// "`404`/`409` then repo `200` with `default_branch` absent or not a
    /// `GitBranchName` -> `Provider` (static message naming `repo`)".
    fn repository_default_branch_unusable(repo: &GitHubRepo) -> ToolError {
        ToolError {
            kind: ToolErrorKind::Provider,
            message: format!("`{repo}` did not report a usable default branch"),
        }
    }

    /// Decision (b)'s table for when `get_branch_head` failed `404` or
    /// `409`: resolves whether the repository itself is absent, empty
    /// (and if so whether `branch` names its own default), or exists
    /// with some other branch -- in which case `branch_was_404` decides
    /// between the unchanged missing-branch message and the "not
    /// available yet" one, since a `409` there means GitHub may simply
    /// not have made the branch visible yet, not that it never will
    /// exist.
    fn observe_without_branch(
        client: &GitHubClient,
        repo: &GitHubRepo,
        branch: &GitBranchName,
        branch_was_404: bool,
    ) -> Result<ScaffoldState, ToolError> {
        let repo_body = match client.get_repo(repo) {
            Ok(body) => body,
            Err(err) if err.status == Some(404) => return Ok(ScaffoldState::RepositoryAbsent),
            Err(err) => return Err(to_tool_error(err)),
        };
        let default_branch = repo_body
            .default_branch
            .as_deref()
            .and_then(|name| GitBranchName::parse(name).ok());
        let Some(default_branch) = default_branch else {
            return Err(Self::repository_default_branch_unusable(repo));
        };
        if client.has_any_branch(repo).map_err(to_tool_error)? {
            return Err(if branch_was_404 {
                not_found(format!(
                    "branch `{branch}` does not exist on `{repo}`; this tool never creates one"
                ))
            } else {
                Self::repository_unavailable(repo)
            });
        }
        if default_branch == *branch {
            Ok(ScaffoldState::Empty { default_branch })
        } else {
            Err(Self::default_branch_mismatch(repo, branch, &default_branch))
        }
    }

    /// Resolve the current head, the marker's state, and -- only when the
    /// marker is absent -- every seed path's state, exactly as decision
    /// (b)'s read table describes. Never downloads a seed file's content;
    /// content is compared by [`git_blob_sha`]. The marker's own blob is
    /// the only one this tool ever fetches.
    ///
    /// When `branch` has a head, this is everything: the only requests
    /// are the ones a repository with that branch has always issued. When
    /// `get_branch_head` instead fails `404` or `409`, every other
    /// outcome below comes from [`Self::observe_without_branch`]
    /// (milestone 3l, task S2, decision (b)) instead -- `get_commit_root_tree`
    /// and every tree/blob read are never reached in that case.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] of kind:
    /// - `Conflict`, naming every seed path that holds content this tool
    ///   did not put there (sorted, never their content), when the
    ///   branch exists and a declared path conflicts; or naming `branch`
    ///   and the repository's actual default branch when the repository
    ///   is empty and they differ (milestone 3l decision (c)).
    /// - `NotFound`, with the unchanged "branch ... does not exist ...
    ///   this tool never creates one" message, when `branch` is missing
    ///   from a repository that is not empty.
    /// - `Provider`, naming `repo`, when the branch's own `404`/`409`
    ///   turned out to mean "not available yet" (a `409` on a
    ///   non-empty repository) or "GitHub reported no usable default
    ///   branch" (absent, or not a valid [`GitBranchName`]).
    /// - [`to_tool_error`] for any other provider failure, including a
    ///   `301` from `GET /repos` (the repository renamed away) -- never
    ///   reported as `Absent`.
    ///
    /// A missing repository or an empty one whose branch matches the
    /// organisation's default is **not** an error here: both become
    /// [`ScaffoldState::RepositoryAbsent`] and [`ScaffoldState::Empty`]
    /// respectively, which `read` maps to [`Observation::Absent`] and
    /// `ensure` turns into its own, different errors (decision (b): a
    /// plan-time error for a repository `github.repo.ensure` would
    /// create in the same plan would make every new-repository document
    /// unplannable).
    fn observe(
        client: &GitHubClient,
        repo: &GitHubRepo,
        branch: &GitBranchName,
        marker: &RepoPath,
        files: &[RepoFile],
    ) -> Result<ScaffoldState, ToolError> {
        let head = match client.get_branch_head(repo, branch) {
            Ok(head) => head,
            Err(err) if err.status == Some(404) || err.status == Some(409) => {
                return Self::observe_without_branch(client, repo, branch, err.status == Some(404));
            }
            Err(err) => return Err(to_tool_error(err)),
        };
        let root_tree = client
            .get_commit_root_tree(repo, &head)
            .map_err(to_tool_error)?;

        // The marker alone, first: "Present issues no tree walk past the
        // marker" (acceptance 8) -- a seed path is never resolved once
        // the scaffold has already landed.
        let marker_paths = std::slice::from_ref(marker);
        let marker_entries = client
            .resolve_tree_paths(repo, &root_tree, marker_paths)
            .map_err(to_tool_error)?;
        match marker_entries.get(marker) {
            Some(PathEntry::Blob { mode, sha }) if mode.as_str() == "100644" => {
                let bytes = client.get_blob(repo, sha).map_err(to_tool_error)?;
                let first_line = bytes.split(|&b| b == b'\n').next().unwrap_or(&[]);
                if first_line == MARKER_HEADER.as_bytes() {
                    return Ok(ScaffoldState::Present);
                }
                return Ok(ScaffoldState::Foreign);
            }
            // `resolve_tree_paths` inserts an entry for every requested
            // path, `Absent` included -- `None` never actually occurs
            // here, but is handled the same way defensively.
            None | Some(PathEntry::Absent) => {}
            Some(_) => return Ok(ScaffoldState::Foreign),
        }

        // Marker absent: every seed path must be either absent or
        // byte-equal to what this call would write.
        let paths: Vec<RepoPath> = files.iter().map(|file| file.path().clone()).collect();
        let entries = client
            .resolve_tree_paths(repo, &root_tree, &paths)
            .map_err(to_tool_error)?;
        let mut already_equal: HashSet<String> = HashSet::new();
        let mut conflicts: Vec<String> = Vec::new();
        for file in files {
            match entries.get(file.path()) {
                None | Some(PathEntry::Absent) => {}
                Some(PathEntry::Blob { mode, sha }) if mode.as_str() == "100644" => {
                    let expected = git_blob_sha(file.content().as_bytes());
                    if sha.as_str() == expected.as_str() {
                        already_equal.insert(file.path().as_str().to_string());
                    } else {
                        conflicts.push(file.path().as_str().to_string());
                    }
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
        Ok(ScaffoldState::Absent {
            head,
            already_equal,
        })
    }

    /// Resolve `state` (already `Absent`, or `Empty` -- in which case
    /// this lands the scaffold's own first write first, via
    /// [`Self::initialize_empty_repository`], milestone 3l's task S3)
    /// into the `(head, already_equal)` pair [`Tool::ensure`]'s own
    /// commit loop needs, or `None` when nothing needs committing at all
    /// (`Present`). Kept separate from `ensure` itself only to stay
    /// under this crate's line-count lint; it has no life of its own
    /// outside that call.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`] for [`ScaffoldState::Foreign`],
    /// [`ScaffoldState::RepositoryAbsent`], or anything
    /// [`Self::initialize_empty_repository`] itself returns.
    fn resolve_initial_state(
        client: &GitHubClient,
        repo: &GitHubRepo,
        branch: &GitBranchName,
        marker: &RepoPath,
        files: &[RepoFile],
        message: &CommitHeadline,
        state: ScaffoldState,
    ) -> Result<Option<(String, HashSet<String>)>, ToolError> {
        match state {
            ScaffoldState::Present => Ok(None),
            ScaffoldState::Foreign => Err(Self::foreign_conflict(repo, branch, marker)),
            ScaffoldState::RepositoryAbsent => Err(Self::repository_absent_error(repo)),
            ScaffoldState::Absent {
                head,
                already_equal,
            } => Ok(Some((head, already_equal))),
            ScaffoldState::Empty { default_branch } => {
                match Self::initialize_empty_repository(
                    client,
                    repo,
                    branch,
                    &default_branch,
                    marker,
                    files,
                    message,
                )? {
                    ScaffoldState::Absent {
                        head,
                        already_equal,
                    } => Ok(Some((head, already_equal))),
                    ScaffoldState::Present => Ok(None),
                    ScaffoldState::Foreign => Err(Self::foreign_conflict(repo, branch, marker)),
                    ScaffoldState::RepositoryAbsent => Err(Self::repository_absent_error(repo)),
                    // `initialize_empty_repository` never returns this
                    // variant itself: its own loop either resolves past
                    // `Empty` or returns `Err` directly.
                    ScaffoldState::Empty { .. } => {
                        unreachable!("initialize_empty_repository resolves Empty before returning")
                    }
                }
            }
        }
    }

    /// Milestone 3l, task S3, decision (a): land the scaffold's own first
    /// write on an empty repository through the Contents API, then
    /// re-observe -- never trusting the `PUT`'s own body -- until the
    /// repository is confirmed non-empty, returning the resulting state
    /// so [`Tool::ensure`]'s own commit loop can land the rest of `files`
    /// plus the marker exactly as it would for any other repository.
    ///
    /// Called only once `branch` is already confirmed equal to
    /// `default_branch`, the repository's own default branch
    /// ([`Self::observe_without_branch`]'s own check, decision (a)'s
    /// first point) -- the `debug_assert_eq!` below checks that
    /// invariant rather than repeating the refusal, since by the time a
    /// caller reaches [`ScaffoldState::Empty`] the `Conflict` path has
    /// already run.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError`]:
    /// - [`to_tool_error`] of the first-file `PUT`'s own failure, once a
    ///   re-observe confirms the repository is still empty and the
    ///   failure was not a `409` (or a `409` that has exhausted
    ///   [`MAX_UNAVAILABLE_ATTEMPTS`] attempts).
    /// - `Provider`, [`Self::ref_not_yet_visible_error`], once a `2xx`
    ///   `PUT`'s re-observe still reads empty after
    ///   [`MAX_REF_VISIBLE_POLLS`] polls.
    /// - [`Self::foreign_conflict`] or [`Self::repository_absent_error`]
    ///   if a re-observe between attempts shows the marker landed foreign
    ///   to this tool, or the repository vanished outright -- both only
    ///   reachable if something else wrote to the same repository
    ///   concurrently.
    ///
    /// Never returns `Ok(ScaffoldState::Empty { .. })`: this method's own
    /// loop only returns once a re-observe has moved past `Empty`, or
    /// returns `Err` instead.
    fn initialize_empty_repository(
        client: &GitHubClient,
        repo: &GitHubRepo,
        branch: &GitBranchName,
        default_branch: &GitBranchName,
        marker: &RepoPath,
        files: &[RepoFile],
        message: &CommitHeadline,
    ) -> Result<ScaffoldState, ToolError> {
        debug_assert_eq!(
            branch, default_branch,
            "decision (a)'s first check already refused a mismatch before this is reached"
        );
        let root_file = Self::root_file(files).clone();
        let root_message = Self::root_commit_message(message, marker);

        let mut unavailable_attempts: u32 = 0;
        loop {
            unavailable_attempts += 1;
            let put_result = client.create_first_file(repo, branch, &root_file, &root_message);
            let put_was_409 = matches!(&put_result, Err(err) if err.status == Some(409));
            let put_was_ok = put_result.is_ok();

            // Whatever the `PUT` answered, re-observe and decide only
            // from that read (decision (a), point 3) -- polling for ref
            // visibility only when the `PUT` itself reported success,
            // since a failed `PUT` reading empty again has nothing left
            // to wait for at this layer.
            let mut ref_polls: u32 = 0;
            loop {
                match Self::observe(client, repo, branch, marker, files)? {
                    state @ (ScaffoldState::Absent { .. }
                    | ScaffoldState::Present
                    | ScaffoldState::Foreign
                    | ScaffoldState::RepositoryAbsent) => return Ok(state),
                    ScaffoldState::Empty { .. } => {
                        if !put_was_ok {
                            break;
                        }
                        if ref_polls >= MAX_REF_VISIBLE_POLLS {
                            return Err(Self::ref_not_yet_visible_error(repo, branch));
                        }
                        client.pause(REF_VISIBLE_WAIT);
                        ref_polls += 1;
                    }
                }
            }

            if put_was_409 && unavailable_attempts < MAX_UNAVAILABLE_ATTEMPTS {
                client.pause(UNAVAILABLE_WAIT);
                continue;
            }
            return Err(to_tool_error(put_result.expect_err(
                "put_result is Err here: put_was_ok would have returned Ok above",
            )));
        }
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
        let token: Option<GitHubToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, token.as_ref());
        match Self::observe(&client, &repo, &branch, &marker, &files)? {
            ScaffoldState::Present => Ok(Observation::Present(Self::outputs_for(
                &repo, &branch, &marker,
            ))),
            ScaffoldState::Foreign => Ok(Observation::Foreign),
            ScaffoldState::Absent { .. }
            | ScaffoldState::Empty { .. }
            | ScaffoldState::RepositoryAbsent => Ok(Observation::Absent {
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
        let message: CommitHeadline = get(inputs, "message")?;
        let bound_token: Option<GitHubToken> = get_optional(inputs, "token")?;
        let client = ScopedClient::default_for(&self.client, bound_token.as_ref());
        let outputs = Self::outputs_for(&repo, &branch, &marker);
        let body = format!("Seeded by willikins. Marker: {marker}.");

        let mut state = Self::observe(&client, &repo, &branch, &marker, &files)?;
        let mut attempts: u32 = 0;
        loop {
            let Some((head, already_equal)) = Self::resolve_initial_state(
                &client, &repo, &branch, &marker, &files, &message, state,
            )?
            else {
                return Ok(Ensured {
                    outputs,
                    changed: false,
                });
            };

            let mut additions: Vec<RepoFile> = files
                .iter()
                .filter(|file| !already_equal.contains(file.path().as_str()))
                .cloned()
                .collect();
            additions.push(marker_file.clone());

            attempts += 1;
            match client.create_commit_on_branch(
                &repo,
                &branch,
                &head,
                &additions,
                &message,
                Some(&body),
            ) {
                Ok(_oid) => {
                    return Ok(Ensured {
                        outputs,
                        changed: true,
                    });
                }
                Err(original_err) => {
                    // Decision (b): the decision is made from a fresh
                    // re-read, never from the failed commit's own error
                    // body -- the compare-and-swap forbids a duplicate,
                    // which is what makes retrying safe.
                    match Self::observe(&client, &repo, &branch, &marker, &files)? {
                        ScaffoldState::Present => {
                            return Ok(Ensured {
                                outputs,
                                changed: false,
                            });
                        }
                        ScaffoldState::Foreign => {
                            return Err(Self::foreign_conflict(&repo, &branch, &marker));
                        }
                        // Only reachable if the branch this attempt
                        // targeted was deleted, and the repository
                        // emptied or vanished outright, between the
                        // first observe and this re-read -- not a case
                        // the original commit failure can explain, so
                        // this takes priority over it.
                        ScaffoldState::RepositoryAbsent => {
                            return Err(Self::repository_absent_error(&repo));
                        }
                        // Same reasoning: a `createCommitOnBranch` cannot
                        // have landed against a branch that no longer has
                        // any head to compare-and-swap on, so there is
                        // nothing this re-read can add -- report the
                        // original failure rather than re-entering the
                        // empty-repository initialisation a second time.
                        ScaffoldState::Empty { .. } => {
                            return Err(to_tool_error(original_err));
                        }
                        ScaffoldState::Absent {
                            head: new_head,
                            already_equal: new_already_equal,
                        } if new_head == head => {
                            // The branch did not move: nothing about our
                            // attempt could have landed, and re-trying
                            // against the same head would fail the same
                            // way, so the original failure is reported
                            // as-is.
                            let _ = new_already_equal;
                            return Err(to_tool_error(original_err));
                        }
                        ScaffoldState::Absent {
                            head: new_head,
                            already_equal: new_already_equal,
                        } => {
                            if attempts >= MAX_COMMIT_ATTEMPTS {
                                return Err(to_tool_error(original_err));
                            }
                            state = ScaffoldState::Absent {
                                head: new_head,
                                already_equal: new_already_equal,
                            };
                        }
                    }
                }
            }
        }
    }
}
